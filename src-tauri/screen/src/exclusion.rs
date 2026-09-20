//! Spec 5.3: keep Vault Buddy's own windows out of a screen recording.
//!
//! `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` (Windows 10
//! 2004+) makes a window invisible to screen capture while it stays fully
//! visible to the user -- exactly what the buddy needs, since it is the
//! recording indicator and must not also be in every recording.
//!
//! **Why this call lives in this crate rather than in the shell, where its
//! only caller is.** The shell crate cannot be cross-compiled to Windows on
//! a Linux box at any scope: `ring`, pulled in by the updater plugin's
//! rustls, fails in `cc-rs` looking for MSVC's `lib.exe`. A `cfg(windows)`
//! FFI call written there would be checked by nothing until CI's Windows
//! job built it. This crate cross-checks cleanly, so the one `unsafe` call
//! sits here and the shell keeps only the Tauri-side glue.
//!
//! **A failure is never an error.** On a pre-2004 build the call fails, the
//! capture proceeds with the buddy in frame, and that is the documented
//! degraded behaviour (spec 5.3), not a reason to refuse a start.

use crate::ScreenError;

/// The `WINDOW_DISPLAY_AFFINITY` value for each state: 17
/// (`WDA_EXCLUDEFROMCAPTURE`) or 0 (`WDA_NONE`).
///
/// Pure, and on both platforms, so the constants are pinned somewhere that
/// runs. `WDA_MONITOR` (1) is deliberately not offered -- it hides a window
/// from capture AND from remote desktop sessions, which is not what spec
/// 5.3 asks for.
pub fn affinity_value(excluded: bool) -> u32 {
    if excluded {
        17
    } else {
        0
    }
}

/// Set one window's display affinity. `hwnd` is a raw window handle widened
/// to `isize` -- signed, like `SourceId::Window`, because an HWND is a
/// pointer whose high bit can be set in a 64-bit process.
///
/// Taking a raw handle rather than a Tauri window type is what keeps this
/// crate free of a Tauri dependency, and is why the call can be type-checked
/// for the Windows target at all.
#[cfg(windows)]
pub fn set_display_affinity(hwnd: isize, excluded: bool) -> Result<(), ScreenError> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
    };
    // SAFETY: `hwnd` is a live top-level window handle owned by this
    // process, obtained from Tauri on the thread that owns it, and the
    // affinity is one of the two documented WINDOW_DISPLAY_AFFINITY values.
    unsafe {
        SetWindowDisplayAffinity(
            HWND(hwnd as *mut std::ffi::c_void),
            WINDOW_DISPLAY_AFFINITY(affinity_value(excluded)),
        )
    }
    .map_err(|e| ScreenError::Io(e.to_string()))
}

#[cfg(not(windows))]
pub fn set_display_affinity(_hwnd: isize, _excluded: bool) -> Result<(), ScreenError> {
    Err(ScreenError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    // WDA_EXCLUDEFROMCAPTURE is 17 and WDA_NONE is 0 (windows 0.62.2,
    // Win32/UI/WindowsAndMessaging/mod.rs:6692 and :6694). Written as bare
    // literals on purpose: this test is a cross-check ON the constant, so
    // importing the constant to compare against itself would assert
    // nothing -- and off Windows the constant is not even in scope, which
    // is precisely where this test runs.
    #[test]
    fn the_affinity_values_are_the_documented_windows_constants() {
        assert_eq!(affinity_value(true), 17);
        assert_eq!(affinity_value(false), 0);
    }
}
