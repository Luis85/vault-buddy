//! The read side: scan the tasks folder, map files to `TaskItem`s, and the
//! clock-free sort ("overdue"/"today" need a clock, so date-bucket grouping
//! is deliberately the frontend's job, not the sort's).

use super::collect::collect_task_file;
use super::parse::is_valid_due;
use std::path::{Path, PathBuf};

/// One task surfaced in the list.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskItem {
    pub path: PathBuf,
    pub title: String,
    pub status: String,
    pub created: String,
    pub done: bool,
    pub due: Option<String>,
    /// The do/plan date (`YYYY-MM-DD`) — when the user plans to WORK the task,
    /// distinct from `due` (the deadline). Read then validator-filtered, so it is
    /// `None` when absent OR unparseable (an honest DTO/MCP boundary).
    pub scheduled: Option<String>,
    pub priority: Option<String>,
    pub tags: Vec<String>,
    /// The task's List = its parent folder relative to the tasks root, always
    /// `/`-joined ("" at the root) — the identity crosses IPC and merges
    /// across platforms, so it never carries the OS separator.
    pub list: String,
    /// Manual rank from the `order:` frontmatter number; lenient read —
    /// unparseable/non-finite is unranked, never an error.
    pub order: Option<f64>,
    /// The generated id read from the vault's configured id property, when
    /// the vault has task IDs enabled; `None` when the feature is off (the
    /// property is never read) or the file simply has no value there.
    pub id: Option<String>,
    /// Free-text detail, decoded from the `description:` frontmatter scalar
    /// (multi-line, `#`-tolerant). `None` when absent/empty.
    pub description: Option<String>,
    /// The parent Task's stable id, read from `parent-id`. Authoritative for
    /// hierarchy resolution. NOT gated on the vault's id feature — a task's own
    /// id is (it is read under the configured property), but this is a plain
    /// key that always means the same thing.
    pub parent_id: Option<String>,
    /// The parent's Obsidian link (`parent`), carried verbatim for navigation.
    /// Never parsed for meaning.
    pub parent_link: Option<String>,
}

/// Sort tier for a priority value: high first, low last, anything else
/// (normal, absent, hand-authored unknown) in the middle.
pub fn priority_rank(p: Option<&str>) -> u8 {
    match p {
        Some("high") => 0,
        Some("low") => 2,
        _ => 1,
    }
}

/// (has-no-valid-due, due) — tuple compare puts valid dues first, ascending;
/// an unparseable hand-authored due sorts with the undated.
fn due_key(t: &TaskItem) -> (bool, &str) {
    match t.due.as_deref().filter(|d| is_valid_due(d)) {
        Some(d) => (false, d),
        None => (true, ""),
    }
}

/// Every `type: Task` file anywhere under `root`, best-effort — the configured
/// tasks folder is walked recursively so tasks organized into subfolders are
/// all surfaced. Open tasks (status != "done") first — sorted by due
/// ascending (no/unparseable due last), then priority tier, then newest
/// `created`, then title; completed tasks after, sorted by newest `created`
/// then title. A missing/unreadable root or file degrades silently.
///
/// `id_property` is the vault's configured task-id frontmatter key, or
/// `None` when task IDs are off — the property is then never read, so a
/// disabled vault pays no extra cost and `TaskItem.id` is always `None`.
///
/// A PRESENTATION function in two ways that make it wrong for a hierarchy
/// guard: `status: archived` Tasks are dropped, and a file that can't be read
/// is silently skipped. A guard (the cycle index, the id-settings guard) must
/// see every `parent-id` edge or a cycle can slip through validation — use
/// `list_tasks_structural` there instead.
pub fn list_tasks(root: &Path, id_property: Option<&str>) -> Vec<TaskItem> {
    // `scan` only ever returns Err in Structural mode — View never fails.
    let mut out = scan(root, id_property, ScanMode::View).unwrap_or_default();
    sort_tasks(&mut out);
    out
}

/// The STRUCTURAL counterpart of `list_tasks`, for a hierarchy guard: the
/// SAME walk (never copied — thread new modes through `ScanMode` instead),
/// but it INCLUDES `status: archived` Tasks (their files still carry
/// `parent-id`, and a cycle routed through one must still be visible to the
/// guard) and FAILS the whole scan — naming the offending path — when any
/// `.md` file cannot be read, rather than silently dropping it. A file's
/// `type:` can't be checked without reading it, so an unreadable `.md` is
/// treated as a POSSIBLE Task: dropping it would drop a possible hierarchy
/// edge, and a missing edge is exactly what lets a cycle pass validation and
/// get written (Codex P2, PR #77). The rule: a view may degrade; a guard must
/// refuse.
pub fn list_tasks_structural(
    root: &Path,
    id_property: Option<&str>,
) -> Result<Vec<TaskItem>, String> {
    let mut out = scan(root, id_property, ScanMode::Structural)?;
    sort_tasks(&mut out);
    Ok(out)
}

