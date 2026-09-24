//! `screen_capture_worker.rs`'s tests, split into a sibling file (Task 52)
//! so the worker stays under the 800-line Rust cap as it grows the webcam.

use super::*;

#[test]
fn a_retained_error_hands_back_the_part_path_as_typed_data() {
    // Task 7 introduced `Retained` precisely so a stop that failed AFTER
    // real footage was written hands the surviving `.part` back as DATA,
    // not as prose the frontend would have to parse out of a message.
    // Folding this arm into the catch-all below is a one-character edit
    // that silently drops `retainedPath` from the emitted payload.
    let err = ScreenError::Retained {
        path: PathBuf::from("/staging/.2026-01-02 0915 Demo.mp4.part"),
        holds_footage: true,
        cause: Box::new(ScreenError::Sink("disk full".into())),
    };
    let (message, retained) = describe_screen_error(&err);
    assert_eq!(
        retained.as_deref(),
        Some(Path::new("/staging/.2026-01-02 0915 Demo.mp4.part"))
    );
    // The message must still name the cause, so a listener that only
    // renders `message` does not regress to "something went wrong".
    assert!(message.contains("disk full"), "got {message}");
}

#[test]
fn a_retained_file_with_no_footage_is_still_surfaced_but_not_called_a_recording() {
    // A capture whose frames were all the wrong size leaves a `.part`
    // with no video in it. The path is still handed back — suppressing
    // it would leave a permanent invisible orphan, since nothing sweeps
    // staging (docs/Gaps.md GAP-115) — but the MESSAGE must not tell the
    // user their footage was kept.
    let err = ScreenError::Retained {
        path: PathBuf::from("/staging/.2026-01-02 0915 Demo.mp4.part"),
        holds_footage: false,
        cause: Box::new(ScreenError::Sink("the capture recorded no video".into())),
    };
    let (message, retained) = describe_screen_error(&err);
    assert_eq!(
        retained.as_deref(),
        Some(Path::new("/staging/.2026-01-02 0915 Demo.mp4.part"))
    );
    assert!(
        !message.contains("still holds the recording"),
        "got {message}"
    );
    assert!(message.contains("no video"), "got {message}");
}

#[test]
fn every_other_screen_error_carries_a_message_and_no_retained_path() {
    // A non-Retained variant has no surviving file, so reporting one
    // would offer the user a path that does not exist.
    for err in [
        ScreenError::Unsupported,
        ScreenError::SourceGone,
        ScreenError::EncoderUnavailable,
        ScreenError::AlreadyCapturing,
        ScreenError::Io("no space".into()),
        ScreenError::Sink("mux refused".into()),
    ] {
        let (message, retained) = describe_screen_error(&err);
        assert_eq!(retained, None, "{err} must carry no retained path");
        assert_eq!(message, err.to_string(), "{err} must render its Display");
    }
}

#[test]
fn the_source_kind_crosses_the_wire_as_the_sidecar_spelling() {
    // The sidecar stores this as a plain string (staging::StagedSidecar),
    // so a renamed spelling here would silently orphan every capture a
    // previous build staged.
    let screen = source::SourceId::parse("screen:0").expect("screen id");
    let window = source::SourceId::parse("window:12345").expect("window id");
    assert_eq!(source_kind_str(&screen), "screen");
    assert_eq!(source_kind_str(&window), "window");
}

#[test]
fn a_finalize_reports_the_capture_even_when_the_sidecar_cannot_be_written() {
    // The capture already landed on disk as `outcome.mp4`. Losing the
    // resume metadata is a lesser, separately-logged problem than
    // reporting a safely-saved capture as failed — so the DTO must come
    // back intact. The parent directory deliberately does not exist, so
    // `staging::write_sidecar` really fails here (nothing is written).
    let bundle = ScreenStopBundle {
        outcome: ScreenOutcome {
            mp4: PathBuf::from("/vb-no-such-dir-9e3a/2026-01-02 0915 Demo.mp4"),
            duration_ms: 61_000,
            paused_ms: 4_000,
            width: 1920,
            height: 1080,
            dropped: 3,
            warning: Some("the capture source closed".to_string()),
            webcam: None,
            stems: Vec::new(),
        },
        source_title: "Demo Window".to_string(),
        source_kind: "window",
        recorded_at: "2026-01-02T09:15:00+01:00".to_string(),
        inputs: vec!["Microphone (Yeti)".to_string()],
    };
    let (dto, warning) = finalize_stopped("vault-1", bundle);
    assert_eq!(dto.base, "2026-01-02 0915 Demo");
    assert_eq!(dto.path, "/vb-no-such-dir-9e3a/2026-01-02 0915 Demo.mp4");
    assert_eq!(dto.duration_ms, 61_000);
    assert_eq!(dto.source_title, "Demo Window");
    assert_eq!(dto.width, 1920);
    assert_eq!(dto.height, 1080);
    // A warning must survive to the stopped event: spec 14's "a closed
    // window still saves" story is told by this string.
    assert_eq!(warning.as_deref(), Some("the capture source closed"));
}

