//! `session/webcam.rs`'s pure rules (Task 52, F-22). A sibling file so the
//! module stays readable beside the `cfg(windows)` producer it feeds — and
//! because every one of these rules would otherwise live only in code no
//! automated test can execute (GAP-117's class).

use super::*;
use crate::convert::ConvertError;
use std::time::Duration;

fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

// --- the shared clock -----------------------------------------------------

// The brief's own numbers. The first sample is anchored where the CLOCK was
// when it arrived; every later one is placed by the READER's spacing, not
// by when it happened to arrive (the second arrives 50 ms later but was
// captured 33.3 ms later, and 33.3 ms is what it is stamped with).
#[test]
fn webcam_timestamps_map_into_the_capture_clock() {
    let mut map = WebcamClockMap::new();
    assert_eq!(map.map(5_000_000, Some(ms(2_000))), Some(ms(2_000)));
    let later = map
        .map(5_000_000 + 333_333, Some(ms(2_050)))
        .expect("a running clock maps every sample");
    assert_eq!(later, ms(2_000) + Duration::from_nanos(333_333 * 100));
    assert_eq!(later.as_millis(), 2_033);
}

// Spec 6.3: while paused, samples are drained and discarded. And the device
// kept delivering through the pause, so the first sample after a resume must
// anchor AFRESH — keeping the old anchor would bill the paused stretch (here
// 1.5 s of reader time) into the webcam while the screen's clock leaves it
// out, and the presenter would trail the screen by the whole pause.
#[test]
fn paused_webcam_samples_are_discarded() {
    let mut map = WebcamClockMap::new();
    assert_eq!(map.map(5_000_000, Some(ms(1_000))), Some(ms(1_000)));
    assert_eq!(map.map(5_333_333, None), None, "paused: discarded");
    assert_eq!(map.map(6_000_000, None), None, "still paused: discarded");
    assert_eq!(
        map.map(20_000_000, Some(ms(1_120))),
        Some(ms(1_120)),
        "the first sample after a resume anchors to the clock, not to the old anchor"
    );
    assert_eq!(
        map.map(20_333_333, Some(ms(1_200))),
        Some(ms(1_120) + Duration::from_nanos(333_333 * 100))
    );
}

// A device clock that steps BACKWARDS (a driver restart) must not wrap into
// a huge timestamp or underflow; it clamps to the anchor and the pacer's
// monotonic rule takes it from there.
#[test]
fn a_reader_time_before_the_anchor_clamps_rather_than_wrapping() {
    let mut map = WebcamClockMap::new();
    assert_eq!(map.map(5_000_000, Some(ms(2_000))), Some(ms(2_000)));
    assert_eq!(map.map(4_000_000, Some(ms(2_100))), Some(ms(2_000)));
}

// --- the file's own timeline (GAP-199) --------------------------------------

// Task 51's migration places file time 0 at `offsetMs` on the capture. A file
// stamped in CLOCK time would count that offset twice: the presenter would
// run 2 s late in this example. So the file starts at its own first frame,
// the first frame's clock time is the offset, and each frame lasts until the
// next one arrived (a 15 fps camera must not play back at 30).
#[test]
fn the_webcam_file_starts_at_its_own_first_frame() {
    let mut p = WebcamPacer::new(30);
    assert_eq!(p.push('a', ms(2_000)), None, "held until its successor");
    assert_eq!(p.offset(), Some(ms(2_000)));
    assert_eq!(p.push('b', ms(2_033)), Some(('a', ms(0), ms(33))));
    assert_eq!(p.push('c', ms(2_100)), Some(('b', ms(33), ms(67))));
    assert_eq!(
        p.finish(),
        Some(('c', ms(100), frame_duration(30))),
        "the last frame gets one nominal frame"
    );
    assert_eq!(p.finish(), None, "and only once");
    assert_eq!(p.offset(), Some(ms(2_000)), "finishing keeps the offset");
}

#[test]
fn a_webcam_frame_stamped_out_of_order_is_forced_forward() {
    // A non-monotonic stream is rejected outright by the muxer, and a zero
    // duration trips Media Foundation's divide-by-zero (pacing::MIN_SAMPLE).
    let mut p = WebcamPacer::new(30);
    assert_eq!(p.push('a', ms(2_000)), None);
    assert_eq!(p.push('b', ms(1_990)), Some(('a', ms(0), MIN_SAMPLE)));
    assert_eq!(
        p.push('c', ms(2_050)),
        Some(('b', MIN_SAMPLE, ms(50) - MIN_SAMPLE))
    );
}

