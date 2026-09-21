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

### GAP-62 · Low · In-process GPU driver fault can crash the app
`src-tauri/transcribe/src/engine.rs` (`WhisperTranscriber::load`, the
`use_gpu` parameter passed to whisper.cpp). GPU inference runs in the same
process as the Tauri runtime. A faulty graphics driver or an incompatible GPU
can crash whisper.cpp during model load or inference (native fault: SEH
exception on Windows, fatal signal on Unix), taking the app with it. Symptom:
crash.log shows a `native crash` record during transcription. Diagnostics:
the first GPU-enabled model load logs the Vulkan device list (id, name,
VRAM) via `ensure_vulkan_backend_registered` — the same guard that works
around upstream whisper.cpp#3750 (a silently CPU-only backend registry on
MSVC static builds) — so the log names the device involved before any fault. **Remedy:** the
`Use GPU (Vulkan)` toggle (Buddy settings → Integrations → Transcription — GPU)
is the immediate escape hatch — turn it off to fall back to CPU inference, which
is always safe. **Permanent fix:** run GPU inference in a sidecar process (a
separate executable), so a GPU fault isolates and the parent app recovers —
the future fix named in the GPU design spec
(`docs/superpowers/specs/2026-07-16-gpu-vulkan-transcription-design.md`).
Mitigation is low-cost (a boolean toggle). Data blast radius is nil even in
the crash case: the recording and its note are fully written before
transcription starts, and the interrupted job's `pending` sidecar is
re-queued by the next launch's backfill — the crash costs the session, not
the vault. That same backfill requeue is also what turns a deterministic
fault (a specific driver/file combination that always crashes) into a
**crash-repeat loop**: every launch re-enqueues the same `pending` job, hits
the same fault, and crashes again, with no built-in circuit breaker. If the
crash recurs too fast to reach Buddy settings and click the toggle, the
last resort is hand-editing `"transcription": {"useGpu": false}` directly
into `config.json` (see docs/DEVELOPMENT.md § Transcription configuration)
before the next launch.

