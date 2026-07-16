//! Per-vault capture-config IPC: `CaptureConfigDto` plus
//! `get_capture_config`/`set_capture_config`. Split out of the grandfathered
//! `capture_commands.rs` (its shrink-only LOC cap) for headroom — the
//! `vault_config`/`mcp_config`/`document_import_config` split-out precedent.
//! No behavior change: the IPC surface is unchanged, only the defining
//! module moves (`lib.rs`'s `generate_handler!` repoints to here).

use std::path::Path;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths};

use crate::capture_commands::ConfigWriteLock;

pub const BITRATES_KBPS: [u32; 3] = [128, 160, 192];
pub const TRANSCRIPTION_MODELS: [&str; 4] = ["base", "small", "medium", "turbo"];

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfigDto {
    pub mode: String,
    pub meeting_folder: Option<String>,
    pub voice_note_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub transcribe: bool,
    pub transcription_model: String,
    pub transcription_language: Option<String>,
    pub transcript_timestamps: bool,
    /// Free-text vocabulary primed into whisper's initial prompt; None/blank
    /// = no priming.
    pub transcription_vocabulary: Option<String>,
    /// Skip silence via Silero VAD before inference (default on).
    pub transcription_vad: bool,
    pub follow_up_template: bool,
    /// Whether NEW recordings land in a dated `YYYY/MM` subfolder — the
    /// Recording settings surface for `VaultCaptureConfig::recording_date_folders`.
    pub recording_date_folders: bool,
}

impl CaptureConfigDto {
    fn from_config(v: &capture_config::VaultCaptureConfig) -> Self {
        Self {
            mode: v.mode.as_key().to_string(),
            meeting_folder: v.meeting_folder.clone(),
            voice_note_folder: v.voice_note_folder.clone(),
            bitrate_kbps: v.bitrate_kbps,
            create_note: v.create_note,
            input_device: v.input_device.clone(),
            output_device: v.output_device.clone(),
            transcribe: v.transcribe,
            transcription_model: v.transcription_model.clone(),
            transcription_language: v.transcription_language.clone(),
            transcript_timestamps: v.transcript_timestamps,
            transcription_vocabulary: v.transcription_vocabulary.clone(),
            transcription_vad: v.transcription_vad,
            follow_up_template: v.follow_up_template,
            recording_date_folders: v.recording_date_folders,
        }
    }
}

#[tauri::command]
pub fn get_capture_config(id: String) -> CaptureConfigDto {
    // Unknown vaults return the defaults — exactly what a fresh form shows.
    CaptureConfigDto::from_config(&capture_config::vault_config(
        &capture_config::load_config(),
        &id,
    ))
}

/// Trim an optional folder override to `None` when blank, so a
/// whitespace-only field behaves like an absent one (falls back to the
/// mode default) instead of resolving to a literal blank-named folder.
fn clean_folder(raw: &Option<String>) -> Option<String> {
    raw.as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string)
}

