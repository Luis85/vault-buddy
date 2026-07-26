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
    fn a_parent_set_mirrors_an_implicitly_typed_id_bare_so_types_agree() {
        // review, PR #77: `task-id: 123` is UNQUOTED, so YAML resolves
        // it as the NUMBER 123 — not the string "123" the old decode-then-
        // requote pipeline assumed. That pipeline decoded the parent's id to
        // the Rust string "123" and re-derived a YAML form from THAT decoded
        // string alone (a charset/keyword heuristic), losing the fact that
        // the source was never quoted, and wrote `parent-id: "123"` — a
        // STRING — so an equality-based Dataview query between the two
        // properties stopped matching.
        //
        // The assertion below is about the RAW on-disk text the child
        // receives, not a decode-and-compare round trip:
        // `parent_id_field(&child) == Some("123")` would ALSO pass under the
        // OLD, buggy `parent-id: "123"` output (both decode to the identical
        // Rust string "123"), which is exactly what let a type-mismatching
        // bug hide behind a passing round-trip test once already.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
        let root = tasks_root(&paths, &vault);
        let parent = root.join("p.md");
        std::fs::write(
            &parent,
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: 123\n---\n",
        )
        .unwrap();
        let child = root.join("c.md");
        update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
        let out = std::fs::read_to_string(&child).unwrap();
        assert!(
            out.contains("parent-id: 123\n"),
            "must mirror the parent's own unquoted, number-typed token, got: {out}"
        );
        assert!(
            !out.contains("parent-id: \"123\""),
            "must not retype the number as a string: {out}"
        );
    }

    #[test]
    fn a_parent_set_mirrors_a_tag_decorated_id_verbatim() {
        // `task-id: !!str 123` forces the SOURCE to resolve as the STRING
        // "123" via an explicit YAML tag. The old pipeline's strict decoder
        // does not understand tags — it treats the whole thing as opaque
        // plain-scalar text — so it decoded to the literal Rust string
        // "!!str 123" (tag syntax included) and quoted THAT for the child:
        // `parent-id: "!!str 123"`, a string whose CONTENT is tag syntax,
        // resolving to neither "123" nor anything the parent's own value
        // equals. Mirroring the raw text lets the child's copy resolve
        // through the identical tag.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
        let root = tasks_root(&paths, &vault);
        let parent = root.join("p.md");
        std::fs::write(
            &parent,
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: !!str 123\n---\n",
        )
        .unwrap();
        let child = root.join("c.md");
        update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
        let out = std::fs::read_to_string(&child).unwrap();
        assert!(
            out.contains("parent-id: !!str 123\n"),
            "must mirror the parent's tag-decorated token verbatim, got: {out}"
        );
    }

    #[test]
    fn a_parent_set_strips_the_anchor_but_mirrors_the_value() {
        // `&stable abc` NAMES the node "stable" so a `*stable` alias
        // elsewhere in the SAME document can reference it. Copying the
        // annotation verbatim into the child's own frontmatter would define
        // a SECOND anchor of that name there. The value itself, `abc`, is
        // what a reference may legitimately copy — decoding the anchor away
        // and mirroring `abc` resolves identically to the parent's own value
        // (both the string "abc").
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
        let root = tasks_root(&paths, &vault);
        let parent = root.join("p.md");
        std::fs::write(
            &parent,
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: &stable abc\n---\n",
        )
        .unwrap();
        let child = root.join("c.md");
        update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
        let out = std::fs::read_to_string(&child).unwrap();
        assert!(
            out.contains("parent-id: abc\n"),
            "must strip the anchor annotation and mirror only its value, got: {out}"
        );
        assert!(
            !out.contains("&stable"),
            "must never define a second anchor of the same name: {out}"
        );
    }

    #[test]
    fn a_parent_set_strips_a_trailing_comment_before_mirroring() {
        // Trap check: `raw_scalar_field` (capture_note.rs) does NOT strip a
        // trailing inline comment on the raw text it returns — it only trims
        // whitespace. Mirroring that raw text VERBATIM would copy the
        // parent's own edit-history comment into the child's
        // machine-managed `parent-id` line.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["c.md"]);
        let root = tasks_root(&paths, &vault);
        let parent = root.join("p.md");
        std::fs::write(
            &parent,
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: 123 # was xyz\n---\n",
        )
        .unwrap();
        let child = root.join("c.md");
        update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
        let out = std::fs::read_to_string(&child).unwrap();
        assert!(
            out.contains("parent-id: 123\n"),
            "must strip the trailing comment before mirroring, got: {out}"
        );
        assert!(
            !out.contains("was xyz"),
            "the parent's own comment must never leak into the child: {out}"
        );
    }

    #[test]
    fn a_parent_set_replaces_an_existing_differently_cased_line_instead_of_duplicating_it() {
        // Fix 2 (final whole-branch review, task report): `update_task`'s own
        // SET branch (update.rs, not `set_task_parent`) needs the identical
        // on-disk-casing fix — `update_task_fields`/`set_fields` matches a key
        // case-SENSITIVELY, so writing the canonical lowercase `parent-id`
        // onto a child that already carries `Parent-Id:` would insert a
        // case-mismatched DUPLICATE rather than replacing the stale line.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md"]);
        let root = tasks_root(&paths, &vault);
        let parent = root.join("p.md");
        let child = root.join("c.md");
        std::fs::write(
            &child,
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\nParent-Id: old99999\n---\n",
        )
        .unwrap();
        update_task(&paths, &vault, &child, &[], ParentOp::Set(parent)).unwrap();
        let out = std::fs::read_to_string(&child).unwrap();
        let id_lines: Vec<&str> = out
            .lines()
            .filter(|l| l.to_ascii_lowercase().starts_with("parent-id:"))
            .collect();
        assert_eq!(
            id_lines.len(),
            1,
            "must replace the existing line, not insert a case-mismatched duplicate: got {out}"
        );
        assert!(
            id_lines[0].starts_with("Parent-Id:"),
            "must preserve the file's own on-disk casing, got {out}"
        );
        assert!(
            !id_lines[0].contains("old99999"),
            "the stale value must be replaced, got {out}"
        );
    }

    #[test]
    fn a_parent_only_clear_removes_both_keys_and_reports_no_parent() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        let child = root.join("c.md");
        let set_result = update_task(
            &paths,
            &vault,
            &child,
            &[],
            ParentOp::Set(root.join("p.md")),
        )
        .unwrap();
        // Review finding 3: the vault (fixture_with_ids_enabled) ALREADY had
        // Task IDs on before this Set, so idsEnabled must read false here —
        // this is the only place in the suite a Set on an already-enabled
        // vault checks that field at all (the sibling bootstrap test,
        // `a_parent_only_set_bootstraps_ids_and_reports_the_childs_own_id`,
        // only covers the ids-were-OFF/turned-on arm). A hardcoded
        // `resolved.ids_enabled` -> `true` at update.rs:177 left every one
        // of the 615 core tests green without this assertion — a false
        // "Task IDs were turned on for this vault" disclosure on every
        // parent assignment in an already-enabled vault.
        assert!(
            !set_result.ids_enabled,
            "ids were already on before this call, not turned on by it"
        );
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
    fn a_parent_clear_removes_a_differently_cased_existing_line_not_a_lowercase_no_op() {
        // The clear-branch counterpart to the SET-branch casing fix above:
        // `update_task`'s own `ParentOp::Clear` arm must target whatever
        // casing is ACTUALLY on disk, or the stale hand-authored line
        // survives untouched while the app believes it cleared the
        // relationship.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        let child = root.join("c.md");
        std::fs::write(
            &child,
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\nParent-Id: x\nParent: \"[[p]]\"\n---\n",
        )
        .unwrap();
        let result = update_task(&paths, &vault, &child, &[], ParentOp::Clear).unwrap();
        assert_eq!(result.parent_id, None);
        assert_eq!(result.parent_link, None);
        let out = std::fs::read_to_string(&child).unwrap();
        assert!(
            !out.to_ascii_lowercase().contains("parent-id:"),
            "the differently-cased parent-id line must be removed, got {out}"
        );
        assert!(
            !out.to_ascii_lowercase().contains("\nparent:"),
            "the differently-cased parent line must be removed, got {out}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_parent_stamp_failure_after_a_committed_field_write_reports_fields_saved() {
        // Review finding 2: `fields_saved` (computed at update.rs:103, right
        // above) had no test driving a REAL step-3 I/O failure after a REAL
        // step-2 commit — `parent_write_error_names_the_fields_saved_state`
        // below only unit-tests the formatting helper directly, so
        // mutating `let fields_saved = !updates.is_empty();` to
        // `let fields_saved = false;` left every one of the 615 core tests
        // green (this file's own report documents that mutation run).
        //
        // Constructed by making the PARENT's own list folder read+execute
        // only: phase 1 (`validate_parent_assignment`) only ever READS —
        // canonicalize, `list_tasks_structural`'s walk, the unassignable
        // forecast — so it passes. The CHILD lives directly in the
        // (writable) tasks root, so step 2's ordinary field write (the
        // title) commits. Only THEN does phase 3a's `ensure_id` try to
        // stamp the parent — which must CREATE a temp file beside it via
        // `write_atomic_replacing` — and that fails with EACCES, so the
        // combined call reports a partial success instead of a clean one.
        //
        // Root bypasses DAC (this sandbox runs every test as root), so a
        // write-probe into the locked directory decides whether to
        // self-skip — the loud, restore-before-assert idiom
        // `services/tasks/id_config.rs`'s chmod tests use
        // (id_config.rs:355-382), adapted to probe a WRITE (what this test
        // denies) rather than a read, matching how `tasks/lists/relocate.rs`
        // varies the very same idiom for its own write-denial tests. CI's
        // rust-core job runs unprivileged and exercises the real assertions
        // below; independently verified for this task by re-running under
        // `setpriv --reuid=65534 --regid=65534 --clear-groups` (see the task
        // report for that output).
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["a.md"]);
        let root = tasks_root(&paths, &vault);
        let locked_dir = root.join("Locked");
        std::fs::create_dir_all(&locked_dir).unwrap();
        let parent = locked_dir.join("p.md");
        std::fs::write(&parent, "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n").unwrap();
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

        let probe = locked_dir.join(".probe");
        let bypassed = std::fs::write(&probe, b"x").is_ok();
        if bypassed {
            let _ = std::fs::remove_file(&probe);
            std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
            eprintln!(
                "SKIPPED a_parent_stamp_failure_after_a_committed_field_write_reports_fields_saved: \
                 running as root, chmod 555 does not deny directory writes here"
            );
            return;
        }

        let child = root.join("a.md");
        let quoted = crate::capture_note::yaml_quote("Renamed");
        let updates: Vec<(&str, Option<&str>)> = vec![("title", Some(quoted.as_str()))];
        let outcome = update_task(&paths, &vault, &child, &updates, ParentOp::Set(parent));

        // Restore before asserting so the tempdir can clean up either way.
        std::fs::set_permissions(&locked_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let err = outcome
            .err()
            .expect("the parent stamp must fail under a read-only list folder");
        assert!(
            err.starts_with("Saved fields, but couldn't set the parent:"),
            "got {err}"
        );
        assert!(err.contains("Permission denied"), "got {err}");
        assert!(
            std::fs::read_to_string(&child)
                .unwrap()
                .contains("title: \"Renamed\""),
            "the committed field write must survive the later parent failure"
        );
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

    #[test]
    fn the_under_lock_recheck_refuses_a_cycle_a_concurrent_write_would_otherwise_create() {
        // Fix 4 (final whole-branch review, task report): `update_task`'s OWN
        // under-lock re-check (the closure above, which reads `tasks::
        // parent_index_for_validation`) is a SEPARATE call site from
        // `set_task_parent`'s (`services::tasks::parent::mod.rs`) — this
        // module builds its own `resolve_parent_for_write` closure rather
        // than delegating to that one. The sibling regression pinned in
        // `parent/tests.rs` covers ONLY that other call site; a mutation here
        // (validation index -> display index) is invisible to it, since
        // nothing in this file's existing coverage exercises a genuine
        // concurrent write landing in the narrow phase-1-to-lock window this
        // recheck exists to close. See that sibling test's own doc comment
        // for the full mechanics (a pre-existing hand-authored X<->Y cycle,
        // Z with no parent yet, and a race between "Z's parent = X" and "Y's
        // parent = Z") — reproduced here against `update_task` instead of
        // `set_task_parent`, since the two never share a recheck closure.
        for _ in 0..60 {
            let dir = tempfile::tempdir().unwrap();
            let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
            let root = tasks_root(&paths, &vault);
            std::fs::write(
                root.join("x.md"),
                "---\ntype: Task\nstatus: new\ntitle: \"X\"\ntask-id: x\nparent-id: y\n---\n",
            )
            .unwrap();
            std::fs::write(
                root.join("y.md"),
                "---\ntype: Task\nstatus: new\ntitle: \"Y\"\ntask-id: y\nparent-id: x\n---\n",
            )
            .unwrap();
            std::fs::write(
                root.join("z.md"),
                "---\ntype: Task\nstatus: new\ntitle: \"Z\"\ntask-id: z\n---\n",
            )
            .unwrap();
            let x = root.join("x.md");
            let y = root.join("y.md");
            let z = root.join("z.md");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

            let thread_a = {
                let paths = paths.clone();
                let vault = vault.clone();
                let (z, x) = (z.clone(), x.clone());
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    update_task(&paths, &vault, &z, &[], ParentOp::Set(x))
                })
            };
            let thread_b = {
                let paths = paths.clone();
                let vault = vault.clone();
                let (y, z) = (y.clone(), z.clone());
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    update_task(&paths, &vault, &y, &[], ParentOp::Set(z))
                })
            };
            let _ = thread_a.join().unwrap();
            let _ = thread_b.join().unwrap();

            let x_parent = tasks::parent_id_field(&std::fs::read_to_string(&x).unwrap());
            let y_parent = tasks::parent_id_field(&std::fs::read_to_string(&y).unwrap());
            let z_parent = tasks::parent_id_field(&std::fs::read_to_string(&z).unwrap());
            let closed_the_cycle = x_parent.as_deref() == Some("y")
                && y_parent.as_deref() == Some("z")
                && z_parent.as_deref() == Some("x");
            assert!(
                !closed_the_cycle,
                "a concurrent pair of parent assignments closed a real cycle \
                 X -> Y -> Z -> X: x.parent={x_parent:?} y.parent={y_parent:?} \
                 z.parent={z_parent:?}"
            );
        }
    }
}
