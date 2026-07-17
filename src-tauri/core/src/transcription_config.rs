//! App-global transcription settings (`config.json`'s `transcription`
//! section): machine-level knobs — today only the GPU escape hatch —
//! as opposed to the per-vault fields in `vault_config`. Split module,
//! same shape as `mcp_config`/`document_import_config`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionConfig {
    /// Ask whisper for GPU inference (Vulkan builds only; CPU fallback is
    /// whisper.cpp's own). Default on — the toggle exists as the escape
    /// hatch for buggy graphics drivers.
    pub use_gpu: bool,
}

impl Default for TranscriptionConfig {
    fn default() -> Self {
        Self { use_gpu: true }
    }
}

/// Per-field defensive parse — one malformed value defaults only itself.
pub fn parse_transcription_section(section: Option<&serde_json::Value>) -> TranscriptionConfig {
    let defaults = TranscriptionConfig::default();
    let Some(section) = section else {
        return defaults;
    };
    TranscriptionConfig {
        use_gpu: section
            .get("useGpu")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.use_gpu),
    }
}

/// The section for `serialize_config` — None when everything is default,
/// so the hand-editable file stays minimal.
pub fn serialize_transcription_section(cfg: &TranscriptionConfig) -> Option<serde_json::Value> {
    if *cfg == TranscriptionConfig::default() {
        return None;
    }
    Some(serde_json::json!({ "useGpu": cfg.use_gpu }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_config::{parse_config, serialize_config, AppConfig};

    #[test]
    fn use_gpu_defaults_on_parses_and_defends() {
        assert!(TranscriptionConfig::default().use_gpu, "GPU defaults on");
        let cfg = parse_config(r#"{ "transcription": { "useGpu": false } }"#);
        assert!(!cfg.transcription.use_gpu);
        // Malformed value defaults only itself (hand-editable file).
        let cfg = parse_config(r#"{ "transcription": { "useGpu": "nope" } }"#);
        assert!(cfg.transcription.use_gpu);
        // Absent section → defaults.
        assert!(parse_config("{}").transcription.use_gpu);
    }

    #[test]
    fn transcription_section_round_trips_and_stays_minimal() {
        // Regression class: serialize_config once dropped a whole section
        // (mcp) — a capture save must never delete this one either.
        let mut cfg = AppConfig::default();
        cfg.transcription.use_gpu = false;
        let json = serialize_config(&cfg);
        assert!(json.contains("\"useGpu\": false"), "got: {json}");
        assert!(!parse_config(&json).transcription.use_gpu);
        // Default-on is omitted — the hand-editable file stays minimal.
        let json2 = serialize_config(&AppConfig::default());
        assert!(!json2.contains("transcription"), "got: {json2}");
    }
}
