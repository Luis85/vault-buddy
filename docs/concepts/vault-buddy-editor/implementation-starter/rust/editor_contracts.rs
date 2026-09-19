//! Proposed native editor DTOs and pure timebase helpers.
//! Integrate into an existing appropriate module; not a complete editor service.
//! No Tauri commands are registered here. Cargo execution was not available in the handover environment.
use serde::{Deserialize, Serialize};

pub const MAX_JS_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EditorSnapshot {
    pub session_id: String,
    pub project_id: String,
    pub revision: u64,
    pub persisted_revision: Option<u64>,
    pub title: String,
    pub duration_ms: u64,
    pub can_undo: bool,
    pub can_redo: bool,
}
impl EditorSnapshot {
    /// Mirrors the small TS summary boundary, not full render-graph validation.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.session_id.trim().is_empty() || self.session_id.encode_utf16().count() > 100
            || self.project_id.trim().is_empty() || self.project_id.encode_utf16().count() > 100
        { return Err("invalid identity"); }
        if self.revision > MAX_JS_INTEGER || self.persisted_revision.is_some_and(|r| r > self.revision)
        { return Err("invalid revision"); }
        if self.duration_ms > 7_200_000 { return Err("duration exceeds two-hour reference safety bound"); }
        if self.title.trim().is_empty() || self.title.encode_utf16().count() > 160 { return Err("invalid title"); }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum EditorCommand {
    Rename { title: String },
    SplitClip {
        #[serde(rename = "clipId")] clip_id: String,
        #[serde(rename = "atMs")] at_ms: u64,
    },
    DeleteSelection {
        #[serde(rename = "clipIds")] clip_ids: Vec<String>,
        #[serde(rename = "closeGap")] close_gap: bool,
    },
    Undo,
    Redo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteRequest {
    pub session_id: String,
    pub expected_revision: u64,
    pub command_id: String,
    pub command: EditorCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveReceipt {
    pub session_id: String,
    pub saved_revision: u64,
    pub project_file_id: String,
}

/// 100-nanosecond frame timestamps from a rational frame rate.
/// Use checked wide arithmetic. JSON transport must never narrow this blindly.
pub fn frame_timestamp(frame: u64, fps_numerator: u64, fps_denominator: u64) -> Result<u64, &'static str> {
    if fps_numerator == 0 || fps_denominator == 0 { return Err("invalid frame rate"); }
    let ticks = u128::from(frame)
        .checked_mul(10_000_000).and_then(|v| v.checked_mul(u128::from(fps_denominator)))
        .ok_or("timestamp overflow")? / u128::from(fps_numerator);
    u64::try_from(ticks).map_err(|_| "timestamp overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn timestamps_do_not_accumulate_rounded_frame_duration() {
        assert_eq!(frame_timestamp(1, 30, 1), Ok(333_333));
        assert_eq!(frame_timestamp(30, 30, 1), Ok(10_000_000));
        assert_eq!(frame_timestamp(30_000, 30_000, 1001), Ok(10_010_000_000));
    }
    #[test] fn rejects_invalid_and_overflowing_rates() {
        assert!(frame_timestamp(1, 0, 1).is_err());
        assert!(frame_timestamp(u64::MAX, 1, u64::MAX).is_err());
    }
    #[test] fn command_wire_names_match_typescript() {
        let request = EditorCommand::SplitClip { clip_id: "clip-a".into(), at_ms: 1200 };
        let json = serde_json::to_value(request).expect("serialize command");
        assert_eq!(json, serde_json::json!({"kind":"splitClip", "clipId":"clip-a", "atMs":1200}));
    }
}
