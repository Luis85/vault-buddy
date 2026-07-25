//! Disk operations: the sanctioned vault writes on an EXISTING task file —
//! the surgical field/status update via `update_task_fields` (and the
//! `backfill_task_id`/`set_task_status` wrappers over it). Task-document
//! CREATION (filename derivation + frontmatter rendering) lives in
//! `tasks::create`.
//!
//! **The per-path write lock (Task 6f).** `update_task_fields` is a plain
//! read -> surgical-edit -> atomic-replacing-write with nothing serializing
//! two IN-PROCESS callers on the SAME file: the panel and the embedded MCP
//! server (its own blocking pool, genuinely concurrent OS threads — see
//! AGENTS.md's MCP section) both reach it through `core::services`, and
//! nothing before this fix stopped two such calls from interleaving into a
//! classic lost update — both read v1, both edit their own key against that
//! v1, whichever writes second silently discards the first writer's key.
//! `with_task_file_lock` (below) closes that window for every writer that
//! routes through it; `structural::delete_task` and
//! `lists::relocate::move_task_to_list` also take it, for the same reason,
//! at their own call sites (see their doc comments for why those two, and
//! not `structural::duplicate_task`, needed it — task report has the full
//! per-call-site audit).

use super::writer::set_fields;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

/// Process-wide map from a task file's CANONICAL path to a per-file mutex.
/// Keyed on the canonical path — not the caller's, possibly symlinked or
/// `..`-relative one — so two spellings of the same file share one lock;
/// every caller of `with_task_file_lock` already canonicalizes for its own
/// containment gate and passes that result straight through, so this never
/// re-resolves the path a second, potentially-divergent way.
///
/// SCOPE (be honest about this, in code as much as in the task report): this
/// is an IN-PROCESS lock. It serializes the panel, the embedded MCP server,
/// and any other in-process writer against EACH OTHER. It does NOT and
/// cannot serialize against Obsidian itself, a sync client, or any other
/// process editing the same file — cross-process contention on a vault file
/// is a pre-existing, accepted reality of this app (the vault domain never
/// claims exclusive ownership of a file on disk), and nothing here changes
/// that.
///
/// LOCK ORDERING — an invariant, not a suggestion: `capture_config::
/// config_write_lock()` may be acquired BEFORE this lock, never after.
/// `services::tasks::parent::resolve_parent_for_write` already holds
/// `config_write_lock()` across two calls into this lock (the parent's own
/// stamp, then the caller's write), so that order is load-bearing today. If
/// it were ever reversed anywhere — this lock taken first, then
/// `config_write_lock()` requested while still holding it — one call path
/// taking config-then-file while another takes file-then-config is the
/// textbook two-lock deadlock. Nothing that takes THIS lock (this module,
/// `structural::delete_task`, `lists::relocate::move_task_to_list`) ever
/// acquires `config_write_lock()` while holding it; a future caller that
/// needs to must take config's lock first, exactly like
/// `resolve_parent_for_write` does, never nest it inside this one.
///
/// NOT REENTRANT: `std::sync::Mutex` is not recursive, so acquiring this
/// lock for a path on a thread that already holds it for that SAME path
/// self-deadlocks (it hangs; it does not error). Every existing multi-call
/// site either targets two DIFFERENT paths that can never coincide
/// (`resolve_parent_for_write` stamps the parent, then — after that call has
/// already returned and released its lock — writes the child; a parent
/// can never equal its own child, asserted before either write) or calls
/// this lock's holder sequentially, never nested inside another call's own
/// critical section (see the task report's full per-call-site audit).
///
/// BOUNDED, not a plain forever-growing map: `with_task_file_lock` prunes
/// every dead (zero-strong-count) entry on every acquire, so the map's
/// steady-state size is the number of task files with a write CURRENTLY in
/// flight, not the number this process has ever touched. That count is
/// bounded by the process's own writer threads — the Tauri/Tokio blocking
/// pool plus the embedded MCP server's single "mcp-blocking" thread — so in
/// practice it is low single digits, never proportional to vault size (see
/// the task report for the reasoning behind calling this "bounded" rather
/// than measuring a hard ceiling).
///
/// `OnceLock`, not a `const`-initialized `static`, because `HashMap::new` is
/// not `const fn` (it seeds a randomized hasher) — the same reason
/// `search_commands.rs`'s process-lifetime cache and `diagnostics.rs`'s
/// lazily-resolved paths use this exact pattern instead of the bare
/// `Mutex::new(())` `config_write_lock()` gets away with for a unit payload.
static FILE_LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

