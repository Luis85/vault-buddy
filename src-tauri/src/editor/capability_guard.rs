//! Structural pin for R8's app-manifest half (Task 11): scoping the
//! `editor_*` commands to the editor window's OWN Tauri capability.
//!
//! Tauri does NOT capability-gate a custom command by default — but this
//! app no longer relies on that default anywhere. `build.rs`'s
//! `AppManifest::commands(ALL_COMMANDS)` lists EVERY command in
//! `generate_handler!`, and doing so switches Tauri's own IPC dispatch
//! into requiring an explicit capability grant for ALL of them, not just
//! the ones named — see the EXHAUSTIVE note below for why that is not
//! optional. So every custom command in this app IS capability-gated,
//! unconditionally: `capabilities/editor.json` grants the `editor_*`
//! session commands (eleven as of Task 22) to the `editor` window alone, and
//! `capabilities/default.json` grants every other command to its
//! `windows` list. A command reachable from a window with no matching
//! grant is a bug this scan exists to catch, not an intentional gap.
//!
//! **The manifest command list must be EXHAUSTIVE, not just the five
//! `editor_*` commands, and that is not obvious from the Tauri docs.**
//! Passing ANY non-empty list to `AppManifest::commands()` flips a
//! process-wide switch — `tauri_utils::acl::resolved::has_app_manifest`,
//! read at runtime as `RuntimeAuthority::has_app_acl` — that `tauri`'s IPC
//! dispatch (`webview/mod.rs`'s `has_app_acl_manifest` check, run ahead of
//! EVERY non-plugin, non-remote invoke) uses to decide whether EVERY
//! custom command needs a capability grant, not just the ones the list
//! names. A first version of this file listed only the five `editor_*`
//! commands and passed every gate here — `cargo build`/`tauri build`
//! compiles and validates SHAPE, it never dispatches an IPC call — while
//! silently making the other 96 commands (`list_vaults`, `toggle_panel`,
//! `start_capture`, `add_task`, `search_vaults`, ...) unreachable from
//! every window at runtime. Caught in review, not by any test that existed
//! at the time.
//!
//! So this scan now checks THREE things, not two:
//! 1. **Exhaustive + disjoint partition** (source files only): every
//!    command in `lib.rs`'s `generate_handler!` appears in `build.rs`'s
//!    `ALL_COMMANDS`, and is granted in EXACTLY ONE of
//!    `capabilities/editor.json` (iff it is one of the `editor_*` session
//!    commands) or `capabilities/default.json` (every other
//!    command) — never both, never neither. No capability grants a
//!    command `generate_handler!` does not register.
//! 2. `capabilities/editor.json` scopes to exactly the `"editor"` window.
//! 3. **The GENERATED artifact, not just the source files that produced
//!    it**: `npx tauri build --no-bundle` (and, as it turns out, any plain
//!    `cargo build`/`cargo test` of this crate — `build.rs` runs and
//!    regenerates these regardless) writes
//!    `gen/schemas/{capabilities,acl-manifests}.json`. A REPLICA of
//!    tauri's own capability-to-command resolution, run over those
//!    generated files rather than our source `capabilities/*.json`, closes
//!    the gap a hand-parsed source file cannot: it would catch
//!    `tauri-build` itself silently dropping or mis-expanding a grant, not
//!    just OUR files disagreeing with each other. This is the same DATA
//!    `tauri`'s `RuntimeAuthority` consumes to build its resolved ACL, not
//!    a live IPC call against a running webview — see `docs/Gaps.md`
//!    GAP-170 for that residual.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::structural_scan::code_only;

/// The `editor_*` session, save/list/reopen, workspace, media and job commands —
/// nineteen as of Task 39 (`editor_export_package`/`editor_import_package`). Add a new one here in the same commit that adds it to
/// `generate_handler!` (`lib.rs`), `build.rs`'s `ALL_COMMANDS`, and
/// `capabilities/editor.json`'s `permissions` — never to
/// `capabilities/default.json`.
const EXPECTED_EDITOR: &[&str] = &[
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
];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_to_string(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path:?}: {e}"))
}

fn is_editor_command(command: &str) -> bool {
    command.starts_with("editor_")
}

