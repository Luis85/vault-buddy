//! The tutorial editor's SHELL-side surface: the owned project store on
//! disk (`project_store`, `store_io`, Task 9), pin-aware staging
//! integration, the `editor_*` session commands (`session_commands`, Task
//! 10) and the durable save/list/reopen commands (`save_commands`, Task 12)
//! behind R8's caller-window check (`authz`).
//!
//! Every `#[tauri::command]` in ANY file under this directory must take
//! `window: WebviewWindow` and call `authz::require_editor_window(&window)`
//! first — `authz_guard.rs` scans the whole directory, whatever a file is
//! named, and fails naming the command that does not.

pub mod authz;
#[cfg(test)]
mod authz_guard;
#[cfg(test)]
mod capability_guard;
pub mod project_store;
pub mod save_commands;
pub mod session_commands;
pub mod store_io;

use std::collections::HashMap;
use std::sync::Mutex;

use vault_buddy_core::editor::EditorSession;

/// The live editing sessions (managed in `lib.rs`).
///
/// `sessions` maps a session id to its session; `by_project` maps a project
/// id to the one session open on it, so a second open of the same project
/// reuses that session instead of forking a second, racing one.
///
/// `open` serializes every open (find-or-mint + pin + register): without
/// it two opens of one unpinned capture both miss the pin and the orphan
/// scan and mint two projects, the second pin silently orphaning the first.
///
/// **Lock order: `open`, then `by_project`, then `sessions`.** `open` is the
/// OUTERMOST lock — never taken while holding either map — and, unlike the
/// maps, it IS held across disk I/O by design (the sidecar read, the store
/// scan, the create and the pin are exactly what it serializes). Only opens
/// wait on it; execute/snapshot/close never take it. The maps are never held
/// across disk I/O.
#[derive(Default)]
pub struct EditorState {
    pub open: Mutex<()>,
    pub sessions: Mutex<HashMap<String, EditorSession>>,
    pub by_project: Mutex<HashMap<String, String>>,
}
