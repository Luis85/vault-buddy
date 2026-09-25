//! The tutorial editor's SHELL-side surface: the owned project store on
//! disk (`project_store`, `store_io`, Task 9), pin-aware staging
//! integration, the `editor_*` session commands (`session_commands`, Task
//! 10) and the durable save/list/reopen commands (`save_commands`, Task 12)
//! behind R8's caller-window check (`authz`), and — among the later ones —
//! the render jobs and their product ledger (`render_jobs`, Task 46).
//!
//! Every `#[tauri::command]` in ANY file under this directory must take
//! `window: WebviewWindow` and call `authz::require_editor_window(&window)`
//! first — `authz_guard.rs` scans the whole directory, whatever a file is
//! named, and fails naming the command that does not.

pub mod authz;
#[cfg(test)]
mod authz_guard;
#[cfg(test)]
mod capability_guard;
pub mod caption_commands;
pub mod checks_commands;
pub mod diagnostics;
pub(crate) mod discard;
pub mod guide_commands;
pub mod media_commands;
pub mod media_derive;
pub mod media_import;
pub mod media_jobs;
pub mod media_probe;
pub mod package_commands;
pub mod package_import;
#[cfg(test)]
mod package_test_support;
pub mod prefs_commands;
pub mod project_discard;
pub mod project_store;
pub mod publish;
pub mod recovery;
pub mod redact;
#[cfg(test)]
mod redact_guard;
pub mod relink_commands;
pub mod relink_media;
pub mod render_commands;
pub mod render_jobs;
pub mod render_review;
pub mod save_commands;
pub mod session_close;
pub mod session_commands;
pub mod store_io;
pub mod subtitle_commands;
pub(crate) mod vault_dir;
pub mod webcam_commands;
pub mod webcam_registry;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use vault_buddy_core::editor::EditorSession;

