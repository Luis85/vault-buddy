# AGENTS.md

Operating guide for coding agents working in this repository. The
human-facing equivalents are [README.md](README.md) (what the product does),
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) (contributor setup, CI, releases)
and [docs/PRD.md](PRD%20-%20Product%20Vision.md) (vision, principles, roadmap). Design specs
live in [docs/superpowers/specs/](docs/superpowers/specs/).

Vault Buddy is a Windows desktop companion for Obsidian: a Tauri v2 shell
(Rust) hosting a Vue 3 + Pinia + Tailwind 4 frontend. A tiny always-on-top
transparent window shows an animated buddy; clicking it expands a panel that
lists Obsidian vaults and opens them via `obsidian://` URIs. Browsing never
writes into a vault; the opt-in capture and transcription paths are the only
writers (recordings, companion notes, transcript sidecars).

## What compiles where (read this first)

The Rust code is deliberately split so agents can work outside Windows:

| Path | What it is | Compiles on |
| --- | --- | --- |
| `src-tauri/core/` | Pure crate: obsidian.json parsing, daily-note resolution, URI building, process detection. No GUI deps. | Anywhere — test and lint locally |
| `src-tauri/transcribe/` | Pure-ish crate: MP3→PCM decode (Symphonia), model registry/download, and whisper.cpp via `whisper-rs` behind the `whisper` feature. | Anywhere with default features (no whisper.cpp); the `whisper` feature + real engine build on **Windows** (CI gate). |
| `src-tauri/` (root crate) | Tauri shell: window, tray, IPC commands, plugins. | **Windows** (release + behavior gate) — **also compiles on Linux** as a compile gate once GUI deps are installed (`npm run setup:linux`, then `npx tauri build --no-bundle`); CI runs both |
| `src/` + `tests/` | Vue frontend + Vitest suite (happy-dom, no Tauri runtime needed) | Anywhere |

When you change the shell crate (`src-tauri/src/*.rs`), you *can* now compile
it in a Linux container as a compile gate: run `npm run setup:linux` once (it
installs the WebView/GTK/tray system libs — the single source of truth is
`scripts/setup-linux-deps.sh`), then `npx tauri build --no-bundle`. This
catches type errors, IPC signature drift, and missing `cfg` gates locally
instead of push-and-wait. It is a **compile gate only** — the Windows job
remains the release + desktop-behavior gate (transparency, tray, drag, the
Obsidian round-trip). Mirror existing `cfg`-gate patterns for any
platform-specific code, run `cargo fmt --check`, and let CI's `windows-app`
and `linux-app` jobs verify the build.

## Commands

```bash
npm ci                              # install (Node 22)
npm test                            # full Vitest suite
npx vitest run tests/<file>.test.ts # single test file
npm run build                       # vue-tsc typecheck + production build
npm run dev                         # Vite dev server only
npm run test-build                  # `tauri dev` — full app, Windows only
npx tauri build                     # real installer build (Windows only)

cd src-tauri && cargo fmt --check   # rustfmt gate (whole workspace)
cd src-tauri/core && cargo clippy --all-targets -- -D warnings
cd src-tauri/core && cargo test
```

Gotcha: in anything automated, invoke the tauri CLI as `npx tauri <cmd>`,
never through npm script indirection — a past `tauri` script aliased
`tauri dev`, and `npm run tauri build` expanded to `tauri dev build`, which
launched the app on the CI runner and never exited.

## Architecture

### IPC surface (Rust commands, registered in `src-tauri/src/lib.rs`)

`list_vaults`, `open_vault`, `open_daily_note`, `prepare_update_install`,
`toggle_panel`, `close_panel`, `close_bubble`, `get_buddy_facing`,
`get_bubble_anchor`, `announce`, `start_buddy_drag`, `show_buddy_menu`,
`open_logs_folder`, `rearm_crash_detection`, plus the capture surface:
`capture_status`, `start_capture`, `stop_capture`, `pause_capture`,
`resume_capture`, `get_capture_config`, `set_capture_config`,
`list_audio_devices`, `rename_capture`, the recordings/transcription
surface: `list_recordings`, `open_recording`, `open_transcript`,
`retranscribe`, `transcribe_recording_now`, and the tasks surface:
`get_tasks_config`, `set_tasks_config`, `list_tasks`, `add_task`,
`set_task_status`, `count_open_tasks`, `open_task`, `update_task` —
commands live in `src-tauri/src/commands.rs`,
`src-tauri/src/capture_commands.rs`, and `src-tauri/src/task_commands.rs`.
Tray + buddy context menu live in `src-tauri/src/tray.rs`; menu item events
are handled in `lib.rs`.

