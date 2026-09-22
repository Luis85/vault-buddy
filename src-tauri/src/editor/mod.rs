//! The tutorial editor's SHELL-side surface: the owned project store on
//! disk (`project_store`, `store_io`, Task 9), pin-aware staging
//! integration, and the `editor_*` session commands (`session_commands`,
//! Task 10) behind R8's caller-window check (`authz`).
//!
//! Every `#[tauri::command]` in ANY file under this directory must take
//! `window: WebviewWindow` and call `authz::require_editor_window(&window)`
//! first — `authz_guard.rs` scans the whole directory, whatever a file is
//! named, and fails naming the command that does not.

pub mod authz;
#[cfg(test)]
mod authz_guard;
pub mod project_store;
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
/// **Lock order: `by_project` before `sessions`** whenever both are held
/// (`session_commands`' module doc). Neither is held across disk I/O.
#[derive(Default)]
pub struct EditorState {
    pub sessions: Mutex<HashMap<String, EditorSession>>,
    pub by_project: Mutex<HashMap<String, String>>,
}
