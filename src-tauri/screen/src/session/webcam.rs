//! The synchronized webcam track (F-22): the optional FOURTH producer of a
//! screen capture, stamping from the same `CaptureClock` as the frames and
//! the audio, into its OWN video-only fragmented MP4
//! (`.<base>.webcam.mp4.part`, published as `<base>.webcam.mp4`).
//!
//! WHY A SECOND FILE rather than a second video stream in the screen's
//! sink: the screen's `IMFSinkWriter` belongs to `screen-mux` alone (the
//! three-threads rule in `session/mod.rs`), a webcam that vanishes
//! mid-capture must be able to FINALIZE while the screen keeps recording
//! (spec 14's posture), and the editor places the presenter as its own
//! asset anyway (Task 51's migration).
//!
//! WHAT IS PURE AND WHAT IS NOT — the rule `session/mod.rs` states for the
//! whole session. Everything here but `webcam_windows` compiles and runs on
//! every platform: mapping a reader's sample time onto the shared clock,
//! discarding while paused, rebasing the file to its own first frame,
//! stamping each frame's duration, picking a device mode and turning a
//! delivered NV12/YUY2 buffer into the NV12 the encoder takes. The
//! `cfg(windows)` arm (`webcam_windows.rs`) moves bytes between Media
//! Foundation and these functions, and EXECUTES IN NO AUTOMATED TEST — the
//! Windows verification checklist's rows are its gate (docs/Gaps.md
//! GAP-117's class).

use std::path::PathBuf;
use std::time::Duration;

use vault_buddy_core::screen_capture_config::{bitrate_bps, ScreenQuality};

use crate::convert::{nv12_len, ConvertError};
use crate::sink::VideoFormat;
use crate::source::WebcamDeviceId;

use super::pacing::{frame_duration, monotonic_ts, MIN_SAMPLE};

/// What a caller hands the session to record a webcam beside the screen.
/// `None` in `ScreenSessionParams::webcam` is today's capture, unchanged.
#[derive(Debug, Clone)]
pub struct WebcamParams {
    pub device: WebcamDeviceId,
    /// The hidden in-progress file, `.<base>.webcam.mp4.part`.
    pub part: PathBuf,
    /// Where `part` is published on a clean finalize, `<base>.webcam.mp4`.
    pub staged: PathBuf,
    pub quality: ScreenQuality,
}

/// A webcam track that finished and was published. Everything the
/// sidecar's `webcam` block needs, MEASURED rather than derived (GAP-199).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebcamOutcome {
    pub file: PathBuf,
    pub width: u32,
    pub height: u32,
    pub device_label: String,
    /// Where the file's first frame sits on the capture's clock, in ms.
    pub offset_ms: i64,
    /// The end of the last sample written, in the FILE's own time (which
    /// starts at 0 at its first frame) — the webcam's real length, shorter
    /// than the capture when the device vanished mid-way.
    pub duration_ms: u64,
}

/// Maps a reader's sample times onto the shared `CaptureClock`.
///
/// The brief's rule: a sample's clock time is its reader time minus the
/// ANCHOR sample's reader time, plus the clock's elapsed when the anchor
/// arrived. The reader's own spacing carries the timing (a frame that sat
/// in a queue is not stamped late), the clock carries the origin.
///
/// PAUSE: while the clock is paused every sample is discarded (spec 6.3's
/// drain-and-discard), and the anchor is DROPPED, so the first sample after
/// a resume anchors afresh. The device kept delivering through the pause, so
/// keeping the old anchor would put the whole paused stretch into the
/// webcam's timeline while the screen's clock leaves it out — the two
/// tracks would part by exactly the pause.
#[derive(Debug, Default)]
pub struct WebcamClockMap {
    anchor: Option<(i64, Duration)>,
}

impl WebcamClockMap {
    pub fn new() -> WebcamClockMap {
        WebcamClockMap::default()
    }

    /// `reader_hns`: the sample's reader time, in Media Foundation's 100 ns
    /// units. `clock_now`: the
    /// shared clock's `output_ts` as the sample arrived — `None` = paused.
    /// Returns the sample's capture-clock time, or `None` to discard it.
    pub fn map(&mut self, reader_hns: i64, clock_now: Option<Duration>) -> Option<Duration> {
        let Some(clock_now) = clock_now else {
            self.anchor = None;
            return None;
        };
        let (anchor_hns, anchor_clock) = *self.anchor.get_or_insert((reader_hns, clock_now));
        // A reader time BEFORE the anchor (a driver restart stepping its
        // clock back) clamps to the anchor; the pacer's monotonic rule
        // takes it from there.
        let since_anchor = u64::try_from(reader_hns.saturating_sub(anchor_hns)).unwrap_or(0);
        Some(anchor_clock + Duration::from_nanos(since_anchor.saturating_mul(100)))
    }
}

