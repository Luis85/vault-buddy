//! The region-capture indicator (GAP-165, spec
//! `2026-09-20-region-capture-indicator-design.md` as amended 2026-09-21).
//!
//! While a REGION capture runs, the recorded rectangle is outlined on the
//! user's screen. Windows' own yellow border surrounds the whole monitor,
//! because WGC really is capturing the whole monitor and
//! `convert::bgra_crop_to_nv12` crops on the CPU -- so that border is
//! truthful about what Windows captures and useless about what we record.
//! This one is ours.
//!
//! **Why drawing over the recorded area is free.** The window is in
//! `capture_exclusion::EXCLUDED_LABELS`, so it is invisible to screen
//! capture while fully visible to the user. That membership is hardware
//! evidence, not a declaration -- see that module and docs/Gaps.md GAP-165.
//!
//! **Paired state, one site each way**, the `capture_exclusion` shape and
//! for the same reason: a leaked indicator is a border the user cannot
//! dismiss with no capture behind it, and both natural relocations of these
//! two calls leak permanently while keeping the counts right. Pinned by the
//! structural tests below.
//!
//! **Best-effort throughout.** A failed indicator must never fail a
//! capture: the recording proceeds without a border and a `log::warn!`
//! records why. This differs from `region:begin`'s hard-error treatment
//! deliberately -- an un-armed overlay IS an unusable full-screen scrim,
//! whereas a missing border merely returns the user to today's behaviour.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use vault_buddy_core::screen_geometry::indicator_bounds;
use vault_buddy_screen::region::RegionSource;
use vault_buddy_screen::source::SourceId;

const INDICATOR_LABEL: &str = "region-indicator";

/// The gate: only a REGION capture raises an indicator.
///
/// Windows' own border is already truthful for a screen or a window capture
/// -- it surrounds exactly what is being captured -- so a second border
/// there would be noise. Extracted as a pure function because `show` needs
/// an `AppHandle` and therefore executes in no automated test on any
/// platform; this way the gate itself is pinned on Linux.
pub(crate) fn indicator_rect(source: &SourceId) -> Option<RegionSource> {
    match source {
        SourceId::Region(r) => Some(*r),
        SourceId::Screen(_) | SourceId::Window(_) => None,
    }
}

/// Raise the border over the recorded rectangle. THE ONE SHOW SITE is
/// `screen_capture_worker`'s start path, beside the exclusion apply.
///
/// **Two threads, deliberately (spec amendment A7).** The caller is a
/// worker -- `start_screen_capture_blocking` runs under `spawn_blocking` --
/// which is the only reason `available_monitors` is safe here: it marshals
/// to the event loop and blocks on the reply with no timeout, so calling it
/// from inside the closure below would be the main thread waiting on a
/// reply only it can produce. Everything needing the event loop is computed
/// FIRST and carried in as four plain numbers.
pub(crate) fn show(app: &AppHandle, source: &SourceId) {
    let Some(region) = indicator_rect(source) else {
        return;
    };
    let monitors = match app.available_monitors() {
        Ok(m) => m,
        Err(e) => {
            log::warn!("region indicator: could not read the display layout: {e}");
            return;
        }
    };
    // The ONE join between Tauri's monitor naming and windows-capture's
    // index lives in `region_commands`; never a second copy, because its
    // own doc records that phase 2 shipped a wrong-screen bug by crossing
    // exactly these two schemes.
    let Some(monitor) = monitors.iter().find(|m| {
        crate::region_commands::matches_display(m.name().map(String::as_str), region.monitor)
    }) else {
        log::warn!(
            "region indicator: no monitor for GDI display {}; the border is skipped",
            region.monitor
        );
        return;
    };
    let origin = *monitor.position();
    let (x, y, w, h) = indicator_bounds((origin.x, origin.y), region.rect);

    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || place_and_show(&handle, x, y, w, h)) {
        log::warn!("region indicator: could not reach the main thread: {e}");
    }
}

/// The main-thread half: window calls only.
///
/// **The ordering is a rule, not a preference.** Click-through FIRST, and a
/// failure there returns without showing anything: a transparent
/// always-on-top window that takes clicks swallows every one of them over
/// the whole region for the whole capture, which the design calls strictly
/// worse than no indicator at all. Then size and position WHILE HIDDEN,
/// then show -- the discipline every companion window follows, because a
/// visible window repaints its stale last frame at the new bounds for a
/// frame.
fn place_and_show(app: &AppHandle, x: i32, y: i32, w: u32, h: u32) {
    let Some(window) = app.get_webview_window(INDICATOR_LABEL) else {
        log::warn!("region indicator: window `{INDICATOR_LABEL}` is not declared");
        return;
    };
    if let Err(e) = window.set_ignore_cursor_events(true) {
        log::warn!(
            "region indicator: could not make the border click-through, so it is not shown: {e}"
        );
        return;
    }
    if let Err(e) = window.set_size(PhysicalSize::new(w, h)) {
        log::warn!("region indicator: could not size the border: {e}");
        return;
    }
    if let Err(e) = window.set_position(PhysicalPosition::new(x, y)) {
        log::warn!("region indicator: could not place the border: {e}");
        return;
    }
    if let Err(e) = window.show() {
        log::warn!("region indicator: could not show the border: {e}");
    }
}

