//! Bounded undo/redo over whole-project snapshots (R14, F-13).
//!
//! Every entry pairs the project state BEFORE an edit with a human-readable
//! label describing that edit -- `EditorSnapshot.undoLabel` is "what undo
//! would undo", not what undo restores you TO. `EditorSession::execute`
//! (`session.rs`) is the only caller: it builds a candidate via
//! `commands::apply`, validates it, and only THEN calls `push`/`undo`/
//! `redo` here as part of installing the result.

use std::collections::VecDeque;

use super::limits::MAX_HISTORY;
use super::model::Project;

/// `undo` holds the most-recent-last deque of `(project-before-the-edit,
/// label)` pairs, capped at `MAX_HISTORY`; `redo` mirrors it for edits just
/// undone, cleared the moment a genuinely new edit lands.
pub struct History {
    undo: VecDeque<(Project, String)>,
    redo: Vec<(Project, String)>,
}

impl History {
    pub fn new() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    /// Records a just-applied edit: `prev` is the project state BEFORE the
    /// edit, `label` describes the edit (`commands::meta::rename` returns
    /// `"Rename"`, for example). A fresh edit always clears the redo stack
    /// -- once the user diverges from the point they undid to, "redo"
    /// would otherwise resurrect a future that no longer exists. Evicts
    /// the OLDEST undo entry once the deque would exceed `MAX_HISTORY`.
    pub fn push(&mut self, prev: Project, label: String) {
        self.redo.clear();
        self.undo.push_back((prev, label));
        while self.undo.len() > MAX_HISTORY {
            self.undo.pop_front();
        }
    }

    /// Pops the most recent undo entry, pushes `current` onto redo under
    /// the SAME label (so redo re-applies the edit undo just reverted), and
    /// returns the project state to restore plus that label. `None` when
    /// there is nothing to undo.
    pub fn undo(&mut self, current: Project) -> Option<(Project, String)> {
        let (restored, label) = self.undo.pop_back()?;
        self.redo.push((current, label.clone()));
        Some((restored, label))
    }

    /// The mirror of `undo`: pops the most recent redo entry, pushes
    /// `current` back onto undo (capped the same way `push` caps it), and
    /// returns the project state to restore plus that label. `None` when
    /// there is nothing to redo.
    pub fn redo(&mut self, current: Project) -> Option<(Project, String)> {
        let (restored, label) = self.redo.pop()?;
        self.undo.push_back((current, label.clone()));
        while self.undo.len() > MAX_HISTORY {
            self.undo.pop_front();
        }
        Some((restored, label))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// What undo would undo, if anything -- `EditorSnapshot.undoLabel`.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.back().map(|(_, label)| label.as_str())
    }

    /// What redo would redo, if anything -- `EditorSnapshot.redoLabel`.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|(_, label)| label.as_str())
    }
}

impl Default for History {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::test_support::minimal_project;

    fn titled(title: &str) -> Project {
        let mut p = minimal_project();
        p.title = title.to_string();
        p
    }

    #[test]
    fn push_records_prev_and_clears_redo() {
        let mut h = History::new();
        h.redo.push((titled("stale"), "Stale".to_string()));
        h.push(titled("before"), "Rename".to_string());
        assert!(h.can_undo());
        assert!(!h.can_redo(), "a fresh push must clear redo");
        assert_eq!(h.undo_label(), Some("Rename"));
    }

    #[test]
    fn undo_then_redo_round_trips() {
        let mut h = History::new();
        h.push(titled("A"), "Rename".to_string());

        let (restored, label) = h.undo(titled("B")).expect("one entry to undo");
        assert_eq!(restored.title, "A");
        assert_eq!(label, "Rename");
        assert!(!h.can_undo());
        assert!(h.can_redo());
        assert_eq!(h.redo_label(), Some("Rename"));

        let (back, label2) = h.redo(titled("A")).expect("one entry to redo");
        assert_eq!(back.title, "B");
        assert_eq!(label2, "Rename");
        assert!(h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn undo_and_redo_are_none_when_empty() {
        let mut h = History::new();
        assert!(h.undo(titled("x")).is_none());
        assert!(h.redo(titled("x")).is_none());
    }

    #[test]
    fn push_evicts_the_oldest_beyond_max_history() {
        let mut h = History::new();
        for i in 0..=MAX_HISTORY {
            h.push(titled(&format!("t{i}")), format!("Rename {i}"));
        }
        assert_eq!(h.undo.len(), MAX_HISTORY, "must not grow past MAX_HISTORY");
        // The newest entry (pushed last) must survive eviction.
        let newest_label = format!("Rename {MAX_HISTORY}");
        assert_eq!(h.undo_label(), Some(newest_label.as_str()));
    }
}
