//! What can be captured: monitors and top-level windows.
//!
//! The id encoding is PURE and lives on both platforms deliberately. A
//! `*mut c_void` cannot cross the IPC boundary and a bare list index is not
//! stable across a re-enumeration, so a source is addressed by a string the
//! webview holds and hands back. That string is untrusted input by the time
//! it returns, which is why `SourceId::parse` is strict: a lenient parse
//! that fell back to a default would start a capture of something the user
//! did not pick.
//!
//! Enumeration DEGRADES to an empty list rather than erroring, following
//! `discovery`'s precedent — an empty state with a Refresh is actionable, a
//! red banner about a WinRT error is not.

use serde::Serialize;

use crate::ScreenError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Screen,
    Window,
    Region,
}

/// A capture target, in the form that crosses the IPC boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceId {
    /// Monitor index as `windows_capture::monitor::Monitor::index` reports it.
    Screen(usize),
    /// `HWND` widened to `isize`. Signed on purpose: an HWND is a pointer,
    /// and on a 64-bit process its high bit can be set.
    Window(isize),
    /// A rectangle on one monitor (phase 3). Carrying the rectangle IN the
    /// id is what lets `start_screen_capture` stay a four-parameter
    /// command: the picker holds one opaque string for every source kind.
    Region(crate::region::RegionSource),
}

impl SourceId {
    pub fn parse(s: &str) -> Option<SourceId> {
        let (kind, rest) = s.split_once(':')?;
        if rest.is_empty() || rest.contains(':') {
            return None;
        }
        match kind {
            "screen" => rest.parse::<usize>().ok().map(SourceId::Screen),
            "window" => rest.parse::<isize>().ok().map(SourceId::Window),
            // Strictness is delegated whole to region::parse_payload, so
            // there is exactly one definition of a legal region id.
            "region" => crate::region::parse_payload(rest).map(SourceId::Region),
            _ => None,
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            SourceId::Screen(_) => SourceKind::Screen,
            SourceId::Window(_) => SourceKind::Window,
            SourceId::Region(_) => SourceKind::Region,
        }
    }
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceId::Screen(i) => write!(f, "screen:{i}"),
            SourceId::Window(h) => write!(f, "window:{h}"),
            SourceId::Region(r) => write!(f, "region:{}", crate::region::encode_payload(*r)),
        }
    }
}

/// One row in the picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSourceInfo {
    pub id: String,
    pub kind: SourceKind,
    /// What the user reads first: a monitor label or a window title.
    pub title: String,
    /// The secondary line: resolution and primary flag, or the owning
    /// process.
    pub detail: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// A window's capture size, derived from a DWM extended-frame-bounds rect.
///
/// PURE and on both platforms deliberately: the `cfg(windows)` arm below
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

/// A source re-checked at START time and ready to capture.
pub struct ResolvedSource {
    pub handle: SourceHandle,
    pub width: u32,
    pub height: u32,
    pub title: String,
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    /// Off Windows there is nothing to hold.
    pub enum SourceHandle {}

    pub fn list_sources(_exclude_titles: &[String]) -> Vec<CaptureSourceInfo> {
        log::debug!("screen source: enumeration is Windows-only; returning an empty list");
        Vec::new()
    }