/// Every command name registered in `lib.rs`'s `generate_handler!` block.
/// Read `lib.rs` directly rather than through `structural_scan::shell_file`:
/// that helper's `production_half` cuts a file off at its FIRST
/// `#[cfg(test)]`, which is correct for a file whose only test code is one
/// trailing module but wrong here — `lib.rs` has several scattered
/// `#[cfg(test)] mod …;` declarations long before the `generate_handler!`
/// block, so `production_half` would truncate the block away entirely.
/// `code_only` alone (comments and string literals stripped, applied per
/// line) is what this scan actually needs, so a doc comment that merely
/// MENTIONS a command name (this file's own module doc does, repeatedly)
/// cannot manufacture a hit.
fn registered_commands() -> Vec<String> {
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
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// `build.rs`'s `ALL_COMMANDS` list. Parsed from `build.rs`'s RAW source —
/// deliberately not through `code_only`, which erases string-literal
/// content (that is exactly right for the call-site scans elsewhere in
/// this crate, which match on code shape and must not be fooled by a
/// string that merely mentions a name, but it would erase the very command
/// names this parse exists to read out of a `&[&str]` literal).
fn build_rs_commands() -> Vec<String> {
    let path = manifest_dir().join("build.rs");
    let src = read_to_string(&path);
    // Anchor on the DECLARATION (`const ALL_COMMANDS`), not the bare name —
    // `ALL_COMMANDS` alone would also match a doc comment mentioning it (as
    // this very module's own doc does), and matching whichever occurrence
    // comes first in the file is exactly the kind of accident this scan
    // must not depend on.
    let name_at = src
        .find("const ALL_COMMANDS")
        .expect("build.rs must declare a const named ALL_COMMANDS");
    // Skip past the `= ` so the bracket search lands on the VALUE's `&[`,
    // not the type annotation's (`const ALL_COMMANDS: &[&str] = &[` has
    // two `[`s before the array literal even starts).
    let eq_at = src[name_at..]
        .find('=')
        .map(|i| name_at + i)
        .expect("ALL_COMMANDS must be initialized with `= &[ ... ]`");
    let open = src[eq_at..]
        .find('[')
        .map(|i| eq_at + i + 1)
        .expect("ALL_COMMANDS's array must open with [");
    let close = src[open..]
        .find(']')
        .map(|i| open + i)
        .expect("ALL_COMMANDS's array must close with ]");
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

/// A capability file's `permissions` array, and its `windows` array.
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

/// The reverse of `allow_permission`, for an app-command permission that
/// carries NO colon (a colon means a plugin permission, e.g.
/// `dialog:allow-open` — never one of this app's own autogenerated
/// grants, which the schema special-cases to an unprefixed identifier).
/// `None` for anything not shaped like `allow-<kebab>` at all (a `deny-`
/// entry, say — none of this app's capability files grant one, but this
/// stays honest about what it can decode rather than assuming).
fn command_from_allow_permission(permission: &str) -> Option<String> {
    permission
        .strip_prefix("allow-")
        .map(|kebab| kebab.replace('-', "_"))
}

#[test]
fn every_registered_command_is_granted_in_exactly_one_capability_file() {
    let registered = registered_commands();
    // Vacuity guard: a broken parse returning an empty or tiny list must
    // not read as "every command partitioned correctly" — there is
    // nothing in this app that would ever make the real count anywhere
    // near this small.
    assert!(
        registered.len() > 50,
        "generate_handler![ ] in lib.rs parsed only {} command(s) -- the parse is broken, not \
         the invariant",
        registered.len()
    );

    let registered_editor: Vec<String> = registered
        .iter()
        .filter(|c| is_editor_command(c))
        .cloned()
        .collect();
    assert_eq!(
        registered_editor, EXPECTED_EDITOR,
        "the registered editor_* command set changed -- update EXPECTED_EDITOR in \
         capability_guard.rs deliberately"
    );

    let manifest = build_rs_commands();
    assert!(
        manifest.len() > 50,
        "build.rs's ALL_COMMANDS parsed only {} command(s) -- the parse is broken, not the \
         invariant",
        manifest.len()
    );

    let default_cap = read_capability("default.json");
    let editor_cap = read_capability("editor.json");
    assert!(
        !default_cap.permissions.is_empty() && !editor_cap.permissions.is_empty(),
        "a capability file's permissions parsed empty -- the parse is broken, not the invariant"
    );

    // 1. Every registered command is in build.rs's manifest.
    let missing_from_manifest: Vec<_> = registered
        .iter()
        .filter(|c| !manifest.iter().any(|m| m == *c))
        .cloned()
        .collect();
    assert!(
        missing_from_manifest.is_empty(),
        "build.rs's ALL_COMMANDS is missing: {missing_from_manifest:?} -- that command is \
         registered in generate_handler! but absent from the app manifest entirely, so no \
         capability grant can ever reach it once ANY other command's presence in the manifest \
         has switched app-wide ACL enforcement on"
    );

    // 2. The reverse: no stale manifest entry for a command that no
    // longer exists.
    let extra_in_manifest: Vec<_> = manifest
        .iter()
        .filter(|c| !registered.contains(c))
        .cloned()
        .collect();
    assert!(
        extra_in_manifest.is_empty(),
        "build.rs's ALL_COMMANDS lists command(s) generate_handler! does not register: \
         {extra_in_manifest:?}"
    );

    // 3. Every registered command is granted in EXACTLY ONE capability
    // file, and the RIGHT one: editor_* -> editor.json only, everything
    // else -> default.json only.
    let mut wrong_grant = Vec::new();
    for command in &registered {
        let perm = allow_permission(command);
        let in_default = default_cap.permissions.iter().any(|p| p == &perm);
        let in_editor = editor_cap.permissions.iter().any(|p| p == &perm);
        let correct = if is_editor_command(command) {
            in_editor && !in_default
        } else {
            in_default && !in_editor
        };
        if !correct {
            wrong_grant.push(format!(
                "{command} (expected {perm:?}; granted in default.json: {in_default}, in \
                 editor.json: {in_editor})"
            ));
        }
    }
    assert!(
        wrong_grant.is_empty(),
        "these commands are not granted in exactly the one capability file they belong in: \
         {wrong_grant:?}"
    );

    // 4. No capability grants an allow-<kebab> permission for a command
    // that is not even registered -- a stale grant left behind after a
    // command's removal would sit unnoticed otherwise. A permission
    // containing a colon (dialog:allow-open, core:default, ...) is a
    // PLUGIN permission, not one of this app's own generated grants, and
    // is out of scope for this check.
    let mut stale_grants = Vec::new();
    for (file, cap) in [("default.json", &default_cap), ("editor.json", &editor_cap)] {
        for perm in &cap.permissions {
            if perm.contains(':') {
                continue;
            }
            let Some(command) = command_from_allow_permission(perm) else {
                continue;
            };
            if !registered.contains(&command) {
                stale_grants.push(format!(
                    "{file} grants {perm:?} for unregistered {command:?}"
                ));
            }
        }
    }
    assert!(
        stale_grants.is_empty(),
        "stale capability grant(s) for a command generate_handler! does not register: \
         {stale_grants:?}"
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
    let leaked: Vec<_> = EXPECTED_EDITOR
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

/// The generated `gen/schemas` directory: `tauri-build`'s own OUTPUT, not
/// a file this repo authors. Written by `build.rs` on every `cargo`
/// invocation of this crate (the doc comment on `acl::build` in
/// `tauri-build` says "mainly to be read by tauri-cli", but nothing gates
/// it to a `tauri build` specifically — plain `cargo check`/`test` already
/// produced it before this test ever runs, since build scripts run ahead
/// of everything else). Git-ignored (`src-tauri/gen/`), correctly: it is
/// 1:1 derived from the checked-in `capabilities/*.json` and `build.rs`,
/// so a committed copy would be redundant, `write_if_changed`-regenerated
/// duplication.
fn generated_schema_dir() -> PathBuf {
    manifest_dir().join("gen").join("schemas")
}

/// Every command allowed for `window_label`, computed by REPLICATING
/// tauri's own resolution over the files `tauri-build` actually wrote —
/// `gen/schemas/capabilities.json` (this app's two capabilities, already
/// expanded to `"local": true` and window arrays) and
/// `gen/schemas/acl-manifests.json` (every permission identifier's
/// `commands.allow`/`commands.deny`, under the `__app-acl__` key for this
/// app's own autogenerated ones). For each capability scoped to
/// `window_label`, union its permissions' `commands.allow`, minus
/// `commands.deny`. A permission containing a colon is a plugin
/// permission (`dialog:allow-open`, `core:default`, ...) and is skipped:
/// this app defines no plugin of its own and grants no plugin command
/// through `commands.allow`, so skipping it cannot hide an app command's
/// grant. This app also never uses a `permission_sets` alias or
/// `default_permission` for its OWN commands (`AppManifest` exposes no
/// builder for either), so a direct `permissions` map lookup is the
/// COMPLETE resolution for this app's commands, not an approximation of
/// one — see GAP-170 for what this still does NOT prove (a live IPC call
/// against a running webview).
fn resolved_commands_for_window(window_label: &str) -> HashSet<String> {
    let caps_path = generated_schema_dir().join("capabilities.json");
    let caps: serde_json::Value = serde_json::from_str(&read_to_string(&caps_path))
        .unwrap_or_else(|e| panic!("{caps_path:?} is not valid JSON: {e}"));
    let manifests_path = generated_schema_dir().join("acl-manifests.json");
    let manifests: serde_json::Value = serde_json::from_str(&read_to_string(&manifests_path))
        .unwrap_or_else(|e| panic!("{manifests_path:?} is not valid JSON: {e}"));
    let app_permissions = manifests
        .get("__app-acl__")
        .and_then(|m| m.get("permissions"))
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| panic!("{manifests_path:?} has no readable __app-acl__.permissions"));

    let caps_obj = caps
        .as_object()
        .unwrap_or_else(|| panic!("{caps_path:?} must be a JSON object keyed by capability id"));
    let mut allowed = HashSet::new();
    for capability in caps_obj.values() {
        let windows = capability
            .get("windows")
            .and_then(|w| w.as_array())
            .unwrap_or_else(|| panic!("{caps_path:?}: a capability has no windows array"));
        let scoped = windows.iter().any(|w| w.as_str() == Some(window_label));
        if !scoped {
            continue;
        }
        let permissions = capability
            .get("permissions")
            .and_then(|p| p.as_array())
            .unwrap_or_else(|| panic!("{caps_path:?}: a capability has no permissions array"));
        for perm in permissions {
            let perm = perm
                .as_str()
                .unwrap_or_else(|| panic!("{caps_path:?}: a permission entry is not a string"));
            if perm.contains(':') {
                continue; // plugin permission, not one of this app's own
            }
            let Some(def) = app_permissions.get(perm) else {
                panic!(
                    "{caps_path:?} grants {perm:?} but {manifests_path:?}'s __app-acl__ has no \
                     such permission -- tauri-build accepted a grant it cannot resolve"
                );
            };
            let commands = def.get("commands").unwrap_or_else(|| {
                panic!("{manifests_path:?}: permission {perm:?} has no commands object")
            });
            for cmd in commands
                .get("allow")
                .and_then(|a| a.as_array())
                .unwrap_or_else(|| {
                    panic!("{manifests_path:?}: permission {perm:?} has no commands.allow array")
                })
            {
                allowed.insert(cmd.as_str().unwrap_or_default().to_string());
            }
            for cmd in commands
                .get("deny")
                .and_then(|d| d.as_array())
                .unwrap_or_else(|| {
                    panic!("{manifests_path:?}: permission {perm:?} has no commands.deny array")
                })
            {
                allowed.remove(cmd.as_str().unwrap_or_default());
            }
        }
    }
    allowed
}

/// The proof beyond a clean compile (Task 11 fix round 1): read what
/// `tauri-build` actually WROTE for this build, resolve it the way
/// `tauri`'s `RuntimeAuthority` would, and confirm a representative
/// non-editor command reaches the windows it always could while no editor
/// command reaches them -- and that the editor window itself still gets
/// its own eleven commands. A test that only re-parsed our own source
/// `capabilities/*.json` files (the other test above) cannot catch
/// `tauri-build` itself mishandling a grant; this one reads its output.
#[test]
fn the_generated_acl_artifact_resolves_the_partition_correctly() {
    for window in ["panel", "main", "bubble", "overlay", "region-indicator"] {
        let allowed = resolved_commands_for_window(window);
        assert!(
            allowed.contains("list_vaults"),
            "gen/schemas says window {window:?} cannot call list_vaults -- \
             capabilities/default.json's grant did not survive into the generated ACL artifact"
        );
        for editor_command in EXPECTED_EDITOR {
            assert!(
                !allowed.contains(*editor_command),
                "gen/schemas says window {window:?} CAN call {editor_command:?} -- the \
                 app-manifest scoping this task exists for has failed at the artifact level"
            );
        }
    }

    let editor_allowed = resolved_commands_for_window("editor");
    for editor_command in EXPECTED_EDITOR {
        assert!(
            editor_allowed.contains(*editor_command),
            "gen/schemas says the editor window itself cannot call {editor_command:?} -- \
             capabilities/editor.json's own grant did not survive into the generated ACL \
             artifact"
        );
    }
    assert!(
        editor_allowed.contains("list_vaults"),
        "gen/schemas says the editor window cannot call list_vaults -- default.json's \"editor\" \
         window entry did not survive into the generated ACL artifact"
    );
}
