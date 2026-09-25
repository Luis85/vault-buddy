//! Recover a webcam take a crash interrupted (GAP-197, hardening Task 9;
//! the user's decision D4).
//!
//! While a take records, its bytes live only in `takes\.<takeId>.webm.part`
//! and its state only in memory, so a crash, a kill, a power loss — or a
//! quit, which closes no editor session — leaves the user's recording (a
//! streamable WebM prefix that plays) where nothing offered it back. Now the
//! project's next open does: when an open MINTS a session (never when it
//! reuses a live one), every stale part of ours in the project's `takes\`
//! is finished through the finish's own land + register
//! (`webcam_finish::land`) — remuxed `-c copy`, probed, hashed, recorded in
//! `sources.json` and added through ONE `AddAssets` as "Webcam take N
//! (recovered)", or, without ffmpeg, kept exactly as recorded with its
//! length unknown (A09). The open then answers the session as it stands
//! AFTER the recovery, so the webview's first edit meets the right revision;
//! the project reads unsaved, like any other take, until the user saves.
//! `EditorOpenResult` is not widened (the rulings' l.87): the recovered take
//! is simply in the library.
//!
//! **Only our own files.** A candidate is a PLAIN file (no-follow: a symlink
//! wearing the name is never read, moved or removed) in a plain `takes\`
//! folder, named `.<take id>.webm.part` for a valid `take-…` id
//! (`core::editor::take::recoverable_part_take_id`), last modified at least
//! `STALE_AFTER` ago — the screen sweep's 60 s (a live take appends a chunk
//! every timeslice), and it is not a take this process holds. A part whose
//! take already landed (its `.webm` exists) or is already registered (a
//! `sources.json` record or a graph asset under its id) is left in place,
//! never recovered twice and never clobbering a record. An EMPTY stale part
//! recorded nothing and is removed; a leftover `.<take id>.remux.webm` of a
//! part being recovered is removed first (the land refuses to run over it).
//! A part that cannot be landed (ffmpeg cannot read it, no picture) stays
//! where it is and is tried again on the next new session: a recording is
//! never deleted for failing to remux.
//!
//! **Bounded.** It runs on the open's blocking thread (never the main
//! thread), under the open's `open` lock — so a discard, which takes that
//! lock first, waits for it rather than meeting it half-done — and recovers
//! at most `MAX_RECOVERED_PER_OPEN` takes per open, each a remux bounded by
//! the finish's own timeout; any others wait for the next new session.
//!
//! **Locks** — `open`, then the take's entry, then the save lock, then
//! `sessions` (`webcam_registry.rs`). A recovering take is registered in
//! `EditorState::takes` while it lands, so a discard's quiesce sees it busy
//! and `forget_session` never removes its part (`TakeSlot::recovered`).
//! Nothing here logs a path or a file name.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use vault_buddy_core::editor::take::{recoverable_part_take_id, TakeError, TakeState};
use vault_buddy_core::editor::{EditorError, EditorProjection};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::project_store::project_dir;
use super::store_io::load_sources;
use super::webcam_finish::{land, Landed, TakeIo};
use super::webcam_registry::{remove_owned, TakeEntry, TakeSlot};
use super::EditorState;

/// How long a part must have gone unwritten before it is a crash's leftover
/// rather than a take still being recorded — the screen sweep's window.
pub(crate) const STALE_AFTER: Duration = Duration::from_secs(60);

/// The most interrupted takes one open recovers (each is a remux).
pub(crate) const MAX_RECOVERED_PER_OPEN: usize = 4;

/// After an open registered `projection`'s session: when the open MINTED
/// it (`minted`), recover the project's interrupted takes, and answer the
/// session's projection as it stands afterwards. A reused session is
/// answered as it is — it is not a new open.
pub(crate) fn recover_after_open(
    state: &EditorState,
    root: &Path,
    projection: EditorProjection,
    minted: bool,
    io: &dyn TakeIo,
) -> EditorProjection {
    if !minted {
        return projection;
    }
    let session_id = projection.snapshot.session_id.clone();
    let project_id = projection.project.id.clone();
    let recovered =
        recover_interrupted_takes(state, root, &session_id, &project_id, io, SystemTime::now());
    if recovered == 0 {
        return projection;
    }
    lock_ignoring_poison(&state.sessions)
        .get(&session_id)
        .map_or(projection, EditorProjection::of)
}

