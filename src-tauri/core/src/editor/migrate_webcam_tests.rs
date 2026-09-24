//! `migrate::from_staged`'s synchronized-webcam half (Task 51; F-22, F26) —
//! a sibling file because `migrate.rs` sits near the 800-line Rust cap.

use super::*;
use crate::editor::probe::ProbeFacts;
use crate::editor::take::take_asset;
use crate::editor::validate_project;

const BASE: &str = "2026-09-21 1430 Demo";

fn webcam(offset_ms: i64) -> WebcamInput {
    WebcamInput {
        // Asymmetric on purpose: a 4:3 device and a length that is neither
        // the capture's nor round.
        duration_ms: 58_370,
        width: 640,
        height: 480,
        file: format!("{BASE}.webcam.mp4"),
        offset_ms,
    }
}

fn migrate(webcam: Option<WebcamInput>) -> Project {
    migrate_cut(webcam, None)
}

fn migrate_cut(webcam: Option<WebcamInput>, legacy: Option<&serde_json::Value>) -> Project {
    let input = StagedInput {
        base: BASE,
        vault_id: "vault-1",
        source_title: "Demo window",
        // 1600x900 lands on the 1280x720 canvas -- never the capture's size.
        duration_ms: 61_500,
        width: 1600,
        height: 900,
        has_audio: true,
        legacy_timeline: legacy,
        stems: &[],
        webcam,
    };
    from_staged(&input, "proj-1").project
}

/// `(start, in, out)` of every clip of `asset_id`, in output order.
fn spans(project: &Project, asset_id: &str) -> Vec<(u64, u64, u64)> {
    let mut out: Vec<(u64, u64, u64)> = project
        .clips
        .iter()
        .filter(|c| c.asset_id == asset_id)
        .map(|c| (c.start_ms, c.in_ms, c.out_ms))
        .collect();
    out.sort();
    out
}

// Review fix round 1 (Important): the screen clips follow the legacy
// timeline -- trimmed AND reordered -- so a webcam placed as one uncut clip
// from `offset_ms` sat every presenter frame over a different screen moment
// than the one it was recorded with, ran past the project's end, and still
// claimed `shared-clock`. Each surviving segment gets its OWN webcam clip,
// mapped through the same source -> output mapping as the screen clip, and
// clipped to the span the webcam file actually covers (capture ms
// `offset .. offset + duration`, here 120 .. 58_490).
#[test]
fn a_cut_and_reordered_capture_keeps_the_presenter_on_its_screen_moments() {
    // Asymmetric on purpose: a reorder (the late block first), a segment
    // straddling the webcam's START and one straddling its END, and a
    // segment the webcam never covered at all.
    let legacy = serde_json::json!({ "segments": [
        { "sourceStartMs": 30_000, "sourceEndMs": 40_000 },
        { "sourceStartMs": 0, "sourceEndMs": 3_000 },
        { "sourceStartMs": 58_000, "sourceEndMs": 61_500 },
        { "sourceStartMs": 5_000, "sourceEndMs": 12_000 }
    ]});
    let project = migrate_cut(Some(webcam(120)), Some(&legacy));
    validate_project(&project).expect("validates");
    assert_eq!(
        spans(&project, "src"),
        [
            (0, 30_000, 40_000),
            (10_000, 0, 3_000),
            (13_000, 58_000, 61_500),
            (16_500, 5_000, 12_000)
        ]
    );
    assert_eq!(
        spans(&project, WEBCAM_ASSET_ID),
        [
            // Same output start, source shifted by the offset.
            (0, 29_880, 39_880),
            // The webcam's first frame is 120 ms into this segment.
            (10_120, 0, 2_880),
            // The webcam file ends at capture ms 58_490.
            (13_000, 57_880, 58_370),
            (16_500, 4_880, 11_880)
        ],
        "every presenter frame must sit over the screen moment it was recorded with"
    );
    // A segment entirely before the webcam started places nothing.
    let early = serde_json::json!({ "segments": [
        { "sourceStartMs": 0, "sourceEndMs": 100 },
        { "sourceStartMs": 200, "sourceEndMs": 1_200 }
    ]});
    let project = migrate_cut(Some(webcam(120)), Some(&early));
    validate_project(&project).unwrap();
    assert_eq!(spans(&project, WEBCAM_ASSET_ID), [(100, 80, 1_080)]);
    let last_end = project
        .clips
        .iter()
        .map(|c| c.start_ms + (c.out_ms - c.in_ms))
        .max();
    assert_eq!(
        last_end,
        Some(1_100),
        "the presenter never outruns the screen"
    );
}

fn num(v: f64) -> Num {
    Num::from_f64(v).unwrap()
}