    pub fn resolve(_id: &SourceId) -> Result<ResolvedSource, ScreenError> {
        Err(ScreenError::Unsupported)
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows_capture::monitor::Monitor;
    use windows_capture::window::Window;

    /// The size to OPEN a window recording at: what WGC will actually
    /// deliver, not what `GetWindowRect` reports.
    ///
    /// THE BUG THIS EXISTS FOR. `Window::width()/height()`
    /// (windows-capture 2.0.1, `window.rs`) are `GetWindowRect`, which on
    /// Windows 10/11 "may include invisible resize borders" (its own MSDN
    /// Remarks) — roughly 7-8 px left, right and bottom of a normal
    /// top-level window. WGC sizes its frame pool from the capture item
    /// (`graphics_capture_api.rs` creates it at `item.Size()` and recreates
    /// it at `frame.ContentSize()`), which is the DWM-COMPOSED size and so
    /// is SMALLER. `pacing::usable_frame` then rejected every single frame
    /// as undersized, the sink was handed nothing, and finalize failed with
    /// a bare `MF_E_SINK_NO_SAMPLES_PROCESSED` (0xC00D4A44) over a
    /// zero-frame file. Monitors were never affected: `Monitor::width()` is
    /// `dmPelsWidth`, which matches WGC exactly.
    ///
    /// `DWMWA_EXTENDED_FRAME_BOUNDS` is what MSDN names as the way to get
    /// "the visible window bounds, not including the invisible resize
    /// borders", and it is the rect the WGC texture corresponds to:
    /// OBS's own WGC backend computes its client-area crop box INSIDE the
    /// capture texture as offsets from this rect
    /// (`libobs-winrt/winrt-capture.cpp`, `get_client_box`), and the
    /// vendored crate's `Window::title_bar_height` crops the captured frame
    /// by this rect's height minus the client height. Neither would be
    /// correct if the frame were any other size.
    ///
    /// STILL A PREDICTION, though — Microsoft documents no equality between
    /// the two, which is why a residual mismatch now diagnoses itself
    /// (`crate::diagnose`) instead of producing an opaque zero-frame file;
    /// docs/Gaps.md GAP-122 records the alternative that would end the
    /// guessing.
    fn window_capture_dims(window: &Window) -> Option<(u32, u32)> {
        let mut rect = RECT::default();
        let bounds = unsafe {
            DwmGetWindowAttribute(
                HWND(window.as_raw_hwnd()),
                DWMWA_EXTENDED_FRAME_BOUNDS,
                std::ptr::from_mut(&mut rect).cast(),
                std::mem::size_of::<RECT>() as u32,
            )
        };
        match bounds {
            Ok(()) => {
                if let Some(dims) =
                    super::capture_size_from_bounds(rect.left, rect.top, rect.right, rect.bottom)
                {
                    return Some(dims);
                }
                log::debug!("screen source: a window reported an empty extended frame bounds");
            }
            Err(e) => log::warn!("screen source: could not read a window's frame bounds: {e}"),
        }
        // DEGRADE rather than drop the window: GetWindowRect is all that is
        // left, and a source missing from the picker is worse than one
        // whose declared size may be a few pixels generous. A capture that
        // then delivers nothing explains itself at finalize (Fix B) instead
        // of failing opaquely.
        let (Ok(w), Ok(h)) = (window.width(), window.height()) else {
            return None;
        };
        // Through the SAME pure conversion, so the non-positive and
        // overflow rules have exactly one definition.
        super::capture_size_from_bounds(0, 0, w, h)
    }

    pub enum SourceHandle {
        Screen(Monitor),
        Window(Window),
    }

    pub fn list_sources(exclude_titles: &[String]) -> Vec<CaptureSourceInfo> {
        let mut out = Vec::new();

        match Monitor::enumerate() {
            Ok(monitors) => {
                for m in monitors {
                    // Every accessor is fallible (a monitor can be unplugged
                    // between enumerate and read). One unreadable monitor
                    // must not empty the whole list.
                    let (Ok(index), Ok(width), Ok(height)) = (m.index(), m.width(), m.height())
                    else {
                        log::warn!("screen source: skipping an unreadable monitor");
                        continue;
                    };
                    let is_primary = index == 1; // Monitor::index is 1-based
                    let title = m.name().unwrap_or_else(|_| format!("Screen {index}"));
                    let detail = if is_primary {
                        format!("{width}x{height} - Primary")
                    } else {
                        format!("{width}x{height}")
                    };
                    out.push(CaptureSourceInfo {
                        id: SourceId::Screen(index).to_string(),
                        kind: SourceKind::Screen,
                        title,
                        detail,
                        width,
                        height,
                        is_primary,
                    });
                }
            }
            Err(e) => log::warn!("screen source: monitor enumeration failed: {e}"),
        }

        match Window::enumerate() {
            Ok(windows) => {
                for w in windows {
                    let Ok(title) = w.title() else { continue };
                    if title.trim().is_empty() {
                        continue;
                    }
                    // Spec 7.2: Vault Buddy's own windows are filtered out.
                    if exclude_titles.iter().any(|t| t == &title) {
                        continue;
                    }
                    // A zero-sized or negative-sized window cannot be
                    // captured; offering it would produce a start that fails
                    // for a reason the user cannot see. The picker must also
                    // show the size the RECORDING will have, so this is the
                    // same DWM-composed measurement `resolve` declares.
                    let Some((width, height)) = window_capture_dims(&w) else {
                        continue;
                    };
                    let detail = w.process_name().unwrap_or_default();
                    out.push(CaptureSourceInfo {
                        id: SourceId::Window(w.as_raw_hwnd() as isize).to_string(),
                        kind: SourceKind::Window,
                        title,
                        detail,
                        width,
                        height,
                        is_primary: false,
                    });
                }
            }
            Err(e) => log::warn!("screen source: window enumeration failed: {e}"),
        }

        out
    }

    /// Re-check the picked source AT START TIME (spec 14). The list the user
    /// clicked may be seconds stale: a window can have closed, a monitor can
    /// have been unplugged. A started-then-dead capture is the outcome this
    /// exists to prevent.
    pub fn resolve(id: &SourceId) -> Result<ResolvedSource, ScreenError> {
        match *id {
            SourceId::Screen(index) => {
                // DELIBERATELY NOT `Monitor::from_index(index)`. That method
                // (windows-capture 2.0.1, monitor.rs) is POSITIONAL: it
                // indexes the Nth element of a fresh `Monitor::enumerate()`.
                // But `list_sources` above labels a screen with `m.index()`,
                // which is the OS DISPLAY NUMBER parsed out of the device
                // name (`\\.\DISPLAYn`). `EnumDisplayMonitors` order is not
                // documented to match ascending `\\.\DISPLAYn`, so on a
                // multi-monitor machine "the 2nd enumerated monitor" and
                // "display number 2" can be two different, both-live
                // monitors — `from_index` would silently resolve to the
                // WRONG screen with no error, not just a stale one. Scan for
                // the monitor whose `.index()` (device-name-derived) equals
                // the stored id instead, so the two numbering schemes never
                // get crossed. If nothing matches, the screen really is gone
                // (unplugged) and `SourceGone` is the correct outcome.
                let monitors = Monitor::enumerate().map_err(|e| {
                    log::warn!("screen source: monitor enumeration failed: {e}");
                    ScreenError::SourceGone
                })?;
                let monitor = monitors
                    .into_iter()
                    .find(|m| match m.index() {
                        Ok(i) => i == index,
                        // One unreadable monitor must not abort the scan —
                        // same skip-and-continue posture as list_sources.
                        Err(_) => false,
                    })
                    .ok_or_else(|| {
                        log::warn!("screen source: monitor {index} is gone");
                        ScreenError::SourceGone
                    })?;
                let (Ok(width), Ok(height)) = (monitor.width(), monitor.height()) else {
                    return Err(ScreenError::SourceGone);
                };
                let title = monitor.name().unwrap_or_else(|_| format!("Screen {index}"));
                Ok(ResolvedSource {
                    handle: SourceHandle::Screen(monitor),
                    width,
                    height,
                    title,
                })
            }
            // HWND-REUSE HAZARD (documented, not fixed here — Fix 2 is a
            // Windows-verification item, not a code change): Windows can
            // reuse an HWND after the window that owned it closes. Between
            // `list_sources` handing out this id and a later `resolve` at
            // Start, the same handle may now belong to an entirely
            // different, unrelated live window — `is_valid()` below only
            // checks that SOME window currently owns the handle and is
            // capturable, not that it's the SAME window the user picked.
            // Do NOT "fix" this with a title re-check: window titles change
            // constantly and legitimately (a browser tab navigating, an
            // editor's title reflecting the current file), so a strict
            // title match would produce false SourceGone rejections on
            // perfectly valid, still-correct windows — trading a rare
            // silent-wrong-capture for a common false-failure is a worse
            // day-to-day bug. Confirming "the captured window is the one
            // the user picked" needs an approach that survives legitimate
            // title changes (e.g. re-deriving identity from something more
            // stable than title text) and is left as a Windows-verification
            // item; behaviour here is unchanged.
            SourceId::Window(hwnd) => {
                let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
                if !window.is_valid() {
                    log::warn!("screen source: window {hwnd} is gone");
                    return Err(ScreenError::SourceGone);
                }
                // The size the sink is OPENED at, so it must be what WGC
                // will deliver — see `window_capture_dims`.
                let Some((width, height)) = window_capture_dims(&window) else {
                    return Err(ScreenError::SourceGone);
                };
                let title = window.title().unwrap_or_else(|_| "Window".to_string());
                Ok(ResolvedSource {
                    handle: SourceHandle::Window(window),
                    width,
                    height,
                    title,
                })
            }
            // Task 3 replaces this: resolving a region needs the monitor
            // lookup this match already does for `Screen`, then clamping
            // the rectangle to that monitor's frame.
            SourceId::Region(_) => Err(ScreenError::Unsupported),
        }
    }
}

pub use imp::{list_sources, resolve, SourceHandle};

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn a_screen_id_round_trips() {
        // If Display/parse ever drift, a stored id would no longer name the
        // same screen it was minted from — `resolve` would then either miss
        // a live monitor (spurious SourceGone) or, worse, silently match a
        // DIFFERENT monitor whose index happens to parse the same way.
        let id = SourceId::Screen(2);
        assert_eq!(id.to_string(), "screen:2");
        assert_eq!(SourceId::parse("screen:2"), Some(SourceId::Screen(2)));
    }

