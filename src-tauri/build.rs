/// Every `editor_*` command registered in `generate_handler!` (`src/lib.rs`),
/// so `tauri-build` autogenerates an `allow-<kebab-case>` / `deny-<kebab-case>`
/// permission pair for each one instead of leaving them capability-gate-free
/// like every other custom command in this app (Task 10's `authz::
/// require_editor_window` native check is the enforcement layer that stays
/// in force regardless; this manifest is the app-manifest half of R8 —
/// scoping WHICH window's capability may even attempt the call).
///
/// `editor::capability_guard` (test-only, `src/editor/capability_guard.rs`)
/// pins this list against `generate_handler!` and against
/// `capabilities/editor.json`'s grants, so a command added to one and
/// forgotten in another fails loudly. Add a new command here in the same
/// commit that adds it to `generate_handler!`.
const EDITOR_COMMANDS: &[&str] = &[
    "editor_close_session",
    "editor_execute",
    "editor_get_snapshot",
    "editor_hide_window",
    "editor_open_staged",
];

fn main() {
    // Mirrors `tauri_build::build()`'s own panic-with-message behavior
    // (`try_build` never panics on its own), since this crate now passes a
    // non-default `Attributes` and can no longer use the plain wrapper.
    if let Err(error) = tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(EDITOR_COMMANDS)),
    ) {
        panic!("tauri-build failed: {error:#}");
    }
}
