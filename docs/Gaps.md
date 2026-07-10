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

### GAP-01 · ~~High~~ FIXED 2026-07-10 · Transcription retry/force paths accept `..` escapes and skip the capture-basename gate
`owning_vault_id` and `open_recording_note` now match on canonical paths via
`capture_paths::vault_owning_path` (unresolvable = rejected), and both
transcription commands require `capture_paths::is_capture_mp3` — the same
ownership filter `rename_plan` enforces (now shared).

### GAP-02 · ~~Medium~~ FIXED 2026-07-10 · A transient config read failure during save wipes every other vault's settings
`update_vault_config_at` and `update_mcp_config_at` now share
`read_config_for_update`: only `NotFound` defaults (first save); any other
read error logs and propagates, so the save fails loudly and the file is
left untouched.

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
open. A performance trap, not a containment violation.
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

### GAP-26 · Low · Inconsistent error strings; paths leak into user-facing errors
`commands.rs:510` (`"vault not found: {id}"`) vs the user-worded
`"Vault not found — was it removed from Obsidian?"`
(`capture_commands.rs:344`, `task_commands.rs:36`); several errors embed
absolute local paths (`capture_commands.rs:347/980`,
`task_commands.rs:130`). Cosmetic on a local desktop app.
**Fix:** unify on the user-worded form via one shared vault lookup (the
`discover_vaults().find(..)` lookup is duplicated 6× — see GAP-45).

## 4. Frontend defects & races

### GAP-27 · Medium · Escape in an open dropdown also closes the whole panel
`src/components/SelectMenu.vue:101-103` + `src/roots/PanelRoot.vue:23-25`.
`onPopupKeydown` handles Escape with `preventDefault()` but no
`stopPropagation()`; the keydown bubbles to `window`, where PanelRoot calls
`close_panel`. Dismissing the bitrate/model/duration dropdown in settings
hides the entire panel. Search's Escape handler shows the intended pattern.
**Fix:** `e.stopPropagation()` in SelectMenu's Escape branch.

### GAP-28 · Medium · The quiet startup update check can stomp a manual check or an in-flight install
`src/stores/updates.ts:61-73`.
The `phase !== "idle"` guard runs only before `await check()`. A slow quiet
check resolving after the user manually checked and hit Install flips
`phase` back to `available` mid-`installUpdate`; landing between
`download()` and `install()` makes `install()` run on a fresh,
never-downloaded `Update` object.
**Fix:** re-check `this.phase === "idle"` after the `await` before
assigning.

### GAP-29 · Medium · The rename prompt is unreachable for saves that happen while the panel is closed
`src/components/ActionPanel.vue:97-103`.
The `shownNonce` watcher calls `capture.dismissRename()` on every
`panel-shown`. A recording stopped from the tray (panel closed) arms
`lastSaved` in the hidden panel's store; opening the panel to name the
recording kills the prompt before it renders — the 30 s rename window only
works if the panel was already open.
**Fix:** only dismiss when the prompt is older than a threshold (a real
*stale* prompt), or skip the dismiss on the first show after a save.

### GAP-30 · Medium · After a failed config read, one transcription toggle rewrites the vault's capture config to defaults
`src/components/RecordMode.vue:105-118, 87`.
`loadConfig`'s `finally` sets `loaded = true` even on failure; the
`transcription` setter then `persist()`s the default-seeded config
(recordingFolder null, bitrate 128, devices null…) via
`set_capture_config`, overwriting the user's settings on disk.
CaptureSettings' `tasksFolderLoaded` gate shows the careful pattern.
**Fix:** on read failure persist only the four transcription fields, or
require an explicit save. (Pairs with GAP-02 — the Rust side makes the
blast radius all vaults.)

### GAP-31 · Medium · No IME-composition guard on the add-task Enter — a vault write
`src/components/Tasks.vue:139`.
Search guards Enter/arrows/Escape with `event.isComposing`; Tasks does not.
A CJK user committing an IME candidate with Enter immediately creates a
task document from the half-composed title (a sanctioned vault write).
`ActionPanel.vue:82`'s filter Escape has the same, lower-stakes gap.
**Fix:** ignore Enter when `event.isComposing`.

### GAP-32 · Low · Assorted store/component edges
- `src/stores/capture.ts:234-241` — `refreshWaitingForRecording` responses
  are unticketed; a slow response can re-set stale state after a newer
  event cleared it (self-corrects on the next event). Add a ticket or
  ignore when `activeTranscription` is set.
- `src/stores/vaults.ts:81-101` — `taskCounts` refreshes only on panel
  open, so the vault-row badge is stale after task edits until reopen.
  Refresh from `back()`/Tasks mutations.