/// Take the border down. THE ONE HIDE SITE is
/// `screen_commands::clear_active_screen`.
///
/// **Unconditional**, exactly like the exclusion clear beside it: hiding an
/// indicator that was never shown is a no-op, which is strictly safer than
/// a conditional that could be wrong in the other direction and strand a
/// border on the user's desktop with no capture behind it.
pub(crate) fn hide(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(INDICATOR_LABEL) {
            if let Err(e) = window.hide() {
                log::warn!("region indicator: could not hide the border: {e}");
            }
        }
    }) {
        log::warn!("region indicator: could not reach the main thread to hide: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural_scan::{call_sites, offset_of, production_half};
    // `use super::*` already brings `SourceId` and `RegionSource` in from
    // the parent's imports; only `PhysicalRect` is new here.
    use vault_buddy_core::screen_geometry::PhysicalRect;

    fn region(monitor: usize) -> SourceId {
        SourceId::Region(RegionSource {
            monitor,
            rect: PhysicalRect {
                x: 10,
                y: 20,
                width: 300,
                height: 400,
            },
        })
    }

    // The gate, extracted precisely so it can be tested off Windows: `show`
    // itself takes an AppHandle and executes in no automated test anywhere.
    // A border drawn around a FULL-SCREEN capture is cosmetic-but-confusing
    // and would otherwise surface only on hardware -- and Windows' own
    // border is already truthful for a screen or window capture, so a
    // second one there is noise.
    #[test]
    fn only_a_region_source_raises_the_indicator() {
        assert!(indicator_rect(&SourceId::Screen(1)).is_none());
        assert!(indicator_rect(&SourceId::Window(0x1234)).is_none());
        let got = indicator_rect(&region(2)).expect("a region raises the indicator");
        assert_eq!(got.monitor, 2);
        assert_eq!(got.rect.width, 300);
        assert_eq!(got.rect.height, 400);
    }

    // The `capture_exclusion` pattern, for the identical reason: a leaked
    // indicator is a border the user cannot dismiss with no capture behind
    // it, and a file-granular assertion would let both natural relocations
    // pass while leaking permanently.
    #[test]
    fn the_indicator_is_shown_and_hidden_from_exactly_one_place_each() {
        let shows = call_sites("region_indicator::show(");
        let hides = call_sites("region_indicator::hide(");
        assert_eq!(
            shows.len(),
            1,
            "expected exactly one show site, found {shows:?}"
        );
        assert_eq!(
            hides.len(),
            1,
            "expected exactly one hide site, found {hides:?}"
        );
        assert!(
            shows[0].ends_with("screen_capture_worker.rs"),
            "the show must sit in start_screen_capture_blocking, beside the exclusion apply \
             and after the reservation -- but it is in {}",
            shows[0]
        );
        assert!(
            hides[0].ends_with("screen_commands.rs"),
            "the hide must sit in clear_active_screen, the one chokepoint every teardown \
             funnels through -- but it is in {}",
            hides[0]
        );

        let worker = production_half(include_str!("screen_capture_worker.rs"));
        let blocking_start = offset_of(worker, "fn start_screen_capture_blocking(");
        let show_call = offset_of(worker, "crate::region_indicator::show(");
        let after_blocking_start = offset_of(worker, "\nfn describe_screen_error(");
        assert!(
            blocking_start < show_call && show_call < after_blocking_start,
            "the show must sit INSIDE start_screen_capture_blocking. In start_screen_capture's \
             async tail it can land AFTER a self-finalized capture has already hidden the \
             indicator, leaving a border on the desktop with nothing recording"
        );

        let commands = production_half(include_str!("screen_commands.rs"));
        let chokepoint = offset_of(commands, "fn clear_active_screen(");
        let hide_call = offset_of(commands, "crate::region_indicator::hide(");
        let after_chokepoint = offset_of(commands, "\npub fn is_capturing(");
        assert!(
            chokepoint < hide_call && hide_call < after_chokepoint,
            "the hide must sit INSIDE clear_active_screen. Moved into stop_screen_capture it \
             is skipped entirely by a self-finalized capture, stranding the border"
        );
    }

    // Spec amendment A7. `available_monitors` marshals to the event loop
    // and blocks on the reply with no timeout -- `region_commands` warns
    // about exactly this at its own call site. Called from inside a
    // `run_on_main_thread` closure, the main thread waits for a reply only
    // it can produce, while executing the closure that is blocking it.
    //
    // The symptom is a HANG on every region capture start: no crash, no log
    // line, no crash record, and `start_screen_capture_blocking` never
    // returning. Structural, because this function executes in no automated
    // test on any platform.
    #[test]
    fn the_monitor_enumeration_happens_before_the_main_thread_closure() {
        let src = production_half(include_str!("region_indicator.rs"));
        let enumerate = offset_of(src, "available_monitors()");
        let marshal = offset_of(src, "run_on_main_thread(");
        assert!(
            enumerate < marshal,
            "available_monitors() must be called on the CALLING thread -- a worker, since \
             start_screen_capture_blocking runs under spawn_blocking -- and never from inside \
             run_on_main_thread. It marshals to the event loop and blocks with no timeout, so \
             the main thread would deadlock waiting on itself. Spec amendment A7"
        );
    }
}
