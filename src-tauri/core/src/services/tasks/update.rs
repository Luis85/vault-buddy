//! `update_task`'s WRITE half: vault resolution, containment, the ordinary
//! field write, and the optional parent-relationship change. The part of the
//! shell's `update_task` IPC command that needs no Tauri types belongs here
//! (the shell's own `tasks_root_for` reads the real `%APPDATA%` config and
//! cannot be unit-tested there) — the same split `id_config.rs` established
//! for `set_task_id_config`. The shell keeps everything BEFORE this: the
//! user-facing validation/error strings and the `TaskPatchDto` -> `updates`
//! slice assembly (`patch_is_empty`/field validation stay in
//! `task_commands.rs`, already testable there).
//!
//! **Ordering, and why it matters for a COMBINED patch** (a title change and
//! a parent assignment in one call, which an IPC caller may legally send):
//! writing the ordinary fields first and only THEN failing parent validation
//! would commit the title while returning an error — the caller reverts its
//! whole optimistic patch and reports total failure, yet the title actually
//! changed on disk (Codex P2, PR #77). So:
//!   1. The parent's READ-ONLY validation (phase 1 of the shared `parent`
//!      module path) runs FIRST, before either write below.
//!   2. Only then does the ordinary field write land, if any.
//!   3. Only then does the parent write (phases 2-3: enable, stamp, compose)
//!      run, if any.
//!
//! A parent failure at step 3 is a real partial state no ordering removes
//! without a journal (the fields, if any, are already on disk) — reported in
//! the fields-saved form so the caller does not claim total failure, the same
//! wording shape `useTaskDetail`'s `saveErrorMessage` already uses for a
//! failed list move.

use std::path::{Path, PathBuf};

use super::parent::{self, ParentWriteCtx};
use super::{assert_root_if_exists, tasks_root_for};
use crate::services::ServicePaths;
use crate::tasks;

/// Which relationship edit this call carries, if any. Three states, not a
/// bool pair: `Set`/`Clear`/`Keep` cannot be spelled ambiguously, whereas
/// `(Option<PathBuf>, bool)` admits a nonsensical set-and-clear.
pub enum ParentOp {
    Set(PathBuf),
    Clear,
    Keep,
}

/// `update_task`'s result. `id` keeps its pre-Task-7 meaning (the task's
/// effective id: freshly stamped when the vault opts in and it lacked one, or
/// the existing value; `None` when ids are off) — now sourced from whichever
/// write actually stamped it, since a parent `Set` can bootstrap ids AFTER an
/// ordinary field write already ran (see `update_task`'s doc comment).
/// `parent_id`/`parent_link` are the pair actually written THIS call — `None`
/// for a `Keep` (an ordinary field save must never misreport the task's
/// standing parent) as well as for a `Clear`. `ids_enabled` is true only when
/// THIS call turned Task IDs on for the vault — the frontend cannot infer it,
/// since an already-enabled vault with an unstamped parent returns the
/// identical shape (design spec §2).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskWriteResult {
    pub id: Option<String>,
    pub parent_id: Option<String>,
    pub parent_link: Option<String>,
    pub ids_enabled: bool,
}

