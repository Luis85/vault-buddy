//! `staging.rs`'s webcam and stem naming + the sidecar's `webcam` block
//! (Task 51, F-22) — a sibling file because `staging.rs` sits near the
//! 800-line Rust cap with its own suite inline.

use super::*;

const BASE: &str = "2026-09-21 1430 Demo";

// The recovery sweep only ever SEES a name through `base_from_part`, and it
// attributes a webcam part to its capture by stripping `WEBCAM_INFIX` off
// what that returns. If minting and stripping ever disagree, an orphaned
// webcam part is attributed to a capture that does not exist (a base ending
// in ".webcam") and promoted as a second, bogus recording.
#[test]
fn webcam_names_round_trip_with_base_from_part() {
    assert_eq!(webcam_file_name(BASE), "2026-09-21 1430 Demo.webcam.mp4");
    let part = webcam_part_file_name(BASE);
    assert_eq!(part, ".2026-09-21 1430 Demo.webcam.mp4.part");
    let recovered = base_from_part(&part).expect("a webcam part is a part file");
    assert_eq!(
        recovered.strip_suffix(WEBCAM_INFIX),
        Some(BASE),
        "the sweep would attribute this webcam part to a different capture"
    );
    // The published name is the part name without the dot and `.part` --
    // exactly what a promotion renames it to.
    assert_eq!(
        part.strip_prefix('.').and_then(|p| p.strip_suffix(".part")),
        Some(webcam_file_name(BASE).as_str())
    );
    // Never the capture's own files.
    assert_ne!(webcam_file_name(BASE), mp4_file_name(BASE));
    assert_ne!(webcam_part_file_name(BASE), part_file_name(BASE));
}

#[test]
fn stem_names_are_recognised_by_pattern_not_by_list() {
    assert_eq!(stem_file_name(BASE, 3), "2026-09-21 1430 Demo.stem-3.m4a");
    assert_eq!(
        stem_part_base(&stem_part_file_name(BASE, 12)),
        Some((BASE.to_string(), "12".to_string()))
    );
    // A run of digits no u32 holds is still ours by shape.
    assert_eq!(
        stem_part_base(".b.stem-99999999999999999999.m4a.part"),
        Some(("b".to_string(), "99999999999999999999".to_string()))
    );
    for not_a_stem_part in [
        ".b.stem-.m4a.part",
        ".b.stem-x1.m4a.part",
        ".b.stem-1.m4a",
        "b.stem-1.m4a.part",
        "..stem-1.m4a.part",
        ".b.stem-1.mp4.part",
    ] {
        assert_eq!(stem_part_base(not_a_stem_part), None, "{not_a_stem_part}");
    }
    assert!(ends_with_stem_marker("Build.stem-3"));
    assert!(ends_with_stem_marker("x.stem-007"));
    assert!(!ends_with_stem_marker("Build.stem-"));
    assert!(!ends_with_stem_marker("Build.stem-3a"));
    assert!(!ends_with_stem_marker("Build.stems-3"));
}

/// Every key `StagedSidecar` declared BEFORE this task, and nothing else --
/// the struct an older build reads a new sidecar with.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct OlderSidecar {
    base: String,
    vault_id: String,
    source_title: String,
    source_kind: String,
    inputs: Vec<String>,
    duration_ms: u64,
    paused_ms: u64,
    width: u32,
    height: u32,
    recorded_at: String,
    #[serde(default)]
    timeline: Option<serde_json::Value>,
    #[serde(flatten, default)]
    extra: serde_json::Map<String, serde_json::Value>,
}

fn webcam_block() -> WebcamSidecar {
    WebcamSidecar {
        file: webcam_file_name(BASE),
        width: 1280,
        height: 720,
        device_label: "Integrated Camera".into(),
        offset_ms: -45,
        extra: Default::default(),
    }
}

