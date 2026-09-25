//! The webcam takes' registry (Task 49), split out of `webcam_commands.rs`
//! when the final whole-branch review's discard fix (I1) needed room there:
//! every take this process began and has not yet discarded, and the
//! owned-file removal both halves share.
//!
//! **Locks.** `TakeRegistry`'s map is a LEAF lock, taken only to find, add
//! or remove one slot. Each slot's own `entry` lock is held across that
//! take's file I/O (so its chunks, its finish and its discard are serial)
//! and, in finish, across the save lock and then `sessions` — the order is:
//! take entry, then save lock, then `sessions`. Nothing takes a take entry
//! while holding the save lock: `forget_session` (run by `drop_session`
//! after the maps are released, under the save lock) never locks an entry,
//! and `wait_idle` (a discard's quiesce, `discard.rs`) only ever TRIES one,
//! before the save lock is taken.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use vault_buddy_core::editor::take::TakeState;
use vault_buddy_core::editor::{EditorError, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::redact::redact_path;

/// One take's mutable state, behind its slot's lock.
pub(crate) struct TakeEntry {
    pub(crate) state: TakeState,
    /// Why the take stopped accepting chunks, once it has.
    pub(crate) failed: Option<String>,
}

/// One live take: who owns it and where its files live (outside the lock,
/// so `forget_session` can clean up without ever waiting on a take).
pub(crate) struct TakeSlot {
    pub(crate) session_id: String,
    pub(crate) project_id: String,
    pub(crate) dir: PathBuf,
    pub(crate) take_id: String,
    pub(crate) entry: Mutex<TakeEntry>,
}

impl TakeSlot {
    pub(crate) fn part(&self) -> PathBuf {
        self.dir.join(format!(".{}.webm.part", self.take_id))
    }

    pub(crate) fn remux_temp(&self) -> PathBuf {
        self.dir.join(format!(".{}.remux.webm", self.take_id))
    }

    pub(crate) fn file_name(&self) -> String {
        format!("{}.webm", self.take_id)
    }

    pub(crate) fn out(&self) -> PathBuf {
        self.dir.join(self.file_name())
    }
}

/// Every take this process began and has not yet discarded, by take id.
#[derive(Default)]
pub struct TakeRegistry(pub(crate) Mutex<HashMap<String, Arc<TakeSlot>>>);

impl TakeRegistry {
    /// The take, if `session_id` owns it — `invalidRequest` otherwise, so a
    /// guessed or another session's take id learns nothing.
    pub(crate) fn get(
        &self,
        session_id: &str,
        take_id: &str,
    ) -> Result<Arc<TakeSlot>, EditorError> {
        lock_ignoring_poison(&self.0)
            .get(take_id)
            .filter(|slot| slot.session_id == session_id)
            .cloned()
            .ok_or_else(|| {
                EditorError::new(
                    EditorErrorCode::InvalidRequest,
                    "This webcam take is not part of this editing session.",
                )
            })
    }

    fn slots_of(&self, session_id: &str) -> Vec<Arc<TakeSlot>> {
        lock_ignoring_poison(&self.0)
            .values()
            .filter(|s| s.session_id == session_id)
            .cloned()
            .collect()
    }

    /// The ids of `session_id`'s takes that are still recording — what the
    /// close guard would lose, and what Checks counts as unfinished (Task
    /// 54, `checks_commands`).
    ///
    /// Never WAITS on an entry (final review M2): a finish holds its entry
    /// across a remux that can run for minutes, and `editor_get_checks`
    /// must not sit on a blocking thread that long. A take whose entry is
    /// busy is being written or finished right now — not finished yet, so
    /// it counts as open.
    pub(crate) fn open_takes(&self, session_id: &str) -> Vec<String> {
        let mut ids: Vec<String> = self
            .slots_of(session_id)
            .iter()
            .filter(|s| match s.entry.try_lock() {
                Ok(entry) => matches!(entry.state, TakeState::Recording { .. }),
                Err(TryLockError::Poisoned(p)) => {
                    matches!(p.into_inner().state, TakeState::Recording { .. })
                }
                Err(TryLockError::WouldBlock) => true,
            })
            .map(|s| s.take_id.clone())
            .collect();
        ids.sort();
        ids
    }

    /// Wait (at most `bound`) until no take of `session_id` is mid-write or
    /// mid-finish — `false` if one still is. A discard's quiesce: it runs
    /// AFTER the session is marked closing, so every append, finish and
    /// begin that takes an entry later refuses (`discard.rs`), and this is
    /// the one that could still be running.
    pub(crate) fn wait_idle(&self, session_id: &str, bound: Duration) -> bool {
        let started = Instant::now();
        loop {
            let busy = self
                .slots_of(session_id)
                .iter()
                .any(|s| matches!(s.entry.try_lock(), Err(TryLockError::WouldBlock)));
            if !busy {
                return true;
            }
            if started.elapsed() >= bound {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// A closing session's takes: forgotten, and every unfinished take's
    /// `.part` removed (nothing could ever finish it now). Runs under the
    /// session's save lock (`drop_session`), so it NEVER locks an entry —
    /// a finish holds its entry while it takes the save lock. A finished
    /// take has no `.part`, so its `.webm` is untouched.
    pub(crate) fn forget_session(&self, session_id: &str) {
        let gone: Vec<Arc<TakeSlot>> = {
            let mut map = lock_ignoring_poison(&self.0);
            let ids: Vec<String> = map
                .iter()
                .filter(|(_, s)| s.session_id == session_id)
                .map(|(id, _)| id.clone())
                .collect();
            ids.iter().filter_map(|id| map.remove(id)).collect()
        };
        for slot in gone {
            remove_owned(&slot.part());
        }
    }
}

/// Remove one of a take's own files: a plain file only (a symlink or a
/// directory wearing the name is left alone), a missing one is fine, any
/// other failure is logged.
pub(crate) fn remove_owned(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => {
            if let Err(e) = std::fs::remove_file(path) {
                log::warn!("webcam take: could not remove {}: {e}", redact_path(path));
            }
        }
        Ok(_) => log::warn!(
            "webcam take: {} is not a plain file; left in place",
            redact_path(path)
        ),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("webcam take: cannot inspect {}: {e}", redact_path(path)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    // Final review M2: a finish holds its take's entry across a remux of up
    // to 15 minutes. `open_takes` (Checks, the close guard's count) waited
    // on that lock, parking a blocking-pool thread for as long.
    #[test]
    fn open_takes_never_waits_on_a_take_that_is_finishing() {
        let registry: &'static TakeRegistry = Box::leak(Box::default());
        let slot = Arc::new(TakeSlot {
            session_id: "ses-a".into(),
            project_id: "proj-a".into(),
            dir: PathBuf::from("unused"),
            take_id: "take-a".into(),
            entry: Mutex::new(TakeEntry {
                state: TakeState::new(),
                failed: None,
            }),
        });
        lock_ignoring_poison(&registry.0).insert("take-a".into(), Arc::clone(&slot));
        let finishing = lock_ignoring_poison(&slot.entry);
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("open-takes-probe".into())
            .spawn(move || tx.send(registry.open_takes("ses-a")).unwrap())
            .unwrap();
        let answer = rx.recv_timeout(Duration::from_secs(2));
        drop(finishing);
        assert_eq!(
            answer.expect("open_takes waited on a take that is finishing"),
            vec!["take-a".to_string()],
            "a take being finished is not finished yet"
        );
    }
}
