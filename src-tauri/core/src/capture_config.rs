//! Per-vault capture settings. App-side (%APPDATA%\vault-buddy\config.json),
//! keyed by Obsidian vault ID — never written into user vaults. Read by the
//! recording path and written by the settings UI (set_capture_config), and
//! still hand-editable — parsing must shrug off any malformed input and
//! fall back to defaults.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

// The per-vault struct/enum + parser live in their own module (LOC headroom
// for the per-mode folders + layout toggle); re-exported here so every
// existing `capture_config::RecordingMode` / `capture_config::VaultCaptureConfig`
// caller keeps compiling unchanged.
pub use crate::vault_config::{RecordingMode, VaultCaptureConfig};

// The MCP and Document Import sections live in their own modules (LOC
// headroom for the tasks fields); re-exported here so every existing
// `capture_config::McpConfig` / `capture_config::DocumentImportConfig`
// caller keeps compiling unchanged.
use crate::document_import_config::document_import_entry;
pub use crate::document_import_config::DocumentImportConfig;
use crate::mcp_config::mcp_entry;
pub use crate::mcp_config::{McpConfig, DEFAULT_MCP_PORT};
pub use crate::transcription_config::TranscriptionConfig;
use crate::transcription_config::{parse_transcription_section, serialize_transcription_section};

#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub vaults: HashMap<String, VaultCaptureConfig>,
    pub mcp: McpConfig,
    pub document_import: DocumentImportConfig,
    pub transcription: TranscriptionConfig,
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
            vaults.insert(id.clone(), crate::vault_config::vault_entry(entry));
        }
    }
    let mcp = value.get("mcp").map(mcp_entry).unwrap_or_default();
    let document_import = value
        .get("documentImport")
        .map(document_import_entry)
        .unwrap_or_default();
    let transcription = parse_transcription_section(value.get("transcription"));
    AppConfig {
        vaults,
        mcp,
        document_import,
        transcription,
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
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
        Err(e) => {
            log::warn!("config: cannot read {}: {e}", path.display());
            AppConfig::default()
        }
    }
}

/// Read the config for a read-modify-write UPDATE. A MISSING file is fine (first
/// run) → default; any OTHER read error (the file EXISTS but can't be read — a
/// permission / AV / indexing hiccup) is propagated so the caller ABORTS the
/// save instead of writing a default that silently drops every other vault/MCP
/// section (Codex review — the save side of GAP-02). Readable-but-corrupt JSON
/// still degrades per-field via `parse_config`, the same as every read path.
fn load_config_for_update(path: &Path) -> std::io::Result<AppConfig> {
    match std::fs::read_to_string(path) {
        Ok(json) => Ok(parse_config(&json)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(e) => {
            log::warn!("config: {} unreadable during save: {e}", path.display());
            Err(e)
        }
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
/// stable and minimal across saves. The mcp/document-import/transcription
/// sections are each included only when non-default, so a user who never
/// touches them never sees the section.
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
    if cfg.document_import != DocumentImportConfig::default() {
        let mut di = Map::new();
        if let Some(p) = &cfg.document_import.pandoc_path {
            di.insert("pandocPath".to_string(), json!(p));
        }
        root.insert("documentImport".to_string(), Value::Object(di));
    }
    if let Some(transcription) = serialize_transcription_section(&cfg.transcription) {
        root.insert("transcription".to_string(), transcription);
    }
    let mut vaults = Map::new();
    let mut ids: Vec<&String> = cfg.vaults.keys().collect();
    ids.sort();
    for id in ids {
        let entry = crate::vault_config::serialize_vault_entry(&cfg.vaults[id]);
        vaults.insert(id.clone(), serde_json::Value::Object(entry));
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
    let mut cfg = load_config_for_update(path)?;
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
    let mut cfg = load_config_for_update(path)?;
    cfg.mcp = mcp;
    write_config(path, &cfg)
}

/// Read-modify-write for the app-global document-import section, mirroring
/// update_mcp_config_at (same no-own-lock rule: IPC callers serialize
/// behind ConfigWriteLock).
pub fn update_document_import_config_at(
    path: &Path,
    di: DocumentImportConfig,
) -> std::io::Result<()> {
    let mut cfg = load_config_for_update(path)?;
    cfg.document_import = di;
    write_config(path, &cfg)
}

pub fn update_document_import_config(di: DocumentImportConfig) -> Result<(), String> {
    let path = config_path().ok_or("Cannot resolve the config directory")?;
    update_document_import_config_at(&path, di)
        .map_err(|e| format!("Could not save document import settings: {e}"))
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

    // A read error other than "missing file" must ABORT the save, not default
    // and clobber every other section (GAP-02, save side). A directory at the
    // config path forces a non-NotFound read error deterministically — even as
    // root, unlike a permission bit.
    #[test]
    fn update_aborts_on_a_non_missing_read_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config-is-a-dir");
        std::fs::create_dir(&path).unwrap();
        let di = DocumentImportConfig {
            pandoc_path: Some("C:/pandoc.exe".into()),
        };
        assert!(update_document_import_config_at(&path, di).is_err());
        // Nothing was written over the directory.
        assert!(path.is_dir());
    }

    #[test]
    fn update_defaults_and_saves_when_the_config_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json"); // does not exist yet
        let di = DocumentImportConfig {
            pandoc_path: Some("C:/pandoc.exe".into()),
        };
        update_document_import_config_at(&path, di).unwrap();
        let cfg = parse_config(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(
            cfg.document_import.pandoc_path.as_deref(),
            Some("C:/pandoc.exe")
        );
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
    fn capture_save_preserves_lists_settings() {
        // The set_capture_config clobber class: a capture-settings save
        // read-modify-writes the vault entry, and the value it writes must
        // carry the lists settings it read — serialize_config dropping (or
        // the DTO layer forgetting) either field would silently reset the
        // lists object on every capture save.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "vaults": { "v": { "defaultList": "Inbox", "listOrder": ["Inbox","Next"], "bitrateKbps": 160 } } }"#,
        )
        .unwrap();
        // A capture-shaped update that (correctly) round-trips the whole entry.
        let mut v = parse_config(&std::fs::read_to_string(&path).unwrap()).vaults["v"].clone();
        v.bitrate_kbps = 192;
        update_vault_config_at(&path, "v", v).unwrap();
        let cfg = load_config_from(&path);
        assert_eq!(cfg.vaults["v"].default_list.as_deref(), Some("Inbox"));
        assert_eq!(cfg.vaults["v"].list_order, vec!["Inbox", "Next"]);
        assert_eq!(cfg.vaults["v"].bitrate_kbps, 192);
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

    // Same regression class as above, for the app-global transcription
    // section: a vault-config save must never delete a non-default
    // transcription section either.
    #[test]
    fn saving_a_vault_config_preserves_the_transcription_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "transcription": { "useGpu": false }, "vaults": {} }"#,
        )
        .unwrap();
        update_vault_config_at(&path, "vault1", VaultCaptureConfig::default()).unwrap();
        let cfg = load_config_from(&path);
        assert!(!cfg.transcription.use_gpu);
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

    #[test]
    fn documents_root_defaults_to_documents() {
        let mut c = VaultCaptureConfig::default();
        assert_eq!(c.documents_root(), "Documents");
        c.documents_folder = Some("Imports".into());
        assert_eq!(c.documents_root(), "Imports");
    }
}