The app is single-instance (`tauri-plugin-single-instance`, registered
FIRST in the builder — keep it first): a second launch exits immediately
and the surviving instance reveals the buddy instead.

### The window system (most invariant-heavy area)

Three separate always-on-top transparent windows, so the buddy window never
resizes. The old design was one window that grew from 88×88 to hold the
panel; WebView2 repaints its stale last frame at the new bounds for a frame
on resize, flashing the buddy to a corner. Splitting the concerns removed the
resize entirely:

- **`main`** — the buddy, fixed 88×88, the only window the user drags and the
  only one whose position is persisted. It never changes size, so it is
  structurally flicker-proof.
- **`panel`** — the vault/settings panel (360×340), created hidden.
- **`bubble`** — the greeting speech bubble (260×150), created hidden.

`panel` and `bubble` are *positioned while hidden, then shown* — a moved-only
window has no stale-frame flash. Placement is one pure function,
`core::companion_placement::place_beside(buddy, work_area, w, h, prefer, vmode)
-> (Point, Anchor)` (unit-tested on Linux): it sits the window on the `prefer`
side of the buddy, flips to the other side when that overflows the screen edge,
and aligns vertically per `vmode` — `Edge` top-aligns (the panel, flipping to
bottom-align near the bottom edge) and `Center` sits level with the buddy's
center (the bubble). It clamps into the monitor work area and returns the
`Anchor` (`side` + `valign` ∈ `Top`/`Middle`/`Bottom`) derived from where the
card actually landed, so the tail points at the buddy. `panel_position` is a
thin wrapper (prefer = `Right`, `Edge`, anchor discarded); the panel always
opens right. One shell helper, `place_beside_buddy` (in `commands.rs`), feeds
it the live buddy/monitor geometry for both windows; `position_panel` /
`show_bubble` call it. Any missing window or monitor info leaves the window
where it was (best-effort, never an error). Facing is **derived from the
buddy's position**, not a stored setting: `core::toward_center_side` picks the
side toward the work-area center (more room), and that drives BOTH the buddy
sprite and the bubble. `place_beside` prefers that side with `VMode::Center`
(the bubble sits level with the buddy), and Rust emits a `bubble-anchor`
`{side, valign}` event so `BubbleRoot` binds `SpeechBubble`'s tail to point
back at the buddy (defaulting to `right`/`middle` before the first event). The
buddy sprite gets its initial direction from the `get_buddy_facing` command on
mount, then flips on a `buddy-facing` event that Rust emits (deduped, from the
`Moved` handler + startup poll) only when the buddy crosses the screen midline
— so the character always looks toward the center. A `BUBBLE_TUCK_FRAC` overlap
(a fraction of the buddy width, so it scales with DPI) pulls the bubble into
the buddy window's transparent padding so it sits snug against the character. While the greeting is up, the
buddy's `Moved` handler re-runs `place_beside_buddy` for the bubble
(`reposition_bubble_if_visible`, keyed on the `main` window and gated on the
bubble being visible) and re-emits the anchor, so the bubble *follows* a drag
and its tail flips live when the buddy crosses the midline or an edge — a
main-thread, lock-free `set_position` that touches no window-state cache lock,
so it cannot recreate the off-main save-vs-`Moved` deadlock. The
greeting is shown via `schedule_show_bubble`
(a ~250 ms worker-thread settle, then a main-thread `show_bubble`), not
synchronously in `setup`: the window-state plugin restores the buddy's parked
position slightly after setup, and a synchronous placement would anchor the
bubble to the buddy's pre-restore default corner. Invariants:

