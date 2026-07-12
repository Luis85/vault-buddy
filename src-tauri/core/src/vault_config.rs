//! Per-vault capture settings: the `RecordingMode`/`VaultCaptureConfig`
//! types, their defaults, and the per-vault-entry parse/serialize functions.
//! Split out of `capture_config` for LOC headroom (the mcp_config /
//! document_import_config precedent) — that module re-exports these names,
//! so every existing `capture_config::RecordingMode` /
//! `capture_config::VaultCaptureConfig` caller keeps compiling unchanged.

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
    pub meeting_folder: Option<String>,
    pub voice_note_folder: Option<String>,
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
    /// Vault-relative folder holding imported documents. None → "Documents".
    pub documents_folder: Option<String>,
    /// The lists settings object (tasks domain): the list new tasks land in
    /// when the caller doesn't pick one. None/empty → the tasks root. Folders
    /// on disk remain the source of truth for which lists EXIST — these
    /// fields only hold preferences about them.
    pub default_list: Option<String>,
    /// Display order for list sections and pickers; folders not named here
    /// append alphabetically, names without folders are ignored.
    pub list_order: Vec<String>,
    /// Whether NEW recordings land in a dated `YYYY/MM` subfolder (true, the
    /// long-standing default) or flat in the recording root (false).
    /// Existing files in either layout are still found — flipping this only
    /// changes where new captures land.
    pub recording_date_folders: bool,
    /// Same toggle for the document-import domain's write path.
    pub document_date_folders: bool,
}

impl Default for VaultCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Meeting,
            meeting_folder: None,
            voice_note_folder: None,
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
            documents_folder: None,
            default_list: None,
            list_order: Vec::new(),
            recording_date_folders: true,
            document_date_folders: true,
        }
    }
}

/// Lexically normalize a vault-relative folder for dedup: split on BOTH
/// separators, drop empty and `.` components, rejoin with `/`. Catches the
/// realistic hand-edit collisions ("Audio" vs "Audio/" vs "Audio/.") without
/// filesystem access. NOT canonical: symlink or case aliasing of two distinct
/// configured folders still double-scans (accepted low-severity edge, Gaps.md).
fn normalize_folder(folder: &str) -> String {
    folder
        .split(['/', '\\'])
        .filter(|c| !c.is_empty() && *c != ".")
        .collect::<Vec<_>>()
        .join("/")
}

impl VaultCaptureConfig {
    /// The vault-relative folder for a given mode: the configured override, or
    /// the mode default (the PRD gives meetings and voice notes distinct homes).
    pub fn folder_for(&self, mode: RecordingMode) -> &str {
        match mode {
            RecordingMode::Meeting => self.meeting_folder.as_deref().unwrap_or("Meetings"),
            RecordingMode::VoiceNote => self.voice_note_folder.as_deref().unwrap_or("Voice Notes"),
        }
    }

    /// The folder the ACTIVE mode records into.
    pub fn effective_recording_folder(&self) -> &str {
        self.folder_for(self.mode)
    }

    /// Every folder a vault's recordings may live in — the deduped union of both
    /// modes' effective folders, so scans that must see EVERY recording (the
    /// Recordings list, recovery, transcription backfill) cover exactly the
    /// folders in use and no more.
    pub fn recording_roots(&self) -> Vec<&str> {
        let m = self.folder_for(RecordingMode::Meeting);
        let v = self.folder_for(RecordingMode::VoiceNote);
        if normalize_folder(m) == normalize_folder(v) {
            vec![m]
        } else {
            vec![m, v]
        }
    }

    /// The vault-relative folder holding this vault's task documents.
    pub fn tasks_root(&self) -> &str {
        self.tasks_folder.as_deref().unwrap_or("Tasks")
    }

    /// The vault-relative folder holding imported documents. None → "Documents".
    pub fn documents_root(&self) -> &str {
        self.documents_folder.as_deref().unwrap_or("Documents")
    }
}

