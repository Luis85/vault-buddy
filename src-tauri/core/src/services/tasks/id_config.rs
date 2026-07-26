//! The Task-ID settings guard. `set_task_id_config` refuses a property
//! re-point or a disable while any task in the vault still carries a
//! `parent-id`, because both of those changes make
//! `id_property_for_generation`'s gate stop resolving that property — every
//! recorded reference would silently point at nothing (design spec
//! docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md
//! §2a). Its own module: the guard-then-write sequencing this encodes — one
//! `config_write_lock()` held across the scan AND the commit, the same lock
//! `services::set_task_parent` holds across ITS phases — is a responsibility
//! of its own, the same reason `parent.rs` is split out rather than folded
//! into `mod.rs`.

use super::{assert_root_if_exists, tasks_root_for};
use crate::capture_config;
use crate::services::{app_config, ServicePaths};
use crate::tasks;

/// Count of tasks in the vault that carry a `parent-id`. The ID configuration
/// is locked while this is nonzero: changing the property name OR disabling
/// the feature would make every recorded reference unresolvable (design spec
/// §2a). The count itself (not just a bool) is the point — the refusal it
/// feeds names it, per spec §2a's "inline error naming the count and the
/// remedy".
///
/// FALLIBLE on purpose. The read paths in this app are best-effort — an
/// unresolvable root degrades to "nothing here" — which is right for a view but
/// wrong for a guard: an offline network vault would report zero links and let
/// the setting through, orphaning every relationship once access returns. An
/// incomplete inspection is an Err, and the caller refuses conservatively
/// (design spec §2a).
///
/// MUST NOT acquire `capture_config::config_write_lock()`. Its only caller,
/// `set_task_id_config`, calls this WHILE HOLDING that lock — one lock across
/// the scan and the commit (design spec §2a) — and the lock is NOT reentrant,
/// so taking it again here would self-deadlock instead of erroring loudly.
pub fn count_parent_links(paths: &ServicePaths, vault_id: &str) -> Result<usize, String> {
    let (vault_path, root, _cfg) = tasks_root_for(paths, vault_id)?;
    // The registry can list a vault whose folder was moved/deleted (add_task
    // guards its own write the same way — services/tasks/mod.rs). That is
    // exactly what an unreachable network vault looks like on disk, and it
    // must be caught HERE, before the ambiguity below can swallow it: a
    // missing TASKS root reads as "no tasks" in EITHER case (a vault that
    // never created one, or a vault that isn't there at all), because
    // list_tasks_structural treats a NotFound root as an empty — not
    // failed — scan regardless of why it's missing. Only checking the vault
    // path itself, separately, can tell the two apart.
    if !vault_path.is_dir() {
        return Err("Vault folder not found — was it moved or deleted?".to_string());
    }
    assert_root_if_exists(&vault_path, &root)?;
    // STRUCTURAL: archived tasks counted (their files still carry parent-id),
    // and an unreadable task is an ERROR — never "no links" (design spec §2a).
    // `id_property: None` is deliberate — this asks only whether references
    // exist, and parent-id is surfaced regardless of the id feature's state.
    Ok(tasks::list_tasks_structural(&root, None)?
        .iter()
        .filter(|t| t.parent_id.is_some())
        .count())
}

