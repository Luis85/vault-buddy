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
pub const TRANSCRIPTION_MODELS: [&str; 3] = ["base", "small", "medium"];

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfigDto {
    pub mode: String,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub transcribe: bool,
    pub transcription_model: String,
    pub transcription_language: Option<String>,
    pub transcript_timestamps: bool,
    pub follow_up_template: bool,
}

impl CaptureConfigDto {
    fn from_config(v: &capture_config::VaultCaptureConfig) -> Self {
        Self {
            mode: v.mode.as_key().to_string(),
            recording_folder: v.recording_folder.clone(),
            bitrate_kbps: v.bitrate_kbps,
            create_note: v.create_note,
            input_device: v.input_device.clone(),
            output_device: v.output_device.clone(),
            transcribe: v.transcribe,
            transcription_model: v.transcription_model.clone(),
            transcription_language: v.transcription_language.clone(),
            transcript_timestamps: v.transcript_timestamps,
            follow_up_template: v.follow_up_template,
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
    // Validate the folder against the real vault path BEFORE writing —
    // an invalid folder is an inline field error, nothing gets written.
    let vault = crate::commands::find_vault(&id)?;
    let folder = cfg
        .recording_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    if let Some(folder) = &folder {
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
        recording_folder: folder,
        bitrate_kbps: cfg.bitrate_kbps,
        create_note: cfg.create_note,
        input_device: cfg.input_device.clone().filter(|d| !d.is_empty()),
        output_device: cfg.output_device.clone().filter(|d| !d.is_empty()),
        transcribe: cfg.transcribe,
        transcription_model: cfg.transcription_model.clone(),
        transcription_language: cfg.transcription_language.clone().filter(|l| !l.is_empty()),
        transcript_timestamps: cfg.transcript_timestamps,
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
    };
    let result = capture_config::update_vault_config(&id, value.clone());
    if result.is_ok() {
        log::info!(
            "capture config saved for vault {id}: mode={}, folder={:?}, bitrate={}kbps, note={}, input={:?}, output={:?}, transcribe={}",
            value.mode.as_key(),
            value.recording_folder,
            value.bitrate_kbps,
            value.create_note,
            value.input_device,
            value.output_device,
            value.transcribe
        );
    }
    result
}
