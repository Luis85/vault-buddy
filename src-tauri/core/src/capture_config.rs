//! Per-vault capture settings. App-side (%APPDATA%\vault-buddy\config.json),
//! keyed by Obsidian vault ID — never written into user vaults. Read by the
//! recording path and written by the settings UI (set_capture_config), and
//! still hand-editable — parsing must shrug off any malformed input and
//! fall back to defaults.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingMode {
    #[default]
    Meeting,
    VoiceNote,
}

impl RecordingMode {
    pub fn label(&self) -> &'static str {
        match self {
            RecordingMode::Meeting => "Meeting",
            RecordingMode::VoiceNote => "Voice Note",
        }
    }

    /// Desktop-audio (loopback) capture is part of this mode. Exhaustive
    /// match: a new mode variant must decide this explicitly.
    pub fn uses_loopback(&self) -> bool {
        match self {
            RecordingMode::Meeting => true,
            RecordingMode::VoiceNote => false,
        }
    }

    /// Stable key used in config.json and the IPC DTOs.
    pub fn as_key(&self) -> &'static str {
        match self {
            RecordingMode::Meeting => "meeting",
            RecordingMode::VoiceNote => "voice-note",
        }
    }

    pub fn from_key(key: &str) -> Option<RecordingMode> {
        match key {
            "meeting" => Some(RecordingMode::Meeting),
            "voice-note" => Some(RecordingMode::VoiceNote),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VaultCaptureConfig {
    pub mode: RecordingMode,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    /// cpal device names; None = system default. A configured device
    /// missing at record time falls back to the default with a warning —
    /// stale config never blocks recording.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub transcribe: bool,
    pub transcription_model: String,
    pub transcription_language: Option<String>,
    pub transcript_timestamps: bool,
    pub follow_up_template: bool,
    /// Vault-relative folder holding this vault's task documents.
    /// None → the default "Tasks".
    pub tasks_folder: Option<String>,
}

impl Default for VaultCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Meeting,
            recording_folder: None,
            bitrate_kbps: 128,
            create_note: true,
            input_device: None,
            output_device: None,
            transcribe: false,
            transcription_model: "small".to_string(),
            transcription_language: None,
            transcript_timestamps: true,
            follow_up_template: true,
            tasks_folder: None,
        }
    }
}

impl VaultCaptureConfig {
    /// Configured folder, or the mode's default (the PRD gives meetings
    /// and voice notes distinct homes).
    pub fn effective_recording_folder(&self) -> &str {
        match &self.recording_folder {
            Some(folder) => folder,
            None => match self.mode {
                RecordingMode::Meeting => "Meetings",
                RecordingMode::VoiceNote => "Voice Notes",
            },
        }
    }

    /// Folders that may hold this vault's recordings, for scans that must see
    /// EVERY past recording (the Recordings list, recovery, transcription
    /// backfill). A configured custom folder holds them all; without one,
    /// meetings and voice notes live in their two distinct default homes and
    /// the mode may have changed over the vault's life, so scan both. This is
    /// the union of `effective_recording_folder`'s branches.
    pub fn recording_roots(&self) -> Vec<&str> {
        match &self.recording_folder {
            Some(folder) => vec![folder.as_str()],
            None => vec!["Meetings", "Voice Notes"],
        }
    }

    /// The vault-relative folder holding this vault's task documents.
    pub fn tasks_root(&self) -> &str {
        self.tasks_folder.as_deref().unwrap_or("Tasks")
    }
}

/// Default port for the embedded MCP server: 0x5642 = ASCII "VB".
pub const DEFAULT_MCP_PORT: u16 = 22082;

/// App-global settings for the embedded MCP server (spec:
/// docs/superpowers/specs/2026-07-09-local-mcp-server-design.md). Stored as
/// a top-level `mcp` section beside `vaults`; parsing is per-field defensive
/// for the same reason the vault entries are.
#[derive(Debug, Clone, PartialEq)]
pub struct McpConfig {
    pub enabled: bool,
    pub port: u16,
    /// Bearer token clients must send. Empty until first enable; the shell
    /// self-heals an enabled-but-tokenless config by generating one.
    pub token: String,
    /// The "Allow vault writes" grant: add_task, set_task_status, and the
    /// daily-note create branch.
    pub allow_writes: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_MCP_PORT,
            token: String::new(),
            allow_writes: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub vaults: HashMap<String, VaultCaptureConfig>,
    pub mcp: McpConfig,
}

