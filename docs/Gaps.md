# Gaps

The audited backlog of known issues, weaknesses, improvement candidates,
tech debt, untested paths, and fragile edge cases. Produced by a
full-codebase audit on **2026-07-10 at v0.5.1** (six parallel audit passes:
core crate, capture + transcribe crates, Tauri shell, frontend + tests,
CI/build/docs, architecture verification). Every finding was verified
against the code at the cited location; line numbers are a snapshot of that
date and will drift.

How to use this file:

- **Before working in an area**, scan its section — the bug you just found
  may be catalogued, and its entry names the constraint a fix must respect.
- **When you fix an entry**, delete it (or move it to a short "fixed in
  vX.Y.Z" note in the PR description) and add a regression test naming the
  failure mode, per the repo's TDD convention.
- **When you find a new gap** you aren't fixing right now, add it here with
  the same shape: severity, location, failure scenario, remediation sketch.

Severity: **High** = plausible user-visible data loss, hang, security hole,
or a broken safety gate. **Medium** = real defect or design weakness with a
concrete failure scenario, bounded blast radius. **Low** = edge case,
polish, debt, or a documented-but-unenforced assumption.

Fixes to any entry must respect the invariants in
[AGENTS.md](../AGENTS.md) — several entries exist precisely because the
naive fix would violate one (noted inline).

## Contents

1. [Correctness & data safety (Rust)](#1-correctness--data-safety-rust)
2. [Main-thread responsiveness (shell)](#2-main-thread-responsiveness-shell)
3. [Robustness & swallowed errors](#3-robustness--swallowed-errors)
4. [Frontend defects & races](#4-frontend-defects--races)
5. [Security & configuration](#5-security--configuration)
6. [CI & release engineering](#6-ci--release-engineering)
7. [Untested paths](#7-untested-paths)
8. [Tech debt & duplication](#8-tech-debt--duplication)
9. [Documentation & repo hygiene](#9-documentation--repo-hygiene)
10. [Verified sound](#10-verified-sound)

---

## 1. Correctness & data safety (Rust)

### GAP-56 · Low · Search content cache: fill-to-cap tail and dead entries
`core/src/search_cache.rs`. The cache fills to 256 MiB then stops inserting
(no eviction — uniform per-search access makes LRU pointless), so once total
note text exceeds the cap the last-walked vaults' notes re-read on every search
(still far cheaper than the pre-cache path). Entries for deleted files also
linger until process exit, bounded by the cap. A per-walk mark-and-sweep and/or
a larger/tunable cap would address both; deferred as documented in the spec.

### GAP-55 · Low (mitigated) · A document dropped during an in-flight import
`src/components/ImportVaultPicker.vue` (`pick`) + `src/stores/vaults.ts`
(`begin_document_import` → `refresh()` re-arms `pendingImportPath`). If a
second document is dropped while the first conversion is still running,
`begin_document_import` re-points `pendingImportPath` to the new path.
Originally the first `pick()` then called `showList()` unconditionally,
clearing `pendingImportPath` and silently discarding the second drop.
**Mitigated** (polish pass): `pick()` snapshots the path it converts and only
`showList()`s if `pendingImportPath` still equals that snapshot — otherwise it
leaves the picker on the newly-dropped document, so the second drop survives
and the user just picks a vault for it. Still single-slot, so a THIRD drop
landing before the second is picked would overwrite the second; a full queue
is the only complete fix, but disproportionate for a narrow, non-destructive
window (imports are serialized by `ImportLock`; nothing is ever written for a
dropped-then-lost path). Surfaced by the Task 9 review.

### GAP-54 · Low · Document-import media publish has a non-atomic crash window
`src-tauri/core/src/document_import.rs` (`publish_inner`, the media
`rename` before the note `write_note_atomic`). Publishing moves the media
folder out of the staging dir first, then commits the note. If the process
is killed / loses power in that ~two-rename window, the media folder is
already published but no note exists, and `run_import_recovery` only sweeps
`.vault-buddy.tmp.import` staging dirs — not the published-but-unreferenced
media folder. Result: a stray media folder (our OWN extracted files — no
user data loss) that a later same-name import suffixes around (` (2)`).
**Accepted as a documented limitation** (comment at the site): a
crash-atomic fix needs two-phase commit across two filesystem objects
(unavailable) or a permanent per-import marker file in every media folder —
disproportionate to a microsecond window whose worst case is a cosmetic
leftover folder. **Fix, if ever pursued:** the staging dir name encodes the
basename and still exists on crash, so the janitor could parse it and remove
a matching `<basename>/` media folder that has no sibling `<basename>.md`
note (provably our orphan, since the basename comes from our owned staging
dir). A crash *inside* `write_note_atomic` (between temp-create and
`rename_noreplace`) can also strand a hidden `.<basename>.md.vault-buddy.tmp`
FILE next to the target — the import janitor sweeps only `.vault-buddy.tmp.import`
dirs, not this temp. It is our own tiny, walk-invisible file (no user data),
and the surface is shared by every domain that uses `write_note_atomic`
(capture/transcript/tasks), not import-specific.

### GAP-01 · ~~High~~ FIXED 2026-07-10 · Transcription retry/force paths accept `..` escapes and skip the capture-basename gate
`owning_vault_id` and `open_recording_note` now match on canonical paths via
`capture_paths::vault_owning_path` (unresolvable = rejected), and both
transcription commands require `capture_paths::is_capture_mp3` — the same
ownership filter `rename_plan` enforces (now shared).

### GAP-02 · ~~Medium~~ FIXED · A transient config read failure during save wiped every other vault's settings
`src-tauri/core/src/capture_config.rs` (`update_vault_config_at`,
`update_mcp_config_at`, `update_document_import_config_at`).
Previously any `read_to_string` error — not just NotFound — mapped to
`AppConfig::default()`, then `write_config` replaced the whole file with
only the edited section. A momentarily locked/unreadable `config.json`
(Windows AV, indexer) while saving one section silently dropped the others;
a voice-note vault could revert to Meeting mode, re-enabling desktop-audio
loopback — exactly the flip the per-field parser exists to prevent.
**Fixed:** all three read-modify-write update paths now go through
`load_config_for_update`, which defaults only on `ErrorKind::NotFound` and
propagates (aborts the save on) any other read error. Regression tests:
`update_aborts_on_a_non_missing_read_error`,
`update_defaults_and_saves_when_the_config_is_missing`.

### GAP-03 · ~~Medium~~ FIXED 2026-07-10 · Transcript ownership markers match anywhere in the file, not the frontmatter
`is_regenerable`, `needs_transcription`, and `transcript_status` now read the
marker via a frontmatter-scoped `note_field(content, "vault-buddy-transcript")`
reader; body text quoting a marker no longer reclassifies a sidecar.

### GAP-04 · ~~Medium~~ FIXED 2026-07-10 · Renaming a transcribed recording strands the transcript and silently re-transcribes
`rename::execute` now moves `<old>.transcript.md` via the same
`rename_noreplace` rails right after the mp3 and retargets the note's
`.transcript` embed; a transcript-side failure degrades to a warning and
keeps the old embed (audio first, never clobber).

### GAP-05 · ~~Medium~~ FIXED 2026-07-10 · System suspend mid-recording appends the whole sleep gap as encoded silence
The tick loop now runs a pure `plan_tick` policy: a wake >500 ms behind
schedule resyncs `next_tick` forward by up to the lag, capped to how much
real audio is currently buffered — near-zero after a suspend (the sources
were asleep too), so `next_tick` lands at ~`now + TICK` and the sleep gap
is never encoded as silence, exactly as before. (A real I/O stall instead
has a full buffer and gets full catch-up — see the Codex PR #46 fix noted
in session.rs.) A wake before schedule (pause/resume control message)
consumes nothing. Catch-up under 500 ms is unchanged (backpressure still
averages out).

### GAP-06 · ~~Medium~~ FIXED 2026-07-10 · Never-clobber degrades to a racy fallback on filesystems without hard links
On Windows the fallback is now MoveFileExW WITHOUT MOVEFILE_REPLACE_EXISTING
(natively non-replacing, no TOCTOU window); non-Windows keeps the guarded
rename (compile gate only, never shipped). Windows-arm execution arrives
with sub-pass D's Windows `cargo test` step (GAP-43).

### GAP-07 · ~~Medium~~ FIXED 2026-07-10 · `rename_capture` has no vault-containment check at all
The command now refuses paths outside every registered vault via the
canonical `capture_paths::vault_owning_path` (GAP-01's helper) before
planning the rename.

### GAP-08 · ~~Medium~~ FIXED 2026-07-10 · A wedged device open makes the app unquittable
The reservation now carries an explicit `startup_wedged` flag (set only in
the start-timeout branch); shutdown paths (`request_stop_and_wait(None)`,
`hide_buddy`, `quit`, CloseRequested) bypass the wait only when it is set
AND `part.is_none()` — nothing on disk. The janitor records a late worker's
`.part`, closing the bypass; recordings that reached disk keep the
wait-forever posture.

### GAP-09 · Low · Daily-note formats with literal words silently create misnamed notes
`src-tauri/core/src/daily_notes.rs:64-87` + `core/src/lib.rs:33-34`.
A format containing a literal word or moment `[...]` escapes (e.g.
`YYYY-MM-DD [Daily]`, common in Obsidian) hits the unsupported-letter-run
rule and falls back to the default format entirely; `daily_note_uri` then
finds no file at the default path and emits `obsidian://new`, so Obsidian
*creates* a note diverging from the user's scheme — the same class of harm
the fallback exists to avoid, just cleaner-looking.
**Fix:** support `[...]` literals (treat bracketed runs as verbatim),
and/or fall back to `obsidian://open` without a `file` parameter.

### GAP-10 · Low · Meeting-mode start is all-or-nothing while mid-recording loss is survivable
`src-tauri/capture/src/devices.rs:213-227` vs `session.rs:267`.
A loopback failure at start (`default_output_config()` / `build_stream`
error) aborts the whole recording even though the mic stream was fine,
whereas the same loopback dying mid-recording degrades to a warning and the
meeting continues.
**Fix:** degrade a loopback start failure to mic-only with a
`start_warning`, matching the mid-recording policy.

### GAP-11 · Low · Source-loss warnings clobber earlier warnings instead of appending
`src-tauri/capture/src/session.rs:247, 257`.
`warning = Some(msg)` overwrites a seeded `start_warning` (or the first
source's loss when both die), so "configured microphone not found" never
reaches the note if the loopback also drops. The write-error and
note-failure paths already append with `"{prior}; {msg}"`.
**Fix:** append with the same pattern.

### GAP-12 · Low · Per-chunk linear resample drifts on non-integral chunk ratios
`src-tauri/capture/src/mixer.rs:13-29` (called per device callback).
Fractional output samples are truncated at every chunk boundary — up to ~1
sample per callback of cumulative mic/loopback clock drift over a
multi-hour meeting, device-dependent (the common 480-frames@48 kHz case
divides exactly). The transcribe crate's `StreamingLinearResampler` was
built for exactly this defect class.
**Fix:** port the streaming resampler's carry logic into the capture mixer.

### GAP-13 · Low · Unbounded source channels can balloon RAM while the disk stalls
`src-tauri/capture/src/devices.rs:85-93` + `session.rs` (`BUFFER_CAP`).
`BUFFER_CAP` (2 s) bounds only the post-drain buffer; the mpsc channels are
unbounded. A worker blocked minutes in `write_all`/`flush` (AV scan,
network volume) accumulates ~350 KB/s/source in RAM, then everything beyond
2 s is dropped at the next drain anyway.
**Fix:** bounded channel (or drop-oldest in the callback) sized to the same
2 s cap.

### GAP-14 · Low · Cached whisper models are trusted without re-verification; torn finalize is permanent
`src-tauri/transcribe/src/model.rs:104-107, 222, 267-273`.
(a) `download_model` returns any existing `dest` unverified — a corrupt
model that still loads transcribes garbage forever. (b) Flush/fsync
failures during finalize only warn, then the rename proceeds; a torn
`ggml-*.bin` at the final path short-circuits every future download until
the load-failure → `remove_model` path fires.
**Fix:** treat `sync_all` failure as download failure (delete the `.part`);
optionally verify the cached file's SHA-256 once per app version.

### GAP-15 · Low · `bitrateKbps` wraps via `as u32` and has no range validation
`src-tauri/core/src/capture_config.rs:158-162`.
A hand-edited `4294967424` truncates to `128`; `0` or `999999999` pass
through to LAME untouched — "one malformed value defaults only itself" is
only half-true for this field (it mangles instead of defaulting).
**Fix:** `u32::try_from(..).ok()` + a sane range filter before
`unwrap_or(default)`.

### GAP-16 · Low · Case-sensitive extension checks are inconsistent with search
`src-tauri/core/src/tasks.rs:123/132`, `recordings.rs:49`,
`transcript.rs:302`.
Search deliberately treats any-case `.md` as a note (with a regression test
citing case-insensitive Windows filesystems), but the tasks scan requires
lowercase `.md` — a `Task.MD` file is invisible to the tasks list yet
surfaced by search — and the recordings/backfill scans require lowercase
`.mp3`.
**Fix:** one shared `eq_ignore_ascii_case` suffix check across the three
scanners.

### GAP-17 · Low · `tasks_folder: "."` turns the whole vault into the tasks root, read in full
`src-tauri/core/src/capture_paths.rs:170-173` + `tasks.rs:126`.
`safe_recording_root` accepts `Component::CurDir`, so `"."` makes the vault
root the tasks root and `list_tasks` recursively reads *every* markdown
file to completion (no analogue of search's 1 MiB cap) on each tasks-view
open. A performance trap, not a containment violation. The lists increment
extends the blast radius cosmetically: `task_lists` enumeration would offer
EVERY vault folder as a List in pickers and the Lists grouping (dirs-only
scan, no content reads — cheap, just noisy).
**Fix:** reject folders normalizing to empty; read only the frontmatter
head in `collect_task_file`.

### GAP-18 · Low · `process.rs` counts any "obsidian <word>" process as Obsidian
`src-tauri/core/src/process.rs:24`.
`starts_with("obsidian ")` matches a third-party tool running as
`obsidian sync` (space, not hyphen), keeping stale "Open now" flags alive —
the exact failure the delimiter rule exists for; only hyphenated names are
tested.
**Fix:** restrict the space-delimited form to known helper shapes
(`obsidian helper*`) or anchor on exact executable names per platform.

### GAP-19 · Low · Assorted small edges in core
- `core/src/uri.rs:63` — `to_string_lossy` turns non-UTF-8 names into
  U+FFFD (Obsidian silently fails to resolve); the blanket `'\\'→'/'`
  rewrite corrupts legitimate backslashes on Unix. Harmless on the Windows
  target; return `None` for non-UTF-8 and gate the replace to Windows.
- `core/src/capture_paths.rs:65` — `MAX_TITLE_CHARS = 120` *chars* can
  exceed 255 filename units with astral characters (120 non-BMP chars =
  240 UTF-16 units + prefix + `.transcript.md` > 255); also cap by encoded
  length.
- `core/src/checkpoint.rs:19` — `BASELINE_TICKS = 3` is a time-based proxy
  for "window-state restore landed"; a >3 s restore (cold login) persists
  the pre-restore default position — the very poisoning the constant
  guards. Have the shell signal restore-completion instead.
- `core/src/transcript.rs:261-267` — `replace_if_ours` has a milliseconds
  check-then-replace TOCTOU (user edit between marker read and rename is
  clobbered); worth a comment plus an mtime re-check at most.
- `core/src/search.rs:133-137` — `truncated` can report `100+` when
  exactly 100 matches exist and no remaining vault has hits.
- `core/src/capture_config.rs:288-293` — `write_config`'s temp uses
  truncating `File::create` on a fixed predictable name (would follow a
  planted symlink), unlike the exclusive-create discipline of the note
  writers; app-side dir + `ConfigWriteLock` make it low risk, but it's one
  refactor away from being copied into a vault path. Reuse
  `write_atomic_replacing`.

### GAP-61 · Low · `recording_roots` dedup is lexical, not canonical
`src-tauri/core/src/vault_config.rs` (`normalize_folder` + `recording_roots`).
The dedup compares lexically-normalized folder paths (splitting on `/` and
`\`, dropping empty and `.` components), which catches hand-edit collisions
like `"Audio"` vs `"Audio/"` vs `"Audio/."`, but skips symlink/junction
aliasing and case-insensitive filesystem aliasing — two DISTINCT configured
folders that resolve to the same directory via either mechanism will
double-scan and duplicate in the Recordings browser. Failure scenario: a user
with symlink-aliased `meetingFolder` and `voiceNoteFolder` (rare, low user
count). Blast radius: Recordings browser only (recovery is idempotent, the
transcription queue dedups by path). **Fix:** a caller-side canonical dedup
(after `canonicalize` the nearest-existing ancestor per AGENTS.md containment
discipline) would be the full fix; deferred as a low-frequency edge.

### GAP-60 · Low · `set_capture_config`/`set_documents_config`'s preserve-vs-write field split has no direct Rust test
`src-tauri/src/capture_config_commands.rs` (`set_capture_config`) and
`src-tauri/src/document_commands.rs` (`set_documents_config`). Each command
owns a subset of `VaultCaptureConfig`'s fields and must carry the rest
forward from the existing config (read under the lock) so the OTHER
command's settings survive. `set_capture_config` builds a whole new struct
literal: it writes its own fields (mode, both folders, bitrate, devices,
transcription, and now `recording_date_folders`) while copying
`existing.tasks_folder` / `existing.documents_folder` / `existing.default_list`
/ `existing.list_order` — and, as of this branch, `existing.document_date_folders`
— verbatim. `set_documents_config` instead mutates a full copy of the
existing config in place, touching only `documents_folder`/
`document_date_folders` and leaving `recording_date_folders` (and everything
else) preserved by omission. Neither shape is checked by a test: nothing
asserts that a capture-settings save leaves `document_date_folders` alone,
or that a documents-settings save leaves `recording_date_folders` alone —
both are plain `bool`s, so a misassigned field on either side (e.g. the
`recording_date_folders:`/`document_date_folders:` pair in
`set_capture_config` transposed) would compile cleanly and pass every
existing test. Failure scenario: such a mistake ships, and a user's
Documents layout choice is silently reset by their next Recording settings
save (or vice versa), with no failing test to catch it before release.
**Not a new class of gap** — `set_capture_config`'s preservation of
`tasks_folder` (owned by `set_tasks_config`), `documents_folder` (owned by
`set_documents_config`), and `default_list`/`list_order` (owned by
`set_task_lists_config`) already carried the same untested-merge risk;
`recording_date_folders`/`document_date_folders` are just the newest fields
to join it. **Fix:** extract each command's preserve-vs-write merge into a
plain-Rust helper in `vault_config.rs` (taking the existing config plus the
fields the command owns) and unit-test it directly there, instead of
relying on a `#[tauri::command]`/`tauri::State` signature to keep the logic
out of reach of the core crate's test suite.

## 2. Main-thread responsiveness (shell)

Sync commands run on the main thread (an AGENTS.md invariant — window APIs
need it), which means **long work in a sync command freezes window
show/hide, drags, and the upkeep tick**. Fixes must not move
window-touching code off the main thread.

### GAP-20 · ~~High~~ FIXED 2026-07-10 · `stop_capture` blocks the main thread for up to 15 s
Now an async command: the condvar wait runs under `spawn_blocking`, and the
15 s expiry returns a typed `{ stillSaving: true }` instead of a bare Ok —
the store keeps its saving UI and the capture events finish the story.
`request_stop_and_wait` returns `StopWait` so no caller can misread a
timeout as success.

### GAP-21 · ~~High~~ FIXED 2026-07-10 · `start_capture` blocks the main thread for up to 10 s
Now an async command: the whole start body (device-ready wait included)
runs under `spawn_blocking` with reservation semantics unchanged; the
buddy-show indicator tail is marshalled back to the main thread
(window show is main-thread-only).

### GAP-22 · ~~Medium~~ FIXED 2026-07-10 · Read-only list commands do unbounded filesystem/device work on the main thread
`list_recordings`, `list_tasks`, `count_open_tasks`, and
`list_audio_devices` are async now, each wrapping its filesystem/COM work
in `spawn_blocking` (the `search_vaults` precedent); a panicked task
degrades to the empty value each already used, with a warn.

## 3. Robustness & swallowed errors

The repo's own invariant: *no swallowed error* — anything caught-and-hidden
goes through `log::warn!`/`log::error!`. These sites violate it.

### GAP-23 · ~~Medium~~ FIXED 2026-07-10 · Silent `Ok`-with-empty on unreadable single-file configs
All six arms (`discovery`, `capture_config::load_config_from`,
`daily_notes::load_settings`, `app_diagnostics::check_previous_run`,
`transcript::needs_transcription`/`transcript_status`) now `log::warn!` on
any read error other than NotFound; return values still degrade unchanged.

### GAP-24 · ~~Medium~~ FIXED 2026-07-10 · `.expect` on thread spawn inside main-thread native callbacks
All eight sites (close-finalize, shutdown-finalize, tray-stop, and the five
start_capture spawns) now log-and-degrade per site instead of panicking
across the WebView2 FFI boundary; the setup-time spawns (recovery,
transcribe-worker, topmost-checkpoint) were never in a native callback and
keep `.expect`.

### GAP-25 · Low · Assorted swallowed results
- `src-tauri/src/diagnostics.rs:85-87, 99-101` — run-marker
  heartbeat/rearm failures use `let _ =`; a persistently failing heartbeat
  silently degrades crash detection. Log once, latched.
- `capture/src/session.rs:341` — the ~30 s `sync_data` result is
  discarded; a dying disk leaves the durability cadence silently dead. Log
  latched.
- `capture/src/recovery.rs:110-111, 138` — a locked/undeletable empty
  `.part` is reported as `DeletedEmpty` anyway (and retried forever,
  unlogged); the recovered-note write result is `let _ =`. Log both; only
  report `DeletedEmpty` on success.
- `transcribe/src/decode.rs:72` — `ResetRequired => break` silently
  truncates decoding; a partial transcript looks complete. Log a warning.
- `transcription.rs:314-316` — a queued job whose vault disabled
  `transcribe` mid-queue returns early with **no terminal event**; a UI
  that saw the job in `transcription_queue_status` never learns its fate
  (polling self-corrects). Emit `capture:transcribeSkipped`.

### GAP-26 · ~~Low~~ FIXED 2026-07-10 · Inconsistent error strings; paths leak into user-facing errors
The four hand-rolled `discovery::discover_vaults().into_iter().find(|v| v.id
== id)` lookups in `capture_commands.rs` (`set_capture_config`,
`start_capture_blocking`) and `task_commands.rs` (`set_tasks_config`,
`tasks_root_for`) now delegate to `crate::commands::find_vault` — the same
`services::find_vault` user-worded copy the panel and MCP already share, so
there is exactly one vault-not-found message left. The user-facing errors
that embedded absolute local paths (`start_capture_blocking`'s vault-folder
check, `open_recording_note`'s outside-its-vault error in
`capture_commands.rs`, `open_task`'s outside-its-vault error in
`task_commands.rs`) now log the path via `log::warn!` and return a
path-free, user-worded `Err`. `services::find_vault` itself (the MCP
contract) was left untouched — out of this pass's scope. The `add_task`
vault-folder check in `core::services` (initially left for the same reason)
was closed out in a later pass: it now logs the path via `log::warn!` and
returns the same path-free copy as `start_capture_blocking`.

## 4. Frontend defects & races

### GAP-58 · ~~Medium~~ FIXED 2026-07-11 · SelectMenu dismissed itself on ANY scroll — its own option list was unreachable
User-reported on the All-tasks vault picker: the capture-phase `window`
scroll listener closed the menu on every scroll event, including the
popup's own `overflow-y-auto` option list, so with more than a handful of
entries the lower options could not be reached by wheel or scrollbar.
Scrolls inside the popup are now ignored (navigation, not dismissal) and
outside scrolls re-anchor the position:fixed popup to its trigger via
`positionPopup` instead of closing; pointerdown-outside and Escape
dismissal are unchanged. Regression tests pin all three behaviors.

### GAP-59 · Low · Lists/ordering increment residuals (accepted)
- Aggregate mode fetches `get_tasks_config` lazily only for the composer's
  target vault, so the inline editor's list picker orders OTHER vaults'
  lists alphabetically (no `listOrder`) — cosmetic; sections in the
  aggregate are deliberately alphabetical anyway.
- Keyboard reordering writes one `order` rank per Arrow press — chatty
  across a long travel (each write is a small fsync'd surgical edit);
  batching on settle would be a polish item.
- A task's manual rank is global, not per-list: reordering a task inside
  one section also moves it relative to tasks in other sections when they
  meet in a different grouping. By design (one `order` field per task),
  documented in the spec.
- A row write that was already in flight when a drag STARTED (e.g. a slow
  toggle that then FAILS) can resolve mid-drag: its revert re-sorts the
  rows, and the drop then commits from/to indices captured against the
  pre-shuffle order, moving a neighbor instead. Needs a failing write
  racing a sub-second drag; the reorder-vs-reorder and write-vs-write
  interleavings are already blocked by the `reordering` guard and the
  per-path busy checks (the busy row's grip is inert), so only this
  revert-reshuffle window remains.

### GAP-63 · Low · Task-ID / lists-first / drag-default increment residuals (accepted)
- Renaming a vault's `task_id_property` (or turning IDs on again after
  turning them off) leaves every already-stamped Task's ID under the OLD
  property name in place. `update_task_fields`'s `ensure_absent` only
  checks/writes the CURRENTLY configured property — it never migrates,
  renames, or removes a stale one — so a vault that changes its property
  name ends up with two differently-named ID properties split across
  old vs. newly-edited Tasks. By design (an edit-time stamp must never mass-
  rewrite the vault to chase a config change); a manual find-and-replace
  across the vault's Tasks folder is the user's escape hatch if this ever
  matters to them.
- Aggregate mode (`vaultId: null`) has no "＋ List" toolbar control —
  `TaskViewControls.vue` gates it on `grouping === 'lists' && !isAggregate`,
  because creating a list needs one target vault and the aggregate view
  spans all of them. Not a regression: the composer's own
  `TaskListPicker.vue` still offers a per-target-vault "New list…" once a
  vault is picked, so aggregate users aren't blocked — only the toolbar
  shortcut is per-vault-only. Wiring a vault-picker into the toolbar's
  control too was judged not worth the complexity this slice.
- A drag-drop reorder that materializes ranks across a whole section
  (`utils/taskOrder.ts`) writes one `update_task` call per affected Task.
  When Task IDs are enabled, `update_task`'s stamp-if-absent check runs on
  EVERY one of those calls, so a single reorder can generate and stamp
  several new IDs at once — one per previously-un-ID'd neighbor the drop
  happens to re-rank. Not a bug (each stamp still only fires when that
  Task's ID line is absent, and the reorder would have touched that Task's
  frontmatter anyway), just a side effect worth knowing before enabling IDs
  on a vault with a lot of pre-existing, never-edited Tasks — the first
  reorder that sweeps through them will stamp the whole batch in one go
  rather than one at a time as each is later hand-edited.

### GAP-27 · ~~Medium~~ FIXED 2026-07-10 · Escape in an open dropdown also closes the whole panel
`onPopupKeydown`'s Escape branch now calls `e.stopPropagation()` before
`closeMenu()`, matching Search's handler; a regression test opens the popup,
dispatches Escape on it, and asserts a `window` keydown listener is never
called.

### GAP-28 · ~~Medium~~ FIXED 2026-07-10 · A slow quiet update check can stomp a manual check or install
`checkForUpdatesQuietly` now re-checks `phase === "idle"` after its
`await check()` and discards the stale result otherwise, so it can never
flip `phase`/`available` under a manual check or a mid-flight install.

### GAP-29 · ~~Medium~~ FIXED 2026-07-10 · The rename prompt is unreachable for saves that happen while the panel is closed
The store now stamps `lastSavedAtMs` on `capture:saved`; the `shownNonce`
watcher calls the new `dismissRenameIfStale()` instead of an unconditional
`dismissRename()`, so a prompt younger than `RENAME_PROMPT_MS` survives a
reopen.

### GAP-30 · ~~Medium~~ FIXED 2026-07-10 · After a failed config read, one transcription toggle rewrites the vault's capture config to defaults
`loaded` now flips only inside the try block (success path); a failed read
leaves it false, so no toggle persists the default-seeded config. The
failure is logged via `logWarning`.

### GAP-31 · ~~Medium~~ FIXED 2026-07-10 · IME-composition guards on the add-task Enter and filter Escape
Added `onTitleEnter` handler in Tasks.vue and early isComposing return in ActionPanel's `onFilterEscape` — both now follow Search's precedent.

### GAP-32 · Low · Assorted store/component edges
- `src/stores/capture.ts:234-241` — `refreshWaitingForRecording` responses
  are unticketed; a slow response can re-set stale state after a newer
  event cleared it (self-corrects on the next event). Add a ticket or
  ignore when `activeTranscription` is set.
- `src/stores/vaults.ts:81-101` — `taskCounts` refreshes only on panel
  open, so the vault-row badge is stale after task edits until reopen.
  Refresh from `back()`/Tasks mutations. (FIXED 2026-07-10 — added
  `refreshTaskCount(id)`, called from Tasks.vue on toggle/archive/add
  success, plus a full `loadTaskCounts()` from `back()` when leaving the
  tasks view.)
- `src/components/Tasks.vue:84-86, 98-104` — failed-toggle revert forges
  `status: "new"` instead of restoring the original (`in-progress` etc.);
  the failure re-insert uses a pre-await index, restoring one slot off
  after a concurrent add. Capture the original status; recompute the index.
  (FIXED 2026-07-10 — toggle now captures `prevStatus` before the
  optimistic flip and restores it verbatim on failure; archive's failure
  path pushes the removed task back and re-sorts instead of trusting a
  captured index.)
- `src/stores/capture.ts:242-430` — `init()` registers 14 listeners with
  no re-entry guard or unlisten storage (safe today; double-init
  double-fires everything). Roots assign `unlisten*` only after `await
  listen(...)`, leaking a listener if unmount races registration. Add an
  `initialized` flag / post-await unmount check.
- `src/stores/notifications.ts:20-26` — dedupe reuses the newest identical
  toast without extending its TTL (a re-raise at t=3.9 s vanishes at 4.0 s
  and reads as flicker); dismissed ids' timers still fire. Restart the
  timer on dedupe-reuse. (FIXED 2026-07-10 — a `timers` map keyed by
  notification id lets dedupe-reuse `clearTimeout`+restart the TTL, and
  `dismiss`/`clear` now cancel their timer instead of leaving it to fire a
  no-op later.)
- `src/stores/vaults.ts:184-195` — `back()` carries duplicated dead
  branches; nothing enforces valid view+vaultId pairs (a null-id
  `captureSettings` renders the list under the wrong header) — unreachable
  today, unguarded. Collapse `back()`; consider one view+id state field.
- `src/types.ts:81` — `TranscriptionQueueStatus.active.progress` is typed
  non-nullable `number` while `capture.ts:63` defends with `!= null`; one
  of them is wrong. Make it `number | null` to match the defensive read.

### GAP-33 · ~~Low~~ FIXED 2026-07-10 · Accessibility gaps in the two listbox surfaces
- `src/components/SelectMenu.vue` — options now carry `optionId(i)` ids, the
  listbox binds `aria-activedescendant` to the highlighted option, keyboard
  moves (`ArrowUp`/`ArrowDown`/`Home`/`End`) call `setActive` which
  `scrollIntoView`s the option (pointermove keeps the bare assignment so
  hover can't fight keyboard scrolling); keyboard-path tests pin
  activedescendant tracking and Home/End.
- `src/components/Search.vue` — `aria-expanded` now binds to
  `visibleHits.length > 0` instead of a static `"true"`, plus
  `aria-autocomplete="list"`.

## 5. Security & configuration

### GAP-34 · ~~Medium~~ FIXED 2026-07-10 · CSP is disabled for all three webviews
`src-tauri/tauri.conf.json:56` — CSP is now `"default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:"` (plus `connect-src ipc: http://ipc.localhost` per Tauri's CSP guidance; note Tauri does NOT auto-append origins — on WebView2 `invoke()` rides `window.chrome.webview.postMessage`, which CSP doesn't police, so the connect-src entries cover the wry `ipc:` scheme and any fetch-based transport; adopting the `asset:` protocol later would need explicit `asset: http://asset.localhost` additions). The policy mitigates injection attacks from strings rendered from vault contents (search results, note titles). Linux compile gate (`npx tauri build --no-bundle`) green. **Runtime behavior in the packaged WebView2 app is NOT yet verified — the next Windows-checklist run must confirm all three windows render (buddy sprites, panel styles, bubble) and the updater/settings views work; a breakage is a one-line revert of this commit.**

### GAP-35 · ~~Medium~~ FIXED 2026-07-10 · GitHub Actions pinned by mutable tag, including the one that holds the updater signing key
All three workflows: `actions/checkout@v4`, `actions/setup-node@v4`,
`Swatinem/rust-cache@v2`, `actions/upload-artifact@v4`,
`tauri-apps/tauri-action@v0` (a floating major-0), and
`dtolnay/rust-toolchain@stable` (a moving branch ref). The release workflow
feeds `TAURI_SIGNING_PRIVATE_KEY` into `tauri-action` — a compromised tag
on that action exfiltrates the key that can ship updates to every user.
**Fix:** pin all third-party actions to full commit SHAs.
(FIXED 2026-07-10 — all 22 `uses:` lines across `ci.yml`, `release.yml`,
and `bump-version.yml` now pin a full commit SHA resolved live via
`git ls-remote`, with the original tag/branch kept as a trailing comment;
`dtolnay/rust-toolchain@stable` pins the branch head with a dated comment
since it has no tag to fall back to.)

### GAP-36 · ~~Medium~~ FIXED 2026-07-10 · CI exposes the signing secrets to same-repo PR branch builds; no `permissions:` block
`ci.yml` now has a top-level `permissions: contents: read` block and the `windows-app` job's signing env is empty on all PR events (only populated on push) — the existing keyless fallback builds unsigned artifacts for every PR.

### GAP-37 · ~~Medium~~ FIXED 2026-07-10 · `bump-version.yml` interpolates the dispatch input into shell
The input and the ref-name error path now travel via `env:` (REQUESTED_VERSION,
REF_NAME) and are quoted in the run line; downstream steps already used the
script's resolved version.

### GAP-38 · Low · Capability breadth vs its own comment
`src-tauri/capabilities/default.json` — the description claims "no
core:window IPC grants are needed", but `core:default` bundles
`core:window:default` (and event/webview/tray defaults) for all three
windows. Modest breadth, wider than documented.
**Fix:** replace `core:default` with the minimal set actually used, or fix
the comment. (No `pull_request_target` anywhere; updater endpoint + pinned
pubkey are sound.)

### GAP-39 · Low · Platform-gate divergences that evaporate documented protections off-Windows
`commands.rs:123-129` (`start_buddy_drag` skips the stale-button re-check
on non-Windows), `lib.rs:616-618` (upkeep tick skips the held-button gate),
`diagnostics.rs:236-237` (dead `target_os = "macos"` branch). Acceptable
while Linux is compile-gate-only; two documented protections silently
vanish if that changes.
**Fix:** comment the divergence at the call sites; delete the dead branch.

## 6. CI & release engineering

### GAP-40 · ~~High~~ FIXED 2026-07-10 · The shell crate's unit tests never ran in any CI job
The `linux-app` job now runs `cargo test -p vault-buddy --lib` (and
workspace clippy) after the tauri build produces the `dist/` that
`generate_context!` embeds. Kept as a tombstone because the constraint is
non-obvious: the shell's tests cannot move to `rust-core` — they need the
WebView/GTK system libs and a built `dist/`.

### GAP-41 · ~~High~~ FIXED 2026-07-10 · The release dispatch path is unvalidated
A new `validate` job in `.github/workflows/release.yml` now rejects a
`workflow_dispatch` off any branch but `main`, checks
`inputs.tag == "v" + tauri.conf.json version`, and — for BOTH trigger
paths — requires the released SHA to be an ancestor of `main` via the
compare API (`identical`/`behind`), closing the hole where a v* tag pushed
on a non-main commit with a matching version and green PR-branch CI would
publish that branch's code (found by Codex on PR #46). Kept as a
tombstone because the workflow can't be exercised locally — the job only
proves itself out on the next real release dispatch.

### GAP-42 · ~~Medium~~ FIXED 2026-07-10 · A release can ship from a red commit
The same `validate` job (`.github/workflows/release.yml`) now queries
`gh run list` for the CI workflow's conclusion on `github.sha` and fails
closed (including on an API error) unless the most recent completed run is
`success`; `windows-installer` gained `needs: validate`. Kept as a
tombstone for the same reason as GAP-41 — untestable outside a real
dispatch/tag push.

### GAP-43 · ~~Medium~~ FIXED 2026-07-10 · No Rust tests run on Windows
The workspace-clippy half is fixed: `linux-app` now runs
`cargo clippy --workspace --all-targets -- -D warnings`, covering the
shell. The test half is fixed: `windows-app` now runs `cargo test` for
core, capture, and transcribe (including `--features whisper`) after the
build step, so the most platform-sensitive code (process detection,
`GetKeyState`, WASAPI loopback gates, MoveFileExW's non-replacing fallback,
whisper on MSVC) executes in CI for the first time — including the GAP-06
`cfg(windows)` MoveFileExW contract test and the GAP-08 startup-wedge
predicate.

### GAP-44 · Low · Release/bump edges
- ~~No CI job runs `node scripts/bump-version.mjs --check`~~ — fixed
  2026-07-10: the `frontend` job runs it before the build.
- `scripts/bump-version.mjs:107-110` — accepts a new version equal to or
  lower than current; equal input later fails at `git commit` with a
  confusing "nothing to commit". Reject `newVersion <= current`. (FIXED 2026-07-10 — resolveNewVersion rejects X.Y.Z <= current with a message naming both)
- ~~No `cargo audit` step~~ — fixed 2026-07-10: `cargo deny check`
  (advisories + licenses + sources, `src-tauri/deny.toml`) runs in
  `rust-core`. Still open: no `npm audit` step and no Dependabot/Renovate
  config, despite deliberate pins (whisper-rs 0.16) that need a tracked
  upgrade path.
- No SECURITY.md / key-rotation procedure for the updater keypair
  ("whoever holds it can ship updates to every user" — DEVELOPMENT.md) and
  no CHANGELOG (release bodies are boilerplate install instructions).

### GAP-62 · ~~Low~~ FIXED 2026-07-16 · `services.rs` outgrew its LOC-baseline ceiling without a baseline update
`scripts/loc-baseline.json` grandfathered `src-tauri/core/src/services.rs` at
927 nonblank lines (shrink-only, per `scripts/check-loc.mjs`'s policy); the
file reached 984 lines when the task-id increment's `add_task` id generation
(+ its two service tests) landed, so `npm run check:loc` — part of the
documented frontend gate chain (AGENTS.md § Commands) — failed on every PR
regardless of what it touched. **Fixed** by ratcheting the baseline entry
927→984 with a justified reason string (the sanctioned first branch of this
entry's own fix criterion) in the same commit that documents the task-id
feature; `check:loc` passes again. The file's standing "splitting it into
per-domain modules is a separate refactor" note remains open as future work,
but no longer red-lines CI.

## 7. Untested paths

What has no automated coverage today, by area. (The Vitest suite and the
core/capture/transcribe crates are otherwise well covered — see §10.)

**Core crate**
- `vault_walk.rs` has no test module of its own — cycle-set re-entry,
  unreadable-dir skip, and canonicalize-failure branches are exercised only
  indirectly via tasks/search tests.
- `capture_paths.rs`: `rename_noreplace`'s link-succeeded-but-remove-failed
  warn path; `assert_root_inside_vault` with a missing vault path. (The
  GAP-06 non-decisive-error fallback itself is no longer untested: the
  non-Windows arm has direct contract tests, and the `cfg(windows)` twin
  now executes on the Windows CI runner, fixed 2026-07-10.)
- `capture_note.rs`: `write_atomic_replacing`'s numbered-temp squatter path
  and failure-cleanup branch (only `write_note_atomic`'s squatter is
  tested).
- `capture_config.rs`: `write_config`'s rename-failure temp cleanup, the
  `update_vault_config` wrapper (the GAP-02 path), oversized/zero bitrate.
- `transcript.rs`: unreadable-sidecar arms; `replace_if_ours` error
  propagation.
- `recordings.rs`: `split_base`'s empty-title fallback (reachable —
  whitespace label passes `is_capture_base`).
- `search.rs`: the thread-spawn-failure inline fallback; the GAP-19
  truncated edge. `tasks.rs`: non-ASCII slugify (`"Café"` → `"caf"`),
  duplicate `status:` lines, nonexistent root. `process.rs`: the GAP-18
  space-delimited false positive.

**Capture / transcribe crates**
- `devices.rs`: only "never panics" smoke tests run in CI. The Windows
  `cargo test` step (2026-07-10) now compiles and smoke-runs the
  format-dispatch arms and the `#[cfg(windows)]` loopback block on the
  Windows runner, but hosted Windows runners are device-less — so that code,
  and the error-callback → `Lost` path, is still never *executed against a
  real audio device* by any test.
- `session.rs`: mid-recording encode/write/flush failure and best-effort
  finalize; `plan_tick` (GAP-05) is unit-tested but the suspend path itself
  cannot be exercised end-to-end (`Instant` is unmockable) — the loop
  wiring is reviewed, not tested.
- `engine.rs`: the FFI trampoline regression tests do run (Linux CI,
  `--features whisper`); the real-model end-to-end test is `#[ignore]`
  (manual). `model.rs`/`decode.rs` have excellent hermetic coverage
  (localhost stall/truncation/checksum/cancel servers); HTTP redirect
  handling is the one untested link (delegated to ureq).

**Shell crate**
- The 11 unit tests now run in the `linux-app` CI job (GAP-40, fixed
  2026-07-10). Everything window-/thread-/event-related (focus-out check,
  pin, metronome backpressure, drag guard, tray, hide chokepoint) remains
  manually verified per the Windows checklists in
  `docs/superpowers/specs/` only.

**Frontend**
- `src/main.ts` — the Vue `errorHandler` and unexpected-label fallback are
  untested (only `rootFor` is).
- `UpdateSettings.vue` is tested only indirectly through
  `buddy-settings.test.ts`; `HighlightText.vue` only via the util's tests.
- `SelectMenu.vue`'s tests now cover the keyboard path (GAP-33, fixed
  2026-07-10) but not outside-click close or positioning.
- Event-listener cleanup paths in the roots and `capture.init()` re-entry
  (GAP-32) have no tests.

## 8. Tech debt & duplication

### GAP-45 · Shell
- `start_capture_blocking` (the async command's moved body, sub-pass B) is
  ~330 lines with four inline thread bodies (`capture_commands.rs:321-655`);
  `process_transcription` ~186 lines.
- ~~The `discover_vaults().find(|v| v.id == id)` lookup is duplicated 6×
  across three files with two error styles~~ FIXED 2026-07-10 — the four
  shell-side lookups now delegate to `commands::find_vault` (GAP-26).
- The roots loop (`recording_roots` → `safe_recording_root` →
  `assert_root_inside_vault`) appears 3× (`list_recordings`,
  `run_recovery`, `scan_and_enqueue`); the owning-vault-by-prefix matcher
  is duplicated (`transcription.rs:584` vs `capture_commands.rs:979` —
  both carriers of GAP-01's defect).
- Inline magic numbers: 120 ms focus settle, 500 ms greeting settle,
  10 s/15 s/30 s/90 s waits, 960 level divisor — name them.
- Task/tasks-config writes (`set_tasks_config`, `add_task`,
  `set_task_status`) log nothing on success, unlike `set_capture_config`
  and `set_autostart`; add the audit-trail `log::info!` lines.

### GAP-46 · Core & capture
- The `YYYY/MM` triple-nested dated scan is copy-pasted between
  `recordings.rs:37-59` and `transcript.rs:290-313`; the exclusive-create
  temp-name loop is duplicated verbatim inside `capture_note.rs`. Extract
  `for_each_dated_capture_mp3` and a shared temp-open helper — the repo's
  own `vault_walk.rs` header warns about exactly this drift class.

### GAP-47 · Frontend
- ~~Inline SVG icon paths are copy-pasted (the identical gear in
  `ActionPanel.vue` and `VaultList.vue`; X-marks in three components) — a
  tiny `<Icon name>` component would end it.~~ PARTIALLY FIXED 2026-07-10
  (polish sub-pass E) — `AppIcon.vue` now wraps the standard stroked
  line-icon `<svg>`; the ActionPanel/VaultList icon buttons were converted,
  dissolving both fallow clone groups (cloneGroups 3→1, duplicatedLines
  100→22). The X-marks in other components can adopt `AppIcon` on next
  touch; two non-standard VaultList icons (omitting `stroke-linejoin`) were
  left raw to keep that pass move-only.
- `Search.vue` (494 LOC) and `stores/capture.ts` (~646 LOC) are the two
  oversized files; split when next touched. (The `tasks.rs` and `Tasks.vue`
  LOC-allowlist split obligations were both retired in polish sub-pass E:
  `tasks.rs`→`doc/parse/writer/list/disk` modules, `Tasks.vue`→
  `TaskRow`/`TaskEditor`/`TaskComposer`.)
- `transcribe_recording_now` is registered but never invoked from the
  frontend — `Recordings.vue:92-101` routes *all* retries (including plain
  `failed` rows) through force `retranscribe`, which bypasses the vault's
  `transcribe` gate. Either wire the gate-respecting retry for non-complete
  rows or delete the command; today it is dead IPC surface plus a behavior
  drift from the documented design.
- Three unused exports flagged by the fallow ratchet baseline
  (`scripts/quality-baseline.json`, deadCodeIssues=3):
  `RENAME_PROMPT_MS` (`src/stores/capture.ts`), `Notification`
  (`src/stores/notifications.ts`), `UpdatePhase` (`src/stores/updates.ts`).
  Inline or de-export them and re-lock the baseline.
- ~~`tsconfig.json` lists `"node"` types but `@types/node` is not a
  devDependency~~ — fixed 2026-07-10: `@types/node` added explicitly with
  the quality-pipeline adoption.

### GAP-51 · Low · minimp3 (dev-only test decoder) carries an ignored RustSec vulnerability
`src-tauri/capture/Cargo.toml` (dev-dependencies) + `src-tauri/deny.toml`.
`minimp3` pulls `slice-ring-buffer` 0.3.4 (RUSTSEC-2025-0044, multiple
double-frees via safe APIs). It is compiled only into the capture crate's
tests, never into a shipped binary, so the advisory is ignored in
`deny.toml` with that justification — but the ignore should not live
forever.
**Fix:** decode test MP3s with Symphonia (already a workspace dependency
via the transcribe crate) and drop minimp3; then remove the `deny.toml`
ignore entry.

### GAP-52 · Low · LGPL-3.0 static-linking compliance for the LAME encoder is undocumented
`src-tauri/capture/Cargo.toml` (`mp3lame-encoder`/`mp3lame-sys`,
LGPL-3.0) — the one copyleft production dependency, allowed in
`deny.toml`. `mp3lame-sys` builds and statically links libmp3lame;
LGPL §4(d) requires that users be able to relink against a modified
library (or that object files be made available), which a statically
linked proprietary-licensed binary must arrange for and this repo does not
document (the repo's own LICENSE applies to the app). Distribution is
public GitHub releases, so exposure is real but low.
**Fix:** document the combination in a NOTICE/README section (source is
public, which satisfies the spirit), or switch the LAME linkage to dynamic
on Windows, or state the app's own license terms in a way compatible with
LGPL static linking.

### GAP-48 · Accepted debt (tracked, no action now)
- `whisper-rs` pinned at 0.16 with hand-wired abort/progress trampolines
  around upstream bugs (documented in DEVELOPMENT.md); upgrade is its own
  tracked change.
- Two documented, bounded leaks: whisper-rs `set_language` leaks a few
  bytes of `CString` per job; `call_with_timeout` abandons its named
  download thread on timeout (OS reclaims the socket).
- Linear 44.1→16 kHz resample without an anti-aliasing low-pass
  (`decode.rs` header notes rubato as the future upgrade); full 16 kHz
  mono PCM held in RAM (~230 MB/hour) is inherent to whisper's full-slice
  API.
- Whole decoded recording transcribed in one whisper call — chunked
  inference would bound memory for very long meetings; out of scope until
  it hurts.

### GAP-53 · Low · MCP polish bundle (from the increment's final review)
Deferred-by-triage follow-ups for the embedded MCP server, best done as one
small slice (all Minor; none merge-blocking — see the final whole-branch
review in the PR-43 ledger):
- Export the service error-message *prefixes* as core constants consumed by
  both `services` and `outcome_label` — the audit labels are copy-coupled
  today, and drift silently degrades a label to `error` without failing the
  `outcome_labels_are_static` test (it hardcodes its own copies).
- Add a dedicated `path-escape` audit label (security observability; those
  rejections currently log as generic `error` plus the detailed warn).
- `status_of` should notice a dead server (`is_finished()` on the join
  handle) instead of reporting `running` until the next settings change.
- `get_mcp_config` is a sync command that can contend on the state mutex
  held across `start()`'s ≤10 s bind wait — make it async or narrow the
  lock (take it only to store the handle, with a `starting` flag).
- `McpSettings.vue`: guard the `mcp:status` listener registration against
  unmount-before-resolve (one leaked listener per fast settings visit).

### GAP-57 · Low · Capture event-ordering corners after the async migration
(Renumbered from a duplicate GAP-56 — two parallel branches minted the same id.)
Catalogued by sub-pass B's final review (2026-07-10); both exotic, neither
worse in kind than the pre-async behavior:
- `capture:saved` can theoretically beat `capture:started`: the monitor
  thread is live before the async shell emits `started`
  (`capture_commands.rs`), so a self-finalizing session (≥1 poll tick,
  ~500 ms) plus a >500 ms async-runtime stall reorders them; the store's
  `started` handler would then set `status = "recording"` after `saved`
  reset it, sticking the recording UI with no terminal event. Fix shape: a
  store-side stale-`started` guard.
- The janitor's worker-replied-`Err` drain clears the reservation without
  emitting (the `capture:failed` fired back at start-timeout time), so a
  stop issued against a resynced wedged reservation can resolve
  `{stillSaving:false}` with no event ever arriving — the store parks in
  "saving" until reload. Requires a webview reload to resync the wedged
  state first; the old bare-`Ok` had the identical hole.

## 9. Documentation & repo hygiene

The 2026-07-10 AGENTS.md overhaul fixed the drift that lived in AGENTS.md
itself (broken PRD link, missing `cancel_transcription` /
`transcription_queue_status` / `count_open_tasks` + `transcription.rs` in
the IPC list, missing `linux-app` job, wrong whisper-CI claim, CONTEXT.md
unreferenced). What remains lives in the *other* docs:

### GAP-49 · ~~Medium~~ FIXED 2026-07-10 · Broken/stale references in the human-facing docs
Every catalogued reference was corrected:
- The PRD was renamed to `docs/PRD.md` (GAP-50) and all 15 referrers'
  link/frontmatter targets were repointed — the README front-page link,
  the AGENTS.md doc map, `docs/DEVELOPMENT.md`, both per-domain PRDs, every
  `docs/use-cases/` page, and the dated increment-1 spec — so none 404s.
- `docs/DEVELOPMENT.md` now names the four member crates plus the shell
  (was "three crates") and its "Tests and checks" command list includes the
  transcribe + mcp clippy/test commands CI actually runs; the updater-signing
  note now says CI builds unsigned by design on PR events rather than
  "needs the secrets to build" (GAP-36).
- `.github/pull_request_template.md` drops the stale "can't compile in this
  container" claim (the Linux compile gate exists) and names all four CI
  jobs (`frontend`, `rust-core`, `linux-app`, `windows-app`) without
  implying a sequence.
- `docs/PRD.md`'s status line reads the shipped v0.5.x reality (Search +
  Tasks shipped, plus the opt-in local MCP server).
- `.github/workflows/release.yml`'s stale `tauri`-npm-script comment and
  `src-tauri/transcribe/Cargo.toml`'s wrong "Windows is the whisper compile
  gate" comment (the Linux `rust-core` job builds *and tests* the feature)
  were both corrected.

### GAP-50 · Low · Naming and structure
- ~~`docs/PRD - Product Vision.md` — spaces in the filename force `%20`
  links, which is what produced the broken references.~~ FIXED 2026-07-10 —
  renamed to `docs/PRD.md`; the 15 referrers (README, AGENTS.md doc map,
  DEVELOPMENT, both per-domain PRDs, every use-cases page, and the dated
  increment-1 spec) were repointed to the new path.
- No CHANGELOG; release bodies are boilerplate. No SECURITY.md (updater
  key rotation/compromise procedure). See GAP-44.

## 10. Verified sound

Explicitly checked during the audit and found correct — do not re-litigate
these without new evidence; they are also the invariants a Gaps fix must
not regress:

- **Shell**: single-instance registered first; panic hook + native crash
  handler before the builder, allocation-free crash path with pre-opened
  handle; every `std::thread::Builder` spawn named; metronome backpressure
  (one outstanding closure, `catch_unwind` both sides); focus-out check
  only-hides + pin exception, both main-thread; all buddy hides route
  through `tray::hide_buddy` with the recording guard; `show_bubble`
  suppression while hidden; window-state saves/getters main-thread-only;
  transcription queue dedup/force-rerun/cancel correct and unit-tested;
  model download cancellable with SHA-256 + size floor + idle timeout +
  `.part` cleanup + corrupt-model self-heal; shutdown mid-transcription
  self-heals via the pending-placeholder backfill; the capability file
  carries no fs/shell/asset-protocol grants.
- **Core**: `place_beside` clamp/anchor math incl. negative-origin
  monitors; `snippet_from_line` char-boundary safety; `set_status`
  CRLF/fence handling and agreement with `is_task`; `yaml_quote` round-
  trip; `is_capture_base` bounds; the filename-before-content search
  guarantee and per-class caps; `rename_noreplace` AlreadyExists semantics
  on dangling symlinks; `EmitThrottle`/`PositionCheckpointer` state
  machines; write-path TOCTOUs backstopped by exclusive-create or
  `rename_noreplace`, including the GAP-06 fallback (direct contract tests
  on the non-Windows arm; the `cfg(windows)` twin now executes on Windows
  CI, fixed 2026-07-10).
- **Capture/transcribe**: exclusive `.part` create; pairwise reservation
  including the transcript name; recovery ownership/layout/staleness
  filters; pause-never-blocks-shutdown; rename keeps the date prefix and
  refuses foreign files; note failure degrades audio-first; download
  hardening (pinned HTTPS URLs, streamed SHA-256, Content-Length check,
  cancel polled per chunk).
- **Frontend**: zero `any` in `src/`; all Rust DTOs
  `rename_all = "camelCase"` matching `types.ts` (spot-checked); Search's
  ticket/debounce logic correct incl. short-query invalidation; Tasks'
  per-row busy set serializes writes; the transcription job map is bounded
  with terminal-only eviction — all covered by tests.
