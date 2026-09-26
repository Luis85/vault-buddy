//! The one place that decides "is this `io::Error` a full disk" (GAP-216):
//! the shell's `errors::write_error` and `save_commands::map_write_error`,
//! `publish::copy_error`, `media_import::copy_error`,
//! `guide_commands::map_prefs_write_error` and `webcam_commands::write_error`
//! each used to answer that question with their own copy of the same two
//! lines; `package::write_failed` answered it with only HALF of them and
//! silently forgot Windows' own raw code. One function now backs every one
//! of those call sites.

use std::io;

/// True when `e` represents "the disk is full": the portable
/// `ErrorKind::StorageFull` (Linux's `ENOSPC` maps to it too), checked on
/// every platform, or Windows' own raw `ERROR_DISK_FULL` (112) — checked
/// ONLY under `cfg(windows)`, because 112 names a different condition
/// elsewhere (`EHOSTDOWN` on Linux) and checking it unconditionally would
/// misclassify an unrelated error on another platform as a full disk.
///
/// On the toolchain this was written against, `std` itself already decodes
/// raw 112 to `StorageFull` on Windows (verified: `from_raw_os_error(112)`
/// has `.kind() == StorageFull` before this function ever runs), so the
/// explicit raw check below cannot currently be exercised as the deciding
/// term by any `io::Error` this crate can construct — the first branch
/// already answers it. It is kept anyway, verbatim from the five shell
/// copies this function replaces (each carried the same belt-and-suspenders
/// pair, some with a comment noting "a real machine" rather than a specific
/// Rust version): a `kind()` that has not been decoded that finely, on an
/// older toolchain, a different platform's error decoder, or through an
/// `io::Error` built some other way, is exactly the case this second term
/// exists for, and dropping it would trade a currently-inert line for a
/// real regression the day any of those stops holding.
pub fn is_disk_full(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::StorageFull {
        return true;
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(112)
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_full_is_disk_full_on_every_platform() {
        assert!(is_disk_full(&io::Error::from(io::ErrorKind::StorageFull)));
    }

    #[cfg(windows)]
    #[test]
    fn raw_os_112_is_disk_full_on_windows() {
        assert!(is_disk_full(&io::Error::from_raw_os_error(112)));
    }

    #[cfg(not(windows))]
    #[test]
    fn raw_os_112_is_not_disk_full_off_windows() {
        // 112 is `EHOSTDOWN` on Linux, not ENOSPC -- the raw check is
        // Windows-only for exactly this reason.
        assert!(!is_disk_full(&io::Error::from_raw_os_error(112)));
    }

    #[test]
    fn an_unrelated_error_is_not_disk_full() {
        assert!(!is_disk_full(&io::Error::from(io::ErrorKind::NotFound)));
        assert!(!is_disk_full(&io::Error::from(
            io::ErrorKind::PermissionDenied
        )));
    }
}