/// Per-field parsing through serde_json::Value: the file is hand-edited,
/// and one malformed value must default only itself — a derived
/// deserializer would reject the whole file, silently flipping every
/// vault back to meeting mode (and thus desktop-audio capture).
pub fn parse_config(json: &str) -> AppConfig {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return AppConfig::default();
    };
    let mut vaults = HashMap::new();
    if let Some(map) = value.get("vaults").and_then(|v| v.as_object()) {
        for (id, entry) in map {
            vaults.insert(id.clone(), vault_entry(entry));
        }
    }
    let mcp = value.get("mcp").map(mcp_entry).unwrap_or_default();
    AppConfig { vaults, mcp }
}

fn vault_entry(entry: &serde_json::Value) -> VaultCaptureConfig {
    let defaults = VaultCaptureConfig::default();
    VaultCaptureConfig {
        mode: entry
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(RecordingMode::from_key)
            .unwrap_or(defaults.mode),
        recording_folder: entry
            .get("recordingFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        bitrate_kbps: entry
            .get("bitrateKbps")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(defaults.bitrate_kbps),
        create_note: entry
            .get("createNote")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.create_note),
        input_device: entry
            .get("inputDevice")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        output_device: entry
            .get("outputDevice")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        transcribe: entry
            .get("transcribe")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.transcribe),
        transcription_model: entry
            .get("transcriptionModel")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| defaults.transcription_model.clone()),
        transcription_language: entry
            .get("transcriptionLanguage")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        transcript_timestamps: entry
            .get("transcriptTimestamps")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.transcript_timestamps),
        follow_up_template: entry
            .get("followUpTemplate")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.follow_up_template),
        tasks_folder: entry
            .get("tasksFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

fn mcp_entry(entry: &serde_json::Value) -> McpConfig {
    let defaults = McpConfig::default();
    McpConfig {
        enabled: entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.enabled),
        port: entry
            .get("port")
            .and_then(|v| v.as_u64())
            .and_then(|v| u16::try_from(v).ok())
            .unwrap_or(defaults.port),
        token: entry
            .get("token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_default(),
        allow_writes: entry
            .get("allowWrites")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.allow_writes),
    }
}

pub fn vault_config(cfg: &AppConfig, vault_id: &str) -> VaultCaptureConfig {
    cfg.vaults.get(vault_id).cloned().unwrap_or_default()
}

/// The app's own data directory: `<config_dir>/vault-buddy`
/// (`%APPDATA%\vault-buddy` on Windows). Single source of truth for the
/// app's top-level AppData folder so `config.json` (here) and the
/// transcription model cache (`transcribe` crate's `model_dir`) always share
/// ONE folder instead of each hardcoding the name and risking a second one.
pub fn app_config_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vault-buddy"))
}

pub fn config_path() -> Option<PathBuf> {
    app_config_dir().map(|d| d.join("config.json"))
}

pub fn load_config_from(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    }
}

pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    load_config_from(&path)
}

