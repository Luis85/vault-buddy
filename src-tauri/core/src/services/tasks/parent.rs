//! The parent-assignment write path: `set_task_parent` (set or clear a Task's
//! parent) plus the shared resolve-the-parent helper the create path will
//! reuse, and the post-move link repair. Its own module because the ordering
//! discipline it encodes — every validation before every side effect, one
//! `ConfigWriteLock` held across the enable, the parent stamp and the caller's
//! own write — is a responsibility of its own, not another task-service verb.
//! Spec: docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md §2.

use std::path::{Path, PathBuf};

use super::tasks_root_for;
use crate::capture_config::{self, VaultCaptureConfig};
use crate::services::{app_config, ServicePaths};
use crate::tasks;

/// The effective pair written, so the caller can reflect it without a reload.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParentSet {
    pub parent_id: Option<String>,
    pub parent_link: Option<String>,
    /// True only when THIS call turned Task IDs on for the vault. The frontend
    /// cannot infer it — an already-enabled vault with an unstamped parent
    /// returns the identical shape — and without it the user discovers IDs
    /// enabled AND locked with no disclosure (design spec §2).
    pub ids_enabled: bool,
}

/// Set (or clear, with `parent_path: None`) a task's parent.
///
/// THREE STRICTLY SEPARATED PHASES — see the design spec §2. Validation must
/// precede every side effect: a rejected self-parent or cycle leaves IDs
/// disabled and nothing stamped.
pub fn set_task_parent(
    paths: &ServicePaths,
    vault_id: &str,
    child_path: &Path,
    parent_path: Option<&Path>,
) -> Result<ParentSet, String> {
    // ---- Phase 1: validate. No writes, nothing mutated. ----
    let (vault_path, root, cfg) = tasks_root_for(paths, vault_id)?;
    // The vault-level root assert every task WRITE shares: safe_recording_root
    // is only lexical, so a tasks folder resolving outside the vault must fail
    // here rather than be written into.
    super::assert_root_if_exists(&vault_path, &root)?;
    let child = canonical_task_in_root(&root, child_path)?;

    let Some(parent_path) = parent_path else {
        // Clear: no parent to validate, no ids needed. `ensure_id: None` — a
        // clear removes a relationship, it does not edit the task (the same
        // reason a status toggle never stamps).
        tasks::update_task_fields(
            &root,
            &child,
            &[("parent-id", None), ("parent", None)],
            None,
        )?;
        return Ok(ParentSet {
            parent_id: None,
            parent_link: None,
            ids_enabled: false,
        });
    };
    let parent = canonical_task_in_root(&root, parent_path)?;
    // Compared as canonical PATHS, not ids: this rejection must be available
    // before anything is stamped, and at this point neither task need have an
    // id at all (design spec §2).
    if parent == child {
        return Err("A task cannot be its own parent.".to_string());
    }

    let prop = cfg.task_id_property_name().to_string();
    if !tasks::is_valid_id_property(&prop) {
        return Err(format!(
            "The vault's task ID property {prop:?} is not a valid frontmatter key; \
             change it in the vault's Task settings first."
        ));
    }

    // Read ids UNCONDITIONALLY — hand-authored tasks carry ids even while
    // generation is off, and an index built from the gated walk would be empty,
    // passing the cycle check vacuously (design spec §2).
    // STRUCTURAL: includes archived tasks (their files still carry `parent-id`)
    // and FAILS on an unreadable task — validating against a partial graph would
    // let a cycle through (design spec §2).
    let all = tasks::list_tasks_structural(&root, Some(&prop))?;
    reject_ambiguous_parent(&all, &parent)?;
    // The graph is keyed on PATHS with edges resolved through ids, so an id-less
    // task still contributes its outgoing edge and the check is never skipped
    // for want of an id (design spec §3).
    // VALIDATION index, not the display one: `parent_index` drops the edges of a
    // pre-existing on-disk cycle so both rows render parentless, but validating
    // against that filtered graph accepts writes that CLOSE a cycle. With
    // A->B->A and C->A on disk, the dropped edges make ancestors(C) = [A], B is
    // never seen, and assigning B's parent to C writes B->C->A->B (Codex P2,
    // PR #77). `parent_index_for_validation` keeps cyclic edges (still dropping
    // ambiguous ids, which resolve nothing either way); `ancestors` is bounded,
    // so walking a cyclic graph still terminates.
    if tasks::would_create_cycle(&tasks::parent_index_for_validation(&all), &child, &parent) {
        return Err(CYCLE_REFUSED.to_string());
    }

    // ---- Phases 2+3: the SHARED resolve path (Task 8's create path reuses
    // it), with the child's own write passed in as a closure so the lock
    // outlives it. ----
    let ctx = ParentWriteCtx {
        paths,
        vault_id,
        vault_path: &vault_path,
        root: &root,
        prop: &prop,
        phase1_cfg: &cfg,
    };
    let (resolved, ()) = resolve_parent_for_write(
        &ctx,
        &parent,
        &child,
        || {
            // Re-validation under the lock, on the freshly-committed graph.
            let all = tasks::list_tasks_structural(&root, Some(&prop))?;
            Ok(tasks::would_create_cycle(
                &tasks::parent_index_for_validation(&all),
                &child,
                &parent,
            ))
        },
        |resolved| {
            // Phase 3b: the child's pair. `ensure_id` rides along, so a legacy
            // child picks up its own id in the same write.
            tasks::update_task_fields(
                &root,
                &child,
                &[
                    // ensure_id preserves ANY usable existing value, so an
                    // inherited `task-id: "[legacy]"` would otherwise emit a
                    // bare flow sequence the reader rejects. quote_id_if_needed
                    // keeps generated base36 bare — the same helper the create
                    // path writes its pair through, so the two agree by
                    // construction.
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
            )?;
            Ok(())
        },
    )?;
    Ok(ParentSet {
        parent_id: Some(resolved.parent_id),
        parent_link: Some(resolved.link),
        ids_enabled: resolved.ids_enabled,
    })
}

/// The one refusal message both cycle checks (pre-lock and under-lock) use —
/// the user cannot tell the two apart, and they must never drift.
const CYCLE_REFUSED: &str = "That would make a task its own ancestor.";

/// What the shared resolve path needs to know about the vault it writes in.
/// A struct, not eight parameters: every field is read-only context, and the
/// create path (Task 8) fills the identical set.
pub(super) struct ParentWriteCtx<'a> {
    pub paths: &'a ServicePaths,
    pub vault_id: &'a str,
    pub vault_path: &'a Path,
    pub root: &'a Path,
    /// The vault's resolved task-id property, ALREADY validated by phase 1.
    pub prop: &'a str,
    /// The config phase 1 validated against — compared with a fresh read once
    /// the lock is held.
    pub phase1_cfg: &'a VaultCaptureConfig,
}

