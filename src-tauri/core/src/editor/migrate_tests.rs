//! `migrate`'s own tests — a sibling file since Task 53 so `migrate.rs`
//! stays under the 800-line cap (the `migrate_webcam_tests.rs` precedent).

use super::*;
use crate::editor::test_support;
use crate::editor::time::{self, ClipSpan};
use crate::editor::validate_project;

const SHARED_FIXTURES: &str = include_str!("../../../../tests/fixtures/timeline-cases.json");

fn fixtures() -> serde_json::Value {
    serde_json::from_str(SHARED_FIXTURES).expect("the shared timeline fixture table is JSON")
}

/// The largest `sourceEndMs` any segment in `segments` names -- what the
/// migrated capture's own `duration_ms` must be at least, or
/// `validate_project` would reject an `out_ms` beyond the asset's
/// duration.
fn duration_for(segments: &serde_json::Value) -> u64 {
    segments
        .as_array()
        .expect("segments array")
        .iter()
        .map(|s| s["sourceEndMs"].as_u64().expect("sourceEndMs"))
        .max()
        .unwrap_or(0)
}

/// Finds the migrated project's clip active at output time `t`, mapping
/// it through `time::source_at` the same way a later playback/render task
/// would -- `None` when no clip covers `t`.
fn source_at_t(project: &Project, t: u64) -> Option<u64> {
    project.clips.iter().find_map(|clip| {
        let span = ClipSpan {
            start_ms: clip.start_ms,
            in_ms: clip.in_ms,
            out_ms: clip.out_ms,
            speed: 1.0,
        };
        time::clip_is_active(&span, t)
            .then(|| time::source_at(&span, t))
            .flatten()
    })
}

#[test]
fn untouched_capture_becomes_one_whole_clip() {
    let input = StagedInput {
        base: "2026-09-21 1430 Demo",
        vault_id: "vault-1",
        source_title: "Demo capture",
        duration_ms: 12_345,
        width: 1920,
        height: 1080,
        has_audio: true,
        legacy_timeline: None,
        stems: &[],
        input_count: 0,
        webcam: None,
    };
    let result = from_staged(&input, "proj-1");
    assert_eq!(result.dropped_segments, 0);

    let mut expected = test_support::minimal_project();
    expected.id = "proj-1".to_string();
    expected.title = "Demo capture".to_string();
    expected.canvas = Canvas {
        width: 1280,
        height: 720,
        fps: 30,
        extra: Map::new(),
    };
    expected.assets = vec![Asset {
        id: "src".to_string(),
        kind: AssetKind::Video,
        name: "Demo capture".to_string(),
        duration_ms: 12_345,
        width: Some(Num::from(1920)),
        height: Some(Num::from(1080)),
        size: None,
        builtin: Some(Builtin::Screen),
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    }];
    expected.tracks = vec![
        Track {
            id: "v1".to_string(),
            kind: TrackKind::Video,
            name: "Screen".to_string(),
            visible: true,
            locked: false,
            muted: false,
            solo: false,
            volume: Num::from(1),
            extra: Map::new(),
        },
        Track {
            id: "a1".to_string(),
            kind: TrackKind::Audio,
            name: "Audio".to_string(),
            visible: true,
            locked: false,
            muted: false,
            solo: false,
            volume: Num::from(1),
            extra: Map::new(),
        },
    ];
    expected.clips = vec![Clip {
        id: "c1".to_string(),
        asset_id: "src".to_string(),
        track_id: "v1".to_string(),
        name: "Clip 1".to_string(),
        start_ms: 0,
        in_ms: 0,
        out_ms: 12_345,
        fade_in_ms: 0,
        fade_out_ms: 0,
        fade_curve: FadeCurve::Linear,
        opacity: Num::from(1),
        volume: Num::from(1),
        muted: false,
        x: Num::from(0),
        y: Num::from(0),
        w: Num::from(1),
        h: Num::from(1),
        speed: None,
        rotation: None,
        frame_shape: None,
        fit: None,
        mirror: None,
        flip_y: None,
        preserve_pitch: None,
        group_id: None,
        crop_zoom: None,
        crop_x: None,
        crop_y: None,
        adjustments: None,
        card: None,
        extra: Map::new(),
    }];
    expected.destination = Destination {
        vault: "vault-1".to_string(),
        folder: String::new(),
        dated: false,
        extra: Map::new(),
    };

    assert_eq!(result.project, expected);
}

#[test]
fn legacy_segments_become_consecutive_clips() {
    for case in fixtures()["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let segments = &case["segments"];
        let legacy = serde_json::json!({ "segments": segments });
        let duration_ms = duration_for(segments).max(6_000);

        let input = StagedInput {
            base: "base",
            vault_id: "vault-1",
            source_title: "Capture",
            duration_ms,
            width: 1920,
            height: 1080,
            has_audio: true,
            legacy_timeline: Some(&legacy),
            stems: &[],
            input_count: 0,
            webcam: None,
        };
        let result = from_staged(&input, "proj-1");

        let source_timeline = Timeline::from_sidecar_value(&legacy, duration_ms);
        assert_eq!(
            time::project_duration(&result.project),
            source_timeline.output_duration_ms(),
            "case {name:?}: migrated duration disagrees with Timeline::output_duration_ms"
        );

        for row in case["toSourceMs"].as_array().expect("toSourceMs") {
            let t = row[0].as_u64().expect("outputMs");
            let expected = row[1].as_u64();
            assert_eq!(
                source_at_t(&result.project, t),
                expected,
                "case {name:?}: source_at({t}) disagrees with Timeline::to_source_ms"
            );
            assert_eq!(
                source_timeline.to_source_ms(t),
                expected,
                "case {name:?}: fixture's own toSourceMs row disagrees with Timeline::to_source_ms"
            );
        }
    }
}