#[test]
fn a_pacer_that_saw_no_frame_finishes_with_nothing() {
    let mut p: WebcamPacer<()> = WebcamPacer::new(30);
    assert_eq!(p.finish(), None);
    assert_eq!(p.offset(), None);
}

// --- delivered buffers -> NV12 ----------------------------------------------

// Rows padded to 6 bytes for a 4-pixel width: inferring the pitch from the
// buffer is what keeps the padding out of the picture.
#[test]
fn a_padded_nv12_frame_is_repacked_tightly() {
    let buf = [
        1, 2, 3, 4, 99, 99, // Y row 0
        5, 6, 7, 8, 99, 99, // Y row 1
        10, 20, 30, 40, 99, 99, // UV row
    ];
    let out = webcam_frame_to_nv12(WebcamPixels::Nv12, &buf, 4, 2).unwrap();
    assert_eq!(out, vec![1, 2, 3, 4, 5, 6, 7, 8, 10, 20, 30, 40]);
}

#[test]
fn a_yuy2_frame_becomes_nv12_with_vertically_averaged_chroma() {
    // Two rows of 4 pixels: Y0 U Y1 V per pixel pair. NV12's chroma is one
    // U/V pair per 2x2 block, so each is the mean of the two rows' pairs.
    let packed = [
        10, 100, 11, 200, 12, 110, 13, 210, //
        20, 120, 21, 220, 22, 130, 23, 230,
    ];
    let want = vec![10, 11, 12, 13, 20, 21, 22, 23, 110, 210, 120, 220];
    assert_eq!(
        webcam_frame_to_nv12(WebcamPixels::Yuy2, &packed, 4, 2).unwrap(),
        want
    );
    // The same picture with 2 bytes of padding per row.
    let padded = [
        10, 100, 11, 200, 12, 110, 13, 210, 0, 0, //
        20, 120, 21, 220, 22, 130, 23, 230, 0, 0,
    ];
    assert_eq!(
        webcam_frame_to_nv12(WebcamPixels::Yuy2, &padded, 4, 2).unwrap(),
        want
    );
}

#[test]
fn a_short_or_odd_webcam_frame_is_refused_rather_than_read_past() {
    assert!(matches!(
        webcam_frame_to_nv12(WebcamPixels::Nv12, &[0; 11], 4, 2),
        Err(ConvertError::ShortInput { .. })
    ));
    assert!(matches!(
        webcam_frame_to_nv12(WebcamPixels::Yuy2, &[0; 15], 4, 2),
        Err(ConvertError::ShortInput { .. })
    ));
    assert_eq!(
        webcam_frame_to_nv12(WebcamPixels::Nv12, &[0; 64], 5, 2),
        Err(ConvertError::OddDimensions)
    );
    assert_eq!(
        webcam_frame_to_nv12(WebcamPixels::Yuy2, &[0; 64], 4, 3),
        Err(ConvertError::OddDimensions)
    );
}

// --- the device mode and the sink format ------------------------------------

fn mode(width: u32, height: u32, fps: u32) -> WebcamMode {
    WebcamMode {
        width,
        height,
        fps_num: fps,
        fps_den: 1,
    }
}

#[test]
fn the_largest_mode_at_or_under_720p_is_opened() {
    let modes = [
        mode(640, 480, 30),
        mode(1920, 1080, 30),
        mode(1280, 720, 15),
        mode(1280, 720, 30),
        mode(320, 240, 30),
    ];
    assert_eq!(choose_webcam_mode(&modes), Some(3));
}

#[test]
fn a_mode_choice_prefers_a_steady_rate_then_the_smallest_above_720p() {
    // Fast modes win over a bigger slow one...
    assert_eq!(
        choose_webcam_mode(&[mode(1280, 720, 10), mode(640, 480, 30)]),
        Some(1)
    );
    // ...30 over 60 at the same size (twice the encoder load for a corner)...
    assert_eq!(
        choose_webcam_mode(&[mode(1280, 720, 60), mode(1280, 720, 30)]),
        Some(1)
    );
    // ...and with nothing at or under 720p, the SMALLEST above it.
    assert_eq!(
        choose_webcam_mode(&[mode(3840, 2160, 30), mode(1920, 1080, 30)]),
        Some(1)
    );
    // Only slow modes: take them rather than nothing.
    assert_eq!(
        choose_webcam_mode(&[mode(640, 480, 5), mode(1280, 720, 10)]),
        Some(1)
    );
    assert_eq!(choose_webcam_mode(&[]), None);
}

