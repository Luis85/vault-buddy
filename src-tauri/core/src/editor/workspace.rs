//! Sanitizer for the saved `workspace` view-preference blob (R16;
//! `workspace.schema.json`: "View preferences are sanitized separately and
//! cannot grant source access"). `sanitize` never fails: it reads a raw
//! `serde_json::Value` field by field, keeping a recognized key only when
//! its value has the right shape — the same defensive-read posture the
//! rest of the vault domain uses for a hand-edited or stale config file —
//! clamps the numeric fields the ADR names, and filters the selection list
//! through `is_valid_id`. There is no `extra`/flatten field: an unknown key
//! is simply never read, so it is dropped rather than round-tripped (this
//! is presentation state, not the interchange document R3 protects).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ids::is_valid_id;
use super::limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeleteMode {
    Gap,
    Close,
}

/// The editor window's own light/dark preference (F16; Task 18). Kept here
/// rather than in `core::editor::model` because it is view state, not part
/// of the project graph — the same reason the whole `workspace.json`
/// sidecar exists as a document `editor_save_project` embeds read-only
/// rather than a field on `Project` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
}

/// The workspace's "what is selected right now" pointer — a kind tag plus
/// an entity id (`{"type":"clip","id":"presenter1"}` in the reference
/// fixture). Kept as its own small struct, not an opaque `Value`, so a
/// malformed shape (missing `id`, non-string `type`) is caught once here
/// rather than by every later reader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Selected {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

/// The 18 workspace preference fields (R16), every one optional. A field
/// that is absent, wrong-typed or out of range is dropped to `None` rather
/// than failing the whole read, so a hand-edited or stale `workspace.json`
/// degrades gracefully instead of blocking the project from opening.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_clip_ids: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected: Option<Selected>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playhead_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_tab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub property_tab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_zoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_scroll_left: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_scroll_top: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snap: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_mode: Option<DeleteMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor_muted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub playback_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties_hidden: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub properties_open: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_preview: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption_settings_open: Option<bool>,
    /// F16: the editor header's theme toggle. Without this field `sanitize`
    /// drops `theme` as an unrecognized key (the module doc's "an unknown
    /// key is simply never read"), so the toggle in `EditorHeader` (Task 16)
    /// would never survive a reopen — the exact gap Task 18 closes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

fn bool_field(v: &Value, key: &str) -> Option<bool> {
    v.get(key)?.as_bool()
}

fn finite_f64_field(v: &Value, key: &str) -> Option<f64> {
    let n = v.get(key)?.as_f64()?;
    n.is_finite().then_some(n)
}

fn clamped_f64_field(v: &Value, key: &str, lo: f64, hi: f64) -> Option<f64> {
    finite_f64_field(v, key).map(|n| n.clamp(lo, hi))
}

fn selection_clip_ids_field(v: &Value) -> Option<Vec<String>> {
    let arr = v.get("selection_clip_ids")?.as_array()?;
    Some(
        arr.iter()
            .filter_map(Value::as_str)
            .filter(|id| is_valid_id(id))
            .take(limits::MAX_CLIPS)
            .map(str::to_string)
            .collect(),
    )
}

fn selected_field(v: &Value) -> Option<Selected> {
    let obj = v.get("selected")?.as_object()?;
    let kind = obj.get("type")?.as_str()?.to_string();
    let id = obj.get("id")?.as_str()?;
    if !is_valid_id(id) {
        return None;
    }
    Some(Selected {
        kind,
        id: id.to_string(),
    })
}

fn delete_mode_field(v: &Value) -> Option<DeleteMode> {
    match v.get("delete_mode")?.as_str()? {
        "gap" => Some(DeleteMode::Gap),
        "close" => Some(DeleteMode::Close),
        _ => None,
    }
}

fn theme_field(v: &Value) -> Option<Theme> {
    match v.get("theme")?.as_str()? {
        "dark" => Some(Theme::Dark),
        "light" => Some(Theme::Light),
        _ => None,
    }
}

