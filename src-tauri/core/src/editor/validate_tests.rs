//! Tests for `validate.rs`, split into this sibling file (`#[path]`) so
//! `validate.rs` itself stays well under the 800-nonblank-line Rust cap
//! as the DATA-MODEL.md § Validation order rule coverage grows.

use super::*;
use crate::editor::model_cues::{Product, Record};

const REFERENCE_WORKSPACE: &str =
    include_str!("../../../../tests/fixtures/editor/reference-workspace.example.json");

fn base_project_json() -> serde_json::Value {
    serde_json::json!({
        "schema": "vault-buddy-video-project/3",
        "id": "proj1",
        "title": "A tutorial",
        "canvas": {"width": 1280, "height": 720, "fps": 30},
        "master_gain": 0.8,
        "assets": [
            {"id": "a1", "kind": "video", "name": "Asset One", "duration_ms": 5000}
        ],
        "tracks": [
            {
                "id": "t1", "kind": "video", "name": "Track",
                "visible": true, "locked": false, "muted": false, "solo": false,
                "volume": 1
            }
        ],
        "clips": [
            {
                "id": "c1", "asset_id": "a1", "track_id": "t1", "name": "Clip",
                "start_ms": 0, "in_ms": 0, "out_ms": 1000,
                "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
                "opacity": 1, "volume": 1, "muted": false,
                "x": 0, "y": 0, "w": 1, "h": 1
            }
        ],
        "effects": [],
        "markers": [],
        "transitions": [],
        "destination": {"vault": "v1", "folder": "Videos", "dated": false}
    })
}

fn base_project() -> Project {
    serde_json::from_value(base_project_json()).expect("base project fixture must parse")
}

fn base_record() -> Record {
    Record {
        id: "rec1".to_string(),
        revision: 1,
        created_at: "2026-09-21T00:00:00Z".to_string(),
        updated_at: "2026-09-21T00:00:00Z".to_string(),
        products: Vec::new(),
        extra: crate::editor::Map::new(),
    }
}

fn base_envelope() -> WorkspaceEnvelope {
    WorkspaceEnvelope {
        schema: super::super::WORKSPACE_SCHEMA.to_string(),
        project: base_project(),
        workspace: serde_json::json!({}),
        record: base_record(),
        saved_at: "2026-09-21T00:00:00Z".to_string(),
        extra: crate::editor::Map::new(),
    }
}

#[test]
fn reference_example_is_valid() {
    let envelope: WorkspaceEnvelope = serde_json::from_str(REFERENCE_WORKSPACE).unwrap();
    assert!(
        validate_envelope(&envelope).is_ok(),
        "the reference tutorial workspace must validate cleanly"
    );
}

#[test]
fn duplicate_clip_ids_are_rejected_naming_the_id() {
    let mut project = base_project();
    let dup = project.clips[0].clone();
    project.clips.push(dup);
    let err = validate_project(&project).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert!(err.message.contains("c1"), "message: {}", err.message);
}

#[test]
fn dangling_asset_reference_is_rejected() {
    let mut project = base_project();
    project.clips[0].asset_id = "missing".to_string();
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("missing"), "message: {}", err.message);
}

#[test]
fn audio_asset_on_video_track_is_rejected() {
    let mut project = base_project();
    project.assets.push(
        serde_json::from_value(serde_json::json!({
            "id": "a2", "kind": "audio", "name": "Audio Asset", "duration_ms": 5000
        }))
        .unwrap(),
    );
    project.clips[0].asset_id = "a2".to_string();
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("c1"), "message: {}", err.message);
}

#[test]
fn empty_source_range_is_rejected() {
    let mut project = base_project();
    project.clips[0].in_ms = 500;
    project.clips[0].out_ms = 500;
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("c1"), "message: {}", err.message);
}

#[test]
fn out_ms_beyond_asset_duration_is_rejected() {
    let mut project = base_project();
    project.clips[0].out_ms = project.assets[0].duration_ms + 1;
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("c1"), "message: {}", err.message);
}

#[test]
fn fade_longer_than_half_the_clip_is_rejected() {
    // clips[0]'s output duration is exactly 1000 ms (in_ms 0, out_ms 1000, no speed).
    let mut too_long = base_project();
    too_long.clips[0].fade_in_ms = 501;
    let err = validate_project(&too_long).unwrap_err();
    assert!(
        err.message.contains("fade_in_ms"),
        "message: {}",
        err.message
    );

    let mut exactly_half = base_project();
    exactly_half.clips[0].fade_in_ms = 500;
    assert!(validate_project(&exactly_half).is_ok());
}