#[test]
fn the_webcam_sink_format_is_even_sized_at_the_devices_whole_frame_rate() {
    let ntsc = WebcamMode {
        width: 1281,
        height: 721,
        fps_num: 30_000,
        fps_den: 1_001,
    };
    let f = webcam_video_format(ntsc, ScreenQuality::Balanced).unwrap();
    assert_eq!((f.width, f.height, f.fps), (1280, 720, 30));
    assert_eq!(
        f.bitrate_bps,
        bitrate_bps(ScreenQuality::Balanced, 1280, 720, 30)
    );
    let slow = webcam_video_format(mode(640, 480, 15), ScreenQuality::High).unwrap();
    assert_eq!((slow.width, slow.height, slow.fps), (640, 480, 15));
    let unknown = WebcamMode {
        fps_den: 0,
        ..mode(640, 480, 30)
    };
    assert_eq!(
        webcam_video_format(unknown, ScreenQuality::Low)
            .unwrap()
            .fps,
        30
    );
    assert_eq!(
        webcam_video_format(mode(640, 480, 120), ScreenQuality::Low)
            .unwrap()
            .fps,
        60
    );
    assert!(webcam_video_format(mode(1, 1, 30), ScreenQuality::Low).is_none());
}

// --- the default path -------------------------------------------------------

/// `windows_session.rs` up to its first test module.
fn windows_session_production() -> &'static str {
    let src = include_str!("windows_session.rs");
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

// "No webcam selected = unchanged capture" (checklist T41). The producer is
// constructed at exactly ONE site, and that site is the `Some` arm of a match
// whose `None` arm constructs nothing: a capture started without a webcam
// opens no device, spawns no webcam thread and writes no webcam file. The
// shell half — `webcamId` absent means `None` — is pinned in
// `screen_webcam_commands.rs`.
//
// It pins the construction site's SHAPE, not its meaning: a behaviour-
// preserving rewrite (`webcam.map(..).transpose()?`) goes red too (Task 52's
// mutation M6). That is accepted — the arm it guards runs in no automated
// test, so a textual pin is the strongest check available.
#[test]
fn start_without_a_webcam_is_byte_identical_to_today() {
    let code = windows_session_production();
    let sites: Vec<usize> = code
        .match_indices("WebcamProducer::open(")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        sites.len(),
        1,
        "the webcam producer must be constructed at exactly one site"
    );
    let line = &code[code[..sites[0]].rfind('\n').unwrap_or(0)..sites[0]];
    assert!(
        line.contains("Some(params) => Some("),
        "the producer must be built only in the Some arm: {line:?}"
    );
    let after = &code[sites[0]..code.len().min(sites[0] + 200)];
    assert!(
        after.contains("None => None,"),
        "the None arm must construct nothing: {after:?}"
    );
}

// --- the stop warning (review fix round 1) --------------------------------

// A webcam that could not be finished rides on a screen capture that DID
// finish. Reusing `Retained`'s Display ("screen capture could not finish…")
// told the user their irreplaceable screen recording had failed -- most
// likely on the zero-frame case a camera format problem produces.
#[test]
fn a_webcam_that_could_not_finish_never_reads_as_a_failed_screen_capture() {
    use crate::ScreenError;
    use std::path::PathBuf;
    let part = PathBuf::from(r"C:\staging\.2026-09-24 1015 Demo.webcam.mp4.part");
    let kept = ScreenError::Retained {
        path: part.clone(),
        holds_footage: true,
        cause: Box::new(ScreenError::Io(
            "could not finish the webcam file: denied".into(),
        )),
    };
    let empty = ScreenError::Retained {
        path: part,
        holds_footage: false,
        cause: Box::new(ScreenError::Sink("the webcam delivered no video".into())),
    };
    let other = ScreenError::Sink("the webcam writer stopped unexpectedly".into());
    for err in [&kept, &empty, &other] {
        let msg = webcam_stop_warning(err);
        assert!(msg.starts_with("The screen capture was saved"), "{msg}");
        assert!(!msg.contains("screen capture could not finish"), "{msg}");
        assert!(
            !msg.contains(".part"),
            "no path in a user-facing line: {msg}"
        );
    }
    assert!(webcam_stop_warning(&kept).contains("kept and will be recovered"));
    assert!(webcam_stop_warning(&empty).contains("no webcam video was recorded"));
}

// The pure message above is only half the fix: the `cfg(windows)` stop arm
// that raises it runs in no automated test, so it is pinned by source.
#[test]
fn the_session_raises_the_webcam_specific_stop_warning() {
    let code = windows_session_production();
    assert!(
        code.contains("self.warnings.raise(super::webcam::webcam_stop_warning(&e))"),
        "the webcam stop arm must raise webcam_stop_warning, not the error's own text"
    );
    assert!(
        !code.contains(".raise(format!(\"the webcam track could not be finished: {e}\"))"),
        "the Retained text must not reach the user again"
    );
}