/// Paces the webcam's own file: rebases every frame to the FIRST one (the
/// file starts at 0; its place on the capture is the sidecar's `offsetMs`,
/// GAP-199) and stamps each frame's duration as the gap to the NEXT one.
///
/// Holding one frame back is what makes the timeline true. A webcam delivers
/// at its own rate (15 fps in low light is ordinary), and an MP4 track's
/// length is the SUM of its sample durations (`pacing::should_repeat`'s doc
/// has the citation) — a nominal per-frame duration on a slower device
/// would play the presenter back fast and short of the screen.
pub struct WebcamPacer<T> {
    fps: u32,
    first: Option<Duration>,
    held: Option<(T, Duration)>,
}

impl<T> WebcamPacer<T> {
    pub fn new(fps: u32) -> WebcamPacer<T> {
        WebcamPacer {
            fps,
            first: None,
            held: None,
        }
    }

    /// Feed a frame stamped in CAPTURE-CLOCK time. Returns the previous
    /// frame, ready to write: `(frame, file_ts, duration)`.
    pub fn push(&mut self, frame: T, clock_ts: Duration) -> Option<(T, Duration, Duration)> {
        let first = *self.first.get_or_insert(clock_ts);
        let last = self.held.as_ref().map(|(_, ts)| *ts);
        let ts = monotonic_ts(clock_ts.saturating_sub(first), last, MIN_SAMPLE);
        let previous = self.held.replace((frame, ts));
        // `ts` is strictly past the held frame's by `monotonic_ts`, so the
        // difference is at least MIN_SAMPLE and never rounds to zero hns.
        previous.map(|(frame, held_ts)| (frame, held_ts, ts - held_ts))
    }

    /// The held last frame, with one nominal frame duration — nothing
    /// follows it to measure against.
    pub fn finish(&mut self) -> Option<(T, Duration, Duration)> {
        let (frame, ts) = self.held.take()?;
        Some((frame, ts, frame_duration(self.fps)))
    }

    /// The first frame's capture-clock time: the sidecar's `offsetMs`.
    pub fn offset(&self) -> Option<Duration> {
        self.first
    }
}

/// The two uncompressed layouts the reader is asked for, in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebcamPixels {
    Nv12,
    Yuy2,
}

/// One mode a device offers, as its native media type reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebcamMode {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
}

/// Turn one delivered frame into tightly packed NV12 at `(width, height)`.
///
/// The row pitch is INFERRED from the buffer's length, never assumed to be
/// the width: a device may pad each row, and reading a padded buffer as
/// packed shears the picture progressively down the frame (`convert.rs`
/// documents the same bug for BGRA).
pub fn webcam_frame_to_nv12(
    pixels: WebcamPixels,
    buf: &[u8],
    width: u32,
    height: u32,
) -> Result<Vec<u8>, ConvertError> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(ConvertError::OddDimensions);
    }
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 {
        return Ok(Vec::new());
    }
    match pixels {
        WebcamPixels::Nv12 => nv12_repacked(buf, w, h),
        WebcamPixels::Yuy2 => yuy2_to_nv12(buf, w, h),
    }
}

/// `rows` rows of at least `row_bytes` each, with the pitch read off the
/// buffer's length. `ShortInput` when the buffer cannot hold them.
fn pitch(buf: &[u8], rows: usize, row_bytes: usize) -> Result<usize, ConvertError> {
    let needed = rows * row_bytes;
    let pitch = buf.len() / rows;
    if pitch < row_bytes {
        return Err(ConvertError::ShortInput {
            needed,
            got: buf.len(),
        });
    }
    Ok(pitch)
}

fn nv12_repacked(buf: &[u8], w: usize, h: usize) -> Result<Vec<u8>, ConvertError> {
    // A full-height luma plane plus a half-height interleaved chroma plane,
    // both at the same pitch.
    let pitch = pitch(buf, h + h / 2, w)?;
    let mut out = Vec::with_capacity(nv12_len(w as u32, h as u32));
    for row in buf.chunks(pitch).take(h + h / 2) {
        out.extend_from_slice(&row[..w]);
    }
    Ok(out)
}