/// The parent half of the write, resolved: the id the child will point at and
/// the link it will display.
pub(super) struct ResolvedParent {
    pub parent_id: String,
    pub link: String,
    pub ids_enabled: bool,
}

/// Validate-under-lock, enable, stamp the parent, compose the link — then run
/// the caller's own write while the guard is STILL HELD, and only then release
/// it. The guard outliving the caller's write is the point: dropping it at the
/// end of this function would reopen the very race the re-check closes.
///
/// Shared by `set_task_parent` (writes onto an existing child) and, later, the
/// create path (passes the pair into `create_task`). Add-subtask is very often
/// a vault's FIRST hierarchy operation — IDs off, parent unstamped — so the
/// create path must run this WHOLE path, not just the read-only validation
/// (design spec §2).
pub(super) fn resolve_parent_for_write<T>(
    ctx: &ParentWriteCtx<'_>,
    parent: &Path,
    // The file the link is written INTO — the markdown fallback's destination
    // resolves relative to the note containing it (design spec §1).
    child: &Path,
    recheck_cycle: impl FnOnce() -> Result<bool, String>,
    write: impl FnOnce(&ResolvedParent) -> Result<T, String>,
) -> Result<(ResolvedParent, T), String> {
    // ONE lock across the config re-check, the enable, the parent stamp and the
    // caller's write. A concurrent Task-ID settings save holds the same lock
    // across its scan AND write, so the two serialize either way (design spec
    // §2). NOT reentrant — nothing inside this scope may re-acquire it.
    let _guard = capture_config::config_write_lock();

    // Phase 1 read the config BEFORE this lock existed, so a settings save may
    // have committed a different property in between; writing under the stale
    // one would orphan the hierarchy immediately. Re-read and refuse when it
    // actually changed (design spec §2).
    let fresh = capture_config::vault_config(&app_config(ctx.paths), ctx.vault_id);
    if fresh.task_id_enabled != ctx.phase1_cfg.task_id_enabled
        || fresh.task_id_property_name() != ctx.prop
    {
        return Err(
            "The vault's Task ID settings changed while this was in flight. Try again.".to_string(),
        );
    }
    // UNCONDITIONAL — not only when the config changed. Two parent assignments
    // can overlap (one setting A->B while the other sets B->A); both phase-1
    // scans pass before either writes, so only a re-check under this lock sees
    // the other's committed write and refuses (design spec §2).
    if recheck_cycle()? {
        return Err(CYCLE_REFUSED.to_string());
    }

    // ---- Phase 2: enable (idempotent, additive). ----
    let ids_enabled = !fresh.task_id_enabled;
    if ids_enabled {
        // MUST NOT re-acquire the lock — it is not reentrant and a nested
        // acquire self-deadlocks. This is the *_locked variant.
        enable_task_ids_locked(ctx.paths, ctx.vault_id)?;
    }

    // ---- Phase 3a: the parent's own id + the link derived from its path. ----
    // `ensure_id` stamps only when the property has no usable value and returns
    // the effective id, so an already-stamped parent costs no write. `None`
    // means the property holds a block/flow value that is not an id and must
    // not be clobbered — there is no id to point the child at.
    let parent_id = tasks::update_task_fields(ctx.root, parent, &[], Some(ctx.prop))?
        .ok_or_else(|| "Could not assign an ID to the parent task.".to_string())?;
    // The link is vault-relative, and `parent`/`child` are canonical — so the
    // vault root must be canonical too or `strip_prefix` fails on a registry
    // path that differs only by symlink (the same reason `open_task` computes
    // its relative path against the canonical vault path).
    let canon_vault = std::fs::canonicalize(ctx.vault_path)
        .map_err(|e| format!("Cannot resolve vault folder: {e}"))?;
    let link = tasks::compose_parent_link(parent, child, &canon_vault, &read_title(parent))
        .ok_or_else(|| "The parent task is outside the vault.".to_string())?;

    let resolved = ResolvedParent {
        parent_id,
        link,
        ids_enabled,
    };
    // ---- Phase 3b: the caller's write, still under the lock. ----
    let out = write(&resolved)?;
    Ok((resolved, out))
}