- `src/components/Tasks.vue:84-86, 98-104` — failed-toggle revert forges
  `status: "new"` instead of restoring the original (`in-progress` etc.);
  the failure re-insert uses a pre-await index, restoring one slot off
  after a concurrent add. Capture the original status; recompute the index.
- `src/stores/capture.ts:242-430` — `init()` registers 14 listeners with
  no re-entry guard or unlisten storage (safe today; double-init
  double-fires everything). Roots assign `unlisten*` only after `await
  listen(...)`, leaking a listener if unmount races registration. Add an
  `initialized` flag / post-await unmount check.
- `src/stores/notifications.ts:20-26` — dedupe reuses the newest identical
  toast without extending its TTL (a re-raise at t=3.9 s vanishes at 4.0 s
  and reads as flicker); dismissed ids' timers still fire. Restart the
  timer on dedupe-reuse.
- `src/stores/vaults.ts:184-195` — `back()` carries duplicated dead
  branches; nothing enforces valid view+vaultId pairs (a null-id
  `captureSettings` renders the list under the wrong header) — unreachable
  today, unguarded. Collapse `back()`; consider one view+id state field.
- `src/types.ts:81` — `TranscriptionQueueStatus.active.progress` is typed
  non-nullable `number` while `capture.ts:63` defends with `!= null`; one
  of them is wrong. Make it `number | null` to match the defensive read.

### GAP-33 · Low · Accessibility gaps in the two listbox surfaces
- `src/components/Search.vue:260` — static `aria-expanded="true"` claims an
  always-open popup even for empty/recents states; bind to
  `visibleHits.length > 0` and add `aria-autocomplete="list"`.
- `src/components/SelectMenu.vue:144-169` — keyboard highlight is
  visual-only: no option `id`s, no `aria-activedescendant`, no
  `scrollIntoView` (a 13-item list scrolls at 220 px, so the highlight
  moves off-screen), no Home/End/typeahead — and the 4 existing tests cover
  none of the keyboard path.

## 5. Security & configuration

### GAP-34 · Medium · CSP is disabled for all three webviews
`src-tauri/tauri.conf.json:56` — `"security": { "csp": null }`. Every
window can invoke all 40 commands, four of which write into vaults; the app
renders strings derived from vault contents (search results, note titles).
`HighlightText` being index-based mitigates, but CSP is cheap
defense-in-depth for exactly the injection class that would weaponize
GAP-01/GAP-07.
**Fix:** set a restrictive CSP (e.g. `default-src 'self'; style-src 'self'
'unsafe-inline'`).

### GAP-35 · Medium · GitHub Actions pinned by mutable tag, including the one that holds the updater signing key
All three workflows: `actions/checkout@v4`, `actions/setup-node@v4`,
`Swatinem/rust-cache@v2`, `actions/upload-artifact@v4`,
`tauri-apps/tauri-action@v0` (a floating major-0), and
`dtolnay/rust-toolchain@stable` (a moving branch ref). The release workflow
feeds `TAURI_SIGNING_PRIVATE_KEY` into `tauri-action` — a compromised tag
on that action exfiltrates the key that can ship updates to every user.
**Fix:** pin all third-party actions to full commit SHAs.

### GAP-36 · Medium · CI exposes the signing secrets to same-repo PR branch builds; no `permissions:` block
`.github/workflows/ci.yml` (top level, and `windows-app` env). No
`permissions:` block means the default `GITHUB_TOKEN` scope; the signing
secrets are present during `npm ci`/`build.rs` on any same-repo branch PR
(fork PRs are safe — secrets are empty and handled).
**Fix:** add `permissions: contents: read`; consider signing only on push
to `main`/release.

### GAP-37 · Medium · `bump-version.yml` interpolates the dispatch input into shell
`.github/workflows/bump-version.yml:37` — `${{ inputs.version }}` lands
directly in a `run:` line (and later in the branch name): a workflow-command
/shell injection vector for write-access users, with a token holding
`contents: write` + `pull-requests: write`.
**Fix:** pass the input via `env:` and reference `"$VERSION"`.

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

### GAP-41 · High · The release dispatch path is unvalidated
`.github/workflows/release.yml:11-15`.
The `tag` input isn't checked against `tauri.conf.json`'s version (the
comment says it "must match" — nothing enforces it) and there is no ref
guard (unlike `bump-version.yml`): dispatching from any branch releases
that branch's code under an arbitrary tag, and a mismatch ships a
`latest.json` whose version disagrees with the tag.
**Fix:** guard `github.ref_name == 'main'` and
`inputs.tag == "v" + tauri.conf.json version`.

### GAP-42 · Medium · A release can ship from a red commit
`.github/workflows/release.yml:20` — the release job runs no tests and has
no dependency on CI success for the SHA; a tag on a broken commit publishes
and is immediately offered to every installed app via the updater.
**Fix:** gate the release job on the CI workflow's success for that SHA.