/// Persist the vault's Task ID settings (enable + frontmatter property),
/// preserving every other per-vault field via the same read-modify-write
/// every other config setter uses (`update_vault_config_at`). Write-strict on
/// the property: empty -> the default (stored as None); an invalid or
/// reserved name is an inline error.
///
/// GUARDED (design spec §2a): refuses a PROPERTY CHANGE or a DISABLE outright
/// while the vault has any task carrying `parent-id` — either one makes
/// `id_property_for_generation`'s gate stop resolving that property, so every
/// recorded reference would point at nothing. The refusal names the count of
/// affected tasks and the remedy (spec §2a's exact requirement). Enabling
/// under an UNCHANGED property is exempt — see `count_parent_links` above and
/// the design spec for why that direction can never orphan a link.
pub fn set_task_id_config(
    paths: &ServicePaths,
    id: &str,
    enabled: bool,
    property: Option<&str>,
) -> Result<(), String> {
    // Validate the property BEFORE the lock (fail-fast; never hold it across a
    // doomed write) — the posture set_tasks_config/set_task_lists_config use.
    // Validate + apply ONLY when enabling: with IDs off no id is written, so an
    // invalid draft must not block turning them off, and the property field is
    // hidden when off so the user could not fix it. Some(_) = set the
    // property; None = preserve the stored one.
    let property_to_set: Option<Option<String>> = if enabled {
        Some(match property.map(str::trim) {
            None | Some("") => None,
            Some(p) if tasks::is_valid_id_property(p) => Some(p.to_string()),
            Some(p) => {
                return Err(format!(
                    "Invalid ID property name (letters, digits, - and _ only; not a reserved task field): {p}"
                ))
            }
        })
    } else {
        None
    };

    // ONE lock across scan + write: without it, set_task_parent could write a
    // new parent link after this scan sees none and before this save commits,
    // orphaning that hierarchy immediately (design spec §2a). NOT reentrant —
    // count_parent_links below must not (and does not) acquire it again (see
    // its own doc comment).
    let _guard = capture_config::config_write_lock();
    let mut value = capture_config::vault_config(&app_config(paths), id);

    // The property name `value` would carry AFTER this save, resolved via a
    // throwaway clone so task_id_property_name's trim/empty/default rule
    // stays single-sourced instead of re-implemented here, and so a refused
    // guard below leaves `value` itself untouched.
    let resolved_new = {
        let mut prospective = value.clone();
        if let Some(prop) = property_to_set.clone() {
            prospective.task_id_property = prop;
        }
        prospective.task_id_property_name().to_string()
    };
    // Compare RESOLVED names, not raw Options: `None` and `Some("task-id")`
    // name the same property, and a save that merely spells the default
    // explicitly must not read as a re-point. CASE-INSENSITIVELY, too:
    // frontmatter_scalar_ci's id lookup/stamp is already case-insensitive and
    // preserves the on-disk casing, so "Task-ID" -> "task-id" resolves every
    // existing parent-id reference exactly as before — a spelling-only edit
    // can never orphan a link, so it must not trip this guard. is_valid_id_
    // property deliberately ACCEPTS uppercase non-reserved names (its own
    // reserved-key check is already case-folded), so this is the one
    // remaining case-sensitive comparison that needed to fold too.
    let property_changing = !value
        .task_id_property_name()
        .eq_ignore_ascii_case(&resolved_new);
    let disabling = value.task_id_enabled && !enabled;
    // Only a PROPERTY CHANGE or a DISABLE can orphan existing links. ENABLING
    // under an unchanged property is always safe — it makes recorded parent-id
    // references resolvable rather than breaking them. Refusing it would trap a
    // user whose hand-authored hierarchy is invisible precisely BECAUSE ids are
    // off: they could not turn ids on without first deleting the very links they
    // were trying to see (Codex P2, PR #77).
    //
    // A scan failure REFUSES the change — it must never read as "no links".
    // The scan itself only runs when a change is actually guarded (the
    // `count_parent_links` call sits inside this branch, not in the `if`
    // condition above), so an unrelated field save on a vault with an
    // unreadable task file is never blocked by a scan it doesn't need.
    if property_changing || disabling {
        let count = count_parent_links(paths, id)?;
        if count > 0 {
            // Spec §2a: "an inline error naming the count and the remedy."
            let (task_noun, link_noun) = if count == 1 {
                ("task", "link")
            } else {
                ("tasks", "links")
            };
            return Err(format!(
                "This vault has {count} {task_noun} with a parent, referencing Task IDs \
                 under the current property. Clear the parent {link_noun} before changing \
                 the Task ID settings."
            ));
        }
    }

    value.task_id_enabled = enabled;
    if let Some(prop) = property_to_set {
        value.task_id_property = prop;
    }
    let cfg_path = paths
        .config_json
        .as_ref()
        .ok_or_else(|| "Cannot resolve the config directory".to_string())?;
    capture_config::update_vault_config_at(cfg_path, id, value)
        .map_err(|e| format!("Could not save capture settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::fixture;
    use std::path::{Path, PathBuf};

    const VAULT_ID: &str = "deadbeef01234567";

    /// Same shape as parent.rs's local fixture (registry + config.json in a
    /// tempdir, plus the vault's Task ID setting) — but this domain's guard
    /// tests also need the vault's own ON-DISK directory (to delete it and
    /// simulate an unreachable vault), so this returns the vault PathBuf
    /// `test_support::fixture` already hands back, rather than throwing it
    /// away for the id string the way parent.rs's version does.
    fn fixture_with_ids(dir: &Path, enabled: bool, files: &[&str]) -> (ServicePaths, PathBuf) {
        let (paths, vault) = fixture(dir, "MyVault");
        if enabled {
            std::fs::write(
                paths.config_json.as_ref().unwrap(),
                format!(r#"{{ "vaults": {{ "{VAULT_ID}": {{ "taskIdEnabled": true }} }} }}"#),
            )
            .unwrap();
        }
        let root = vault.join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        for f in files {
            let title = f.trim_end_matches(".md");
            write(
                &root,
                f,
                &format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\n---\n"),
            );
        }
        (paths, vault)
    }

    fn fixture_with_ids_disabled(dir: &Path, files: &[&str]) -> (ServicePaths, PathBuf) {
        fixture_with_ids(dir, false, files)
    }

    fn fixture_with_ids_enabled(dir: &Path, files: &[&str]) -> (ServicePaths, PathBuf) {
        fixture_with_ids(dir, true, files)
    }

    /// The vault's tasks root as this fixture always lays it out (no test
    /// here customizes `tasksFolder`) — `vault.join("Tasks")`, not a second
    /// derivation through config.
    fn tasks_root(vault: &Path) -> PathBuf {
        vault.join("Tasks")
    }

    /// Simulate an unreachable vault: remove the vault directory itself, not
    /// just its Tasks subfolder — see `an_unreachable_vault_refuses_rather_
    /// than_reporting_no_links` for why that distinction is the point.
    fn remove_vault_dir(vault: &Path) {
        std::fs::remove_dir_all(vault).unwrap();
    }

    /// The complement: remove only the Tasks subfolder, leaving the vault
    /// itself (and the rest of its contents) present.
    fn remove_tasks_root(vault: &Path) {
        std::fs::remove_dir_all(tasks_root(vault)).unwrap();
    }

    fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn count_parent_links_detects_any_parent_id() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
        );
        assert_eq!(count_parent_links(&paths, VAULT_ID).unwrap(), 0);
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\nparent-id: x\n---\n",
        );
        assert_eq!(count_parent_links(&paths, VAULT_ID).unwrap(), 1);
    }

    #[test]
    fn count_parent_links_detects_a_differently_cased_parent_id_key() {
        // Fix 2 (final whole-branch review, task report): `parent_id_field`
        // matched the literal lowercase `"parent-id:"` case-sensitively, so a
        // hand-authored `Parent-Id:` line undercounted here — letting
        // `set_task_id_config` disable or re-point the property while a real
        // reference still existed, defeating the very guard this lock exists
        // for.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\nParent-Id: x\n---\n",
        );
        assert_eq!(count_parent_links(&paths, VAULT_ID).unwrap(), 1);
    }

    // Review finding 1: the ORIGINAL version of this test appended an
    // archived task carrying parent-id to a fixture that ALREADY had a
    // non-archived parent-id from a sibling `write` call, so its "archived
    // tasks count too" assertion passed regardless of whether archived tasks
    // were scanned at all. The reviewer proved this by mutating the scan to
    // add `.filter(|t| t.status != "archived")` and watching every test in
    // this module — including that one — stay green. This fixture's ONLY
    // `parent-id` is on an archived task, with a non-archived sibling that
    // carries none, so the assertion is meaningful: it goes red the instant
    // archived tasks are excluded (verified by re-applying that exact
    // mutation here — see the task report for the observed red/green).
    #[test]
    fn count_parent_links_counts_an_archived_only_parent_id() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
        );
        write(
            &root,
            "c.md",
            "---\ntype: Task\nstatus: archived\ntitle: \"C\"\nparent-id: y\n---\n",
        );
        assert_eq!(count_parent_links(&paths, VAULT_ID).unwrap(), 1);
    }

    #[test]
    fn enabling_ids_under_an_unchanged_property_is_allowed_with_parent_links() {
        // The catch-22 guard: a hand-authored hierarchy is INVISIBLE while ids
        // are off, so refusing the enable would leave the user unable to reveal
        // it without deleting it first (Codex P2, PR #77). Only a property
        // change or a disable can orphan links.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        // enable, same property -> ALLOWED
        assert!(set_task_id_config(&paths, VAULT_ID, true, None).is_ok());
        // disable -> refused (would hide every own id and orphan the links)
        assert!(set_task_id_config(&paths, VAULT_ID, false, None).is_err());
        // re-point the property -> refused
        assert!(set_task_id_config(&paths, VAULT_ID, true, Some("uid")).is_err());
    }

    #[test]
    fn an_unreachable_vault_refuses_rather_than_reporting_no_links() {
        // Best-effort reads are right for a view, wrong for a guard: an offline
        // vault must not read as "no parent links" (Codex P2, PR #77).
        // Remove the VAULT, not just the tasks subfolder — an absent tasks
        // folder under a reachable vault is legitimately "no tasks" (design
        // spec §2a), so it is vault RESOLUTION that must fail here.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        remove_vault_dir(&vault);
        assert!(count_parent_links(&paths, VAULT_ID).is_err());
    }

    #[test]
    fn an_absent_tasks_folder_under_a_reachable_vault_is_simply_link_free() {
        // The complement: a brand-new vault that has never created a Tasks
        // folder must NOT be blocked from configuring Task IDs.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        remove_tasks_root(&vault); // vault still present
        assert_eq!(count_parent_links(&paths, VAULT_ID).unwrap(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_task_file_refuses_rather_than_reporting_no_links() {
        // guard-vs-view (design spec §2a / list.rs's "a view may degrade; a
        // guard must refuse"): list_tasks_structural aborts the whole scan on
        // an unreadable .md instead of silently skipping it, and this guard
        // must propagate that Err rather than let it collapse to Ok(0) — a
        // scan that never actually finished must not be read as "no links".
        //
        // Root bypasses DAC, so chmod 000 is a no-op there and this probe
        // self-skips — SKIPPED HERE: this sandbox runs every test as root.
        // CI's rust-core job runs unprivileged and exercises the real
        // assertions below (same self-skip idiom as list.rs's own
        // structural_scan_errors_on_an_unreadable_task). Independently
        // verified for this task by re-running just this test as an
        // unprivileged user (`setpriv --reuid=65534 --regid=65534
        // --clear-groups`) — see the task report for that output. The
        // uid-independent counterpart below (review finding 2) exercises the
        // same strict-error guarantee on every uid, root included, since this
        // probe's own self-skip cannot.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
        );
        let locked = root.join("b.md");
        std::fs::write(&locked, "---\ntype: Task\nstatus: new\ntitle: \"B\"\n---\n").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let bypassed = std::fs::read_to_string(&locked).is_ok();
        if bypassed {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
            eprintln!(
                "SKIPPED an_unreadable_task_file_refuses_rather_than_reporting_no_links: \
                 running as root, chmod 000 does not deny access here"
            );
            return;
        }
        let out = count_parent_links(&paths, VAULT_ID);
        // Restore before asserting so the tempdir can clean up either way.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            out.is_err(),
            "an unreadable task must refuse the settings change, not report no links"
        );
    }

    // Review finding 2: the chmod-based test above self-skips as root (this
    // sandbox runs every test as root; CI's rust-core job does not), so
    // mutating the scan's `list_tasks_structural(&root, None)?` to
    // `.unwrap_or_default()` left every test in this module green on a root
    // dev machine — the self-skip can never catch a scan that quietly
    // swallowed its own error. `read_to_string` fails with `InvalidData` for
    // non-UTF-8 bytes regardless of uid/permissions, so this exercises the
    // same strict-error guarantee on every uid, root included — the
    // uid-independent counterpart, mirroring core::tasks::list's own
    // `structural_scan_errors_on_a_non_utf8_task_file`, itself the documented
    // "uid-independent counterpart to structural_scan_errors_on_an_
    // unreadable_task" (list.rs).
    #[test]
    fn a_non_utf8_task_file_refuses_rather_than_reporting_no_links() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\n---\n",
        );
        std::fs::write(root.join("b.md"), [0xff, 0xfe]).unwrap();
        let out = count_parent_links(&paths, VAULT_ID);
        assert!(
            out.is_err(),
            "a non-UTF-8 task file must refuse the settings change, not report no links"
        );
    }

    #[test]
    fn a_no_op_save_is_allowed_even_with_parent_links() {
        // The guard fires only on an actual CHANGE — re-saving the same
        // enabled state and the same (here, default-spelled) property must
        // never be blocked by links it isn't touching.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        assert_eq!(count_parent_links(&paths, VAULT_ID).unwrap(), 1); // links exist
                                                                      // Same enabled (true, from fixture_with_ids_enabled) and the same
                                                                      // property (None resolves to the already-stored default) -> no
                                                                      // change -> allowed even though the vault has parent links.
        assert!(set_task_id_config(&paths, VAULT_ID, true, None).is_ok());
    }

    // Review finding 3 (spec §2a: "an inline error naming the count and the
    // remedy"): the refusal must state HOW MANY tasks carry a parent link,
    // not just that some do.
    #[test]
    fn set_task_id_config_refusal_names_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        write(
            &root,
            "c.md",
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\ntask-id: c\nparent-id: a\n---\n",
        );
        // Disabling is one of the two guarded changes; two tasks (b, c) carry
        // parent-id.
        let err = set_task_id_config(&paths, VAULT_ID, false, None).unwrap_err();
        assert!(
            err.contains('2'),
            "expected the refusal to name the count (2), got: {err}"
        );
    }

    // Review finding 4: the write-strict property validation moved from the
    // shell (task_commands.rs) to core with no test following it to its new
    // home — neither branch of `is_valid_id_property`'s rejection (a bad
    // charset, or a reserved structured-task key) had coverage here.
    #[test]
    fn set_task_id_config_rejects_an_invalid_or_reserved_property() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, _vault) = fixture_with_ids_disabled(dir.path(), &[]);
        // Charset: a space is not letters/digits/-/_ (mirrors
        // core::tasks::id's own is_valid_id_property_charset_and_reserved).
        let err = set_task_id_config(&paths, VAULT_ID, true, Some("task id")).unwrap_err();
        assert!(
            err.contains("Invalid ID property name"),
            "expected the charset error, got: {err}"
        );
        // Reserved: "status" is one of RESERVED_TASK_KEYS.
        let err = set_task_id_config(&paths, VAULT_ID, true, Some("status")).unwrap_err();
        assert!(
            err.contains("Invalid ID property name"),
            "expected the reserved-key error, got: {err}"
        );
    }

    // Review finding 6: the guard compares RESOLVED property names, not raw
    // `Option`s — a stored `None` and an incoming `Some("task-id")` both name
    // the default and must not read as a re-point. This seam is only
    // observable on a vault WITH parent links: with none present the guard
    // never fires either way (the same vacuity finding 1 pins), so both tests
    // below plant a parent link before asserting the save is ALLOWED, not
    // refused.
    #[test]
    fn resolved_name_treats_an_explicit_default_as_unchanged() {
        // stored: None (the default — fixture_with_ids_enabled never sets
        // taskIdProperty); incoming: Some("task-id"), spelling that same
        // default out loud.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        assert!(
            set_task_id_config(&paths, VAULT_ID, true, Some("task-id")).is_ok(),
            "None -> Some(\"task-id\") names the same property and must not refuse"
        );
    }

    #[test]
    fn resolved_name_treats_the_default_spelled_either_way_as_unchanged() {
        // The inverse direction: stored explicitly as Some("task-id") (a
        // hand-edited config.json spelling out the default); incoming: None
        // (omit the field, which also resolves to the default).
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            format!(
                r#"{{ "vaults": {{ "{VAULT_ID}": {{ "taskIdEnabled": true, "taskIdProperty": "task-id" }} }} }}"#
            ),
        )
        .unwrap();
        let root = tasks_root(&vault);
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        assert!(
            set_task_id_config(&paths, VAULT_ID, true, None).is_ok(),
            "Some(\"task-id\") -> None resolves to the same property and must not refuse"
        );
    }

    // Fix (task-7-fixb): the guard compared resolved property names
    // CASE-SENSITIVELY, so a vault storing "Task-ID" saving "task-id" (or the
    // reverse) was classified as a re-point and refused even though nothing
    // it points at changes — id lookup/stamping is already case-insensitive
    // (frontmatter_scalar_ci) and preserves the on-disk casing, so a
    // spelling-only edit can never orphan a parent-id reference. Both
    // directions are exercised on a vault WITH parent links (the guard is a
    // no-op otherwise, same vacuity note as resolved_name_treats_*).
    #[test]
    fn a_casing_only_property_change_uppercase_to_lowercase_is_allowed_with_parent_links() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            format!(
                r#"{{ "vaults": {{ "{VAULT_ID}": {{ "taskIdEnabled": true, "taskIdProperty": "Task-ID" }} }} }}"#
            ),
        )
        .unwrap();
        let root = tasks_root(&vault);
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        assert!(
            set_task_id_config(&paths, VAULT_ID, true, Some("task-id")).is_ok(),
            "\"Task-ID\" -> \"task-id\" is a spelling-only change and must not refuse"
        );
    }

    #[test]
    fn a_casing_only_property_change_lowercase_to_uppercase_is_allowed_with_parent_links() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            format!(
                r#"{{ "vaults": {{ "{VAULT_ID}": {{ "taskIdEnabled": true, "taskIdProperty": "task-id" }} }} }}"#
            ),
        )
        .unwrap();
        let root = tasks_root(&vault);
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        assert!(
            set_task_id_config(&paths, VAULT_ID, true, Some("Task-ID")).is_ok(),
            "\"task-id\" -> \"Task-ID\" is a spelling-only change and must not refuse"
        );
    }

    // The complement: a GENUINE re-point (a different property, not just a
    // different casing of the same one) must still be refused with parent
    // links present — the fix must only fold case, never widen "unchanged"
    // to cover an actual property swap. (enabling_ids_under_an_unchanged_
    // property_is_allowed_with_parent_links above already pins the
    // default-property case; this pins it again explicitly under a
    // non-default stored property so the two tests can't quietly rot into
    // covering the same path.)
    #[test]
    fn a_genuine_property_re_point_is_still_refused_with_parent_links() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture(dir.path(), "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            format!(
                r#"{{ "vaults": {{ "{VAULT_ID}": {{ "taskIdEnabled": true, "taskIdProperty": "task-id" }} }} }}"#
            ),
        )
        .unwrap();
        let root = tasks_root(&vault);
        std::fs::create_dir_all(&root).unwrap();
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\nparent-id: a\n---\n",
        );
        assert!(
            set_task_id_config(&paths, VAULT_ID, true, Some("uid")).is_err(),
            "\"task-id\" -> \"uid\" is a real re-point and must still be refused"
        );
    }
}
