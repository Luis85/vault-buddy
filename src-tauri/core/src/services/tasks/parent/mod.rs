//! The parent-assignment write path: `set_task_parent` (set or clear an
//! EXISTING Task's parent), `add_subtask` (create a brand-new child under a
//! parent, reusing the same shared resolve path — `add_task` in the parent
//! module calls this), and the post-move link repair. Its own module because
//! the ordering discipline it encodes — every validation before every side
//! effect, one `capture_config::config_write_lock()` held across the enable,
//! the parent stamp and the caller's own write — is a responsibility of its
//! own, not another task-service verb.
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

    let Some(parent_path) = parent_path else {
        // Clear: no parent to validate, no ids needed. `ensure_id: None` — a
        // clear removes a relationship, it does not edit the task (the same
        // reason a status toggle never stamps).
        let child = canonical_task_in_root(&root, child_path)?;
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
    let (parent, child, prop) = validate_parent_assignment(&root, &cfg, child_path, parent_path)?;

    // ---- Phases 2+3: the SHARED resolve path (`add_subtask` below and
    // `update_task`'s combined-patch path, core/src/services/tasks/update.rs,
    // both reuse it), with the child's own write passed in as a closure so the
    // lock outlives it. ----
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

/// Phase 1 (read-only) validation of a prospective parent assignment against
/// an EXISTING child: containment + `is_task` for both paths (via
/// `canonical_task_in_root`), self-parent by path, id-property validity,
/// ambiguity, cycle, and parent-id assignability — every check
/// `set_task_parent` ran inline before Task 7, now ALSO shared with
/// `update_task`'s combined-patch path (`core/src/services/tasks/update.rs`),
/// which must run this validation BETWEEN validating and an ordinary field
/// write that has to land first (see that module's doc comment for the
/// ordering rationale — a rejected parent must not leave a committed title).
/// Returns the canonicalized `(parent, child, prop)` ready for
/// `resolve_parent_for_write`. `pub(super)` (not private): `update.rs` is a
/// SIBLING of this module under `services::tasks`, and `pub(super)` here means
/// `pub(in services::tasks)` — visible throughout that subtree, not just to
/// `parent`'s own descendants.
pub(super) fn validate_parent_assignment(
    root: &Path,
    cfg: &VaultCaptureConfig,
    child_path: &Path,
    parent_path: &Path,
) -> Result<(PathBuf, PathBuf, String), String> {
    let child = canonical_task_in_root(root, child_path)?;
    let parent = canonical_task_in_root(root, parent_path)?;
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
    let all = tasks::list_tasks_structural(root, Some(&prop))?;
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
    // Forecast phase 3a's ensure_id BEFORE phase 2 enables Task IDs: without
    // this, a parent whose id property holds a value ensure_id must never
    // clobber (e.g. a synced external id like `task-id: {source: jira}`) let
    // phase 2 run first and phase 3a fail second — a refused assignment that
    // still silently switched the vault's Task IDs on with no stamp and no
    // disclosure (this sub-case wasn't visible to phase 1 above, which
    // validates the CHILD/graph, never the parent's own frontmatter).
    let parent_content =
        std::fs::read_to_string(&parent).map_err(|e| format!("Cannot read task: {e}"))?;
    if parent_id_unassignable(&parent_content, &prop) {
        return Err("Could not assign an ID to the parent task.".to_string());
    }
    Ok((parent, child, prop))
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
/// Shared by `set_task_parent` (writes onto an existing child) and
/// `add_subtask` below (passes the pair into `create_task`). Add-subtask is
/// very often a vault's FIRST hierarchy operation — IDs off, parent unstamped
/// — so the create path must run this WHOLE path, not just the read-only
/// validation (design spec §2).
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
    // have committed a different property — OR a different tasks folder — in
    // between; writing under either stale value would orphan the hierarchy
    // immediately (an id under a property nothing reads, or the whole pair
    // under a root list_tasks/open_task no longer walk — set_tasks_config
    // takes this same config_write_lock() around its own read-modify-write,
    // see task_commands.rs, so it is exactly as reachable in this window as
    // a Task-ID settings save). Re-read and refuse when EITHER actually
    // changed (design spec §2). `tasks_root()` is compared as the raw
    // configured string (already defaulted to "Tasks"), not a re-resolved
    // path — the point is to detect the CONFIG VALUE moving, not to
    // re-derive `ctx.root` a second way that could itself drift from how
    // phase 1 derived it.
    let fresh = capture_config::vault_config(&app_config(ctx.paths), ctx.vault_id);
    if fresh.task_id_enabled != ctx.phase1_cfg.task_id_enabled
        || fresh.task_id_property_name() != ctx.prop
        || fresh.tasks_root() != ctx.phase1_cfg.tasks_root()
    {
        return Err(
            "The vault's Task settings changed while this was in flight. Try again.".to_string(),
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

/// Create a brand-new CHILD task under `parent_path`, running the FULL shared
/// resolve-the-parent path (validate, lock, re-check, enable, stamp the
/// parent, compose the link) rather than validation alone — Add subtask is
/// very often a vault's FIRST hierarchy operation, so validation alone would
/// leave no authoritative parent-id to write (design spec §2, Codex P1, PR
/// #77). The child's own file write happens INSIDE `resolve_parent_for_write`'s
/// held guard, exactly like `set_task_parent`'s write closure — dropping the
/// lock before creating the file would reopen the very race phases 2-3 close.
///
/// Unlike `set_task_parent` there is no self-parent or cycle check here: the
/// child does not exist on disk yet, so it cannot already equal the chosen
/// parent (`create_task` never clobbers an existing file, so the eventual
/// child path can never coincide with a pre-existing one) and nothing can
/// already reference a file that doesn't exist — the under-lock recheck is
/// therefore unconditionally `Ok(false)`.
///
/// The child's OWN id is drawn here, under the validated `prop` — which stays
/// correct across phase 2's enable, since the property NAME never changes,
/// only the enabled flag does — rather than from the caller's pre-call config
/// snapshot. Using that stale snapshot would pass `None` to `create_task`
/// whenever THIS call is what turns Task IDs on, leaving a child with a
/// `parent-id` but no `task-id` of its own (Codex P2, PR #77).
#[allow(clippy::too_many_arguments)]
pub(super) fn add_subtask(
    paths: &ServicePaths,
    vault_id: &str,
    vault_path: &Path,
    root: &Path,
    cfg: &VaultCaptureConfig,
    parent_path: &Path,
    target_root: &Path,
    title: &str,
    today: &str,
    due: Option<&str>,
    priority: Option<&str>,
    tags: &[String],
    scheduled: Option<&str>,
) -> Result<(ResolvedParent, PathBuf, String), String> {
    // ---- Phase 1 (read-only): the parent half only — there is no child path
    // to validate yet, so this is NOT `validate_parent_assignment` (which
    // requires an existing child). ----
    let parent = canonical_task_in_root(root, parent_path)?;
    let prop = cfg.task_id_property_name().to_string();
    if !tasks::is_valid_id_property(&prop) {
        return Err(format!(
            "The vault's task ID property {prop:?} is not a valid frontmatter key; \
             change it in the vault's Task settings first."
        ));
    }
    let all = tasks::list_tasks_structural(root, Some(&prop))?;
    reject_ambiguous_parent(&all, &parent)?;
    let parent_content =
        std::fs::read_to_string(&parent).map_err(|e| format!("Cannot read task: {e}"))?;
    if parent_id_unassignable(&parent_content, &prop) {
        return Err("Could not assign an ID to the parent task.".to_string());
    }

    // The path the child WILL land at. The collision-safe writer inside the
    // `write` closure below may still append a ` (N)` suffix on conflict, but
    // that only changes the FILENAME, never the directory — and
    // `compose_parent_link`'s fallback form only depends on the child's
    // DIRECTORY (see its own doc comment), so resolving the link against this
    // prospective path is safe even though it is not yet the real file.
    //
    // CANONICALIZED — not the caller's `target_root` verbatim. Below,
    // `resolve_parent_for_write` strips a CANONICAL vault path off this child
    // (`uri::vault_relative`'s `strip_prefix` does no canonicalization of its
    // own), so a `target_root` built from the vault's REGISTRY path — a
    // symlink, or, unconditionally on Windows, obsidian.json's `C:\...`
    // against canonicalize's `\\?\C:\...` (the same divergence `open_task`
    // guards against) — would never match, and `compose_parent_link` would
    // wrongly report the child "outside the vault" for any parent whose List
    // folder carries a wikilink metacharacter (`parent_link.rs`'s markdown
    // fallback is the only branch that resolves the child at all). By the
    // time that failure surfaced, phase 2/3a above had already enabled Task
    // IDs and stamped the parent — a rejected assignment with a silent side
    // effect and no child created. `set_task_parent` never hit this: its
    // child already exists on disk and is canonicalized up front via
    // `canonical_task_in_root`. `target_root` already exists here (`add_task`
    // always creates it before calling this), so this only fails on a
    // genuine race/removal.
    let canon_target_root = std::fs::canonicalize(target_root)
        .map_err(|e| format!("Cannot resolve the list folder: {e}"))?;
    let prospective_child =
        canon_target_root.join(format!("{}.md", tasks::task_basename(title, today)));

    let ctx = ParentWriteCtx {
        paths,
        vault_id,
        vault_path,
        root,
        prop: &prop,
        phase1_cfg: cfg,
    };
    let (resolved, (path, child_id)) = resolve_parent_for_write(
        &ctx,
        &parent,
        &prospective_child,
        || Ok(false), // a brand-new leaf can never already be on a cycle
        |resolved| {
            // Reached only once Task IDs are enabled (already, or by phase 2
            // just above) under `prop` — generate the child's own id here,
            // never from a pre-call snapshot (see this fn's doc comment).
            let child_id = tasks::new_task_id();
            let written = tasks::create_task(
                target_root,
                title,
                today,
                due,
                priority,
                tags,
                Some((prop.as_str(), child_id.as_str())),
                cfg.task_extra_frontmatter.as_deref(),
                cfg.task_body_template.as_deref(),
                scheduled,
                Some((&resolved.parent_id, &resolved.link)),
            )
            .map_err(|e| format!("Could not create task: {e}"))?;
            Ok((written, child_id))
        },
    )?;
    Ok((resolved, path, child_id))
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

/// True when phase 3a's `ensure_id` is FORECAST to fail on `parent_content` —
/// delegates entirely to `tasks::parse::id_property_unassignable`, the SAME
/// block/flow/strict-decode predicate `update_task_fields`'s writer (phase
/// 3a) itself consults, so the two can never define "assignable" two
/// different ways (design spec §2).
///
/// An earlier version of this forecast re-implemented only a single-line raw
/// scan (`raw_scalar_field`), which cannot see an IMPLICIT block/list value
/// with no `|`/`>`/`{`/`[` marker on the key's own line (e.g. `task-id:`
/// followed by unmarked indented children): it read that blank first line as
/// assignable, let phase 2 switch Task IDs on for the vault, and only then
/// hit the writer's OWN (correct) block detection — after the setting was
/// already flipped. Reusing the writer's predicate instead of a second copy
/// closes that gap for every present and future non-scalar shape, not just
/// the one that was found.
fn parent_id_unassignable(parent_content: &str, prop: &str) -> bool {
    tasks::parse::id_property_unassignable(parent_content, prop)
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
mod tests;