/// Recover the stale parts in `project_id`'s `takes\` into `session_id`
/// (see the module doc); the number registered.
pub(crate) fn recover_interrupted_takes(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    project_id: &str,
    io: &dyn TakeIo,
    now: SystemTime,
) -> usize {
    let Some(dir) = project_dir(root, project_id).map(|d| d.join("takes")) else {
        return 0;
    };
    let found = stale_parts(&dir, now);
    if found.is_empty() {
        return 0;
    }
    let registered = match registered_ids(state, root, session_id, project_id) {
        Ok(ids) => ids,
        Err(e) => {
            log::warn!(
                "webcam recovery: the project's sources could not be read; nothing recovered: {}",
                e.message
            );
            return 0;
        }
    };
    let mut recovered = 0;
    let mut attempted = 0;
    for part in found {
        if registered.contains(&part.take_id) {
            log::warn!(
                "webcam recovery: take {} is already registered; its part was left in place",
                part.take_id
            );
            continue;
        }
        if attempted == MAX_RECOVERED_PER_OPEN {
            log::info!("webcam recovery: more interrupted takes wait for the next open");
            break;
        }
        let slot = TakeSlot {
            session_id: session_id.to_string(),
            project_id: project_id.to_string(),
            dir: dir.clone(),
            take_id: part.take_id.clone(),
            recovered: true,
            entry: Mutex::new(TakeEntry {
                state: TakeState::Discarded,
                failed: None,
            }),
        };
        if std::fs::symlink_metadata(slot.out()).is_ok() {
            log::warn!(
                "webcam recovery: take {} already landed; its part was left in place",
                part.take_id
            );
            continue;
        }
        let state_now = match TakeState::recovered(part.bytes) {
            Ok(s) => s,
            Err(TakeError::NothingRecorded) => {
                // A stale, plain, empty part of ours recorded nothing.
                remove_owned(&slot.part());
                continue;
            }
            Err(e) => {
                log::warn!("webcam recovery: take {}: {}", part.take_id, e.message());
                continue;
            }
        };
        attempted += 1;
        match recover_one(state, root, io, slot, state_now) {
            Ok(Some(Landed::Indexed(_))) => recovered += 1,
            Ok(Some(Landed::Raw(_))) => {
                log::warn!(
                    "webcam recovery: take {} was kept as recorded (no ffmpeg; length unknown)",
                    part.take_id
                );
                recovered += 1;
            }
            Ok(None) => {}
            Err(e) => log::warn!(
                "webcam recovery: take {} could not be recovered; its part was left in place: {}",
                part.take_id,
                e.message
            ),
        }
    }
    if recovered > 0 {
        log::info!("webcam recovery: {recovered} interrupted take(s) registered as recovered");
    }
    recovered
}

/// One stale part of ours.
struct StalePart {
    take_id: String,
    bytes: u64,
    modified: SystemTime,
}

/// Every stale part of ours in `dir`, oldest first — so "Webcam take N"
/// numbers them in the order they were recorded. A `dir` that is missing or
/// not a plain folder (a link wearing its name) has none.
fn stale_parts(dir: &Path, now: SystemTime) -> Vec<StalePart> {
    match std::fs::symlink_metadata(dir) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            log::warn!("webcam recovery: a project's takes folder is not a plain folder; skipped");
            return Vec::new();
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            log::warn!("webcam recovery: cannot inspect a project's takes folder: {e}");
            return Vec::new();
        }
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("webcam recovery: cannot list a project's takes folder: {e}");
            return Vec::new();
        }
    };
    let mut parts: Vec<StalePart> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let take_id = recoverable_part_take_id(name.to_str()?)?.to_string();
            // No-follow: a symlink (or a directory) wearing the name is not
            // a take's part.
            let meta = std::fs::symlink_metadata(entry.path()).ok()?;
            if !meta.is_file() {
                log::warn!(
                    "webcam recovery: take {take_id}'s part is not a plain file; left in place"
                );
                return None;
            }
            let modified = meta.modified().ok()?;
            // A modification time in the future reads as fresh.
            let age = now.duration_since(modified).unwrap_or(Duration::ZERO);
            (age >= STALE_AFTER).then_some(StalePart {
                take_id,
                bytes: meta.len(),
                modified,
            })
        })
        .collect();
    parts.sort_by(|a, b| (a.modified, &a.take_id).cmp(&(b.modified, &b.take_id)));
    parts
}

/// The take ids already registered: a `sources.json` record or a graph
/// asset under the id. Recovering one again would clobber its record.
fn registered_ids(
    state: &EditorState,
    root: &Path,
    session_id: &str,
    project_id: &str,
) -> Result<BTreeSet<String>, EditorError> {
    let mut ids: BTreeSet<String> = load_sources(root, project_id)?.into_keys().collect();
    if let Some(session) = lock_ignoring_poison(&state.sessions).get(session_id) {
        ids.extend(session.project().assets.iter().map(|a| a.id.clone()));
    }
    Ok(ids)
}

/// Land one recovered take: registered in `EditorState::takes` (only if no
/// take of this process holds its id) with its entry held, a leftover remux
/// temp removed, then the finish's own land + register. `None` when a live
/// take of this process holds the id — then nothing is touched.
fn recover_one(
    state: &EditorState,
    root: &Path,
    io: &dyn TakeIo,
    slot: TakeSlot,
    recovered: TakeState,
) -> Result<Option<Landed>, EditorError> {
    let slot = Arc::new(slot);
    let mut entry = lock_ignoring_poison(&slot.entry);
    entry.state = recovered;
    {
        let mut takes = lock_ignoring_poison(&state.takes.0);
        if takes.contains_key(&slot.take_id) {
            log::warn!(
                "webcam recovery: take {} is live in this process; left alone",
                slot.take_id
            );
            return Ok(None);
        }
        takes.insert(slot.take_id.clone(), Arc::clone(&slot));
    }
    let landed = super::discard::refuse_if_closing(state, &slot.session_id).and_then(|()| {
        remove_owned(&slot.remux_temp());
        let mut next = entry.state.clone();
        next.finish_recovered()?;
        let landed = land(state, root, io, &slot)?;
        entry.state = next;
        Ok(landed)
    });
    drop(entry);
    lock_ignoring_poison(&state.takes.0).remove(&slot.take_id);
    landed.map(Some)
}

#[cfg(test)]
#[path = "webcam_recover_tests.rs"]
mod tests;