// Backward: every sidecar staged before this build has no `webcam` key, and
// a strict read would make every one of them un-resumable at once. Forward:
// an OLDER build rewrites a sidecar on every editor keystroke, so the block
// must survive that build's struct through its flattened catch-all -- and
// arrive back here intact, or a downgrade silently unlinks the webcam track.
#[test]
fn old_sidecars_without_webcam_still_parse() {
    let old = serde_json::json!({
        "base": BASE, "vaultId": "v", "sourceTitle": "t", "sourceKind": "screen",
        "inputs": [], "durationMs": 9_000, "pausedMs": 0, "width": 1920,
        "height": 1080, "recordedAt": "r"
    });
    let parsed: StagedSidecar = serde_json::from_value(old).expect("an old sidecar parses");
    assert_eq!(parsed.webcam, None);
    assert!(
        !serde_json::to_string(&parsed).unwrap().contains("webcam"),
        "an absent block must not be written back as null"
    );

    let mut new: StagedSidecar = parsed;
    new.webcam = Some(webcam_block());
    let written = serde_json::to_value(&new).unwrap();
    assert_eq!(written["webcam"]["offsetMs"], -45);
    assert_eq!(written["webcam"]["deviceLabel"], "Integrated Camera");

    let older: OlderSidecar = serde_json::from_value(written).unwrap();
    assert!(
        older.extra.contains_key("webcam"),
        "the older build keeps it"
    );
    let rewritten = serde_json::to_value(&older).unwrap();
    let back: StagedSidecar = serde_json::from_value(rewritten).unwrap();
    assert_eq!(back.webcam, Some(webcam_block()));
    assert!(
        !back.extra.contains_key("webcam"),
        "the block is a field again, not a stray extra key"
    );
}

// The pairwise reservation, widened again: a base whose webcam file (or its
// in-progress part) is still on disk is taken, or a new capture would adopt
// an old capture's webcam track as its own.
#[test]
fn reserve_base_suffixes_past_a_leftover_webcam_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(webcam_file_name("cap")), b"x").unwrap();
    assert_eq!(reserve_base(dir.path(), "cap"), "cap (2)");
    std::fs::write(dir.path().join(webcam_part_file_name("cap (2)")), b"x").unwrap();
    assert_eq!(reserve_base(dir.path(), "cap"), "cap (3)");
}

// Review fix round 1: `WebcamSidecar` is strictly typed, so a malformed or
// future-shaped block (a missing `deviceLabel`, a numeric `file`) failed the
// WHOLE sidecar -- the recording vanished from the staged list and could
// not be opened. The block now degrades to `None`, and its raw JSON is kept
// so the next rewrite (a pin, a timeline save) does not erase it.
#[test]
fn a_malformed_webcam_block_degrades_to_none_and_survives_a_rewrite() {
    let dir = tempfile::tempdir().unwrap();
    for block in [
        serde_json::json!({ "file": 5, "width": 1, "height": 1, "deviceLabel": "x", "offsetMs": 0 }),
        serde_json::json!({ "file": "a.webcam.mp4", "width": 640, "height": 480, "offsetMs": 7 }),
    ] {
        let raw = serde_json::json!({
            "base": BASE, "vaultId": "v", "sourceTitle": "t", "sourceKind": "screen",
            "inputs": [], "durationMs": 9_000, "pausedMs": 0, "width": 1920,
            "height": 1080, "recordedAt": "r", "webcam": block
        });
        let path = dir.path().join(sidecar_file_name(BASE));
        std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();

        let read = read_sidecar(&path).expect("the capture itself must stay readable");
        assert_eq!(read.webcam, None, "{block}");
        assert_eq!(read.duration_ms, 9_000);

        write_sidecar(dir.path(), BASE, &read).unwrap();
        let rewritten: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(rewritten["webcam"], block, "the rewrite erased the block");
    }
    // A well-formed block still parses as the field.
    let path = dir.path().join(sidecar_file_name(BASE));
    let mut good: StagedSidecar = read_sidecar(&path).unwrap();
    good.extra.remove("webcam");
    good.webcam = Some(webcam_block());
    write_sidecar(dir.path(), BASE, &good).unwrap();
    assert_eq!(read_sidecar(&path).unwrap().webcam, Some(webcam_block()));
}