    #[test]
    fn a_window_id_round_trips_including_a_negative_handle() {
        // An HWND is a pointer widened to isize. On a 64-bit process the
        // high bit can be set, so the decimal form can be negative; parsing
        // it as unsigned would silently address a different window.
        let id = SourceId::Window(-1_234_567);
        assert_eq!(id.to_string(), "window:-1234567");
        assert_eq!(
            SourceId::parse("window:-1234567"),
            Some(SourceId::Window(-1_234_567))
        );
    }

    #[test]
    fn parse_rejects_anything_it_does_not_fully_understand() {
        // The id crosses the IPC boundary, so it is untrusted input. A
        // lenient parse that fell back to a default would start a capture of
        // something the user did not pick.
        assert_eq!(SourceId::parse("screen"), None);
        assert_eq!(SourceId::parse("screen:"), None);
        assert_eq!(SourceId::parse("screen:abc"), None);
        assert_eq!(
            SourceId::parse("screen:-1"),
            None,
            "a monitor index is never negative"
        );
        assert_eq!(
            SourceId::parse("region:0"),
            None,
            "a region payload needs all five fields, not just a monitor"
        );
        assert_eq!(SourceId::parse(""), None);
        assert_eq!(SourceId::parse("window:1:2"), None);
    }

    #[test]
    fn the_kind_serializes_lowercase_for_the_webview_and_the_sidecar() {
        // A casing regression here breaks the TS contract at the IPC
        // boundary: the frontend matches on the literal strings "screen"/
        // "window", so "Screen"/"Window" would silently fail every kind
        // comparison in the picker.
        assert_eq!(
            serde_json::to_string(&SourceKind::Screen).unwrap(),
            "\"screen\""
        );
        assert_eq!(
            serde_json::to_string(&SourceKind::Window).unwrap(),
            "\"window\""
        );
    }