// F26: the webcam track is its own clip on its own track ABOVE the screen
// (index 0 is the top), starting where its first frame arrived on the shared
// clock, placed as the presenter -- and the whole project still validates.
#[test]
fn migration_places_the_webcam_on_its_own_track_at_its_offset() {
    let project = migrate(Some(webcam(120)));
    validate_project(&project).expect("a migrated webcam project validates");

    let ids: Vec<&str> = project.tracks.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["v2", "v1", "a1"], "the presenter is the top layer");
    assert_eq!(project.tracks[0].kind, TrackKind::Video);

    let clip = project
        .clips
        .iter()
        .find(|c| c.asset_id == WEBCAM_ASSET_ID)
        .expect("the webcam has a clip");
    assert_eq!(clip.track_id, "v2");
    assert_eq!(clip.start_ms, 120, "the clip starts at the webcam's offset");
    assert_eq!((clip.in_ms, clip.out_ms), (0, 58_370));
    assert_eq!(
        (&clip.x, &clip.y, &clip.w, &clip.h),
        (&num(0.775), &num(0.06), &num(0.19), &num(0.3378)),
        "the presenter placement, never cornerPreset's margin"
    );
    assert_eq!(clip.frame_shape, Some(FrameShape::Circle));
    assert_eq!(clip.fit, Some(Fit::Cover));
    // The screen is untouched by it.
    let screen: Vec<&Clip> = project
        .clips
        .iter()
        .filter(|c| c.asset_id == "src")
        .collect();
    assert_eq!(screen.len(), 1);
    assert_eq!((screen[0].start_ms, screen[0].out_ms), (0, 61_500));

    let asset = project
        .assets
        .iter()
        .find(|a| a.id == WEBCAM_ASSET_ID)
        .expect("the webcam asset");
    assert_eq!(asset.kind, AssetKind::Video);
    assert_eq!(asset.duration_ms, 58_370);
    assert_eq!(
        (asset.width.clone(), asset.height.clone()),
        (Some(Num::from(640)), Some(Num::from(480)))
    );
    assert_eq!(
        asset.builtin, None,
        "a real staged file, not a synthesized builtin"
    );
    assert_eq!(
        asset.original_name.as_deref(),
        Some("2026-09-21 1430 Demo.webcam.mp4")
    );
}

// A device that delivered BEFORE the screen's first frame: the clip cannot
// start before 0, so the head it recorded early is trimmed off instead --
// every later frame still lands on the screen frame it was recorded with.
#[test]
fn a_negative_offset_trims_the_webcam_head_rather_than_shifting_it() {
    let project = migrate(Some(webcam(-500)));
    validate_project(&project).unwrap();
    let clip = project
        .clips
        .iter()
        .find(|c| c.asset_id == WEBCAM_ASSET_ID)
        .unwrap();
    assert_eq!((clip.start_ms, clip.in_ms, clip.out_ms), (0, 500, 58_370));
    // Entirely before the screen: nothing to place, the asset stays in the
    // library.
    let project = migrate(Some(webcam(-58_370)));
    validate_project(&project).unwrap();
    assert!(project.clips.iter().all(|c| c.asset_id != WEBCAM_ASSET_ID));
    assert!(project.assets.iter().any(|a| a.id == WEBCAM_ASSET_ID));
}

// "Synchronized" is a claim about THIS path only (ADR §4, SCREENS 05): the
// capture itself is not marked, a capture without a webcam marks nothing,
// and a webcam TAKE recorded later in the editor never claims it.
#[test]
fn only_synchronized_webcam_assets_are_marked_shared_clock() {
    let sync = |a: &Asset| a.extra.get(CAPTURE_SYNC_KEY).cloned();
    let project = migrate(Some(webcam(120)));
    let marked: Vec<&str> = project
        .assets
        .iter()
        .filter(|a| sync(a).is_some())
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(marked, [WEBCAM_ASSET_ID]);
    let webcam_asset = project
        .assets
        .iter()
        .find(|a| a.id == WEBCAM_ASSET_ID)
        .unwrap();
    assert_eq!(sync(webcam_asset), Some(serde_json::json!("shared-clock")));
    // The document spelling (R3): the key sits on the asset itself.
    assert_eq!(
        serde_json::to_value(webcam_asset).unwrap()["capture_sync"],
        "shared-clock"
    );

    assert!(migrate(None).assets.iter().all(|a| sync(a).is_none()));
    let take = take_asset(
        "take-1".to_string(),
        1,
        ProbeFacts {
            duration_ms: 4_000,
            width: Some(640),
            height: Some(480),
            has_video: true,
            has_audio: true,
        },
        1_024,
    );
    assert_eq!(sync(&take), None, "a later take is never synchronized");
}

/// The ONE presenter-placement table both languages read (the
/// `timeline-cases.json` precedent): `tests/editorWebcamDialog.test.ts`
/// pins `PRESENTER_CORNER` + `presenterBox` to it, this pins the Rust side,
/// so neither can drift without reddening a suite. `include_str!`, so moving
/// the file breaks this build.
const PLACEMENT: &str = include_str!("../../../../tests/fixtures/editor-presenter-placement.json");

#[test]
fn the_presenter_placement_matches_the_shared_table() {
    let table: serde_json::Value = serde_json::from_str(PLACEMENT).unwrap();
    assert_eq!(table["x"], PRESENTER_X);
    assert_eq!(table["y"], PRESENTER_Y);
    assert_eq!(table["w"], PRESENTER_W);
    assert_eq!(
        table["frameShape"],
        serde_json::to_value(PRESENTER_FRAME_SHAPE).unwrap()
    );
    assert_eq!(table["fit"], serde_json::to_value(PRESENTER_FIT).unwrap());
    let heights = table["heights"].as_array().unwrap();
    // One row per supported canvas: a table that silently shrinks proves
    // nothing.
    assert_eq!(heights.len(), limits::CANVASES.len());
    for row in heights {
        let (w, h) = (
            row["canvas"][0].as_u64().unwrap(),
            row["canvas"][1].as_u64().unwrap(),
        );
        let canvas = (u32::try_from(w).unwrap(), u32::try_from(h).unwrap());
        assert!(limits::CANVASES.contains(&canvas), "{canvas:?}");
        assert_eq!(
            presenter_height(canvas.0, canvas.1),
            row["h"].as_f64().unwrap(),
            "{canvas:?}: the circle is not round in pixels"
        );
    }
}
