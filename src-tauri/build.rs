/// Every command registered in `generate_handler!` (`src/lib.rs`), fed
/// whole to `AppManifest::commands()` below.
///
/// This MUST be exhaustive, not just the five `editor_*` commands Task 11
/// set out to scope -- passing ANY non-empty command list here flips a
/// process-wide switch, `RuntimeAuthority::has_app_acl` (`tauri-utils`'s
/// `acl::resolved::has_app_manifest`), that the running app's IPC dispatch
/// (`tauri`'s `webview/mod.rs`, the `has_app_acl_manifest` check ahead of
/// every non-plugin, non-remote invoke) reads to decide whether EVERY
/// custom command needs a capability grant, not just the ones this list
/// names. A five-command manifest would have made the other 96 --
/// `list_vaults`, `toggle_panel`, `start_capture`, `add_task`,
/// `search_vaults`, ... -- unreachable from every window at runtime, a
/// defect no `cargo build`/`tauri build` catches (they compile and
/// validate SHAPE; they never dispatch an IPC call). Caught in review
/// after Task 11 first landed a five-command list; `editor::
/// capability_guard` (test-only, `src/editor/capability_guard.rs`) now
/// pins this list against `generate_handler!` in both directions AND
/// against a REPLICA of tauri's own resolution run over the generated
/// `gen/schemas/{capabilities,acl-manifests}.json`, so neither kind of
/// gap can recur silently.
///
/// Add a new command here in the same commit that adds it to
/// `generate_handler!`, and grant it in EXACTLY ONE of
/// `capabilities/editor.json` (only if it is one of the `editor_*` session
/// commands -- ten as of Task 18) or `capabilities/default.json` (every
/// other command).
const ALL_COMMANDS: &[&str] = &[
    "add_task",
    "announce",
    "begin_document_import",
    "cancel_export",
    "cancel_transcription",
    "capture_status",
    "clear_staged_captures",
    "close_bubble",
    "close_panel",
    "convert_document",
    "count_open_tasks",
    "create_task_list",
    "delete_task",
    "delete_task_list",
    "delete_transcription_model",
    "detect_ffmpeg",
    "detect_pandoc",
    "discard_staged_capture",
    "duplicate_task",
    "editor_cancel_job",
    "editor_close_session",
    "editor_execute",
    "editor_export_package",
    "editor_get_jobs",
    "editor_get_snapshot",
    "editor_get_workspace",
    "editor_hide_window",
    "editor_import_captions",
    "editor_import_media",
    "editor_import_package",
    "editor_list_projects",
    "editor_media_peaks",
    "editor_media_thumbnail",
    "editor_media_url",
    "editor_open_project",
    "editor_open_staged",
    "editor_save_project",
    "editor_save_workspace",
    "export_and_save_capture",
    "get_autostart",
    "get_bubble_anchor",
    "get_buddy_facing",
    "get_capture_config",
    "get_documents_config",
    "get_mcp_config",
    "get_panel_config",
    "get_screen_capture_config",
    "get_tasks_config",
    "get_transcription_config",
    "list_audio_devices",
    "list_capture_sources",
    "list_recordings",
    "list_staged_captures",
    "list_task_lists",
    "list_tasks",
    "list_transcription_models",
    "list_tutorial_projects",
    "list_vaults",
    "load_staged_capture",
    "move_task_to_list",
    "open_capture_editor",
    "open_daily_note",
    "open_external_url",
    "open_imported_document",
    "open_logs_folder",
    "open_panel",
    "open_project_editor",
    "open_recording",
    "open_screen_capture",
    "open_search_result",
    "open_task",
    "open_transcript",
    "open_vault",
    "pause_capture",
    "pause_screen_capture",
    "prepare_update_install",
    "rearm_crash_detection",
    "regenerate_mcp_token",
    "rename_capture",
    "rename_task_list",
    "resolve_region_selection",
    "resume_capture",
    "resume_screen_capture",
    "retranscribe",
    "save_capture_timeline",
    "screen_capture_status",
    "search_vaults",
    "select_capture_region",
    "set_autostart",
    "set_capture_config",
    "set_dialog_active",
    "set_documents_config",
    "set_ffmpeg_path",
    "set_mcp_config",
    "set_pandoc_path",
    "set_panel_size",
    "set_screen_capture_config",
    "set_task_id_config",
    "set_task_lists_config",
    "set_task_status",
    "set_task_template_config",
    "set_tasks_config",
    "set_transcription_config",
    "show_buddy_menu",
    "staging_usage",
    "start_buddy_drag",
    "start_capture",
    "start_screen_capture",
    "stop_capture",
    "stop_screen_capture",
    "take_add_document_request",
    "take_editor_request",
    "take_pending_import",
    "toggle_panel",
    "transcribe_recording_now",
    "transcription_queue_status",
    "update_task",
];

fn main() {
    // Mirrors `tauri_build::build()`'s own panic-with-message behavior
    // (`try_build` never panics on its own), since this crate now passes a
    // non-default `Attributes` and can no longer use the plain wrapper.
    if let Err(error) = tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(ALL_COMMANDS)),
    ) {
        panic!("tauri-build failed: {error:#}");
    }
}
