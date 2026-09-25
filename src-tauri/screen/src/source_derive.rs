//! What `resolve` and `list_sources` DERIVE from a live monitor or window:
//! the size a recording is opened at, the crop a source contributes, and the
//! label a monitor is shown under.
//!
//! Its own module rather than more of `source.rs` for a measured reason: that
//! file sat at 783 of this repo's 800-line Rust cap (shrink-only, inline tests
//! counted), with the next source kind or resolve fix certain to breach it.
//! The seam is the one `source.rs` already drew in prose on every function
//! moved here: each exists because the `cfg(windows)` arm that calls it
//! executes in no automated test anywhere (docs/Gaps.md GAP-117), so the
//! arithmetic had to be lifted out to where Linux can pin it.
//!
//! **Which side of the seam this is: the PURE side.** Nothing here names a
//! `windows` or `windows-capture` type; every function takes plain integers,
//! a `PhysicalRect` or an `Option<&str>`, and compiles and tests on every
//! platform. What stays in `source.rs` is the id encoding (`SourceId`, also
//! pure) and the enumeration/resolution that turns a live OS object into
//! these inputs (`cfg(windows)`, with `Unsupported` arms elsewhere). Logic
//! must not drift back into that arm: logic added there is logic nothing can
//! reach. `source.rs` re-exports all four, so `source::region_dims` and its
//! siblings keep their paths.