#[test]
fn cyclic_linked_assets_are_rejected() {
    let mut project = base_project();
    project.clips.clear();
    project.assets = vec![
        serde_json::from_value(serde_json::json!({
            "id": "a1", "kind": "audio", "name": "A", "duration_ms": 1000,
            "linked_asset": "a2"
        }))
        .unwrap(),
        serde_json::from_value(serde_json::json!({
            "id": "a2", "kind": "audio", "name": "B", "duration_ms": 1000,
            "linked_asset": "a1"
        }))
        .unwrap(),
    ];
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("cycle"), "message: {}", err.message);
}

#[test]
fn self_transition_is_rejected() {
    let mut project = base_project();
    project.transitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "tr1", "from": "c1", "to": "c1", "duration_ms": 100, "kind": "dissolve"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("tr1"), "message: {}", err.message);
}

#[test]
fn cross_track_transition_is_rejected() {
    let mut project = base_project();
    project.tracks.push(
        serde_json::from_value(serde_json::json!({
            "id": "t2", "kind": "video", "name": "Track Two",
            "visible": true, "locked": false, "muted": false, "solo": false,
            "volume": 1
        }))
        .unwrap(),
    );
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c2", "asset_id": "a1", "track_id": "t2", "name": "Clip Two",
            "start_ms": 2000, "in_ms": 0, "out_ms": 1000,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    project.transitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "tr1", "from": "c1", "to": "c2", "duration_ms": 100, "kind": "dissolve"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("tr1"), "message: {}", err.message);
    assert!(
        err.message.contains("track"),
        "message should name the track mismatch: {}",
        err.message
    );
}

#[test]
fn oversized_collection_is_rejected() {
    let mut project = base_project();
    let template = project.clips[0].clone();
    project.clips = (0..601)
        .map(|i| {
            let mut c = template.clone();
            c.id = format!("c{i}");
            c
        })
        .collect();
    let err = validate_project(&project).unwrap_err();
    assert!(err.message.contains("clips"), "message: {}", err.message);
}

#[test]
fn snapshot_is_validated_too() {
    let mut envelope = base_envelope();
    let mut bad_snapshot = base_project();
    bad_snapshot.clips[0].asset_id = "missing".to_string();
    envelope.record.products.push(Product {
        id: "prod1".to_string(),
        project_id: envelope.project.id.clone(),
        name: "Product".to_string(),
        filename: "product.mp4".to_string(),
        mime: "video/mp4".to_string(),
        revision: 1,
        duration_ms: 1000,
        created_at: "2026-09-21T00:00:00Z".to_string(),
        edit_fingerprint: "abc123".to_string(),
        snapshot: Some(Box::new(bad_snapshot)),
        render_range: None,
        extra: crate::editor::Map::new(),
    });
    let err = validate_envelope(&envelope).unwrap_err();
    assert!(err.message.contains("missing"), "message: {}", err.message);
}

#[test]
fn track_name_at_200_chars_is_valid_but_201_is_rejected() {
    let mut ok = base_project();
    ok.tracks[0].name = "n".repeat(200);
    assert!(validate_project(&ok).is_ok(), "200 chars must be accepted");

    let mut too_long = base_project();
    too_long.tracks[0].name = "n".repeat(201);
    let err = validate_project(&too_long).unwrap_err();
    assert!(err.message.contains("t1"), "message: {}", err.message);
}

// --- Fix-round 1: dedicated rejection tests for every remaining Behavior
// rule (each asserts the error names the offending id/rule) ---