- **Window show/hide and the placement getters run on the MAIN thread only.**
  `toggle_panel`, `close_panel`, `close_bubble` are *synchronous* commands
  (custom commands aren't capability-gated — only `core:`/`plugin:` are), so
  they run on the main thread where the window getters, `set_position`,
  `show`/`hide` and `set_focus` are valid. `toggle_panel` positions the hidden
  panel, shows it, focuses it, emits `panel-shown`, and hides the bubble;
  opening never touches the buddy window. `panel-shown` is the panel webview's
  precise "opened" signal — `PanelRoot` re-runs discovery and picks its view on
  it (window focus is a leaky proxy that also fires on a mere refocus). Every
  exit path and the updater reuse these commands — there is no offset/shift to
  undo, because the buddy never moves to make room.
- **The panel closes itself when focus really leaves the app.**
  `schedule_focus_out_check` is fired only from the **panel** window's
  `WindowEvent::Focused(false)` (keyed on `window.label() == "panel"`): only
  the panel's own blur can mean "clicked away from the panel". Scheduling on
  every window's blur spawned a worker thread per blur (the buddy blurs
  constantly) and, worse, the buddy blurs AS the panel takes focus on open, so
  a check fired from that could hide the just-opened panel before its focus
  landed. The check cannot sample focus inline: clicking from panel to buddy
  fires the panel's blur BEFORE the buddy's focus lands, so an inline check
  would see neither focused and wrongly hide. `run_on_main_thread` alone won't
  defer it — that runs the closure INLINE when called from the main thread
  (where window events are dispatched). So it sleeps 120 ms on a named worker
  thread, then marshals the check back to the main thread. A thread-spawn
  failure is logged, never `.expect`-panicked (the handler runs on the main
  thread, where a panic aborts across the WebView2 FFI boundary). The check
  only ever HIDES, never shows, so it can never fight `toggle_panel` into a
  reopen: a buddy click that closed the panel leaves the deferred check a
  no-op.
- Buddy drags go through the `start_buddy_drag` command, never the raw
  `startDragging()` JS API. Being synchronous it runs on the main thread,
  where it re-checks the **logical (swap-aware) primary button** via
  `GetKeyState(VK_LBUTTON)` right before entering the OS move loop and
  drops requests that went stale in IPC transit (a stale synthetic
  WM_NCLBUTTONDOWN starts a buttonless "sticky" move loop on Windows). The
  re-check is **mouse-only** (a touch/pen contact reports `buttons=1` to the
  webview but need not surface as `WM_LBUTTONDOWN`), so the frontend passes
  the pointer type. The command returns whether the drag actually started, and
  the frontend refuses to start one from a pointermove whose button is already
  up. Dragging the buddy closes the panel (`BuddyRoot` invokes `close_panel`
  on drag-start): the panel is its own window now, so it simply hides instead
  of riding the buddy along.
- **Window-state saves and window getters run on the MAIN thread only.**
  `save_window_state` takes the plugin's cache lock and then reads window
  geometry; the plugin's Moved listener takes the same lock on the main
  thread. An off-main save colliding with a drag's Moved flood deadlocked
  both threads and froze the app with no crash record (the original
  "drag crash"). The 1s loop in `lib.rs` is therefore a pure metronome: it
  posts `window_upkeep_tick` (always-on-top re-assert + position
  checkpoint) to the main thread via `run_on_main_thread` with backpressure
  (at most one closure outstanding), and it warns when the main thread
  stops servicing those closures. `finish_quit` marshals its save the same
  way for the finalize-on-worker quit path; `prepare_update_install` must
  stay a synchronous command for the same reason.
- The upkeep tick never touches a window in motion: it is gated by both the
  time since the last Moved event (2s quiescence) and a direct primary-button
  check (a drag paused with the button held emits no Moved events), and the
  position checkpoint (`core::checkpoint::PositionCheckpointer`, unit-tested
  on Linux) only persists a position after it has settled — never on the
  tick a change is first seen. First save still waits out the window-state
  plugin's restore. Exit-time saves alone proved lossy (the updater kills
  the process via `std::process::exit`).
- The metronome thread heartbeat-refreshes a run marker (`app_diagnostics`
  in the core crate) that detects unclean shutdowns the panic hook
  structurally cannot see (native faults, kills, power loss) — every
  graceful exit path (tray/buddy quit, Alt+F4 close, update install) must
  stamp `diagnostics::mark_clean_shutdown()`. All hide paths funnel through
  `tray::hide_buddy`, which hides all three windows (`panel`, `bubble`,
  `main`) and no-ops mid-recording (the buddy is the recording indicator).

### Updater flow (`src/stores/updates.ts`, `UpdateSettings.vue`)

Check → download (panel stays open so spinner/errors are visible) →
`close_panel` (hide the panel window) → `prepare_update_install` (Rust saves
the buddy position and stamps a clean shutdown) → `install()` → `relaunch()`.
The buddy window never shifts, so there is no home position to restore. On
failure the panel reopens on the settings view via `toggle_panel`, `available`
is kept so the install button stays visible for retry, and
`rearm_crash_detection` turns the run marker back on (the prepare step latched
it off). The `Update` object is stored with `markRaw()` — a Vue reactive
proxy breaks its private-field `rid` and every real install would throw.

