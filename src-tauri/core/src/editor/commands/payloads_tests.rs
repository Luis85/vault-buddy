//! Tests for `payloads.rs`'s `AddEffectPayload`/`EffectProps` kind-driven
//! decode (controller ruling, Task 6 fix round 1), split into this sibling
//! file (`#[path]`, the `validate.rs`/`validate_tests.rs` precedent) so
//! `payloads.rs` itself stays well under the 800-nonblank-line Rust cap.

use super::*;

#[test]
fn add_effect_payload_round_trips_text_props() {
    let json = serde_json::json!({
        "clipId": "c1",
        "effectKind": "text",
        "startMs": 0,
        "endMs": 500,
        "props": {
            "x": 0.1,
            "y": 0.2,
            "text": "Hello",
            "fontSize": 24,
            "color": "#ffffff",
            "background": true,
        },
    });
    let payload: AddEffectPayload =
        serde_json::from_value(json.clone()).expect("a well-formed text effect must decode");
    assert_eq!(payload.clip_id, "c1");
    assert_eq!(payload.kind, EffectKind::Text);
    assert_eq!(
        payload.props,
        EffectProps::Text(TextEffectProps {
            x: Some(Num::from_f64(0.1).unwrap()),
            y: Some(Num::from_f64(0.2).unwrap()),
            w: None,
            h: None,
            text: Some("Hello".to_string()),
            font_size: Some(Num::from(24)),
            color: Some("#ffffff".to_string()),
            background: Some(true),
        })
    );
    assert_eq!(
        serde_json::to_value(&payload).unwrap(),
        json,
        "must round-trip byte-for-byte"
    );
}

#[test]
fn add_effect_payload_round_trips_arrow_props() {
    let json = serde_json::json!({
        "clipId": "c1",
        "effectKind": "arrow",
        "startMs": 0,
        "endMs": 500,
        "props": {
            "x": 0.1,
            "y": 0.2,
            "x2": 0.5,
            "y2": 0.6,
            "color": "#ff0000",
            "stroke": 3,
        },
    });
    let payload: AddEffectPayload =
        serde_json::from_value(json.clone()).expect("a well-formed arrow effect must decode");
    assert_eq!(payload.kind, EffectKind::Arrow);
    assert_eq!(
        payload.props,
        EffectProps::Arrow(ArrowEffectProps {
            x: Some(Num::from_f64(0.1).unwrap()),
            y: Some(Num::from_f64(0.2).unwrap()),
            x2: Some(Num::from_f64(0.5).unwrap()),
            y2: Some(Num::from_f64(0.6).unwrap()),
            color: Some("#ff0000".to_string()),
            stroke: Some(Num::from(3)),
        })
    );
    assert_eq!(serde_json::to_value(&payload).unwrap(), json);
}

#[test]
fn add_effect_payload_rejects_an_unknown_field_for_its_kind() {
    // fontSize belongs to `text`, not `arrow` -- ArrowEffectProps' own
    // deny_unknown_fields must reject it rather than silently drop it.
    let json = serde_json::json!({
        "clipId": "c1",
        "effectKind": "arrow",
        "startMs": 0,
        "endMs": 500,
        "props": { "x": 0.1, "y": 0.2, "fontSize": 24 },
    });
    assert!(serde_json::from_value::<AddEffectPayload>(json).is_err());
}

#[test]
fn add_effect_payload_rejects_props_mismatched_with_its_kind() {
    // `text` belongs to the `text`/`step` kinds, not `spotlight`.
    let json = serde_json::json!({
        "clipId": "c1",
        "effectKind": "spotlight",
        "startMs": 0,
        "endMs": 500,
        "props": { "x": 0.1, "y": 0.2, "text": "nope" },
    });
    assert!(serde_json::from_value::<AddEffectPayload>(json).is_err());
}

#[test]
fn effect_props_from_kind_and_value_dispatches_every_kind() {
    for kind in [
        EffectKind::Text,
        EffectKind::Arrow,
        EffectKind::Highlight,
        EffectKind::Spotlight,
        EffectKind::Zoom,
        EffectKind::Step,
        EffectKind::Mask,
    ] {
        EffectProps::from_kind_and_value(kind, serde_json::json!({})).unwrap_or_else(|e| {
            panic!("{kind:?}: an empty object must decode (every field is Option): {e}")
        });
    }
}
