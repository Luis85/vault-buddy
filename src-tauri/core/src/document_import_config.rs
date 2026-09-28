//! App-global external-tool settings. Pandoc and ffmpeg are each one
//! system-wide binary, so their path overrides are app-global, not per-vault:
//! a top-level `documentImport` section beside `vaults`/`mcp` in the
//! hand-editable config.json, parsed per-field defensively like every other
//! section. ffmpeg (screen-capture export, phase 5) shares the section rather
//! than opening a second one: the two fields are the same kind of thing (a
//! manual override for a tool not on PATH) and a new section would have to be
//! defended by its own round-trip regression test for nothing.
//! Split out of `capture_config` for LOC headroom (the mcp_config
//! precedent) — that module re-exports the name, so callers are unchanged.

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentImportConfig {
    /// Manual override for a Pandoc not on PATH (a portable install).
    /// None → detect on PATH only.
    pub pandoc_path: Option<String>,
    /// Manual override for an ffmpeg not on PATH (a portable/extracted build).
    /// None → detect on PATH only. `ffprobe` is NOT configured separately: it
    /// is resolved beside the chosen ffmpeg first, then PATH.
    pub ffmpeg_path: Option<String>,
}

pub(crate) fn document_import_entry(entry: &serde_json::Value) -> DocumentImportConfig {
    DocumentImportConfig {
        pandoc_path: tool_path(entry, "pandocPath"),
        ffmpeg_path: tool_path(entry, "ffmpegPath"),
    }
}

/// Per-field defensive read of one tool-path override: a non-string, or a
/// blank/whitespace-only string, reads as unset rather than as a path we would
/// later try to spawn.
fn tool_path(entry: &serde_json::Value, key: &str) -> Option<String> {
    entry
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
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
    fn parses_the_ffmpeg_override_beside_pandoc() {
        let json = r#"{"documentImport":{"pandocPath":"/usr/bin/pandoc","ffmpegPath":"/opt/ffmpeg/ffmpeg"},"vaults":{}}"#;
        let cfg = parse_config(json);
        assert_eq!(
            cfg.document_import.pandoc_path.as_deref(),
            Some("/usr/bin/pandoc")
        );
        assert_eq!(
            cfg.document_import.ffmpeg_path.as_deref(),
            Some("/opt/ffmpeg/ffmpeg")
        );
        // Blank / wrong-typed values read as unset, never as a spawnable path.
        let blank = parse_config(r#"{"documentImport":{"ffmpegPath":"   "},"vaults":{}}"#);
        assert_eq!(blank.document_import.ffmpeg_path, None);
        let wrong = parse_config(r#"{"documentImport":{"ffmpegPath":7},"vaults":{}}"#);
        assert_eq!(wrong.document_import.ffmpeg_path, None);
    }

    #[test]
    fn serialize_roundtrips_both_tool_overrides_independently() {
        // Regression shape: the section is serialized field by field, so an
        // ffmpeg-only override must survive a save that knows nothing about
        // Pandoc, and vice versa — set_ffmpeg_path and set_pandoc_path each
        // read-modify-write this ONE struct.
        let mut only_ffmpeg = AppConfig::default();
        only_ffmpeg.document_import.ffmpeg_path = Some("/opt/ffmpeg/ffmpeg".into());
        let round = parse_config(&serialize_config(&only_ffmpeg));
        assert_eq!(round.document_import, only_ffmpeg.document_import);
        assert_eq!(round.document_import.pandoc_path, None);

        let mut both = AppConfig::default();
        both.document_import.pandoc_path = Some("/usr/bin/pandoc".into());
        both.document_import.ffmpeg_path = Some("/opt/ffmpeg/ffmpeg".into());
        let round = parse_config(&serialize_config(&both));
        assert_eq!(round.document_import, both.document_import);
    }

    #[test]
    fn serialize_omits_default_document_import_section() {
        let cfg = AppConfig::default();
        assert!(!serialize_config(&cfg).contains("documentImport"));
    }
}
