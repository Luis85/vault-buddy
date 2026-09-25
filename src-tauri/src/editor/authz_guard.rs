//! Structural pin for R8's native caller check (test-only).
//!
//! Since Task 11's exhaustive app manifest, `capabilities/editor.json`
//! already scopes every `editor_*` command to the `editor` window at the
//! Tauri ACL layer — but this native check is the SECOND, independent
//! layer (`authz.rs`'s module doc), not a substitute for it. An `editor_*`
//! command that forgets `require_editor_window(&window)` is still a real
//! defect: it stays reachable from any window a FUTURE capability edit
//! (or a new command mis-granted to the wrong capability file) newly
//! permits, and nothing else in the suite would notice, because the
//! command still works perfectly from the editor either way. This scan
//! reads every `.rs` file under `src/editor/` (whatever it is named — a
//! command moved into a new sibling file is still covered) and requires,
//! for each `#[tauri::command]`:
//! - a `window: WebviewWindow` parameter, and
//! - `require_editor_window(&window)?` as the body's first statement — the
//!   `?` included, so a discarded refusal (`let _ = …`, `.ok()`) fails (only
//!   plain `let` bindings with no call in them may precede it).
//!
//! It also pins the exact SET of commands found, so a new command has to be
//! added here deliberately — and an empty scan (a broken walk) cannot pass.

use std::path::{Path, PathBuf};

use crate::structural_scan::code_only;

const THE_CALL: &str = "require_editor_window(&window)";

/// Every editor command, by name. Add a new command here in the same commit
/// that adds it.
const EXPECTED: &[&str] = &[
    "editor_cancel_job",
    "editor_close_session",
    "editor_execute",
    "editor_export_diagnostics",
    "editor_export_guide_progress",
    "editor_export_package",
    "editor_export_subtitles",
    "editor_get_checks",
    "editor_get_guide_progress",
    "editor_get_jobs",
    "editor_get_products",
    "editor_get_snapshot",
    "editor_get_workspace",
    "editor_hide_window",
    "editor_import_captions",
    "editor_import_guide_progress",
    "editor_import_media",
    "editor_import_package",
    "editor_list_projects",
    "editor_media_peaks",
    "editor_media_thumbnail",
    "editor_media_url",
    "editor_open_project",
    "editor_open_staged",
    "editor_publish_product",
    "editor_relink_media",
    "editor_restore_product",
    "editor_save_guide_progress",
    "editor_save_project",
    "editor_save_workspace",
    "editor_start_render",
    "editor_webcam_append",
    "editor_webcam_begin",
    "editor_webcam_discard",
    "editor_webcam_finish",
];

/// One `#[tauri::command]` found in `src`: its name, and what is wrong with
/// it (`None` = compliant).
pub(crate) fn check_commands(src: &str) -> Vec<(String, Option<String>)> {
    let lines: Vec<String> = src.lines().map(code_only).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !lines[i].trim_start().starts_with("#[tauri::command") {
            i += 1;
            continue;
        }
        // The signature: from the attribute to the line whose code ends in
        // the body's opening brace.
        let mut sig = String::new();
        let mut j = i + 1;
        while j < lines.len() {
            sig.push_str(&lines[j]);
            sig.push(' ');
            if lines[j].trim_end().ends_with('{') {
                break;
            }
            j += 1;
        }
        let name = sig
            .split("fn ")
            .nth(1)
            .and_then(|rest| rest.split(['(', '<']).next())
            .map(|n| n.trim().to_string())
            .unwrap_or_else(|| format!("<unparsed command at line {}>", i + 1));
        let compact: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");
        let takes_window = compact.contains("window: WebviewWindow")
            || compact.contains("window: tauri::WebviewWindow");
        let first = lines[j + 1..]
            .iter()
            .map(|l| l.trim())
            .find(|l| !(l.is_empty() || l.starts_with("let ") && !l.contains('(')));
        let problem = if !takes_window {
            Some("does not take `window: WebviewWindow`".to_string())
        } else if !first.is_some_and(|l| l.starts_with(&format!("{THE_CALL}?"))) {
            // `starts_with` + `?`, never `contains`: `let _ = …;` and `….ok();`
            // make the call and throw its refusal away.
            Some(format!(
                "does not call {THE_CALL}? first (first statement: {:?})",
                first.unwrap_or("")
            ))
        } else {
            None
        };
        out.push((name, problem));
        i = j + 1;
    }
    out
}

fn editor_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("editor");
    let mut files = Vec::new();
    let mut stack = vec![dir.clone()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)
            .unwrap_or_else(|e| panic!("cannot read {d:?}: {e}"))
            .flatten()
        {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
                files.push(p);
            }
        }
    }
    assert!(
        files.len() >= 4,
        "scan under {dir:?} found only {} file(s) -- the walk is broken",
        files.len()
    );
    files
}

#[test]
fn every_editor_command_checks_the_caller_window() {
    let mut found = Vec::new();
    let mut violations = Vec::new();
    for path in editor_files() {
        let src = std::fs::read_to_string(&path).unwrap();
        for (name, problem) in check_commands(&src) {
            if let Some(p) = problem {
                violations.push(format!("{name} ({}): {p}", path.display()));
            }
            found.push(name);
        }
    }
    assert!(
        violations.is_empty(),
        "editor commands callable from any window:\n{}",
        violations.join("\n")
    );
    found.sort();
    assert_eq!(
        found, EXPECTED,
        "the editor command set changed -- add the new command to EXPECTED deliberately"
    );
}

// The checker itself, against fixtures built by concatenation so this file
// declares no command of its own.
#[test]
fn the_checker_names_a_command_without_the_call() {
    let attr = concat!("#[tauri", "::command]");
    let good = format!(
        "{attr}\npub async fn good(\n    window: WebviewWindow,\n    id: String,\n) -> R {{\n    \
         {THE_CALL}?;\n    go(id)\n}}\n"
    );
    let late = format!(
        "{attr}\npub fn late(window: WebviewWindow) -> R {{\n    let x = work()?;\n    \
         {THE_CALL}?;\n    x\n}}\n"
    );
    let no_window = format!("{attr}\npub fn no_window(app: AppHandle) -> R {{\n    go()\n}}\n");
    // The call made but its refusal DISCARDED: an unauthorized caller would
    // sail straight past it, so both must be reported, naming the fn.
    let discarded = format!(
        "{attr}\npub fn discarded(window: WebviewWindow) -> R {{\n    let _ = {THE_CALL};\n    \
         go()\n}}\n"
    );
    let ok_ed = format!(
        "{attr}\npub fn ok_ed(window: WebviewWindow) -> R {{\n    {THE_CALL}.ok();\n    go()\n}}\n"
    );
    let result = check_commands(&format!("{good}{late}{no_window}{discarded}{ok_ed}"));
    assert_eq!(result.len(), 5);
    for (i, name) in [(3, "discarded"), (4, "ok_ed")] {
        assert_eq!(result[i].0, name);
        assert!(
            result[i].1.is_some(),
            "{name} discards the refusal and must be reported"
        );
    }
    assert_eq!(result[0], ("good".to_string(), None));
    assert_eq!(result[1].0, "late");
    assert!(result[1].1.as_deref().unwrap().contains("first"));
    assert_eq!(result[2].0, "no_window");
    assert!(result[2]
        .1
        .as_deref()
        .unwrap()
        .contains("window: WebviewWindow"));
}