fn file_locks() -> &'static Mutex<HashMap<PathBuf, Weak<Mutex<()>>>> {
    FILE_LOCKS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Run `f` while holding the per-canonical-path lock for `canon_path` — see
/// `FILE_LOCKS`'s doc comment for the scope boundary, the lock-ordering
/// invariant, and the reentrancy hazard. The caller MUST pass an
/// already-canonicalized path (every caller here does, as a side effect of
/// its own containment gate) or two spellings of the same file will take
/// two different locks and the whole thing is decorative.
///
/// Poison-tolerant like `capture_config::config_write_lock`: a panic while
/// either mutex is held (the map lock, or a specific file's lock) leaves no
/// torn in-memory state — every writer re-reads the file from disk before
/// editing it — so recovering via `into_inner()` is correct and wedging
/// every future write behind one unrelated panic is not.
pub(super) fn with_task_file_lock<T>(canon_path: &Path, f: impl FnOnce() -> T) -> T {
    let entry = {
        let mut map = file_locks().lock().unwrap_or_else(|e| e.into_inner());
        // Prune every dead entry on every acquire — not just this path's —
        // so a burst of writes across many distinct files can never leave
        // the map growing for the life of the process; see FILE_LOCKS's doc
        // comment for the resulting bound. Safe to do while holding `map`:
        // a Weak's strong count can only reach 0 once the LAST Arc clone
        // (held by whichever call is mid-`f()` for that path) is dropped,
        // and that drop happens after this function returns its `entry`,
        // never while `map` itself is locked.
        map.retain(|_, weak| weak.strong_count() > 0);
        match map.get(canon_path).and_then(Weak::upgrade) {
            Some(arc) => arc,
            None => {
                let arc = Arc::new(Mutex::new(()));
                map.insert(canon_path.to_path_buf(), Arc::downgrade(&arc));
                arc
            }
        }
    };
    let _guard = entry.lock().unwrap_or_else(|e| e.into_inner());
    f()
}