/// A window's capture size, derived from a DWM extended-frame-bounds rect.
///
/// PURE and on both platforms deliberately: `source.rs`'s `cfg(windows)` arm
/// executes in no automated test anywhere (docs/Gaps.md GAP-117), so the
/// arithmetic that decides how big a recording is opened has to live where
/// Linux can pin it.
///
/// `i64` throughout: an extended-frame-bounds rect is in screen
/// coordinates, a minimized window reports a far-off-screen one, and
/// `right - left` in `i32` would overflow rather than answer.
pub fn capture_size_from_bounds(
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> Option<(u32, u32)> {
    let width = i64::from(right) - i64::from(left);
    let height = i64::from(bottom) - i64::from(top);
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((width as u32, height as u32))
}

/// The `(crop_x, crop_y, width, height)` a WHOLE source contributes: the
/// full frame, cropped at the origin.
///
/// Trivial, and pinned by a test anyway, because the rest of the pipeline
/// depends on the zero: `pacing::usable_frame` measures from
/// `crop + want`, so a non-zero origin here would reject every frame of
/// every non-region capture with nothing but a rising drop counter to say
/// why. PURE and on both platforms for the same reason
/// `capture_size_from_bounds` is — the `cfg(windows)` arm that calls it
/// executes in no automated test anywhere (docs/Gaps.md GAP-117).
pub fn uncropped_dims(width: u32, height: u32) -> (u32, u32, u32, u32) {
    (0, 0, width, height)
}

/// The `(crop_x, crop_y, width, height)` a REGION contributes, or `None`
/// when the rectangle no longer intersects the monitor -- which is
/// `SourceGone`: the thing the user picked is not there any more.
///
/// The clamp runs HERE, at capture start and not only when the user drew
/// the rectangle, because the monitor's resolution can change in between
/// (spec 5.2); an unclamped stale rectangle indexes outside the frame
/// buffer.
///
/// PURE, and returning `uncropped_dims`' shape so the two arms of the same
/// decision read identically at their call sites, for the reason that
/// function's doc gives: the `cfg(windows)` arm that calls it executes in
/// no automated test anywhere (docs/Gaps.md GAP-117). An axis or a
/// dimension transposed in there captures the wrong rectangle in silence.
pub fn region_dims(
    rect: vault_buddy_core::screen_geometry::PhysicalRect,
    mon_w: u32,
    mon_h: u32,
) -> Option<(u32, u32, u32, u32)> {
    let r = vault_buddy_core::screen_geometry::clamp_to_frame(rect, mon_w, mon_h)?;
    Some((r.x, r.y, r.width, r.height))
}

/// What to call a monitor: its own name, or `Screen <index>` when it has
/// none we can use.
///
/// THE BUG THIS EXISTS FOR. `Monitor::name()` is a `Result<String, _>`, and
/// the obvious `.unwrap_or_else(|_| format!("Screen {index}"))` reads as
/// though it covers the no-name case. It does not: it covers `Err` only, and
/// an `Ok("")` passes straight through as a perfectly valid empty label. On
/// the machine of the 2026-09-21 manual pass that is exactly what came back,
/// so the region arm built `format!("Region on {label}")` out of nothing and
/// the user's log read `screen capture: started (Region on )`.
///
/// It did not stop at the log. The title is the capture bar's label, the
/// staging sidecar's `sourceTitle`, and — through `sanitize_title` and
/// `capture_paths::base_name` — the name of the `.mp4` the export commits
/// into a vault. `sanitize_title` trims the trailing space, so the file was
/// never malformed; it was just called `2026-09-21 1152 Region on.mp4`,
/// permanently, in somebody's notes. A dangling preposition is a small thing
/// to read and not a small thing to have written into a user's knowledge
/// base, where nothing in this app renames it (docs/Gaps.md GAP-142).
///
/// Taking `Option<&str>` rather than the `Result` is deliberate: it makes
/// the caller spell `.ok()` and leaves this function with one question to
/// answer — is there a usable name? — which is the question the three call
/// sites kept getting wrong. Whitespace-only is treated as absent for the
/// same reason empty is: neither tells a reader which screen this is.
pub fn display_label(name: Option<&str>, index: usize) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None => format!("Screen {index}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A monitor whose name comes back EMPTY must fall back, not label the
    /// source with nothing.
    ///
    /// Found by the 2026-09-21 manual Windows pass, not by any gate:
    /// `Monitor::name()` answered `Ok("")` on that machine, the three
    /// `.unwrap_or_else(|_| ...)` call sites handled only the `Err` arm, and
    /// the empty string sailed through into `format!("Region on {label}")`.
    /// The user's log read `screen capture: started (Region on )` and a file
    /// called `2026-09-21 1152 Region on.mp4` landed permanently in their
    /// vault. An `Err` and an `Ok("")` mean the same thing to a reader — we
    /// do not know what this screen is called — so they must take the same
    /// branch.
    #[test]
    fn an_empty_monitor_name_falls_back_like_a_missing_one() {
        assert_eq!(display_label(Some("DELL U2720Q"), 1), "DELL U2720Q");
        // The Err arm, which already worked.
        assert_eq!(display_label(None, 2), "Screen 2");
        // The arm that shipped: empty is not a name.
        assert_eq!(display_label(Some(""), 2), "Screen 2");
        // Whitespace-only is empty for every purpose a label has.
        assert_eq!(display_label(Some("   "), 3), "Screen 3");
        // A real name keeps its content but loses framing whitespace, which
        // would otherwise reach `sanitize_title` and the capture bar.
        assert_eq!(display_label(Some("  DELL U2720Q  "), 1), "DELL U2720Q");
    }
    use vault_buddy_core::screen_geometry::PhysicalRect;

    #[test]
    fn a_window_rect_becomes_its_capture_size() {
        // The size a window capture is OPENED at. Getting this wrong by a
        // few pixels is the whole bug this function exists for: every
        // delivered frame is then judged against a size WGC never sends.
        assert_eq!(
            capture_size_from_bounds(100, 100, 1620, 942),
            Some((1520, 842))
        );
    }

    #[test]
    fn a_rect_on_a_negative_origin_monitor_still_measures_correctly() {
        // A secondary monitor left of the primary has negative screen
        // coordinates. Treating the corners as unsigned would give a
        // nonsense size and open the recording at it.
        assert_eq!(
            capture_size_from_bounds(-1920, -100, -400, 742),
            Some((1520, 842))
        );
    }

    #[test]
    fn a_degenerate_rect_yields_no_size_rather_than_a_wrong_one() {
        // A minimized or not-yet-shown window reports an empty or inverted
        // rect. Returning 0 (or a wrapped huge number) would open a sink
        // that can never be fed.
        assert_eq!(capture_size_from_bounds(0, 0, 0, 0), None);
        assert_eq!(capture_size_from_bounds(10, 10, 10, 400), None);
        assert_eq!(capture_size_from_bounds(10, 10, 400, 10), None);
        assert_eq!(capture_size_from_bounds(400, 400, 10, 10), None);
    }

    #[test]
    fn an_extreme_rect_measures_without_overflowing() {
        // `right - left` across the full i32 range overflows i32 — which in
        // a debug build PANICS, and this runs on the capture start path.
        assert_eq!(
            capture_size_from_bounds(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
            Some((u32::MAX, u32::MAX))
        );
    }

    // A whole screen or window is the (0, 0) case of a region, and the
    // rest of the pipeline relies on that: `pacing::usable_frame` measures
    // from `crop + want`, so a non-zero default here would reject every
    // frame of every non-region capture. Constructible off Windows on
    // purpose, so the contract is pinned somewhere CI runs.
    #[test]
    fn a_whole_source_crops_at_the_origin() {
        let whole = uncropped_dims(1920, 1080);
        assert_eq!(whole, (0, 0, 1920, 1080));
    }

    // The REGION case, and the one the `cfg(windows)` arm cannot prove.
    // Every number here is DISTINCT on purpose: a symmetric rectangle (or
    // equal offsets) cannot tell an x/y or a width/height transposition
    // from a correct assignment, which is exactly how the ledger's T2/M2
    // test passed while an axis swap survived.
    #[test]
    fn a_region_keeps_its_own_origin_and_size_untransposed() {
        let rect = rect(1280, 600, 640, 480);
        assert_eq!(region_dims(rect, 1920, 1080), Some((1280, 600, 640, 480)));
    }

    #[test]
    fn a_region_is_reclamped_against_the_monitor_it_is_starting_on() {
        // Spec 5.2: the clamp runs at START, not only when the rectangle
        // was drawn, because the resolution can change in between. Without
        // it a stale rectangle indexes outside the frame buffer, and the
        // only thing left standing between it and an out-of-bounds read is
        // `convert`'s own guard -- i.e. every frame dropped, with nothing
        // saying why.
        let rect = rect(1280, 600, 640, 480);
        assert_eq!(region_dims(rect, 1600, 900), Some((1280, 600, 320, 300)));
    }

    #[test]
    fn a_region_that_no_longer_touches_its_monitor_resolves_to_nothing() {
        // `None` is what the arm turns into `SourceGone`: the thing the
        // user picked is not there any more. Returning a clamped-to-zero
        // rectangle instead would open a sink that can never be fed.
        let rect = rect(1280, 600, 640, 480);
        assert_eq!(region_dims(rect, 1000, 1000), None);
    }

    #[test]
    fn a_regions_size_is_rounded_to_even_but_its_origin_is_not() {
        // NV12 has no way to express an odd dimension, so the SIZE rounds
        // down; the ORIGIN must not, because rounding it would silently
        // move the rectangle the user drew (`convert`'s own doc).
        assert_eq!(
            region_dims(rect(3, 5, 7, 9), 1920, 1080),
            Some((3, 5, 6, 8))
        );
    }

    fn rect(x: u32, y: u32, width: u32, height: u32) -> PhysicalRect {
        PhysicalRect {
            x,
            y,
            width,
            height,
        }
    }
}
