//! App-global Document Import settings. Pandoc is one system-wide binary,
//! so its path override is app-global, not per-vault: a top-level
//! `documentImport` section beside `vaults`/`mcp` in the hand-editable
//! config.json, parsed per-field defensively like every other section.
//! Split out of `capture_config` for LOC headroom (the mcp_config
//! precedent) — that module re-exports the name, so callers are unchanged.

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentImportConfig {
    /// Manual override for a Pandoc not on PATH (a portable install).
    /// None → detect on PATH only.
    pub pandoc_path: Option<String>,
}

pub(crate) fn document_import_entry(entry: &serde_json::Value) -> DocumentImportConfig {
    DocumentImportConfig {
        pandoc_path: entry
            .get("pandocPath")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use crate::capture_config::{parse_config, serialize_config, AppConfig};

    #[test]
    fn parses_document_import_section() {
        let json = r#"{"documentImport":{"pandocPath":"C:\\pandoc\\pandoc.exe"},"vaults":{}}"#;
        let cfg = parse_config(json);
        assert_eq!(
            cfg.document_import.pandoc_path.as_deref(),
            Some("C:\\pandoc\\pandoc.exe")
        );
    }

    #[test]
    fn serialize_roundtrips_document_import_section() {
        // Regression: serialize_config once emitted only `vaults`; a save from
        // another surface would silently delete this section. Mirrors the mcp test.
        let mut cfg = AppConfig::default();
        cfg.document_import.pandoc_path = Some("/usr/bin/pandoc".into());
        let round = parse_config(&serialize_config(&cfg));
        assert_eq!(round.document_import, cfg.document_import);
    }

    #[test]
    fn serialize_omits_default_document_import_section() {
        let cfg = AppConfig::default();
        assert!(!serialize_config(&cfg).contains("documentImport"));
    }
}