### The vault domain (core crate + `vaults` store)

Hard rule, amended by the Knowledge Intake increment: **the vault domain
never writes into a vault** — opening notes and creating daily notes is
delegated to Obsidian via `obsidian://` URIs, and every launched URI is
logged (`uri::launch`) as the audit trail. Two sanctioned write paths exist,
both below: the capture domain, which stores recordings and their companion
notes under strict safety rules; see
`docs/superpowers/specs/2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md`.
The transcription worker (increment 3) adds a second sanctioned write, a
`<base>.transcript.md` sidecar the note embeds, extending the same write
path rather than inventing a new one — same never-clobber/atomic rules, and
a `vault-buddy-transcript: pending/failed/complete` frontmatter marker means
only `pending`/`failed` sidecars are ours to replace, so a completed
transcript or a user's hand edit is never overwritten; see
`docs/superpowers/specs/2026-07-04-increment-3-local-speech-to-text-design.md`.
The tasks domain (below) adds two more sanctioned writes under the same rules —
creating a task document (collision-safe) and a surgical multi-key frontmatter
field write (status toggle, rename, due/priority edit — all one generalized
writer) — see
`docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md`
and `docs/superpowers/specs/2026-07-09-tasks-todo-list-design.md`.
Any other code touching vault contents directly is a design change, not a
patch.

Data flow: `%APPDATA%\obsidian\obsidian.json` → `discovery.rs` →
`list_vaults` (open-flag scrub) → `vaults` Pinia store → `VaultList.vue` →
`open_vault` / `open_daily_note` → `uri.rs` → OS URI handler → Obsidian.

