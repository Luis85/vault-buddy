//! Tests for `package_plan.rs`. Fixtures come from `test_support` and are
//! asymmetric on purpose: each asset id is used for a different reason
//! (live clip, snapshot-only, unreferenced, card, linked), so a rule that
//! confuses two of them fails.

use super::*;
use crate::editor::model::{AssetKind, Builtin, MediaType, Project, TrackKind};
use crate::editor::model_cues::{Product, Record, WorkspaceEnvelope};
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::{new_product, Map, WORKSPACE_SCHEMA};

fn envelope(project: Project, products: Vec<Product>) -> WorkspaceEnvelope {
    WorkspaceEnvelope {
        schema: WORKSPACE_SCHEMA.to_string(),
        project,
        workspace: serde_json::json!({}),
        record: Record {
            id: "proj1".to_string(),
            revision: 4,
            created_at: "2026-09-21T00:00:00Z".to_string(),
            updated_at: "2026-09-22T00:00:00Z".to_string(),
            products,
            extra: Map::new(),
        },
        saved_at: "2026-09-23T10:00:00Z".to_string(),
        extra: Map::new(),
    }
}

/// `live` on a clip; `card` a card builtin on a clip; `detached` linked to
/// `live` on a clip; `screen` a screen builtin on a clip; `spare` in the
/// library but on no clip; `old` only in a retained product's snapshot.
fn mixed() -> WorkspaceEnvelope {
    let mut snap = minimal_project();
    snap.id = "proj1".to_string();
    snap.tracks.push(track("v1", TrackKind::Video, false));
    snap.assets.push(asset("old", AssetKind::Video, 4200));
    snap.clips.push(clip("s0", "v1", "old", 0, 0, 900));
    let product = new_product(
        &snap,
        2,
        "prod1",
        "Take one",
        "take-one.mp4",
        900,
        None,
        "t",
    );

    let mut p = minimal_project();
    p.id = "proj1".to_string();
    p.tracks.push(track("v1", TrackKind::Video, false));
    p.tracks.push(track("a1", TrackKind::Audio, false));
    p.assets.push(asset("live", AssetKind::Video, 5000));
    let mut card = asset("card", AssetKind::Video, 3000);
    card.builtin = Some(Builtin::Card);
    p.assets.push(card);
    let mut detached = asset("detached", AssetKind::Audio, 5000);
    detached.linked_asset = Some("live".to_string());
    p.assets.push(detached);
    let mut screen = asset("screen", AssetKind::Video, 7000);
    screen.builtin = Some(Builtin::Screen);
    p.assets.push(screen);
    p.assets.push(asset("spare", AssetKind::Audio, 1000));
    p.clips.push(clip("c0", "v1", "live", 0, 0, 1000));
    p.clips.push(clip("c1", "v1", "card", 1000, 0, 1000));
    p.clips.push(clip("c2", "a1", "detached", 0, 0, 1000));
    p.clips.push(clip("c3", "v1", "screen", 2000, 0, 1000));
    envelope(p, vec![product])
}

#[test]
fn format_is_read_from_the_last_extension_in_any_case() {
    assert_eq!(
        PackageFormat::of_file_name("Demo.vbproject.ZIP"),
        Some(PackageFormat::Portable)
    );
    assert_eq!(
        PackageFormat::of_file_name("Demo.json"),
        Some(PackageFormat::Lightweight)
    );
    assert_eq!(PackageFormat::of_file_name("Demo.zip.txt"), None);
    assert_eq!(PackageFormat::of_file_name("zip"), None);
}

#[test]
fn format_wire_spelling_is_pinned() {
    assert_eq!(
        serde_json::to_value(PackageFormat::Portable).unwrap(),
        serde_json::json!("portable")
    );
    assert_eq!(
        serde_json::from_value::<PackageFormat>(serde_json::json!("lightweight")).unwrap(),
        PackageFormat::Lightweight
    );
}

