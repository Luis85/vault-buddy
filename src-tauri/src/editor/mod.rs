//! The tutorial editor's SHELL-side surface (P02, Task 9 onward): the owned
//! project store on disk, pin-aware staging integration, and — once Task 10
//! wires the IPC commands — the `editor_*` session surface.
//!
//! Task 9 is the first shell-crate task in the tutorial-editor plan: the
//! project store (`project_store`, `store_io`) and the pin that connects a
//! staged screen capture to the tutorial project editing it
//! (`docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md`
//! R6). Nothing outside this module's own `#[cfg(test)]` code calls into it
//! yet — `lib.rs`'s `mod editor;` declaration carries a temporary
//! `#[allow(dead_code)]` for exactly that reason (workspace
//! `clippy -D warnings` would otherwise fail on a lib crate with no external
//! caller). Task 10 wires `editor_open_staged` and the session commands that
//! give this module a real caller, and removes the allow then.

pub mod project_store;
pub mod store_io;

/// Placeholder for the session/authorization state Task 10 introduces
/// (`editor::authz::require_editor_window`, the per-window session map,
/// R8). Exists now only so this module has a public item beyond its two
/// sibling sub-modules; later tasks grow it into the real thing rather than
/// this file staying a bare pair of `pub mod` lines.
#[derive(Debug, Default)]
pub struct EditorState;