/// The archived-INCLUSIVE counterpart of `list_tasks`, for a READ that
/// decides whether a relationship exists rather than merely displaying a
/// list — the frontend's parent/subtask hierarchy resolution (Task Detail's
/// Parent row and the main list's parent chip/subtask count, both built
/// through the one shared `buildParentIndex` rule). An archived task can
/// still be somebody's PARENT: a resolver built from the archived-EXCLUDED
/// `list_tasks` view can never see that edge at all, so it wrongly reports
/// "no parent" for an active child whose parent was later archived — and a
/// user who believes there is no relationship can then pick a new parent,
/// silently REPLACING the real one they were never shown. That is why this
/// mirrors `list_tasks_structural`'s archived-inclusive posture — but NOT
/// its abort-on-unreadable-file strictness: this is still a best-effort
/// VIEW, not a write-time guard, so a single unreadable file degrades and
/// the scan continues, and a missing root is simply empty, exactly like
/// `list_tasks`. "A view may degrade; a guard must refuse" — this function
/// is the view that must not filter archived rows, not the guard that must
/// not miss an edge.
pub fn list_tasks_including_archived(root: &Path, id_property: Option<&str>) -> Vec<TaskItem> {
    let mut out = scan(root, id_property, ScanMode::ViewIncludingArchived).unwrap_or_default();
    sort_tasks(&mut out);
    out
}

/// Which of the three callers is walking. Two independent properties
/// (include-archived, abort-on-unreadable) used to collapse to only two
/// combinations — lenient+presentation, strict+structural — so this enum
/// deliberately had no room for a third nobody had asked for. Fix 1 (the
/// subtasks vault-UX-polish increment) asked for exactly that third
/// combination: `ViewIncludingArchived` is lenient like `View` (a single
/// unreadable file degrades and the scan continues — this reader has no
/// write to protect, so there is nothing to refuse) but keeps `status:
/// archived` rows like `Structural` (an archived task can still be
/// somebody's PARENT; see `list_tasks_including_archived`'s own doc comment
/// for the failure this prevents). It is intentional, not accidental drift,
/// precisely because it is still a genuine best-effort VIEW.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanMode {
    View,
    ViewIncludingArchived,
    Structural,
}

impl ScanMode {
    /// Only View drops `status: archived` Tasks; both other modes keep them.
    pub(super) fn include_archived(self) -> bool {
        self != ScanMode::View
    }

    /// Only Structural aborts the whole scan on the first unreadable `.md`
    /// file; both View variants degrade-and-continue.
    pub(super) fn strict(self) -> bool {
        self == ScanMode::Structural
    }
}

/// The ONE walk both `list_tasks` and `list_tasks_structural` drive over
/// `crate::vault_walk` (canonical containment, cycle set, dot-dir skip,
/// single-sourced with the search scan) — do not copy it; a future mode
/// belongs in `ScanMode`. Structural mode stops at the first unreadable file
/// (`Flow::Stop`) instead of scanning the rest of a vault it already knows it
/// must reject.
fn scan(root: &Path, id_property: Option<&str>, mode: ScanMode) -> Result<Vec<TaskItem>, String> {
    let canon_root = match std::fs::canonicalize(root) {
        Ok(p) => p,
        // A missing tasks folder is legitimately empty in EITHER mode — a
        // vault that has never created one has no graph to protect yet
        // (finding 1). Any OTHER root failure (EACCES, an unavailable
        // network share) must not read as "no tasks" in Structural mode: a
        // settings guard would then conclude no parent links exist and
        // permit an unsafe id-property change on the strength of a scan
        // that never actually ran. View mode keeps today's exact
        // best-effort/empty behavior regardless of the error kind.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound || !mode.strict() => {
            return Ok(Vec::new())
        }
        Err(e) => return Err(format!("Cannot resolve tasks folder: {e}")),
    };
    let mut out = Vec::new();
    let mut first_error: Option<String> = None;
    let unreadable_dirs = crate::vault_walk::walk_vault(&canon_root, &mut |path, name| {
        collect_task_file(
            path,
            name,
            &canon_root,
            id_property,
            mode,
            &mut first_error,
            &mut out,
        );
        if first_error.is_some() {
            crate::vault_walk::Flow::Stop
        } else {
            crate::vault_walk::Flow::Continue
        }
    });
    if let Some(e) = first_error {
        return Err(e);
    }
    // A directory the walk could not fully enumerate hides possible
    // `parent-id` edges just as an unreadable FILE does — in Structural mode
    // that must refuse rather than report a silently-partial graph as
    // complete (finding 2). View mode (list_tasks) ignores it, same as
    // always.
    if mode.strict() {
        if let Some(first) = unreadable_dirs.first() {
            return Err(format!("Cannot fully scan the tasks folder: {first}"));
        }
    }
    Ok(out)
}

/// Open first. Open tasks: due ascending (no/invalid due last), then
/// priority tier, then newest created, then title. Done tasks ignore due —
/// newest created first, then title. Clock-free: "overdue"/"today" need a
/// clock, so bucketing is the frontend's job, not the sort's. Shared by both
/// entry points so they can never disagree on order.
fn sort_tasks(out: &mut [TaskItem]) {
    out.sort_by(|a, b| {
        a.done.cmp(&b.done).then_with(|| {
            if a.done {
                b.created
                    .cmp(&a.created)
                    .then_with(|| a.title.cmp(&b.title))
            } else {
                due_key(a)
                    .cmp(&due_key(b))
                    .then_with(|| {
                        priority_rank(a.priority.as_deref())
                            .cmp(&priority_rank(b.priority.as_deref()))
                    })
                    .then_with(|| b.created.cmp(&a.created))
                    .then_with(|| a.title.cmp(&b.title))
            }
        })
    });
}

#[cfg(test)]
mod tests;