// A17 + the Task 38 ruling: a snapshot-only source is carried, a screen
// builtin IS a file, and only cards and linked (detached) audio need no
// bytes. `spare` is on no clip, so no package may carry it.
#[test]
fn media_needs_follow_references_and_the_file_backed_rule() {
    let env = mixed();
    let needed: Vec<String> = assets_needing_media(&env).into_iter().collect();
    assert_eq!(needed, ["live", "old", "screen"]);
    let backed: Vec<String> = file_backed_asset_ids(&env).into_iter().collect();
    assert_eq!(backed, ["live", "old", "screen", "spare"]);
}

#[test]
fn an_id_that_is_a_card_in_one_graph_and_a_file_in_another_needs_bytes() {
    let mut env = mixed();
    // The snapshot's `old` becomes a card while the live graph adopts a
    // real `old` on no clip: the snapshot still references it, and one
    // file-backed definition is enough to need its bytes.
    let snap = env.record.products[0].snapshot.as_mut().unwrap();
    snap.assets[0].builtin = Some(Builtin::Card);
    env.project
        .assets
        .push(asset("old", AssetKind::Video, 4200));
    assert!(assets_needing_media(&env).contains("old"));
}

#[test]
fn ids_differing_only_by_case_are_a_problem_named_by_both_ids() {
    let mut env = mixed();
    assert_eq!(asset_id_problem(&env), None);
    env.project.assets.push(asset("LIVE", AssetKind::Video, 10));
    let problem = asset_id_problem(&env).expect("a case collision");
    assert!(
        problem.contains("LIVE") && problem.contains("live"),
        "{problem}"
    );
}

#[test]
fn a_case_collision_across_a_snapshot_is_also_a_problem() {
    let mut env = mixed();
    env.project.assets.push(asset("OLD", AssetKind::Video, 10));
    assert!(asset_id_problem(&env).is_some());
}

#[test]
fn a_device_name_id_is_a_problem() {
    let mut env = mixed();
    env.project.assets.push(asset("Con", AssetKind::Video, 10));
    let problem = asset_id_problem(&env).expect("a device name");
    assert!(problem.contains("Con"), "{problem}");
}

#[test]
fn media_extension_keeps_only_a_short_alphanumeric_extension() {
    assert_eq!(media_extension("clip.MP4").as_deref(), Some("mp4"));
    assert_eq!(media_extension("a.b.webm").as_deref(), Some("webm"));
    assert_eq!(media_extension("noext"), None);
    assert_eq!(media_extension("x.toolongext"), None);
    assert_eq!(media_extension("x.m-4"), None);
    assert_eq!(media_extension("x."), None);
}

#[test]
fn placeholder_names_use_the_original_extension_else_the_kind() {
    let mut named = asset("a1", AssetKind::Video, 10);
    named.original_name = Some("Talk.MOV".to_string());
    assert_eq!(placeholder_file_name(&named), "a1.mov");
    let mut by_name = asset("a2", AssetKind::Audio, 10);
    by_name.name = "voice.wav".to_string();
    assert_eq!(placeholder_file_name(&by_name), "a2.wav");
    assert_eq!(
        placeholder_file_name(&asset("a3", AssetKind::Audio, 10)),
        "a3.m4a"
    );
    let mut image = asset("a4", AssetKind::Video, 10);
    image.media_type = Some(MediaType::Image);
    assert_eq!(placeholder_file_name(&image), "a4.png");
    assert_eq!(
        placeholder_file_name(&asset("a5", AssetKind::Video, 10)),
        "a5.mp4"
    );
}

#[test]
fn a_chosen_name_always_ends_in_the_formats_double_extension() {
    let p = PackageFormat::Portable;
    assert_eq!(package_file_name("Demo", p), "Demo.vbproject.zip");
    assert_eq!(package_file_name("Demo.zip", p), "Demo.vbproject.zip");
    assert_eq!(package_file_name("Demo.ZIP", p), "Demo.vbproject.zip");
    assert_eq!(
        package_file_name("Demo.vbproject.ZIP", p),
        "Demo.vbproject.ZIP"
    );
    let l = PackageFormat::Lightweight;
    assert_eq!(package_file_name("Demo.json", l), "Demo.vbproject.json");
    assert_eq!(package_file_name("Demo.zip", l), "Demo.zip.vbproject.json");
}