/// Per-field parsing through serde_json::Value: the file is hand-edited,
/// and one malformed value must default only itself — a derived
/// deserializer would reject the whole file, silently flipping every
/// vault back to meeting mode (and thus desktop-audio capture).
pub fn vault_entry(entry: &serde_json::Value) -> VaultCaptureConfig {
    let defaults = VaultCaptureConfig::default();
    // Pre-split configs stored one unified `recordingFolder`. Fall back to it
    // per field so an upgrade seeds BOTH modes from the old value instead of
    // losing it — an explicit new key still wins over this legacy fallback.
    let legacy = entry.get("recordingFolder").and_then(|v| v.as_str());
    VaultCaptureConfig {
        mode: entry
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(RecordingMode::from_key)
            .unwrap_or(defaults.mode),
        meeting_folder: entry
            .get("meetingFolder")
            .and_then(|v| v.as_str())
            .or(legacy)
            .map(str::to_string),
        voice_note_folder: entry
            .get("voiceNoteFolder")
            .and_then(|v| v.as_str())
            .or(legacy)
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
        documents_folder: entry
            .get("documentsFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        default_list: entry
            .get("defaultList")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        list_order: entry
            .get("listOrder")
            .and_then(|v| v.as_array())
            .map(|a| {
                // Non-string items are dropped, not an error — one hand-edited
                // entry must not discard the rest of the order.
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default(),
        recording_date_folders: entry
            .get("recordingDateFolders")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
        document_date_folders: entry
            .get("documentDateFolders")
            .and_then(|v| v.as_bool())
            .unwrap_or(true),
    }
}

/// Serialize ONE vault entry to the schema `vault_entry` reads. Optional
/// fields omitted so the hand-editable file stays minimal.
pub fn serialize_vault_entry(v: &VaultCaptureConfig) -> serde_json::Map<String, serde_json::Value> {
    use serde_json::{json, Map};
    let mut entry = Map::new();
    entry.insert("mode".to_string(), json!(v.mode.as_key()));
    if let Some(folder) = &v.meeting_folder {
        entry.insert("meetingFolder".to_string(), json!(folder));
    }
    if let Some(folder) = &v.voice_note_folder {
        entry.insert("voiceNoteFolder".to_string(), json!(folder));
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
    if let Some(folder) = &v.documents_folder {
        entry.insert("documentsFolder".to_string(), json!(folder));
    }
    if let Some(list) = &v.default_list {
        entry.insert("defaultList".to_string(), json!(list));
    }
    if !v.list_order.is_empty() {
        entry.insert("listOrder".to_string(), json!(v.list_order));
    }
    if !v.recording_date_folders {
        entry.insert("recordingDateFolders".to_string(), json!(false));
    }
    if !v.document_date_folders {
        entry.insert("documentDateFolders".to_string(), json!(false));
    }
    entry
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_config::{parse_config, serialize_config, vault_config, AppConfig};

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
    fn folder_for_is_per_mode_with_defaults_and_overrides() {
        let cfg = crate::capture_config::parse_config(
            r#"{ "vaults": {
                "a": {},
                "b": { "meetingFolder": "Mtgs" },
                "c": { "meetingFolder": "Mtgs", "voiceNoteFolder": "Notes" }
            } }"#,
        );
        let a = crate::capture_config::vault_config(&cfg, "a");
        assert_eq!(a.folder_for(RecordingMode::Meeting), "Meetings");
        assert_eq!(a.folder_for(RecordingMode::VoiceNote), "Voice Notes");
        let b = crate::capture_config::vault_config(&cfg, "b");
        assert_eq!(b.folder_for(RecordingMode::Meeting), "Mtgs");
        assert_eq!(b.folder_for(RecordingMode::VoiceNote), "Voice Notes"); // untouched → default
        let c = crate::capture_config::vault_config(&cfg, "c");
        assert_eq!(c.folder_for(RecordingMode::VoiceNote), "Notes");
    }

    #[test]
    fn effective_folder_follows_the_active_mode() {
        let mut v = VaultCaptureConfig {
            meeting_folder: Some("M".into()),
            voice_note_folder: Some("V".into()),
            ..VaultCaptureConfig::default()
        };
        v.mode = RecordingMode::Meeting;
        assert_eq!(v.effective_recording_folder(), "M");
        v.mode = RecordingMode::VoiceNote;
        assert_eq!(v.effective_recording_folder(), "V");
    }

    #[test]
    fn recording_roots_is_the_deduped_union_of_both_modes() {
        // none → both defaults
        let none = VaultCaptureConfig::default();
        assert_eq!(none.recording_roots(), vec!["Meetings", "Voice Notes"]);
        // both custom, distinct → both
        let both = VaultCaptureConfig {
            meeting_folder: Some("A".into()),
            voice_note_folder: Some("B".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(both.recording_roots(), vec!["A", "B"]);
        // both custom, same → deduped to one
        let same = VaultCaptureConfig {
            meeting_folder: Some("Audio".into()),
            voice_note_folder: Some("Audio".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(same.recording_roots(), vec!["Audio"]);
        // one custom → custom + the other default
        let one = VaultCaptureConfig {
            meeting_folder: Some("Audio".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(one.recording_roots(), vec!["Audio", "Voice Notes"]);
    }

    #[test]
    fn legacy_recording_folder_seeds_both_and_retires_on_reserialize() {
        // A pre-split config with the unified key seeds BOTH modes (no data loss).
        let cfg = crate::capture_config::parse_config(
            r#"{ "vaults": { "v": { "recordingFolder": "Audio" } } }"#,
        );
        let v = crate::capture_config::vault_config(&cfg, "v");
        assert_eq!(v.meeting_folder.as_deref(), Some("Audio"));
        assert_eq!(v.voice_note_folder.as_deref(), Some("Audio"));
        // Explicit new keys win over the legacy fallback, per field.
        let cfg2 = crate::capture_config::parse_config(
            r#"{ "vaults": { "v": { "recordingFolder": "Audio", "voiceNoteFolder": "Notes" } } }"#,
        );
        let v2 = crate::capture_config::vault_config(&cfg2, "v");
        assert_eq!(v2.meeting_folder.as_deref(), Some("Audio")); // fell back
        assert_eq!(v2.voice_note_folder.as_deref(), Some("Notes")); // explicit
                                                                    // Re-serialize writes the two new keys, never the legacy one.
        let json = crate::capture_config::serialize_config(&cfg);
        assert!(json.contains("meetingFolder"));
        assert!(json.contains("voiceNoteFolder"));
        assert!(!json.contains("recordingFolder"));
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
        assert_eq!(v.meeting_folder, None);
        assert_eq!(v.voice_note_folder, None);
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
                meeting_folder: Some("Inbox/Audio".to_string()),
                voice_note_folder: Some("Inbox/Voice".to_string()),
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
                documents_folder: Some("Inbox/Documents".to_string()),
                default_list: Some("Inbox".to_string()),
                list_order: vec!["Inbox".to_string(), "Next".to_string()],
                recording_date_folders: false,
                document_date_folders: false,
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
        assert!(!json.contains("meetingFolder"), "omitted when None: {json}");
        assert!(
            !json.contains("voiceNoteFolder"),
            "omitted when None: {json}"
        );
        assert!(!json.contains("inputDevice"), "omitted when None: {json}");
        assert!(!json.contains("outputDevice"), "omitted when None: {json}");
        assert!(json.ends_with('\n'), "hand-editable file ends in newline");
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
    fn vault_entry_reads_lists_settings_defensively() {
        // The per-vault lists settings object (defaultList + listOrder): one
        // malformed entry defaults only itself, and non-string listOrder
        // items are dropped — the file is hand-editable.
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "defaultList": "Inbox",
                "listOrder": ["Next", 5, "  ", "Waiting"]
            } } }"#,
        );
        let v = vault_config(&cfg, "a");
        assert_eq!(v.default_list.as_deref(), Some("Inbox"));
        assert_eq!(v.list_order, vec!["Next", "Waiting"]);
        // Malformed types default only themselves.
        let cfg = parse_config(
            r#"{ "vaults": { "a": { "defaultList": 7, "listOrder": "Next", "mode": "voice-note" } } }"#,
        );
        let v = vault_config(&cfg, "a");
        assert_eq!(v.default_list, None);
        assert!(v.list_order.is_empty());
        assert_eq!(v.mode, RecordingMode::VoiceNote);
        // An empty/whitespace defaultList means "the tasks root" — stored as None.
        let cfg = parse_config(r#"{ "vaults": { "a": { "defaultList": "  " } } }"#);
        assert_eq!(vault_config(&cfg, "a").default_list, None);
    }

    #[test]
    fn lists_settings_round_trip_and_stay_minimal() {
        let mut cfg = AppConfig::default();
        cfg.vaults.insert(
            "a".to_string(),
            VaultCaptureConfig {
                default_list: Some("Inbox".to_string()),
                list_order: vec!["Inbox".to_string(), "Next".to_string()],
                ..VaultCaptureConfig::default()
            },
        );
        let json = serialize_config(&cfg);
        let parsed = parse_config(&json);
        assert_eq!(parsed.vaults["a"].default_list.as_deref(), Some("Inbox"));
        assert_eq!(parsed.vaults["a"].list_order, vec!["Inbox", "Next"]);
        // Defaults are omitted — the hand-editable file stays minimal.
        let mut cfg2 = AppConfig::default();
        cfg2.vaults
            .insert("b".to_string(), VaultCaptureConfig::default());
        let json2 = serialize_config(&cfg2);
        assert!(!json2.contains("defaultList"), "got: {json2}");
        assert!(!json2.contains("listOrder"), "got: {json2}");
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
    fn parses_documents_folder_per_vault() {
        let json = r#"{"vaults":{"v1":{"documentsFolder":"Imports"}}}"#;
        let cfg = parse_config(json);
        assert_eq!(
            cfg.vaults["v1"].documents_folder.as_deref(),
            Some("Imports")
        );
    }

    #[test]
    fn date_folder_toggles_default_true_and_round_trip() {
        let d = VaultCaptureConfig::default();
        assert!(d.recording_date_folders);
        assert!(d.document_date_folders);
        // Absent → true; present false parses.
        let cfg = crate::capture_config::parse_config(
            r#"{ "vaults": { "a": { "recordingDateFolders": false, "documentDateFolders": false } } }"#,
        );
        let a = crate::capture_config::vault_config(&cfg, "a");
        assert!(!a.recording_date_folders);
        assert!(!a.document_date_folders);
        // Serialize omits when true, writes when false.
        let mut only_true = crate::capture_config::AppConfig::default();
        only_true
            .vaults
            .insert("t".into(), VaultCaptureConfig::default());
        let jt = crate::capture_config::serialize_config(&only_true);
        assert!(!jt.contains("recordingDateFolders"));
        assert!(!jt.contains("documentDateFolders"));
        let mut has_false = crate::capture_config::AppConfig::default();
        has_false.vaults.insert(
            "f".into(),
            VaultCaptureConfig {
                recording_date_folders: false,
                document_date_folders: false,
                ..VaultCaptureConfig::default()
            },
        );
        let jf = crate::capture_config::serialize_config(&has_false);
        assert!(jf.contains("\"recordingDateFolders\": false"));
        assert!(jf.contains("\"documentDateFolders\": false"));
    }

    #[test]
    fn recording_roots_dedups_paths_that_normalize_equal() {
        // Two DIFFERENT strings resolving to the same dir must not double-scan.
        let c = VaultCaptureConfig {
            meeting_folder: Some("Audio".into()),
            voice_note_folder: Some("Audio/.".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(c.recording_roots(), vec!["Audio"]); // deduped to the meeting string
        let d = VaultCaptureConfig {
            meeting_folder: Some("Audio".into()),
            voice_note_folder: Some("Audio/".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(d.recording_roots(), vec!["Audio"]);
        // Genuinely distinct folders still yield both.
        let e = VaultCaptureConfig {
            meeting_folder: Some("A".into()),
            voice_note_folder: Some("B".into()),
            ..VaultCaptureConfig::default()
        };
        assert_eq!(e.recording_roots(), vec!["A", "B"]);
    }
}
