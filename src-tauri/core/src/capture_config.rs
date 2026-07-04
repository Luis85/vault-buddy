//! Per-vault capture settings. App-side (%APPDATA%\vault-buddy\config.json),
//! keyed by Obsidian vault ID — never written into user vaults. Hand-edited
//! for now; a settings UI arrives in a later increment, so parsing must
//! shrug off any malformed input and fall back to defaults.

use std::collections::HashMap;
use std::path::PathBuf;

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
}

#[derive(Debug, Clone, PartialEq)]
pub struct VaultCaptureConfig {
    pub mode: RecordingMode,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
}

impl Default for VaultCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Meeting,
            recording_folder: None,
            bitrate_kbps: 128,
            create_note: true,
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
}

#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub vaults: HashMap<String, VaultCaptureConfig>,
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
    AppConfig { vaults }
}

fn vault_entry(entry: &serde_json::Value) -> VaultCaptureConfig {
    let defaults = VaultCaptureConfig::default();
    VaultCaptureConfig {
        mode: match entry.get("mode").and_then(|v| v.as_str()) {
            Some("voice-note") => RecordingMode::VoiceNote,
            Some("meeting") => RecordingMode::Meeting,
            _ => defaults.mode,
        },
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
    }
}

pub fn vault_config(cfg: &AppConfig, vault_id: &str) -> VaultCaptureConfig {
    cfg.vaults.get(vault_id).cloned().unwrap_or_default()
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vault-buddy").join("config.json"))
}

pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