/// Canonicalize `path`, require containment inside the (canonicalized) tasks
/// root, and require it to BE a task document. Same gates — and the same
/// messages — `update_task_fields` applies before a write; applied here first
/// so a vanished or escaping path fails in phase 1, before any side effect.
fn canonical_task_in_root(root: &Path, path: &Path) -> Result<PathBuf, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    let content =
        std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
    if !tasks::is_task(&content) {
        return Err("That file is not a task document.".to_string());
    }
    Ok(canon_path)
}

/// Refuse a parent whose own id is shared with another task: it identifies no
/// single task, so the child's `parent-id` would resolve to nothing (the
/// hierarchy index drops ambiguous ids outright).
///
/// The parent's id is taken from the scan rather than re-read from disk on
/// purpose: `ambiguous` is computed from that same scan, so the two sides of
/// this comparison are decoded by one reader and cannot disagree — the Defect A
/// failure mode, one layer up. A parent the walk never yielded (it skips
/// symlinked files) simply has no recorded id and nothing to compare.
fn reject_ambiguous_parent(all: &[tasks::TaskItem], parent: &Path) -> Result<(), String> {
    let ambiguous = tasks::ambiguous_ids(all);
    let existing = all
        .iter()
        .find(|t| t.path == parent)
        .and_then(|t| t.id.as_deref());
    match existing {
        Some(id) if ambiguous.contains(id) => Err("Two tasks share that ID, so it can't \
             identify a parent. Change one of their IDs first."
            .to_string()),
        _ => Ok(()),
    }
}

