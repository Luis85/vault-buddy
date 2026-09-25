//! The REGION capture source: a rectangle on one monitor, encoded as the
//! string that crosses the IPC boundary.
//!
//! PURE and on both platforms deliberately. Everything here is either
//! parsing untrusted input or joining two monitor-numbering schemes, and
//! both are things no CI runner can check on Windows (docs/Gaps.md
//! GAP-117) — so they live where Linux can pin them.
//!
//! **Why commas.** `source::SourceId::parse` splits on the FIRST `:` and
//! rejects a payload containing a second one, so the five fields of a
//! region cannot be colon-separated without changing that contract for
//! every source kind.
//!
//! **Why a canonical round-trip is the strictness rule.** `"+1"`, `"01"`
//! and `" 1"` are all things `u32::from_str` will or will not accept in
//! ways nobody remembers; requiring `encode_payload(parsed) == input`
//! makes exactly one spelling legal without enumerating the illegal ones.
//! The id is untrusted by the time it comes back — a lenient parse that
//! resolved to *some* rectangle would record something the user never drew.

use vault_buddy_core::screen_geometry::PhysicalRect;

/// A capture region: which monitor, and where on it in PHYSICAL pixels
/// relative to that monitor's own origin (never virtual-desktop
/// coordinates — see `core::screen_geometry`'s module doc for why the
/// distinction is load-bearing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSource {
    /// The GDI display number, i.e. the `n` in `\\.\DISPLAYn`. The SAME
    /// value `windows_capture::monitor::Monitor::index()` reports.
    pub monitor: usize,
    pub rect: PhysicalRect,
}

/// The part of the source id after `region:`.
pub fn encode_payload(r: RegionSource) -> String {
    format!(
        "{},{},{},{},{}",
        r.monitor, r.rect.x, r.rect.y, r.rect.width, r.rect.height
    )
}

/// Strict parse of that payload. `None` for anything that is not exactly
/// what `encode_payload` produces.
pub fn parse_payload(rest: &str) -> Option<RegionSource> {
    let mut fields = rest.split(',');
    let monitor = fields.next()?.parse::<usize>().ok()?;
    let x = fields.next()?.parse::<u32>().ok()?;
    let y = fields.next()?.parse::<u32>().ok()?;
    let width = fields.next()?.parse::<u32>().ok()?;
    let height = fields.next()?.parse::<u32>().ok()?;
    if fields.next().is_some() {
        return None;
    }
    // A zero-sized region is not encodable (NV12 has no zero dimension)
    // and `clamp_to_frame` would reject it later anyway — refusing it
    // here means the failure names the id rather than the monitor.
    if width == 0 || height == 0 {
        return None;
    }
    let parsed = RegionSource {
        monitor,
        rect: PhysicalRect {
            x,
            y,
            width,
            height,
        },
    };
    // Canonical spelling only; see the module doc.
    (encode_payload(parsed) == rest).then_some(parsed)
}

/// `\\.\DISPLAY2` -> `2`.
pub fn display_number_from_device_name(name: &str) -> Option<usize> {
    let digits = name.strip_prefix(r"\\.\DISPLAY")?;
    // Reject anything that is not purely digits. `.parse()` below already
    // rejects most of it -- including `\\.\DISPLAY1\Monitor0`, a real
    // Windows device string for the MONITOR under a display -- but NOT a
    // leading `+`, which usize::from_str accepts: without this guard
    // `\\.\DISPLAY+1` would resolve to display 1.
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(monitor: usize, x: u32, y: u32, width: u32, height: u32) -> RegionSource {
        RegionSource {
            monitor,
            rect: PhysicalRect {
                x,
                y,
                width,
                height,
            },
        }
    }

    #[test]
    fn a_region_payload_round_trips() {
        // The expected string is written out by hand, field by field, in
        // the documented order: monitor, x, y, width, height.
        let r = region(2, 320, 180, 1280, 720);
        assert_eq!(encode_payload(r), "2,320,180,1280,720");
        assert_eq!(parse_payload("2,320,180,1280,720"), Some(r));
    }

    #[test]
    fn an_origin_region_round_trips() {
        let r = region(1, 0, 0, 1920, 1080);
        assert_eq!(encode_payload(r), "1,0,0,1920,1080");
        assert_eq!(parse_payload("1,0,0,1920,1080"), Some(r));
    }

    // Untrusted input. A lenient parse would start a capture of a
    // rectangle the user never drew, so every one of these must be None
    // rather than "close enough".
    #[test]
    fn a_malformed_region_payload_is_refused() {
        for bad in [
            "",            // empty
            "1",           // one field
            "1,2,3,4",     // four fields
            "1,2,3,4,5,6", // six fields
            "a,2,3,4,5",   // non-numeric monitor
            "1,-2,3,4,5",  // negative origin
            "1,2,3,4.5,6", // fractional
            "1,2,3,0,5",   // zero width: unencodable
            "1,2,3,4,0",   // zero height: unencodable
            " 1,2,3,4,5",  // leading space
            "1,2,3,4,5 ",  // trailing space
            "+1,2,3,4,5",  // non-canonical monitor
            "01,2,3,4,5",  // non-canonical monitor
            "1,02,3,4,5",  // non-canonical origin
            "1,2,3,4,5,",  // trailing separator
        ] {
            assert_eq!(parse_payload(bad), None, "must refuse {bad:?}");
        }
    }

    // The join between the two monitor numbering schemes. Getting this
    // wrong is not a cosmetic bug: `Monitor::from_index` is POSITIONAL
    // while `Monitor::index()` is the GDI display number, and crossing
    // them silently records the WRONG SCREEN with no error — which is
    // exactly what phase 2's task 5 shipped before review caught it.
    #[test]
    fn a_gdi_device_name_yields_its_display_number() {
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY1"), Some(1));
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY2"), Some(2));
        // Two digits, so a prefix-only implementation that grabbed one
        // character fails here.
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY12"), Some(12));
    }

    #[test]
    fn a_non_display_device_name_is_refused() {
        // `\\.\DISPLAY1\Monitor0` is a real Windows device string — the
        // MONITOR under a display, not the display. Accepting it by
        // "parsing the leading digits" would map two different devices
        // onto one id.
        assert_eq!(
            display_number_from_device_name(r"\\.\DISPLAY1\Monitor0"),
            None
        );
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY"), None);
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAYx"), None);
        assert_eq!(display_number_from_device_name("DISPLAY1"), None);
        assert_eq!(display_number_from_device_name(""), None);
        // The `all(is_ascii_digit)` guard's ONE observable effect: Rust's
        // usize::from_str accepts a leading `+`, so without the guard
        // `\\.\DISPLAY+1` parses as display 1. Every other case in this
        // test is already rejected by `.parse()` alone, so this is the
        // only assertion here that fails when the guard is deleted.
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY+1"), None);
    }
}