// Structural regression, the style this file's sibling already uses. The
// monitor must drop the reservation BEFORE it announces the outcome —
// audio's capture-monitor does (capture_commands.rs), and a listener
// that reacts to `screen:stopped` by reading `screen_capture_status`
// would otherwise still be told a capture is running.
#[test]
fn the_monitor_clears_the_reservation_before_it_emits() {
    let src = include_str!("screen_capture_worker.rs");
    let production = src.split("#[cfg(test)]").next().unwrap_or(src);
    let clear = production
        .find("clear_active_screen(&monitor_app)")
        .expect("the monitor must clear the reservation");
    let emit = production
        .find("emit_screen_stopped(&monitor_app")
        .expect("the monitor must emit the stopped event");
    assert!(
        clear < emit,
        "clear the reservation before emitting, as capture_commands.rs's monitor does: \
         emitting first lets a listener read `capturing: true` for a capture it was \
         just told had finished, and puts the sidecar write and the OS toast inside \
         stop_screen_capture's 30 s budget (a slow disk then reports stillSaving on a \
         fully-saved capture)"
    );
}

/// The slice of `start_screen_capture_blocking` that runs when the ready
/// handshake times out, comments stripped.
fn ready_timeout_arm(code: &str) -> &str {
    let after_match =
        &code[crate::structural_scan::offset_of(code, "ready_rx.recv_timeout(READY_TIMEOUT)")..];
    let arm = crate::structural_scan::offset_of(after_match, "Err(_) =>");
    // The whole arm, not just up to its first `return`: the
    // monitor-spawn fallback returns EARLY, so a nearer anchor would cut
    // the slice off before the code these tests are about. This one sits
    // after the match closes.
    let end = crate::structural_scan::offset_of(after_match, "Announce::Events");
    &after_match[arm..end]
}

// GAP-110 residual 2. The 15 s ready timeout fires while the device
// thread is, by definition, still alive — the commonest shape is one
// wedged inside `open_selected_sources` on a bad audio driver. Freeing
// the reservation there frees `CaptureGuard` with it, so the user's
// retry opens the SAME audio endpoint beside a capture that is still
// starting: the exact reliability hazard spec 7.3 says the guard
// exists to prevent. The release has to belong to whoever learns the
// device thread has really ended, which is the outcome monitor.
#[test]
fn the_ready_timeout_hands_the_release_to_a_monitor_rather_than_freeing_it() {
    let code = crate::structural_scan::production_code(include_str!("screen_capture_worker.rs"));
    let arm = ready_timeout_arm(&code);
    assert!(
        arm.contains("spawn_outcome_monitor"),
        "the ready-timeout arm must hand `done_rx` to a monitor so the \
         reservation is released when the device thread really ends, not \
         while it may still be opening audio endpoints (GAP-110). Arm was:\n{arm}"
    );
}

