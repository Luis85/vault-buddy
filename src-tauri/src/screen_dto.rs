//! The screen-capture domain's WIRE types: the shapes that cross IPC to the
//! webview.
//!
//! Its own module for the reason `src/screenTypes.ts` is the frontend's —
//! the same split, on the same feature, for the same cause.
//! `screen_commands.rs` is the capture LIFECYCLE (the commands, the
//! reservation, the teardown chokepoint) and reached this repo's 800-line
//! Rust cap, which is shrink-only; these are the shapes that lifecycle
//! *reports*, and they answer to the webview rather than to any of it.
//!
//! Every type here is `rename_all = "camelCase"` and is tested to be,
//! because a Rust field name arriving at the frontend is a silently
//! `undefined` property rather than a compile error — the GAP-135 class.

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenStatusPayload {
    pub capturing: bool,
    pub vault_id: Option<String>,
    pub started_at_ms: Option<u64>,
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    pub source_title: Option<String>,
}

impl ScreenStatusPayload {
    /// Every field cleared. A reloaded webview re-reads the status; leaking
    /// the last capture's vault id or start time would render a phantom
    /// capture bar counting up from a recording that already ended.
    pub fn idle() -> ScreenStatusPayload {
        ScreenStatusPayload {
            capturing: false,
            vault_id: None,
            started_at_ms: None,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            source_title: None,
        }
    }
}

/// What the editor/capture-bar needs about a just-finished capture. `path`
/// is the staged `.mp4` inside the (outside-every-vault) staging directory —
/// nothing here is a vault write yet.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedCaptureDto {
    pub base: String,
    pub path: String,
    pub duration_ms: u64,
    pub source_title: String,
    pub width: u32,
    pub height: u32,
}

/// Wire result for `stop_screen_capture`, mirroring `StopOutcomeDto`:
/// `still_saving` = the bounded wait expired while finalize was still
/// running, so the frontend keeps its saving UI and lets `screen:stopped` /
/// `screen:failed` finish the story instead of reporting a false success.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenStopOutcomeDto {
    pub still_saving: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_payload_serializes_camel_case_for_the_webview() {
        let payload = ScreenStatusPayload {
            capturing: true,
            vault_id: Some("v1".into()),
            started_at_ms: Some(1_700_000_000_000),
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            source_title: Some("Screen 1".into()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"startedAtMs\""), "got {json}");
        assert!(json.contains("\"sourceTitle\""), "got {json}");
        assert!(!json.contains("started_at_ms"), "got {json}");
    }

    #[test]
    fn an_idle_status_payload_reports_nothing_rather_than_stale_values() {
        // A reloaded webview re-reads this. Leaking the last capture's vault
        // id or start time into an idle payload would render a phantom
        // capture bar counting up from a recording that ended.
        // EVERY field, not a sample: `paused: true` with a stale
        // `paused_since_ms` is the worst of the phantoms — it renders a
        // PAUSED capture bar counting from a timestamp that belongs to a
        // recording that already ended, and a sampled assertion leaves that
        // exact pair unpinned.
        let p = ScreenStatusPayload::idle();
        assert!(!p.capturing);
        assert_eq!(p.vault_id, None);
        assert_eq!(p.started_at_ms, None);
        assert!(!p.paused);
        assert_eq!(p.paused_total_ms, 0);
        assert_eq!(p.paused_since_ms, None);
        assert_eq!(p.source_title, None);
    }
}