- **`discovery.rs`** parses Obsidian's own registry into
  `Vault { id, name, path, open }`. The `id` is the registry's hex key; the
  display name is the last path component (split on `/` **and** `\` —
  obsidian.json stores backslash paths on Windows but tests run on Unix).
  Lists sort case-insensitively by name. Malformed or missing config always
  degrades to an empty list, never an error.
- **`process.rs`** exists because the registry's `open` flag survives a full
  Obsidian quit (that's how Obsidian restores vaults on relaunch).
  `list_vaults` clears all open flags when no Obsidian process is running;
  the name match requires the exact executable or a real delimiter
  (`obsidian`, `Obsidian.exe`, `Obsidian Helper …`) so tools like
  `obsidian-sync` don't count.
- **`daily_notes.rs`** reads each vault's `.obsidian/daily-notes.json`
  (folder + moment-style format). Only the `YYYY`/`MM`/`DD` tokens are
  supported, matched as whole letter runs — `MMMM` or `YYYYMMDD` fall back
  to the default format entirely rather than half-substituting, because a
  wrong literal path would make Obsidian silently create a misnamed note.
  The rendered path is vault-relative **without** `.md` (the URI `file`
  parameter's expected form).
- **`uri.rs`** addresses vaults by **ID, never name** (folder names can
  collide across vaults) and percent-encodes every parameter.
  `daily_note_uri` (in `core/src/lib.rs`) picks `obsidian://open` when the
  note file exists and `obsidian://new` otherwise — creation happens inside
  Obsidian.
- **`vaults` store** re-runs discovery on every panel open (one JSON read;
  a user who just launched Obsidian must not stay stuck on a cached empty
  list) but keeps the previous list when a refresh fails transiently, so a
  working panel never blanks. Launching a vault closes the panel; a failed
  launch keeps it open with the error banner.
- **`VaultList.vue`** surfaces `open: true` vaults first under an "Open
  now" header (flat list when nothing is open); the name/path filter only
  appears above 5 vaults.

### The capture domain (`src-tauri/capture/` + `capture_commands.rs` + `capture` store)

One-click meeting/voice recording into the vault (Knowledge Intake,
increment 2). `vault_buddy_capture` owns the audio engine: cpal devices
(WASAPI loopback on Windows in meeting mode) → mixer → streaming LAME MP3
into a hidden dot-prefixed `.mp3.part` in the target folder (flush ~1 s,
fsync ~30 s) → finalize. Invariants — every one exists because a reviewer
found the failure it prevents:

- **Never lose captured audio; never clobber user files.** Base names are
  reserved pairwise (`.mp3` + `.md` + `.mp3.part` all free), files are
  exclusive-created, finalization uses `rename_noreplace` (hard-link based;
  `std::fs::rename` replaces on every platform) with suffix-retry, and
  companion notes are written atomically via owned `.vault-buddy.tmp` temps.
- **Recovery touches only our own files**: dated `YYYY/MM` layout only,
  capture-pattern basenames only (`is_capture_base` lives beside
  `base_name` with round-trip tests), marker-suffixed note temps only;
  staleness-gated, postponed while a recording is active, retried while
  work is pending.
- **The buddy is the recording indicator**: all hide paths funnel through
  `tray::hide_buddy` (the single guarded chokepoint); quit/close finalize
  on worker threads — never block the event loop — and the app exits only
  after the save lands.
- **Pause is a session Control message** (`Control { Stop, Pause, Resume }`
  on the one channel the shell's device thread forwards): streams stay
  open, drained samples are discarded, nothing is encoded, the fsync
  cadence keeps running — and pause can never block shutdown
  (stop-while-paused finalizes normally). Level metering (`capture:level`,
  ~5 Hz, 0–1) is advisory and lossy by design.
- **Rename never breaks the capture contract**: `rename_plan` (core)
  keeps the `YYYY-MM-DD HHmm ` prefix and refuses non-capture files;
  execution reuses the reservation + `rename_noreplace` + suffix-retry
  loop, retargets exactly the note's embed line, and a note-side failure
  after a successful audio move degrades to a warning (audio first).
  Config writes stay app-side: owned temp + REPLACING rename is correct
  for `config.json` only, serialized behind `ConfigWriteLock`.
- **Companion note & follow-up template**: the optional `.md` embeds the audio
  and carries recording metadata; with the per-vault `follow_up_template` on
  (default), `render_note` (core) also appends a `## Follow-up` scaffold (action
  items / decisions / notes). Threaded through the capture crate, same atomic
  temp write, still never clobbering an existing note.
- Per-vault settings live app-side in `%APPDATA%\vault-buddy\config.json`
  (documented in `docs/DEVELOPMENT.md`); parsing is per-field defensive so
  one malformed value can never flip a vault's mode.

### The transcription & recordings domains (`src-tauri/transcribe/` + `core/src/{transcript,recordings}.rs` + `capture_commands.rs`)

Local speech-to-text runs *after* a recording, never live. `vault_buddy_transcribe`
owns the pipeline: MP3→16 kHz mono PCM (Symphonia) → whisper.cpp via `whisper-rs`
(behind the `whisper` feature — the real engine is Windows-only, CI-gated) → a
rendered transcript. The shell (`capture_commands.rs`) drives it through a single
worker queue — `enqueue_transcription` / `process_transcription`, one
`TranscriptionJob { mp3, vault_id, force }` at a time — so jobs never run
concurrently and the model loads once per tier. The model downloads on demand
(`ensure_model` → `download_model`, progress via `capture:modelDownload`); tier +
language come from the vault config. State is surfaced as `capture:transcribing` /
`capture:transcribed` / `capture:transcribeFailed` (each carries the `mp3`).

The transcript is the second sanctioned vault write — a `<base>.transcript.md`
sidecar the note embeds, under the same never-clobber discipline as the audio
note (`core::transcript`):

- **Never overwrite a finished or hand-edited transcript.** A
  `vault-buddy-transcript: pending/failed/complete` frontmatter marker tags our
  own regenerable output. `write_placeholder` is idempotent (skips an existing
  sidecar); `replace_if_ours` overwrites **only** a `pending`/`failed` marker (a
  `complete` transcript or any unmarked/hand-edited file is left untouched,
  `SkippedForeign`); `transcript_status` classifies Missing/Pending/Failed/
  Complete for the recordings list. The atomic temp+fsync+rename is shared with
  the audio note's writer.
- **`retranscribe` (force) vs `transcribe_recording_now` (retry).** The retry
  path respects the vault's `transcribe` gate and only regenerates a regenerable
  sidecar. `retranscribe` is the explicit per-row action: it bypasses the gate
  and uses `force_write_sidecar` (an unguarded, **sidecar-only** overwrite) for
  the final write, so it regenerates even a `complete` transcript — but the
  up-front "transcribing…" placeholder is skipped when the sidecar is already
  `Complete`, so a forced job that fails mid-flight leaves the original intact
  (the UI confirms before replacing a finished transcript).
- **Recovery backfill.** `pending_transcriptions` scans the dated `YYYY/MM`
  capture layout for capture-named MP3s whose sidecar is missing, or a `pending`
  placeholder from an attempt that didn't get to finish (e.g. a crash
  mid-download/mid-inference), and enqueues them — same layout/basename
  discipline as the recording recovery. A `failed` sidecar is deliberately
  **not** backfilled — the buddy must not keep silently re-attempting a
  completed failure on every launch; only an explicit user retry
  (`transcribe_recording_now` / `retranscribe`) regenerates it, same as
  `cancelled`.

The **recordings list** (`core::recordings`) is a read-only surface over the same
folders: `recording_roots` enumerates a vault's capture folders, `list_recordings`
scans them and reads each companion note's frontmatter (`note_field` for `type` /
title) plus `transcript_status`, returning `RecordingEntry` rows the panel groups
by type. Opening a row hands off to Obsidian via `open_recording` /
`open_transcript` (`obsidian://`, read-only, `uri::launch`-logged) — it never
writes.

