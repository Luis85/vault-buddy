//! What a capture TELLS the user when its frames do not fit the file.
//!
//! Pure on purpose, like `pacing`, `select`, `clock` and `convert`: no CI
//! runner can record a screen (docs/Gaps.md GAP-117), so a message composed
//! inside the `cfg(windows)` arm is a message nothing can ever prove. The
//! Windows arms below read atomics and call these functions; the wording and
//! the decision live here, where Linux can pin them.

/// Pack a frame size into one `u64` so the frame callback can record it in a
/// single relaxed atomic store.
pub fn pack_dims(width: u32, height: u32) -> u64 {
    ((width as u64) << 32) | height as u64
}

/// The inverse of [`pack_dims`]. `None` for `0`, the "nothing recorded yet"
/// value.
pub fn unpack_dims(packed: u64) -> Option<(u32, u32)> {
    if packed == 0 {
        return None;
    }
    Some(((packed >> 32) as u32, packed as u32))
}

/// Why a delivered frame was dropped for being smaller than the size the
/// sink was opened with.
pub fn undersized_drop_reason(got_w: u32, got_h: u32, want_w: u32, want_h: u32) -> String {
    format!(
        "it arrived smaller than the recorded size (got {got_w}x{got_h}, \
         declared {want_w}x{want_h})"
    )
}

/// The failure to report for a capture that finalized having written no
/// video sample at all, or `None` when video really was written.
pub fn zero_video_diagnosis(
    video_written: u64,
    declared: (u32, u32),
    undersized: Option<(u32, u32)>,
) -> Option<String> {
    if video_written > 0 {
        return None;
    }
    let (want_w, want_h) = declared;
    Some(match undersized {
        // `undersized` is stored ONLY by the size-mismatch drop, so a value
        // here means every frame really did arrive too small — which is the
        // whole diagnosis, and the two numbers are what makes it actionable.
        Some((got_w, got_h)) => format!(
            "the capture recorded no video: every frame arrived at {got_w}x{got_h}, smaller \
             than the {want_w}x{want_h} the recording was opened at, so none could be used"
        ),
        None => format!(
            "the capture recorded no video: no usable frame ever reached the recorder \
             (the recording was opened at {want_w}x{want_h})"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_size_round_trips_through_one_atomic_word() {
        // The frame callback cannot take a lock (it runs across the
        // WinRT/COM boundary, where a panic aborts the process with no crash
        // record), so the observed size travels to `stop()` packed into one
        // relaxed atomic. A packing bug would report garbage dimensions in
        // the very message a user is meant to act on.
        assert_eq!(unpack_dims(pack_dims(1520, 842)), Some((1520, 842)));
        assert_eq!(
            unpack_dims(pack_dims(u32::MAX, 1)),
            Some((u32::MAX, 1)),
            "the width must not bleed into the height"
        );
        assert_eq!(
            unpack_dims(pack_dims(1, u32::MAX)),
            Some((1, u32::MAX)),
            "the height must not bleed into the width"
        );
    }

    #[test]
    fn nothing_recorded_reads_back_as_no_observation() {
        // Zero is the initial value of the atomic. Reading it as a real
        // 0x0 observation would make the zero-sample message claim a
        // measurement that was never taken.
        assert_eq!(unpack_dims(0), None);
    }

    #[test]
    fn an_undersized_drop_names_both_sizes() {
        // The failure this exists to prevent: the log said only "the source
        // shrank below the recorded size", so a window capture that dropped
        // EVERY frame because the declared size was wrong (GetWindowRect
        // includes DWM's invisible resize border) looked like a user
        // resizing a window. Without both numbers the log cannot tell the
        // two apart.
        let reason = undersized_drop_reason(1520, 842, 1534, 856);
        assert!(reason.contains("1520x842"), "got {reason}");
        assert!(reason.contains("1534x856"), "got {reason}");
    }

    #[test]
    fn a_capture_that_wrote_video_is_not_diagnosed() {
        // A single written frame means the file has video in it; claiming
        // otherwise would turn a successful capture into a failure.
        assert_eq!(
            zero_video_diagnosis(1, (1534, 856), Some((1520, 842))),
            None
        );
    }

    #[test]
    fn a_size_mismatch_that_wrote_nothing_names_the_cause_and_both_sizes() {
        // The real bug: every frame arrived smaller than the declared size,
        // all were dropped, and finalize failed with a raw
        // MF_E_SINK_NO_SAMPLES_PROCESSED the user could not act on.
        let msg = zero_video_diagnosis(0, (1534, 856), Some((1520, 842)))
            .expect("a capture that wrote no video must explain itself");
        assert!(msg.contains("1520x842"), "the observed size: got {msg}");
        assert!(msg.contains("1534x856"), "the declared size: got {msg}");
        assert!(
            msg.contains("no video"),
            "it must say what is wrong with the file: got {msg}"
        );
    }

    #[test]
    fn no_frames_at_all_still_explains_itself_without_inventing_a_size() {
        // A capture that received NO frame has no observation to report.
        // Interpolating a zero size here would be a fabricated measurement.
        let msg = zero_video_diagnosis(0, (1534, 856), None)
            .expect("a capture that wrote no video must explain itself");
        assert!(msg.contains("no video"), "got {msg}");
        assert!(msg.contains("1534x856"), "got {msg}");
        assert!(!msg.contains("0x0"), "no invented observation: got {msg}");
    }
}