/// Serialize to the same schema `parse_config` reads. Vault ids are
/// sorted and optional fields omitted so the hand-editable file stays
/// stable and minimal across saves. The mcp section is included only
/// when non-default, so users who never enable MCP never see it.
pub fn serialize_config(cfg: &AppConfig) -> String {
    use serde_json::{json, Map, Value};
    let mut root = Map::new();
    if cfg.mcp != McpConfig::default() {
        let mut mcp = Map::new();
        mcp.insert("enabled".to_string(), json!(cfg.mcp.enabled));
        mcp.insert("port".to_string(), json!(cfg.mcp.port));
        mcp.insert("token".to_string(), json!(cfg.mcp.token));
        mcp.insert("allowWrites".to_string(), json!(cfg.mcp.allow_writes));
        root.insert("mcp".to_string(), Value::Object(mcp));
    }
    let mut vaults = Map::new();
    let mut ids: Vec<&String> = cfg.vaults.keys().collect();
    ids.sort();
    for id in ids {
        let v = &cfg.vaults[id];
        let mut entry = Map::new();
        entry.insert("mode".to_string(), json!(v.mode.as_key()));
        if let Some(folder) = &v.recording_folder {
            entry.insert("recordingFolder".to_string(), json!(folder));
        }
        entry.insert("bitrateKbps".to_string(), json!(v.bitrate_kbps));
        entry.insert("createNote".to_string(), json!(v.create_note));
        if let Some(device) = &v.input_device {
            entry.insert("inputDevice".to_string(), json!(device));
        }
        if let Some(device) = &v.output_device {
            entry.insert("outputDevice".to_string(), json!(device));
        }
        entry.insert("transcribe".to_string(), json!(v.transcribe));
        entry.insert(
            "transcriptionModel".to_string(),
            json!(v.transcription_model),
        );
        if let Some(language) = &v.transcription_language {
            entry.insert("transcriptionLanguage".to_string(), json!(language));
        }
        entry.insert(
            "transcriptTimestamps".to_string(),
            json!(v.transcript_timestamps),
        );
        entry.insert("followUpTemplate".to_string(), json!(v.follow_up_template));
        if let Some(folder) = &v.tasks_folder {
            entry.insert("tasksFolder".to_string(), json!(folder));
        }
        vaults.insert(id.clone(), Value::Object(entry));
    }
    root.insert("vaults".to_string(), Value::Object(vaults));
    let mut out =
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
}