fn yuy2_to_nv12(buf: &[u8], w: usize, h: usize) -> Result<Vec<u8>, ConvertError> {
    let pitch = pitch(buf, h, w * 2)?;
    let mut out = vec![0u8; nv12_len(w as u32, h as u32)];
    let (luma, chroma) = out.split_at_mut(w * h);
    for (y, row) in buf.chunks(pitch).take(h).enumerate() {
        for x in 0..w {
            luma[y * w + x] = row[x * 2];
        }
    }
    // One U/V pair per 2x2 block: the mean of the two rows' pairs. YUY2
    // already shares a pair across each horizontal pixel pair.
    for pair_row in 0..h / 2 {
        let top = &buf[pair_row * 2 * pitch..];
        let bottom = &buf[(pair_row * 2 + 1) * pitch..];
        for block in 0..w / 2 {
            let at = block * 4;
            let mean =
                |i: usize| (u16::from(top[at + i]) + u16::from(bottom[at + i])).div_ceil(2) as u8;
            chroma[pair_row * w + block * 2] = mean(1);
            chroma[pair_row * w + block * 2 + 1] = mean(3);
        }
    }
    Ok(out)
}

/// Which native mode to open: the largest at or under 1280x720 among those
/// running at 24 fps or more, else the smallest above it. A presenter bubble
/// is a corner of the frame; opening a 1080p or 4K camera at full size would
/// double the encoder load of the capture it sits beside.
pub fn choose_webcam_mode(modes: &[WebcamMode]) -> Option<usize> {
    const PRESENTER_AREA: u64 = 1280 * 720;
    let fps = |m: &WebcamMode| m.fps_num.checked_div(m.fps_den).unwrap_or(0);
    let area = |m: &WebcamMode| u64::from(m.width) * u64::from(m.height);
    let any_fast = modes.iter().any(|m| fps(m) >= 24);
    let candidates = modes
        .iter()
        .enumerate()
        .filter(|(_, m)| !any_fast || fps(m) >= 24);
    // Ordered so the BEST mode is the minimum: fits under 720p first; then
    // the biggest that fits (or the smallest that does not); then the rate
    // nearest 30; then the device's own order.
    candidates
        .min_by_key(|(i, m)| {
            let fits = area(m) <= PRESENTER_AREA;
            let size_rank = if fits {
                PRESENTER_AREA - area(m)
            } else {
                area(m)
            };
            (!fits, size_rank, fps(m).abs_diff(30), *i)
        })
        .map(|(i, _)| i)
}

/// The webcam sink's video-only format: even dimensions, the device's rate
/// rounded to whole frames (1..=60, 30 when the device reports none), and
/// the capture's own quality preset's bitrate.
pub fn webcam_video_format(mode: WebcamMode, quality: ScreenQuality) -> Option<VideoFormat> {
    let (width, height) = crate::convert::even_dims(mode.width, mode.height);
    if width == 0 || height == 0 {
        return None;
    }
    let fps = match mode.fps_den {
        0 => 30,
        den => ((f64::from(mode.fps_num) / f64::from(den)).round() as u32).clamp(1, 60),
    };
    Some(VideoFormat {
        width,
        height,
        fps,
        bitrate_bps: bitrate_bps(quality, width, height, fps),
    })
}

/// The warning a screen capture carries when its WEBCAM track could not be
/// finished (review fix round 1).
///
/// The screen capture itself succeeded, so the message must never read as a
/// failed capture — which `ScreenError::Retained`'s own Display does ("screen
/// capture could not finish…"), because that wording is the SCREEN's. It is
/// built from the cause instead, and names no path: where the part lives is
/// the log's business, and the recovery sweep promotes a part that holds
/// footage on the next start.
pub fn webcam_stop_warning(err: &crate::ScreenError) -> String {
    const SAVED: &str = "The screen capture was saved";
    match err {
        crate::ScreenError::Retained {
            holds_footage: true,
            ..
        } => format!(
            "{SAVED}, but its webcam track could not be finished. The webcam \
             footage was kept and will be recovered the next time Vault Buddy starts."
        ),
        crate::ScreenError::Retained {
            holds_footage: false,
            ..
        } => format!("{SAVED}, but no webcam video was recorded."),
        _ => format!("{SAVED}, but its webcam track could not be finished."),
    }
}

#[cfg(test)]
#[path = "webcam_tests.rs"]
mod tests;
