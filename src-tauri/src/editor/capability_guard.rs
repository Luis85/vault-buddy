//! Structural pin for R8's app-manifest half (Task 11): scoping the
//! `editor_*` commands to the editor window's OWN Tauri capability.
//!
//! Custom commands are not capability-gated by default in this app
//! (`authz_guard.rs`'s module doc) — every command in `generate_handler!`
//! is callable from every window unless an app-manifest command list
//! (`build.rs`'s `EDITOR_COMMANDS`) takes it out of that default and a
//! capability file grants it back to specific windows. That is two
//! separate files agreeing with `generate_handler!` in a THIRD, and
//! nothing in `cargo build` checks that they still agree once one of them
//! drifts: `build.rs` forgetting a command leaves it reachable from every
//! window despite Task 10's native `authz::require_editor_window` check
//! existing (defense in depth, not the only layer); `editor.json`
//! forgetting one makes the editor itself unable to call its own command.
//! This scan reads all three and fails naming the command and the file
//! that is missing it.
//!
//! It also asserts `capabilities/default.json` grants NONE of the
//! `allow-editor-*` permissions — if it ever did, every window in its
//! `windows` list (which still includes `"editor"`, for the app commands
//! every window shares) would regain the very access the manifest exists
//! to take away.

use std::path::{Path, PathBuf};

use crate::structural_scan::code_only;

