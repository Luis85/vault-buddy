//! App-global panel window preset size, stored as a top-level `panel` section
//! beside `vaults`/`mcp` in config.json. Pure size→dims mapping (no Tauri
//! types) so the shell reads it on the flicker-safe panel-open path (the
//! panel is sized only while hidden — see `commands::position_panel`).

/// The three panel presets. `Comfortable` is the default (and the
/// tauri.conf.json default), so an absent/malformed config lands there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelSize {
    Compact,
    #[default]
    Comfortable,
    Large,
}

impl PanelSize {
    /// Infallible by design (unrecognized input defaults to `Comfortable`),
    /// so this intentionally isn't `std::str::FromStr` — that trait's
    /// `from_str` returns a `Result`, which doesn't fit "always resolves to
    /// a size" (same rationale as `transcribe::ModelTier::from_str`).
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> PanelSize {
        match s {
            "compact" => PanelSize::Compact,
            "large" => PanelSize::Large,
            _ => PanelSize::Comfortable, // "comfortable" + any unknown value
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PanelSize::Compact => "compact",
            PanelSize::Comfortable => "comfortable",
            PanelSize::Large => "large",
        }
    }

    /// Logical (width, height) for this preset. Height-biased — tasks need
    /// vertical room. `place_beside` clamps into the work area, so `large` is
    /// safe on small screens.
    pub fn dims(self) -> (f64, f64) {
        match self {
            PanelSize::Compact => (400.0, 460.0),
            PanelSize::Comfortable => (448.0, 580.0),
            PanelSize::Large => (560.0, 720.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PanelConfig {
    pub size: PanelSize,
}

/// Parse a `panel` config entry defensively — a missing or non-string `size`
/// degrades to the default. Mirrors `mcp_config::mcp_entry`'s idiom exactly.
pub(crate) fn panel_entry(entry: &serde_json::Value) -> PanelConfig {
    let size = entry
        .get("size")
        .and_then(|v| v.as_str())
        .map(PanelSize::from_str)
        .unwrap_or_default();
    PanelConfig { size }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_defaults_unknown_to_comfortable() {
        assert_eq!(PanelSize::from_str("compact"), PanelSize::Compact);
        assert_eq!(PanelSize::from_str("large"), PanelSize::Large);
        assert_eq!(PanelSize::from_str("comfortable"), PanelSize::Comfortable);
        assert_eq!(PanelSize::from_str("nonsense"), PanelSize::Comfortable);
        assert_eq!(PanelSize::default(), PanelSize::Comfortable);
    }

    #[test]
    fn dims_match_the_presets() {
        assert_eq!(PanelSize::Compact.dims(), (400.0, 460.0));
        assert_eq!(PanelSize::Comfortable.dims(), (448.0, 580.0));
        assert_eq!(PanelSize::Large.dims(), (560.0, 720.0));
    }

    #[test]
    fn panel_entry_reads_size_defensively() {
        assert_eq!(
            panel_entry(&serde_json::json!({"size": "large"})).size,
            PanelSize::Large
        );
        // missing / wrong-type → default
        assert_eq!(
            panel_entry(&serde_json::json!({})).size,
            PanelSize::Comfortable
        );
        assert_eq!(
            panel_entry(&serde_json::json!({"size": 5})).size,
            PanelSize::Comfortable
        );
    }
}