/// The live editing sessions (managed in `lib.rs`).
///
/// `sessions` maps a session id to its session; `by_project` maps a project
/// id to the one session open on it, so a second open of the same project
/// reuses that session instead of forking a second, racing one.
///
/// `open` is taken by every open (find-or-mint + pin + register), every
/// project removal and every pin writer — including a session's
/// `discardProject` (`session_close::close_in`, review finding I-1), which
/// holds it from before its closing mark until the project is gone, and the
/// staged-capture Discard/Clear, the no-session project discard, a package
/// import and both startup sweeps. Without it two opens of one unpinned
/// capture both miss the pin and the orphan scan and mint two projects, the
/// second pin silently orphaning the first — and a discard could scan for
/// pins before an open in progress landed one, leaving a pin to a deleted
/// project. Both open paths refuse a project whose session is being
/// discarded (`discard::refuse_if_project_closing`).
///
/// **Lock order: `open` → the per-session save lock → `by_project` →
/// `sessions`.** `jobs` and `closing` are LEAVES, not links in that chain:
/// either may be taken anywhere after `open` — `drop_session` takes `jobs`
/// under a save lock, `discard::mark_closing` takes it before one — and
/// between the two of them the order is `jobs`, then `closing` (below).
/// Neither is ever held while `open`, a save lock or a map is taken. `open`
/// is the OUTERMOST lock — never taken while holding any other — and,
/// unlike the maps, it IS held across disk I/O by design (the sidecar read,
/// the store scan, the create, the pin and a discard's quiesce and removal
/// are exactly what it serializes). So an open can wait behind a discard
/// that holds `open` for as long as that discard's quiesce takes — up to
/// four bounded waits of 5 s each (`discard::quiesce`), about 20 s. No
/// holder of `open` other than a discard takes a save lock, and nothing a
/// discard's quiesce waits for takes `open`. Execute, snapshot, save and a
/// `keep`/`discardRecovery` close never take it. The maps are never held
/// across disk I/O.
///
/// `save_locks` (Task 12) is TWO DIFFERENT LOCKS wearing one field, and the
/// lock-order rule is different for each:
/// - **The per-session lock** — each `Arc<Mutex<()>>` the map's values
///   hold — is what `save_commands::session_save_lock`'s callers actually
///   care about: `save_project_with` holds it across its whole
///   read-the-revision → commit → `mark_saved` sequence, and
///   `session_close::close_in`'s `discardProject` holds the SAME one
///   across its unpin-then-remove sequence (fix round 2), so a save and a
///   discard on the SAME session can never interleave either. This lock is
///   NEVER taken while holding `by_project` or `sessions`; a discard takes
///   it while holding `open` (the order above), nothing else holding `open`
///   takes it, and it is never held while `open` is taken. `sessions` is
///   taken only BRIEFLY *inside* it, the same posture `open` has toward the
///   maps.
/// - **The map's OWN outer `Mutex` — `Mutex<HashMap<String,
///   Arc<Mutex<()>>>>` itself** — is a plain LEAF lock: `session_save_lock`
///   takes it only to look up or insert one entry and clone the `Arc` out,
///   and `session_close::drop_session` takes it (after releasing
///   `by_project` and `sessions`, final review M1) only to remove one
///   entry, so the map only grows with sessions currently open. Neither
///   ever does I/O or waits on another lock while holding it, so it MAY be
///   taken while `by_project`/`sessions` are held without violating the
///   rule above — that rule is about the per-session `Arc<Mutex<()>>`, not
///   the container around it.
///
/// `session_save_lock` also refuses `sessionGone` before ever touching
/// `save_locks` at all (fix round 2), so a save or discard on an unknown
/// session id leaves the map untouched rather than minting an entry
/// nothing would ever prune.
///
/// `jobs` (Task 25) is every background job this process started — the
/// authoritative record `editor_get_jobs` answers from, and the registry the
/// shutdown gate asks whether a render is still running (Task 46), bounded
/// per session (GAP-174). A LEAF lock like `save_locks`'
/// map: never held across I/O, a channel send, or while taking another
/// lock (`media_jobs.rs`' own doc).
///
/// `thumbnails` (Task 28 fix round 1) is every thumbnail render in flight,
/// by session, with its cancel flag — `media_derive::ThumbnailRenders`. A
/// leaf lock too, for the same reasons.
///
/// `caption_imports` (Task 36 fix round 1) is the sessions with a caption
/// import in flight (`caption_commands::claim_caption_import`), so a second
/// one is refused rather than opening a second dialog. A leaf lock: taken
/// only to insert or remove one id.
///
/// `relinks` (Task 40) is the sessions with a reconnect in flight
/// (`relink_commands`), the `caption_imports` posture: a leaf lock, taken
/// only to insert or remove one id.
///
/// `journal` (Task 37) is the recovery journal's per-session debounce
/// state (`recovery::JournalQueue`) — a leaf lock, taken only to schedule,
/// take or forget one entry. Every journal WRITE runs under the session's
/// save lock instead (`recovery.rs`' module doc).
///
/// `takes` (Task 49) is every webcam take begun and not yet discarded
/// (`webcam_registry::TakeRegistry`) — a leaf lock around per-take slots
/// whose own lock order is in `webcam_registry.rs`' module doc.
///
/// `closing` (final whole-branch review I1/C2) is the sessions a
/// `discardProject` is removing right now (`discard.rs`): every start path
/// refuses them. A leaf lock, taken inside `jobs` by `discard::mark_closing`
/// and `media_jobs::start_job_in` (order: `jobs`, then `closing`) and alone
/// everywhere else.
#[derive(Default)]
pub struct EditorState {
    pub open: Mutex<()>,
    pub sessions: Mutex<HashMap<String, EditorSession>>,
    pub by_project: Mutex<HashMap<String, String>>,
    pub save_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    pub jobs: Mutex<media_jobs::JobRegistry>,
    pub thumbnails: media_derive::ThumbnailRenders,
    pub caption_imports: Mutex<HashSet<String>>,
    pub relinks: Mutex<HashSet<String>>,
    pub journal: recovery::JournalQueue,
    pub takes: webcam_registry::TakeRegistry,
    pub closing: Mutex<HashSet<String>>,
    #[cfg(test)]
    pub test_hooks: TestHooks,
}

/// Test-only handshakes that let a race test wait for a thread to reach a
/// named point instead of sleeping and hoping it has.
#[cfg(test)]
#[derive(Default)]
pub struct TestHooks {
    /// Set by a session's `discardProject` immediately BEFORE it locks
    /// `open` (`session_close::close_in`).
    pub discard_waiting_for_open: std::sync::atomic::AtomicBool,
}