### The tasks domain (`core/src/tasks.rs` + `task_commands.rs` + `Tasks.vue`)

A per-vault todo list over `type: Task` markdown documents (v0.5.0). A Task is
its own document — Obsidian-Properties/Dataview-compatible frontmatter, not an
inline checklist:

```
---
type: Task
status: new
title: "Buy milk"
created: 2026-07-08
due: 2026-07-15
priority: high
---
```

`type: Task` is the identity (so hand-authored task files count too), and the
checkbox is binary against `status: done`. `due` (`YYYY-MM-DD`) and `priority`
(`high|normal|low`) are optional widened fields (v0.5.2, the tasks-todo-list
increment): both lines are written only when present — absent `due` means no
due date and clearing it on edit **removes the line**; absent `priority` means
normal and `priority: normal` is **never written** (keeps hand-authored files
minimal and round-trip stable). Reads degrade gracefully: an unparseable `due`
(anything not plain `YYYY-MM-DD`, checked by `is_valid_due` — no calendar
validity, so `2026-02-31` is accepted like Obsidian's own date picker tolerates
it) sorts/buckets as no-date, and an unknown `priority` value sorts/renders as
normal — same defensive-read posture as the rest of the vault domain. Per-vault
config adds one field, `tasks_folder` (default `Tasks`), alongside the capture
config in the same app-side `config.json` (`tasks_root()` resolves the
default). All logic lives in the pure `core::tasks` crate (unit-tested on
Linux); the shell (`task_commands.rs`) resolves a vault + tasks root and
delegates.

- **Two sanctioned vault writes, same discipline as capture/transcript.**
  *Create* (`create_task`, now threading through optional `due`/`priority`)
  reuses the collision-safe atomic note writer (exclusive-create temp +
  `rename_noreplace`, ` (N)` suffix on collision — never clobbers). *Field
  write* is `set_fields(content, updates: &[(&str, Option<&str>)])`, the
  generalized multi-key surgical rewriter behind both the status toggle and
  the inline editor: for each `(key, value)`, `Some(v)` rewrites the existing
  `key:` line in place or inserts one at the closing fence, `None` removes the
  line, and everything else (CRLF, unknown keys, key order, body) is preserved
  byte-for-byte; it refuses a non-`type: Task` document or an unclosed
  frontmatter fence (`None`). `set_status` is now a thin one-entry wrapper over
  `set_fields` so the list/toggle agreement invariants stay on one
  implementation. On disk, `update_task_fields(root, path, updates)` is the
  shared write path (canonicalize root+path + containment + read + atomic
  `capture_note::write_atomic_replacing` — temp + fsync + REPLACING rename);
  `set_task_status` and the shell's `update_task` command both delegate to it,
  so a rename/due/priority edit and a status flip go through the exact same
  containment and atomicity guarantees.
- **`is_task` requires a CLOSED frontmatter fence** so the list and the writer
  agree on what is a task — the list must never surface a row a write would
  reject.