/// Apply a surgical frontmatter patch to a task file on disk. Canonicalizes
/// `root` and `path` and requires containment — a lexical check can't see
/// through a symlink at the file or folder — then, under the per-path lock
/// (see `with_task_file_lock`'s doc comment — this is the read-modify-write
/// it exists to serialize), reads, applies `set_fields`, and writes
/// atomically (hidden `create_new` temp + fsync + REPLACING rename).
/// Replacing is correct here: the target is the `type: Task` file we just
/// read and are editing in place, touching only the named lines.
/// `ensure_id` names the vault's task-id property (`None` = ids off): when the
/// property has no USABLE value — absent, or present with a blank scalar (a
/// bare `task-id:` from an Obsidian property panel / template; Codex, PR #59)
/// — a fresh id is GENERATED HERE and stamped alongside the patch. Generating
/// inside this branch, rather than callers pre-drawing a candidate, means no
/// discarded CSPRNG draws on already-stamped tasks and no caller can get the
/// blank/casing rules wrong. An existing non-empty value (top-level, any
/// casing — `frontmatter_scalar_ci`; a nested `metadata.task-id` never
/// counts) is never overwritten, so IDs stay stable. Returns the property's
/// effective value after the write — freshly stamped or pre-existing — or
/// `None` when `ensure_id` is `None`; callers reflect a just-stamped ID
/// without a second read (Codex, PR #59).
pub fn update_task_fields(
    root: &Path,
    path: &Path,
    updates: &[(&str, Option<&str>)],
    ensure_id: Option<&str>,
) -> Result<Option<String>, String> {
    let canon_root =
        std::fs::canonicalize(root).map_err(|e| format!("Cannot resolve tasks folder: {e}"))?;
    let canon_path =
        std::fs::canonicalize(path).map_err(|e| format!("Cannot resolve task file: {e}"))?;
    if !canon_path.starts_with(&canon_root) {
        return Err("Task file is outside the vault's tasks folder".to_string());
    }
    with_task_file_lock(&canon_path, || {
        let content =
            std::fs::read_to_string(&canon_path).map_err(|e| format!("Cannot read task: {e}"))?;
        let mut effective: Vec<(&str, Option<&str>)> = updates.to_vec();
        // Owned storage for a freshly-generated id and the on-disk casing a blank
        // line is rewritten under — both must outlive `effective`'s borrows.
        let mut generated: Option<String> = None;
        let mut blank_casing: Option<String> = None;
        let ensured = ensure_id.and_then(|key| {
            // `id_property_unassignable` is the SINGLE-SOURCED gate
            // `services::tasks::parent`'s phase-1 pre-lock forecast also
            // consults (design spec §2) — see its own doc comment for why the
            // two must never be two independently-maintained notions of
            // "assignable". A BLOCK (a nested map/list under the key) or FLOW
            // (`key: {..}` / `[..]`) value is the USER'S frontmatter, not a
            // stamp target and not an id: set_fields' rewrite would
            // consume/rewrite it and delete their data (review, PR #59), and
            // reporting it as the effective id would let a duplicate that
            // preserved a flow value read as sharing the source's stable id
            // (Codex P2, PR #76). A present, non-blank scalar the strict reader
            // can't decode is likewise not a usable id. Leave either untouched
            // and report no id — the read (`scalar_id_ci`) agrees: non-scalar =
            // non-id.
            if super::parse::id_property_unassignable(&content, key) {
                return None;
            }
            match super::parse::frontmatter_scalar_ci(&content, key) {
                // Already has a usable PLAIN-SCALAR id (any casing) → never overwritten.
                // Decode it with the SAME strict reader `scalar_id_ci`/`list_tasks`
                // use, not the shallow `frontmatter_scalar_ci` value: for a quoted
                // hand-authored id like `task-id: 'a''b'` the shallow read yields
                // a''b while the list shows a'b, so callers that write this value
                // back (set_task_parent writes it as the child's `parent-id`) would
                // record a reference the parent does not answer to, and the
                // frontend's reflectStampedId would overwrite the correct row value
                // (Codex P2, PR #77). A value the strict reader cannot decode
                // reports no id — exactly as the list reports none (unreachable
                // here in practice: `id_property_unassignable` already ruled out
                // a non-empty value the strict reader rejects).
                Some((on_disk, v)) if !v.is_empty() => {
                    super::parse::strict_scalar_field(&content, &on_disk, false)
                }
                // Truly blank or absent → generate + stamp. A BLANK line is
                // rewritten under its ON-DISK casing so set_fields (case-
                // sensitive) replaces it — stamping the configured casing would
                // insert a case-mismatched DUPLICATE that scalar_id_ci's CI
                // read then shadows, hiding the id forever (Codex, PR #59).
                // Absent stamps a new line under the configured property name.
                found => {
                    blank_casing = found.map(|(on_disk, _)| on_disk);
                    let id = super::id::new_task_id();
                    generated = Some(id.clone());
                    Some(id)
                }
            }
        });
        if let (Some(key), Some(id)) = (ensure_id, generated.as_deref()) {
            effective.push((blank_casing.as_deref().unwrap_or(key), Some(id)));
        }
        // Nothing to write (an ensure-only call — a move backfill — on a task
        // whose id is already present): skip the redundant atomic rewrite, still
        // report the id. update_task always passes a non-empty `updates`, so this
        // only short-circuits those callers.
        if effective.is_empty() {
            return Ok(ensured);
        }
        let updated = set_fields(&content, &effective).ok_or(
            "Task frontmatter could not be updated (not a type: Task document, or its frontmatter is malformed)",
        )?;
        crate::capture_note::write_atomic_replacing(&canon_path, &updated)
            .map_err(|e| format!("Cannot save task: {e}"))?;
        Ok(ensured)
    })
}

/// Best-effort id backfill on a task file a structural move just relocated
/// (drag / editor move, delete-list): stamp a missing/blank id under
/// `property` (`None` = ids off → no-op). The move already mutated the vault,
/// so a stamp failure only WARNS — it must never fail the move that carried
/// it (audio-first discipline, borrowed from the capture domain). Returns the
/// task's effective id — freshly stamped or already present — for callers
/// that reflect it without a reload.
pub fn backfill_task_id(root: &Path, path: &Path, property: Option<&str>) -> Option<String> {
    let prop = property?;
    match update_task_fields(root, path, &[], Some(prop)) {
        Ok(id) => id,
        Err(e) => {
            log::warn!("task id backfill on {path:?} failed: {e}");
            None
        }
    }
}

/// Set a task's `status:` frontmatter on disk (see `update_task_fields`). A
/// status toggle never stamps an ID (`ensure_id: None` — a checkbox click is
/// not an edit), so the id return is discarded.
pub fn set_task_status(root: &Path, path: &Path, new_status: &str) -> Result<(), String> {
    update_task_fields(root, path, &[("status", Some(new_status))], None).map(|_| ())
}

#[cfg(test)]
mod tests;