/// Atomic write via owned temp + rename. The REPLACING std::fs::rename is
/// correct here — config.json is our own app-side file, never a vault
/// file, and replacing the previous version is exactly the intent (the
/// capture domain's rename_noreplace rule protects vault files, not this).
pub fn write_config(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string());
    let tmp = dir.join(format!(".{file_name}.vault-buddy.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(serialize_config(cfg).as_bytes())?;
        f.sync_all()?;
    }
    let result = std::fs::rename(&tmp, path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Read-modify-write so saving one vault preserves the others. No lock of
/// its own: callers that can race (the IPC command layer) must serialize
/// calls behind a mutex.
pub fn update_vault_config_at(
    path: &Path,
    vault_id: &str,
    v: VaultCaptureConfig,
) -> std::io::Result<()> {
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    };
    cfg.vaults.insert(vault_id.to_string(), v);
    write_config(path, &cfg)
}

pub fn update_vault_config(vault_id: &str, v: VaultCaptureConfig) -> Result<(), String> {
    let path = config_path().ok_or("Cannot resolve the config directory")?;
    update_vault_config_at(&path, vault_id, v)
        .map_err(|e| format!("Could not save capture settings: {e}"))
}

/// Read-modify-write for the app-global mcp section, mirroring
/// update_vault_config_at (same no-own-lock rule: IPC callers serialize
/// behind ConfigWriteLock).
pub fn update_mcp_config_at(path: &Path, mcp: McpConfig) -> std::io::Result<()> {
    let mut cfg = load_config_from(path);
    cfg.mcp = mcp;
    write_config(path, &cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_nests_in_the_shared_app_config_dir() {
        // config.json and the transcription model cache (transcribe crate)
        // both derive from app_config_dir(), so the app keeps ONE top-level
        // AppData folder — never a second one. Asserting the derivation here
        // keeps the co-location structural, not coincidental.
        if let (Some(cfg), Some(app)) = (config_path(), app_config_dir()) {
            assert_eq!(cfg.parent(), Some(app.as_path()));
            assert_eq!(cfg.file_name().unwrap(), "config.json");
            assert_eq!(app.file_name().unwrap(), "vault-buddy");
        }
    }

    #[test]
    fn defaults_when_json_is_garbage() {
        let cfg = parse_config("not json at all");
        let v = vault_config(&cfg, "any-id");
        assert_eq!(v.mode, RecordingMode::Meeting);
        assert_eq!(v.effective_recording_folder(), "Meetings");
        assert_eq!(v.bitrate_kbps, 128);
        assert!(v.create_note);
    }

    #[test]
    fn folder_defaults_follow_the_mode_but_config_overrides() {
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": { "mode": "voice-note" },
                "b": { "mode": "voice-note", "recordingFolder": "Inbox" }
            } }"#,
        );
        assert_eq!(
            vault_config(&cfg, "a").effective_recording_folder(),
            "Voice Notes"
        );
        assert_eq!(
            vault_config(&cfg, "b").effective_recording_folder(),
            "Inbox"
        );
    }

    #[test]
    fn defaults_for_unknown_vault() {
        let cfg = parse_config(r#"{ "vaults": {} }"#);
        assert_eq!(vault_config(&cfg, "missing"), VaultCaptureConfig::default());
    }

    #[test]
    fn partial_entry_fills_missing_fields_with_defaults() {
        let cfg = parse_config(
            r#"{ "vaults": { "abc": { "mode": "voice-note", "createNote": false } } }"#,
        );
        let v = vault_config(&cfg, "abc");
        assert_eq!(v.mode, RecordingMode::VoiceNote);
        assert!(!v.create_note);
        assert_eq!(v.recording_folder, None);
        assert_eq!(v.bitrate_kbps, 128);
    }

    #[test]
    fn unknown_mode_string_falls_back_to_meeting() {
        let cfg = parse_config(r#"{ "vaults": { "abc": { "mode": "karaoke" } } }"#);
        assert_eq!(vault_config(&cfg, "abc").mode, RecordingMode::Meeting);
    }

    #[test]
    fn malformed_field_defaults_locally_not_globally() {
        // One bad field must not drop the entry (or the whole file):
        // a voice-note vault must never silently flip back to meeting
        // mode (which would open loopback) because bitrate was quoted.
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": { "mode": "voice-note", "bitrateKbps": "160" },
                "b": { "createNote": false }
            } }"#,
        );
        let a = vault_config(&cfg, "a");
        assert_eq!(a.mode, RecordingMode::VoiceNote);
        assert_eq!(a.bitrate_kbps, 128);
        assert!(!vault_config(&cfg, "b").create_note);
    }

    #[test]
    fn mode_labels() {
        assert_eq!(RecordingMode::Meeting.label(), "Meeting");
        assert_eq!(RecordingMode::VoiceNote.label(), "Voice Note");
    }

    #[test]
    fn loopback_is_an_explicit_per_mode_decision() {
        assert!(RecordingMode::Meeting.uses_loopback());
        assert!(!RecordingMode::VoiceNote.uses_loopback());
    }

    #[test]
    fn transcription_defaults_are_opt_in_small_timestamped() {
        let v = vault_config(&parse_config("{}"), "any");
        assert!(!v.transcribe, "opt-in: off by default");
        assert_eq!(v.transcription_model, "small");
        assert_eq!(v.transcription_language, None);
        assert!(v.transcript_timestamps);
    }

    #[test]
    fn transcription_fields_parse() {
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "transcribe": true,
                "transcriptionModel": "medium",
                "transcriptionLanguage": "es",
                "transcriptTimestamps": false
            } } }"#,
        );
        let v = vault_config(&cfg, "a");
        assert!(v.transcribe);
        assert_eq!(v.transcription_model, "medium");
        assert_eq!(v.transcription_language.as_deref(), Some("es"));
        assert!(!v.transcript_timestamps);
    }

    #[test]
    fn malformed_transcribe_defaults_locally() {
        // A quoted bool must not enable transcription, nor drop the entry.
        let cfg =
            parse_config(r#"{ "vaults": { "a": { "transcribe": "yes", "mode": "voice-note" } } }"#);
        let v = vault_config(&cfg, "a");
        assert!(!v.transcribe);
        assert_eq!(v.mode, RecordingMode::VoiceNote);
    }

    #[test]
    fn recording_roots_are_the_custom_folder_or_both_defaults() {
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": { "mode": "voice-note" },
                "b": { "recordingFolder": "Inbox" }
            } }"#,
        );
        // No custom folder → scan both mode homes (mode may have changed).
        assert_eq!(
            vault_config(&cfg, "a").recording_roots(),
            vec!["Meetings", "Voice Notes"]
        );
        // Custom folder → it holds every recording, scan just it.
        assert_eq!(vault_config(&cfg, "b").recording_roots(), vec!["Inbox"]);
    }

    #[test]
    fn device_fields_parse_and_default_to_none() {
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "inputDevice": "Headset Mic",
                "outputDevice": "Speakers"
            } } }"#,
        );
        let a = vault_config(&cfg, "a");
        assert_eq!(a.input_device.as_deref(), Some("Headset Mic"));
        assert_eq!(a.output_device.as_deref(), Some("Speakers"));
        let b = vault_config(&cfg, "missing");
        assert_eq!(b.input_device, None);
        assert_eq!(b.output_device, None);
    }

    #[test]
    fn mode_keys_round_trip() {
        for mode in [RecordingMode::Meeting, RecordingMode::VoiceNote] {
            assert_eq!(RecordingMode::from_key(mode.as_key()), Some(mode));
        }
        assert_eq!(RecordingMode::from_key("karaoke"), None);
    }

    #[test]
    fn config_round_trips_through_serialize_and_parse() {
        let mut cfg = AppConfig::default();
        cfg.vaults.insert(
            "abc".to_string(),
            VaultCaptureConfig {
                mode: RecordingMode::VoiceNote,
                recording_folder: Some("Inbox/Audio".to_string()),
                bitrate_kbps: 192,
                create_note: false,
                input_device: Some("USB Mic".to_string()),
                output_device: Some("Speakers (Realtek)".to_string()),
                transcribe: true,
                transcription_model: "medium".to_string(),
                transcription_language: Some("es".to_string()),
                transcript_timestamps: false,
                follow_up_template: true,
                tasks_folder: Some("Inbox/Tasks".to_string()),
            },
        );
        cfg.vaults
            .insert("def".to_string(), VaultCaptureConfig::default());
        let json = serialize_config(&cfg);
        let parsed = parse_config(&json);
        assert_eq!(parsed.vaults, cfg.vaults);
    }

    #[test]
    fn serialize_omits_absent_optional_fields() {
        let mut cfg = AppConfig::default();
        cfg.vaults
            .insert("a".to_string(), VaultCaptureConfig::default());
        let json = serialize_config(&cfg);
        assert!(
            !json.contains("recordingFolder"),
            "omitted when None: {json}"
        );
        assert!(!json.contains("inputDevice"), "omitted when None: {json}");
        assert!(!json.contains("outputDevice"), "omitted when None: {json}");
        assert!(json.ends_with('\n'), "hand-editable file ends in newline");
    }

    #[test]
    fn update_vault_config_preserves_sibling_vaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "vaults": { "keep": { "mode": "voice-note", "bitrateKbps": 160 } } }"#,
        )
        .unwrap();
        let updated = VaultCaptureConfig {
            input_device: Some("Mic 2".to_string()),
            ..VaultCaptureConfig::default()
        };
        update_vault_config_at(&path, "edited", updated.clone()).unwrap();
        let cfg = parse_config(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(vault_config(&cfg, "edited"), updated);
        let kept = vault_config(&cfg, "keep");
        assert_eq!(kept.mode, RecordingMode::VoiceNote);
        assert_eq!(kept.bitrate_kbps, 160);
    }

    #[test]
    fn write_config_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        write_config(&path, &AppConfig::default()).unwrap();
        // second write replaces the first — our own file, replacement is intent
        let mut cfg = AppConfig::default();
        cfg.vaults
            .insert("a".to_string(), VaultCaptureConfig::default());
        write_config(&path, &cfg).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"a\""));
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp not cleaned: {leftovers:?}");
    }

    #[test]
    fn update_on_malformed_file_starts_fresh_but_writes_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "not json at all").unwrap();
        update_vault_config_at(&path, "a", VaultCaptureConfig::default()).unwrap();
        let cfg = parse_config(&std::fs::read_to_string(&path).unwrap());
        assert!(cfg.vaults.contains_key("a"));
    }

    #[test]
    fn follow_up_template_defaults_on_and_parses() {
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": {},
                "b": { "followUpTemplate": false }
            } }"#,
        );
        assert!(vault_config(&cfg, "a").follow_up_template, "default on");
        assert!(!vault_config(&cfg, "b").follow_up_template);
    }

    #[test]
    fn follow_up_template_survives_a_round_trip() {
        let v = VaultCaptureConfig {
            follow_up_template: false,
            ..VaultCaptureConfig::default()
        };
        let mut cfg = AppConfig::default();
        cfg.vaults.insert("a".into(), v);
        let reparsed = parse_config(&serialize_config(&cfg));
        assert!(!vault_config(&reparsed, "a").follow_up_template);
    }

    #[test]
    fn tasks_folder_round_trips_and_defaults() {
        // Regression: a per-vault tasks folder must survive serialize→parse and
        // default to "Tasks" when absent, exactly like the recording folder does.
        let mut cfg = AppConfig::default();
        let mut v = VaultCaptureConfig::default();
        assert_eq!(v.tasks_root(), "Tasks"); // None → default
        v.tasks_folder = Some("Inbox/Tasks".to_string());
        assert_eq!(v.tasks_root(), "Inbox/Tasks");
        cfg.vaults.insert("v1".to_string(), v);

        let json = serialize_config(&cfg);
        assert!(json.contains("\"tasksFolder\": \"Inbox/Tasks\""));
        let parsed = parse_config(&json);
        assert_eq!(
            parsed.vaults["v1"].tasks_folder.as_deref(),
            Some("Inbox/Tasks")
        );

        // A None tasks_folder is omitted from the serialized entry.
        let mut cfg2 = AppConfig::default();
        cfg2.vaults
            .insert("v2".to_string(), VaultCaptureConfig::default());
        assert!(!serialize_config(&cfg2).contains("tasksFolder"));
    }

    #[test]
    fn mcp_config_defaults_when_absent_or_malformed() {
        let cfg = parse_config(r#"{ "vaults": {} }"#);
        assert_eq!(cfg.mcp, McpConfig::default());
        assert!(!cfg.mcp.enabled);
        assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT);
        // One malformed field defaults only itself — the file is hand-editable.
        let cfg = parse_config(
            r#"{ "mcp": { "enabled": true, "port": "not-a-number", "token": 5, "allowWrites": true } }"#,
        );
        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.port, DEFAULT_MCP_PORT);
        assert_eq!(cfg.mcp.token, "");
        assert!(cfg.mcp.allow_writes);
    }

    #[test]
    fn mcp_config_round_trips_through_serialize() {
        let cfg = AppConfig {
            mcp: McpConfig {
                enabled: true,
                port: 4321,
                token: "abc_-123".to_string(),
                allow_writes: true,
            },
            ..Default::default()
        };
        let reparsed = parse_config(&serialize_config(&cfg));
        assert_eq!(reparsed.mcp, cfg.mcp);
    }

    #[test]
    fn default_mcp_section_is_omitted_from_the_file() {
        // The hand-editable file stays minimal: users who never enable MCP
        // never see the section.
        let json = serialize_config(&AppConfig::default());
        assert!(!json.contains("mcp"), "got: {json}");
    }

    // Regression: serialize_config used to emit ONLY the vaults section, so a
    // capture/tasks settings save would silently DELETE an mcp section.
    #[test]
    fn saving_a_vault_config_preserves_the_mcp_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "mcp": { "enabled": true, "port": 22082, "token": "tok", "allowWrites": false }, "vaults": {} }"#,
        )
        .unwrap();
        update_vault_config_at(&path, "vault1", VaultCaptureConfig::default()).unwrap();
        let cfg = load_config_from(&path);
        assert!(cfg.mcp.enabled);
        assert_eq!(cfg.mcp.token, "tok");
        assert!(cfg.vaults.contains_key("vault1"));
    }

    #[test]
    fn update_mcp_config_at_preserves_vaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        update_vault_config_at(&path, "vault1", VaultCaptureConfig::default()).unwrap();
        let mcp = McpConfig {
            enabled: true,
            port: DEFAULT_MCP_PORT,
            token: "tok".to_string(),
            allow_writes: false,
        };
        update_mcp_config_at(&path, mcp.clone()).unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg.mcp, mcp);
        assert!(cfg.vaults.contains_key("vault1"));
    }
}
