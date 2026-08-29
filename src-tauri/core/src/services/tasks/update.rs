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
            // `set_task_parent`'s own clear branch, including the on-disk
            // casing resolution (Fix 2, final whole-branch review task
            // report): a clear must target whatever casing the file actually
            // carries, or a hand-authored `Parent-Id:`/`Parent:` line
            // survives untouched while the app believes it cleared the
            // relationship.
            let content = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(e) => {
                    return Err(parent_write_error(
                        fields_saved,
                        format!("Cannot read task: {e}"),
                    ))
                }
            };
            let (id_key, link_key) = parent::parent_field_keys(&content);
            match tasks::update_task_fields(
                &root,
                path,
                &[(id_key.as_str(), None), (link_key.as_str(), None)],
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
                // Unconditional re-check under the lock, on the
                // freshly-committed graph. Cycle-checking here already
                // mirrored `set_task_parent`'s own; Fix 2 (PR #78) folded
                // BOTH into one shared `recheck_set_or_update`, closing the
                // "hand-copied closure, fixed in one place, not the other"
                // seam this module's own doc comment used to warn about —
                // this IS the actual IPC-wired path the Parent picker's
                // Change/Set-parent control uses (`task_commands.rs::
                // update_task` -> `services::update_task`), so the shared
                // function's archived-status half matters here as much as
                // it does at `set_task_parent`'s own call site.
                || parent::recheck_set_or_update(&root, &prop, &child_p, &parent_p),
                |resolved| {
                    // The child's pair. `ensure_id` rides along so a legacy
                    // child picks up its own id in the same write — reported
                    // back so `TaskWriteResult.id` reflects a bootstrap even
                    // when `updates` above was empty (a parent-only patch).
                    // `parent_id_ref` is already the exact YAML text that
                    // means the same thing as the parent's own id line
                    // (`tasks::mirror_id_reference`, resolved once inside
                    // `resolve_parent_for_write` alongside `parent_id`). The
                    // key CASING is resolved fresh here, from a read of
                    // `child_p`'s CURRENT content (Fix 2, final whole-branch
                    // review task report) — this closure runs after the lock,
                    // and nothing before it has written to this CHILD, so the
                    // read reflects whatever the file actually carries.
                    let content = std::fs::read_to_string(&child_p)
                        .map_err(|e| format!("Cannot read task: {e}"))?;
                    let (id_key, link_key) = parent::parent_field_keys(&content);
                    let link_value = crate::yaml_scalar::yaml_quote(&resolved.link);
                    tasks::update_task_fields(
                        &root,
                        &child_p,
                        &[
                            (id_key.as_str(), Some(resolved.parent_id_ref.as_str())),
                            (link_key.as_str(), Some(link_value.as_str())),
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
mod tests;
