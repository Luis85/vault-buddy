//! `screen_webcam_commands.rs`'s tests (Task 52, F-22).

use super::*;
use std::path::PathBuf;

const BASE: &str = "2026-09-24 1015 Demo";
const HASH: &str = "0f3a9c";

// The shell half of "no webcam selected = unchanged capture" (checklist
// T41): an absent `webcamId` requests no webcam at all, so the session's
// `webcam` is `None` and its producer is never constructed (the screen
// crate pins that half, `start_without_a_webcam_is_byte_identical_to_today`).
#[test]
fn a_start_without_a_webcam_id_requests_no_webcam() {
    assert_eq!(parse_webcam_request(None), Ok(None));
    assert!(webcam_params(None, Path::new("staging"), BASE, ScreenQuality::Balanced).is_none());
}

#[test]
fn a_chosen_webcam_is_parsed_strictly() {
    let id = format!("webcam:{HASH}");
    assert_eq!(
        parse_webcam_request(Some(&id)),
        Ok(WebcamDeviceId::parse(&id))
    );
    // Untrusted input: never trimmed, case-folded or defaulted.
    for bad in ["webcam:0F3A9C", " webcam:0f3a9c", "webcam:", "screen:0", ""] {
        assert_eq!(
            parse_webcam_request(Some(bad)),
            Err("Unknown webcam.".to_string()),
            "{bad:?}"
        );
    }
}

// The webcam records into THIS capture's own names, so the recovery sweep
// attributes an orphaned part to the right capture and Task 51's migration
// finds the file the block names.
#[test]
fn webcam_params_name_this_captures_own_webcam_files() {
    let dir = PathBuf::from("staging");
    let device = WebcamDeviceId::parse(&format!("webcam:{HASH}"));
    let params = webcam_params(device.clone(), &dir, BASE, ScreenQuality::High)
        .expect("a chosen webcam is requested");
    assert_eq!(Some(params.device), device);
    assert_eq!(params.part, dir.join(staging::webcam_part_file_name(BASE)));
    assert_eq!(params.staged, dir.join(staging::webcam_file_name(BASE)));
    assert_eq!(params.quality, ScreenQuality::High);
}

// GAP-199: the block carries the MEASURED offset and length, and a file
// NAME — never the path the outcome reports, which migration would refuse
// as a file the capture does not own.
#[test]
fn the_sidecar_block_records_the_measured_webcam() {
    let outcome = WebcamOutcome {
        file: PathBuf::from("C:/staging").join(staging::webcam_file_name(BASE)),
        width: 1280,
        height: 720,
        device_label: "Integrated Camera".into(),
        offset_ms: 37,
        duration_ms: 20_250,
    };
    let block = webcam_block(BASE, &outcome);
    assert_eq!(block.file, staging::webcam_file_name(BASE));
    assert_eq!((block.width, block.height), (1280, 720));
    assert_eq!(block.device_label, "Integrated Camera");
    assert_eq!(block.offset_ms, 37);
    assert_eq!(block.duration_ms, Some(20_250));
    assert!(block.extra.is_empty());
}

// An unparseable webcam id must be refused before `CaptureGuard` is claimed,
// the source id's own rule: a malformed request must not wedge either
// capture domain, and must not start a capture without the webcam the user
// asked for.
#[test]
fn an_unparseable_webcam_id_is_refused_before_anything_is_claimed() {
    let code = crate::structural_scan::production_code(include_str!("screen_capture_worker.rs"));
    let parse = crate::structural_scan::offset_of(&code, "parse_webcam_request(");
    let claim = crate::structural_scan::offset_of(&code, "try_claim(CaptureKind::Screen)");
    assert!(
        parse < claim,
        "parse the webcam id before claiming the guard"
    );
    assert!(
        code.contains("webcam: crate::screen_webcam_commands::webcam_params("),
        "the session's webcam must come from the parsed request"
    );
}
