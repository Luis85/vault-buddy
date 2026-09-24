//! `migrate::from_staged`'s audio-stem half (Task 53; F-05, F26) — a sibling
//! file because `migrate.rs` sits near the 800-line Rust cap.

use super::*;
use crate::editor::model::TrackKind;
use crate::editor::validate_project;

const BASE: &str = "2026-09-24 1015 Demo";

fn stem(index: u32, input: &str) -> StemInput {
    StemInput {
        index,
        input: input.to_string(),
        file: format!("{BASE}.stem-{index}.m4a"),
    }
}

fn migrate(stems: &[StemInput], legacy: Option<&serde_json::Value>) -> Project {
    let input = StagedInput {
        base: BASE,
        vault_id: "vault-1",
        source_title: "Demo window",
        duration_ms: 61_500,
        width: 1600,
        height: 900,
        has_audio: true,
        legacy_timeline: legacy,
        stems,
        webcam: None,
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

// F-05/F26: each stem becomes its own audio asset on its own audio track,
// placed on EVERY screen moment the legacy timeline kept (a cut and a
// reorder follow through, exactly as the webcam's clips do), while the
// capture's embedded MIX is muted — otherwise every voice would play twice,
// once in the mix and once in its stem. Asymmetric: a reorder, three stems
// with distinct names, a non-round duration.
#[test]
fn migration_creates_one_audio_track_per_stem_and_mutes_the_embedded_mix() {
    let legacy = serde_json::json!({ "segments": [
        { "sourceStartMs": 30_000, "sourceEndMs": 40_000 },
        { "sourceStartMs": 0, "sourceEndMs": 3_000 },
        { "sourceStartMs": 5_000, "sourceEndMs": 12_000 }
    ]});
    let stems = [stem(1, "USB Mic"), stem(2, "Speakers"), stem(3, "Line In")];
    let project = migrate(&stems, Some(&legacy));
    validate_project(&project).expect("validates");

    let screen = spans(&project, "src");
    assert_eq!(
        screen,
        [
            (0, 30_000, 40_000),
            (10_000, 0, 3_000),
            (13_000, 5_000, 12_000)
        ]
    );
    for s in &stems {
        let id = format!("stem-{}", s.index);
        let asset = project
            .assets
            .iter()
            .find(|a| a.id == id)
            .expect("a stem asset");
        assert_eq!(asset.kind, AssetKind::Audio);
        assert_eq!(asset.name, s.input);
        assert_eq!(
            asset.duration_ms, 61_500,
            "a stem spans the whole mixed track"
        );
        assert_eq!(asset.original_name.as_deref(), Some(s.file.as_str()));
        assert_eq!(
            spans(&project, &id),
            screen,
            "{id} must sit on exactly the screen's moments"
        );
        let clips: Vec<&Clip> = project.clips.iter().filter(|c| c.asset_id == id).collect();
        let track = &clips[0].track_id;
        assert!(clips.iter().all(|c| &c.track_id == track && !c.muted));
        let track = project.tracks.iter().find(|t| &t.id == track).unwrap();
        assert_eq!(
            (track.kind, track.name.as_str()),
            (TrackKind::Audio, s.input.as_str())
        );
    }
    let audio_tracks = project
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Audio)
        .count();
    assert_eq!(audio_tracks, 1 + stems.len(), "a1 plus one per stem");
    assert!(
        project
            .clips
            .iter()
            .filter(|c| c.asset_id == "src")
            .all(|c| c.muted),
        "the embedded mix must be muted when its inputs are placed as stems"
    );

    // No stems: nothing muted, no stem asset or track — today's migration.
    let plain = migrate(&[], Some(&legacy));
    assert!(plain.clips.iter().all(|c| !c.muted));
    assert_eq!(plain.assets.len(), 1);
    assert_eq!(plain.tracks.len(), 2);
}