- **Path safety.** `safe_recording_root` (lexical) + `assert_path_inside_vault`
  (canonicalizes the nearest existing ancestor, catching a symlink/junction
  even when the leaf doesn't exist yet) gate the save/create paths;
  `assert_root_inside_vault` gates the read; `update_task_fields` (and
  `open_task`, separately) canonicalize root+path and require containment.
  `add_task` also rejects a missing vault dir (`!is_dir()`) before creating, so
  a stale registry can't resurrect a deleted vault. `set_capture_config`
  preserves `tasks_folder` (read under `ConfigWriteLock`) so saving capture
  settings can't reset it.
- **`list_tasks` walks the configured tasks folder RECURSIVELY** (v0.5.x) so
  tasks organized into subfolders are all surfaced. `collect_tasks` descends
  only after canonicalizing each child directory and confirming it stays under
  the canonical tasks root (a symlink/junction escaping the folder is skipped;
  a reparse cycle is bounded by a walked-set), and skips dot-directories
  (`.obsidian`/`.trash`/`.git`). The sort stays clock-free: open tasks
  (`status != "done"`) first, then due ascending (no/unparseable due sorts
  last), then priority tier (high < normal < low), then newest `created`, then
  title; done tasks ignore due and sort by newest `created` then title.
  "Overdue"/"Today" need a clock, so date-bucket grouping is deliberately the
  frontend's job, not the sort's.
- **`open_task(id, path)`** is a read-only Obsidian handoff for the row's
  title click, mirroring `open_recording`: canonicalize + require containment
  inside the vault's tasks root, compute the vault-relative path against the
  **canonical** vault path (a lexical relative path would fail `strip_prefix`
  against `list_tasks`' canonical paths, notably Windows' `\\?\` form), then
  `uri::launch(uri::open_file_uri(...))` — logged like every other vault open,
  never writes.
- **Frontend** (`Tasks.vue`, self-contained like `Recordings.vue` — no new
  store): a `tasks` panel view reached from a per-row Tasks button; a folder
  setting, an add-task input with an optional due/priority row, and a
  date-bucketed list (Overdue / Today / Upcoming / No date / Done — bucket
  headers render only once a dated open task exists, so a vault that never
  uses due dates keeps the flat list it always had). A task's title is a click
  target that calls `open_task`; a pencil opens an inline editor (title, due,
  priority) with one row editable at a time, Save sending only the changed
  fields (`clearDue: true` for an emptied date) in a single `update_task` call.
  Toggle/archive/edit are all optimistic (revert + toast on failure) and
  **serialized per row** (a reactive in-flight Set disables the row's controls
  until its write resolves, so two concurrent writes for one task can't land
  out of order — the editor shares this guard with toggle/archive). A title
  filter appears above 5 tasks, same threshold as the vault list.
  `TaskItem`/`TaskDto` fields (now including `due`/`priority`) match camelCase
  across Rust↔TS.

### Diagnostics invariants

- Every spawned thread is named (`std::thread::Builder`) — crash records
  must identify the dying thread.
- No swallowed error: anything caught-and-hidden goes through
  `log::warn!`/`log::error!` (Rust) or `src/logging.ts` (frontend);
  user-facing failures funnel through their domain chokepoint (e.g.
  `emit_failed`).
- Exit paths: the run loop stamps the clean-shutdown marker
  automatically; any code that terminates via `std::process::exit` must
  call `diagnostics::mark_clean_shutdown()` first or the next launch
  reports a crash.
- The panic hook + native crash handler are installed before the
  builder — nothing may be moved ahead of them.

### Frontend state

Each window loads the same bundle and mounts a different root by its label:
`main.ts` reads `getCurrentWindow().label` and `rootFor(label)`
(`src/roots/index.ts`, a pure map, unit-tested) picks the component —
`main` → `BuddyRoot`, `panel` → `PanelRoot`, `bubble` → `BubbleRoot`, any
unexpected label → `BuddyRoot`. The roots are thin: `BuddyRoot` hosts
`CompanionCharacter` and invokes `toggle_panel`/`close_panel`; `PanelRoot`
hosts `ActionPanel` and closes via `close_panel` on Escape/gutter-click;
`BubbleRoot` hosts the greeting and calls `close_bubble` on dismiss. Each
window is its own webview with its own Pinia stores, so any store that mirrors
Rust state must be wired up per window: **both** `BuddyRoot` and `PanelRoot`
call `capture.init()` (or the panel never sees `capture:*` events — dead level
meter, stuck "saving") and both install `useSettingsStorageSync` (or a tray
toggle handled in one window is invisible to, and gets reverted by, the other).

Panel visibility is no longer a store flag — it IS the panel window's
show/hide state, owned by Rust. So the `vaults` store lost `panelOpen`/
`togglePanel` and gained `refresh()`, which `PanelRoot` runs on the Rust
`panel-shown` event (each open), NOT on mount or window focus. `refresh()`
re-runs discovery and defaults the view to the vault list, unless a one-shot
`requestView(view)` asked otherwise — a failed update install `requestView`s
`settings` so the reopen lands on the error/retry UI instead of being reset to
the list. It also bumps `shownNonce`; because the panel window is only
hidden/shown (never unmounted), `ActionPanel` watches `shownNonce` to clear
transient UI a close used to reset (an open record dialog, the filter, a
lingering rename prompt). The store still holds the list and the panel view
state (`view: list | settings | captureSettings | recordings | recordMode |
transcriptions | tasks`, with `captureSettingsVaultId` / `recordingsVaultId` /
`recordModeVaultId` / `tasksVaultId`) because that must survive the panel window
being hidden. Views form a fixed one-parent-per-view tree (no history stack):
the vault-row capture button `openRecordMode`s (Meeting / Voice Note / Browse
recordings), `openRecordings` opens the read-only list, the vault-row Tasks
button `openTasks` opens the per-vault todo view, and `back()` returns to the
immediate parent (`recordings` → record view, everything else → the list) — the
header renders the cog (buddy settings) on the list and a ← back button on every
other view.

Other Pinia stores: `updates` (phase machine:
idle/checking/upToDate/available/installing/error), `settings` (buddy
character/animation, persisted to localStorage), and `capture` (recording
state mirrored from Rust: `paused`, `pausedTotalMs`, `pausedSinceMs`, `level`,
`vaultId`, `lastSaved`, plus transcription state `transcribing` /
`transcribingVaultId` driven by
`capture:transcribing`/`transcribed`/`transcribeFailed`).

Cross-window state travels two ways: Tauri events broadcast to every window
(Rust-driven animation/dragging toggles from the menu handlers; capture
level/state; `panel-shown`), and localStorage `storage` events — a settings
change in one window fires `settings.syncFromStorage()` in the others (via the
shared `useSettingsStorageSync` composable, installed by the buddy and panel
roots that read settings) so they re-read character/animation without an IPC
round-trip.

## Testing conventions

- Tests live in `tests/*.test.ts` (Vitest + happy-dom + @vue/test-utils).
  Tauri IPC is mocked with `mockIPC` from `@tauri-apps/api/mocks`; plugin
  modules are mocked with `vi.mock` + `vi.hoisted`. Tests must never require
  a real Tauri runtime.
- Rust unit tests sit next to the code in `src-tauri/core/` and
  `src-tauri/capture/`; keep new logic in the core crate whenever it doesn't
  need Tauri types, precisely so it's testable everywhere. (`capture` needs
  `libasound2-dev` on Linux for cpal — CI installs it.)
- This repo practices TDD via the vendored superpowers skills
  (`.claude/skills/`, injected by a SessionStart hook): failing test first,
  then the fix. Regression tests name the failure mode in a comment.

## Conventions

- **Commits:** Conventional Commits with scopes seen in history:
  `feat(ui)`, `fix(updates)`, `fix(shell)`, `fix(vaults)`, `style(core)`,
  `ci(release)`, `chore(release)`. Imperative subject, body explains the
  *why* and the failure mode being fixed.
- **Comments:** explain constraints the code can't show (race windows,
  platform quirks, why an ordering matters) — not what the next line does.
  Match the existing density; this codebase comments invariants heavily.
- **PRs:** every PR gets an automated Codex review (chatgpt-codex-connector
  bot) plus GitGuardian secret scanning. CI = frontend job + rust-core job,
  then the Windows build. Treat bot findings as real leads: verify against
  the code, fix what's confirmed, resolve the thread.

## Releases

Release = version bump in `package.json`, `src-tauri/tauri.conf.json`, and
`src-tauri/Cargo.toml` (+ both lockfiles) on `main` — run
`npm run bump-version -- <version|patch|minor|major>`
(`scripts/bump-version.mjs`) rather than editing the five files by hand; it
refuses to run if they've already drifted apart. The `Bump version` GitHub
Actions workflow (`workflow_dispatch`) runs the same script from `main` and
opens a PR with the result, for bumping without a local checkout. Once the
bump lands on `main`, either push a
`v*` tag **or** dispatch the Release workflow with the tag as input
(`gh workflow run release.yml -f tag=vX.Y.Z` / the Actions UI). The
dispatch path exists because remote agent sessions can push branches but
not tags (the git proxy 403s tag refs); `tauri-action` creates the tag and
the GitHub release itself either way. The workflow signs updater artifacts
(`TAURI_SIGNING_PRIVATE_KEY` secrets) and attaches `latest.json`, which
installed apps poll from Settings → Updates. CI builds without updater
artifacts when the signing secrets are absent (forked PRs) instead of
failing.
