//! Pure hierarchy resolution over a loaded task set. Everything is keyed on
//! `parent-id`; the link is never consulted.

use super::TaskItem;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Child PATH -> parent PATH, for ONE vault's tasks, with edges resolved
/// THROUGH ids.
///
/// Keyed on paths, NOT ids: every task has a path, but a task can lack an id
/// while still naming a parent. An id-keyed index would drop such a task's
/// outgoing edge, so `P(no id, parent-id: c)` + `C(id: c)` would let "make P the
/// parent of C" pass and then create a P<->C cycle. Lacking an id only means a
/// task cannot be REFERENCED as a parent (design spec §3).
pub type ParentIndex<'a> = HashMap<&'a Path, &'a Path>;

/// Ids carried by more than one task — ambiguous, so unusable as a parent
/// reference. VB never creates these (duplicate regenerates, ensure_id never
/// overwrites), but a file copied in Explorer, a sync conflict, or a hand edit
/// does, and never-clobber then preserves them.
pub fn ambiguous_ids(tasks: &[TaskItem]) -> HashSet<&str> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for t in tasks {
        if let Some(id) = t.id.as_deref() {
            *seen.entry(id).or_insert(0) += 1;
        }
    }
    seen.into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(id, _)| id)
        .collect()
}

pub fn parent_index(tasks: &[TaskItem]) -> ParentIndex<'_> {
    let ambiguous = ambiguous_ids(tasks);
    // id -> path, for the UNambiguous ids only.
    let by_id: HashMap<&str, &Path> = tasks
        .iter()
        .filter_map(|t| {
            let id = t.id.as_deref()?;
            (!ambiguous.contains(id)).then_some((id, t.path.as_path()))
        })
        .collect();
    let mut idx = HashMap::new();
    for t in tasks {
        let Some(pid) = t.parent_id.as_deref() else {
            continue;
        };
        // An unresolvable or ambiguous parent-id yields no edge: the child is an
        // orphan, never a guess.
        if let Some(&parent_path) = by_id.get(pid) {
            idx.insert(t.path.as_path(), parent_path);
        }
    }
    drop_cyclic_edges(&mut idx);
    idx
}

/// Remove the edges of every node lying on a cycle. A hand-authored A -> B -> A
/// resolves two REAL edges; bounding `ancestors` only stops the walk, it does not
/// make either edge unresolved, so both rows would render each other as parent
/// and subtask. Dropping them makes both render parentless — visibly wrong data
/// the user can see and fix, rather than a confidently-rendered loop (design
/// spec §3). It also leaves the index cycle-free, so `would_create_cycle`
/// validates against exactly what the user is looking at.
fn drop_cyclic_edges(idx: &mut ParentIndex<'_>) {
    let cyclic: Vec<&Path> = idx
        .keys()
        .copied()
        .filter(|start| {
            // Walk up; if we come back to `start`, it is on a cycle.
            let mut seen = HashSet::new();
            let mut cur = *start;
            while let Some(&next) = idx.get(cur) {
                if next == *start {
                    return true;
                }
                if !seen.insert(next) {
                    return false; // a different cycle upstream, not ours
                }
                cur = next;
            }
            false
        })
        .collect();
    for path in cyclic {
        idx.remove(path);
    }
}

/// Ancestor paths of `start`, nearest first, EXCLUDING `start`. Bounded by a
/// visited set so a pre-existing hand-authored cycle terminates.
pub fn ancestors<'a>(index: &ParentIndex<'a>, start: &'a Path) -> Vec<&'a Path> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = start;
    while let Some(&parent) = index.get(cur) {
        if !seen.insert(parent) {
            break; // cycle already on disk
        }
        out.push(parent);
        cur = parent;
    }
    out
}

/// True when making `parent` the parent of `child` would create a cycle.
pub fn would_create_cycle(index: &ParentIndex<'_>, child: &Path, parent: &Path) -> bool {
    child == parent || ancestors(index, parent).contains(&child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// `name` doubles as the filename stem and the own-id (unless `id` is None).
    fn t(name: &str, id: Option<&str>, parent: Option<&str>) -> TaskItem {
        TaskItem {
            path: PathBuf::from(format!("/v/Tasks/{name}.md")),
            title: name.to_string(),
            status: "new".into(),
            created: "2026-07-25".into(),
            done: false,
            due: None,
            scheduled: None,
            priority: None,
            tags: vec![],
            list: String::new(),
            order: None,
            id: id.map(str::to_string),
            description: None,
            parent_id: parent.map(str::to_string),
            parent_link: None,
        }
    }

    fn p(name: &str) -> PathBuf {
        PathBuf::from(format!("/v/Tasks/{name}.md"))
    }

    #[test]
    fn detects_self_direct_and_transitive_cycles() {
        // c -> b -> a
        let tasks = vec![
            t("a", Some("a"), None),
            t("b", Some("b"), Some("a")),
            t("c", Some("c"), Some("b")),
        ];
        let idx = parent_index(&tasks);
        assert!(would_create_cycle(&idx, &p("a"), &p("a"))); // self
        assert!(would_create_cycle(&idx, &p("a"), &p("b"))); // direct
        assert!(would_create_cycle(&idx, &p("a"), &p("c"))); // transitive
        assert!(!would_create_cycle(&idx, &p("c"), &p("a"))); // c under a is fine
    }

    #[test]
    fn an_id_less_task_still_contributes_its_outgoing_edge() {
        // REGRESSION (design spec §3): P has no id of its own but names C as its
        // parent. An id-keyed index would drop P's edge and let "make P the
        // parent of C" pass, creating a P<->C cycle on the next write.
        let tasks = vec![t("p", None, Some("c")), t("c", Some("c"), None)];
        let idx = parent_index(&tasks);
        assert_eq!(idx.get(p("p").as_path()), Some(&p("c").as_path()));
        assert!(would_create_cycle(&idx, &p("c"), &p("p")));
    }

    #[test]
    fn a_preexisting_on_disk_cycle_drops_both_edges() {
        // Bounding the walk is not enough: both rows must resolve PARENTLESS, or
        // they render each other as parent and subtask (design spec §3).
        let tasks = vec![t("a", Some("a"), Some("b")), t("b", Some("b"), Some("a"))];
        let idx = parent_index(&tasks);
        assert!(idx.is_empty(), "cyclic nodes contribute no edges");
        assert!(ancestors(&idx, &p("a")).is_empty());
    }

    #[test]
    fn a_cycle_does_not_drop_unrelated_edges() {
        // c -> d is fine even though a <-> b loop elsewhere.
        let tasks = vec![
            t("a", Some("a"), Some("b")),
            t("b", Some("b"), Some("a")),
            t("c", Some("c"), Some("d")),
            t("d", Some("d"), None),
        ];
        let idx = parent_index(&tasks);
        assert_eq!(idx.get(p("c").as_path()), Some(&p("d").as_path()));
        assert!(!idx.contains_key(p("a").as_path()));
    }

    #[test]
    fn duplicate_ids_are_ambiguous_and_resolve_no_edges() {
        // Two files share id "dup" — a copied file. Neither end can identify a
        // unique task, so no edge is invented.
        let tasks = vec![
            t("x", Some("dup"), None),
            t("y", Some("dup"), None),
            t("z", Some("z"), Some("dup")),
        ];
        assert!(ambiguous_ids(&tasks).contains("dup"));
        assert!(parent_index(&tasks).is_empty());
    }

    #[test]
    fn an_unresolvable_parent_id_yields_no_edge() {
        let tasks = vec![t("orphan", Some("o"), Some("gone"))];
        assert!(parent_index(&tasks).is_empty());
    }
}