#[test]
fn the_suggested_name_is_a_safe_windows_file_name() {
    let p = PackageFormat::Portable;
    assert_eq!(
        suggested_file_name("Intro: a/b?", p),
        "Intro- a-b-.vbproject.zip"
    );
    assert_eq!(
        suggested_file_name("  . ", p),
        "Tutorial project.vbproject.zip"
    );
    assert_eq!(
        suggested_file_name("NUL", PackageFormat::Lightweight),
        "Project NUL.vbproject.json"
    );
    let long = "x".repeat(300);
    assert_eq!(
        suggested_file_name(&long, p),
        format!("{}.vbproject.zip", "x".repeat(100))
    );
}

#[test]
fn rekey_moves_the_project_record_products_and_snapshots_together() {
    let mut env = mixed();
    rekey_envelope(&mut env, "fresh123");
    assert_eq!(env.project.id, "fresh123");
    assert_eq!(env.record.id, "fresh123");
    assert_eq!(env.record.products[0].project_id, "fresh123");
    assert_eq!(
        env.record.products[0].snapshot.as_ref().unwrap().id,
        "fresh123"
    );
    crate::editor::validate_envelope(&env).expect("still a valid envelope");
}

fn facts(has_audio: bool, kind: FactsMediaKind) -> SourceFacts {
    SourceFacts {
        has_audio,
        has_video: kind != FactsMediaKind::Audio,
        width: Some(1600),
        height: Some(900),
        media_kind: kind,
        size: 47,
        duration_ms: 61_500,
    }
}

// Fix round 1 (GAP-182): the facts ride in `record.extra` and come back
// out unchanged; taking them removes the transport key, so nothing of it
// is ever stored.
#[test]
fn source_facts_round_trip_through_the_record_and_are_taken_out() {
    let mut env = mixed();
    let sent = BTreeMap::from([
        ("screen".to_string(), facts(false, FactsMediaKind::Video)),
        ("live".to_string(), facts(true, FactsMediaKind::Video)),
    ]);
    attach_source_facts(&mut env, &sent);
    let wire: WorkspaceEnvelope =
        serde_json::from_str(&serde_json::to_string(&env).unwrap()).unwrap();
    let mut env = wire;
    assert!(env.record.extra.contains_key(SOURCE_FACTS_KEY));
    assert_eq!(take_source_facts(&mut env).unwrap(), sent);
    assert!(!env.record.extra.contains_key(SOURCE_FACTS_KEY));
    assert_eq!(take_source_facts(&mut env).unwrap(), BTreeMap::new());
}

#[test]
fn the_source_facts_wire_shape_is_pinned() {
    assert_eq!(
        serde_json::to_value(facts(false, FactsMediaKind::Image)).unwrap(),
        serde_json::json!({"hasAudio": false, "hasVideo": true, "width": 1600, "height": 900,
                           "mediaKind": "image", "size": 47, "durationMs": 61500})
    );
}

#[test]
fn malformed_source_facts_are_refused() {
    for bad in [
        serde_json::json!("not a map"),
        serde_json::json!({"live": {"hasAudio": "yes", "hasVideo": true, "mediaKind": "video", "size": 1, "durationMs": 1}}),
        serde_json::json!({"live": {"hasAudio": true, "hasVideo": true, "mediaKind": "video", "size": 1, "durationMs": 1, "path": "C:/x"}}),
    ] {
        let mut env = mixed();
        env.record
            .extra
            .insert(SOURCE_FACTS_KEY.to_string(), bad.clone());
        assert!(take_source_facts(&mut env).is_err(), "{bad}");
    }
}
