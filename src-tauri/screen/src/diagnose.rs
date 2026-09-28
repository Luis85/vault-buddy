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

/// Why a delivered frame was dropped for being too small to produce the
/// output this capture writes.
///
/// `need_w`/`need_h` are the crop's FAR EDGE (`pacing::required_dims`), not
/// the output size. Those were the same number until regions existed --
/// every other capture crops at `(0, 0)` -- and a 640x480 region at
/// (1280, 600) rejecting a 1919x1080 frame then read "it arrived smaller
/// than ... 640x480", which is arithmetically false. This message exists
/// because phase 2 surfaced a zero-frame file as a bare untranslatable
/// HRESULT; a confidently wrong replacement is worse than the HRESULT.
pub fn undersized_drop_reason(got_w: u32, got_h: u32, need_w: u32, need_h: u32) -> String {
    format!(
        "it arrived smaller than this recording needs (got {got_w}x{got_h}, \
         needed at least {need_w}x{need_h})"
    )
}

/// The failure to report for a capture that finalized having written no
/// video sample at all, or `None` when video really was written.
///
/// `needed` is the crop's far edge, the same pair
/// [`undersized_drop_reason`] names — NOT the size the sink was opened at.
/// For a region those differ, and reporting the output size here produced
/// the same falsehood the drop line did.
pub fn zero_video_diagnosis(
    video_written: u64,
    needed: (u32, u32),
    undersized: Option<(u32, u32)>,
) -> Option<String> {
    if video_written > 0 {
        return None;
    }
    let (need_w, need_h) = needed;
    Some(match undersized {
        // `undersized` is stored ONLY by the size-mismatch drop, so a value
        // here means every frame really did arrive too small — which is the
        // whole diagnosis, and the two numbers are what makes it actionable.
        Some((got_w, got_h)) => format!(
            "the capture recorded no video: every frame arrived at {got_w}x{got_h}, smaller \
             than the {need_w}x{need_h} this recording needed, so none could be used"
        ),
        None => format!(
            "the capture recorded no video: no usable frame ever reached the recorder \
             (it needed at least {need_w}x{need_h})"
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::pacing;

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

    // A 640x480 region at (1280, 600) needs a WHOLE 1920x1080 frame: its
    // far edge is the monitor's. When the monitor drops to 1919 wide
    // mid-capture (the case `a_region_is_judged_by_its_far_edge_not_its_size`
    // is written for) every frame is dropped and these two lines are all
    // the user gets. Composed against the OUTPUT size they read "got
    // 1919x1080, declared 640x480" and "smaller than the 640x480 the
    // recording was opened at" -- 1919x1080 is not smaller than 640x480.
    #[test]
    fn a_regions_drop_names_the_size_the_frame_had_to_reach() {
        let (need_w, need_h) = pacing::required_dims(1280, 600, 640, 480);
        let reason = undersized_drop_reason(1919, 1080, need_w, need_h);
        assert!(
            reason.contains("1920x1080"),
            "the line must name the size the frame had to reach: {reason}"
        );
        assert!(
            reason.contains("1919x1080"),
            "and the size it got: {reason}"
        );
        assert!(
            !reason.contains("640x480"),
            "the output size is not what the frame failed to reach: {reason}"
        );
        // The claim itself, not just its numbers: whatever the line says
        // the frame was too small for must really be bigger than it.
        assert!(
            need_w > 1919 || need_h > 1080,
            "a drop line that names a requirement the frame already met is false"
        );
    }

    #[test]
    fn a_region_that_recorded_nothing_is_not_told_a_falsehood_either() {
        // Same numbers one layer up: `run_mux` replaces the finalize
        // HRESULT with this, so it is the whole of what the user sees.
        let (need_w, need_h) = pacing::required_dims(1280, 600, 640, 480);
        let msg = zero_video_diagnosis(0, (need_w, need_h), Some((1919, 1080)))
            .expect("a capture that wrote no video must explain itself");
        assert!(msg.contains("1919x1080"), "got {msg}");
        assert!(msg.contains("1920x1080"), "got {msg}");
        assert!(
            !msg.contains("640x480"),
            "the output size is not what the frames failed to reach: {msg}"
        );
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