/// Apply an already-validated field patch (`updates`, assembled by the shell
/// from `TaskPatchDto` — each value pre-rendered exactly as it will be
/// written, e.g. YAML-quoted) and/or a parent relationship change to a task on
/// disk. See the module doc comment for the phase ordering.
pub fn update_task(
    paths: &ServicePaths,
    vault_id: &str,
    path: &Path,
    updates: &[(&str, Option<&str>)],
    parent: ParentOp,
) -> Result<TaskWriteResult, String> {
    let (vault_path, root, cfg) = tasks_root_for(paths, vault_id)?;
    // Mirrors every other task command: safe_recording_root is only lexical,
    // so a tasks folder resolving outside the vault must fail here rather
    // than be written into.
    assert_root_if_exists(&vault_path, &root)?;

    // ---- Step 1: the parent's read-only validation, before ANY write. A
    // rejected parent (self-parent, cycle, ambiguous/unassignable id) must
    // leave the ordinary fields below untouched. ----
    let validated = match &parent {
        ParentOp::Set(parent_path) => Some(parent::validate_parent_assignment(
            &root,
            &cfg,
            path,
            parent_path,
        )?),
        ParentOp::Clear | ParentOp::Keep => None,
    };

    // ---- Step 2: the ordinary field write, if any. ----
    let id_property =
        tasks::id_property_for_generation(cfg.task_id_enabled, cfg.task_id_property_name());
    let field_id = if updates.is_empty() {
        None
    } else {
        tasks::update_task_fields(&root, path, updates, id_property)?
    };
    let fields_saved = !updates.is_empty();

    // ---- Step 3: the parent write (clear / set), if any. ----
    let (parent_id, parent_link, ids_enabled, parent_child_id) = match parent {
        ParentOp::Keep => (None, None, false, None),
        ParentOp::Clear => {
            // No parent to validate, no ids needed — `ensure_id: None`, a
            // clear removes a relationship, it does not edit the task (the
            // same reason a status toggle never stamps). Mirrors
            // `set_task_parent`'s own clear branch.
            match tasks::update_task_fields(
                &root,
                path,
                &[("parent-id", None), ("parent", None)],
                None,
            ) {
                Ok(_) => (None, None, false, None),
                Err(e) => return Err(parent_write_error(fields_saved, e)),
            }
        }
        ParentOp::Set(_) => {
            let (parent_p, child_p, prop) =
                validated.expect("Set was validated in step 1, before any write");
            let ctx = ParentWriteCtx {
                paths,
                vault_id,
                vault_path: &vault_path,
                root: &root,
                prop: &prop,
                phase1_cfg: &cfg,
            };
            let attempt = parent::resolve_parent_for_write(
                &ctx,
                &parent_p,
                &child_p,
                || {
                    // Unconditional re-check under the lock, on the
                    // freshly-committed graph — mirrors set_task_parent
                    // exactly (design spec §2): two parent assignments can
                    // overlap, and only a re-check under the lock sees the
                    // other's committed write.
                    let all = tasks::list_tasks_structural(&root, Some(&prop))?;
                    Ok(tasks::would_create_cycle(
                        &tasks::parent_index_for_validation(&all),
                        &child_p,
                        &parent_p,
                    ))
                },
                |resolved| {
                    // The child's pair. `ensure_id` rides along so a legacy
                    // child picks up its own id in the same write — reported
                    // back so `TaskWriteResult.id` reflects a bootstrap even
                    // when `updates` above was empty (a parent-only patch).
                    tasks::update_task_fields(
                        &root,
                        &child_p,
                        &[
                            (
                                "parent-id",
                                Some(&tasks::quote_id_if_needed(&resolved.parent_id) as &str),
                            ),
                            (
                                "parent",
                                Some(&crate::yaml_scalar::yaml_quote(&resolved.link) as &str),
                            ),
                        ],
                        Some(&prop),
                    )
                },
            );
            match attempt {
                Ok((resolved, child_id)) => (
                    Some(resolved.parent_id),
                    Some(resolved.link),
                    resolved.ids_enabled,
                    child_id,
                ),
                Err(e) => return Err(parent_write_error(fields_saved, e)),
            }
        }
    };

    Ok(TaskWriteResult {
        // The parent step's stamp (when present) is the more authoritative
        // answer: it runs AFTER phase 2's enable, so it is the one that
        // actually stamps a child that had no id before this very call — a
        // field-only stamp from step 2 (which ran before any enable a Set
        // might trigger) would otherwise under-report a bootstrap.
        id: parent_child_id.or(field_id),
        parent_id,
        parent_link,
        ids_enabled,
    })
}