#[tauri::command]
pub fn set_capture_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    cfg: CaptureConfigDto,
) -> Result<(), String> {
    let mode = capture_config::RecordingMode::from_key(&cfg.mode)
        .ok_or_else(|| format!("Unknown recording mode: {}", cfg.mode))?;
    if !BITRATES_KBPS.contains(&cfg.bitrate_kbps) {
        return Err(format!("Bitrate must be one of {BITRATES_KBPS:?} kbps"));
    }
    if !TRANSCRIPTION_MODELS.contains(&cfg.transcription_model.as_str()) {
        return Err(format!(
            "Unknown transcription model: {}",
            cfg.transcription_model
        ));
    }
    // Validate BOTH folders against the real vault path BEFORE writing —
    // an invalid folder is an inline field error, nothing gets written.
    let vault = crate::commands::find_vault(&id)?;
    let meeting_folder = clean_folder(&cfg.meeting_folder);
    let voice_note_folder = clean_folder(&cfg.voice_note_folder);
    for folder in [&meeting_folder, &voice_note_folder].into_iter().flatten() {
        capture_paths::safe_recording_root(Path::new(&vault.path), folder)?;
    }
    let _guard = lock_ignoring_poison(&lock.0);
    // Preserve fields CaptureConfigDto doesn't carry (tasks are configured on
    // their own surface) so saving capture settings can't reset them. The read
    // must sit INSIDE the lock: a concurrent set_tasks_config also
    // read-modify-writes this vault, so reading tasks_folder before the guard
    // would let us write back a stale value and clobber its update.
    let existing = capture_config::vault_config(&capture_config::load_config(), &id);
    let value = capture_config::VaultCaptureConfig {
        mode,
        meeting_folder,
        voice_note_folder,
        bitrate_kbps: cfg.bitrate_kbps,
        create_note: cfg.create_note,
        input_device: cfg.input_device.clone().filter(|d| !d.is_empty()),
        output_device: cfg.output_device.clone().filter(|d| !d.is_empty()),
        transcribe: cfg.transcribe,
        transcription_model: cfg.transcription_model.clone(),
        transcription_language: cfg.transcription_language.clone().filter(|l| !l.is_empty()),
        transcript_timestamps: cfg.transcript_timestamps,
        // Blank/whitespace vocabulary collapses to None (no priming), the
        // same treatment transcription_language gets one line up.
        transcription_vocabulary: cfg
            .transcription_vocabulary
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        transcription_vad: cfg.transcription_vad,
        follow_up_template: cfg.follow_up_template,
        tasks_folder: existing.tasks_folder,
        // Preserve the per-vault documents folder (its own set_documents_config
        // command owns it) so saving capture settings can't reset it — same
        // read-inside-the-lock discipline as tasks_folder above.
        documents_folder: existing.documents_folder,
        // Same rule for the lists settings object (set_task_lists_config owns
        // it): a capture save must never reset the default list or the order.
        default_list: existing.default_list,
        list_order: existing.list_order,
        // recording_date_folders is this form's own field now (Vault settings
        // → Recording → Folders) — written from the DTO like every other
        // capture field. document_date_folders belongs to the Documents
        // settings surface (set_documents_config owns it), so it stays
        // preserved from the existing config, same read-inside-the-lock
        // discipline as tasks_folder/documents_folder above.
        recording_date_folders: cfg.recording_date_folders,
        document_date_folders: existing.document_date_folders,
    };
    let result = capture_config::update_vault_config(&id, value.clone());
    if result.is_ok() {
        log::info!(
            "capture config saved for vault {id}: mode={}, meeting={:?}, voice_note={:?}, bitrate={}kbps, note={}, input={:?}, output={:?}, transcribe={}",
            value.mode.as_key(),
            value.meeting_folder,
            value.voice_note_folder,
            value.bitrate_kbps,
            value.create_note,
            value.input_device,
            value.output_device,
            value.transcribe
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcription_models_gate_includes_every_tier_the_ui_offers() {
        // The settings dropdown (TranscriptionSettings.vue MODELS) and this
        // validation gate must agree — a tier the UI offers that this array
        // misses would fail every save with "Unknown transcription model".
        for m in ["base", "small", "medium", "turbo"] {
            assert!(
                TRANSCRIPTION_MODELS.contains(&m),
                "{m} missing from the gate"
            );
        }
    }

    #[test]
    fn clean_folder_blanks_to_none_and_trims_both_folders_the_same_way() {
        // The two-folder validation/construction branch in set_capture_config
        // leans on this helper treating meeting_folder and voice_note_folder
        // identically: whitespace-only collapses to None (mode default),
        // surrounding whitespace is trimmed, and an absent field stays absent.
        assert_eq!(clean_folder(&None), None);
        assert_eq!(clean_folder(&Some("   ".to_string())), None);
        assert_eq!(
            clean_folder(&Some("  Meetings/2026  ".to_string())),
            Some("Meetings/2026".to_string())
        );
        assert_eq!(
            clean_folder(&Some("Voice".to_string())),
            Some("Voice".to_string())
        );
    }
}