// The other half of GAP-110's fix, and the defect the fix itself would
// otherwise have introduced. `clear_active_screen` does FOUR things at
// once: drop the reservation, release the guard, clear the buddy's
// capture exclusion and hide the region border. Keeping the reservation
// past a ready timeout keeps all four — including a click-through border
// drawn over the user's screen with nothing recording, which
// `region_indicator`'s own doc calls worse than no border at all, and
// which they cannot dismiss without quitting.
//
// The reservation tracks what the DEVICES are doing; the window effects
// track what the USER sees. A ready timeout is exactly the case where
// those diverge, so the window effects go now and the reservation waits
// for the monitor.
#[test]
fn the_ready_timeout_tears_down_the_window_effects_even_though_it_keeps_the_reservation() {
    let code = crate::structural_scan::production_code(include_str!("screen_capture_worker.rs"));
    let arm = ready_timeout_arm(&code);
    assert!(
        arm.contains("clear_capture_window_effects"),
        "a region capture whose start times out must not leave its border on \
         screen with nothing recording, nor the buddy excluded from every \
         other application's capture. Arm was:\n{arm}"
    );
}

fn stop_bundle(
    dir: &Path,
    webcam: Option<vault_buddy_screen::session::webcam::WebcamOutcome>,
) -> ScreenStopBundle {
    ScreenStopBundle {
        outcome: ScreenOutcome {
            mp4: dir.join("2026-09-24 1015 Demo.mp4"),
            duration_ms: 61_000,
            paused_ms: 0,
            width: 1920,
            height: 1080,
            dropped: 0,
            warning: None,
            webcam,
            stems: Vec::new(),
        },
        source_title: "Demo".to_string(),
        source_kind: "screen",
        recorded_at: "2026-09-24T10:15:00+02:00".to_string(),
        inputs: Vec::new(),
    }
}

// Task 53: the stems that finished are listed by NAME in the sidecar, in
// input order — which is what makes them the capture's to discard, Clear and
// serve (`capture_file_names` reads this list). A capture without stems
// writes no `stems` key at all (the test below pins today's key set).
#[test]
fn a_capture_with_stems_lists_them_in_its_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let mut bundle = stop_bundle(dir.path(), None);
    bundle.outcome.stems = vec![
        vault_buddy_screen::session::stems::StemOutcome {
            index: 1,
            input: "USB Mic".into(),
            file: dir.path().join("2026-09-24 1015 Demo.stem-1.m4a"),
        },
        vault_buddy_screen::session::stems::StemOutcome {
            index: 3,
            input: "Line In".into(),
            file: dir.path().join("2026-09-24 1015 Demo.stem-3.m4a"),
        },
    ];
    finalize_stopped("vault-1", bundle);
    let json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("2026-09-24 1015 Demo.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        json["stems"],
        serde_json::json!([
            { "index": 1, "input": "USB Mic", "file": "2026-09-24 1015 Demo.stem-1.m4a" },
            { "index": 3, "input": "Line In", "file": "2026-09-24 1015 Demo.stem-3.m4a" }
        ])
    );
}

// Checklist T41, the sidecar half: a capture made WITHOUT a webcam writes
// exactly the keys it wrote before F-22 -- no `webcam`, not even `null`.
#[test]
fn a_capture_without_a_webcam_writes_todays_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    finalize_stopped("vault-1", stop_bundle(dir.path(), None));
    let json: serde_json::Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("2026-09-24 1015 Demo.json")).unwrap(),
    )
    .unwrap();
    let mut keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "base",
            "durationMs",
            "height",
            "inputs",
            "pausedMs",
            "recordedAt",
            "sourceKind",
            "sourceTitle",
            "timeline",
            "vaultId",
            "width"
        ]
    );
}

// GAP-199: the finished webcam track lands in the sidecar with its MEASURED
// offset and length, under this capture's own webcam file name.
#[test]
fn a_capture_with_a_webcam_writes_its_measured_block() {
    let dir = tempfile::tempdir().unwrap();
    let base = "2026-09-24 1015 Demo";
    let webcam = vault_buddy_screen::session::webcam::WebcamOutcome {
        file: dir.path().join(staging::webcam_file_name(base)),
        width: 1280,
        height: 720,
        device_label: "Integrated Camera".into(),
        offset_ms: 37,
        duration_ms: 20_250,
    };
    finalize_stopped("vault-1", stop_bundle(dir.path(), Some(webcam)));
    let read = staging::read_sidecar(&dir.path().join(format!("{base}.json"))).unwrap();
    let block = read.webcam.expect("the webcam block is written");
    assert_eq!(block.file, staging::webcam_file_name(base));
    assert_eq!((block.offset_ms, block.duration_ms), (37, Some(20_250)));
}