### GAP-63 · Low · Auto-detect can fail a job that used to get a "No speech detected" transcript
`src-tauri/transcribe/src/engine.rs` (the `state.full` call; whisper.cpp
returns -3 when `whisper_lang_auto_detect_with_state` cannot run). Since the
auto-language fix (auto now truly engages whisper's detection), an
auto-language vault with VAD off transcribing audio whose spectrogram is too
short for even one detection window fails the job with the mapped
inference-failure message, where the old forced-"en" path would have produced
an empty "No speech detected" transcript. Strictly more honest (the engine
really couldn't classify anything) and vanishingly rare — capture minimums
make sub-window recordings hard to produce — but it is a behavior delta from
the housekeeping increment's C1 fix (final review, Minor). Remediation
sketch if it ever bites: pre-check the decoded sample count and short-circuit
to the no-speech transcript before invoking the engine.

### GAP-67 · Low · Panel preset clamp uses the buddy monitor's scale but `set_size` uses the panel's, on mixed-DPI multi-monitor
`src-tauri/src/commands.rs` (`position_panel` / `buddy_work_area_logical`, the
`clamp_dims_to_work_area` call). The preset-size clamp added to stop an
oversized non-resizable panel from stranding controls off-screen (Codex,
PR #73) converts the buddy monitor's work area to logical units using the
**buddy** monitor's scale factor, but the subsequent
`panel.set_size(LogicalSize)` is interpreted at the **panel** window's current
monitor scale. These agree in the common case — the panel opens beside the
buddy on the SAME monitor — but diverge for exactly one transient: right after
the buddy is dragged to a monitor with a DIFFERENT DPI, the hidden panel is
still associated with (scaled for) its previous monitor, so on that FIRST open
the clamp is computed in destination-monitor units while `set_size` applies in
source-monitor units; `place_beside_buddy` then positions from the pre-move
physical `outer_size()`, and when the panel is shown Windows runs the DPI
transition and changes its physical dimensions, invalidating the clamp and
leaving it mis-sized or partly off-screen. **Self-healing:** the next open (the
panel now on the destination monitor) is correct. **Why not fixed now:** the
correct fix — move the hidden panel onto the destination monitor BEFORE sizing
and final placement, so size + clamp + position share one coordinate system —
reorders the flicker-critical size-while-hidden → place → show sequence and
hinges on whether a `set_position` while hidden triggers a synchronous Windows
DPI rescale, which cannot be validated in the Linux/headless compile-gate
environment; a blind change to this sequence risks the exact stale-frame flash
the three-window split exists to prevent. **Remediation sketch:** on a Windows
multi-monitor mixed-DPI rig, reorder `position_panel` to (1) `set_position` the
hidden panel onto the buddy's monitor, (2) re-query its scale, (3) clamp +
`set_size` in that scale, (4) final `place_beside_buddy`; verify no flash and
correct first-open sizing across the DPI boundary. The clamp is still
net-positive without this — it fixes the far commoner same-monitor
oversized-preset case — so it stays.

### GAP-56 · Low · Search content cache: fill-to-cap tail and dead entries
`core/src/search_cache.rs`. The cache fills to 256 MiB then stops inserting
(no eviction — uniform per-search access makes LRU pointless), so once total
note text exceeds the cap the last-walked vaults' notes re-read on every search
(still far cheaper than the pre-cache path). Entries for deleted files also
linger until process exit, bounded by the cap. A per-walk mark-and-sweep and/or
a larger/tunable cap would address both; deferred as documented in the spec.

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

Also covers the Silero VAD artifact (`ggml-silero-v5.1.2.bin`,
`transcribe/src/model.rs::download_vad_model`) since the accuracy & speed
increment: verified at download, trusted from disk thereafter — same class,
same accepted posture. A corrupt cached VAD file surfaces as an inference
failure (`failed` sidecar), not a silent wrong transcript. **Update:** the
engine's `Err` arm of `detect_speech_centiseconds` (`transcribe/src/engine.rs`)
now best-effort removes the cached VAD file whenever Silero fails to
load/detect on it, so the next VAD-enabled job redownloads (SHA-verified)
instead of silently degrading to no-VAD forever — self-heal parity with the
main model's load-failure → `remove_model` path. The residual accepted gap
for VAD is narrower: a corrupt file that still LOADS and DETECTS (wrong but
non-erroring segments) is caught by neither self-heal path.

**Update (GPU increment):** the same load-failure → `remove_model` self-heal
also fires on a GPU-side load failure that has nothing to do with disk
corruption — e.g. VRAM exhaustion loading `medium`/`turbo` with `Use GPU`
on. The model file is fine; `process_transcription` can't tell a corrupt
download apart from a transient/environmental GPU failure, so it deletes a
perfectly good 0.5–3 GB file and the next attempt (a manual retry, or the
next launch's backfill) re-downloads it. Wasteful, not wrong — turning the
GPU toggle off avoids the redownload entirely; named as an accepted rough
edge in the GPU design spec's risk section
(`docs/superpowers/specs/2026-07-16-gpu-vulkan-transcription-design.md`).

**Update (transcription housekeeping increment):** the Transcription models
card (Buddy settings → Integrations) is the user-facing remedy for a
suspect cached model — delete a cached artifact in-app to force a
SHA-verified re-download instead of waiting for the load-failure self-heal
to fire.

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

### GAP-64 · Low · `delete_task_list` gives no partial-failure signal when a mid-loop move fails
`src-tauri/core/src/tasks/lists.rs` (`delete_task_list`, the `for f in
&task_files` loop). Each of a list's direct task files is relocated to the
tasks root one at a time via `move_task_to_list(...)?`; if the Nth move
fails (a doc that stopped being `type: Task` between the initial scan and
the move, a mid-loop permission error, or a `rename_noreplace`
source-could-not-be-removed rollback per
`move_task_fails_and_rolls_back_when_source_cannot_be_removed`), files
`1..N-1` are already relocated, the `moved` count accumulated so far is
discarded by the `?` early return, and the caller gets an opaque `Err` with
no signal the vault was partially mutated — **"Err ⇒ nothing happened" does
not hold for this function.** **No data loss**: every moved file rode
`move_task_to_list`'s own never-clobber rails (exclusive
`rename_noreplace` + ` (N)` suffix retry), so nothing is overwritten or
lost — only some tasks silently changed list membership before the error
surfaced. **Accepted as a documented limitation, code unchanged** (a
comment pinning this GAP id sits at the loop): the loop is verbatim from
the list-lifecycle design plan. **What a fix must respect:** the later
services/IPC/UI layers that call `delete_task_list` MUST refresh the task
list after a delete regardless of `Ok`/`Err` — they cannot treat `Err` as
"nothing changed" and skip the refresh. **Fix, if ever pursued:** report
`moved` alongside the error (e.g. an error variant carrying the partial
count) or continue best-effort and aggregate per-file failures, without
breaking the existing `Result<DeleteListOutcome, String>` contract today's
callers depend on.

**Update (subtasks & parent-tasks increment):** `tasks::delete_task_list`'s
error is now `DeleteListError { message, landed }`
(`core/src/tasks/lists/relocate.rs`)
— the "error variant carrying the partial paths" this entry's Fix section
named, though core's own `Result<DeleteListOutcome, String>` contract still
changed (only the ONE caller, the service layer, needed updating).
`services::tasks::lists::delete_task_list` uses `landed` to repair each
already-relocated child's own stale fallback `parent` link (design spec
`2026-07-25-task-subtasks-and-parent-tasks-design.md` §7 — a relocated
child's markdown-fallback link is depth-relative, so a move always stales it)
BEFORE propagating the failure, closing the parent-link consequence this
partial-failure window left when the subtasks increment landed. The residual
is narrower than before: the mid-loop `move_task_to_list` failure's own
message still doesn't name `moved` (only the remove_dir failure's message
did, and still does); and the shell command
(`task_commands.rs::delete_task_list`) still surfaces only a `String` to the
frontend, so a caller wanting the landed count/paths on failure — not just
the best-effort repair core now performs — would need `DeleteListDto`/the IPC
contract widened too.

### GAP-65 · Low · Tasks-polish increment residuals (list lifecycle / copy-ID / drag-to-move, accepted)
- ~~**A move stamps a Task ID on disk but the row reflects it only after a
  reload.**~~ — FIXED in the polish pass (Codex, PR #59): `move_task_to_list`
  now returns `{path, id}` (the effective id rides back from the same
  `update_task_fields` stamp write), and the drag (`useTaskReorderCommit`) and
  list-only editor-save (`useTaskActions.moveToList`) callers set `task.id`
  alongside the landed path — so a moved legacy Task reveals copy-ID
  immediately, matching the `update_task` edit/reorder paths. Every id-stamping
  write path that a caller reads back now returns its effective id.
- ~~**Deleting a list moved its tasks to No list without stamping a missing
  Task ID.**~~ — FIXED in the polish pass (Codex, PR #59): a delete-list
  relocates each direct task through the core `move_task_to_list`, which
  bypassed the service wrapper's id backfill, so a legacy Task lost its one
  chance to be stamped on that user-initiated move (inconsistent with drag /
  editor moves, which stamp). `services::delete_task_list` now threads the
  vault's `id_property` into the core delete loop, which stamps each relocated
  Task best-effort (a stamp failure warns, never fails the delete; an existing
  id is never overwritten). It returns no per-task id — the frontend already
  reloads the task list after a delete (see the GAP-64 note), so the reload
  surfaces the fresh ids. Stamping now spans create/edit/move/delete-list; only
  a status toggle/archive is excluded.
- **The two oversized-file splits flagged here are now done** (polish pass,
  both pure refactors, behavior-preserving): `services.rs` (1229 LOC) was
  split into a per-domain `services/` module — `vault` (registry/open/daily-
  note), `tasks/{mod,lists}` (task-document CRUD vs list-folder lifecycle),
  `recordings`, with `ServicePaths` + the shared `app_config` and test
  `fixture` in `mod.rs`; every resulting file is under the 800 cap, so its
  allowlist entry was removed. `Tasks.vue`'s reorder-commit cluster
  (`writeSingleRank` / `materializeRanks` / `moveTaskToList` / `commitReorder`)
  moved into the `useTaskReorderCommit` composable, dropping the view 648 →
  555. Remaining split candidate: `Tasks.vue`'s buckets/display-state group
  (the view is no longer over its historical mark, so low priority).
- **The aggregate inline editor can show an unselected vault's archived lists
  unfiltered.** `listsForEditor` filters archived lists per vault, but in
  aggregate mode a row's vault config may not be loaded yet
  (`loadVaultConfig` is lazy), so its archived set reads empty and an archived
  list can briefly appear in that row's list picker. Fails open (worst case:
  a hidden list is offered as a move target), single-vault mode unaffected;
  cosmetic, deferred.
- **2026-07-17 full-PR review pass — fixed:** Escape with a section ⋯ menu /
  delete-confirm open closed the whole panel (GAP-27 class; focus-managed
  popover + root Escape handler now step back a level); `archiveList` could
  persist computed-empty prefs over a vault's stored `defaultList`/`listOrder`
  when the config read had failed (now retry-then-refuse); `count_open_tasks`
  counted archived-list open tasks the default Lists view hides (badge now
  mirrors `visibleTasks`); the settings card rendered an archived list as an
  unmarked reorderable row above its own unarchive row (reorder rows now
  filter, slots preserved); stale hidden filter text blocked manual reorder
  (`reorderView` now consumes `filterActive`); config.json was read twice per
  core task-service call; the id-stamp machinery pre-drew discarded CSPRNG ids
  across four duplicated sites (now generated inside `update_task_fields`,
  shared `backfill_task_id`).
- **2026-07-17 review pass — accepted residuals (deliberately not churned):**
  the SelectMenu/TaskSectionMenu outside-click wiring stays two bespoke copies
  (different needs: teleported dual-ref capture-phase vs in-tree single-ref;
  extract only when a third popover appears); TaskViewControls duplicates
  TaskListPicker's inline-create state machine (~15 lines; the two surfaces
  are deliberately distinct entry points — extract on the next consumer);
  `sectionAt`/`rowsFor` re-query rects per pointermove (layout-neutral classes,
  single-digit section counts in a 400×420 panel — negligible);
  `delete_task_list`'s per-file move re-canonicalizes the root (2N realpath
  calls vs N fsync'd writes — negligible).

### GAP-80 · Low · `delete_task` is a permanent unlink, not a recoverable trash (accepted departure)
`src-tauri/core/src/tasks/structural.rs` (`delete_task`). The app's first
destructive vault write removes the task file with `std::fs::remove_file` — an
irreversible unlink, NOT a move to the OS Recycle Bin or a vault `.trash/`
folder (Obsidian's own delete convention). It is heavily gated: canonical
containment, `type: Task` re-validation, a file-identity re-check at unlink
time (GAP-79), a no-follow symlink refusal, and a two-step hardened confirm in
the Task Detail surface — so an *accidental* delete takes a deliberate
confirm-through. But a delete that IS confirmed cannot be undone from within
the app. **Why Low / accepted:** delete is an explicit, confirmed, single-file
action the user opts into (unlike a background write), and the guards make a
stray click safe. **Future refinement:** route deletes to the OS trash (a
`trash`-style crate) or a vault `.trash/` folder so a confirmed delete stays
recoverable — the posture Obsidian itself takes — tracked here as the
follow-up, deliberately out of this increment's scope.

### GAP-81 · Low · A hand-authored multi-line / block / flow `description` reads as empty in Task Detail
`src-tauri/core/src/tasks/description.rs` (`description_field`). The reader
supports only SINGLE-LINE scalar forms (plain, single-quoted, double-quoted);
a block scalar (`description: |` / `>`), a multi-line quoted scalar, and a flow
collection (`[..]` / `{..}`) all degrade to `None` — deliberately, so the field
never surfaces a partial/wrong value or a bare `|`/`>` marker. So a task whose
`description` was hand-authored in one of those forms shows a BLANK description
in the Task Detail surface. `set_fields` consumes a block scalar / block list /
multi-line quoted value whole on the next save (its `skip_block_scalar` path via
`opens_multiline_quoted`), so saving a new description over an UNDECORATED
block/multi-line form cleanly REPLACES the old block rather than orphaning its
continuation lines — the original multi-line content is simply not displayed,
and is replaced on that save. **This no-corruption claim held only for the
undecorated forms** (fixed in the final whole-branch review, task report): a
YAML tag or anchor decorating a multi-line QUOTED value (`description: !!str
"abc` / `  def"`, `description: &a "abc` / `  def"`) hid it from
`opens_multiline_quoted`'s bare `starts_with('"')` check, so a save DID orphan
the continuation line and broke the file's very next YAML parse — the same
corruption class this entry used to say couldn't happen. `opens_multiline_
quoted` now peels a leading anchor/tag (reusing `parse::strip_leading_tag` /
`id::strip_anchor`) before its quote-open test, closing this for `description`
and, since the same gate covers every key `set_fields` rewrites, for `title` on
a rename too. The app only ever WRITES escaped single-line scalars
(`core::yaml_scalar`). **Why Low:**
hand-authored multi-line descriptions are rare and the failure mode is "not
shown / replaced on edit", never broken frontmatter. **Future enhancement:**
teach `description_field` + the detail editor to round-trip a multi-line
description (a textarea ↔ a block scalar). **Related payload note:** every
`list_tasks` row now carries its full `description`, so a vault with long
descriptions inflates the list payload and the MCP DTO — acceptable for now
(descriptions are short free text), with a lazy `get_task` command the fallback
if it ever bites.

### GAP-79 · Low · `delete_task`'s identity re-check leaves an irreducible re-stat→unlink residual (accepted)
`src-tauri/core/src/tasks/structural.rs` (`delete_task`). The one destructive
vault write validates a task's bytes + inode identity from a single open handle,
then re-stats the path (no-follow) immediately before `remove_file` and refuses
on a symlink or an identity mismatch (`is_different_file`) — so a swap during the
validation window is caught, not deleted (Codex P1, PR #76). TWO residual windows
remain, both irreducible in portable `std`: (a) between `symlink_metadata(path)`
and `canonicalize(path)`, a swap of the clicked leaf to a symlink pointing at
ANOTHER valid task would let `canonicalize` resolve to that target (the identity
re-check is self-consistent on the target, so it passes) — closing this
deterministically needs `O_NOFOLLOW` on the ORIGINAL path (libc FFI, Unix-only)
rather than a canonical re-open; (b) between the pre-unlink re-stat and
`remove_file` itself, since `std` has no unlink-by-handle (`funlinkat` /
`NtCreateFile`+`FILE_DELETE_ON_CLOSE`). **Why Low / accepted:** the whole of
`delete_task` runs at machine speed with NO user pause inside it — the delete
confirm happens client-side, before the IPC call — so both windows are
microseconds on a single-user desktop with no adversarial local process racing
the buddy; the earlier deterministic confirm-time symlink attack is already
closed (GAP is not that). A fully-atomic unlink (platform FFI: `funlinkat` on
Unix, delete-on-close handle on Windows) is the tracked fix if the exposure ever
proves real; the identity re-verification is the proportionate portable close.

### GAP-78 · Low · A duplicated/edited Task keeps a SECOND case-variant ID key when the source has two
`src-tauri/core/src/tasks/disk.rs` (`duplicate_task` and `update_task_fields`
id-key resolution) via `parse::frontmatter_scalar_ci`, which returns only the
FIRST case-insensitive match. If a source task carries two physically distinct
frontmatter keys that differ only in case AND both match the configured id
property name (e.g. a legacy `Task-ID: aaa` line followed by `task-id: bbb`),
the id-stamp/strip path rewrites or removes only the first: with IDs enabled the
duplicate/edit gets a fresh id on `Task-ID:` while `task-id: bbb` survives (two
id-ish keys; Obsidian may resolve the stale one → two tasks sharing a stable
identity); with IDs disabled a strip leaves the second key behind (incomplete
strip). **Why Low / document-only:** the trigger is pathological — the app
writes exactly ONE id key in its configured casing and Obsidian's Properties UI
normalizes keys, so two case-variant id keys only arise from hand-authoring or
an external tool that violates the "one id property per task" invariant at the
source. **Why not fix only `duplicate_task`:** the single-occurrence
`frontmatter_scalar_ci` + `set_fields` assumption is IDENTICAL in the
already-shipped `update_task_fields` edit path (and the `ensure_id`/backfill
sites that share it), so a fix belongs across all of them at once — a shared
"collect every case-insensitive occurrence, rewrite one + remove the rest"
helper — not bolted onto the newest write path in isolation. Codex (P2, PR #76)
raised it against `duplicate_task`; documenting it here (consistent with the
GAP-68/GAP-77 id-edge precedent) is the proportionate call, with the shared
multi-occurrence helper the tracked fix if the exposure ever proves real.

### GAP-77 · Low · Reserving `description` disables ID generation for a (formerly settable) `description`-as-id-property config
`src-tauri/core/src/tasks/id.rs` (`RESERVED_TASK_KEYS`, `is_valid_id_property`,
`id_property_for_generation`). The Task Detail increment
(docs/superpowers/specs/2026-07-24-task-detail-surface-description-verbs-design.md)
adds `description` as a managed frontmatter field and reserves it in the shared
`RESERVED_TASK_KEYS` set (so BOTH the id-property validator and the template
filter refuse it — see the closing note). Before this increment `is_valid_id_property`
did NOT reserve `description`, so the supported `set_task_id_config` command
would ACCEPT `description` as a task-id property. A vault that had
`task_id_enabled` ON and its id property set to the literal `description`
therefore has on-disk tasks carrying `description: <stable-id>`.
**Consequence:** reserving `description` makes `id_property_for_generation`
re-validate and turn id generation OFF for that vault (logged); `list_tasks`
stops surfacing the stored ids (the property is no longer read) and
creates/edits stop stamping, while the stored config still reads `enabled`.
This is the exact shape of GAP-68 (`scheduled`-as-id). **Why Low / document-only:**
the exposure is near-zero (it needs the id property *named* the literal
`description`, a nonsensical choice for a stable-handle property), and the
remedy — re-point the id property to a non-reserved name — is a one-line
settings change. As with GAP-68 we deliberately do NOT auto-migrate (rewriting
every task's id property is the mass vault mutation this app forbids) nor
hard-block (punishing the overwhelmingly common vaults for a config essentially
no one has). Codex (PR #76) re-raised migrate/block; document-only is the
proportionate, precedent-consistent call. A non-mutating startup detection +
warning is the tracked future option if the exposure ever proves real.
`description` is reserved in the template set too (it is a managed detail-view
field, like `due`/`status`), so a task template cannot seed it — which also
avoids a template block-scalar `description` orphaning on the first detail-view
save (Codex P2, PR #76).

### GAP-68 · Low · A do-date write can overwrite a stable Task ID in the (formerly settable) `scheduled`-as-id-property config
`src-tauri/core/src/tasks/id.rs` (`RESERVED_TASK_KEYS`, `is_valid_id_property`,
`id_property_for_generation`) and `src-tauri/core/src/tasks/disk.rs`
(`RESERVED_TASK_KEYS`, the surgical `set_fields`/`update_task_fields` writer).
The do-date increment
(docs/superpowers/specs/2026-07-24-task-management-do-date-planner-foundation-design.md)
adds `scheduled` as a managed frontmatter field and reserves it in BOTH
reserved-key sets. **Accuracy note (Codex, PR #75):** before this increment `is_valid_id_property`
did NOT reserve `scheduled`, so the supported `set_task_id_config` command (and
the Task-IDs settings card) would ACCEPT `scheduled` as a task-id property — this
config was reachable *through the app*, not only by hand-editing `config.json`. A
vault that had `task_id_enabled` ON and its task-id property set to the literal
`scheduled` therefore has on-disk tasks carrying `scheduled: <stable-id>`.
**Mitigation, going forward:** reserving `scheduled` makes
`id_property_for_generation` re-validate and turn id generation OFF for that vault
(logged) — no duplicate `scheduled:` on create, and `set_task_id_config` now
refuses to set it — while on READ `scheduledOf` accepts only a plain
`YYYY-MM-DD`, so a non-date id value reads as *unscheduled* and never surfaces as
a do-date. **What is NOT claimed:** never-clobber for the pre-existing on-disk ids
in that config. Those tasks still contain `scheduled: <stable-id>`; if the user
later schedules (or clears the do-date on) such a task, the surgical write
rewrites or removes the `scheduled:` line and the id is lost. **Why still Low /
document-only for now:** the exposure needs `task_id_enabled` on (opt-in, off by
default) AND the id property *named* the literal `scheduled` — a semantically
nonsensical choice for a stable-handle property (it reads as a date field), which
is why the realistic affected population is near-zero even though the config was
command-reachable. The near-zero claim rests on that name-choice implausibility,
NOT on a false "hand-edit only" barrier. **Remedy:** re-point the id property to a
non-reserved name *before* scheduling such a task. **Alternatives weighed,
deliberately declined this increment (the approved design's call):**
auto-migrating the on-disk ids to a new property is exactly the mass vault
mutation this app forbids; hard-blocking scheduling reintroduces the
duplicate-`scheduled:` clobber and punishes the overwhelmingly common vaults.
**Proportionate escalation IF real affected vaults surface:** a one-time,
non-mutating startup DETECTION + user warning ("your Task-ID property `scheduled`
is now a reserved do-date field — re-point it in Settings to keep your IDs")
keeps the human in control without an automatic rewrite — a tracked future
option, not built here. **What any future fix must respect:** it must ride the
same never-clobber/atomic rails as every other vault write and must not rewrite
files the user did not touch. (Codex re-raised migration/block on PR #75; the
document-only posture is the approved spec's decision — revisiting it is a human
call, recorded for the final review.)

### GAP-83 · ~~Medium~~ FIXED 2026-07-25 · No per-file lock serialized concurrent task-file writes (lost updates, deleted-file resurrection, moved-file duplication)
`src-tauri/core/src/tasks/disk.rs` (`update_task_fields`),
`src-tauri/core/src/tasks/structural.rs` (`delete_task`),
`src-tauri/core/src/tasks/lists/relocate.rs` (`move_task_to_list`). None of
the three sanctioned task-file writers took any lock keyed on the file they
were about to touch: each is a plain read -> compute -> atomic-write (or
read -> unlink, or read -> rename) with nothing serializing two IN-PROCESS
callers on the SAME file. The embedded MCP server's
`set_task_status`/`add_task` reach the exact same `core::services` functions
on their own "mcp-blocking" OS thread (see AGENTS.md's MCP section),
genuinely concurrently with a panel write — and the frontend's per-row
`busy` guard is per-webview, so it cannot see an MCP call at all. Three
concretely reachable failure modes, all silent (nothing surfaces the
corruption as an error):
- **Lost update** (`update_task_fields` racing itself): two concurrent field
  writes to the same task both read the same pre-edit content; whichever
  writes second silently discards the first writer's key along with
  everything else it re-derived from that stale read.
- **Resurrection** (`delete_task` racing a concurrent field write): a write
  that already read the file before delete unlinks it will `rename` its own
  temp onto that path afterward — `write_atomic_replacing`'s rename
  recreates the destination unconditionally — bringing back a file the user
  just deleted.
- **Duplication** (`move_task_to_list` racing a concurrent field write): the
  same pattern at the file's OLD path after a move relocates it elsewhere,
  leaving the same document at both the old (a stale resurrected copy) and
  the new path.

The subtasks/parent-tasks increment made the first failure mode a worse
casualty: `resolve_parent_for_write` stamps the parent's Task ID via
`update_task_fields`, and a racing `set_task_status` on that same parent file
can overwrite the stamp away, silently orphaning the child's `parent-id`
reference the moment the hierarchy is created.

**Fixed:** a process-wide, canonical-path-keyed lock
(`disk::with_task_file_lock`, a `Weak`-pruned map so it cannot grow
unboundedly with how many files this process has ever touched — only with
how many are being written RIGHT NOW) is now held across each of the three
writers' own read-through-write/unlink/rename sequence. Lock ordering is a
documented invariant at the definition site: `capture_config::
config_write_lock()` may be acquired before this lock, never after —
`resolve_parent_for_write` already does so across two calls into it, and
nothing that takes the new lock ever reaches back into config. Regression
tests (`tasks::disk::tests`, `tasks::structural::tests`,
`tasks::lists::relocate::tests`): Barrier-synchronized two-thread races for
all three writers, the exact parent-id-vs-status-flip scenario
(`concurrent_parent_id_stamp_and_status_flip_never_lose_either`), a
canonical-vs-symlink-path aliasing test proving the lock cannot be
sidestepped by a different spelling of the same file, and a lock-map
growth-bounding test. Pre-fix, all four race scenarios reproduced their
failure on 299-300 out of 300 iterations across repeated runs in the
authoring environment — a reliably reproducible defect, not a rare
interleaving that needed an injected hook or a sleep to hit.

**What remains open, by design:**
- **Cross-process writes are entirely out of scope.** This is an in-process
  lock; it cannot and does not serialize against Obsidian itself or a sync
  client (OneDrive/Dropbox/Syncthing) writing the same file from another
  process. That is the same pre-existing, accepted reality already true of
  the rest of the vault domain — nothing here claims otherwise.
- **`structural::duplicate_task` deliberately does NOT take the lock.** It
  reads the source exactly once and writes to a brand-new path nobody else
  could be racing (the collision-safe writer picks that path itself); a
  concurrent source edit can make the copy a stale-but-consistent snapshot
  (never torn — `update_task_fields` only ever replaces the source via a
  full create-temp-then-rename), but never a lost or resurrected one. A
  different, accepted category from the three fixed above.
- **`lists/mod.rs`'s `rename_task_list` (a whole-FOLDER `std::fs::rename`,
  not a per-file write) was evaluated and left unchanged.** A concurrent
  `update_task_fields` racing a folder rename fails cleanly — its temp-file
  create targets a parent directory that, after the rename, no longer
  exists at that path — a legitimate write error, not a resurrection or a
  duplication; a materially milder failure shape than the three fixed here.
  Extending the per-file lock to a whole-subtree rename would mean locking
  every contained file (or inventing a coarser folder-level lock with its
  own new ordering rules) to harden a failure mode that already degrades
  safely — deferred as disproportionate to the risk.

### GAP-84 · Low · Moving a Parent Task leaves its children's `parent` link stale (design decision, tracked)
`src-tauri/core/src/services/tasks/parent/mod.rs` (`repair_parent_link`,
called from `services::tasks::lists::move_task_to_list`) and
`src-tauri/core/src/tasks/lists/relocate.rs` (`move_task_to_list`). A moved
Task recomposes its OWN `parent` link when it is itself a child on the
markdown-fallback form (the destination is relative to the child's own
directory, so a depth change would otherwise point at nothing). It does
**not** do the reverse: moving a Task that is itself a **parent** never
touches the `parent` link recorded on any of its children, because their
own paths did not change — only the PARENT's path did, and the markdown
fallback destination is computed at write time from the child's directory,
which is unaffected. **Consequence:** every child of a moved parent that
uses the markdown-fallback link form (a List name containing a wikilink
metacharacter — `# | [ ] ^`) now has a `parent` link that resolves to
nothing, or — for a child still on the plain wikilink form — a
vault-relative wikilink is unaffected by the parent's move (Obsidian
resolves wikilinks by name/path lookup regardless of the linking note's own
location), so only the fallback form actually degrades. **Why this is
harmless to the app's own logic:** `parent-id` is authoritative for every
resolution VB itself performs (children, ancestors, cycle checks); only
Obsidian's click-through and Dataview degrade, and they degrade *visibly*
(an unresolved link) rather than silently resolving to the wrong Task — the
whole reason the link form was chosen at all. The link self-heals the next
time that parent is set again (a fresh `parent` link is always recomposed
against the current path pair). **Why deliberately NOT fixed:** refreshing
every child of a moved parent is an unbounded batch write with no natural
size limit — the exact thing this increment's design
(`docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md`
§7) declines to bolt onto the move path, for the same reason `delete_task`
stays a single-file op instead of cascading. **Tracked fix, if this ever
proves real:** on a parent's own move, enumerate its direct children (a
`parent_index` lookup already available from the loaded task set) and run
the same best-effort, warn-only `repair_parent_link` on each — bounded by
this vault's own child count, not the whole tree, and still never failing
the move that carried it.

### GAP-85 · Low · Deleting a Parent Task leaves orphaned children with stale `parent-id`/`parent` keys (design decision, tracked)
`src-tauri/core/src/tasks/structural.rs` (`delete_task`). Deleting a Task
that is somebody's parent does not touch its children at all: their
`parent-id` now names an id no Task answers to, and their `parent` link
resolves to nothing. `tasks::hierarchy::parent_index` already handles this
gracefully for display — an unresolvable `parent-id` yields no edge, so
each orphaned child renders as an ordinary top-level Task, not a broken row
— but the stale keys themselves remain on disk, harmless but inert, until
the user next sets (or clears) that Task's parent, which rewrites both keys
from scratch. **Why deliberately NOT fixed:** `delete_task` is the app's
ONE destructive vault write, deliberately scoped to a single, identity-
re-validated file (docs/Gaps.md GAP-79/GAP-80) — bolting a "find and clear
every child's parent keys" batch write onto it would both widen that
destructive path's blast radius and duplicate the exact batch-write problem
GAP-84 above declines to solve for a move. The design spec (§7) records the
tidier alternative explicitly: best-effort clearing each child's parent
keys, mirroring how `delete_task_list` relocates (not deletes) the tasks in
a removed list rather than leaving them dangling. **Tracked fix, if orphan
clutter ever proves real in practice:** a best-effort, warn-only sweep — using
the already-loaded task set to find every child of the path about to be
deleted and clearing (not touching anything else) their `parent-id`/`parent`
pair — run before or after the unlink, never allowed to fail the delete
itself.

### GAP-86 · Low · Reserving `parent`/`parent-id` disables ID generation for a (formerly settable) `parent`-as-id-property config
`src-tauri/core/src/tasks/mod.rs` (`RESERVED_TASK_KEYS`, now including
`parent-id` and `parent`) and `src-tauri/core/src/tasks/id.rs`
(`is_valid_id_property`, `id_property_for_generation`). Before this
increment, neither `parent` nor `parent-id` was reserved, so the supported
`set_task_id_config` command would have ACCEPTED either literal string as a
vault's task-id property — this is the exact GAP-68 (`scheduled`)/GAP-77
(`description`) shape, not a new failure mode. A vault that had
`task_id_enabled` on with its property set to the literal `parent` or
`parent-id` therefore has on-disk Tasks carrying `parent: <stable-id>` or
`parent-id: <stable-id>` as their OWN id, not a relationship reference.
**Consequence:** reserving both keys now makes `id_property_for_generation`
re-validate and turn id generation OFF for that vault (logged); `list_tasks`
stops surfacing the stored ids under that property, and creates/edits stop
stamping, while the stored config still reads `enabled`. **Why Low /
document-only, same reasoning as GAP-68/GAP-77:** the exposure needs a
property literally named `parent` or `parent-id` — an especially perverse
choice now that both are meaningful, reserved relationship fields, so the
realistic affected population is effectively zero, and the remedy (re-point
the id property to a non-reserved name in the vault's Task ID settings) is a
one-line change. Consistent with the established precedent, this is
document-only, not auto-migrated (rewriting every Task's id property is the
mass vault mutation this app forbids) and not hard-blocked (punishing every
other vault for a config essentially no one has).

### GAP-87 · Low · The Parent Task write's three phases are not atomic — accepted, since every partial state is benign (by design)
`src-tauri/core/src/services/tasks/parent/mod.rs`
(`resolve_parent_for_write`). Setting a Task's parent is, on disk, up to
three separate writes under one `config_write_lock()`: (2) idempotently
enabling Task IDs for the vault if they were off, (3a) stamping the
parent's own id if it lacked one, and (3b) writing the child's `parent-id`/
`parent` pair. This app has no journal, so a crash or a process kill between
any two of those writes is a genuine partial state, not merely a race the
per-file/config locks (GAP-83) can close. **Why this is accepted rather than
a gap needing work:** the ordering is deliberately chosen so every
reachable partial state is self-correcting — enabling Task IDs and stamping
an id are both additive and idempotent (exactly what a later edit would do
anyway), and the one write that actually creates the relationship, the
child's pair, is LAST. A crash can therefore leave Task IDs enabled and/or a
stamped parent id with nothing pointing at either yet, but it can **never**
leave a `parent-id` that no Task answers to — the property that actually
matters for the hierarchy's integrity. Claiming true transactionality here
would be dishonest (there is no rollback), but the weaker guarantee — no
dangling reference is ever created by a partial failure — is both accurate
and sufficient, and is the explicit design call recorded in
`docs/superpowers/specs/2026-07-25-task-subtasks-and-parent-tasks-design.md`
§2 ("On transactionality"). No fix is tracked; a real journal/transaction
log would be a substantially larger change to the whole task-write path,
disproportionate to a failure mode that already can't corrupt the graph.

### GAP-88 · Low · The anchor-stripping charset in `mirror_id_reference` is conservative (disclosed limit, not a silent gap)
`src-tauri/core/src/tasks/id.rs` (`mirror_id_reference`, `strip_anchor`).
When a parent's own id line carries a YAML anchor (`&name value`, e.g.
`task-id: &stable abc`), mirroring it onto a child must strip the anchor
annotation (copying it verbatim would define a SECOND anchor of the same
name in the child's document) and mirror only the value. The anchor NAME is
matched against `[A-Za-z0-9_-]+` — the same conservative charset this
module already uses for id PROPERTY names — not the full legal YAML anchor
character set (which is far broader: YAML permits most printable
characters in an anchor name apart from flow indicators and whitespace). An
anchor name using a character outside `[A-Za-z0-9_-]` fails the charset
match and falls back to the safe, quoted encoding of the DECODED value
(`quote_id_if_needed`) rather than being mirrored — never a wrong or
corrupt mirror, just a less literal one for that one uncommon case. **Why
Low / disclosed limit:** a hand-authored YAML anchor name outside the
conservative charset is rare to begin with (most authoring tools and
conventions produce alphanumeric anchor names), and the fallback path is
always safe — it still resolves to the identical decoded string the strict
reader would report, just via a quoted scalar instead of a bare mirrored
token. This is a documented, deliberate scope limit of a line-oriented
reader that was never meant to be a full YAML parser (the same posture the
rest of the id/parent reading takes), not an oversight; widening the
charset (or replacing the line-oriented scan with a real YAML anchor
parser) is the fix if a real anchor-name collision is ever reported.

### GAP-93 · Low · The parent link's markdown-fallback destination hardcodes a lowercase `.md`
`src-tauri/core/src/tasks/parent_link.rs` (`compose`, line 45:
`format!("[{}]({dest}.md)", escape_label(parent_title))`). `dest` is built
from `rel_no_ext` — the parent's vault-relative path with its extension
ALREADY stripped case-agnostically (`uri::vault_relative_no_ext`'s
`.with_extension("")`), so the composer has no idea what the ORIGINAL
extension's casing was by the time it reappends a literal `.md`. **Failure
scenario:** a parent Task lives at a hand-authored, case-sensitive/synced
path like `Tasks/Proj#1/Plan.MD` (a legal on-disk form this domain already
treats leniently everywhere else — `is_markdown_name`/`collect.rs` match
`.md`/`.MD`/`.Md` alike, precisely so a hand-authored mixed-case file isn't
invisible to the structural scan). Because `Proj#1` contains a wikilink-
unsafe `#`, `compose` falls back to the percent-encoded markdown form —
and reappends `.md` unconditionally, producing
`[Plan](../Tasks/Proj%231/Plan.md)`: a dead link, since the real file is
`Plan.MD`. `repair_parent_link` (the post-move link-repair path) recomposes
the IDENTICAL wrong form, so the link can never self-heal even after a
move that would otherwise refresh it. **Why Low:** requires BOTH a
mixed-case markdown extension (rare — most authoring tools emit lowercase
`.md`) AND a wikilink-unsafe List name (the metacharacter is what forces the
markdown-fallback branch at all; the far more common wikilink form never
appends an extension and is entirely unaffected); the failure is a dead
click-through link, not data loss or corruption, and harmless on Windows'
case-insensitive filesystem. **Fix shape:** thread the parent's ACTUAL
on-disk extension (available from `parent_path` before `vault_relative_
no_ext` strips it) through to the markdown-fallback branch instead of a
hardcoded literal, or special-case only when the source casing differs
from `.md`.

### GAP-94 · Low · A double-anchored id property orphans the relationship on write (invalid-source, narrow)
`src-tauri/core/src/tasks/id.rs` (`mirror_id_reference`/`strip_anchor`).
Both the write-side mirror and the read-side strict decoder strip only ONE
leading YAML anchor. A source carrying two — `task-id: &a &b c` — is itself
unusual/borderline-invalid YAML (a scalar carries at most one anchor in
any real authoring tool), but nothing here rejects it outright.
**Verified by trace:** `strip_anchor("&a &b c")` peels `&a`, leaving `&b c`;
`mirror_id_reference` treats that remainder as an ordinary mirrorable plain
scalar (`is_valid_scalar_source` parses `&b c` as the scalar "c" decorated
by anchor "b", which is scalar-shaped) and mirrors it VERBATIM — so the
child's `parent-id:` becomes `&b c`, which DEFINES a second anchor named
"b" inside the CHILD's own document, exactly the corruption
`mirror_id_reference`'s own doc comment says copying an anchor verbatim
must never cause. Meanwhile the READ side hits the identical one-anchor
limit from the other direction: the PARENT's own id, decoded via
`scalar_id_ci`/`strict_scalar_field`, also strips only the first anchor and
reports `"&b c"` (the leftover second anchor is never recognized as
anything but ordinary text) — so the parent's `id` ("&b c") and the child's
`parentId` (which decodes its OWN, already-mirrored "&b c" down to "c",
since IT only carries one anchor) never compare equal as strings, and
`parent_index` resolves no edge at all. **Why Low:** the triggering source
line is not something any real tool or Obsidian's own properties UI would
ever author — a double anchor on one scalar is degenerate YAML to begin
with — and the outcome is "no edge" (an orphan, the domain's own "wrong
reference is worse than none" posture holding even here) plus a stray,
unreferenced anchor definition, never a crash or a wrong-parent mismatch.
**Fix shape:** either reject a value whose stripped remainder ITSELF starts
with `&` (fold back to the safe quoted-decoded encoding, matching every
other anchor-adjacent fallback in this module) or loop `strip_anchor`
until it stops matching, so a doubly (or N-times) anchored source degrades
safely instead of leaking a second anchor into the child.

### GAP-100 · Low · `render_extra_frontmatter` re-emits a user-quoted sexagesimal scalar bare
`src-tauri/core/src/template.rs` (`render_extra_frontmatter`'s
sentinel-round-trip through `serde_yaml_ng`). A template line quoting its
own placeholder, e.g. `len: "{{duration}}"`, renders as `len: 3:17` — the
quotes the author wrote are gone. This is pre-existing shared-helper
behaviour that the capture-note, document-import, task, and (as of this
branch) screen-capture template surfaces all inherit equally; it was not
introduced by Screen Capture, only newly exercised by it
(`screen_note.rs`'s `template_placeholders_resolve` test pins the exact
`len: 3:17` output as current behaviour, not a bug it's asserting against).
**The risk is narrower than "re-quoting is dropped" sounds:**
`serde_yaml_ng` already re-quotes every implicit int/float/bool/null on
re-emit (`1.0` → `'1.0'`, `197` → `'197'`, `true` → `'true'`), so the only
shape that reaches the page unquoted is a **sexagesimal-looking string**
such as `3:17` or `1:02:03` — precisely `capture_note::format_duration`'s
output, the one placeholder value with that shape today. Under YAML 1.2 /
js-yaml 4.x — what Obsidian ships — `3:17` is already a plain string with
no special sexagesimal handling, so practical impact in Obsidian is nil.
The exposure is confined to a YAML-1.1 consumer (e.g. a Python
`yaml.safe_load` on an old PyYAML, or a Dataview-adjacent tool built on an
older parser), which reads `3:17` as the integer 197, not the string.
**Severity: Low / informational** — narrow consumer surface, no data loss,
and the workaround (don't wrap `{{duration}}` in quotes in a template,
since the renderer already quotes every managed field itself) is already
available. **Fix shape, if ever taken up:** have the sentinel round-trip
preserve an explicit source quote style per key rather than only
re-deriving it from the parsed value's type.

### GAP-101 · Low · Region-capture geometry: even-dimension enforcement is done, chroma-plane offset parity and `to_physical`'s floor are not
`src-tauri/core/src/screen_geometry.rs`. **Resolved as of this branch:**
`clamp_to_frame` now rounds each of `width`/`height` down to the nearest
even value (returning `None` if either rounds to 0), closing the H.264/NV12
"odd luma dimension" failure this entry originally tracked — see the
function's own doc comment for the reasoning, and its test module for the
review case (`1921x1081` frame, `400x400` rect at `x=1800,y=1000` → `120x80`,
not `121x81`).

**What remains, narrower than the original entry:**
1. **x/y offset parity.** NV12's chroma plane is half-resolution on each
   axis, so cropping at an ODD `x` or `y` cannot land the chroma plane on an
   integer boundary either — `clamp_to_frame` does not touch `rect.x`/
   `rect.y` at all today, only `width`/`height`. Whether this matters depends
   on which layer performs the actual pixel crop: if Phase 2 crops via Media
   Foundation's video-processor MFT using a source rectangle, the VP handles
   arbitrary offsets internally; if it instead slices the NV12 buffer
   in-process before handing it to the encoder, an odd offset is a real bug.
   Undecided until the fragmented-MP4 spike determines which approach Phase 2
   takes.
2. **`to_physical`'s floor stays `.max(1)`, not `.max(2)`.** This was a
   deliberate, narrower-scope choice (not an oversight) when the even-
   dimension fix landed: `clamp_to_frame` is documented as the last pure
   gate before the encoder, and is where the even-and-at-least-2 enforcement
   lives; `to_physical` only guarantees "not zero" for a value that has not
   yet been clamped against a real frame. Whether `to_physical` should also
   floor at 2 for consistency (so a caller who used `to_physical`'s output
   directly, without going through `clamp_to_frame`, couldn't observe an odd
   1-px dimension) is an open question — no such caller exists today, but
   one could be added without necessarily going through `clamp_to_frame`.

Owner: Phase 2 (the capture-engine phase, alongside the fMP4 spike) for (1);
either Phase 2 or a follow-up hardening pass for (2).

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

### GAP-82 · Low · Task Detail (like every panel sub-view) loses unsaved edits on panel auto-hide + reopen
`src/components/TaskDetail.vue` + the `panel-shown` refresh
(`src/stores/vaults.ts` `refresh()` → `showList()`). Editing a field in the
Task Detail surface and then clicking outside the panel — or switching
applications — triggers the focus-out auto-hide; reopening the panel emits
`panel-shown`, whose `refresh()` defaults to `showList()` and unmounts
`TaskDetail`, discarding its component-local draft + `dirty` state without
warning. This is NOT specific to Task Detail: the panel keeps no history or
draft state, so EVERY sub-view (the inline `TaskEditor`, `RecordMode` config,
the import picker, …) resets to the list on reopen — the detail view simply has
a larger draft to lose. **Why Low:** the reopen-resets-to-list behavior is
long-standing and consistent; the user's edits are one explicit Save away, and
the surface makes Save prominent. **Future work (cross-cutting):** persist a
dirty draft across reopen, or confirm / autosave before a dirty exit — an
app-wide panel concern (it should cover the inline editor too), deliberately
not bolted onto TaskDetail alone. Codex raised it (P2, PR #76) against Task
Detail specifically.

### GAP-89 · Low · A hierarchy write that also bootstraps Task IDs can silently hide its own success behind a degraded reload
`src/composables/useTaskHierarchy.ts` (`setParent`) and
`src/composables/useTaskDetailTaskSet.ts` (`reload`). When a parent-set (or
Add Subtask) is the vault's FIRST hierarchy write, the backend's response
carries `idsEnabled: true` — the whole cached task set was loaded with every
id suppressed, so `setParent` deliberately skips its cheap two-row
optimistic patch and instead calls `reload()` to re-fetch everything with
ids now visible (the two-row patch would correctly reveal the relationship
just created, but leave any PRE-EXISTING dormant hierarchy — hand-authored
ids + parent links that were invisible only because ids were off — still
orphaned on screen). The problem is that `reload()` is best-effort in
exactly the way `list_tasks` (the `include_archived: true` path,
`services::list_tasks_including_archived`) already documents itself to be:
an inaccessible tasks root degrades to an EMPTY array, and one unreadable or
non-UTF8 `type: Task` file is silently skipped — both as a **successful**
resolve, never a rejected promise (`reload`'s own `try`/`catch` only ever
fires on a genuine IPC-bridge failure, which `list_tasks`'s never-throw
contract makes rare in practice). `reload()` has no plausibility check on
what comes back — it unconditionally replaces the ENTIRE cached
`allTasks.value` with whatever `list_tasks` returned, even an empty or
partial array. So the parent write that just landed on disk (the backend
call already returned success) can be followed, within the same `setParent`
call, by a "successful" reload that returns fewer rows than before —
silently dropping the just-made relationship from view (the Parent row
reads "No parent" again) and potentially discarding the WHOLE previously-
loaded, correct hierarchy for this Task Detail session, not just the two
rows the write touched. **Failure scenario:** a vault on a flaky network
share or sync client where the tasks root or a specific file is transiently
unreadable for the split second between the write and the immediate
follow-up reload — plausible for exactly the kind of vault this app already
documents as a source of transient read failures elsewhere (search's
content cache, the capture recovery sweep). **Why Low:** no data loss (the
write already landed correctly on disk) and no crash; the view self-heals
the next time this Task Detail is remounted (drilling away and back, or a
panel reopen) and successfully re-fetches the real state. **Fix shape, not
implemented here:** apply the returned `TaskWriteResult`'s patch onto the
CURRENT two rows unconditionally (as the non-bootstrap branch already does)
before — or regardless of the outcome of — the reload, so a degraded reload
can only fail to reveal a pre-existing dormant hierarchy, never hide the
write this call itself just made; alternatively, have `reload()` report
success/failure and retain the prior `allTasks.value` on a failed or
suspiciously-empty response rather than unconditionally replacing it.

### GAP-95 · Low · Sibling settings-tab autosaves don't revert their optimistic value on a rejected save (audited alongside the Task-ID toggle fix)
`src/components/TasksConfigTab.vue` (`autosave`, lines 36-46, feeding the
tasks-folder field via `onFolderInput`, lines 132-135) and
`src/components/TaskListSettings.vue` (`onDefaultChange` line 102, `move`
line 109, `unarchive` line 123, all backed by its own `autosave` at line 58)
all mutate a ref optimistically, call `saveNow()`/`schedule()`, and never
revert on a caught rejection — the same no-revert shape the Task-ID toggle
had before this round's fix (`TasksConfigTab.vue`'s `onIdEnabledChange` /
`idAutosave`, which now reloads from `get_tasks_config` in its `catch`).
**Failure scenario:** a rejected `set_tasks_config` leaves the folder input
showing the rejected text indefinitely — `pendingFolderChange` stays `true`,
so `TaskListSettings` stays replaced by the "reload once the tasks folder is
saved…" placeholder until the user manually retypes the last-good folder or
a later save succeeds; a rejected `set_task_lists_config` from a default-list
pick, a reorder, or an unarchive leaves the picker/order/archived-set showing
a choice that was never actually persisted. **Why filed as a gap rather than
fixed alongside the toggle:** unlike the toggle, none of these fields gate a
message behind their own state — each one's inline error
(`tasks-folder-error`, `task-lists-error`) renders unconditionally alongside
the stale value, so the specific "the error is hidden exactly when it fires"
compounding bug does not reproduce here; the residual is a UI that silently
disagrees with disk until the next successful save, not an invisible
refusal. **Fix shape:** the same one applied to the Task-ID toggle — reload
the affected field(s) from `get_tasks_config` in each `catch` block instead
of trusting a locally-cached "previous" value, which a concurrent external
change could already have invalidated.

### GAP-96 · Low · TaskTemplateSettings never surfaces a save error at all
`src/components/TasksConfigTab.vue` (`templateAutosave`, lines 107-116)
computes an `error` ref via `useAutosave` exactly like every sibling field,
but `<TaskTemplateSettings>` (lines 227-234) is never passed one —
`TaskTemplateSettings.vue`'s `defineProps` (line 8) declares only
`extraFrontmatter`/`bodyTemplate`, no `error` prop at all. **Failure
scenario:** a rejected `set_task_template_config` (an unknown vault id — the
vault was removed while settings were open — or a disk-level failure from
`update_vault_config`) sets `templateAutosave.error.value` to the detailed
message, but nothing in this card ever renders it; only the shared
`settingsStatus` header's generic failure indicator (if the user happens to
be looking at it) hints anything went wrong. **Why distinct from GAP-95
above:** this is not merely "no revert" — the per-field error is
unreachable BY CONSTRUCTION (never wired to a prop), not just hidden behind
a stale conditional the way the Task-ID toggle's was before this round's
fix. **Fix shape:** add an `error: string | null` prop to
`TaskTemplateSettings.vue` (the `TaskIdSettings.vue` precedent) and pass
`:error="templateAutosave.error.value"` from `TasksConfigTab.vue`; a
revert-on-failure story is a separate, smaller question here since both
fields are free text, not a persisted-state indicator like the toggle.


### GAP-97 · Low · Duplicate-ID detection compares decoded strings, not YAML scalar identity
`src-tauri/core/src/tasks/hierarchy.rs` (`ambiguous_ids`, line 26) counts ids
by their decoded Rust `String`. Since the subtasks increment, the WRITE path
deliberately preserves a parent's YAML scalar TYPE (`id::mirror_id_reference`,
so Obsidian/Dataview equality between `task-id` and `parent-id` holds), but
this read-side collision check never got the same type awareness. Two
directions fall out, and the second is the worse one:

- **False ambiguity (fails closed).** One Task with `task-id: 123` (a YAML
  NUMBER) and another with `task-id: "123"` (a STRING) decode to the same
  Rust `"123"`, so both are marked ambiguous: their children render as
  top-level orphans and either Task is refused as a prospective parent. Wrong,
  but it errs toward refusing — the direction this domain's defensive posture
  prefers.
- **False DISTINCTNESS (fails open — the more concerning half).** The strict
  reader does not decode tags, so `task-id: !!str 123` surfaces as the literal
  text `!!str 123` while a sibling's `task-id: "123"` surfaces as `123`. We
  treat those as two different ids and resolve a child confidently to one of
  them — but js-yaml resolves BOTH to the string `"123"`, so Obsidian and
  Dataview see a genuine collision we told the user does not exist. That is
  the one case here where we are confidently wrong rather than conservatively
  unhelpful.

**Why Low:** both need TWO hand-authored Tasks whose ids differ only in YAML
spelling — nothing Vault Buddy itself ever writes (generated ids are
letter-first base36, always plain). No corruption: nothing is written
incorrectly, and the false-ambiguity direction refuses rather than mis-links.
**Fix shape:** give the read side the same type awareness the write side has —
canonicalise to a type-tagged identity (resolved value + resolved YAML type)
before counting, rather than comparing display strings; or decide that a
non-string id is unusable and reject it up front, which is simpler and matches
`new_task_id`'s own letter-first rule. Whichever is chosen, `ambiguous_ids`,
`parent_index`/`parent_index_for_validation` and `count_parent_links` must all
adopt it together — they are the same rule seen from three places, and this
branch's most repeated defect was fixing one such site and leaving its siblings.

### GAP-98 · Low · An unread archived-list config reads as a CONFIRMED-empty one in the list badge
`src/utils/taskHierarchy.ts` (`openSubtaskCounts`, the
`archivedByVault.get(vaultId) ?? []` fallback). When `get_tasks_config` fails
for a vault, `loadVaultConfig` leaves that vault out of the map entirely, and
the `?? []` treats the absence as "this vault archives no lists" rather than
"we do not know". Any open child that IS in an archived list therefore keeps
counting toward its parent's open-subtask badge for the rest of the mounted
session — the badge disagreeing with the open-task counts beside it, which
apply the exclusion.

**Why this is filed rather than fixed, while the visually similar Task Detail
case WAS fixed:** the two differ on the axis this codebase draws everywhere —
*a view may degrade; a guard must refuse.* Task Detail's parent picker
**authorizes a write**: an unknown archived set there let the user persist a
relationship the rule forbids, and core deliberately does not validate archived
LISTS, so the frontend is the only enforcement point. That is a guard, so it now
gates on the config having actually resolved and offers a retry. The subtask
badge **displays a number**. A wrong count is visibly wrong, corrects itself on
the next successful load, and authorizes nothing. Gating it would mean either
suppressing a badge the user expects or plumbing an error state through a purely
presentational path, for a degradation the project's own rule permits.

**Fix shape, if it is ever worth it:** represent the missing entry as
*unresolved* rather than empty (`Map<string, string[] | undefined>` is already
the shape — the `?? []` is what erases the distinction), and have the count
either suppress itself or retry for an unresolved vault. Note the same `?? []`
idiom appears wherever a per-vault archived set is read; fix them together or
the surfaces will disagree in a new place, which is the defect this file already
records three facets of under GAP-91.

### GAP-99 · Low · TaskDetail's List picker does not gate on the archived-list config either
`src/components/TaskDetail.vue` (the `lists` computed feeding `TaskListPicker`
/ `draftList`). The third consumer of `archivedLists` on that surface, found by
the per-consumer audit the Add-Subtask gate prompted. Unlike the parent picker
and Add Subtask — both now gated on `archivedListsResolved` — this one still
reads the set while it may be unresolved or failed, so an archived list can be
offered as a destination.

**Deliberately left alone**, for three reasons that make it materially different
from its two siblings: it predates this whole increment (present since
`TaskDetail.vue`'s first commit, `a213873`); it is an **explicit user pick** of
a destination rather than a silent inherited assignment, so nothing happens
without the user choosing it and seeing it; and `TaskListPicker.vue` is shared
with `Tasks.vue`'s composer and editor, so gating it here would ripple into
surfaces this increment never touched. **Fix shape:** if it is taken up, gate at
the shared component rather than at this one call site, or the three consumers
drift apart again.
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
- ~~With Manual now the default sort, the aggregate "All tasks" view
  (`vaultId: null`) opens with drag grips and permits reordering rows within
  a merged, cross-vault list section.~~ — fixed 2026-07-16 (Codex P2, PR #59,
  `taskSort.ts:106`/`Tasks.vue reorderView`): `reorderView` now also requires
  `!isAggregate.value`, so the aggregate view never renders a grip and a
  cross-vault drag can no longer write a meaningless rank. Still true:
  `order` ranks are per-vault numbers (there is no cross-vault rank space),
  so a Manual-sorted aggregate view still DISPLAYS mixed-vault rows ordered
  by each task's own per-vault rank — a rank of `500` from one vault and
  `500` from another have no relationship to each other. That display-order
  quirk remains an accepted consequence of "drag-and-drop is the standard
  sort" (the user's explicit request); cross-vault list ordering remains a
  non-goal — only the reorder-writes-a-meaningless-rank half of this bullet
  is fixed.

### GAP-69 · Low · A "This Evening" sub-bucket and a distinct "Someday" planner bucket are deferred (each needs a second signal)
`src/utils/taskSections.ts` (`plannerBuckets`). The Plan grouping's five
buckets (Overdue / Today / Upcoming / Anytime / Done — see
`docs/superpowers/specs/2026-07-24-task-management-do-date-planner-foundation-design.md`)
are the do-date increment's full scope. Two finer time-horizon buckets some
task-planning tools offer were considered and deliberately deferred, not
forgotten — both need a second signal `scheduled`/`due` alone can't supply:
- **"This Evening"**, a same-day sub-bucket of Today separating "still to
  do" from "already handled for today," needs a time-of-day component —
  either widening `scheduled` past its plain `YYYY-MM-DD` shape or a second
  boolean/tag convention layered on top of it. Neither exists; adding one is
  a Task Model change, not a bucketing tweak.
- **A "Someday" planner bucket**, distinct from **Anytime**, needs a
  back-burner signal to key on: today "no do-date and no due-date" is the
  only test Anytime uses, so splitting off a lower-priority subset means
  either a new frontmatter flag or overloading `priority`/`tags` (e.g. a
  reserved `#someday` convention) — out of scope for a foundation increment.
  (Unrelated to a user hand-creating a **List** folder literally named
  "Someday" under the Lists grouping, which already works today — the
  deferred item here is a Plan-grouping BUCKET derived from dates, not a
  List.)
**Remedy, if ever pursued:** either addition is additive (a new optional,
leniently-read field) and would not change `scheduled`'s existing shape or
the other four buckets — but it is a follow-up increment, not a patch,
since it touches the Task Model.

### GAP-71 · Low · Plan-grouping buckets can shift a Task by a day at local midnight (accepted, by design)
`src/utils/taskFields.ts` (`localToday`) and `src/utils/taskSections.ts`
(`plannerBuckets`). The Plan grouping computes "today" from the local
wall-clock date (`new Date()`'s local getters, never UTC slicing) at the
moment a bucket is computed — the SAME rule the Rust-side `add_task`
already uses (`task_commands.rs`'s `chrono::Local::now().date_naive()`,
deliberately local rather than UTC per its own comment, so a task isn't
named with tomorrow's/yesterday's date near local midnight). Because the
panel is a long-lived webview (hidden/shown, never reloaded), a Task open
across local midnight can flip bucket without any write happening: a Task
in Today at 11:59 PM silently becomes Overdue at 12:00 AM the next time the
view re-renders (a re-sort/re-bucket — e.g. triggered by any other row's
optimistic write, or simply reopening the panel). **The row's relative
do-date LABEL has the same staleness:** `relativeDateLabel`
(Today/Tomorrow/weekday, `taskFields.ts`) also reads `localToday()` with no
reactive dependency, so a "Tomorrow" chip keeps reading "Tomorrow" after
local midnight until that row happens to re-render. **Accepted as a
deliberate design call (contested — Codex, PR #75):** it matches the
pre-existing due-date bucketing (recomputed against wall-clock "today" the
same way before this increment existed) and the local-not-UTC choice
`add_task` already made. Codex argued for driving "today" from a *reactive*
value that advances at local midnight (a shared reactive clock, or an
on-midnight / metronome-driven view refresh) — that is the proportionate fix
if this is ever reconsidered, and is NOT the same as freezing "today" for the
mounted view (freezing would make buckets AND labels silently stale across a
long session — strictly worse). Kept accepted for now on **low exposure**:
the panel is a transient popup that re-runs discovery on every open, so the
stale window requires it left open ACROSS midnight with no interaction.
Recorded so a future reviewer doesn't mistake the day-boundary flip for a new
bug; the accept-vs-reactive-clock call is a judgment this increment
deliberately left open.

### GAP-73 · Low · Quick-schedule popover: one remaining UX edge (rebucket focus) — (a)/(c) addressed
`src/components/TaskScheduleMenu.vue`, `src/composables/useTaskSchedule.ts`.
Genuine but non-trivial edges Codex surfaced on PR #75 AFTER the whole-branch
review, tracked here rather than reactively patched onto the completed increment
(each needed deliberate, possibly architectural work):
- **(a) Flip read one side only · ADDRESSED by the polish pass via a two-sided
  heuristic.** `shouldFlipUp` clipped against the `.panel-scroll` ancestor's
  bottom but never its top, and used a fixed ~200px estimate rather than the
  rendered popover — so in a compact scroll area where the popover fit BELOW
  the trigger but had even less room ABOVE, it could wrongly flip up and clip
  its first controls. Fixed the pragmatic way: `shouldFlipUp` now takes both
  the trigger's and the clip container's `top`/`bottom` and only flips up when
  there ISN'T room below AND there's MORE room above than below — never toward
  the tighter side. `toggle()` measures both rects (falling back to `0`/
  `window.innerHeight` when there's no `.panel-scroll` ancestor, as before).
  Regression-tested in `tests/task-schedule-menu.test.ts` (a case pinning the
  exact old bug — insufficient room below, even less room above, must NOT
  flip — fails against the prior one-sided implementation and passes against
  the fix). **The render-and-measure and Teleport options considered here
  originally were deliberately REJECTED as over-engineering** for a five-item,
  ~200px popover with one clip ancestor to track — this pass's review judged
  the two-sided heuristic proportionate to the actual bug (a comparison
  reading the wrong side), not a reason to add a portal + scroll-reposition
  layer to a presentational popover this small.
- **(b) Focus lost when scheduling re-buckets the row.** Choosing a date that
  changes the effective plan date (`scheduled ?? due`) moves the task to a
  different Plan bucket; the row `:key` is `bucketKey:path`, so Vue UNMOUNTS the
  row (and this popover) on the next render, and `closeAndRefocus`'s `nextTick`
  then finds `root` null → focus drops to `<body>` despite the restore logic. A
  real fix restores focus from the PARENT (`Tasks.vue`) onto the newly-mounted
  row's trigger, not from the unmounting child.
- **(c) Reschedule-overdue discards an open editor's unsaved draft · ADDRESSED
  by the polish pass.** `rescheduleOverdue` held out rows whose write was in
  `busy`, but a row merely OPEN in the inline editor (drafting, not yet saving)
  was NOT in `busy`, so the batch would reschedule it → it re-buckets to Today →
  the editor row unmounts → unsaved draft fields silently lost. The sharpest of
  the three (silent loss of typed input, however narrow the trigger: overdue AND
  open-in-editor AND reschedule-all clicked). Fixed via the scoped bounded fix:
  `useTaskActions` now tracks `editingPath` (the row's absolute path — kept
  alongside `editingKey`, not derived by splitting it, since both bucketKey and
  an absolute path can themselves contain `:`; set in `startEdit`, cleared in
  `cancelEdit` and `onEditorSave`), and `Tasks.vue` threads it into
  `rescheduleOverdue(bucket.tasks, editingPath)`. The composable excludes any
  task whose `path === editingPath` from `targets` exactly like a busy row and
  names it in the summary toast ("still overdue") — regression-tested in
  `tests/task-schedule.test.ts`.

(a) and (c) landed in the do-date polish pass; (b) remains open and is
architectural (cross-component focus restoration) — none blocked the shipped
do-date foundation (whole-branch review: merge-ready). Codex, PR #75.

### GAP-74 · Low · Fallow quality-baseline loosened for the do-date increment (documented)
`scripts/quality-baseline.json`. The do-date/planner increment (PR #75) moved two
shrink-only fallow counters, each a reviewed, deliberate trade-off. Codex flagged
(P1) that the reasons lived only in commit messages, not discoverably in-repo —
this entry (linked from the baseline file's `description`) is the durable record:
- **averageMaintainability 91.8 → 91.7** (commit 371a60f): the additive do-date
  chip single-sourced `shortDate`/`relativeDateLabel` into `taskFields.ts`
  (required to keep the duplicate-code/clone gate green — two `MONTHS` arrays
  would trip it) and added one conditionally-rendered element to `TaskRow.vue`'s
  template. The pre-change tree measured exactly 91.8 (zero slack), so any change
  touching those files would trip the floor regardless of implementation. Still
  the baseline today — this half of the loosening stands.
- **complexFunctions 13 → 15 (commit 64e876a) · RETIRED by the polish pass.**
  `TaskEditor.vue::buildPatch` (CRAP 43.1) and `useTaskActions.ts::applyFieldPatch`
  (CRAP 31.6) had crossed the CRAP-30 gate when the do-date's set/clear branch was
  mirrored beside `due`'s — "wide, not deep" complexity (parallel field blocks, no
  added nesting); `criticalComplexity` stayed 4 and every other fallow counter was
  unchanged. The deferred follow-up landed: both functions now call a shared pure
  helper (`diffDateField` in `TaskEditor.vue`, `applyDateField` in
  `useTaskActions.ts`) that collapses the duplicated `due`/`scheduled` blocks into
  one call per field, dropping both back under the CRAP gate.
  `complexFunctions` is back to 13 in the committed baseline; behavior is
  byte-identical (task-editor.test.ts + tasks.test.ts unchanged and green).

### GAP-75 · Low · Inline editor's `type="date"` inputs can't display a shape-valid but calendar-invalid do/due date (accepted, symmetric with `due`)
`src/components/TaskEditor.vue`. The task contract deliberately accepts a
shape-valid but calendar-invalid date like `2026-02-31` (`is_valid_due` checks
only the `YYYY-MM-DD` shape, matching Obsidian's own tolerant date picker; core
returns it unfiltered for `scheduled` too). The editor's Do and Due fields are
native `<input type="date">` controls, which cannot represent such a value — the
browser sanitizes it to an empty display, so the editor shows the stored date as
*unset* and the user can't see or directly round-trip it. Codex flagged this on
the `scheduled` input (PR #75); it applies **identically to `due`**, which has
used the same control since the tasks-todo-list increment — this is a
pre-existing, symmetric edge, not a do-date regression.
- **Not data loss.** `editScheduled`/`editDue` are seeded with the raw stored
  value; a programmatic `v-model` set fires no `input`/`change` event, so the ref
  keeps `2026-02-31` even while the field renders empty. An untouched save
  early-returns from `diffDateField` (`draft === original`), so nothing is
  written and the value is preserved on disk. Only if the user actively edits the
  empty-looking field does it change — an explicit action, not silent loss.
- **Why accepted, not fixed:** swapping the native picker for a free-text control
  (the only way to display every accepted value) would regress the common-case
  UX for both fields to serve a value that essentially only arises from
  hand-authoring frontmatter. The proportionate remedy is to re-enter a valid
  date if you want to edit such a task from the panel; the calendar-invalid value
  otherwise keeps rendering correctly on the row (`relativeDateLabel` falls back
  to the literal `shortDate`, see the Task 5 guard) and round-trips untouched.

### GAP-76 · Low · The composer/editor options row crowds at the 400px compact panel preset
`src/components/TaskComposer.vue` (the `showAddOptions` row) and the sibling
Due/Do/priority row in `src/components/TaskEditor.vue`. The do-date/planner
increment added a second labeled date input (**Do**) beside the existing
**Due** input on a single `flex items-center gap-1` (nowrap) row that also
holds three priority buttons and a tags input. All three inputs are
`min-w-0 flex-1`, so at the 400px **compact** preset (≈356px usable after
gutters/card padding) they shrink below a usable width — a native `type="date"`
control can't show a full date at ~60px, and the tags input loses most of its
text. The comfortable/large presets have room; only compact is affected, and
the row is behind the `⋯` options toggle. Codex flagged the composer (PR #75,
`TaskComposer.vue:262`); the editor's row shares the shape.
- **Why not fixed in the increment:** the fix is a responsive-layout change —
  add `flex-wrap` and give the date inputs a real min basis (removing `min-w-0`
  so they wrap instead of collapsing), or split dates onto their own row — whose
  correctness is purely *visual*. The frontend test stack is happy-dom, which
  does no layout, so no automated test can confirm the result looks right across
  the three presets (compact/comfortable/large). Shipping a blind CSS change on
  an already-green, reviewed branch trades a P2 nit for unverifiable visual risk.
- **Recommended fix (needs visual verification):** container
  `flex flex-wrap items-center gap-1`; the two date inputs `flex-1 basis-32`
  (≈128px floor) instead of `min-w-0 flex-1`; keep the tags input
  `min-w-0 flex-1`. Apply to BOTH the composer and editor rows so they stay
  consistent, then eyeball all three presets. Pairs naturally with the tracked
  GAP-65 `Tasks.vue`/composer refactor.

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

### GAP-102 · ~~Low~~ FIXED 2026-09-19 · `vault_buddy_screen` did not run its tests in `windows-app`
`.github/workflows/ci.yml`. **Resolved as of this branch, for the coverage
half:** the `rust-core` job's `cargo llvm-cov` line now includes `-p
vault_buddy_screen` in its `--fail-under-lines 94` floor (verified locally:
95.58% lines with the crate included — `clock.rs`/`select.rs`/`engine.rs`
all 100%, `lib.rs` 82.61%). **What remains:** the `windows-app` job's
post-build `cargo test` line (`-p vault_buddy_core -p vault_buddy_capture -p
vault_buddy_transcribe`) still does not include `-p vault_buddy_screen`
(the `windows-app`/`rust-core` clippy and non-coverage test lines already DO
include it, per the CI-gating commit that added the crate). **This omission
is correct as of Phase 1**: `screen::engine` (`src-tauri/screen/src/
engine.rs`) is a deliberate, platform-independent Phase-1 stub — one
unconditional `Err(ScreenError::Unsupported)` body with no Windows-only code
path yet — so there is nothing Windows-specific yet for `windows-app`'s Rust
tests to exercise that `rust-core` doesn't already. **The gap:** the moment
Phase 2's `cfg(windows)` split lands real capture logic in `engine.rs` (spec
§4.1, §6), Windows becomes the *only* place that code can execute at all,
and this CI edit will not have been revisited automatically — a
Windows-only regression in the capture engine could land with `windows-app`
never having run its tests, even though `rust-core`'s coverage floor now
covers the crate's Linux-testable surface. **Fix shape:** add `-p
vault_buddy_screen` to the `windows-app` test line in the same PR as Phase
2's `cfg(windows)` split — not before, since there would be nothing
Windows-specific yet to exercise. Owner: Phase 2.

**FIXED 2026-09-19, on exactly those terms.** Phase 2's `cfg(windows)` split
landed (`sink.rs`, `source.rs`, `session/windows_session.rs`; the Phase-1
`engine.rs` stub this entry was written against is gone, replaced by
`session`), so Windows is now the only place that code can execute — and the
`windows-app` job's post-build test line became `cargo test -p
vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p
vault_buddy_screen` in the same PR. The comment above that step in
`.github/workflows/ci.yml` records why the crate was absent until now, so a
future reader does not "clean up" the list back. **What this does NOT close**
is the coverage of those Windows arms themselves — the crate's tests are the
pure modules', and the `cfg(windows)` bodies still execute nowhere automated;
that honest statement is GAP-117, and manual Windows verification is the only
thing that exercises them.

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
- `services::tasks::mod.rs`'s `assert_root_if_exists` — the tasks-domain
  wrapper every task read/write command calls before touching a vault's
  tasks folder, gating `capture_paths::assert_root_inside_vault` behind
  `root.exists()` — has no test constructing a `tasksFolder` that resolves,
  via symlink, outside the vault while the folder is actually present on
  disk. Verified for this task report: replacing its body with an
  unconditional `Ok(())` (a global no-op) leaves all 693 core tests green.
  Pre-existing (not a regression introduced by this branch's fixes) — the
  guard itself is real (`assert_root_inside_vault` does canonicalize and
  compare), simply unexercised by anything in the suite.
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
- `UpdateSettings.vue` now has direct coverage (`update-settings.test.ts`:
  the manual-check control + the "View update →" link), and the update view
  itself is covered by `update-view.test.ts`. `HighlightText.vue` is still
  tested only via the util's tests.
- `SelectMenu.vue`'s tests now cover the keyboard path (GAP-33, fixed
  2026-07-10) but not outside-click close or positioning.
- Event-listener cleanup paths in the roots and `capture.init()` re-entry
  (GAP-32) have no tests.

## 8. Tech debt & duplication

### GAP-66 · Frontend · Design-system migration
~~The rest of the frontend still uses raw utility strings (the compact
tasks-view controls and the ~18 other components).~~ **The token/primitive
migration is FIXED 2026-07-23.** Every panel component now speaks the
semantic-token vocabulary, and each bespoke pattern that is a clean drop-in
was converted to its primitive (`Banner` ×6 error strips, `Field` ×2
inputs, `Spinner` in `UpdateView`, `IconButton` in `Transcriptions`,
on top of the earlier `ActionPanel`/`VaultList`/`TaskRow`). The compact
tasks-view controls took the documented 1:1 token-swap (not a
size-changing primitive conversion). The raw utilities that deliberately
remain (settings-card `<h2>` header idiom vs the `SectionHeader` primitive;
segmented/mode-card/tab selectors with active `border-violet-400`; themed /
dense buttons and custom-handler inputs that match no primitive; and colors
with no token — `accent-violet-500`, `rose-*`, `amber-*`, toast variants,
`violet-100`/`slate-200`, active-state `violet-400`) are NOT debt — see the
"Raw utilities that deliberately REMAIN" note in AGENTS.md § Frontend state
→ UI primitives. A justified -0.1 `averageMaintainability` baseline move
(91.8→91.7) covers the shared-primitive import overhead.

Still deferred from the design-system increment (separate polish, NOT the
consistency migration): the empty-state polish (the bare `<p>` "Obsidian
not found" / "no vaults match" states in `ActionPanel` → `EmptyState`), an
optional reduced-motion-safe cross-view fade, and the light-touch
panel-density opportunities — the list-view banner stack (up to six banners
can stack above the vault list) and `VaultList`'s five-per-row row actions
plus the still-bespoke favorite star.

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

### GAP-70 · ~~Low~~ FIXED 2026-07-25 · `RESERVED_TASK_KEYS` was duplicated verbatim across two guard sites
Two `const RESERVED_TASK_KEYS: &[&str]` arrays (the template-frontmatter
filter and the id-property validator) were kept aligned only by a pair of
`// keep in sync with <other file>::RESERVED_TASK_KEYS` comments — a reviewer
convention, not a compiler-enforced one. The Task Detail increment's move of
one array (`disk.rs`→`create.rs`) left the other's sync comment pointing at a
file that no longer held the const, half-breaking the only guard (Codex, PR
#76). **Fixed:** hoisted a single `const RESERVED_TASK_KEYS` into
`src-tauri/core/src/tasks/mod.rs`, referenced as `super::RESERVED_TASK_KEYS`
by both `create::render_task` (template filter) and `id::is_valid_id_property`
(id-property validator) — the two can no longer drift, and the id.rs tests
that iterate every reserved key guard against a silent removal. Mechanical, no
behavior change.

### GAP-72 · Low · `due` stays an unfiltered raw scalar in `TaskItem`/`TaskDto`; `scheduled` is core-validated at the same boundary
`src-tauri/core/src/tasks/list.rs` (`collect_task_file`). Line 129,
`let due = scalar_field(&content, "due");`, carries whatever raw scalar
sits in the frontmatter straight onto `TaskItem.due`/`TaskDto.due` — an
unparseable value (anything not plain `YYYY-MM-DD`) passes through
unfiltered. Line 133 (added by the do-date increment), `let scheduled =
scalar_field(&content, "scheduled").filter(|s| is_valid_due(s));`, does the
opposite: an unparseable `scheduled` becomes `None` right here, at the
DTO/MCP boundary, per the field's own doc comment on `TaskItem.scheduled`
("Read then validator-filtered... an honest DTO/MCP boundary" — already
called out in-code as the deliberate choice, Codex PR #75) but never
applied back to `due`. **Consequence:** `due`'s honesty currently lives
only in each CONSUMER — the frontend's `dueOf()` (`src/utils/taskFields.ts`)
re-filters it client-side, and the Rust-side sort's `due_key` filters it
again for ordering — so an MCP client or any other `TaskDto` consumer
reading `due` directly gets an un-validated raw value, while the same
consumer reading `scheduled` gets a pre-filtered guarantee. **Why left
alone this increment:** `due`'s DTO/MCP contract predates this work;
filtering it now is a compatibility change (a malformed `due` a consumer
currently sees verbatim would become `None` instead) that deserves its own
reviewed change, not a side effect of adding `scheduled`. **Fix, if
pursued:** apply the same `.filter(|d| is_valid_due(d))` to `due` in
`collect_task_file`, then drop `due_key`'s redundant re-filter and
`dueOf`'s client-side re-check once the DTO itself is honest — landing both
fields on one consistent contract.

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

### GAP-103 · FIXED (2026-09-21) · `set_screen_capture_config` (Phase 6) inherits three obligations already visible from Phase 1's config work
Deferred-by-phasing follow-ups the settings command must not land without,
found while wiring the seven Screen Capture config fields into
`vault_config.rs`/`config_merge.rs` (spec §12) ahead of the command that
will own writing them:
- **Normalize `screen_fps` on write, not only on read.** `vault_entry`
  parses `screenFps` through `screen_capture_config::normalize_fps`
  (line ~411), so an in-memory `VaultCaptureConfig` can only ever hold 30 or
  60 — but `serialize_vault_entry` (line ~521) hardcodes `if v.screen_fps !=
  30` and writes the raw field verbatim, with no call to `normalize_fps` on
  the way out. This is unreachable **today** only because every DTO the
  serializer sees came from a normalizing parse; the moment
  `set_screen_capture_config` accepts an `fps` straight from the frontend's
  IPC payload (bypassing `vault_entry`'s parse), an out-of-range value like
  `45` would serialize as `screenFps: 45` and silently normalize to 30 only
  on the NEXT read — a save-then-immediately-re-read-without-restart
  mismatch. The command must normalize its incoming DTO value before it
  ever reaches `serialize_vault_entry`.
- **Document the seven keys in `docs/DEVELOPMENT.md`'s `config.json`
  reference.** This phase deliberately left that doc untouched (Phase 1 is
  core-only, no settings UI or IPC surface yet) — `screenCaptureFolder`,
  `screenCaptureDateFolders`, `screenQuality`, `screenFps`,
  `screenCreateNote`, `screenExtraFrontmatter`, `screenBodyTemplate` are
  parsed and round-tripped today but appear in no human-facing config
  reference, mirroring the precedent every other settings command's launch
  PR follows (`mcp`, `document_import`, `transcription` sections are all
  documented there).
- **Take `capture_config::config_write_lock()`.** `set_screen_capture_config`
  is a seventh settings surface writing into the same `VaultCaptureConfig`
  struct capture/transcription/documents/tasks/task-id/task-lists/
  task-template settings already share — precisely the struct GAP-83's
  single-lock-plus-per-file-lock discipline exists to protect. Landing it
  under any lock other than the one core lock (or under no lock) would
  reopen the exact desync GAP-83 closed, just for an eighth writer instead
  of a seventh.

**FIXED** by `src-tauri/src/screen_config_commands.rs` +
`src/components/ScreenCaptureConfigTab.vue` (the **Screen** tab of Vault
settings), after a hardware session reported the absent surface as a defect
rather than a deferral. All three obligations are met, but the FIRST was met
in the opposite direction from the one this entry proposed, which is worth
recording so nobody "restores" it:

- **fps is REFUSED, not normalized on write.** The entry asked the command to
  normalize its incoming value before it reaches `serialize_vault_entry`.
  It refuses anything but 30 or 60 instead. Normalizing is right for the
  PARSE layer — a hand-edited `config.json` must still open the app — and
  wrong for a settings screen, where the user is looking straight at the
  control and a value that silently became something else is exactly how a
  setting reads as broken. The serializer hazard the entry described is
  closed either way: an out-of-range value now never reaches it.
  `screen_quality` is refused on the same reasoning.
- **The seven keys are documented** in docs/DEVELOPMENT.md's `config.json`
  reference, with the two non-obvious ones spelled out: `screenQuality`
  applies only to an EDITED save (an untouched one is `-c copy`'d as
  recorded) and `screenFps` to the NEXT recording, not one already staged.
  Both are also stated on the controls themselves.
- **`config_write_lock()` is taken**, read-modify-write under it, via a new
  `config_merge::merge_screen_owned` — the seventh merge helper, so a screen
  save cannot reset another domain's fields or be reset by one.

Two notes for whoever extends it:

- `merge_screen_owned` takes a NAMED `ScreenOwned` struct, not seven
  positional arguments. `date_folders` and `create_note` are both `bool` with
  two unrelated fields between them, so a transposed positional call would
  have compiled and written each user's choice into the other's setting.
- An eighth `screen_*` field added to `VaultCaptureConfig` and forgotten in
  the merge would be silently unwritable through the only surface that offers
  it, with every save restoring the old value. A test counts the declarations
  against the assignments so that cannot happen quietly.

**Residuals, none of them this entry's:**

- The tab is reachable only from **Vault settings**, not from the Record
  Screen picker where a user is actually choosing a source. Nothing links the
  two.
- Changing `screenFps` or `screenQuality` says which capture it affects, but
  nothing in the app shows what a STAGED capture was recorded at, so the
  sidecar remains the only record.
- `screenExtraFrontmatter` is validated only at render time by
  `render_extra_frontmatter` (malformed YAML yields `""`). The tab accepts any
  text and reports nothing, so a typo is discovered as a note that quietly
  lacks the frontmatter — the same posture as the three sibling template
  fields, not a regression, but the one place a preview would earn its keep.

Owner: Phase 6 (the settings-surface phase). None of these are bugs in
Phase 1's landed code — `config_merge.rs::merge_capture_owned` already
carries the `// Screen Capture is owned by set_screen_capture_config
(phase 6)` comment above the fields it preserves — they are obligations
the next phase inherits and must not skip.

### GAP-105 · Low · `CaptureClock` is the repo's third pause-elapsed accumulator
`src-tauri/screen/src/clock.rs`'s `CaptureClock` (`started` + `paused_total`
+ `paused_since`, idempotent `pause`/`resume`, `elapsed`) implements the
exact same "wall clock minus accumulated paused time" rule as two existing,
independently-written carriers of it:
- `src-tauri/capture/src/session.rs` (~254-276) — the audio mixer loop's
  inline `paused` / `paused_total: Duration` / `pause_started: Option<Instant>`
  locals, folded directly into the `Control::Pause`/`Control::Resume` match
  arms of the device thread's `loop`.
- `src-tauri/src/capture_commands.rs` (~756-760) — the shell's UI-facing
  mirror, `paused_total_ms: u64` / `paused_since_ms: Option<u64>` on the
  active-recording struct, updated by the `pause_capture`/`resume_capture`
  commands.
- `src-tauri/core/src/tasks.rs` and `src-tauri/screen/src/clock.rs` both now
  carry unit tests pinning "idempotent pause", "idempotent resume", and
  "accumulate across multiple pause cycles" as three *separately* proven
  invariants — proof the same rule is worth centralizing once, not proof it
  already is.

None of the three is a drop-in replacement for either of the others as it
stands: `session.rs`'s locals are loop-scoped (no struct to extract without
restructuring the device thread), and `capture_commands.rs`'s pair is a
millisecond-valued UI mirror the frontend reads directly, not a computation
site. The spec for this increment mandated `CaptureClock` as a new,
A/V-sync-driving abstraction (see its module doc: "there is no second time
base to drift from"), so introducing it was correct — but the underlying
pause/resume/total rule the repo encodes now exists in three independently
maintained forms, which is exactly the drift class `vault_walk.rs`'s own
header already warns about for a different pair of duplicated walks
(GAP-46). Once Phase 2 wires `screen::clock` in and the session/UI mirror
pair are touched again, consider whether `CaptureClock` (already the most
general and best-tested of the three) should become the ONE home for this
rule, with the audio session and the UI mirror built on it or on a shared
sibling type, rather than a third independent implementation living
alongside the other two indefinitely.

### GAP-106 · Low · Units differ at the seams Phase 2 will join: ms, `Duration`, and seconds
Three time-value representations meet at the boundary Phase 2 wires
together, with no documented conversion between them:
- `core::timeline` (`Segment::duration_ms`, `Timeline::output_duration_ms`/
  `split_at`/`to_source_ms`) and `screen::select` (`PlanSpan::duration_ms`,
  `plan`) both use `u64` milliseconds.
- `screen::clock`'s `CaptureClock` uses `std::time::Duration` throughout
  (`elapsed`, `output_ts`) — deliberately, per its module doc, so pause
  edges are `Instant`-injected and unit-testable, not because it shares the
  timeline/select modules' convention.
- `ScreenNoteMeta::duration_secs` (`core/src/screen_note.rs`) is `u64`
  seconds — matching the existing `capture_note::NoteMeta::duration_secs`
  precedent (`core/src/capture_note.rs`) and `transcript::TranscriptMeta`'s
  same field, so the seconds form is not new, only the screen-capture use
  of it.
The ms↔seconds hop already has a precedent to follow (whatever
`capture_note`'s callers do today to go from a recording's millisecond
duration to `NoteMeta.duration_secs`); the ms↔`Duration` hop between
`core::timeline`/`screen::select` and `screen::clock::CaptureClock` is the
genuinely new seam this feature introduces, and nothing today converts
between them (Phase 1 has no caller that needs to). Before Phase 2 wires
the clock's `Duration` output into a `Timeline`/`PlanSpan`'s `u64` ms field
by hand at each call site, add one small, documented, unit-tested
conversion helper (e.g. `Duration` → ms via `as_millis` truncation, with a
comment on which direction is lossy and why truncation — not rounding — is
the right choice for a monotonically-advancing output timestamp) so every
call site converts the same way instead of five ad hoc `.as_millis() as
u64`s drifting apart.

### GAP-107 · Low · Overflow posture is inconsistent between `core::timeline` and `screen::select` (cosmetic)
`core/src/timeline.rs`'s `split_at` (line ~69) and `to_source_ms` (line
~116) both compute `elapsed + seg.duration_ms()` with a plain `+`, while
`screen/src/select.rs`'s `plan` (line ~43) computes the equivalent running
offset with `output_start_ms.saturating_add(seg.duration_ms())`. Both
values are milliseconds-since-recording-start on a `u64`, which overflows
only after roughly 584 million years of continuous accumulated segment
duration — unreachable in practice, so this is not a correctness bug. It
is worth a consistent posture (both saturating, or both plain, with a
comment on why) the next time either module is touched, so a future
reader does not read the difference as meaningful when it isn't.

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

### GAP-108 · Medium · `sanitize_title` does not neutralize Windows reserved device names
`src-tauri/screen/src/staging.rs` `sanitize_title` maps reserved *characters*
(`: \ / ? * " < > |`) and collapses separator runs, but leaves the reserved
*device names* untouched: `sanitize_title("CON")` returns `"CON"`.

Win32 reserves `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9` and `LPT1`–`LPT9`
**with or without an extension** — `CON.mp4` is as unopenable as `CON`. Vault
Buddy ships only on Windows, so a user who titles a screen capture "CON" (or
"nul", the match is case-insensitive) gets a capture that cannot create its
own staged file. The failure surfaces at `File::create` inside the capture
session, i.e. after the user has already started recording.

Found by the Task 2 review (phase 2). Not the implementer's error: the case is
absent from the plan's own Step 2 test list, so it is a plan omission rather
than a deviation.

**Still open after phase 4, and the boundary of what phase 4 closed matters.**
Phase 4 introduced `staging::is_reserved_device_stem` — a public,
case-insensitive, any-extension predicate over `CON`/`PRN`/`AUX`/`NUL`/
`COM1`–`COM9`/`LPT1`–`LPT9` — and `editor_commands::is_safe_base` consumes it,
so no name arriving from the frontend can become a device path. Phase 4 also
gave `write_sidecar` a structural containment check (it compares the joined
path's `parent()` against the staging dir, which catches a separator, a `..`
and a Windows drive prefix in one comparison). **Neither of those is this
entry.** `sanitize_title` and `reserve_base` — the NAMING path, where a
window title first becomes a base — still do not consult
`is_reserved_device_stem`: `sanitize_title("CON")` is still `"CON"`, so a
capture of a window titled that still fails at `File::create`, after the user
has started recording. The list exists now; the naming path simply does not
read it.

Two related path-safety items belong with it:

- `reserve_base` still does not re-assert that `base` contains no path
  separators; it trusts the caller sanitized first, and unlike `write_sidecar`
  it was not hardened. It only probes `.exists()`, so this remains
  defence-in-depth rather than a live bug while the only caller is the
  sanitizing one.
- Two distinct titles can collide onto one sanitized base (`"a:b"` and
  `"a/b"` both become `"a-b"`). That one IS handled, by `reserve_base`'s
  ` (N)` suffix retry — recorded here only so a future reader does not
  re-discover it as a bug.

**Fix shape:** have `sanitize_title` rename rather than merely refuse — the
open question this entry has always carried is what `"CON"` should BECOME
(`"CON_"`? `"Capture"`?), which is why it belongs with the capture session's
write-site hardening and not with the editor's input validation, where
refusing is the right answer and is already done.

### GAP-109 · Medium · A reused HWND can resolve to a different live window than the one picked
`src-tauri/screen/src/source.rs`, `resolve`'s `SourceId::Window` arm validates
the stored handle with `Window::is_valid()` only — visible, not a tool window,
not a child (windows-capture 2.0.1, `window.rs`). Windows reuses HWND values
after a window closes, so between the user picking a window and pressing Start
the same handle can belong to a DIFFERENT live window. The capture then records
the wrong window with no error at all.

**The obvious remedy is rejected deliberately.** Re-checking the title at
resolve time would throw `SourceGone` on windows that merely renamed
themselves — browsers on a tab change, editors on a file switch, anything
showing a document name. That is a frequent, everyday false failure traded
against a rare silent one, and it is the worse bargain. A process-id re-check
is the more promising direction (it catches the cross-process reuse case
without punishing a rename) but needs the pid captured at list time, which the
current `CaptureSourceInfo` does not carry.

Sibling bug, already FIXED in the same review (723abfd): the SCREEN arm had a
worse version of this — `list_sources` minted ids from `Monitor::index()` (the
OS display number in `\\.\DISPLAYn`) while `resolve` looked them up with
`Monitor::from_index()` (positional into `enumerate()`). Those are different
numbering schemes, so a multi-monitor machine could resolve to the wrong screen
on the very first call, with no staleness required. Now resolved by scanning
for a matching `.index()`.

Both belong on the Windows verification checklist: pick screen 2 and confirm
screen 2 is what recorded; pick a window, close it, and confirm the capture
refuses rather than recording something else.

### GAP-110 · Medium · The 15 s screen-capture ready timeout frees the guard while the capture may still be starting
`src-tauri/src/screen_capture_worker.rs`, the `Err(_)` arm of
`ready_rx.recv_timeout(READY_TIMEOUT)`. The behaviour is brief-mandated
("release the guard and fail cleanly"), so this is a design gap, not a
deviation — but the design has two residuals worth naming before Phase 5.

The timeout was justified in-code by the claim that no file can exist yet.
**That claim was false and is now corrected** (same commit): `run_mux`
creates the sink — which opens the `.part` — and only THEN sends on
`ready_tx`, and `ScreenSession::start` blocks on that handshake
(`src-tauri/screen/src/session/windows_session.rs`, the `ready_rx.recv()`
before `spawn_producers`). So the sink exists strictly BEFORE `start`
returns success, and the 15 s timeout can fire with an open `.part` on disk.

Two residuals follow:

1. **An orphan with no recovery path.** The commonest shape is a device
   thread still inside `open_selected_sources` (a wedged audio driver — the
   documented premise for the audio domain's own timeout) when the 15 s
   expires. The start path releases the reservation and returns an error.
   The device thread later succeeds, creates the sink, records briefly, sees
   the pre-emptively-sent `Control::Stop`, finalizes, and RENAMES to a staged
   `.mp4`. Its `done_tx.send` lands in a dropped receiver (the monitor is
   spawned only on the success path), so no sidecar is written and no
   `screen:stopped` is emitted. Nothing sweeps it: there is no
   `run_screen_recovery` anywhere, and `capture/src/recovery.rs` sweeps vault
   recording roots, not `%LOCALAPPDATA%\…\screen-captures`. Spec §10's
   staging recovery is Phase 5. No user data is lost (the file is near-empty
   and un-resumable without its sidecar), but staging grows silently.
2. **A mutual-exclusion window.** Between the timeout and that old device
   thread finishing, `CaptureGuard` is FREE. The user sees "Screen capture
   did not start in time.", retries, and the second capture opens the same
   audio endpoint while the first is still opening or recording it — the
   exact reliability hazard spec §7.3 says the guard exists to prevent. The
   pre-emptive `Stop` narrows this window but cannot close it: the first
   thread is by definition not yet reading its control channel.

**Two candidate fixes, both bigger than a comment.** Either hold the claim
until the device thread reports terminally — return the error to the user
without freeing the guard and let the monitor free it, which costs a monitor
spawned on the timeout path too — or bring a minimal staging janitor forward
from Phase 5, which fixes (1) but not (2). Residual (2) is the one that
argues for the first option. For the plan owner to decide; until then the
timeout arm's comment states the real behaviour rather than the invariant it
does not have.

### GAP-111 · Low · One Phase-2 screen-capture surface deliberately falls short of the approved spec
`src/components/ScreenAudioPicker.vue`. The phase-2 source picker shipped three
knowing deviations from the screen-capture spec. **Two are closed** — phase 3
task 7 added the Region tab and restored the chooser hint (see the closed items
below) — and one remains, recorded here so a reader diffing the shipped picker
against the spec finds the reasoning instead of assuming an oversight.

1. **§7.2's per-device audio level bars are omitted** (`ScreenAudioPicker`).
   Nothing could feed them: `capture:level` is emitted from exactly one place,
   `src-tauri/src/capture_commands.rs`'s audio path, and only *while an audio
   recording runs* — so it could not drive a meter on a pre-start picker even
   if the screen domain listened for it. `screen_capture_worker.rs` forwards
   `screen:warning` and `screen:frames` only, and `src-tauri/screen/src/**`
   has no level plumbing at all. A meter fed by nothing is a permanently dead
   indicator, which is worse than none (the VAD stats-row lesson: never render
   intent as engagement). **Restoring it is a Rust change first** — a
   per-device level emit that runs during enumeration/preview, not a frontend
   one — so phase 3 should not "add the bars" against the current event set.
2. ~~**There is no Region tab** (`ScreenSourcePicker`'s `TABS`).~~ **Closed by
   `b78648c` (phase 3 task 7).** The Region tab ships: it lists the monitors as *targets*
   (a region lives on exactly one monitor and the overlay covers exactly one),
   opens `select_capture_region` on the picked one, and renders the result as a
   selectable row. The inverted pin is `tests/screenSourcePicker.test.ts`'s
   "offers Screen, Window and Region tabs", which also asserts all three tab
   labels.
3. ~~**The chooser hint reads "Screen or window"**~~ **Closed by `b78648c` (phase 3 task
   7).** `RecordMode`'s `OPTIONS` now reads "Screen, window, or region", §7.1's
   own wording, pinned in `tests/record-mode.test.ts` in both the visible text
   and the aria label.

### GAP-112 · Low · The static-screen heartbeat repeats at a fixed 500 ms rather than adapting
`src-tauri/screen/src/session/mod.rs` (`HEARTBEAT: Duration =
Duration::from_millis(500)`) and `session/pacing.rs`
(`VideoPacer::on_idle` / `should_repeat`). When no new frame arrives — a
completely still screen — the mux repeats the last frame every 500 ms so the
output timeline keeps advancing and fragments keep closing (without it a
still screen recorded alongside a microphone closes no fragments at all,
which is the regression `the_wait_shrinks_so_the_heartbeat_fires_even_while_
audio_keeps_arriving` pins). The interval is a constant: it does not widen
when the screen has been still for a long time. **Failure scenario:** a
one-hour screencast of a mostly-static slide costs ~7 200 repeated frames of
bitrate it did not need; every one is a P-frame of a scene identical to its
predecessor, so the cost is small per frame but entirely avoidable.
**Not a correctness bug** — the repeats are what make a still stretch play
through at a steady rate at all (checklist item 9) — and the fixed interval
is also what makes the pacer unit-testable on Linux with an injected
`Instant`. **Fix shape:** back off the interval while the screen stays still
(500 ms → 1 s → 2 s, resetting on the first real frame), keeping the cap
below whatever fragment duration the editor's seek granularity needs; the
change belongs in `pacing::VideoPacer` where it stays pure and testable, not
in the Windows arm. Defer until Phase 4/5 knows what fragment cadence the
editor actually wants.

### GAP-113 · Low · Audio and video derive their timestamps from different sources; no drift measurement exists
`src-tauri/screen/src/session/pacing.rs`. Video timestamps come from the
wall clock — `CaptureClock::output_ts`, stamped by `VideoPacer::on_frame` /
`on_idle` — while audio timestamps come from the **emitted sample count**
(`AudioPacer::take` → `audio_ts(self.emitted, self.rate)`, a running total
divided by the nominal `AUDIO_RATE`). Each is individually right: the video
side must track real time or a pause would not be excisable, and the audio
side must be contiguous or the sink would see a gap. **Failure scenario:** if
a device's *actual* sample rate differs from its nominal one (a cheap USB
interface clocking 47 980 Hz while reporting 48 000, a resampler with a
systematic bias), the two time bases diverge linearly — roughly 0.04 % is
~1.5 s over an hour. Short captures are unaffected; a long one would drift
audio out of sync with video, progressively, with nothing in the pipeline
noticing. **There is no measurement yet.** Item 10 of the Windows
verification checklist
(`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`)
is where the first real number comes from; deciding a fix before that number
exists would be guessing. **Fix shape, if a real drift is measured:** stamp
audio from the shared `CaptureClock` too and let the sample count only fill
gaps, or resample against the observed rather than nominal rate. Both are
bigger than this phase and neither is justified by evidence yet.

### GAP-114 · Low · A full mux channel drops frames rather than blocking; a sustained encoder stall silently degrades the frame rate
`src-tauri/screen/src/frames.rs` (`try_send` → `TrySendError::Full` →
`drop_frame("the muxer is behind")`). The WGC frame callback hands frames to
the `screen-mux` thread over a bounded `SyncSender` and uses `try_send`, so
when the mux is behind — a software H.264 fallback on a 4K/60 source, a
stalled disk — the frame is **dropped and counted**, never queued. This is
deliberate: blocking the WGC callback thread back-pressures the compositor
itself, and an unbounded queue turns an encoder stall into unbounded memory
growth on 4K BGRA frames. **Failure scenario:** the recording silently runs
at, say, 22 fps instead of 60 with no failure anywhere — the file is valid
and plays, it is simply choppier than the user asked for. **The mitigation,
which is not a fix:** the drop is counted and surfaced three ways — a
`log::warn!` on the first drop and one in every 300
(`pacing::should_log_drop`), the running total in the `screen:frames` event
(~2 Hz), and `screen capture: finalizing after N dropped frame(s)` at
teardown. Spec §17.3 deliberately leaves the RESPONSE (drop to 30 fps? warn
the user?) to be decided from real measurements rather than guessed, which is
what checklist item 10 produces. The running total is visible in the UI as of
2026-09-19 (`ScreenCaptureBar.vue`'s dropped chip, GAP-118); the per-drop
log lines and the teardown total remain the fuller record.

### GAP-115 · ~~Medium~~ Low (NARROWED 2026-09-20, phase 5) · The staging directory is swept and surfaced now, but still unbounded and not bulk-clearable

**What phase 5 closed, verified at source rather than assumed:**
- **The sweep exists.** `src-tauri/src/screen_recovery.rs`'s
  `run_screen_recovery`, wired into `lib.rs`'s `setup` immediately after
  `document_commands::run_import_recovery`. A stale orphaned `.part` whose
  prefix really holds footage (`part_holds_footage` = `mp4_boxes::scan`
  reporting a `moov` AND at least one fragment) is PROMOTED to a staged
  capture rather than deleted — which is exactly what this entry asked for,
  since a sweeper that deleted evidence the crash-safe container was designed
  to preserve would have been strictly worse than leaving it. A header-only
  shell and an abandoned `.export.mp4.part` are deleted. Anything whose stem
  does not round-trip through `capture_paths::is_capture_base` is classified
  `Foreign` and never touched, symlinks are never followed, and the pass is
  postponed while EITHER capture domain holds `CaptureGuard`.
- **Completed staged captures are surfaced AND user-deletable.**
  `staged_commands::list_staged_captures` backs
  `src/components/StagedCaptureList.vue`, shown FIRST in the Record Screen
  picker (spec §10's resume-or-discard), and `discard_staged_capture` removes
  the `.mp4`, the sidecar and any export temp behind a two-step confirm.
- **A staged capture now has somewhere to GO.** The export (phase 5's ninth
  vault write) deletes it after the vault write lands, so the common path no
  longer accumulates anything at all.

**What is still open, and it is the original entry's second half:** nothing
bounds the directory's total SIZE and there is no bulk "Clear staged
captures" action or size readout. A user who records a dozen 1080p captures
and saves none still accumulates gigabytes under `%LOCALAPPDATA%`; the
Record Screen list shows them, so they are at least discoverable and
removable one at a time, which is why this drops to Low. The 60 s staleness
rule deliberately does NOT expire a COMPLETE staged capture — only orphans
and temps — because deleting a recording the user has not decided about is
the loss this whole design exists to prevent. **Fix shape:** Phase 6's
settings card gets the size readout and the bulk clear (spec §10's
disk-pressure paragraph); Task 9's sweep and Task 8's
`list_staged_captures` are what it builds on.

### GAP-115 (original text, for the record) · Phase 2 never sweeps, surfaces or bounds its staging directory — for CRASHED and for cleanly finished captures alike
`src-tauri/src/lib.rs` (`setup` wires `capture_commands::run_recovery` and
`document_commands::run_import_recovery`, and nothing for the screen domain)
plus `src-tauri/screen/src/staging.rs`. A screen capture writes into
`%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\.<base>.mp4.part` and
publishes it to `<base>.mp4` on a clean stop. If the process dies
mid-capture — the exact case §6.4's fragmented container exists to survive —
the `.part` survives with playable footage in it and **nothing ever looks at
it again**: no sweep offers it to the user, no janitor removes a stale one.
**Failure scenario:** a user whose machine crashes mid-capture loses nothing
(the bytes are there and decode as a prefix) but is never told so, and the
staging directory accumulates orphans across crashes with no size bound and
no clear action. **Why it is not fixed here:** the resume-or-discard UI and
`run_screen_recovery` are Phase 5's row in spec §13, and a sweeper with
nowhere to offer its findings would only delete evidence the crash-safe
container was designed to preserve — strictly worse than leaving it. Until
Phase 5, the orphan is recoverable by hand (rename `.part` → `.mp4`), which
the Windows verification checklist documents as item 8's method. **Fix
shape:** Phase 5's `run_screen_recovery` — sweep the staging dir, pair each
`.part` with its sidecar, offer resume-or-discard, and only then apply a
staleness rule.

**The happy path leaks too, and it is the far more common case.** Everything
above is about the crash. But EVERY cleanly stopped capture also leaves a
`<base>.mp4` plus its `<base>.json` sidecar in the same directory, and in
Phase 2 nothing surfaces them, nothing deletes them and nothing bounds their
total size: there is no editor (Phase 4), no vault write (Phase 5), no entry
in the Recordings browser (out of scope, spec §1), no staged-capture list and
no "Clear staged captures" action (spec §10 puts it in Phase 6). The store's
`lastStaged` is rendered by nothing. **Failure scenario:** a user tries the
new Record Screen button a dozen times over a week at 1080p; several
gigabytes of MP4 accumulate under `%LOCALAPPDATA%` with no in-app way to see
or remove them, and no UI ever mentions that the directory exists. Spec §10
names disk pressure as something the design cares about; the Phase-2 slice
inherits none of it. The stop notification at least no longer calls this
"saved" (it reads "Screen capture ready … editing and saving into a vault
arrive in a later update", `screen_commands.rs::stopped_toast_copy`), so the
user is not sent hunting through their vault — but they are still not told
where the file went. **Fix shape:** Phase 6's "Clear staged captures" action
plus a size readout, and Phase 5's editor/export giving a finished capture
somewhere to GO; until then the honest statement is that Phase 2 stages
without ever collecting.

### GAP-116 · Low · Vault Buddy's own windows are filtered out of the picker by TITLE, not HWND
`src-tauri/src/screen_commands.rs` (`our_window_titles`, used only to filter
`list_capture_sources`). Spec §7.2's rule — never offer ourselves as a
capture *source* — is implemented, though **by TITLE rather than by HWND**,
which is a fourth unrecorded deviation from that section: `our_window_titles`
collects `WebviewWindow::title()` strings and `source.rs` drops any
enumerated window whose title matches one exactly.

**This has now MATERIALIZED (phase 4), and the filter held.** It was inert
through phases 2–3 — the four companion windows are `skipTaskbar: true`,
which tao implements as `WS_EX_TOOLWINDOW`, and `windows-capture`'s
`Window::is_valid()` already rejects those, so the comparison never ran. The
`editor` window is `skipTaskbar: false` (verified in `tauri.conf.json`), so
it enumerates, and the title filter is now the only thing standing between
the picker and offering Vault Buddy's own editor as a source. Two things make
it hold with no change: `our_window_titles` walks `app.webview_windows()` at
call time rather than reading a hardcoded list, so the editor is covered
automatically; and the editor's title is a static string in
`tauri.conf.json` with no `set_title` call anywhere in the repo, so the
EXACT match cannot go stale under a runtime suffix (a title carrying the open
capture's name would have broken it — do not add one without replacing this
filter first).

**What remains open is the filter's SHAPE, not its coverage.** A title match
is still, in principle, capable of hiding a USER's window that happens to
carry the identical string.

**Phase 3's `WDA_EXCLUDEFROMCAPTURE` does NOT close this entry.** Spec §5.3's
separate rule — never appear *in* a recording — IS implemented as of `714bbb7`
for `main`/`panel`/`bubble`/`overlay` (`src-tauri/src/capture_exclusion.rs`).
That affinity keeps our windows out of a recording's PIXELS; this entry is
about our windows being offered as SOURCES. Two rules, two spec sections,
neither one closing the other — do not retire GAP-116 on the strength of the
exclusion.

**The surviving warning below is still load-bearing**: the affinity must be
re-applied per window creation, and today that is satisfied only because
NOTHING builds a window at runtime — verified, there is no
`WebviewWindowBuilder` or `WindowBuilder` anywhere under `src-tauri/src/`.
Every window is declared in `tauri.conf.json`, which is what makes the
config-derived label test a complete source and a one-shot apply at capture
start sufficient. If Phase 4 builds its `editor` window on demand instead,
BOTH halves go blind at once: the test never checks a window it cannot see in
the config, and an editor opened DURING a capture is never excluded and
appears in the footage. **Failure scenario:** any window of THEIRS whose title
happens to equal one of ours is silently missing from the Window tab, with
nothing to explain the absence. And in the other direction, a later change
that gives the editor a dynamic title — the open capture's name in the
titlebar is the obvious one — silently stops matching, so the editor is
offered as a capture source and capturing it recurses. Note this is the
SOURCE half only; the PIXEL half is closed, and on a supported build the buddy
appearing in the footage IS a failure — file it against verification row 16,
not here. **Fix
shape, for the SOURCE half that remains open:** filter by HWND rather than by
title — collect our windows' handles on the main thread and drop enumerated
windows by handle, so a user window sharing a title is never hidden and a
`skipTaskbar: false` window of ours is never offered. (The PIXEL half is
done: `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` over
`capture_exclusion::EXCLUDED_LABELS`. It fails on Windows builds older than
2004; that degrade path logs a warning and records with the buddy in frame,
by design.)

### GAP-117 · Medium · The `cfg(windows)` arms of `sink`/`source`/`session`/`exclusion` execute in no automated test anywhere
`src-tauri/screen/src/sink.rs`, `source.rs`, `session/windows_session.rs`,
`session/audio.rs`, `session/mux.rs`, `frames.rs`, and — added by Phase 3 —
`src-tauri/screen/src/exclusion.rs` (`set_display_affinity`'s `cfg(windows)`
body) plus `src-tauri/src/capture_exclusion.rs` (`set_affinity`'s whole
`run_on_main_thread` closure, and the `cfg(windows)` `window_handle` arm that
compiles NOWHERE locally — CI's `windows-app` job is its first compile). This
is the honest coverage statement for Phases 2–3, recorded so nobody reads the
green CI badge as covering it.

**What Phase 3 added to this list, specifically:** `source::resolve`'s Region
arm (monitor lookup and the live monitor dimensions it clamps against — the
pure `region_dims` it delegates to IS tested on Linux, the resolution around it
is not), the widened `FrameFlags` crop fields and the session plumbing that
carries them into `frames.rs`'s conversion call, and the exclusion apply/clear
pair above. **The Windows-target clippy runs
(`cargo clippy -p vault_buddy_screen --all-targets --target
x86_64-pc-windows-msvc` and the capture crate's `--lib` twin) prove only that
these arms TYPE-CHECK.** They are not compiled into a test binary and nothing
executes them; a wrong monitor index, an inverted affinity direction or a crop
field threaded to the wrong parameter all type-check perfectly. (The shell
crate has no such cross-check at all: `cargo clippy -p vault-buddy --target
x86_64-pc-windows-msvc` cannot run on Linux at any scope — `ring v0.17.14`
fails in cc-rs for want of MSVC's `lib.exe` — which is the reason
`set_display_affinity` lives in the screen crate and takes its `HWND` as an
`isize`.) `rust-core` runs the crate on Linux, where those modules are
either absent or reduce to an `Unsupported` arm; `windows-app` now runs the
crate's tests on Windows too (GAP-102, closed in this PR), but **those tests
are the pure modules' tests** — `clock`, `select`, `convert`, `staging`,
`session::pacing`, `mp4_boxes`, `source`'s id encoding. Nothing in CI opens a
Media Foundation sink, resolves a real HWND, or runs the three-thread
session. **Failure scenario:** a Windows-only regression — a wrong media-type
attribute, a stride assumption, a thread-join ordering change — compiles
clean, passes every gate, and is caught only by a human recording their
screen. **This is a deliberate limit, not an oversight:** no CI runner can
record a screen, which is precisely why the architecture pushes every
decision it can into pure modules (`session/mod.rs`'s "WHAT IS PURE AND WHAT
IS NOT" header states the rule) and why the manual checklist
(`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`)
is Phase 2's gate. **Fix shape, partial:** the `fmp4-spike` feature is the
precedent for a Windows-only test that drives real Media Foundation with
synthetic input; a sink-level smoke test in that shape could run on
`windows-app` and would cover sink creation and finalize without needing a
screen. Frame acquisition and the WGC callback remain out of reach.

### GAP-118 · ~~Medium~~ FIXED 2026-09-19 · The screen capture store's live state had no renderer: Phase 2 shipped no capture bar
`src/stores/screenCapture.ts` (holds `status`, `startedAtMs`,
`pausedTotalMs`, `pausedSinceMs`, `fps`, `dropped`, `warning`) with no
component consuming any of it — `ScreenCaptureBar.vue`, the plan's Task 10
and spec §7.3's during-capture surface, **did not land in Phase 2**
(verified: no such file, no `tests/screenCaptureBar.test.ts`, and
`ActionPanel.vue`'s list view renders `RecordingBar` for the audio domain
only). `ScreenSourcePicker.vue`'s `onStart` already calls `store.showList()`
with a comment saying "the capture bar lives on the list view beside
RecordingBar" — it navigates to a view that shows nothing.
**Failure scenario:** after starting a screen capture the panel returns to
the vault list with no visible sign a capture is running. Elapsed time,
paused state, the dropped-frame indicator and any `screen:warning` are
invisible **while the capture is live** — a terminal warning still reaches
the user, through `emit_screen_stopped`'s `"Recorded {base} with a warning:
{w}"` toast (`screen_commands.rs::stopped_toast_copy`); it is the live window that has no surface; the only controls are the tray / buddy right-click menu items
(`tray.rs` does route Stop/Pause/Resume to the screen domain, so the capture
is controllable and never strandable — this is a missing surface, not a
missing capability), and the only place the dropped-frame count can be read
is `vault-buddy.log`. It also blunts GAP-114's stated mitigation, which
assumes the count is surfaced. **Fix shape:** land plan Task 10 as written —
`ScreenCaptureBar.vue` rendered on the list view beside `RecordingBar`,
reading the existing store (elapsed excluding paused time, Pause/Resume,
Stop with an in-flight guard, and the dropped-frame badge shown only once
`dropped > 0`), plus `tests/screenCaptureBar.test.ts`. The store side is
done and tested; this is a presentational component and its wiring.

**FIXED 2026-09-19, on exactly those terms.** Plan Task 10 landed — it had
been skipped by the dispatcher (Task 8 → 9 → 11) and is filled in here, which
is why it sits out of order in the history. `src/components/
ScreenCaptureBar.vue` reads the store and renders on the list view beside
`RecordingBar` (`ActionPanel.vue`, gated `view === 'list' &&
showScreenBar` — the gate read `screenCapture.status !== 'idle'` when this
entry was written, and phase 4 widened it to
`status !== 'idle' || lastStaged !== null` so the finished capture's Edit
action has somewhere to render; the narrow form is exactly the shape this
entry exists to warn against), so `ScreenSourcePicker.vue`'s
`store.showList()` now lands on the surface its comment always claimed:
elapsed via `store.elapsedMs(now)` (paused time excluded — the Rust clock
excludes it by construction, and the bar delegates rather than re-deriving
it), Pause/Resume, Stop, the source title, an inline `screen:warning` line —
the store deliberately withholds that toast while a capture is live *because*
this line now exists — and a `dropped` chip shown only once `dropped > 0`.
`tests/screenCaptureBar.test.ts` covers all six. Stop's in-flight guard is a
new `stopping` flag on the store (Task 9 left it out because nothing consumed
it yet): set when `stop()` is asked for, cleared only when the capture really
ends (every route to idle) or when the stop was REJECTED, since Stop must be
offered again after a rejection that changed nothing — `stop_screen_capture`
can answer `stillSaving` while the session is still tearing down, so clearing
on the command's own reply would re-arm Stop against a live finalize. (The
rejection is *usually* `is_capturing` saying no, where nothing is indeed
finalizing; the command's `JoinError` arm can reject after `Control::Stop`
already went out, where re-arming costs at worst a duplicate fire-and-forget
send.) **One residual, deliberate:** `fps` is still rendered nowhere. It is
a rate, not an anomaly; §17.3's signal is the drop COUNT, and the Windows
verification checklist's item 10 already reads the fps values from
`vault-buddy.log` by design. GAP-114's mitigation is no longer blunted — the
dropped count is now visible in the UI as that entry assumed.

### GAP-119 · Low · "Every spawned thread is named" has one unnameable exception: the WGC frame worker
`src-tauri/screen/src/session/windows_session.rs` (the `start_free_threaded`
call) against AGENTS.md's Diagnostics-invariants rule. Vault Buddy names
every thread it spawns with `std::thread::Builder` so a crash record
identifies the dying thread — `screen-mux`, `screen-audio`, `screen-warn`,
`screen-stats`, `screen-capture-device`, `screen-capture-monitor` and the
rest all comply. `windows-capture`'s `start_free_threaded`, however, spawns
the WGC frame worker itself with a bare `thread::spawn`, and that thread is
where the frame callback (`frames.rs`) runs. **Failure scenario:** a native
fault inside frame acquisition produces a crash record naming an unnamed
thread, and the first reader of that record wastes time deciding whether the
invariant was violated by our own code. **Why it is not fixed:** the thread
is created inside a third-party crate, so naming it would need an upstream
change; the blocking alternative (`start` rather than `start_free_threaded`)
is strictly worse and the call site says why — it offers no way to stop a
session that is receiving no frames, so a still screen would hang Stop.
**Fix shape:** none locally beyond keeping the exception written down in both
places (the call site and AGENTS.md's invariant, both done) — or, upstream,
a PR giving `windows-capture` a named worker.

### GAP-120 · Low · `fmp4_spike` and `mp4_boxes` are compiled by no CI job
`src-tauri/screen/src/fmp4_spike.rs` (behind the non-default `fmp4-spike`
feature) and `src-tauri/screen/src/mp4_boxes.rs` (its only consumer). The
per-build `fmp4-spike` CI step was added with the spike and retired once the
question it answered was settled — correctly, since re-running a 300-frame
Media Foundation measurement on every push buys nothing. The consequence is
that the module now builds nowhere in CI: `cargo clippy --workspace
--all-targets` does not enable non-default features, and neither does either
`tauri build`. AGENTS.md describes the spike as "kept re-runnable", which is
true today and untested tomorrow. **Failure scenario:** a refactor of
`ScreenSession`, `staging` or `sink` breaks the spike's call sites; nothing
notices; the next person who needs to re-measure the container decision
(Phase 4's editor work is the likely trigger) finds it does not compile and
must repair it before they can measure anything. **Fix shape:** one cheap
`cargo check -p vault_buddy_screen --features fmp4-spike` line on the
`windows-app` job — no link, no run — or accept the rot and re-check by hand
before any re-run. Verified 2026-09-19 that it still cross-compiles clean
against `x86_64-pc-windows-msvc`.

### GAP-121 · Low · An `encoderUnavailable` start leaves a zero-byte `.part` in staging
`src-tauri/screen/src/sink.rs` (`create`) against spec §14, which says an
encoder-init failure is "refused before anything is written". `create` calls
`MFCreateFile` (with `MF_OPENMODE_DELETE_IF_EXIST`) **before** it builds the
media types and the sink writer, so a machine with no usable H.264 encoder
refuses the capture only after the `.part` file exists. Nothing is *written*,
so §14's letter holds; the deviation is that a zero-byte orphan is left
behind. Combined with GAP-115 (no staging sweep of any kind in Phase 2) that
orphan is permanent. **What is NOT a problem here, checked rather than
assumed:** `DELETE_IF_EXIST` cannot clobber a previous crashed capture's
retained `.part`, because `staging::reserve_base` tests all three names
(`.mp4`, `.json`, `.mp4.part`) for freeness before a base is handed out.
**Failure scenario:** a user on a machine whose encoder is missing or held by
another application presses Record Screen repeatedly and accumulates one
zero-byte file per attempt, invisible to them. **Fix shape:** build the media
types and the sink writer first and call `MFCreateFile` last, so the refusal
really does precede the file; or delete the `.part` on the failure arm.
Either is a small, local change — deferred only because it is untestable
outside Windows and this phase's Windows path is entirely unverified
(GAP-117).

### GAP-122 · Medium · The sink's frame size is still PREDICTED before the first frame arrives
`src-tauri/screen/src/session/windows_session.rs` (`ScreenSession::start`),
`src-tauri/screen/src/session/mux.rs` (`run_mux`) and
`src-tauri/screen/src/source.rs` (`window_capture_dims`). The H.264 sink's
frame size is frozen when `FragmentedSink::create` runs, which happens on the
mux thread **before any frame has been delivered** — `start` deliberately
waits on a ready handshake so a capture that cannot be written fails
synchronously, with a real message, instead of looking like it started and
producing nothing. So the declared size is a *prediction* derived from the
source, and `pacing::usable_frame` rejects every frame that comes in smaller
(reading the declared height out of a shorter mapped texture would run past
its end). **The failure this already caused, now fixed:** for a WINDOW the
prediction came from `windows-capture`'s `Window::width()/height()`, i.e.
`GetWindowRect`, which on Windows 10/11 includes DWM's invisible resize
border (~7-8 px left, right and bottom) — but WGC sizes its frame pool from
the capture item's own (DWM-composed, smaller) size. Every frame was
therefore undersized, every frame was dropped, and finalize failed with a raw
`MF_E_SINK_NO_SAMPLES_PROCESSED` (0xC00D4A44) over a zero-frame file, in the
user's OS language. Monitors were unaffected (`dmPelsWidth` matches WGC
exactly). The fix declares `DWMWA_EXTENDED_FRAME_BOUNDS` instead, which MSDN
names as the visible window bounds and which OBS's own WGC backend treats as
the capture texture's rect (`libobs-winrt/winrt-capture.cpp`'s
`get_client_box` offsets into the texture from exactly this rect), and which
the vendored crate's `Window::title_bar_height` also assumes. **Why this is
still a gap:** Microsoft documents no equality between the extended frame
bounds and `GraphicsCaptureItem::Size`, so the new value is a
better-evidenced prediction, not a contract. Anything that moves the two
apart again — a future Windows compositor change, an unusual window style, a
DWM call that answers with stale bounds mid-resize — reopens the same class
of failure. **Fix shape (the robust one, deliberately NOT taken here):** stop
predicting. Create the sink from the FIRST DELIVERED FRAME's dimensions, so
the declared size is correct by construction for every source kind, forever.
That inverts the ready handshake: `start` would have to report "the capture
is writable" only after a frame has arrived, which means either a bounded
wait for the first frame on the start path (a still window may not deliver
one promptly — the very reason the heartbeat exists) or moving the
unwritable-capture failure from a synchronous start error to an asynchronous
`screen:failed`, losing the property that a broken encoder is refused while
the user is still looking at the Start button. It is a significant refactor
in code that executes in no automated test anywhere (GAP-117), which is why
it is recorded rather than attempted on a bug-fix branch. **What now stands
in for it:** a size mismatch is no longer silent or opaque — the drop log
names both the actual and the declared dimensions, and a capture that
finalizes having written zero video samples fails with
`diagnose::zero_video_diagnosis`'s explanation (naming both sizes) instead of
the HRESULT, with the retained `.part` described as holding no usable video
rather than "the recording". All of that decision and wording is pure and
Linux-tested in `src-tauri/screen/src/diagnose.rs`.

**Update 2026-09-19:** the prediction now has empirical support — a real
Windows 11 window capture, which produced zero frames before the fix,
records correctly after it, with pause/resume working. That confirms the
DWM-extended-frame-bounds size matches what WGC delivers *on this
machine and this build*. It does not make the equality a contract:
Microsoft still documents none, so the diagnosis path added alongside the
fix (a zero-video capture explains itself and names both sizes) remains
the thing that keeps a future drift loud instead of silent.

### GAP-123 · Medium · The pacer governs only the UNDER-delivery direction of the video timeline
`src-tauri/screen/src/session/pacing.rs` (`VideoPacer`),
`src-tauri/screen/src/frames.rs` (`on_frame_arrived`) and
`src-tauri/screen/src/session/windows_session.rs`
(`minimum_update_interval_settings`). An MP4 track's timeline is the sum of
its sample durations, and every video sample is stamped with the nominal
`frame_duration(fps)`. The still-screen fix makes the pacer fill every frame
slot WGC left empty, so the track can no longer come out shorter than the
capture — the confirmed bug where 40 recorded seconds played back in 3-4.
The opposite direction is still ungoverned: nothing drops a frame that
arrives for a slot already written, so if WGC delivers FASTER than `fps`
the track comes out longer than the capture and plays in slow motion, by
exactly the delivery/`fps` ratio.

Today that is held off one layer up rather than in the pacer.
`minimum_update_interval_settings` asks WGC for a `MinUpdateInterval` of one
frame period, which caps delivery at `fps`, and on the Windows 11 the app
targets that is the path taken. But it is deliberately a capability query,
not a version check: a build whose `GraphicsCaptureSession.MinUpdateInterval`
probe returns false or errors falls back to `MinimumUpdateIntervalSettings::
Default`, and WGC then delivers at the monitor's refresh — 2x on a 60 Hz
panel at 30 fps, ~4.8x on a 144 Hz one. On such a machine a busy screen
records in slow motion while a still one is correct, and nothing in the
capture reports it; the `dropped` counter stays at zero because these frames
are written, not dropped.

The fix is the symmetric half of the slot rule the still-screen work added:
have the pacer decline a real frame whose timestamp falls in a slot it has
already written, exactly as it declines a repeat for a slot still in
progress. That is a constant-frame-rate converter, which is what the sink's
own declared `MF_MT_FRAME_RATE` already claims the stream is. It was left out
of the still-screen fix because it means DROPPING captured content on a path
no CI runner and no reviewer here can exercise, and because the confirmed
production failure was the other direction. Pin it with the same pure
pacing.rs tests, whose fps and elapsed time are hand-derived.

Not to be confused with the accepted COST of constant rate, which is not a
gap: a still 4K60 capture now hands the encoder 60 identical frames a second
instead of two. The bitrate cost is small (H.264 codes an unchanged frame as
an all-skip P-slice, on the order of bytes) and bounded by the
`MF_MT_AVG_BITRATE` the sink was already opened with for `fps` frames a
second, but the per-frame NV12 copy and encode load on a still screen now
match a busy one. That is the price of a timeline whose durations are true,
and the alternative — variable durations with a frame of lookahead — trades
it for VFR MP4s that players handle unevenly.

### GAP-124 · Low · The capture exclusion is applied fire-and-forget, so the first frames of a capture can still contain Vault Buddy's own windows
`src-tauri/src/capture_exclusion.rs` (`set_affinity` posts to
`run_on_main_thread` and returns immediately) and
`src-tauri/src/screen_capture_worker.rs` (the apply site, inside
`start_screen_capture_blocking`). The exclusion is applied *before* the WGC
session opens, which is the right ordering — but the apply itself is a queued
main-thread closure, and the start path carries on without acknowledging it.
Both call sites carry a comment pointing here, which is what this entry
answers. **Failure scenario:** on a busy event loop the session can deliver
its first frame or two before the affinity lands, so a recording opens with
the buddy (and whatever the panel was showing) visible for ~30 ms. Cosmetic
in the ordinary case, but it is the one window where spec §5.3's promise is
not kept. **Why not fixed:** waiting for the closure means a worker thread
blocking on the event loop, which is exactly the deadlock shape this
codebase's window rules exist to prevent (the original drag crash — see
AGENTS.md's window-system section). **Fix shape:** acknowledge the closure
through a bounded channel the way `select_capture_region`'s own
`OVERLAY_SHOW_TIMEOUT` setup handshake does, so the start waits a bounded few
hundred milliseconds and reports a wedged event loop rather than waiting
forever; or apply the exclusion once at app startup and never lift it, which
trades this window for making our own windows permanently invisible to every
other capture tool, which is worse.

### GAP-125 · Low · Region capture reads back the whole monitor every frame and crops on the CPU
`src-tauri/screen/src/frames.rs` (the WGC callback copies the full frame out
of the GPU staging texture) and `src-tauri/screen/src/convert.rs`
(`bgra_crop_to_nv12`). Spec §17.2 asks whether the crop can happen on the GPU
texture before readback. It does not, and that is deliberate: the crop lives
in a pure function precisely so Linux can test it, and that pure test is the
ONLY automated coverage region capture has anywhere (GAP-117). **Failure
scenario:** recording a 640×480 region of a 4K monitor at 60 fps costs the
full-monitor readback and the full BGRA→NV12 pass regardless of how small the
region is, so a small region is no cheaper than a full-screen capture. On a
machine where 4K60 is already near the edge, a region does not buy the
headroom a user would reasonably expect it to. **Fix shape:** a D3D11
`CopySubresourceRegion` before the CPU readback — which would move the crop
into code that nothing can test on any CI runner. Worth it only if the
verification checklist's 4K60 measurement (row 10, deferred to Phase 6) shows
the readback is actually the bottleneck.

### GAP-126 · Low · A region selection nobody answers cancels silently after two minutes
`src-tauri/src/region_commands.rs` (`REGION_TIMEOUT`, 120 s). If the overlay
webview dies, never renders, or never resolves, the wait expires,
`finish_region_selection` hides the overlay and `select_capture_region`
returns `Ok(None)` — indistinguishable, to the picker, from the user pressing
Escape. **Failure scenario:** a user whose overlay failed to render sees the
picker come back with no region and no explanation, and every repeat attempt
costs another two minutes of a full-screen invisible always-on-top window
before it clears. **Why it is this way:** the alternative — an error toast on
expiry — would also fire on an ordinary slow-but-legitimate selection that
happened to run past the deadline, and a false "something went wrong" on a
selection the user simply took their time over is worse than a quiet cancel.
**Fix shape:** distinguish the two outcomes in the reply (`cancelled` vs
`timedOut`) and surface only the latter — once real hardware has shown which
one actually occurs. The bound itself is not the gap: it exists so a dead
webview cannot strand that window forever, and it must stay.

### GAP-127 · Low · There is no keyboard-only way to draw a region
`src/roots/RegionRoot.vue`. The overlay reads pointer events only. Escape
cancels, but nothing selects. **Failure scenario:** a user who cannot use a
pointing device can record a screen or a window but not a region — the one
capture source with no keyboard path, on a surface that takes over the whole
display and therefore cannot be worked around with an external tool. **Fix
shape:** arrow-key cursor movement (with a larger step on Shift) plus Space to
anchor and Enter to commit, and an `aria-live` readout of the rectangle's
current size and origin so the state is legible without seeing the band. The
task list's own drag-and-drop keyboard fallback (`TaskDragHandle`) is the
established pattern in this repo for "a pointer gesture that needs a key
route".

### GAP-128 · Low · `DIALOG_ACTIVE` is a process-wide bool with two independent drivers, so a region selection and a native dialog can stomp each other's suppression
`src-tauri/src/lib.rs` (`DIALOG_ACTIVE`, a plain `AtomicBool`),
`src-tauri/src/commands.rs` (`set_dialog_active`),
`src/utils/nativeDialog.ts` (`withDialogSuppressed`) and
`src-tauri/src/region_commands.rs` (`select_capture_region`). A bool was right
while `withDialogSuppressed` was the only driver; phase 3 made the region
overlay a second one. Each driver pairs its own set and clear at one scope, so
neither can leave the flag stuck ON — but a bool has no depth, so the two
overlap badly. **Failure scenario** (multi-monitor, no unusual timing): a
region selection starts on monitor 2 and sets the flag; the panel is on
monitor 1 and remains interactive *because* the flag suppresses its
auto-hide, so the user opens Buddy settings → Integrations → Pandoc
**Browse**; `withDialogSuppressed`'s `finally` clears the flag when that
picker closes, while the region selection is still live; the user clicks back
onto the overlay, the panel blurs, `schedule_focus_out_check` sees
`dialog_active() == false` and hides the panel with the picker's state in it.
**Why not fixed in phase 3:** the symmetric case (a failing region select
clearing an open dialog's suppression) WAS fixed, by pairing set and clear at
one scope — but the overlap needs `DIALOG_ACTIVE` to become a counter, which
touches `lib.rs`'s shared flag and every existing caller, and that is a
change to a window-system invariant rather than a region-capture one. **Fix
shape:** an `AtomicUsize` with `fetch_add`/`fetch_sub` and a
`> 0` read, plus a saturating decrement so an unbalanced clear cannot wrap the
counter into a permanent suppression — which is the failure mode a bool
cannot have and a counter can.

### GAP-129 · Low · A hide-to-tray during a live region selection leaves the selection claim standing for up to two minutes
`src-tauri/src/region_commands.rs` (`RegionSelectionState`,
`finish_region_selection`) and `src-tauri/src/tray.rs` (`hide_buddy`, which
walks `ALL_WINDOW_LABELS` — including `"overlay"`). Hiding to tray makes the
overlay disappear, but the answer slot stays `Some` and `DIALOG_ACTIVE` stays
`true` until `REGION_TIMEOUT` (120 s) expires and the one cleanup function
runs. **Failure scenario:** a user re-reveals the buddy, picks *Select
region…* again, and is told "A region selection is already in progress." with
nothing on screen to corroborate it; meanwhile the panel cannot auto-hide.
Self-healing within two minutes, and impossible to reach without deliberately
hiding to tray mid-drag. **A fix must respect the one-cleanup-site rule:** a
hide path that called `finish_region_selection` itself would be the second
call site that
`the_region_selection_is_cleaned_up_from_exactly_one_place` exists to forbid.
Route the cancel through the answer channel instead — the same shape
`resolve_region_selection` already uses for Escape — so the existing wait
wakes and the existing cleanup runs.

**A second, unrelated path to the same stuck slot, recorded for
completeness rather than as a fix request:** the wait is
`tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(REGION_TIMEOUT))`,
and dropping a `JoinHandle` does not cancel a blocking task. If the command
future were dropped, the blocking task would hold `rx` for the full 120 s and
`finish_region_selection` would never run. Tauri spawns command futures and
runs them to completion, so this is reachable only at runtime shutdown, where
it does not matter.

### GAP-130 · Low · `available_monitors()` is an unbounded main-thread round trip on a tokio worker
`src-tauri/src/region_commands.rs` (`select_region_inner`). The call marshals
to the event loop and blocks on the reply with **no timeout**. It follows the
accepted precedent of `screen_commands::our_window_titles`, but it is the
unbounded version of it — and the sibling handshake a few lines below in the
same function (`setup_rx.recv_timeout(OVERLAY_SHOW_TIMEOUT)`) IS bounded at
5 s, for exactly the reason this one is not. **Failure scenario:** a wedged
event loop parks one tokio worker indefinitely; with enough such calls the
blocking pool starves and unrelated async commands stop answering. Remote, and
a wedged event loop is already a fatal condition for this app, which is why it
is accepted. Noted so the `our_window_titles` precedent is not read as
covering the unbounded case. **Fix shape:** the bounded-channel +
`run_on_main_thread` pattern the same function already uses for the overlay
show, so a wedged loop is reported rather than waited on.

### GAP-131 · Low · A wedged device thread leaks the capture exclusion until restart
`src-tauri/src/screen_capture_worker.rs` (the `screen-capture-monitor`
thread's `done_rx.recv()`), `src-tauri/src/screen_commands.rs`
(`clear_active_screen`), `src-tauri/src/capture_exclusion.rs`.
`clear_active_screen` is the one place spec §5.3's `WDA_EXCLUDEFROMCAPTURE` is
lifted, and on every ordinary teardown path the `screen-capture-monitor`
thread is what reaches it. If the device thread never returns from
`session.stop()` — a wedged Media Foundation `Finalize` — the monitor blocks in
`done_rx.recv()` forever: the reservation is never dropped, the `CaptureGuard`
is never released, and the exclusion is never lifted.

**Pre-existing, with a newly EXTERNAL blast radius.** The wedged-monitor leak
predates the exclusion work; until phase 3 its symptom was entirely internal
("Vault Buddy will not start another capture"). It is now *additionally* "the
user's Vault Buddy windows are missing from every other application's screen
recording", with no error, no log line, and no plausible bug report — the
buddy is still perfectly visible to the user, so nothing looks wrong from
inside the app. **Bounded:** `WDA_*` is per-HWND and dies with the window, so
restarting Vault Buddy always clears it; do not let a later edit reintroduce
an unbounded reading of this. **A fix must respect** the one-clear-site rule
pinned by `capture_exclusion.rs`'s structural test: any timeout-based rescue
has to funnel through `clear_active_screen`, not add a second clear.

### GAP-132 · Low · The screen source-row button markup is copy-pasted three times
`src/components/ScreenSourcePicker.vue` (the enumerated row inside the two
list tabs) and `src/components/ScreenRegionPicker.vue` (the target row and the
selected-region row) are the same `<button>` with an identical nine-utility
class string and an identical `border-violet-400 : border-white/10`
active-state ternary. It sits under fallow's clone threshold
(`cloneGroups=0`), so no gate catches it, but AGENTS.md's "UI primitives &
design tokens" section exists specifically to stop this growth — the focus-ring
string was in 25 files before that layer landed. **Failure scenario:** the
Phase-4 editor brings a fourth copy; a later token or focus-treatment change
lands in two of the four and the picker's tabs visibly disagree. **Fix
shape:** a `ScreenSourceRow.vue` presentational component (`title` / `detail` /
`pressed` / `testid`, `@click`) collapses all three without changing a pixel.
Deliberately NOT done in the phase-3 task-7 fix wave: extracting a shared row
is scope growth for a fix wave, and Phase 4 is the natural moment, since it
adds the fourth call site.

### GAP-133 · Low · The source-picker test fixture spells a monitor detail with a separator Rust never emits
`tests/screenSourcePicker.test.ts`'s `SOURCES` fixture gives the monitor row
`detail: "2560x1440 · Primary"` (a middle dot). That is the SPEC's separator;
`list_capture_sources` actually emits `format!("{width}x{height} - Primary")`
(`src-tauri/screen/src/source.rs`), an ASCII hyphen. Landed with the phase-2
picker, untouched by phase 3. **Failure scenario:** limited today — the suite
asserts `toContain("2560x1440")`, which is true of both spellings, so nothing
is masked. But it is a fixture that does not match the wire format, which is
exactly the kind of drift a later assertion on the full detail string would
enshrine backwards — and the region row's own detail separator was reviewed FOR
parity against that same Rust line, so the two now disagree inside one file.
**Fix:** change the fixture to `"2560x1440 - Primary"`; no assertion in the
suite depends on the dot.

### GAP-134 · Low (NARROWED 2026-09-20, phase 5 — the EXPORT side is defended, the EDITOR side is not)
**Phase 5 defended the half that mattered most, in a different place from the
one this entry proposed.** `export_commands::timeline_from_sidecar` is now
the ONE place the hand-editable `timeline` field is interpreted, and it is
fully defensive: absent, null, wrong-typed, a non-numeric / negative /
fractional bound all degrade to the WHOLE capture — safe only because
`Timeline::is_untouched` then answers exactly as an absent field would, so a
degraded read REMUXES rather than re-encoding. An EXPLICITLY empty segment
list is deliberately NOT degraded: the user deleted everything, and
`export_refusal` has to see that rather than silently restore their
recording.

**What is still open is exactly what this entry names below**, and the fix
shape it proposed was NOT taken: `load_staged_capture` still returns
`Option<serde_json::Value>` verbatim (`editor_commands.rs:77`, `:143`), and
`useEditorTimeline.ts`'s `snapshot` still does `t.segments.map(...)` on it.
A sidecar carrying `"timeline": {}` still throws `TypeError: Cannot read
properties of undefined (reading 'map')` into the editor's load banner. So
the export can no longer be fooled by a malformed sidecar; the editor can
still be confused by one. **Fix shape, updated:** parse the field into a
`Timeline` in `load_from_staging_dir` as below — and now that
`timeline_from_sidecar` exists and is tested, reuse it rather than writing a
third reader.

### GAP-134 (original text, for the record) · The editor accepts a hand-edited sidecar timeline without validating its shape
`src/composables/useEditorTimeline.ts` (`snapshot`, at construction) and
`src/roots/EditorRoot.vue`'s `load`. The sidecar's `timeline` field is an
unvalidated `serde_json::Value` from Rust all the way to the webview, and
`editor_commands.rs`'s own doc says the sidecar "may be hand-edited". A
sidecar carrying `"timeline": {}` reaches `snapshot(initial)`, which does
`t.segments.map(...)` and throws `TypeError: Cannot read properties of
undefined (reading 'map')` at construction. `EditorRoot`'s `catch` turns that
into the raw error string in the load banner, so nothing is lost or corrupted
— but the message names a JavaScript internal rather than the file. Segment
shapes are unchecked too: a hand-edited `sourceEndMs < sourceStartMs` is
carried through the editor and written back. **Failure scenario:** a user who
hand-edits or sync-conflicts a staging sidecar gets `TypeError: Cannot read
properties of undefined` where the app could have said "that capture's saved
edit could not be read; starting from the whole recording". **Fix shape:**
validate on the Rust side, where the untrusted value already crosses a typed
boundary — parse the field into `core::timeline::Timeline` in
`load_from_staging_dir` and answer `None` (i.e. "no saved edit") when it does
not fit, which also gets phase 5's parse done in the one place that already
reads the file. Deliberately NOT done in the P4 fix wave: it changes the wire
contract's meaning for a case no shipped code can produce, and phase 5 has to
parse that field into a `Timeline` regardless. **One thing to know while it
stands open:** `src/types.ts` declares
`StagedCaptureDetail.timeline: TimelineDto | null`, which asserts a guarantee
nothing provides — `editor_commands.rs` returns `Option<serde_json::Value>`
and `load_from_staging_dir` never inspects its shape. The annotation is what
let `snapshot()` be written without a shape check; read it as "whatever the
sidecar held", not as a contract (the phase-4 review's m-5; `types.ts` now
says so at the field).

### GAP-135 · Medium (REWRITTEN 2026-09-20, phase 5) · The on-disk timeline shape is now spelled by hand in THREE places and enforced by no derive

Phase 5's export had to read the sidecar, and the "failure scenario" below
predicted how it would go wrong: add `#[derive(Deserialize)]`, forget
`rename_all = "camelCase"`, and every timeline the shipped editor has written
fails to parse. **Task 7 sidestepped that by not deriving at all** — it reads
`sourceStartMs` / `sourceEndMs` by hand in
`export_commands::timeline_from_sidecar` and degrades to the whole capture on
anything it cannot read. That is a genuinely safer failure mode than the one
predicted, and it is tested; it also means the convention is still enforced by
nothing.

**The count went up, not down.** The on-disk camelCase names are now spelled
in three independent places:
1. `src/utils/timelineGeometry.ts` + `useEditorTimeline.ts` — what WRITES them;
2. `core/src/timeline.rs`'s test helper `segments_of` — the shared fixture
   table's hand mapping (its comment used to claim it was the only place in
   Rust; phase 5 made that false and the comment now says so);
3. `export_commands::timeline_from_sidecar` — **production** Rust, the one
   the export's correctness depends on.

Nothing connects them: not a derive, not a type, not a test that fails when
one is renamed. Rename `sourceStartMs` on the TypeScript side and (3) reads
nothing, degrades silently to the whole capture, and exports footage the user
deleted — with every test green, because (2) and its fixture table use the old
name and still agree with each other.

**A fourth, smaller instance of the same class landed with it, and IS pinned:**
`screen:exported`'s payload is a `serde_json::json!` object literal in
`emit_exported`, read by `src/types.ts`'s `ExportResult`. Dropping a key
compiles in Rust (the field is still constructed) and passes Vitest (which
emits its own payload). `export_commands`'s
`the_exported_event_carries_every_field_the_editor_reads` scans the emitter's
source for each key — a structural pin, because there is no derive to enforce
it. Do the same for any new hand-mapped payload.

**Fix shape, unchanged in substance:** derive `Serialize`/`Deserialize` with
`rename_all = "camelCase"` on `Segment`/`Timeline`, have (2) and (3) both go
through it, and keep a test that round-trips a LITERAL JSON string spelled the
way `useEditorTimeline` writes it — a literal, not a re-serialize, so it fails
when the names change on either side.

### GAP-135 (original text, for the record) · `core::timeline` has no serde derives, so the editor's on-disk timeline shape is an unenforced convention
`src-tauri/core/src/timeline.rs` (`Segment`, `Timeline` — `#[derive(Debug,
Clone, Copy, PartialEq, Eq)]` and `#[derive(Debug, Clone, PartialEq, Eq,
Default)]`; no `Serialize`, no `Deserialize`, no `rename_all`) against
`src/types.ts`'s `SegmentDto { sourceStartMs, sourceEndMs }` and
`src-tauri/src/editor_commands.rs`'s `save_capture_timeline`, whose `timeline`
parameter is an opaque `serde_json::Value` it stores verbatim.

Phase 4 ships the editor with the segment algebra implemented TWICE: in
`core::timeline` (Rust, unit-tested, uncalled) and in
`src/utils/timelineGeometry.ts` + `src/composables/useEditorTimeline.ts`
(TypeScript, what actually runs). The TypeScript side writes camelCase keys;
the Rust struct's fields are `source_start_ms` / `source_end_ms`. Nothing
connects the two — not a derive, not a test, not a type. The staging sidecar
round-trips today only because Rust never parses the field at all.

**Failure scenario:** Phase 5 adds `#[derive(Deserialize)]` to `Timeline` to
read the sidecar for the export, forgets `#[serde(rename_all =
"camelCase")]`, and every timeline the shipped editor has ever written fails
to deserialize. Whether that surfaces as an error or as an exported whole
recording (the untouched fast path, silently resurrecting cut footage)
depends on how the failure is handled — and the second is the likelier
reading of `Option<Timeline>`. **Fix shape:** derive
`Serialize`/`Deserialize` with `rename_all = "camelCase"` on both types NOW,
and add a test that round-trips a literal JSON string spelled the way
`useEditorTimeline` writes it — a literal, not a re-serialize, so the test
fails if the names change on either side. GAP-134's fix (parse the field in
`load_from_staging_dir`) needs this first and should carry it.

### GAP-136 · Low · The output-to-source mapping exists twice, in two languages — now held apart by a shared fixture table, but only for the rows in it
`src-tauri/core/src/timeline.rs`'s `Timeline::to_source_ms` and
`src/utils/timelineGeometry.ts`'s `toSourceMs` (plus their siblings —
`output_duration_ms`/`outputDurationMs`, `whole`/`wholeTimeline`, and
`split_at`/`delete`/`reorder`, which live in `useEditorTimeline.ts` on the
TypeScript side). Both implement spec §8.1's segment algebra. The TypeScript
side also has `toOutputMs`, the inverse the preview needs, which the Rust side
does not have at all.

They coexist for a real reason, but NOT the one this entry used to give. It
said "the preview has to map the playhead in the webview, and the export has
to map it in Rust". **The second clause is false, measured on this tree:**
`Timeline::{split_at, delete, reorder, to_source_ms}` — 70 non-blank lines —
have ZERO production callers. Every hit is inside a `#[cfg(test)]` module or
`screen/tests/export_roundtrip.rs`. The editing algebra shipped in TypeScript
in phase 4 and Rust's copy was never called; what the export actually uses
from `core::timeline` is `Segment`, `Timeline`, `whole`,
`output_duration_ms`, `is_empty` and `is_untouched`.

The real reason to keep the Rust copy is that it is the EXECUTABLE SPEC the
shared fixture table checks TypeScript against — which is worth keeping. But
nobody should read this entry and conclude the Rust algebra is
battle-tested production code, or "optimize" the export onto it expecting
that. It is a test oracle. This is still not a delete-one gap. The risk is that they DRIFT, and phase 5's export plans on the Rust one
while the user watched the TypeScript one, so a disagreement means the
exported file does not match the preview the user approved.

**They already HAD drifted, measurably.** The phase-4 review built one fixture
table and ran it through both languages. Well-formed cases agreed exactly; two
did not, and neither is the pair this entry used to name as candidates
(`splitAt`'s `Math.round`, and `segmentAtOutputMs`'s strict `<` — both were
checked and neither diverges):

- **a backwards segment** (`[0,2000] [4000,3000] [4000,6000]`, reachable from
  a hand-edited or sync-conflicted sidecar, which nothing validates —
  GAP-134). `Segment::duration_ms` is a `saturating_sub`, so the middle
  segment contributed NOTHING in Rust; TypeScript subtracted raw, so it
  contributed a NEGATIVE length that walked the accumulator backwards. Rust:
  duration 4000, `to_source(2000) = 4000`. TypeScript: duration 3000,
  `toSourceMs(2000) = 5000`.
- **`whole(0)`**. Rust returns an EMPTY timeline and carries a named
  regression test against minting a zero-length segment ("reaches the exporter
  as an unplayable frame plan"); `EditorRoot`'s inline re-implementation of
  the same function seeded `[{0, 0}]` with no such guard.

**FIXED 2026-09-20 (the phase-4 final fix wave), on both halves.**
`timelineGeometry.ts` gained a private `durationOf` (`Math.max(0, end -
start)`) used by every function that measures a segment, and an exported
`wholeTimeline(durationMs)` carrying Rust's zero guard, which `EditorRoot`'s
`load` now calls instead of building the seed inline. And the shared fixture
table the plan's hand-off asked for now exists:
`tests/fixtures/timeline-cases.json`, read by `tests/timelineFixtures.test.ts`
and by `core/src/timeline.rs`'s two `shared_fixture_table_*` tests via
`include_str!` (so moving the file breaks the Rust build rather than silently
testing nothing). Both sides assert the row COUNTS as well as the rows, so a
table quietly shrinking to one case fails too.

**What remains, and why this is now Low rather than Medium:** the table holds
six mapping cases, two `whole` rows and four operation rows. Nothing forces a
NEW divergence into it — a function added to one side (`toOutputMs` is already
TypeScript-only) or a behaviour changed outside a covered row still drifts
unobserved. The residual fix, when phase 5 gives the Rust side a production
caller, is to extend the table wherever the export's own mapping is exercised
rather than writing a Rust-only fixture for it.

**UPDATE 2026-09-20 (phase 5). The Rust side now HAS its production caller,
and the residual fix above was followed — once.** `Timeline::is_untouched`
is the single predicate deciding whether an export remuxes losslessly or
re-encodes, so it rides the shared table: every case in
`tests/fixtures/timeline-cases.json` carries an `isUntouched` array, Rust
asserts the rows, and the Vitest side asserts only that no case can SKIP
declaring them. **Rust deliberately owns the rule** — `timelineGeometry.ts`
was explicitly NOT given an `isUntouched` twin, because a second copy one IPC
hop from the original is the shape this entry exists to warn about.

Building that row found a real hole, which is the argument for the table:
relaxing `is_untouched` to ignore where a segment STARTS left the existing
test green, because it probed a trimmed TAIL. A capture trimmed from the
FRONT would have read as untouched, and the fast path would have restored the
footage the user deleted — the same failure the null-timeline rule was removed
to prevent, arriving through another door. Two cases now separate "is
untouched" from "has one segment".

**What phase 5 ADDED to this gap:** `PlanSpan::restamp`
(`screen/src/select.rs`) is the source-to-output direction the export needs,
and it is the Rust counterpart of TypeScript's `toOutputMs` — but it has NO
shared row, so the pair that now exists in both languages is held apart by
nothing. It is also the direction the phase-4 review already caught the
TypeScript twin getting subtly wrong (a boundary written `<=` stayed green
against seven fixtures because every probe fell outside a segment under both
rules), which is why `restamp` is half-open at the far edge and its fixtures
probe a span's own end. **Fix shape:** add a `restamp`/`toOutputMs` row pair
to the shared table. Add a ROW, never a Rust-only fixture.

### GAP-137 · CLOSED · The asset protocol's staging-only scope
`src-tauri/tauri.conf.json` — `app.security.assetProtocol = { "enable": true,
"scope": ["$APPLOCALDATA/screen-captures/*"] }` — against
`src/roots/EditorRoot.vue`'s `convertFileSrc(detail.assetPath, "asset")`. That
one scope line is what keeps the editor webview from reading arbitrary files
through `asset.localhost`.

**This entry was FALSE when it was written, and that is the part worth
keeping.** It claimed the scope was "pinned by no test" and asked a future
agent to write one. The test already existed:
`src-tauri/src/tray.rs`'s
`the_asset_protocol_scope_is_pinned_to_the_staging_directory_alone`, landed in
`69ba75f` — the FIRST commit of phase 4, three commits before this entry — and
it asserts the scope array EXACTLY (an equality, so an added entry fails too),
that `enable` is `true`, and that the CSP still carries `media-src 'self'
asset: http://asset.localhost`. Widening the scope to `$APPLOCALDATA/*` turns
`cargo test -p vault-buddy --lib` red. AGENTS.md repeated the same falsehood in
the one paragraph it devotes to this security boundary; both were corrected in
the phase-4 final fix wave (2026-09-20).

**Residual, deliberately not tracked as an open gap:** the scope is
app-WIDE, not per-window, so any future webview inherits the same read access
to the staging directory. That is a property of Tauri's asset protocol rather
than of this config, there is nothing to tighten today, and the test above
makes any change to the line visible. Nothing else here is open.

### GAP-138 · Low · The staged-capture row on the panel's list view never expires or dismisses
`src/stores/screenCapture.ts` (`lastStaged`, cleared only by a SUCCESSFUL
`start()`) and `src/components/ActionPanel.vue`'s `showScreenBar`. After one
screen capture the list view carries a "Recorded <base> · Edit" row for the
rest of the process, with no dismiss and no expiry.

The audio domain's sibling affordance is explicitly time-boxed: `RenamePrompt`
is gated `view === 'list' && capture.lastSaved` and swept by
`dismissRenameIfStale` after `RENAME_PROMPT_MS = 30_000`, because a permanent
row on the main list was considered wrong there.

There is a real argument for permanence here — until phase 5's staged-capture
browser exists, that row is the ONLY handle anything has on the footage, and
the same reasoning is why it is allowed to stack above a live `RecordingBar`
(the phase-4 review's T7-I1). This entry exists because the trade-off was
recorded nowhere, not because it is settled. **Fix shape:** when phase 5's
browser lands, the row stops being the only handle and should either expire
like `RenamePrompt` or gain a dismiss; until then, leaving it is the safer of
the two, and it should be a conscious decision in that phase's plan.

**NARROWED 2026-09-20 (phase 5), on both halves, and now a conscious
decision.** `lastStaged` gained a SECOND clear site: `forgetStaged(base)`,
fired from the `screen:exported` and `screen:discarded` listeners, clears it
when — and only when — the base matches. So the row now disappears the moment
that capture is saved or discarded, which is the common path and the one that
mattered. It is keyed on IDENTITY rather than lifecycle, which is what makes a
second clear safe beside the first (`reset()`), and it deliberately does not
bump `seq`, because it is not a capture-state transition. And the row is no
longer the only handle: `StagedCaptureList` in the Record Screen picker
reaches EVERY staged capture, so removing the bar's row would no longer
strand footage.

**What is still open is the narrow case the entry opened with:** a capture the
user neither saves nor discards leaves the row on the list view for the rest
of the process, with no dismiss and no expiry. Since the footage is now
reachable elsewhere, an expiry or a dismiss is finally available as a choice
rather than a loss — and that choice was NOT made in phase 5 (leaving it is
still the safer default while the Record Screen list is a picker rather than
spec §10's browser). **Fix shape:** Phase 6, alongside the browser — expire
like `RenamePrompt` or add a dismiss, and decide it explicitly.

### GAP-139 · Low · Three of `useEditorTimeline`'s exports have no production consumer
`src/composables/useEditorTimeline.ts` — `revert`, `isDirty` and `outputMs`
are returned and read only by tests (`grep` over `src/`). `flushPending` was
the fourth until the phase-4 final fix wave gave it one: `EditorRoot.load`
awaits it before reading the next capture's sidecar, which also closed the
reopen-during-a-write race (the phase-4 review's m-8). Its own comment used to
say "the window-close path will", which was never reachable as written —
`window_close.rs` answers the editor's X wholly in Rust with
`prevent_close()` + `hide()`, so the webview receives no close event at all.

All three remaining are plausible phase-5 hand-offs: `isDirty`/`revert` are
spec §8.2's Discard, and `outputMs` is what a Save dialog would show. They are
unit-tested and not dead code in fallow's sense (object properties, not
exports), so nothing flags them. **Fix shape:** phase 5 either wires them to
the Discard/Save surfaces the spec names, or deletes them in the same commit
that decides not to — a returned function nobody calls is a claim about a
surface that does not exist.

**NARROWED 2026-09-20 (phase 5): THREE became TWO, and the title is now
wrong by one.** `outputMs` got its production consumer exactly where this
entry predicted — `EditorRoot`'s `canSave` computed, which disables **Save to
vault** when the timeline holds no footage. That it reads the composable's
`outputMs` rather than spelling the predicate inline is deliberate: a
hand-rolled `segments.some(s => s.sourceEndMs > s.sourceStartMs)` would agree
but would be a THIRD implementation of this feature's segment arithmetic, held
against nothing (GAP-136).

`revert` and `isDirty` still have no production consumer (`grep` over `src/`
finds only tests). Phase 5 DID ship spec §8.2's Discard — but it discards the
whole staged CAPTURE (`discard_staged_capture`, a file delete behind a
two-step confirm), not the edit, so it needs neither. The undo stack already
covers "undo my edits". **Fix shape, sharpened:** these two are now waiting on
a surface nobody has proposed since the spec named it. Phase 6 should either
name it or delete them.

### GAP-140 · Medium · The export's Windows-only arms, and the whole capture side, still execute in no automated test on any platform
`src-tauri/screen/src/disk.rs`'s `cfg(windows)` `free_bytes`
(`GetDiskFreeSpaceExW`) and `src-tauri/screen/src/export.rs`'s three-line
`cfg(windows)` `creation_flags` arm, plus everything GAP-117 already names
(`sink`, `frames`, `source`'s enumeration, `session::{audio,mux,windows_session}`,
`exclusion`, the shell's `capture_exclusion`).

**State this narrowly, because the route change earned a real narrowing and
the honest figure is much better than phase 4's.** The phase-5 plan was
written against a Media Foundation export whose reader, sink and sample pump
would all have been Windows-only and untestable; commit `1b458dd` replaced it
with a user-installed ffmpeg, which moved the export's correctness into
`ffmpeg_args` (pure) and `export` (portable), and `rust-core` now installs
ffmpeg and runs `screen/tests/export_roundtrip.rs` — a real round trip that
synthesizes a clip, applies a `Timeline`, and reads the output back checking
both length AND that each block still carries its own colour and tone. That
is the first executable end-to-end proof anywhere in this feature.

**What genuinely remains untested:**
- `disk::free_bytes`'s Windows arm. Its signature was the hard part
  (`windows` 0.62.2 hands `GetDiskFreeSpaceExW` three `Option<*mut u64>` RAW
  pointers, not `Option<&mut u64>`), it is type-checked by
  `cargo clippy -p vault_buddy_screen --target x86_64-pc-windows-msvc`, and
  its runtime behaviour is proven by nothing on any platform. The blast radius
  is deliberately small: the API returns `Option`, so a wrong answer of `None`
  lets a save proceed. A wrong non-`None` answer could refuse a save that
  would have fit.
- `export`'s `CREATE_NO_WINDOW` arm. Its absence is invisible on Linux and
  shows on Windows as a console window that flashes and steals focus — the
  exact Pandoc symptom that closed the settings panel out from under users.
  The flag's VALUE and placement are pinned by a two-arm Linux test through
  `creation_flags_for(cfg!(windows))`; that the flag reaches a real
  `CreateProcess` is not.
- **ffmpeg's own behaviour on Windows against a real Media Foundation
  fragmented MP4.** CI proves the arguments and a round trip on Linux against
  a SYNTHESIZED clip; it does not prove that MF's fMP4 remuxes cleanly through
  a Windows ffmpeg build. That is verification-checklist row 29 and it is the
  single highest-value manual row in the phase.

**Failure scenario:** the export produces a correct file on every Linux CI run
and fails, or silently produces an unplayable one, on the only platform that
ships. **Fix shape:** none available in CI — this is checklist rows 29–36 and
GAP-117's standing answer. Do not let a green `rust-core` be read as
"the export works on Windows".

### GAP-141 · Low · An export is refused while any capture is running, so a user cannot save an old capture while recording a new one
`src-tauri/src/export_commands.rs` — `busy_refusal(app.state::<CaptureGuard>()
.active())`, checked before the `ExportState` reservation. An audio recording
or a screen capture in progress refuses `export_and_save_capture` with the
guard's own busy message.

**This is a deliberate trade, recorded so it is not re-discovered as a bug.**
An export and a live capture both drive the machine's H.264 encoder, and
between an in-progress recording (irreplaceable) and a save that can simply be
repeated, the recording wins. Note the shape: the export READS the guard and
never CLAIMS it, because a claim would need a second
`release(CaptureKind::Screen)` site and a structural test pins that at exactly
one, in `screen_commands::clear_active_screen`.

**Failure scenario:** a user who records back-to-back meetings never has an
idle moment to save the previous capture in, and the refusal names the running
capture rather than offering to queue. Nothing is lost — staged captures
persist — but the workflow is "stop recording to save". **Fix shape:** either
leave it and say so in the UI (the refusal message is already specific), or
queue the export behind the capture the way transcription already yields to a
live recording. The transcription worker is the precedent worth copying, not
a second `CaptureGuard` claim.

### GAP-142 · Low · A saved capture cannot be renamed or re-exported, and the staged original is gone
`src-tauri/src/export_worker.rs`'s `remove_staged_capture` (correctly, after
the vault write lands) against the absence of any screen-capture analogue of
`capture_commands::rename_capture`.

Two consequences, both permanent once Save is pressed:
- **No rename.** The audio domain has `rename_capture`, which keeps the
  `YYYY-MM-DD HHmm ` prefix, moves the MP3 and the transcript sidecar on the
  never-clobber rails, and retargets the note's embed lines. A screen capture
  is named from its window title via `sanitize_title`, which is whatever the
  recorded application put in its title bar — the case most in need of a
  rename — and there is none. The user renames in Obsidian, which breaks the
  note's embed.
- **No re-export.** Export deletes the staged `.mp4` and its sidecar after the
  commit, by design (spec §8.3: "the staged capture is deleted only after the
  vault write has landed"). So a capture saved with the wrong trim, or at the
  wrong quality, cannot be re-cut: the source of truth for the edit is gone
  and only the exported result remains.

**Failure scenario:** a user saves a 40-minute capture, notices the last
segment is wrong, and has no path back — the editor's own timeline lived in
the sidecar that was just deleted. **Fix shape:** the rename is a direct port
of `rename_plan` + `rename_noreplace` + embed retargeting, and is the cheaper
half. Re-export is a product decision, not a bug: keeping the staged copy
after a save contradicts spec §10's discard-leaves-no-litter principle, so the
honest options are "export is final, and the UI says so" or "an explicit Keep
the original".

### GAP-143 · Low · Screen captures are invisible to the Recordings browser and are never transcribed
`src-tauri/core/src/transcript.rs`'s `capture_mp3s` — the walker BOTH
`recordings::list_recordings` and `transcript::pending_transcriptions` share —
matches `name.strip_suffix(".mp3")` only. A saved screen capture is a `.mp4`
in the vault's `Screen Captures` folder, so:
- it never appears in **Browse recordings**, even though it is a capture with
  a companion note in the same house style;
- it is never queued for transcription, so a recorded meeting captured as a
  screen share gets no transcript while the same meeting captured as audio
  does.

Both are consistent with spec §1, which put the Recordings browser out of
scope for this feature, and the audio path is unaffected — this is a missing
capability, not a regression. It is recorded because the two domains now
produce near-identical artifacts (a media file plus a `type:`-tagged note in a
per-vault folder) and a user has no way to know why only one of them is
listed and transcribed. **Fix shape:** `capture_mp3s` is already the single
walker both readers share, which is the right place: widen it to a set of
extensions and give `RecordingEntry` a kind. Transcription is the larger half
— `transcribe::decode` is Symphonia over MP3, and an MP4's audio track needs
demuxing first (ffmpeg is now a resolved dependency of the screen domain and
could extract it, but only when the user has installed it — see GAP-144).

### GAP-144 · ~~Medium~~ FIXED 2026-09-21 · `detect_ffmpeg` and `set_ffmpeg_path` have NO frontend caller, so nothing surfaces the export's hard dependency and the error message names a screen that does not exist
`src-tauri/src/ffmpeg.rs` (both commands, registered in `lib.rs`'s
`generate_handler!`) against `grep -rn 'detect_ffmpeg\|set_ffmpeg_path' src/
tests/`, which returns **nothing**.

The export shells out to a user-installed ffmpeg and refuses without one.
Phase 5's own plan claimed this was handled — "Task 11 surfaces that at Record
Screen — before the recording, not after it — so the discovery never happens
at the payoff" — and it is not: Task 11 shipped the staged-capture list and no
ffmpeg gate. There is no ffmpeg settings card, no `usePandocStore` analogue,
no Record Screen pre-flight, and no Browse button anywhere.

Two things follow, and the second is the sharper one:
1. **The discovery happens at the payoff.** A user records for forty minutes,
   edits, presses **Save to vault**, and only then learns the app needs
   software they do not have. Capture and editing work fine without it, which
   makes the surprise later rather than earlier.
2. **The refusal points at a screen that does not exist.**
   `export_worker::prepare` returns *"Saving a screen capture needs ffmpeg,
   which is not installed. Install it, then set its location in Buddy settings
   if it is not on your PATH."* There is no such setting in Buddy settings.
   The override is real and is read (`document_import.ffmpeg_path`, the same
   app-global section `pandocPath` lives in), but the only way to write it is
   `set_ffmpeg_path` over IPC or a `config.json` hand-edit.

**Failure scenario:** a user follows the error message into Buddy settings,
finds nothing, and concludes the app is broken — having already invested a
recording. **Fix shape:** the whole pattern exists one domain over and should
be copied rather than invented. `DocumentImportSettings.vue` +
`src/stores/pandoc.ts` are status card, Browse picker, Recheck and a cached
`ensureDetected()` that intake surfaces consult; `RecordMode`'s
blocked-Import route to a focused setup view is the pre-flight. Until then,
**do not write documentation claiming the gate is surfaced** — the phase-5
plan's "honest limit" section does, and it is wrong.

> **Fixed.** The card is `FfmpegSettings.vue` in Buddy settings → Integrations,
> the cached store is `src/stores/ffmpeg.ts`, and the Record Screen picker
> consults it on mount. The refusal now names the tab it lives on, so the
> string quoted above is stale.
>
> **What shipped is a NOTICE, not a gate, and the distinction is the point.**
> The phase-5 plan's sentence promised a gate and is *still* wrong: blocking
> Record Screen would remove a capture the user can perfectly well make and
> edit, since only the Save needs ffmpeg. Start is enabled identically either
> way and a regression test reddens if a future change disables it.
>
> The card was, as instructed, a copy of the Pandoc one — and `check:quality`
> caught it (cloneGroups 4, duplicatedLines 204 against a baseline of 0). No
> `--update` was run: the shared half became `useExternalTool` and BOTH cards
> were refactored onto it.
>
> **Residuals, named rather than implied:**
> 1. **The editor has no pre-flight.** A capture RESUMED from
>    `StagedCaptureList` or the capture bar's Edit never passes the picker, so
>    `ExportBar`'s Save is still that user's first notice. The largest one.
> 2. **The missing-encoder case has no notice arm.** `ffmpegMissing` is
>    `!installed`; a minimal LGPL build warns nowhere but the card, and fails
>    only on an EDITED capture. Widening it needs a second, differently-worded
>    arm ("you can save this only if you don't edit it") — a copy decision.
> 3. **A failed probe reads as "not installed"** in the picker. Deliberate
>    (warn on unknown; it blocks nothing) but the card distinguishes the two
>    and the notice does not.
> 4. **`FfmpegStatus` has no Rust↔TS key-parity test** — the GAP-135 class.
>    `export_commands.rs` pins `ExportResult`'s keys against `src/types.ts`;
>    this status has no equivalent, so a renamed field renders
>    `Installed (undefined)`.
> 5. **Nothing exercises the reply shape against a real `ffmpeg -version`**
>    from the frontend side; every new test mocks IPC.

### GAP-145 · Low · The preview seeks at every cut, so what the user approves is not frame-exact — and the export now makes that difference land in their vault
`src/components/editor/CapturePreview.vue` — one `<video>` element seeking
around a single source file, so a cut renders as a seek rather than a splice.
Spec §8.2 accepted this explicitly ("boundaries are not gapless… the UI
labels this a preview and treats export as authoritative"), and that was a
deliberate honesty choice rather than an oversight.

It is filed now because phase 5 changed what it costs. Through phase 4 the
preview was the only artifact; the approximation had nothing to be
approximate AGAINST. Now the user approves the preview and an exported file
lands in their vault, and the two are produced by different machinery in
different languages (GAP-136) — so "the preview is not authoritative" has
become a statement about a file they keep. Verification-checklist row 30 is
the only thing that has ever compared them, and it has not been run.

**Failure scenario:** a user trims tightly against a spoken word, the preview's
seek lands a few frames early, the export's `trim` filter lands elsewhere, and
the saved recording cuts a syllable. **Fix shape:** none cheap — gapless
multi-segment preview means either MSE or a pre-rendered proxy. The realistic
mitigation is to say so at the moment it matters (a line in the editor, not
only in the spec) and to keep row 30 as the thing that would catch a real
divergence.

### GAP-146 · Low · Export time on a long recording is unmeasured, and the progress bar is the only thing standing in for it
`src-tauri/screen/src/export.rs`. An untouched capture takes the `-c copy`
remux, which is I/O-bound and should be seconds; an EDITED one is a full
`filter_complex` decode-and-re-encode pass whose cost is unknown for any real
input. Nothing in the repository has measured either: the round-trip tests use
clips of a few seconds, and no phase has run on hardware.

Concretely unknown: how long a 60-minute 1080p edited export takes; whether
the chosen H.264 encoder on a given machine is hardware or software (the
capability probe picks one, and `041b567` already found that sending an x264
preset to an encoder that does not speak it fails outright); and whether
`STOP_TIMEOUT`-style patience is needed anywhere in the UI. The command is
deliberately unbounded — "a deadline here would abandon a worker that is still
writing into the user's vault" — so a pathological case has no ceiling.

**Failure scenario:** a user saves a long edited capture and the editor sits
at 12% for twenty minutes with no estimate, and no way to know whether that is
normal. **Fix shape:** measure it first (checklist rows 29 and 30 are the
opportunity), then decide whether the bar needs a rate or an ETA. Do not add
one before there is a number.

### GAP-147 · ~~Low~~ FIXED 2026-09-20 · `screen_recovery.rs` sat at exactly 800/800 nonblank lines, so the next line added to it breached the Rust cap
Closed by the fix wave, which needed to add a line to it (GAP-149 below) and
so paid the extraction this entry prescribed rather than taking the allowlist
entry it warned about. `src-tauri/src/screen_recovery.rs` is now
`screen_recovery/mod.rs` (**618** nonblank — the `read_dir` walk, the
promote/delete actions and the retry loop) plus `screen_recovery/decide.rs`
(**280** — `classify`, `owned`, `part_holds_footage`, `should_postpone`,
`is_stale_at`, all pure, and the mp4-box test fixtures both halves share),
split along exactly the seam the module's own doc already named. `lib.rs` is
unchanged, because `mod screen_recovery;` resolves a directory module
identically.

`src-tauri/src/export_worker.rs` hit the same wall in the same wave (the
rollback fix, GAP-150 below, took it from 738 to 905) and took the same
answer: `export_worker/mod.rs` (**676**) plus `export_worker/vault_dir.rs`
(**264** — create-the-directory-contained, measure free space, roll it back).
`export_commands.rs`'s `worker_src()` structural scan now concatenates BOTH
halves, since a scan that saw only one would stop covering whatever moved
into the other. `scripts/loc-baseline.json` was NOT touched: nothing has been
loosened on this branch.

Still worth knowing for the same reason: `src/types.ts` at 466/500 and
`src/roots/EditorRoot.vue` at 470/500 are the frontend files closest to their
own cap.

### GAP-148 · Low · Three deliberate departures from the screen-capture design spec, each shipped and each recorded here so the spec is not read as the implementation
`docs/superpowers/specs/2026-09-18-screen-capture-intake-design.md` §8.3, §11
and §14 against what phase 5 shipped. All three are in the spec's own
reconciliation note; they are duplicated here because this file is what an
agent scans before working in an area.

1. **§8.3's per-span cancellation poll is a TIMED poll.** The spec says
   "cancellation is a polled atomic checked per span, mirroring the search
   scan-generation pattern". With ffmpeg owning the whole pass there are no
   spans to poll between — and the fast path is ONE span, so a per-span poll
   would have made the longest, most unattended export the only uncancellable
   one. `export.rs` blocks on `recv_timeout(CANCEL_POLL)` (200 ms) against the
   progress channel, so even an export emitting no progress answers Cancel
   promptly, then kills the child, reaps it, joins both reader threads and
   deletes the truncated output.
2. **§14's "exported temp kept" on a failed vault write is "temp deleted,
   staged capture kept".** A kept temp is a promise `screen_recovery` breaks
   60 s later, so the same Retry button would behave differently depending on
   how long the user spent reading the error message. A retry re-exports from
   the staged capture, which is the artifact that must never be lost — and is
   kept. Every `Err` return in `export_worker`/`export.rs` removes the temp.
3. **`screen:discarded` exists and §11 does not list it.** Without it a
   discarded capture leaves `lastStaged` pointing at a base no longer on disk,
   so the panel keeps offering **Edit** and the editor fails with a banner the
   user cannot act on.

Not a defect list — each is a reasoned change — but a spec read as the
implementation would get all three wrong. **Fix shape:** none. Re-check them
if §8.3, §11 or §14 is ever edited.


### GAP-149 · ~~Medium~~ FIXED 2026-09-20 · The companion note's embed was built from a scraped window title with no wikilink escaping, so a title containing `#`, `^`, `[` or `]` produced a silently dead embed
`src-tauri/core/src/screen_note.rs` emitted `![[{mp4_file_name}]]`
unconditionally, while `src-tauri/screen/src/staging.rs`'s `sanitize_title`
maps only `: \ / ? * " < > |` and drops control characters. All four
wikilink metacharacters survive into the base. A window titled
`Issue #42 - GitHub - Mozilla Firefox` yielded
`![[2026-09-20 1432 Issue #42 - GitHub - Mozilla Firefox.mp4]]`, which
Obsidian splits at the `#` into a file named `2026-09-20 1432 Issue ` plus a
heading — neither of which exists. The `.mp4` is correctly named and sits
right beside the note; only the embed is dead, with no error anywhere.

**Fixed in the NOTE, not in `sanitize_title`, and the choice is the
interesting part.** Those four characters are perfectly legal in a Windows
or POSIX file name, so mapping them in `sanitize_title` would rename every
user's captures, throw away title fidelity (`C++ [Debug]`), and change the
base a staged capture is ADDRESSED by — the identity `is_capture_base`, the
sidecar's own `base` round-trip and `screen_recovery::classify` all key on.
Only the note was wrong, so only the note changed:
`screen_note::embed` falls back to a percent-encoded markdown embed
(`![<escaped name>](<encoded stem>.mp4)`) exactly as `tasks::parent_link`
already did for a List folder carrying the same characters. The character
set and the label escape were extracted into the new
`core::obsidian_link` and are now shared by both, so they cannot drift.

### GAP-150 · ~~Medium~~ FIXED 2026-09-20 · A cancelled, refused or failed export left an empty `Screen Captures/YYYY/MM` in the user's vault forever
`src-tauri/src/export_worker.rs` called `prepare_export_dir` (a
`create_dir_all`) and only then `check_free_space`; `grep -n remove_dir` over
`export_worker.rs`, `export_commands.rs` and `screen/src/export.rs` returned
nothing at all. So a user **Cancel** — spec §14's ordinary way out, whose
whole promise is "staged capture and timeline untouched" — plus a disk-space
refusal, an ffmpeg failure and a commit failure each left an empty dated
folder in somebody's notes permanently. AGENTS.md and the module doc both
asserted the opposite ("every refusal is ordered ahead of any vault
mutation").

The `create` → `measure` ordering is CORRECT and stays: a directory that does
not exist yet reports no free space at all. It was the claim that was wrong.
`prepare_export_dir` now returns the ancestors it created — sampled BEFORE
`create_dir_all`, the only moment the answer is knowable — and all three
non-saving exits hand them to `rollback_export_dir`, which removes them
deepest-first with `remove_dir`, **never `remove_dir_all`**: a directory that
is not empty holds something this export did not put there, and the error
`remove_dir` returns for it IS the guard. A directory the user already had is
never in the created set at all. Both documents now say what the code does.

### GAP-151 · ~~Medium~~ FIXED 2026-09-20 · A recovered capture was listed as `edited · 0:00` and offered Resume, although it can never be saved
`src-tauri/src/screen_recovery`'s `minimal_sidecar` writes
`duration_ms: 0, vault_id: ""` — nothing on disk records either once the real
sidecar is gone — and marks the rebuild `recovered: true` in the sidecar's
flattened catch-all. Two things then went wrong at once.
`Timeline::whole(0)` is the EMPTY timeline and `is_untouched` never matches
one, so `staged_commands::summary_is_edited(None, 0)` answered **true**;
and `StagedCaptureSummaryDto` dropped `recovered` entirely. The result: the
resume-or-discard list — the ONE surface the recovery sweep exists to feed —
rendered `edited · 0:00` beside a source title that was only the base name,
offered **Resume editing**, and hid the single fact the user needs, while
`export_worker::prepare` refuses an empty `vault_id` outright.

Fixed on both counts. `summary_is_edited` returns false for an unknown source
duration (unknown is not an edit) — an arm deliberately kept OUT of
`Timeline::is_untouched`, which is the export fast path's predicate and is
held byte-for-byte against a TypeScript twin by
`tests/fixtures/timeline-cases.json` (GAP-136), and where the divergence is
unreachable because `prepare` refuses a recovered capture before a timeline
is ever consulted. `StagedCaptureSummaryDto` carries `recovered` (a `true`
BOOLEAN only, since the sidecar is hand-editable), the row reads
"recovered · length unknown", and **Resume is not rendered at all** — Discard
is the only honest action left.

### GAP-152 · ~~Medium~~ FIXED 2026-09-20 · Staging recovery postponed on `CaptureGuard` alone and knew nothing about `ExportState`, so a live export's temp was protected only by the 60 s mtime window
`src-tauri/src/screen_recovery.rs`'s `should_postpone(active) { active
.is_some() }`, against `grep -n ExportState src-tauri/src/screen_recovery.rs`
returning nothing. `sweep_staging_dir` classifies a stale
`.<base>.export.mp4.part` as `Entry::ExportTemp` and deleted it
unconditionally, justified in a comment as safe because "the staged capture
it came from is still on disk" — which is equally true of a temp being
written right now, so it was never the guard.

The window does not hold: `ffmpeg_args::reencode_args` builds `trim`/`atrim`
+ `concat` with **no `-ss`**, so ffmpeg decodes from zero and emits no output
packets until the first KEPT span. An edited export of a long recording that
keeps only late footage writes its header at T0 and then nothing for minutes;
its mtime never advances, so it reads stale at 60 s — and the recovery thread
is alive for exactly that long, because a `pending` file keeps it retrying
every 90 s for up to 24 h.

`should_postpone(active, exporting)` now reads BOTH sources, the run loop
passes `is_exporting(&app)` (one process-wide `ExportState` reservation, so a
bool suffices), and the wrong comment on the delete arm is replaced with the
real reasoning. The predicate is unit-tested and the CALL SITE is pinned by
its own structural test, because a correct predicate handed a hardcoded
`false` postpones nothing and the run loop needs a live `AppHandle`.

**Residual, recorded rather than fixed:** on Windows the unlink would have
failed anyway — ffmpeg's output handle is opened through the CRT without
`FILE_SHARE_DELETE`, so both `DeleteFileW` and the `FileDispositionInfoEx`
path `std::fs::remove_file` prefers return a sharing violation, and `delete`
degrades to a `log::warn!`. That is luck rather than a guard, and it is
reasoned from the Win32 sharing rules, not measured: no runner in this
repository is Windows, and this path executes in no automated test anywhere
(GAP-140's class).

### GAP-153 · ~~Medium~~ FIXED 2026-09-21 · The AUDIO domain's note embed has GAP-149's exact bug, via `rename_capture`, and has shipped with it far longer
Found while fixing GAP-149, out of that fix wave's scope, and recorded rather
than fixed because it needs a decision GAP-149's did not.

`src-tauri/core/src/capture_note.rs:138` emits `![[{mp3_file_name}]]` and
`:167` emits `![[{stem}.transcript]]`, both unconditionally.
`core/src/capture_paths.rs:99`'s own `sanitize_title` — a DIFFERENT function
from `screen/src/staging.rs`'s, with a different character set — filters
`/ \ < > : " | ? *` and control characters, and like the screen one it lets
`#`, `^`, `[` and `]` through. Verified directly:
`sanitize_title("Sprint #4 retro [draft]")` returns that string unchanged.

A recording's base is safe **at start** (`capture_commands.rs:236` uses
`cfg.mode.label()`, a fixed `"Meeting"`/`"Voice Note"`), so this is reachable
only through `rename_capture`, where the title is typed by the user. Rename a
meeting to `Sprint #4 retro` and the note's embed becomes
`![[2026-07-04 1405 Sprint #4 retro.mp3]]`, which Obsidian splits at the `#`
— a dead embed beside a correctly-named `.mp3`, no error anywhere. The
transcript embed on the next line breaks identically.

**Why it is not a copy of GAP-149's fix.** `capture_note::retarget_embed`
rewrites the embed by matching the LITERAL line `![[{old}]]`, so a
markdown-form embed would need that retarget to recognise both shapes or a
rename would silently stop following the file — which is a worse failure than
the dead link. And unlike a screen capture, an audio recording's note can
already have been hand-edited by the user between the write and the rename.
**Fix shape:** reuse `core::obsidian_link` (the module GAP-149 extracted, which
already holds the character set and the label escape) in `render_note`, and
teach `retarget_embed` both forms in the same change, with a round-trip test
that renames a metacharacter title twice. Do not fix the writer without the
retarget.

> **Fixed, exactly as prescribed.** Both emit sites and `retarget_embed` now
> live in `src-tauri/core/src/capture_embed.rs` — extracted because the fix
> plus its tests took `capture_note.rs` to 838 nonblank against the 800 cap,
> so the file references above are stale. They share a module on purpose: the
> markdown shape has ONE producer, consumed by the writer and the retarget
> alike, so the two cannot drift.
>
> `sanitize_title` is unchanged, per this entry and the reasoning AGENTS.md
> already records for the screen side. Only the note was wrong.
>
> The rename-twice test is the proof the halves are wired, and it fails in
> BOTH crates when the retarget is reverted — including end to end through the
> real `rename_plan` and `execute` on disk, with the embeds still naming the
> pre-rename title.
>
> **Residual: notes already on disk are NOT repaired.** Nothing sweeps them, so
> a note whose embed was broken by an earlier rename stays broken. But it
> self-heals on the NEXT rename, because a pre-existing dead embed is exactly
> the literal wikilink line the retarget still matches first — pinned by
> `retarget_rewrites_a_pre_existing_wikilink_embed_to_either_form`. Renamed to
> a plain title it becomes a working wikilink; to another metacharacter title,
> a working markdown link; never renamed again, it stays dead.

### GAP-154 · High · Alt+F4 on the buddy destroys only `main`, leaving the process alive with four hidden windows and crash detection switched off

`src-tauri/src/window_close.rs`, the non-capturing arm of `handle_main_close`.
It does not `prevent_close()`; it logs "clean shutdown (window close)", calls
`diagnostics::mark_clean_shutdown()` and lets the default close destroy `main`,
on the strength of its own comment: *"the window is about to be destroyed and
the process exits with it."*

That comment is false. `tauri.conf.json` declares FIVE windows, all built at
startup and only ever hidden; `lib.rs`'s run handler matches `RunEvent::Exit`
alone and never prevents or forces an exit. Tauri exits when the LAST window is
destroyed, so destroying `main` leaves four live hidden webviews and the process
survives. `tray::finish_quit` is the proof the codebase already knows this: it
walks `ALL_WINDOW_LABELS` destroying every window and only then calls
`app2.exit(0)`.

Failure: the user presses Alt+F4. `main` is destroyed, so
`get_webview_window("main")` returns `None` for the rest of the process — tray
"Show / Hide" can never bring the buddy back, `show_bubble` refuses, and
`single_instance`'s reveal callback no-ops. Meanwhile `mark_clean_shutdown()`
has already latched `MARKER_GATE`, so the metronome stops heartbeating the run
marker and any later native fault goes unreported at the next launch. The
capturing arm re-triggers `window.close()` after finalizing and lands in this
same arm, inheriting it.

**NOT introduced by the screen-capture increment.** `git show
main:src-tauri/tauri.conf.json` declares three windows and `main`'s `lib.rs`
carries the identical arm and the identical comment; phase 4's `window_close.rs`
moved it verbatim and added the screen gate beside the audio one. The increment
raised the window count 3 → 5, which does not change reachability.

Fix shape: route this arm through `tray::finish_quit`, or at minimum the
`ALL_WINDOW_LABELS` destroy walk followed by `app.exit(0)`.
**Trap:** this is the application's exit path and nothing in CI exercises it —
`linux-app` only compiles the shell, and manual Windows verification is deferred
by standing decision. Land it with a Windows check, not inside an unrelated PR.

### GAP-155 · ~~Medium~~ PARTLY FIXED 2026-09-20 · No shutdown path consults `ExportState`, so quitting mid-export abandons the one write that touches a vault

> **Closed for the two quit paths.** `tray::quit` and
> `window_close::handle_main_close` now carry a third gate term, and both
> workers run `export_shutdown::cancel_if_exporting` FIRST — which sets the
> flag the export loop already polls, so ffmpeg's child really is killed
> rather than orphaned — bounded at 5 s, warning and proceeding on expiry so a
> wedged export cannot make the app unquittable. `hide_buddy` is deliberately
> excluded and a structural test forbids adding it.
>
> **Still open, and WIDER than this entry described:** the updater route.
> `commands::prepare_update_install` is a *sync* command — it must stay on the
> main thread for `save_window_state`, so it cannot sleep-wait for a cancel —
> and today it gates on **nothing at all**: not the export, and not either
> capture domain. So the updater can `std::process::exit` through a live
> recording, a live screen capture, or a live export. That is pre-existing for
> all three domains, not an export-specific residual, and closing it means
> making the prepare step async or arming a worker. See GAP-160.
>
> **Second residual, narrower:** an export already past ffmpeg and inside
> `commit_into_vault` ignores the cancel flag. The 5 s wait usually covers it
> (a same-volume `rename_noreplace` plus a small note write), but on expiry the
> video-then-note window is still theoretically reachable. The `log::warn!` is
> the evidence trail and `screen_recovery` clears the export temp next launch.

`src-tauri/src/tray.rs` (`quit`) and `src-tauri/src/window_close.rs`
(`handle_main_close`) both gated solely on
`capture_commands::recording_blocks_shutdown || screen_commands::capture_blocks_shutdown`.
Measured: every `ExportState` reference in the shell outside `export_commands.rs`
is in `screen_recovery/{mod,decide}.rs`, `staged_commands.rs` and `lib.rs`'s
`.manage` — there is no shutdown reference at all.

So a user who starts a ten-minute export and then quits from the tray (or
Alt+F4s) hits `finish_quit` immediately: neither capture domain is live, every
window is destroyed and `app2.exit(0)` runs while the `screen-export` thread is
mid-`commit_into_vault`. Because that commit is video → note → staged-removal,
the process can die between the video's landing and the note, and
`Prepared::created_dirs` never reaches `rollback_export_dir`. The updater's
`std::process::exit` route had the same exposure — that is GAP-160, now
fixed; since then all three doors read `shutdown_gate::shutdown_is_blocked`.

Worse, the ffmpeg child is a separate process: nothing kills it on the way out,
so it keeps writing `.<base>.export.mp4.part` into staging after the app is gone.

This is asymmetric with the rest of the design — both capture domains get
`finalize_if_recording` / `finalize_if_capturing` on a `shutdown-finalize`
worker, and the export is the only one of the three that touches a vault.
AGENTS.md's "The quit path carries the same pair" is silent on the export, which
reads as coverage.

Fix shape: add `export_commands::export_blocks_shutdown(app)` to both gates, and
in the `shutdown-finalize` worker set the cancel flag and wait bounded on the
reservation clearing — a cancel already kills the child, deletes the truncated
output and rolls the directory back. Cancelling beats waiting: an export is
repeatable and the staged capture is kept.

### GAP-156 · Medium · Spec §14 promises an explicit "disk full" stop during capture that does not exist

The error-handling table says: *"Disk fills during capture | Capture stops and
finalizes; the partial capture is staged and offered, with an explicit
'stopped: disk full'."*

Measured: `grep -rniE 'disk.?full|no space|ENOSPC'` over `src-tauri/screen/src`
and `src-tauri/src` finds no production detection — every hit is a test fixture
string or the EXPORT-side pre-flight free-space check
(`export_worker/vault_dir.rs`), which runs before a save and says nothing about a
capture in progress. Nothing polls free space during a capture and there is no
typed disk-full stop; ENOSPC surfaces as a generic `ScreenError::Io`/`Sink`.

The `Retained` path still preserves the footage, which is the half that matters.
The promised message does not exist.
Fix shape: either poll free space on the mux thread with a typed stop, or
reconcile the spec row the way the export-temp row directly above it was
reconciled after phase 5.

### GAP-157 · Low · `retainedPath` is typed frontend state that nothing renders

Measured: `grep -rn retainedPath src/` matches only
`src/stores/screenCapture.ts` (declared, assigned from `screen:failed`, cleared).
No component reads it.

AGENTS.md states the path is carried *"TYPED, not stringified into the message,
so a caller can offer the retained file to the user instead of only logging its
location"*. Nothing offers it. The user's only route to a capture that failed
AFTER writing real footage — the entire reason the fragmented-MP4 container was
chosen — is the raw path interpolated into the failure toast by
`ScreenError::Retained`'s `Display`, on Windows possibly in `\\?\` extended form.

Fix shape: render it (an actionable toast, or a persistent line in
`ScreenCaptureBar` while `retainedPath !== null`), or delete the field and the
claim. A typed field nothing reads is a promise made in three documents and kept
in none.

**Related, and cheap:** `screen_recovery` only `log::info!`s, while audio
recovery toasts "Recording recovered" (`capture_commands.rs`). A user who never
opens Record Screen never learns a recording was recovered at all.

### GAP-158 · Low · docs/Gaps.md records no tombstone for its own retired number

The backlog jumps 103 → 105. GAP-104 is deliberately retired — three phase plans
say *"GAP-104 is retired and must never be reused"* — but this file carries no
note, so the hole reads as an accident and invites reuse. **GAP-104 is retired.**

Separately, five gap ids are cited from shipped code and defined nowhere:
GAP-55, 60, 90, 91, 92 (measured: 146 entries defined, 55 distinct ids cited
across `.rs`/`.ts`/`.vue`). Those are task/document-domain entries closed and
DELETED, whereas the screen era closes with a strikethrough and keeps the entry.
Two conventions in one file.

### GAP-159 · Low · `assert_every_exit_is_paired` has one documented false negative

`src-tauri/src/structural_scan.rs`. The shared exit-pairing walk tracks brace
depth and arms per block, which is what lets the idiomatic
`.map_err(|e| { rollback(..); .. })?;` pass while still catching a bare
`return Err(..)` on a sibling branch.

Its one blind spot: a `}` and an unguarded `return Err(` **sharing a single
line** are not distinguished, because the line is checked before its closing
braces are applied. Written across two lines, the same code is caught.

Both current call sites are otherwise STRICTER than the language elsewhere (an
exit inside a closure ahead of the release also trips them), which is the safe
direction — it produces false alarms, not false confidence. Recorded so the
next author does not discover the gap by shipping through it.


### GAP-160 · ~~Medium~~ FIXED 2026-09-20 · The updater exits the process through a live recording, screen capture or export, gating on nothing at all

`src-tauri/src/commands.rs`, `prepare_update_install`. The updater flow is
check → download → `close_panel` → `prepare_update_install` → `install()` →
`relaunch()`, and the prepare step calls `save_window_state` and
`mark_clean_shutdown` and returns. It consults **no** shutdown predicate:
not `capture_commands::recording_blocks_shutdown`, not
`screen_commands::capture_blocks_shutdown`, and not
`export_shutdown::export_blocks_shutdown`.

So installing an update mid-capture strands a `.part`, and mid-export kills the
one operation that writes into a vault — the failure GAP-155 closed for the
tray and Alt+F4 paths, reachable here by a different door. Found while closing
that entry.

It cannot simply inherit the same fix: `prepare_update_install` is a **sync**
command, and deliberately so — it must run on the main thread for
`save_window_state`, whose off-main variant caused the original drag deadlock.
A sleep-wait there would freeze the very event loop the wait depends on.

Fix shape: make the prepare step async (it touches a window API, so the
window-thread invariant has to be honoured — marshal the save back via
`run_on_main_thread` the way `finish_quit` already does), or arm a worker that
finalizes/cancels and then drives the install. Either way the three predicates
belong in one helper so a fourth exit path cannot miss one, the way this one
did.

> **Fixed.** `prepare_update_install` now REFUSES when any of the three is
> live, before `close_panel`, `save_window_state` or `mark_clean_shutdown` —
> so a refused install never latches crash detection off, nor hides the panel
> the message is read in. It stays SYNC: the fix shape above offered "make the
> prepare step async … or arm a worker", and **neither was taken**, because
> async re-opens the very deadlock the paragraph above forbids. Refusing is
> also right on the merits — unlike a tray quit the user is present, having
> just clicked Install and restart.
>
> **The other half was the real fix.** The store called it as
> `invoke("prepare_update_install").catch(() => {})`, so a Rust refusal would
> have been swallowed and `install()` would have run anyway: the Rust change
> alone would have looked complete and protected nothing. The catch is gone,
> the install aborts, and `rearm_crash_detection` is guarded on having
> actually prepared.
>
> The three predicates now compose in one `shutdown_gate`, and both quit doors
> were re-pointed onto it; their structural pin was re-pointed rather than
> deleted and is now stronger — it fails if `hide_buddy` grows EITHER
> spelling.
>
> **Residuals:** a narrow TOCTOU between the gate returning `Ok` and
> `install()` running — the same gap the tray and Alt+F4 doors have between
> their gate and `finish_quit`, not closable by any lock this app could hold
> across a process-replacing install; and a refusal rendering through the
> `error` phase, since the machine has no `refused` state.

### GAP-161 · ~~Critical~~ FIXED 2026-09-21 · The export named no output format, so every save failed — and the round trip that exists to prove otherwise used a filename production never mints
Found by running the app on Windows, not by any gate. Every **Save to vault**
failed before ffmpeg wrote a byte:

```
Unable to choose an output format for
'...\screen-captures\.2026-09-21 0848 Screen Capture.export.mp4.part';
use a standard extension for the filename or specify the format manually.
```

ffmpeg picks its muxer from the output's EXTENSION unless `-f` says
otherwise. The export writes to `staging::export_part_file_name(base)` —
`.<base>.export.mp4.part` — so a killed or crashed export can never be
mistaken for a finished one, and `.part` names no format. Both paths were
affected (`remux_args` and `reencode_args` alike), so the ninth sanctioned
vault write had **never once landed on real hardware** across the whole of
phase 5.

**FIXED** by naming the container explicitly in both builders
(`ffmpeg_args::OUTPUT_FORMAT`). The `.part` convention is deliberately NOT
what changed: it is load-bearing across both capture domains,
`screen_recovery::classify` keys on it, and renaming the temp to something
ffmpeg can infer would trade a total failure for a subtler one. Being
explicit about the container is correct whatever the file is called, which is
what makes the two facts independent.

**The reason it shipped green is the part worth keeping.**
`screen/tests/export_roundtrip.rs` is this feature's only executable
end-to-end proof and the stated reason the export route left Media Foundation
(see AGENTS.md's phase-5 bullet, and PR #79's own description). Every one of
its seven round trips invented its own destination — `edited.mp4`,
`remuxed.mp4`, `reordered.mp4`, `cancelled.mp4`, `fast.mp4`, `slow.mp4` — all
of which ffmpeg can infer a muxer for. **The suite proved the export worked
against a filename the app never produces.** Pointing the tests at
`staging::export_part_file_name` reddens SIX of the seven immediately, each
with the user's exact message.

This is the campaign's signature defect in its most expensive form, and the
generalisation is the lesson, not the `-f`:

> A test that constructs its own version of a production value proves the
> code works on the test's value. Where the value is an INPUT the code
> branches on — a filename, an extension, a path shape, an id — the test must
> mint it through the same function production calls, or it is testing a
> sibling of the real thing.

**Residuals:**

- `export.rs`'s two own tests still use `out.mp4`, and that is deliberate:
  both point ffmpeg at `/nonexistent` and assert no child is ever spawned, so
  the name cannot matter. Changing them would be noise.
- CI proves this on **Linux** ffmpeg. Windows ffmpeg is a different binary
  with a different muxer set, and nothing automated runs it — checklist row
  38 is the only evidence there will be, and it says not to skip it as a
  duplicate of row 29.
- Nothing audits the OTHER direction: a test that mints a production value
  correctly but then asserts against a hand-written expectation. No sweep has
  been done for that shape elsewhere in this feature.

### GAP-162 · ~~Medium~~ FIXED 2026-09-21 · The editor's layout was never tested at any window size, and happy-dom cannot be
Two defects from one hardware session, one root cause, both invisible to every
gate: `EditorRoot` was a fixed `h-screen` flex column whose preview `<video>`
was `w-full` with no height bound, so widening the window made the video
TALLER and the column carried more content than window. The timeline strip
collapsed to a hairline (`h-16` is a height, not a minimum, and a flex item
with no intrinsic content height yields first), and maximised, **Save to
vault** and **Discard** were pushed past the bottom edge with nothing to
scroll — the capture could be edited and never saved.

**FIXED** (`tests/editorLayout.test.ts`): the preview takes the slack
(`flex-1 min-h-0`), everything below it is `shrink-0`, and `main` gains
`overflow-y-auto` for the case where the chrome alone exceeds the window.
(An earlier draft of this entry credited `object-contain` with preventing a
stretch. It does not: `object-fit: contain` is Chromium's own UA default for
`<video>`, measured in a real browser. The class is a pinned INTENT, not a
fix, and the test comment now says so.)

**The TESTING half is now closed too** (`tests/e2e/editorLayout.spec.ts`,
`playwright.config.ts`): Playwright drives the built `dist/` in real Chromium
at 960x640, 1280x720, 1600x900 and 1920x1080, measuring `boundingBox()` and
`main`'s own scroll height. Restoring the three pre-fix files reddens all
four sizes with `the timeline strip collapsed to 0px at 1920x1080`, and the
same mutation puts `main` 97px (960x640) and 197px (1920x1080) past its
container with **Save to vault**'s top at 705px and 1245px — below the fold at
both. It runs in `frontend` after the build, and needs no Tauri runtime: a
stub installs `__TAURI_INTERNALS__` with the editor's window label and a
canned `invoke`.

**Three things about it are worth knowing before trusting or extending it:**

- **The video fixture is the control, not scenery.** A `<video>` with no
  media has no intrinsic aspect ratio and renders at the CSS default 300x150,
  where the defect cannot reproduce at all — the first version of this suite
  passed against the pre-fix layout for exactly that reason. `the fixture
  really loads` asserts 1920x1080 and every other test waits on
  `videoWidth > 0`. The fixture is VP9/WebM because Playwright's bundled
  Chromium carries no proprietary codecs; an H.264 clip fails with
  `video.error = 4` and no other symptom.
- **Two assertions in the first draft could not fail, and both were the
  documentElement/`main` confusion.** The scroll container here is `main`
  (`h-screen` + `overflow-y-auto`), so `document.documentElement.scrollHeight
  - window.innerHeight` reads 0 at every size forever. The short-window test
  had the same problem from the other end: at its original 900x360 the chrome
  (230px) fit comfortably, so `scrollIntoViewIfNeeded` scrolled nothing and
  the assertion passed having exercised no fallback. It is now 900x160 and
  asserts the precondition — Save really is below the container's bottom edge
  — before scrolling.
- **A third test was deleted rather than fixed**: it asserted
  `getComputedStyle(video).objectFit === "contain"`, which is the UA default
  (see above) and unfailable either way.

**What is still open:**

- Chromium only. WebView2 is Chromium-derived but not identical, and the app
  ships on WebView2. Checklist row 37 remains the only evidence there.
- Four viewport sizes, one window. No other window has any layout coverage,
  and the panel is the one users resize least — but also the one with the
  most surfaces.
- It measures the editor's EMPTY-ish state driven by a stubbed `invoke`. A
  layout that only breaks under real content (a very long source title, a
  timeline with fifty segments) is not covered.

### GAP-163 · Medium · Neither Linux gate compiles a `cfg(windows)` body, and one shipped broken through both
AGENTS.md sells the Linux shell build as catching "type errors, IPC signature
drift, and missing `cfg` gates locally instead of push-and-wait". True of
everything it COMPILES — and a `#[cfg(windows)]` body is exactly what it does
not. The other Linux gate cannot cover it either: `cargo clippy -p vault-buddy
--target x86_64-pc-windows-msvc` cannot run in this container at any scope
(`ring` fails in cc-rs for want of MSVC's `lib.exe`), which is the documented
reason `set_display_affinity` lives in `vault_buddy_screen` instead.

**A real commit fell through the hole.** Splitting `window_upkeep.rs` out of
`lib.rs` at the 800-line cap moved this into a sibling module:

```rust
#[cfg(windows)]
if commands::primary_button_down() { return; }
```

In `lib.rs` that resolved (`mod commands;` puts it at the crate root); from a
sibling it needs `crate::commands::`. `cargo fmt --check`, workspace clippy,
the shell's 205 tests and a full `tauri build --no-bundle` were **all green**,
and `cargo check -p vault-buddy` reports **0 errors** for the broken code
today — measured. The Windows job failed the build with E0433 eleven minutes
after the push.

**Partly closed** by `src-tauri/src/cfg_windows_guard.rs`: a source scan
requiring every sibling-module path inside a `cfg(windows)` region to be
`crate::`/`super::`/`self::`-qualified or brought in by a `use`. It reddens on
the exact line that failed CI, naming file and line, and a second test pins
its own parser and file set so a scan that found nothing could not pass
forever. Run against the whole shell it found no other instance.

**What it does NOT close, and must not be read as closing:**

- It catches the **unresolvable-path** class only. A type error, a wrong
  argument count, a moved field, a borrow error or a changed signature inside
  a `cfg(windows)` body is still invisible until the Windows job runs. That is
  most of what a compiler does.
- It is a text scan with a 30-line window after each `cfg(windows)` attribute.
  A gated `mod`, or a long gated `fn` whose offending line sits further down,
  is outside its reach.
- It says nothing about `cfg(windows)` code in the member crates — only the
  shell. `vault_buddy_screen` genuinely cross-checks under the Windows target,
  which is why its `cfg(windows)` arms at least type-check (GAP-140); the
  shell has no equivalent and this is the substitute.
- The real fix is making the shell cross-check for `x86_64-pc-windows-msvc`,
  which needs `ring` to build — a vendored `lib.exe`, a different TLS backend,
  or dropping the dependency that pulls it. Worth pricing before the next
  extraction moves code across a `cfg` boundary again.

### GAP-164 · ~~Medium~~ FIXED 2026-09-21 · An empty monitor name put "Region on " — dangling preposition and all — into a user's vault, because three fallbacks caught `Err` and not `Ok("")`
Found by the manual Windows pass, by reading a log line nobody was
scrutinising. `screen/src/source.rs` had the same construction at three
sites:

```rust
let title = m.name().unwrap_or_else(|_| format!("Screen {index}"));
```

`Monitor::name()` is `Result<String, _>`, so that reads as though it covers
"this monitor has no usable name". It does not — it covers `Err` alone, and
an `Ok("")` passes through as a perfectly valid empty label. On the
2026-09-21 machine that is precisely what came back:

```
screen capture: started (Region on ) -> ...\.2026-09-21 1152 Region on.mp4.part
screen export: saved 2026-09-21 1152 Region on to C:\...\Screen Captures\2026-09-21 1152 Region on.mp4
```

**It did not stop at the log, which is why this is Medium and not Low.** The
resolved title is the capture bar's label, the staging sidecar's
`sourceTitle`, and — through `sanitize_title` and `capture_paths::base_name`
— the name of the `.mp4` the export commits into a vault. So a file called
`2026-09-21 1152 Region on.mp4` is now in somebody's notes, permanently:
nothing in this app renames a saved capture (GAP-142), so the only remedy is
Explorer.

`sanitize_title` is what kept it merely ugly rather than broken — it trims a
trailing space, because Windows silently strips one from a file name and a
name ending in one stops matching the name that was reserved. The reservation
machinery was never at risk. The *reading* was.

**FIXED** by one pure function, `source::display_label(name: Option<&str>,
index: usize)`, used by all three sites: the picker's monitor list, the
whole-screen resolve, and the region resolve. It takes `Option<&str>` rather
than the `Result` deliberately — that makes the caller spell `.ok()` and
leaves the function one question, "is there a usable name?", which is the
question the three sites kept answering wrong. Whitespace-only is treated as
absent for the same reason empty is: neither tells a reader which screen this
is. A test asserts all five arms and reddens with `left: ""` / `right:
"Screen 2"` when the emptiness filter is removed — the user's exact symptom,
reproduced.

**The TypeScript twin was deliberately NOT changed.**
`src/utils/regionLabel.ts` formats `Region on ${monitorTitle}` from whatever
`list_capture_sources` returns, and site one is now guaranteed non-empty, so
it is fixed transitively. Adding an emptiness rule there too would be a
second implementation of the same decision, one IPC hop apart, in a codebase
that already carries GAP-136 for exactly that shape — and it has no index to
fall back to anyway.

**What this says about the gates is the larger half.** Every Linux gate was
green on the broken code and stayed green on the first draft of the fix:
`cargo test` passed, workspace clippy passed, the new unit test passed. The
three call sites are inside `cfg(windows)`, and the first version of
`display_label` took `u32` while all three sites hold a `usize`
(`Monitor::index()`'s type) — the old `format!("Screen {index}")` accepted
any `Display`, which is why the original compiled. **`cargo clippy -p
vault_buddy_screen --target x86_64-pc-windows-msvc` caught all three**, and
nothing else here could have: that is GAP-163's hole, and this is the second
time in one day a `cfg(windows)` body would have shipped broken. Run that
command for any change touching this crate's Windows arms.

**Residuals:**

- Why Windows returned an empty name for that display is not established.
  Nothing here can reach it, and it does not matter to the fix — an
  unnameable monitor is exactly what a fallback is for — but it means the
  `Ok("")` case is real on shipped hardware and not theoretical.
- The already-saved file in the user's vault is not renamed by this or
  anything else (GAP-142).
- The identical `.unwrap_or_else(|_| ...)`-over-a-`Result<String, _>` shape
  has not been swept for elsewhere in the codebase. This one was found by
  eye, in a log line, during a manual pass.

### GAP-165 · Medium · An APPROVED design for the region-capture indicator was never implemented, and nothing tracked that
Reported again by the 2026-09-21 manual pass: *"when recording a region, it
draws the yellow border around the whole screen and does not draw a region
box; the recording looks correctly bounded to the region."*

That is not a new finding. It is
`docs/superpowers/specs/2026-09-20-region-capture-indicator-design.md`,
**status: approved (2026-09-20)**, whose own Context paragraph describes the
exact symptom and says it "Lands on: `claude/screen-capture-intake-g0j49q`
(PR #79), after Phase 4's editor tasks". It did not land. Measured on this
tree:

- No implementation plan in `docs/superpowers/plans/` (phase 3's region
  overlay is there; the indicator is not).
- `grep -rn "region-indicator\|region_indicator" src-tauri/src/
  src-tauri/tauri.conf.json src/` returns **nothing**. The sixth window was
  never declared, and none of the five list memberships the spec enumerates
  exist.
- **No entry in this file.** That is the part worth naming: an approved,
  unimplemented design with no backlog entry is invisible to every later
  reader, so the next person to hit the symptom re-discovers it as a bug —
  which is exactly what happened.

**Why the symptom is not a bug in the capture.** Region capture resolves to
`SourceHandle::Screen(monitor)`; WGC captures the whole monitor and
`convert::bgra_crop_to_nv12` crops on the CPU (§6, kept that way so the crop
stays testable on Linux — GAP-125). Windows draws its border around what it
is genuinely capturing, which really is the whole screen. WGC captures
monitors or windows, never an arbitrary rectangle, so no change to what we
ask Windows for can retarget that border. The recording is correctly
cropped — the user confirmed it — and only the on-screen feedback lies.

**`DrawBorderSettings::WithoutBorder` is NOT the fix, and it is worth saying
why** so the next reader does not reach for it. `windows_session.rs:467`
passes `DrawBorderSettings::Default`, and the crate does expose
`WithoutBorder`, so it looks like a one-token fix. But (a) Windows gates
`IsBorderRequired = false` behind the `graphicsCaptureWithoutBorder`
restricted capability, which an unpackaged desktop app does not have, so it
would most likely fail at runtime in a `cfg(windows)` path nothing here can
test; and (b) even if it worked it is the wrong outcome — the border is
Windows' own "you are being recorded" privacy affordance, and suppressing it
on a full-monitor capture removes the only signal that recording is
happening, replacing a misleading indicator with no indicator.

**The approved answer is an indicator of our own**, enabled by
`WDA_EXCLUDEFROMCAPTURE` (§5.3): a window in `EXCLUDED_LABELS` is invisible
to screen capture while fully visible to the user, so a border we draw over
the recorded area is seen and does not appear in the recording. The spec's
own trap list is worth carrying forward — the window must be DECLARED in
`tauri.conf.json` rather than built at runtime (the config-derived label
tests and `capture_exclusion`'s one-shot apply both go blind to a
runtime-built window), it needs `set_ignore_cursor_events(true)` before it is
first shown (without it the indicator is a full-region transparent window
swallowing every click for the whole capture, "strictly worse than no
indicator at all"), and `focus: false` so it cannot blur the panel and trip
`schedule_focus_out_check`.

**Residuals:**

- The spec is approved but unreviewed against Phase 5, which landed after it.
  The export and the staged-capture list did not exist when it was written.
- No sweep has been done for OTHER approved-but-unimplemented specs in
  `docs/superpowers/specs/`. This one was found because a user hit its
  symptom; there is no mechanism that would have surfaced it otherwise, and
  the same mechanism (or absence of one) covers every other spec in there.

### GAP-166 · High · Windows Explorer's toolbar stops accepting clicks while Vault Buddy is running — UNLOCALISED
Reported by the 2026-09-21 manual pass: *"in windows explorer the top tool
bar is not clickable anymore when the recording editor window is active or
the vault buddy is active."*

**This entry deliberately proposes no cause.** Two hypotheses were formed and
both were killed by the reporter's own answers; a third would be guessing,
which is what the systematic-debugging skill exists to stop. What follows is
only what is established.

**Established:**

- It is OUR app. The reporter confirms it does **not** persist after tray →
  Quit.
- It is **not the buddy window covering it**. The buddy is 88x88 and parked
  bottom-right; the affected toolbar is at the top of the screen.
- It affects the **whole toolbar width**, not a small patch — so it is not
  any of our small transparent windows sitting over a corner of it.
- It occurs while **the editor** is the active window, and the editor is
  `alwaysOnTop: false` (verified in `tauri.conf.json`). That is the fact that
  kills the whole "a topmost window of ours is over it" family of
  explanations: when the editor has focus, nothing of ours is necessarily
  above Explorer at all.
- The 1 s metronome's always-on-top re-assert is not a candidate:
  `window_upkeep_tick` fetches `"main"` alone and returns early unless it is
  visible, so it never touches another window's z-order.

**Not established:** whether it needs a capture to have run first; whether
the panel is open when it happens; whether the first click is swallowed and a
second works. Those three are the discriminators and they have been put to
the reporter.

**Why this is High rather than Medium.** It degrades an application the user
did not launch us to affect, it is invisible from inside our app, and the
only remedy the reporter has found is quitting Vault Buddy. Whatever the
cause, a companion app that quietly breaks File Explorer is worse than any
defect inside our own surfaces.

**Constraints a fix must respect** (from the window-system section, so a
future fix does not trade this for a worse regression):

- The buddy's always-on-top is re-asserted every tick on purpose — Windows
  re-shuffles the topmost band when taskbar previews and flyouts appear, and
  no event reaches us, so dropping the re-assert puts the buddy behind the
  taskbar.
- `set_ignore_cursor_events` is NOT a blanket answer: the buddy and panel
  must stay clickable, and the one window the approved region-indicator
  design does apply it to (GAP-165) does not exist yet.
- Any change here touches `cfg(windows)` behaviour that no automated gate on
  any platform can observe (GAP-117/GAP-163), so it needs a checklist row of
  its own and a hardware re-run, not a green CI badge.