/// A parent write that fails is a genuine partial state no ordering removes
/// without a journal WHEN fields already committed (step 2 ran) — report it
/// in the fields-saved form so the caller does not claim total failure,
/// reusing `useTaskDetail`'s `saveErrorMessage` wording shape for a failed
/// list move. Conditional, unlike the brief's literal unconditional prefix:
/// a parent-ONLY patch (no ordinary fields) that fails to set the parent
/// saved nothing, and "Saved fields, but..." would misreport that.
fn parent_write_error(fields_saved: bool, e: String) -> String {
    if fields_saved {
        format!("Saved fields, but couldn't set the parent: {e}")
    } else {
        format!("Couldn't set the parent: {e}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::test_support::fixture;

    const VAULT: &str = "deadbeef01234567";

    /// Registry + config.json in a tempdir, the vault's Task ID setting, and
    /// any task files the case needs — the `parent/tests.rs` fixture shape,
    /// reproduced here rather than shared: each services test module owns its
    /// own minimal fixture (the established convention — see `id_config.rs`'s
    /// near-identical local copy).
    fn fixture_with_ids(dir: &Path, enabled: bool, files: &[&str]) -> (ServicePaths, String) {
        let (paths, vault) = fixture(dir, "MyVault");
        if enabled {
            std::fs::write(
                paths.config_json.as_ref().unwrap(),
                format!(r#"{{ "vaults": {{ "{VAULT}": {{ "taskIdEnabled": true }} }} }}"#),
            )
            .unwrap();
        }
        let root = vault.join("Tasks");
        std::fs::create_dir_all(&root).unwrap();
        for f in files {
            let title = f.trim_end_matches(".md");
            std::fs::write(
                root.join(f),
                format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\n---\n"),
            )
            .unwrap();
        }
        (paths, VAULT.to_string())
    }

    fn fixture_with_ids_disabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
        fixture_with_ids(dir, false, files)
    }

    fn fixture_with_ids_enabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
        fixture_with_ids(dir, true, files)
    }

    fn tasks_root(paths: &ServicePaths, vault: &str) -> PathBuf {
        tasks_root_for(paths, vault).unwrap().1
    }

    #[test]
    fn a_combined_patch_with_an_invalid_parent_writes_nothing() {
        // Title + a self-parent in one call: validation runs first, so the
        // title must NOT be committed (Codex P2, PR #77). (Brief deviation:
        // the plan's literal test called a nonexistent `apply_task_patch` —
        // the shell cannot be unit-tested (tasks_root_for reads the real
        // %APPDATA% config) and the write half now lives here, per the
        // controller amendment. Same invariant, exercised against this
        // function directly with the `updates` slice the shell would build.)
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["a.md"]);
        let root = tasks_root(&paths, &vault);
        let p = root.join("a.md");
        let before = std::fs::read_to_string(&p).unwrap();
        let quoted = crate::capture_note::yaml_quote("Renamed");
        let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
        let result = update_task(&paths, &vault, &p, &updates, ParentOp::Set(p.clone()));
        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // title untouched
    }

    #[test]
    fn a_plain_field_patch_is_unaffected_by_the_parent_machinery() {
        // ParentOp::Keep must reproduce the exact pre-Task-7 behavior: only
        // the field write runs, and the id reflects it.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["a.md"]);
        let root = tasks_root(&paths, &vault);
        let p = root.join("a.md");
        let quoted = crate::capture_note::yaml_quote("Renamed");
        let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
        let result = update_task(&paths, &vault, &p, &updates, ParentOp::Keep).unwrap();
        assert!(
            result.id.is_some(),
            "an id-enabled vault stamps on any edit"
        );
        assert_eq!(result.parent_id, None);
        assert_eq!(result.parent_link, None);
        assert!(!result.ids_enabled);
        assert!(std::fs::read_to_string(&p)
            .unwrap()
            .contains("title: \"Renamed\""));
    }

    #[test]
    fn a_parent_only_set_bootstraps_ids_and_reports_the_childs_own_id() {
        // No ordinary fields — the parent-only patch the Parent picker's
        // Change control sends. `id` must reflect the child's own stamp even
        // though step 2 (the field write) never ran (Codex P2, PR #77 class:
        // a stale pre-enable snapshot would otherwise under-report it).
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        let child = root.join("c.md");
        let result = update_task(
            &paths,
            &vault,
            &child,
            &[],
            ParentOp::Set(root.join("p.md")),
        )
        .unwrap();
        assert!(result.ids_enabled, "this call turned Task IDs on");
        let pid = result.parent_id.expect("a parent id was written");
        assert!(!pid.is_empty());
        // `parent_link` is the RAW composed link (compose_parent_link's own
        // output, e.g. `[[Tasks/p]]`) — YAML-quoting is a detail of the WRITE
        // (the closure below wraps it via yaml_scalar::yaml_quote before it
        // reaches disk), not part of what this field reports back.
        assert!(result.parent_link.unwrap().starts_with("[["));
        let cid = result.id.expect("the child's own id is reported");
        assert!(std::fs::read_to_string(&child)
            .unwrap()
            .contains(&format!("parent-id: {pid}")));
        assert!(std::fs::read_to_string(&child)
            .unwrap()
            .contains(&format!("task-id: {cid}")));
    }

    #[test]
    fn a_parent_only_clear_removes_both_keys_and_reports_no_parent() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        let child = root.join("c.md");
        update_task(
            &paths,
            &vault,
            &child,
            &[],
            ParentOp::Set(root.join("p.md")),
        )
        .unwrap();
        assert!(std::fs::read_to_string(&child)
            .unwrap()
            .contains("parent-id"));

        let result = update_task(&paths, &vault, &child, &[], ParentOp::Clear).unwrap();
        assert_eq!(result.parent_id, None);
        assert_eq!(result.parent_link, None);
        assert!(!result.ids_enabled);
        let after = std::fs::read_to_string(&child).unwrap();
        assert!(!after.contains("parent-id"));
        assert!(!after.contains("parent:"));
    }

    #[test]
    fn a_vanished_parent_refuses_at_validation_and_never_claims_fields_saved() {
        // A parent that vanished between load and write fails phase 1
        // (canonical_task_in_root can't resolve it) — BEFORE the field write
        // runs — so this is a validation failure, not the step-3 partial-
        // state case `parent_write_error` exists for; asserted here so that
        // distinction stays pinned (a regression that let this reach step 3
        // instead would silently start committing fields ahead of a doomed
        // parent write).
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["a.md", "p.md"]);
        let root = tasks_root(&paths, &vault);
        let p = root.join("a.md");
        let parent = root.join("p.md");
        let before = std::fs::read_to_string(&p).unwrap();
        let quoted = crate::capture_note::yaml_quote("Renamed");
        let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
        std::fs::remove_file(&parent).unwrap();
        let err = update_task(&paths, &vault, &p, &updates, ParentOp::Set(parent))
            .err()
            .expect("a vanished parent must fail the write");
        assert!(!err.starts_with("Saved fields"), "{err}");
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // title untouched
    }

    #[test]
    fn parent_write_error_names_the_fields_saved_state() {
        // The step-3 partial-state wording directly: a parent write that
        // fails AFTER the ordinary fields already committed must not claim
        // total failure (Codex P2, PR #77) — but a parent-ONLY patch (no
        // fields in `updates` at all) that fails must not falsely claim
        // fields were saved either. Exercising this through a real end-to-end
        // I/O failure at step 3 specifically (validation already having
        // passed) would need a genuine race — `resolve_parent_for_write`'s
        // own mid-flight races are already pinned directly in
        // `parent/tests.rs` — so the conditional wording itself, the thing
        // Task 7 actually adds, is pinned here instead.
        assert_eq!(
            parent_write_error(true, "boom".to_string()),
            "Saved fields, but couldn't set the parent: boom"
        );
        assert_eq!(
            parent_write_error(false, "boom".to_string()),
            "Couldn't set the parent: boom"
        );
    }
}
