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

    /// Stable `u8` encoding for the shell's lock-free in-memory cache of the
    /// current preset (an `AtomicU8`), so the main-thread panel-show path never
    /// reads config.json from disk. The values are a wire format — keep them
    /// fixed; `from_u8` maps anything unrecognized back to the default.
    pub fn as_u8(self) -> u8 {
        match self {
            PanelSize::Compact => 0,
            PanelSize::Comfortable => 1,
            PanelSize::Large => 2,
        }
    }

    /// Inverse of `as_u8`; an unknown byte degrades to `Comfortable` (matching
    /// `from_str`'s infallible-default posture).
    pub fn from_u8(v: u8) -> PanelSize {
        match v {
            0 => PanelSize::Compact,
            2 => PanelSize::Large,
            _ => PanelSize::Comfortable,
        }
    }

    /// Logical (width, height) for this preset. Height-biased — tasks need
    /// vertical room. NOTE: `place_beside` clamps only the window's *position*
    /// into the work area, never its size, so the shell additionally shrinks
    /// these dims with `clamp_dims_to_work_area` before `set_size` — otherwise
    /// `large` on a small/scaled, non-resizable monitor would push controls off
    /// the bottom/right edge.
    pub fn dims(self) -> (f64, f64) {
        match self {
            PanelSize::Compact => (400.0, 460.0),
            PanelSize::Comfortable => (448.0, 580.0),
            PanelSize::Large => (560.0, 720.0),
        }
    }
}

/// Leave this much logical margin between the panel and the work-area edges so
/// a clamped panel isn't flush against the screen border.
const WORK_AREA_MARGIN: f64 = 24.0;
/// Never shrink a panel below this in either dimension — a floor for absurd
/// (sub-monitor) work areas that keeps the window from collapsing to nothing.
/// Real small/scaled monitors are far larger than this, so the floor never
/// bites them; it only guards degenerate inputs.
const MIN_PANEL_DIM: f64 = 320.0;

/// Shrink a preset's logical dims to fit within the monitor's logical work
/// area (minus a small margin), never growing them. `place_beside` positions
/// but never resizes an oversized non-resizable window, so without this a tall
/// preset can strand controls off-screen on a small or display-scaled monitor.
pub fn clamp_dims_to_work_area(w: f64, h: f64, area_w: f64, area_h: f64) -> (f64, f64) {
    let max_w = (area_w - WORK_AREA_MARGIN).max(MIN_PANEL_DIM);
    let max_h = (area_h - WORK_AREA_MARGIN).max(MIN_PANEL_DIM);
    (w.min(max_w), h.min(max_h))
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
    fn u8_round_trips_and_defaults_unknown() {
        for s in [PanelSize::Compact, PanelSize::Comfortable, PanelSize::Large] {
            assert_eq!(PanelSize::from_u8(s.as_u8()), s);
        }
        // Any unrecognized byte degrades to the default.
        assert_eq!(PanelSize::from_u8(9), PanelSize::Comfortable);
    }

    #[test]
    fn clamp_leaves_a_fitting_size_unchanged() {
        // comfortable on a normal 1080p work area — well within bounds.
        assert_eq!(
            clamp_dims_to_work_area(448.0, 580.0, 1920.0, 1040.0),
            (448.0, 580.0)
        );
    }

    #[test]
    fn clamp_shrinks_an_oversized_preset_to_the_work_area() {
        // large (560x720) on a 150%-scaled 1366x768 laptop → logical ~911x512:
        // width fits, height must shrink to the work area minus the margin.
        let (w, h) = clamp_dims_to_work_area(560.0, 720.0, 911.0, 512.0);
        assert_eq!(w, 560.0);
        assert_eq!(h, 512.0 - WORK_AREA_MARGIN);
        assert!(h <= 512.0);
    }

    #[test]
    fn clamp_floors_on_a_degenerate_work_area() {
        // An absurd sub-monitor work area can't fit anything; the floor keeps
        // the window from collapsing rather than chasing the tiny area.
        let (w, h) = clamp_dims_to_work_area(560.0, 720.0, 80.0, 80.0);
        assert_eq!(w, MIN_PANEL_DIM);
        assert_eq!(h, MIN_PANEL_DIM);
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
