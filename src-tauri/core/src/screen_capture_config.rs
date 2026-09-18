//! Screen Capture's own per-vault settings vocabulary: the quality preset,
//! its bitrate maths, frame-rate validation, and the default folder name.
//!
//! Lives in its own module (the mcp_config / document_import_config
//! precedent) for LOC headroom; `vault_config` owns the fields themselves.

/// Default per-vault folder for saved screen captures (spec §9).
pub const DEFAULT_SCREEN_FOLDER: &str = "Screen Captures";

/// Floor so a degenerate resolution can never ask the encoder for 0 bps,
/// which it rejects outright.
pub const MIN_BITRATE_BPS: u32 = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenQuality {
    Low,
    #[default]
    Balanced,
    High,
}

impl ScreenQuality {
    /// Stable key used in config.json and the IPC DTOs.
    pub fn as_key(&self) -> &'static str {
        match self {
            ScreenQuality::Low => "low",
            ScreenQuality::Balanced => "balanced",
            ScreenQuality::High => "high",
        }
    }

    pub fn from_key(key: &str) -> Option<ScreenQuality> {
        match key {
            "low" => Some(ScreenQuality::Low),
            "balanced" => Some(ScreenQuality::Balanced),
            "high" => Some(ScreenQuality::High),
            _ => None,
        }
    }

    /// Bits per pixel per frame. Exhaustive match: a new preset must decide
    /// this explicitly rather than inherit a default.
    pub fn bits_per_pixel(&self) -> f64 {
        match self {
            ScreenQuality::Low => 0.08,
            ScreenQuality::Balanced => 0.15,
            ScreenQuality::High => 0.25,
        }
    }
}

/// Target video bitrate for a given output size and frame rate.
///
/// Area-relative rather than absolute so a 4K capture is not starved at the
/// same bitrate as a 720p one (spec §12).
pub fn bitrate_bps(quality: ScreenQuality, width: u32, height: u32, fps: u32) -> u32 {
    let pixels = width as f64 * height as f64;
    let raw = pixels * fps as f64 * quality.bits_per_pixel();
    if !raw.is_finite() || raw <= 0.0 {
        return MIN_BITRATE_BPS;
    }
    (raw.min(u32::MAX as f64) as u32).max(MIN_BITRATE_BPS)
}

/// Only 30 and 60 are offered. Anything else — including a hand-edited
/// config.json — reads as 30 rather than erroring, matching the per-field
/// defensive-parse posture of the whole config module.
pub fn normalize_fps(fps: u32) -> u32 {
    match fps {
        60 => 60,
        _ => 30,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_keys_round_trip() {
        for q in [
            ScreenQuality::Low,
            ScreenQuality::Balanced,
            ScreenQuality::High,
        ] {
            assert_eq!(ScreenQuality::from_key(q.as_key()), Some(q));
        }
    }

    #[test]
    fn an_unknown_quality_key_is_none_so_the_caller_can_default() {
        assert_eq!(ScreenQuality::from_key("ultra"), None);
        assert_eq!(ScreenQuality::from_key(""), None);
    }

    #[test]
    fn quality_is_balanced_by_default() {
        assert_eq!(ScreenQuality::default(), ScreenQuality::Balanced);
    }

    // The presets are bits per pixel per frame (spec §12) rather than a flat
    // bitrate, so a 4K capture is not starved at the same absolute rate as a
    // 720p one.
    #[test]
    fn bitrate_scales_with_area_and_frame_rate() {
        let hd_30 = bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 30);
        let hd_60 = bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 60);
        let uhd_30 = bitrate_bps(ScreenQuality::Balanced, 3840, 2160, 30);
        assert_eq!(hd_60, hd_30 * 2, "doubling fps doubles bitrate");
        assert_eq!(uhd_30, hd_30 * 4, "quadrupling area quadruples bitrate");
    }

    #[test]
    fn bitrate_uses_the_documented_bits_per_pixel_presets() {
        assert_eq!(ScreenQuality::Low.bits_per_pixel(), 0.08);
        assert_eq!(ScreenQuality::Balanced.bits_per_pixel(), 0.15);
        assert_eq!(ScreenQuality::High.bits_per_pixel(), 0.25);
        // 1920*1080*30*0.15 = 9_331_200
        assert_eq!(
            bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 30),
            9_331_200
        );
    }

    #[test]
    fn higher_presets_produce_higher_bitrates() {
        let low = bitrate_bps(ScreenQuality::Low, 1920, 1080, 30);
        let bal = bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 30);
        let high = bitrate_bps(ScreenQuality::High, 1920, 1080, 30);
        assert!(low < bal && bal < high);
    }

    // A degenerate size must never yield a zero bitrate: the encoder rejects
    // it outright.
    #[test]
    fn a_degenerate_size_still_yields_a_usable_floor() {
        assert!(bitrate_bps(ScreenQuality::Low, 0, 0, 0) >= MIN_BITRATE_BPS);
        assert!(bitrate_bps(ScreenQuality::Low, 2, 2, 1) >= MIN_BITRATE_BPS);
    }

    #[test]
    fn only_30_and_60_fps_are_legal_and_anything_else_is_30() {
        assert_eq!(normalize_fps(30), 30);
        assert_eq!(normalize_fps(60), 60);
        assert_eq!(normalize_fps(0), 30);
        assert_eq!(
            normalize_fps(24),
            30,
            "a hand-edited config defaults, never errors"
        );
        assert_eq!(normalize_fps(144), 30);
    }
}