/// Every editor command as of this task. Add a new command here in the
/// same commit that adds it to `generate_handler!` (`lib.rs`), `build.rs`'s
/// `EDITOR_COMMANDS`, and `capabilities/editor.json`'s `permissions`.
const EXPECTED: &[&str] = &[
    "editor_close_session",
    "editor_execute",
    "editor_get_snapshot",
    "editor_hide_window",
    "editor_open_staged",
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_to_string(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"))
}

/// The `editor_*` command names registered in `lib.rs`'s
/// `generate_handler!` block. Read `lib.rs` directly rather than through
/// `structural_scan::shell_file`: that helper's `production_half` cuts a
/// file off at its FIRST `#[cfg(test)]`, which is correct for a file whose
/// only test code is one trailing module but wrong here — `lib.rs` has
/// several scattered `#[cfg(test)] mod …;` declarations long before the
/// `generate_handler!` block, so `production_half` would truncate the
/// block away entirely. `code_only` alone (comments and string literals
/// stripped, applied per line) is what this scan actually needs, so a doc
/// comment that merely MENTIONS a command name (this file's own module doc
/// does, repeatedly) cannot manufacture a hit.
fn registered_editor_commands() -> Vec<String> {
    let path = manifest_dir().join("src").join("lib.rs");
    let src = read_to_string(&path);
    let lib: String = src.lines().map(code_only).collect::<Vec<_>>().join("\n");
    let start = lib
        .find("generate_handler![")
        .expect("lib.rs must contain a tauri::generate_handler![ block");
    let end = lib[start..]
        .find("])")
        .map(|i| start + i)
        .expect("generate_handler![ block must close with a bare `])`");
    let mut names: Vec<String> = lib[start..end]
        .split(',')
        .filter_map(|entry| {
            let name = entry.rsplit("::").next()?.trim();
            (!name.is_empty() && name.starts_with("editor_")).then(|| name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// `build.rs`'s `EDITOR_COMMANDS` list. Parsed from `build.rs`'s RAW source
/// -- deliberately not through `code_only`, which erases string-literal
/// content (that is exactly right for the call-site scans elsewhere in
/// this crate, which match on code shape and must not be fooled by a
/// string that merely mentions a name, but it would erase the very command
/// names this parse exists to read out of a `&[&str]` literal).
fn build_rs_editor_commands() -> Vec<String> {
    let path = manifest_dir().join("build.rs");
    let src = read_to_string(&path);
    let name_at = src
        .find("EDITOR_COMMANDS")
        .expect("build.rs must declare a const named EDITOR_COMMANDS");
    // Skip past the `= ` so the bracket search lands on the VALUE's `&[`,
    // not the type annotation's (`const EDITOR_COMMANDS: &[&str] = &[` has
    // two `[`s before the array literal even starts).
    let eq_at = src[name_at..]
        .find('=')
        .map(|i| name_at + i)
        .expect("EDITOR_COMMANDS must be initialized with `= &[ ... ]`");
    let open = src[eq_at..]
        .find('[')
        .map(|i| eq_at + i + 1)
        .expect("EDITOR_COMMANDS's array must open with [");
    let close = src[open..]
        .find(']')
        .map(|i| open + i)
        .expect("EDITOR_COMMANDS's array must close with ]");
    quoted_strings(&src[open..close])
}

/// Every double-quoted string literal's content within `s`, in order. No
/// escape handling -- every caller here only ever feeds it a plain
/// identifier list (command names), never arbitrary Rust string syntax.
fn quoted_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while chars.by_ref().find(|&c| c == '"').is_some() {
        out.push(chars.by_ref().take_while(|&c| c != '"').collect());
    }
    out
}

/// A capability file's `permissions` array, and (for `editor.json`'s own
/// scoping check) its `windows` array.
struct Capability {
    permissions: Vec<String>,
    windows: Vec<String>,
}

fn read_capability(file_name: &str) -> Capability {
    let path = manifest_dir().join("capabilities").join(file_name);
    let src = read_to_string(&path);
    let json: serde_json::Value =
        serde_json::from_str(&src).unwrap_or_else(|e| panic!("{path:?} is not valid JSON: {e}"));
    let strings = |key: &str| -> Vec<String> {
        json[key]
            .as_array()
            .unwrap_or_else(|| panic!("{path:?} has no {key:?} array"))
            .iter()
            .map(|v| {
                v.as_str()
                    .unwrap_or_else(|| panic!("{path:?}'s {key:?} entries must be strings"))
                    .to_string()
            })
            .collect()
    };
    Capability {
        permissions: strings("permissions"),
        windows: strings("windows"),
    }
}

/// `tauri-build`'s own autogeneration rule
/// (`tauri_utils::acl::build::autogenerate_command_permissions`): `_`
/// becomes `-`, prefixed `allow-`. Duplicated here deliberately rather
/// than imported — this test exists precisely to catch the two spellings
/// drifting apart, so it must compute the expected string itself rather
/// than trust the library to agree with itself.
fn allow_permission(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

#[test]
fn every_editor_command_is_scoped_and_granted_only_to_the_editor_window() {
    let registered = registered_editor_commands();
    assert!(
        !registered.is_empty(),
        "generate_handler![ ] in lib.rs has no editor_* command -- the parse is broken, not the \
         invariant"
    );
    assert_eq!(
        registered, EXPECTED,
        "the registered editor command set changed -- update EXPECTED in capability_guard.rs \
         deliberately"
    );

    let manifest = build_rs_editor_commands();
    assert!(
        !manifest.is_empty(),
        "build.rs's EDITOR_COMMANDS parsed empty -- the parse is broken, not the invariant"
    );
    let editor_cap = read_capability("editor.json");
    assert!(
        !editor_cap.permissions.is_empty(),
        "capabilities/editor.json's permissions parsed empty -- the parse is broken, not the \
         invariant"
    );

    let mut missing_from_manifest = Vec::new();
    let mut missing_from_capability = Vec::new();
    for command in &registered {
        if !manifest.iter().any(|c| c == command) {
            missing_from_manifest.push(command.clone());
        }
        let expected_perm = allow_permission(command);
        if !editor_cap.permissions.iter().any(|p| p == &expected_perm) {
            missing_from_capability.push(format!("{command} (expected {expected_perm:?})"));
        }
    }
    assert!(
        missing_from_manifest.is_empty(),
        "build.rs's EDITOR_COMMANDS is missing: {missing_from_manifest:?} -- that command stays \
         reachable from every window, not just the editor"
    );
    assert!(
        missing_from_capability.is_empty(),
        "capabilities/editor.json does not grant: {missing_from_capability:?} -- the editor \
         window itself could no longer call it"
    );

    // The reverse direction too: a stale entry left in build.rs after a
    // command's removal from generate_handler! would sit unnoticed,
    // autogenerating a permission for a command that no longer exists.
    let extra_in_manifest: Vec<_> = manifest
        .iter()
        .filter(|c| !registered.contains(c))
        .cloned()
        .collect();
    assert!(
        extra_in_manifest.is_empty(),
        "build.rs's EDITOR_COMMANDS lists command(s) generate_handler! does not register: \
         {extra_in_manifest:?}"
    );

    assert_eq!(
        editor_cap.windows,
        vec!["editor".to_string()],
        "capabilities/editor.json must scope to exactly the \"editor\" window, not {:?}",
        editor_cap.windows
    );
}

#[test]
fn default_capability_grants_no_editor_command() {
    let default_cap = read_capability("default.json");
    assert!(
        !default_cap.permissions.is_empty(),
        "capabilities/default.json's permissions parsed empty -- the parse is broken, not the \
         invariant"
    );
    let leaked: Vec<_> = EXPECTED
        .iter()
        .map(|c| allow_permission(c))
        .filter(|perm| default_cap.permissions.contains(perm))
        .collect();
    assert!(
        leaked.is_empty(),
        "capabilities/default.json grants editor-only permission(s): {leaked:?} -- every window \
         in its \"windows\" list (which still includes \"editor\") would regain the command \
         build.rs's app manifest is supposed to scope away from them"
    );
}