/// Turn Task IDs on for the vault, WITHOUT taking the config lock — the caller
/// holds it (it is not reentrant). Read-modify-write like every other config
/// setter, so the vault's other fields and the file's other sections survive.
fn enable_task_ids_locked(paths: &ServicePaths, vault_id: &str) -> Result<(), String> {
    let path = paths
        .config_json
        .as_ref()
        .ok_or_else(|| "Cannot resolve the config directory".to_string())?;
    let mut value = capture_config::vault_config(&app_config(paths), vault_id);
    value.task_id_enabled = true;
    capture_config::update_vault_config_at(path, vault_id, value)
        .map_err(|e| format!("Could not enable Task IDs for this vault: {e}"))
}

/// Recompose a just-MOVED child's own `parent` link, best-effort (warn-only,
/// like the id backfill it runs beside — the move already mutated the vault and
/// a link repair must never fail it).
///
/// Only the markdown fallback can go stale: its destination is resolved
/// relative to the note CONTAINING it, so a child that changed depth now points
/// at nothing even though the parent never moved (Codex P2, PR #77). A wikilink
/// is vault-relative and survives any move, which is also why it short-circuits
/// before the scan below — the common case costs one file read.
///
/// Deliberately ONE file: a moved *parent's* children are the unbounded batch
/// write this design declines.
pub(super) fn repair_parent_link(
    vault_path: &Path,
    root: &Path,
    child: &Path,
    cfg: &VaultCaptureConfig,
) {
    if let Err(e) = recompose_parent_link(vault_path, root, child, cfg) {
        log::warn!("parent link repair on {child:?} failed: {e}");
    }
}

fn recompose_parent_link(
    vault_path: &Path,
    root: &Path,
    child: &Path,
    cfg: &VaultCaptureConfig,
) -> Result<(), String> {
    let child = std::fs::canonicalize(child)
        .map_err(|e| format!("Cannot resolve the moved task file: {e}"))?;
    let content =
        std::fs::read_to_string(&child).map_err(|e| format!("Cannot read the moved task: {e}"))?;
    let (Some(current), Some(parent_id)) = (
        tasks::parent_link_field(&content),
        tasks::parent_id_field(&content),
    ) else {
        // No link, or a link with no authoritative `parent-id` behind it (a
        // hand-authored one): nothing this app composed, nothing it rewrites.
        return Ok(());
    };
    if current.starts_with("[[") {
        return Ok(());
    }
    let prop = cfg.task_id_property_name();
    if !tasks::is_valid_id_property(prop) {
        return Ok(());
    }
    // Read ids UNCONDITIONALLY (not through the generation gate): a vault can
    // hold ids and parent links while generation is off, and a gated read would
    // resolve nothing — the same reason `set_task_parent`'s validation reads the
    // property directly.
    let all = tasks::list_tasks_structural(root, Some(prop))?;
    if tasks::ambiguous_ids(&all).contains(parent_id.as_str()) {
        return Ok(()); // identifies no single task — a rewrite would guess
    }
    let Some(parent) = all
        .iter()
        .find(|t| t.id.as_deref() == Some(parent_id.as_str()))
    else {
        return Ok(()); // dangling reference: leave the user's link alone
    };
    // The VAULT root, threaded in — NOT `root.parent()`, which for a nested
    // tasks folder (`Notes/Tasks`) would drop a whole path segment from every
    // link it composes (Codex P2, PR #77). Canonical, since the scan's paths are.
    let canon_vault = std::fs::canonicalize(vault_path)
        .map_err(|e| format!("Cannot resolve vault folder: {e}"))?;
    let Some(fresh) = tasks::compose_parent_link(&parent.path, &child, &canon_vault, &parent.title)
    else {
        return Ok(());
    };
    if fresh == current {
        return Ok(()); // unchanged — never spend a write to rewrite a file identically
    }
    tasks::update_task_fields(
        root,
        &child,
        &[("parent", Some(&crate::yaml_scalar::yaml_quote(&fresh)))],
        None,
    )?;
    Ok(())
}

