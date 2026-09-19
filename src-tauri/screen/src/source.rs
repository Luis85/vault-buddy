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
}

/// A capture target, in the form that crosses the IPC boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceId {
    /// Monitor index as `windows_capture::monitor::Monitor::index` reports it.
    Screen(usize),
    /// `HWND` widened to `isize`. Signed on purpose: an HWND is a pointer,
    /// and on a 64-bit process its high bit can be set.
    Window(isize),
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
            // "region" deliberately absent: region capture is phase 3, and
            // accepting an id we cannot honour would start the wrong capture.
            _ => None,
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            SourceId::Screen(_) => SourceKind::Screen,
            SourceId::Window(_) => SourceKind::Window,
        }
    }
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceId::Screen(i) => write!(f, "screen:{i}"),
            SourceId::Window(h) => write!(f, "window:{h}"),
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
    use windows_capture::monitor::Monitor;
    use windows_capture::window::Window;

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
                    let (Ok(width), Ok(height)) = (w.width(), w.height()) else {
                        continue;
                    };
                    // A zero-sized or negative-sized window cannot be
                    // captured; offering it would produce a start that fails
                    // for a reason the user cannot see.
                    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height))
                    else {
                        continue;
                    };
                    if width == 0 || height == 0 {
                        continue;
                    }
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
                let monitor = Monitor::from_index(index).map_err(|e| {
                    log::warn!("screen source: monitor {index} is gone: {e}");
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
            SourceId::Window(hwnd) => {
                let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
                if !window.is_valid() {
                    log::warn!("screen source: window {hwnd} is gone");
                    return Err(ScreenError::SourceGone);
                }
                let (Ok(width), Ok(height)) = (window.width(), window.height()) else {
                    return Err(ScreenError::SourceGone);
                };
                let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
                    return Err(ScreenError::SourceGone);
                };
                if width == 0 || height == 0 {
                    return Err(ScreenError::SourceGone);
                }
                let title = window.title().unwrap_or_else(|_| "Window".to_string());
                Ok(ResolvedSource {
                    handle: SourceHandle::Window(window),
                    width,
                    height,
                    title,
                })
            }
        }
    }
}

pub use imp::{list_sources, resolve, SourceHandle};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_id_round_trips() {
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
            "region arrives in phase 3"
        );
        assert_eq!(SourceId::parse(""), None);
        assert_eq!(SourceId::parse("window:1:2"), None);
    }

    #[test]
    fn the_kind_serializes_lowercase_for_the_webview_and_the_sidecar() {
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
