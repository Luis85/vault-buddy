//! One home for the shell's `EditorError`-construction shorthand
//! (GAP-216). Every `editor/*` command module had grown its own copy of
//! the same handful of tiny functions -- fifteen `err`s, ten `internal`s
//! and three private `local_data`s, all producing byte-identical results,
//! plus five separate copies of the same `#[cfg(windows)]` "is this a full
//! disk" test wearing different function names (`map_write_error`,
//! `copy_error` twice over, `map_prefs_write_error`, `write_error`). This
//! module is the one place a sibling module reaches for `err`/`internal`/
//! `invalid`/`write_error`; `prefs_commands::local_data` (kept there, not
//! moved here, since it is already `pub(crate)` and every other file
//! already imports it from that module) is the fourth.
//!
//! `write_error`'s exact two-way split -- disk-full vs. a generic message
//! -- was `webcam_commands`' own shape before this task and is now shared
//! verbatim by its four call sites. `save_commands::map_write_error`,
//! `publish::copy_error`, `media_import::copy_error` and
//! `guide_commands::map_prefs_write_error` each need a DIFFERENT split
//! (`WriteDenied`, `Cancelled`, `FileTooLarge` codes, and their own
//! wording) that this shape cannot reproduce without changing what they
//! report, so they keep their own function bodies and call
//! `vault_buddy_core::editor::io_errors::is_disk_full` directly instead of
//! re-deriving the `#[cfg(windows)]` test by hand.

use std::io;

use vault_buddy_core::editor::io_errors::is_disk_full;
use vault_buddy_core::editor::{EditorError, EditorErrorCode};

/// Build an [`EditorError`] with an explicit code. Every helper below is a
/// named shorthand over this one call, so a caller that already knows its
/// code (most of them) uses this directly.
pub(super) fn err(code: EditorErrorCode, message: impl Into<String>) -> EditorError {
    EditorError::new(code, message)
}

/// `EditorErrorCode::Internal` -- something this app is at fault for, not
/// the request or the environment.
pub(super) fn internal(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::Internal, message)
}

/// `EditorErrorCode::InvalidRequest` -- the request itself is malformed.
/// `package_import`'s own `InvalidProject` shorthand answers a different
/// question (a whole package file failed validation, not one request) and
/// keeps its own local `invalid_project` rather than sharing this name for
/// two different codes.
pub(super) fn invalid(message: impl Into<String>) -> EditorError {
    err(EditorErrorCode::InvalidRequest, message)
}

/// A write that failed for an ordinary, nameable reason: "not enough disk
/// space to `{what}`" when the OS reports a full disk (`is_disk_full`,
/// core's one rule for that question), else a generic "could not `{what}`".
/// `webcam_commands`'s exact shape before this task; its four call sites
/// (starting a take, saving a chunk twice over, reaching a take's file) are
/// unchanged.
pub(super) fn write_error(what: &str, e: &io::Error) -> EditorError {
    if is_disk_full(e) {
        err(
            EditorErrorCode::DiskFull,
            format!("Not enough disk space to {what}: {e}"),
        )
    } else {
        internal(format!("Could not {what}: {e}"))
    }
}