    #[test]
    fn source_info_serializes_camel_case() {
        // Same IPC-boundary contract as the kind test above: a regression
        // to snake_case here would silently break the TS side's `isPrimary`
        // field access (it would read `undefined`, not throw), so the
        // primary badge would just stop appearing with no visible error.
        let info = CaptureSourceInfo {
            id: "screen:0".into(),
            kind: SourceKind::Screen,
            title: "Screen 1".into(),
            detail: "2560x1440 - Primary".into(),
            width: 2560,
            height: 1440,
            is_primary: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"isPrimary\":true"), "got {json}");
        assert!(!json.contains("is_primary"), "got {json}");
    }

    #[cfg(not(windows))]
    #[test]
    fn listing_sources_off_windows_degrades_to_empty_rather_than_erroring() {
        // The discovery precedent: an unavailable source list shows an empty
        // state with a Refresh, never a banner the user cannot act on. This
        // also keeps the Linux compile gate honest about what it proves.
        assert!(list_sources(&[]).is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn resolving_off_windows_reports_unsupported() {
        assert!(matches!(
            resolve(&SourceId::Screen(0)),
            Err(crate::ScreenError::Unsupported)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn listing_sources_on_windows_never_panics_and_labels_everything() {
        // A CI runner's session may have no capturable windows at all, so
        // this asserts shape, not contents: whatever comes back is fully
        // labelled and carries a parseable id.
        for info in list_sources(&[]) {
            assert!(!info.title.is_empty(), "every source is named");
            assert!(
                SourceId::parse(&info.id).is_some(),
                "id parses: {}",
                info.id
            );
        }
    }

    // Phase 3: a region is a first-class source id, so the same untrusted
    // string the webview hands back carries the whole rectangle and
    // `start_screen_capture` needs no new parameter.
    #[test]
    fn a_region_source_id_round_trips() {
        let id = SourceId::Region(crate::region::RegionSource {
            monitor: 2,
            rect: vault_buddy_core::screen_geometry::PhysicalRect {
                x: 320,
                y: 180,
                width: 1280,
                height: 720,
            },
        });
        assert_eq!(id.to_string(), "region:2,320,180,1280,720");
        assert_eq!(SourceId::parse("region:2,320,180,1280,720"), Some(id));
        assert_eq!(id.kind(), SourceKind::Region);
    }

    #[test]
    fn a_malformed_region_source_id_is_refused() {
        // Delegated strictness: whatever region::parse_payload refuses,
        // SourceId::parse must refuse too, rather than falling back to a
        // screen or a default.
        assert_eq!(SourceId::parse("region:"), None);
        assert_eq!(SourceId::parse("region:2,320,180,1280"), None);
        assert_eq!(SourceId::parse("region:2,320,180,1280,0"), None);
    }

    // The existing kinds must be untouched by the new arm.
    #[test]
    fn screen_and_window_ids_still_round_trip_unchanged() {
        assert_eq!(SourceId::parse("screen:3"), Some(SourceId::Screen(3)));
        assert_eq!(SourceId::Screen(3).to_string(), "screen:3");
        assert_eq!(SourceId::parse("window:-42"), Some(SourceId::Window(-42)));
        assert_eq!(SourceId::Window(-42).to_string(), "window:-42");
        assert_eq!(SourceId::parse("region"), None, "no separator");
        assert_eq!(SourceId::parse("nonsense:1"), None);
    }

    #[cfg(windows)]
    #[test]
    fn our_own_windows_are_filtered_out_by_title() {
        // Spec 7.2: Vault Buddy's own windows must not appear in the picker.
        // (Phase 3 additionally hides them from the RECORDING itself via
        // WDA_EXCLUDEFROMCAPTURE; this is only the picker.)
        let all = list_sources(&[]);
        if let Some(first_window) = all.iter().find(|s| s.kind == SourceKind::Window) {
            let filtered = list_sources(std::slice::from_ref(&first_window.title));
            assert!(
                !filtered.iter().any(|s| s.id == first_window.id),
                "an excluded title must not come back"
            );
        }
    }
}