#[test]
fn backwards_segment_is_dropped_and_counted() {
    let table = fixtures();
    let case = table["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|c| c["name"] == "a backwards segment (hand-edited sidecar)")
        .expect("the backwards-segment fixture case exists");
    let legacy = serde_json::json!({ "segments": case["segments"] });

    let input = StagedInput {
        base: "base",
        vault_id: "vault-1",
        source_title: "Capture",
        duration_ms: 6_000,
        width: 1920,
        height: 1080,
        has_audio: true,
        legacy_timeline: Some(&legacy),
        stems: &[],
        input_count: 0,
        webcam: None,
    };
    let result = from_staged(&input, "proj-1");

    assert_eq!(result.dropped_segments, 1);
    assert!(
        result
            .project
            .clips
            .iter()
            .all(|c| !(c.in_ms == 4_000 && c.out_ms == 3_000)),
        "the backwards segment must not become a clip: {:?}",
        result.project.clips
    );
    // Two of the three fixture segments survive (0..2000, 4000..6000).
    assert_eq!(result.project.clips.len(), 2);
}

#[test]
fn explicitly_empty_timeline_migrates_to_no_clips() {
    let legacy = serde_json::json!({ "segments": [] });
    let input = StagedInput {
        base: "base",
        vault_id: "vault-1",
        source_title: "Capture",
        duration_ms: 5_000,
        width: 1920,
        height: 1080,
        has_audio: true,
        legacy_timeline: Some(&legacy),
        stems: &[],
        input_count: 0,
        webcam: None,
    };
    let result = from_staged(&input, "proj-1");
    assert!(
        result.project.clips.is_empty(),
        "an explicit empty edit must not be resurrected as the whole capture"
    );
    assert_eq!(result.dropped_segments, 0);
}

#[test]
fn malformed_timeline_degrades_to_the_whole_capture() {
    let legacy = serde_json::json!("junk");
    let input = StagedInput {
        base: "base",
        vault_id: "vault-1",
        source_title: "Capture",
        duration_ms: 8_000,
        width: 1920,
        height: 1080,
        has_audio: true,
        legacy_timeline: Some(&legacy),
        stems: &[],
        input_count: 0,
        webcam: None,
    };
    let result = from_staged(&input, "proj-1");
    assert_eq!(result.dropped_segments, 0);
    assert_eq!(result.project.clips.len(), 1);
    let clip = &result.project.clips[0];
    assert_eq!(clip.in_ms, 0);
    assert_eq!(clip.out_ms, 8_000);
    assert_eq!(clip.start_ms, 0);
}

#[test]
fn migrated_project_validates() {
    for case in fixtures()["cases"].as_array().expect("cases") {
        let name = case["name"].as_str().expect("name");
        let segments = &case["segments"];
        let legacy = serde_json::json!({ "segments": segments });
        let duration_ms = duration_for(segments).max(6_000);

        let input = StagedInput {
            base: "base",
            vault_id: "vault-1",
            source_title: "Capture",
            duration_ms,
            width: 1920,
            height: 1080,
            has_audio: true,
            legacy_timeline: Some(&legacy),
            stems: &[],
            input_count: 0,
            webcam: None,
        };
        let result = from_staged(&input, "proj-1");
        assert!(
            validate_project(&result.project).is_ok(),
            "case {name:?}: migrated project failed validation"
        );
    }

    // And the two degrade paths (absent / malformed) validate too.
    let whole_input = StagedInput {
        base: "base",
        vault_id: "vault-1",
        source_title: "Capture",
        duration_ms: 4_000,
        width: 1920,
        height: 1080,
        has_audio: true,
        legacy_timeline: None,
        stems: &[],
        input_count: 0,
        webcam: None,
    };
    assert!(validate_project(&from_staged(&whole_input, "proj-1").project).is_ok());
}

#[test]
fn vault_id_comes_from_the_input_not_a_default() {
    let input = StagedInput {
        base: "base",
        vault_id: "vault-distinct-42",
        source_title: "Capture",
        duration_ms: 1_000,
        width: 1920,
        height: 1080,
        has_audio: false,
        legacy_timeline: None,
        stems: &[],
        input_count: 0,
        webcam: None,
    };
    let result = from_staged(&input, "proj-1");
    assert_eq!(result.project.destination.vault, "vault-distinct-42");
    assert_ne!(result.project.destination.vault, String::new());
}

#[test]
fn nearest_canvas_picks_by_aspect() {
    assert_eq!(nearest_canvas(1920, 1080), (1280, 720));
    assert_eq!(nearest_canvas(1080, 1920), (720, 1280));
    assert_eq!(nearest_canvas(1000, 1000), (720, 720));
    assert_eq!(nearest_canvas(1024, 768), (960, 720));
    assert_eq!(nearest_canvas(2560, 1080), (1280, 720));
    assert!(
        !canvas_is_exact(2560, 1080),
        "an ultrawide capture is not aspect-exact against its nearest canvas"
    );
    assert!(canvas_is_exact(1920, 1080), "16:9 exactly matches 1280x720");
    assert!(canvas_is_exact(1024, 768), "4:3 exactly matches 960x720");
}
