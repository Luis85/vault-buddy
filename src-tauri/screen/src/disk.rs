//! Free space on the volume holding a path.
//!
//! Returns `Option<u64>`, never `Result`, and that is a deliberate API
//! choice rather than laziness: the ONE thing this probe must never do is
//! cause a save to be refused because the measurement failed. A `Result`
//! invites a caller to write `free.unwrap_or(0)`, which turns every
//! unmeasurable volume — and every non-Windows build — into a permanently
//! full disk. `None` means "unknown"; `core::screen_capture_config::
//! export_space_shortfall` consumes it and lets the save through.
//!
//! Signature note, `windows` 0.62.2: `GetDiskFreeSpaceExW`'s three out-params
//! are `Option<*mut u64>` -- RAW POINTERS, not `Option<&mut u64>` -- and the
//! path parameter is generic over `Param<PCWSTR>`, which `&HSTRING`
//! satisfies. It returns `windows_core::Result<()>`, already `.ok()`-mapped
//! from the underlying `BOOL`, so a failure arrives as `Err` rather than as
//! a `false` that has to be paired with `GetLastError`.

use std::path::Path;

#[cfg(windows)]
pub fn free_bytes(path: &Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // GetDiskFreeSpaceExW wants a directory (or any path on the volume).
    let wide = HSTRING::from(path.as_os_str().to_string_lossy().as_ref());
    let mut available: u64 = 0;
    // SAFETY: `wide` outlives the call, and the one out-param we ask for
    // points at a live local `u64`. The three-`Option` signature takes RAW
    // pointers (`Option<*mut u64>`), not references, so the cast is what
    // makes the borrow's address the out-param -- see the module's
    // signature note.
    let ok = unsafe { GetDiskFreeSpaceExW(&wide, Some(&mut available as *mut u64), None, None) };
    match ok {
        Ok(()) => Some(available),
        Err(e) => {
            log::warn!(
                "screen export: could not measure free space at {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// The Linux compile gate and the test suite. "Unknown", which by the rule
/// above means the save proceeds.
#[cfg(not(windows))]
pub fn free_bytes(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    // The import rides the SAME cfg as the only test below. On a Windows
    // target that test is absent (the probe needs a real volume to assert
    // anything), leaving the module empty -- and an unconditional
    // `use super::*` there is an unused-import error under the
    // windows-target clippy gate, which is the one check this file's
    // Windows arm gets anywhere.
    #[cfg(not(windows))]
    use super::*;

    // Off Windows this must be None, and `export_space_shortfall(_, None)`
    // must therefore let every save through. Pinning both halves here stops
    // a later "helpful" `unwrap_or(0)` from silently refusing every export
    // on the compile-gate build.
    #[cfg(not(windows))]
    #[test]
    fn an_unsupported_platform_reports_unknown_rather_than_zero() {
        assert_eq!(free_bytes(std::path::Path::new("/tmp")), None);
        assert_eq!(
            vault_buddy_core::screen_capture_config::export_space_shortfall(
                u64::MAX,
                free_bytes(std::path::Path::new("/tmp"))
            ),
            None
        );
    }
}