#[test]
fn clip_y_outside_unit_range_is_rejected() {
    // Asymmetric on purpose (x stays valid, only y goes out of range) so a
    // swapped axis in the implementation — checking x's bound against y's
    // value, or vice versa — cannot pass this test by accident.
    let mut project = base_project();
    project.clips[0].x = serde_json::Number::from_f64(0.5).unwrap();
    project.clips[0].y = serde_json::Number::from_f64(1.2).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c1: y 1.2"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_w_below_minimum_is_rejected() {
    // Asymmetric on purpose (h stays valid, only w goes below 0.1) — same
    // reasoning as `clip_y_outside_unit_range_is_rejected`.
    let mut project = base_project();
    project.clips[0].w = serde_json::Number::from_f64(0.05).unwrap();
    project.clips[0].h = serde_json::Number::from_f64(0.5).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c1: w 0.05"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_x_outside_unit_range_is_rejected() {
    // Asymmetric on purpose (y stays valid, only x goes out of range) —
    // the converse of `clip_y_outside_unit_range_is_rejected`, so an x/y
    // bound genuinely swapped in the implementation (checking x against
    // y's value or vice versa) cannot pass either test.
    let mut project = base_project();
    project.clips[0].x = serde_json::Number::from_f64(1.2).unwrap();
    project.clips[0].y = serde_json::Number::from_f64(0.5).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c1: x 1.2"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_h_below_minimum_is_rejected() {
    // Asymmetric on purpose (w stays valid, only h goes below 0.1) — the
    // converse of `clip_w_below_minimum_is_rejected`, so a w/h bound
    // genuinely swapped in the implementation cannot pass either test.
    let mut project = base_project();
    project.clips[0].w = serde_json::Number::from_f64(0.5).unwrap();
    project.clips[0].h = serde_json::Number::from_f64(0.05).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c1: h 0.05"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_speed_out_of_range_is_rejected() {
    let mut too_slow = base_project();
    too_slow.clips[0].speed = Some(serde_json::Number::from_f64(0.1).unwrap());
    let err = validate_project(&too_slow).unwrap_err();
    assert!(
        err.message.contains("c1: speed 0.1"),
        "message: {}",
        err.message
    );

    let mut too_fast = base_project();
    too_fast.clips[0].speed = Some(serde_json::Number::from_f64(5.0).unwrap());
    let err = validate_project(&too_fast).unwrap_err();
    assert!(
        err.message.contains("c1: speed 5"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_crop_zoom_out_of_range_is_rejected() {
    let mut too_small = base_project();
    too_small.clips[0].crop_zoom = Some(serde_json::Number::from_f64(0.5).unwrap());
    let err = validate_project(&too_small).unwrap_err();
    assert!(
        err.message.contains("c1: crop_zoom 0.5"),
        "message: {}",
        err.message
    );

    let mut too_big = base_project();
    too_big.clips[0].crop_zoom = Some(serde_json::Number::from_f64(3.5).unwrap());
    let err = validate_project(&too_big).unwrap_err();
    assert!(
        err.message.contains("c1: crop_zoom 3.5"),
        "message: {}",
        err.message
    );
}

#[test]
fn canvas_not_a_supported_pair_is_rejected() {
    let mut project = base_project();
    project.canvas.width = 1920;
    project.canvas.height = 1080;
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("proj1") && err.message.contains("canvas"),
        "message: {}",
        err.message
    );
}

#[test]
fn canvas_fps_other_than_30_is_rejected() {
    let mut project = base_project();
    project.canvas.fps = 60;
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("proj1") && err.message.contains("fps"),
        "message: {}",
        err.message
    );
}

#[test]
fn master_gain_out_of_range_is_rejected() {
    let mut negative = base_project();
    negative.master_gain = -0.1;
    let err = validate_project(&negative).unwrap_err();
    assert!(
        err.message.contains("proj1: master_gain -0.1"),
        "message: {}",
        err.message
    );

    let mut too_high = base_project();
    too_high.master_gain = 1.1;
    let err = validate_project(&too_high).unwrap_err();
    assert!(
        err.message.contains("proj1: master_gain 1.1"),
        "message: {}",
        err.message
    );
}

#[test]
fn project_schema_mismatch_is_rejected() {
    let mut project = base_project();
    project.schema = "vault-buddy-video-project/2".to_string();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("proj1") && err.message.contains("schema"),
        "message: {}",
        err.message
    );
}