/// Reads and sanitizes the workspace preference blob. Every field is
/// looked up independently, so one wrong-typed or unknown key never
/// affects any other — a hostile or merely stale document can shrink what
/// comes back but can never make `sanitize` fail or panic.
pub fn sanitize(v: &Value) -> Workspace {
    Workspace {
        selection_clip_ids: selection_clip_ids_field(v),
        selected: selected_field(v),
        playhead_ms: v.get("playhead_ms").and_then(Value::as_u64),
        library_tab: str_field(v, "library_tab"),
        property_tab: str_field(v, "property_tab"),
        timeline_zoom: clamped_f64_field(v, "timeline_zoom", 0.1, 20.0),
        timeline_height: clamped_f64_field(v, "timeline_height", 160.0, 900.0),
        timeline_scroll_left: finite_f64_field(v, "timeline_scroll_left"),
        timeline_scroll_top: finite_f64_field(v, "timeline_scroll_top"),
        snap: bool_field(v, "snap"),
        delete_mode: delete_mode_field(v),
        monitor_muted: bool_field(v, "monitor_muted"),
        playback_rate: clamped_f64_field(v, "playback_rate", 0.25, 2.0),
        library_hidden: bool_field(v, "library_hidden"),
        properties_hidden: bool_field(v, "properties_hidden"),
        properties_open: bool_field(v, "properties_open"),
        focus_preview: bool_field(v, "focus_preview"),
        caption_settings_open: bool_field(v, "caption_settings_open"),
        theme: theme_field(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_drops_unknown_and_mistyped_fields() {
        let v = serde_json::json!({
            "bogus_future_field": 123,
            "snap": "not-a-bool",
            "timeline_zoom": "5",
            "library_tab": "media",
            "playhead_ms": 13400,
        });
        let ws = sanitize(&v);
        assert_eq!(ws.snap, None, "wrong-typed snap must be dropped");
        assert_eq!(
            ws.timeline_zoom, None,
            "wrong-typed timeline_zoom must be dropped"
        );
        assert_eq!(ws.library_tab, Some("media".to_string()));
        assert_eq!(ws.playhead_ms, Some(13400));
    }

    #[test]
    fn sanitize_clamps_zoom_and_height() {
        let v = serde_json::json!({
            "timeline_zoom": 99,
            "timeline_height": 5,
            "playback_rate": 100,
        });
        let ws = sanitize(&v);
        assert_eq!(ws.timeline_zoom, Some(20.0));
        assert_eq!(ws.timeline_height, Some(160.0));
        assert_eq!(ws.playback_rate, Some(2.0));
    }

    #[test]
    fn sanitize_filters_invalid_selection_ids() {
        let v = serde_json::json!({
            "selection_clip_ids": ["ok-1", "bad id", "", "another_ok", 42],
        });
        let ws = sanitize(&v);
        assert_eq!(
            ws.selection_clip_ids,
            Some(vec!["ok-1".to_string(), "another_ok".to_string()])
        );
    }

    #[test]
    fn sanitize_caps_selection_ids_at_max_clips() {
        let ids: Vec<String> = (0..(limits::MAX_CLIPS + 50))
            .map(|i| format!("clip-{i}"))
            .collect();
        let v = serde_json::json!({ "selection_clip_ids": ids });
        let ws = sanitize(&v);
        assert_eq!(ws.selection_clip_ids.unwrap().len(), limits::MAX_CLIPS);
    }

    // F16: without the `theme` field on `Workspace`, `sanitize` drops it as
    // an unrecognized key (the module doc's own "an unknown key is simply
    // never read") and the header's theme toggle (Task 16) would never
    // survive a reopen. `"dark"`/`"light"` must survive; anything else
    // (including a plausible-looking third theme) must drop to `None`
    // rather than being accepted as a free-form string.
    #[test]
    fn sanitize_keeps_theme_and_rejects_other_values() {
        assert_eq!(
            sanitize(&serde_json::json!({ "theme": "dark" })).theme,
            Some(Theme::Dark)
        );
        assert_eq!(
            sanitize(&serde_json::json!({ "theme": "light" })).theme,
            Some(Theme::Light)
        );
        assert_eq!(
            sanitize(&serde_json::json!({ "theme": "purple" })).theme,
            None,
            "an unrecognized theme must be dropped, not passed through"
        );
        assert_eq!(
            sanitize(&serde_json::json!({})).theme,
            None,
            "an absent theme must stay absent"
        );
    }

    #[test]
    fn sanitize_drops_unknown_delete_mode_and_reads_known_ones() {
        let v = serde_json::json!({ "delete_mode": "ripple" });
        assert_eq!(sanitize(&v).delete_mode, None);

        let v = serde_json::json!({ "delete_mode": "gap" });
        assert_eq!(sanitize(&v).delete_mode, Some(DeleteMode::Gap));

        let v = serde_json::json!({ "delete_mode": "close" });
        assert_eq!(sanitize(&v).delete_mode, Some(DeleteMode::Close));
    }
}