/// The parent's display title for the link label — the frontmatter `title:`,
/// falling back to the file stem, exactly as the task list derives it (a
/// hand-authored task may carry no title, and the label must never be empty).
fn read_title(path: &Path) -> String {
    let stem = || {
        path.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    match std::fs::read_to_string(path) {
        Ok(content) => crate::capture_note::note_field(&content, "title").unwrap_or_else(stem),
        Err(e) => {
            log::warn!("set_task_parent: could not re-read {path:?} for the parent title: {e}");
            stem()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::move_task_to_list;
    use super::*;
    use crate::services::test_support::fixture;

    const VAULT: &str = "deadbeef01234567";

    /// The existing services harness (`test_support::fixture`: registry +
    /// config.json in a tempdir) plus the vault's Task ID setting and any task
    /// files the case needs. The caller owns the tempdir, exactly like every
    /// other test in this crate's services suite.
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
            write(
                &root,
                f,
                &format!("---\ntype: Task\nstatus: new\ntitle: \"{title}\"\n---\n"),
            );
        }
        (paths, VAULT.to_string())
    }

    fn fixture_with_ids_disabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
        fixture_with_ids(dir, false, files)
    }

    fn fixture_with_ids_enabled(dir: &Path, files: &[&str]) -> (ServicePaths, String) {
        fixture_with_ids(dir, true, files)
    }

    /// The vault's tasks root, resolved the way production does (so a vault
    /// with a nested `tasksFolder` needs no second derivation here).
    fn tasks_root(paths: &ServicePaths, vault: &str) -> PathBuf {
        tasks_root_for(paths, vault).unwrap().1
    }

    fn config_for(paths: &ServicePaths, vault: &str) -> VaultCaptureConfig {
        capture_config::vault_config(&app_config(paths), vault)
    }

    fn write(root: &Path, rel: &str, content: &str) -> PathBuf {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn rejects_a_self_parent_without_enabling_ids_or_stamping() {
        // Phase separation: validation precedes EVERY side effect.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["a.md"]);
        let p = tasks_root(&paths, &vault).join("a.md");
        let before = std::fs::read_to_string(&p).unwrap();
        assert!(set_task_parent(&paths, &vault, &p, Some(&p)).is_err());
        assert_eq!(std::fs::read_to_string(&p).unwrap(), before); // no stamp
        assert!(!config_for(&paths, &vault).task_id_enabled); // still disabled
    }

    #[test]
    fn refuses_a_cycle_through_an_id_less_prospective_parent() {
        // REGRESSION (design spec §3): P has no id but already names C as its
        // parent. The path-keyed graph must see P->C and refuse; an id-keyed one
        // would skip the check and create a P<->C cycle on the next write.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        write(
            &root,
            "p.md",
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\nparent-id: c\n---\n",
        );
        write(
            &root,
            "c.md",
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\ntask-id: c\n---\n",
        );
        assert!(
            set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).is_err()
        );
        // And nothing was stamped onto the id-less parent by the failed attempt.
        assert!(!std::fs::read_to_string(root.join("p.md"))
            .unwrap()
            .contains("task-id:"));
    }

    #[test]
    fn refuses_a_cycle_using_dormant_ids_while_generation_is_disabled() {
        // Hand-authored ids exist even with the feature off; the ordinary
        // list_tasks walk suppresses them, so validation must read the property
        // unconditionally or this passes vacuously and creates a real cycle.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        write(
            &root,
            "a.md",
            "---\ntype: Task\nstatus: new\ntitle: \"A\"\ntask-id: a\nparent-id: b\n---\n",
        );
        write(
            &root,
            "b.md",
            "---\ntype: Task\nstatus: new\ntitle: \"B\"\ntask-id: b\n---\n",
        );
        // A already points at B; making A the parent of B closes the loop.
        let err = set_task_parent(&paths, &vault, &root.join("b.md"), Some(&root.join("a.md")));
        assert!(err.is_err(), "a cycle through dormant ids must be refused");
    }

    #[test]
    fn refuses_an_ambiguous_parent_id() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        write(
            &root,
            "p1.md",
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\ntask-id: dup\n---\n",
        );
        write(
            &root,
            "p2.md",
            "---\ntype: Task\nstatus: new\ntitle: \"P2\"\ntask-id: dup\n---\n",
        );
        write(
            &root,
            "c.md",
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
        );
        assert!(set_task_parent(
            &paths,
            &vault,
            &root.join("c.md"),
            Some(&root.join("p1.md"))
        )
        .is_err());
    }

    #[test]
    fn enables_ids_stamps_both_and_writes_a_resolvable_pair() {
        // The bootstrap: IDs off (the default), so no id is surfaced anywhere —
        // the parent is named by PATH and the service does the rest.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        let out =
            set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).unwrap();
        let pid = out.parent_id.unwrap();
        assert!(!pid.is_empty());
        assert!(out.ids_enabled, "this call turned Task IDs on");
        assert!(config_for(&paths, &vault).task_id_enabled); // auto-enabled
        assert!(std::fs::read_to_string(root.join("p.md"))
            .unwrap()
            .contains(&format!("task-id: {pid}")));
        let child = std::fs::read_to_string(root.join("c.md")).unwrap();
        assert!(child.contains(&format!("parent-id: {pid}")));
        assert!(child.contains("parent: \"[[")); // a link was written
    }

    /// A vault whose parent link is a MARKDOWN FALLBACK (the parent sits in a
    /// List folder carrying a wikilink metacharacter), with the child nested
    /// under `child_list`. Returns (paths, vault id, tasks root, child path).
    fn fixture_with_a_fallback_link(
        dir: &Path,
        tasks_folder: &str,
        child_list: &str,
    ) -> (ServicePaths, String, PathBuf, PathBuf) {
        let (paths, _vault) = fixture(dir, "MyVault");
        std::fs::write(
            paths.config_json.as_ref().unwrap(),
            format!(
                r#"{{ "vaults": {{ "{VAULT}": {{ "taskIdEnabled": true, "tasksFolder": "{tasks_folder}" }} }} }}"#
            ),
        )
        .unwrap();
        let vault_id = VAULT.to_string();
        let root = tasks_root(&paths, &vault_id);
        std::fs::create_dir_all(&root).unwrap();
        // `Project#1` is a legal List folder; `#` has no wikilink escape, so
        // the composer must fall back to a percent-encoded markdown link —
        // the only form a move can stale.
        let parent = write(
            &root,
            "Project#1/p.md",
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n",
        );
        let child = write(
            &root,
            &format!("{child_list}/c.md"),
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
        );
        set_task_parent(&paths, &vault_id, &child, Some(&parent)).unwrap();
        (paths, vault_id, root, child)
    }

    #[test]
    fn moving_a_child_recomposes_its_link_under_a_nested_tasks_folder() {
        // tasks root = <vault>/Notes/Tasks, so vault_root != tasks_root.parent().
        // Getting this wrong silently drops a path segment from every link.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault, _root, child) =
            fixture_with_a_fallback_link(dir.path(), "Notes/Tasks", "Deep/Sub");
        let landed = move_task_to_list(&paths, &vault, &child.to_string_lossy(), "").unwrap();
        let out = std::fs::read_to_string(&landed.path).unwrap();
        // Vault-relative, `Notes/` included: deriving the vault root as
        // tasks_root.parent() would emit `../../Tasks/Project%231/p.md`.
        assert!(
            out.contains("](../../Notes/Tasks/Project%231/p.md)"),
            "link must be vault-relative, got {out}"
        );
    }

    #[test]
    fn moving_a_child_recomposes_its_own_fallback_link() {
        // Child moves Tasks/Deep/Sub -> Tasks, so the ../ depth changes.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault, _root, child) =
            fixture_with_a_fallback_link(dir.path(), "Tasks", "Deep/Sub");
        let before = std::fs::read_to_string(&child).unwrap();
        assert!(
            before.contains("](../../../Tasks/Project%231/p.md)"),
            "the pre-move link is three levels deep, got {before}"
        );
        let landed = move_task_to_list(&paths, &vault, &child.to_string_lossy(), "").unwrap();
        let out = std::fs::read_to_string(&landed.path).unwrap();
        // One `../` now: the child sits at <vault>/Tasks/c.md, and a markdown
        // destination resolves from the note's OWN directory.
        assert!(out.contains("](../Tasks/Project%231/p.md)"), "got {out}");
    }

    #[test]
    fn moving_a_child_with_an_unchanged_link_writes_nothing_extra() {
        // A wikilink is vault-relative, so a move cannot stale it — no rewrite.
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &[]);
        let root = tasks_root(&paths, &vault);
        let parent = write(
            &root,
            "Plain/p.md",
            "---\ntype: Task\nstatus: new\ntitle: \"P\"\n---\n",
        );
        let child = write(
            &root,
            "Work/c.md",
            "---\ntype: Task\nstatus: new\ntitle: \"C\"\n---\n",
        );
        set_task_parent(&paths, &vault, &child, Some(&parent)).unwrap();
        let before = std::fs::read_to_string(&child).unwrap();
        assert!(before.contains("parent: \"[[Tasks/Plain/p]]\""), "{before}");
        let landed = move_task_to_list(&paths, &vault, &child.to_string_lossy(), "Home").unwrap();
        assert_eq!(std::fs::read_to_string(&landed.path).unwrap(), before);
    }

    #[test]
    fn clearing_removes_both_keys() {
        let dir = tempfile::tempdir().unwrap();
        let (paths, vault) = fixture_with_ids_enabled(dir.path(), &["p.md", "c.md"]);
        let root = tasks_root(&paths, &vault);
        set_task_parent(&paths, &vault, &root.join("c.md"), Some(&root.join("p.md"))).unwrap();
        set_task_parent(&paths, &vault, &root.join("c.md"), None).unwrap();
        let child = std::fs::read_to_string(root.join("c.md")).unwrap();
        assert!(!child.contains("parent-id"));
        assert!(!child.contains("parent:"));
    }

    /// TASK 6b regression pin. The defect: config.json read-modify-writes were
    /// serialized by TWO mutexes that did not exclude each other — this core
    /// `config_write_lock()`, taken here, and a separate shell-only
    /// `ConfigWriteLock` the IPC settings commands took instead. A capture
    /// settings save could read `task_id_enabled: false`, race this function's
    /// enable, and write `false` back over it via `config_merge::
    /// merge_capture_owned`'s `task_id_enabled: existing.task_id_enabled` —
    /// while the child it raced already carried a stamped `parent-id`,
    /// orphaning the reference the instant it was created.
    ///
    /// HONESTY NOTE (can't-go-red, by design): both threads below take
    /// `config_write_lock()` — the ONE lock the shell now takes at every
    /// config-write site after the fix. That is deliberate, not an oversight:
    /// core has only ever had this one lock; the second mutex was a shell
    /// (`src-tauri/src`) type built on `tauri::State`/`AppHandle`, which
    /// cannot be constructed or invoked from a `core`-crate unit test — there
    /// is no way to reach the actual pre-fix code path from here. So this
    /// test passes identically before and after the fix; it does not catch
    /// today's bug, it PINS the invariant so a future core write path that
    /// forgets to take this lock reopens the same race. The fix itself is
    /// structural, not something a core test can observe: `capture_commands::
    /// ConfigWriteLock` no longer exists anywhere in the compiled shell crate
    /// (see the task report), so there is no second lock left to pick by
    /// mistake. The task report also documents a manual, uncommitted
    /// experiment confirming this harness DOES fail reliably when thread B is
    /// changed to skip the lock — proof the apparatus below is sensitive to
    /// the class of bug being fixed, even though it cannot reach the specific
    /// pre-fix shell code.
    #[test]
    fn concurrent_capture_save_and_parent_assignment_never_desync_task_id_enabled() {
        // Iterated (not asserted once): both threads share one lock, so every
        // ordering the OS scheduler produces must converge on a consistent
        // state — this stress-tests that claim across many orderings rather
        // than trusting a single lucky interleaving, and would also surface a
        // hang/deadlock if the lock were ever made reentrant-unsafe.
        for _ in 0..50 {
            let dir = tempfile::tempdir().unwrap();
            let (paths, vault) = fixture_with_ids_disabled(dir.path(), &["p.md", "c.md"]);
            let root = tasks_root(&paths, &vault);
            let parent = root.join("p.md");
            let child = root.join("c.md");
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));

            let thread_a = {
                let paths = paths.clone();
                let vault = vault.clone();
                let (parent, child) = (parent.clone(), child.clone());
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    set_task_parent(&paths, &vault, &child, Some(&parent))
                })
            };

            let thread_b = {
                let paths = paths.clone();
                let vault = vault.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || -> Result<(), String> {
                    barrier.wait();
                    // Mirrors set_capture_config exactly: read, change a
                    // capture-owned field, merge (which preserves
                    // task_id_enabled — config_merge.rs's clobbering line),
                    // write — all under the ONE process-wide lock, with the
                    // read INSIDE it so a concurrent writer's commit is never
                    // read as stale (the same rule set_capture_config's own
                    // doc comment states).
                    let _guard = capture_config::config_write_lock();
                    let existing = capture_config::vault_config(&app_config(&paths), &vault);
                    let incoming = VaultCaptureConfig {
                        bitrate_kbps: 192,
                        ..VaultCaptureConfig::default()
                    };
                    let merged = capture_config::merge_capture_owned(&existing, incoming);
                    capture_config::update_vault_config_at(
                        paths.config_json.as_ref().unwrap(),
                        &vault,
                        merged,
                    )
                    .map_err(|e| e.to_string())
                })
            };

            thread_a.join().unwrap().unwrap();
            thread_b.join().unwrap().unwrap();

            let enabled = config_for(&paths, &vault).task_id_enabled;
            let child_has_parent_id = std::fs::read_to_string(&child)
                .unwrap()
                .contains("parent-id:");
            assert_eq!(
                child_has_parent_id, enabled,
                "interleaving a capture save with a parent assignment left the \
                 vault inconsistent: child parent-id present={child_has_parent_id} \
                 but vault task_id_enabled={enabled} — a child must never carry a \
                 parent-id in a vault whose Task IDs are off"
            );
        }
    }
}