#[test]
fn arrow_effect_missing_x2_y2_is_rejected() {
    let mut project = base_project();
    project.effects.push(
        serde_json::from_value(serde_json::json!({
            "id": "e1", "clip_id": "c1", "kind": "arrow",
            "start_ms": 0, "end_ms": 100, "x": 0.1, "y": 0.1, "color": "#ffffff"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("e1") && err.message.contains("x2"),
        "message: {}",
        err.message
    );
}

#[test]
fn zoom_effect_missing_factor_is_rejected() {
    let mut project = base_project();
    project.effects.push(
        serde_json::from_value(serde_json::json!({
            "id": "e1", "clip_id": "c1", "kind": "zoom",
            "start_ms": 0, "end_ms": 100, "x": 0.1, "y": 0.1, "color": "#ffffff"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("e1") && err.message.contains("factor"),
        "message: {}",
        err.message
    );
}

#[test]
fn step_effect_missing_number_is_rejected() {
    // carries `text` so the missing-number requirement is isolated from
    // step's separate missing-text requirement (see
    // `step_effect_missing_text_is_rejected`).
    let mut project = base_project();
    project.effects.push(
        serde_json::from_value(serde_json::json!({
            "id": "e1", "clip_id": "c1", "kind": "step",
            "start_ms": 0, "end_ms": 100, "x": 0.1, "y": 0.1, "color": "#ffffff",
            "text": "Step one"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("e1") && err.message.contains("number"),
        "message: {}",
        err.message
    );
}

#[test]
fn text_effect_missing_text_is_rejected() {
    let mut project = base_project();
    project.effects.push(
        serde_json::from_value(serde_json::json!({
            "id": "e1", "clip_id": "c1", "kind": "text",
            "start_ms": 0, "end_ms": 100, "x": 0.1, "y": 0.1, "color": "#ffffff"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("e1") && err.message.contains("text"),
        "message: {}",
        err.message
    );
}

#[test]
fn step_effect_missing_text_is_rejected() {
    // carries `number` so the missing-text requirement is isolated from
    // step's separate missing-number requirement (see
    // `step_effect_missing_number_is_rejected`).
    let mut project = base_project();
    project.effects.push(
        serde_json::from_value(serde_json::json!({
            "id": "e1", "clip_id": "c1", "kind": "step",
            "start_ms": 0, "end_ms": 100, "x": 0.1, "y": 0.1, "color": "#ffffff",
            "number": 3
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("e1") && err.message.contains("text"),
        "message: {}",
        err.message
    );
}

#[test]
fn transition_side_reused_is_rejected() {
    // c1 (from `base_project`) is 0..1000ms on t1, output duration 1000ms,
    // so it ends at 1000ms. c2 and c3 both start at 1000ms on the same
    // track, so a transition from c1 into EITHER is individually valid —
    // but c1 cannot be the "from" side of two transitions at once.
    let mut project = base_project();
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c2", "asset_id": "a1", "track_id": "t1", "name": "Clip Two",
            "start_ms": 1000, "in_ms": 0, "out_ms": 1000,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c3", "asset_id": "a1", "track_id": "t1", "name": "Clip Three",
            "start_ms": 1000, "in_ms": 0, "out_ms": 500,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    project.transitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "tr1", "from": "c1", "to": "c2", "duration_ms": 100, "kind": "dissolve"
        }))
        .unwrap(),
    );
    project.transitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "tr2", "from": "c1", "to": "c3", "duration_ms": 100, "kind": "dissolve"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("tr2") && err.message.contains("already has a transition"),
        "message: {}",
        err.message
    );
}

#[test]
fn transition_to_side_reused_is_rejected() {
    // The converse of `transition_side_reused_is_rejected`: two different
    // clips (c1 and c5) each end exactly at 1000ms, so a transition from
    // EITHER into c6 (which starts at 1000ms) is individually valid — but
    // c6 cannot be the "to" side of two transitions at once.
    let mut project = base_project();
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c5", "asset_id": "a1", "track_id": "t1", "name": "Clip Five",
            "start_ms": 500, "in_ms": 0, "out_ms": 500,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    project.clips.push(
        serde_json::from_value(serde_json::json!({
            "id": "c6", "asset_id": "a1", "track_id": "t1", "name": "Clip Six",
            "start_ms": 1000, "in_ms": 0, "out_ms": 500,
            "fade_in_ms": 0, "fade_out_ms": 0, "fade_curve": "linear",
            "opacity": 1, "volume": 1, "muted": false,
            "x": 0, "y": 0, "w": 1, "h": 1
        }))
        .unwrap(),
    );
    project.transitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "tr1", "from": "c1", "to": "c6", "duration_ms": 100, "kind": "dissolve"
        }))
        .unwrap(),
    );
    project.transitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "tr2", "from": "c5", "to": "c6", "duration_ms": 100, "kind": "dissolve"
        }))
        .unwrap(),
    );
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("tr2") && err.message.contains("already has a transition"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_opacity_out_of_range_is_rejected() {
    let mut project = base_project();
    project.clips[0].opacity = serde_json::Number::from_f64(1.5).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c1: opacity 1.5"),
        "message: {}",
        err.message
    );
}

#[test]
fn clip_volume_out_of_range_is_rejected() {
    let mut project = base_project();
    project.clips[0].volume = serde_json::Number::from_f64(2.5).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("c1: volume 2.5"),
        "message: {}",
        err.message
    );
}

#[test]
fn track_volume_out_of_range_is_rejected() {
    let mut project = base_project();
    project.tracks[0].volume = serde_json::Number::from_f64(2.5).unwrap();
    let err = validate_project(&project).unwrap_err();
    assert!(
        err.message.contains("t1: volume 2.5"),
        "message: {}",
        err.message
    );
}