### GAP-43 · Medium · No Rust tests run on Windows (clippy half FIXED 2026-07-10)
The workspace-clippy half is fixed: `linux-app` now runs
`cargo clippy --workspace --all-targets -- -D warnings`, covering the
shell. Still open: the most platform-sensitive code (process detection,
`GetKeyState`, whisper on MSVC, WASAPI loopback) is Windows-only, yet
`windows-app` is build-only.
**Fix:** a `cargo test` step (core + capture + transcribe
`--features whisper`) in the Windows job.

### GAP-44 · Low · Release/bump edges
- ~~No CI job runs `node scripts/bump-version.mjs --check`~~ — fixed
  2026-07-10: the `frontend` job runs it before the build.
- `scripts/bump-version.mjs:107-110` — accepts a new version equal to or
  lower than current; equal input later fails at `git commit` with a
  confusing "nothing to commit". Reject `newVersion <= current`.
- ~~No `cargo audit` step~~ — fixed 2026-07-10: `cargo deny check`
  (advisories + licenses + sources, `src-tauri/deny.toml`) runs in
  `rust-core`. Still open: no `npm audit` step and no Dependabot/Renovate
  config, despite deliberate pins (whisper-rs 0.16) that need a tracked
  upgrade path.
- No SECURITY.md / key-rotation procedure for the updater keypair
  ("whoever holds it can ship updates to every user" — DEVELOPMENT.md) and
  no CHANGELOG (release bodies are boilerplate install instructions).

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
  non-Windows arm has direct contract tests, and a `cfg(windows)` twin
  awaits GAP-43's Windows `cargo test` CI step.)
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
- `devices.rs`: only "never panics" smoke tests can run on device-less CI
  runners — the format-dispatch arms, the error-callback → `Lost` path, and
  the entire `#[cfg(windows)]` loopback block are never *executed* by any
  test anywhere (Windows CI never runs `cargo test`, GAP-43).
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
- `SelectMenu.vue`'s 4 tests cover none of the keyboard path, outside-click
  close, or positioning (GAP-33).
- Event-listener cleanup paths in the roots and `capture.init()` re-entry
  (GAP-32) have no tests.

## 8. Tech debt & duplication

### GAP-45 · Shell
- `start_capture_blocking` (the async command's moved body, sub-pass B) is
  ~330 lines with four inline thread bodies (`capture_commands.rs:321-655`);
  `process_transcription` ~186 lines.
- The `discover_vaults().find(|v| v.id == id)` lookup is duplicated 6×
  across three files with two error styles (GAP-26).
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
- Inline SVG icon paths are copy-pasted (the identical gear in
  `ActionPanel.vue` and `VaultList.vue`; X-marks in three components) — a
  tiny `<Icon name>` component would end it.
- `Search.vue` (494 LOC) and `stores/capture.ts` (602 LOC) are the two
  oversized files; split when next touched.
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

### GAP-54 · Low · Capture event-ordering corners after the async migration
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

### GAP-49 · Medium · Broken/stale references in the human-facing docs
- `README.md:14` — the PRD link targets `PRD%20-%20Product%20Vision.md` at
  the repo root; the file is under `docs/`. The front-page product link
  404s on GitHub.
- `docs/DEVELOPMENT.md:76-91` — "Tests and checks" omits the transcribe
  crate from the clippy/test commands CI actually runs, and the prose says
  "split into three crates" (there are four).
- `docs/DEVELOPMENT.md:159` — says the signing secrets are needed "to
  build"; CI explicitly builds without them.
- `.github/pull_request_template.md` — claims Windows-only shell changes
  "can't compile in this container" (the Linux compile gate exists) and
  lists the CI gates as `frontend` → `rust-core` → `windows-app`, omitting
  `linux-app` and implying sequence.
- `docs/PRD - Product Vision.md:4,580,602` — status says "Shipping —
  v0.3.0" and "except Search and Tasks"; the repo is at 0.5.1 with both
  shipped. The use-cases README says to re-run reconciliation on each
  release; it wasn't.
- `.github/workflows/release.yml:47-49` — comment still claims the `tauri`
  npm script aliases `tauri dev`; `package.json` fixed that (the override
  is harmless, the justification stale).
- `src-tauri/transcribe/Cargo.toml:8-10` — comment says the Windows job is
  the whisper compile gate; the Linux `rust-core` job builds *and tests*
  the feature.

### GAP-50 · Low · Naming and structure
- `docs/PRD - Product Vision.md` — spaces in the filename force `%20`
  links, which is what produced the broken references. Rename to
  `docs/PRD.md` and fix the three referrers (README, DEVELOPMENT, and the
  AGENTS.md doc map).
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
  on the non-Windows arm; the `cfg(windows)` twin awaits GAP-43's Windows
  `cargo test` CI step).
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
