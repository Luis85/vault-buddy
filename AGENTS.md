# AGENTS.md

Operating guide for coding agents working in this repository: what the app
is, how the pieces fit together, where everything lives, the invariants you
must not break, and the conventions the repo runs on. Read the sections
relevant to the area you're changing before touching it — most of the prose
here documents a failure mode somebody already hit.

## Table of contents

- [What Vault Buddy is](#what-vault-buddy-is)
- [Documentation map](#documentation-map)
- [Repository map](#repository-map)
- [What compiles where (read this first)](#what-compiles-where-read-this-first)
- [Commands](#commands)
- [Architecture overview](#architecture-overview)
  - [The IPC surface](#the-ipc-surface)
  - [Events (Rust → webviews)](#events-rust--webviews)
  - [Key data flows](#key-data-flows)
  - [Where state lives on disk](#where-state-lives-on-disk)
- [The window system (most invariant-heavy area)](#the-window-system-most-invariant-heavy-area)
- [The vault domain](#the-vault-domain-core-crate--vaults-store)
- [The capture domain](#the-capture-domain-src-tauricapture--capture_commandsrs--capture-store)
- [The transcription & recordings domains](#the-transcription--recordings-domains-src-tauritranscribe--coresrctranscriptrecordingsrs--transcriptionrs)
- [The tasks domain](#the-tasks-domain-coresrctasksrs--task_commandsrs--tasksvue)
- [The search domain](#the-search-domain-coresrcsearchrs--search_commandsrs--searchvue)
- [The MCP server domain](#the-mcp-server-domain-src-taurimcp--mcp_commandsrs--mcpsettingsvue)
- [Updater flow](#updater-flow-srcstoresupdatests-updatesettingsvue)
- [Diagnostics invariants](#diagnostics-invariants)
- [Frontend state](#frontend-state)
- [Testing conventions](#testing-conventions)
- [Conventions](#conventions)
- [CI](#ci)
- [Releases](#releases)
- [Known gaps](#known-gaps)

## What Vault Buddy is

Vault Buddy is a **Windows desktop companion for Obsidian**: a Tauri v2
shell (Rust) hosting a Vue 3 + Pinia + Tailwind 4 frontend. A tiny
always-on-top transparent window shows an animated buddy character;
clicking it opens a panel that lists the user's Obsidian vaults and opens
them (or today's daily note) via `obsidian://` URIs. On top of that base
the app has grown five vertical domains:

- **Capture** — one-click meeting/voice-note recording (cpal + WASAPI
  loopback → streaming LAME MP3) saved into a vault folder with an optional
  companion note.
- **Transcription** — opt-in, fully local speech-to-text (whisper.cpp via
  `whisper-rs`) run after a recording, writing a transcript sidecar the
  note embeds; plus a read-only recordings browser.
- **Tasks** — a per-vault todo list over `type: Task` markdown documents.
- **Search** — cross-vault, read-only, on-demand filename + content search.
- **MCP server** — an opt-in, disabled-by-default local MCP endpoint so AI
  clients (Claude Code/Desktop, Cursor) act on vaults through the same
  `core::services` chokepoints the panel uses.

The product principles that shape the code: **local-first** (no accounts,
no cloud; models download once and inference is offline), **the vault is
sacred** (browsing never writes; the few sanctioned write paths are listed
per-domain below and defend themselves with never-clobber discipline), and
**human in control** (updates and transcription are user-initiated or
opt-in; every launched URI is logged as an audit trail). The long-term
vision (knowledge lifecycle, MCP hub, plugins) lives in the PRD — the code
here is deliberately only the shipped increments.

## Documentation map

| Document | What it holds |
| --- | --- |
| [README.md](README.md) | What the product does, install, usage — user-facing |
| [AGENTS.md](AGENTS.md) (this file) | Agent operating guide — keep it (not CLAUDE.md) up to date when the repo changes |
| [CLAUDE.md](CLAUDE.md) | Thin pointer at this file for Claude Code |
| [CONTEXT.md](CONTEXT.md) | The domain glossary / ubiquitous language (Vault, Buddy, Capture, Task vs Todo vs Task Tag, Runtime, Capability…). Use these terms in code, docs, and commits; keep it current via the `domain-modeling` skill |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Contributor setup, build prerequisites, CI/release pipelines, logs & crash reporting, capture config reference |
| [docs/PRD.md](docs/PRD.md) | Vision, principles, capability roadmap |
| [docs/prds/](docs/prds/) | Per-domain PRDs (knowledge intake, task management, …) |
| [docs/use-cases/](docs/use-cases/) | One file per use case, reconciled against reality on each release |
| [docs/superpowers/specs/](docs/superpowers/specs/) | Dated design specs — the *why* behind each increment's shape |
| [docs/superpowers/plans/](docs/superpowers/plans/) | Dated implementation plans that executed those specs |
| [docs/Gaps.md](docs/Gaps.md) | The audited backlog of known issues, weaknesses, tech debt, and untested paths — check it before "discovering" a known problem, extend it when you find a new one |

## Repository map

```
vault-buddy/
├── AGENTS.md / CLAUDE.md / CONTEXT.md / README.md
├── index.html                  # single HTML entry — all three windows load it
├── package.json / vite.config.ts / tsconfig.json
├── .github/workflows/          # ci.yml, release.yml, bump-version.yml
├── .claude/                    # vendored superpowers skills + SessionStart hook
├── docs/                       # see the documentation map above
├── scripts/                    # bump-version.mjs, setup-linux-deps.sh, make-icon.mjs
├── src/                        # Vue 3 frontend — ONE bundle, three window roots
│   ├── main.ts                 # mounts rootFor(window label)
│   ├── roots/                  # BuddyRoot / PanelRoot / BubbleRoot + rootFor() map
│   ├── components/             # panel views + buddy character (ActionPanel is the shell)
│   ├── stores/                 # Pinia: vaults, capture, updates, settings, notifications
│   ├── composables/            # settings sync, startup update check, bubble, announcements
│   └── utils/                  # highlight, recentSearches, formatDuration
├── src-tauri/                  # Rust workspace: root shell crate + 3 member crates
│   ├── tauri.conf.json         # the 3 windows, updater endpoint, version
│   ├── capabilities/           # single default capability (all 3 windows)
│   ├── src/                    # SHELL: lib.rs (builder/setup/metronome), commands.rs,
│   │                           #   capture_commands.rs, transcription.rs, task_commands.rs,
│   │                           #   search_commands.rs, mcp_commands.rs, tray.rs,
│   │                           #   diagnostics.rs, main.rs
│   ├── core/src/               # PURE crate: discovery, uri, daily_notes, search, tasks, services,
│   │                           #   transcript, recordings, capture_{config,note,paths},
│   │                           #   companion_placement, checkpoint, app_diagnostics,
│   │                           #   vault_walk, crash, throttle, sync_util
│   ├── capture/src/            # AUDIO engine: devices, mixer, encoder, session,
│   │                           #   recovery, rename
│   ├── mcp/src/                # MCP server: service (7 tools), http (guards+runner),
│   │                           #   token; real-socket integration tests in mcp/tests/
│   └── transcribe/src/         # STT: decode (Symphonia), model (download+verify),
│                               #   engine (whisper-rs FFI), lib (orchestration)
└── tests/                      # Vitest suite (happy-dom + mockIPC, no Tauri runtime)
```

Rule of thumb for where logic goes: **anything that doesn't need Tauri
types belongs in `core`** (testable everywhere), audio-engine mechanics in
`capture`, STT mechanics in `transcribe`, and the shell only wires domains
to windows, threads, and IPC.

## What compiles where (read this first)

The Rust code is deliberately split so agents can work outside Windows:

| Path | What it is | Compiles on |
| --- | --- | --- |
| `src-tauri/core/` | Pure crate: obsidian.json parsing, daily-note resolution, URI building, process detection, placement math, all vault-write logic. No GUI deps. | Anywhere — test and lint locally |
| `src-tauri/transcribe/` | Pure-ish crate: MP3→PCM decode (Symphonia), model registry/download, and whisper.cpp via `whisper-rs` behind the `whisper` feature. | Anywhere — CI builds *and tests* the `whisper` feature on Linux (the only place the whisper FFI regression tests run); the shipped engine builds on Windows |
| `src-tauri/capture/` | Audio engine (cpal, LAME). | Anywhere (Linux needs `libasound2-dev`); the WASAPI loopback block is Windows-only, compile-gated |
| `src-tauri/mcp/` | Tauri-free crate: the embedded MCP server — rmcp service (seven tools over `core::services`), HTTP guards, streamable-HTTP runner. | Anywhere — unit + real-socket integration tests run on Linux; CI gates it explicitly (`-p vault_buddy_mcp`) because `tauri build` alone wouldn't run its tests. |
| `src-tauri/` (root crate) | Tauri shell: windows, tray, IPC commands, plugins. | **Windows** (release + behavior gate) — **also compiles on Linux** as a compile gate once GUI deps are installed (`npm run setup:linux`, then `npx tauri build --no-bundle`); CI runs both |
| `src/` + `tests/` | Vue frontend + Vitest suite (happy-dom, no Tauri runtime needed) | Anywhere |

When you change the shell crate (`src-tauri/src/*.rs`), compile it in a
Linux container as a compile gate: run `npm run setup:linux` once (it
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

# Frontend quality gates — CI runs these in this order; check:quality must
# run with NO coverage/ dir present, so test:coverage goes last (see
# docs/DEVELOPMENT.md § Quality pipeline for the gate + ratchet policy)
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage

cd src-tauri && cargo fmt --check   # rustfmt gate (whole workspace)
cd src-tauri/core && cargo clippy --all-targets -- -D warnings
cd src-tauri/core && cargo test
# capture, transcribe, and mcp test the same way (capture needs
# libasound2-dev on Linux; mcp's roundtrip tests bind real localhost
# sockets); transcribe's whisper tests: cargo test --features whisper

# Rust quality gates (CI: machete/coverage/deny in rust-core; workspace
# clippy + shell tests in linux-app — the shell needs `npm run setup:linux`
# and a built ../dist first; see docs/DEVELOPMENT.md § Rust quality gates)
cd src-tauri && cargo machete .
cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --fail-under-lines 94
cd src-tauri && cargo deny check
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri && cargo test -p vault-buddy --lib
```

Gate mechanics in brief: ESLint severity is staged (backlogged rules sit at
`warn`, promoted to `error` at zero — never blanket-disabled); the LOC
guard (`scripts/loc-baseline.json`) and fallow quality ratchet
(`scripts/quality-baseline.json`) are shrink-only baselines — when your
change improves a metric, re-run the gate with `--update` and commit the
baseline in the same PR; coverage floors in `vite.config.ts` rise the same
way. Loosening any baseline is a reviewed decision that needs a
justification in the PR.

Gotcha: in anything automated, invoke the tauri CLI as `npx tauri <cmd>`,
never through npm script indirection — a past `tauri` script aliased
`tauri dev`, and `npm run tauri build` expanded to `tauri dev build`, which
launched the app on the CI runner and never exited.

## Architecture overview

Three OS windows, one frontend bundle, one Rust process:

```
   ┌───────────────────────────── Rust shell (src-tauri/src) ─────────────────────────────┐
   │  lib.rs: builder, plugins (single-instance FIRST), window events, 1 s metronome,     │
   │          setup (crash handler → marker → restore → tray → recovery → transcriber →   │
   │          MCP start_if_enabled)                                                       │
   │  commands.rs ── window placement/toggle, drag, vault open, updater prep, autostart   │
   │  capture_commands.rs ── recording lifecycle + device/monitor/janitor threads         │
   │  transcription.rs ── single-worker transcription queue + model download              │
   │  task_commands.rs / search_commands.rs ── thin gates over core::tasks / core::search │
   │  mcp_commands.rs ── embedded MCP server lifecycle + settings (vault_buddy_mcp crate) │
   │  tray.rs ── tray icon/menu + hide_buddy chokepoint;  diagnostics.rs ── crash/marker  │
   └──────┬───────────────────────────┬───────────────────────────┬───────────────────────┘
          │ IPC commands + events     │                           │
   ┌──────┴──────┐             ┌──────┴──────┐             ┌──────┴──────┐
   │ main (88²)  │             │ panel       │             │ bubble      │
   │ BuddyRoot   │             │ PanelRoot   │             │ BubbleRoot  │
   │ character,  │             │ ActionPanel │             │ greeting /  │
   │ drag, dots  │             │ all views   │             │ announce    │
   └─────────────┘             └─────────────┘             └─────────────┘
      each webview = own Pinia; cross-window sync = Tauri events + localStorage `storage`

   pure logic lives below the shell:
   core (vault domain, placement, writers)   capture (audio)   transcribe (STT)
```

- The **frontend never touches the filesystem or windows directly** — every
  effect goes through an IPC command; every state change Rust owns comes
  back as an event.
- **Sync commands run on the main thread** — the window-thread invariant
  pins only *window-touching* commands to sync. Everything that does
  blocking filesystem/device work and touches no window API is async on
  the blocking pool: `search_vaults`, `start_capture`, `stop_capture`
  (typed `stillSaving` on its bounded-wait expiry), `list_recordings`,
  `list_tasks`, `count_open_tasks`, `list_audio_devices`, the task WRITE
  commands `add_task`/`set_task_status`/`update_task` (fsync'd frontmatter
  writes — a slow/network vault must not freeze the event loop on a save;
  `open_task` stays sync like the sibling `open_*` URI launches), and the
  MCP settings commands. `start_capture`'s buddy-show indicator tail is
  marshalled back via `run_on_main_thread`.
- The app is **single-instance** (`tauri-plugin-single-instance`, registered
  FIRST in the builder — keep it first): a second launch exits immediately
  and the surviving instance reveals the buddy instead.

### The IPC surface

All 45 commands, registered in `src-tauri/src/lib.rs` (`generate_handler`).
Keep this table in sync when adding/removing commands.

| Defined in | Commands |
| --- | --- |
| `commands.rs` | `list_vaults`, `open_vault`, `open_daily_note`, `prepare_update_install`, `toggle_panel`, `close_panel`, `close_bubble`, `announce`, `get_buddy_facing`, `get_bubble_anchor`, `start_buddy_drag`, `show_buddy_menu`, `open_logs_folder`, `rearm_crash_detection`, `get_autostart`, `set_autostart` |
| `capture_commands.rs` | `start_capture` *(async)*, `stop_capture` *(async)*, `capture_status`, `pause_capture`, `resume_capture`, `rename_capture`, `list_recordings` *(async)*, `open_recording`, `open_transcript`, `get_capture_config`, `set_capture_config`, `list_audio_devices` *(async)* |
| `transcription.rs` | `transcribe_recording_now`, `retranscribe`, `cancel_transcription`, `transcription_queue_status` |
| `task_commands.rs` | `get_tasks_config`, `set_tasks_config`, `list_tasks` *(async)*, `add_task` *(async)*, `set_task_status` *(async)*, `count_open_tasks` *(async)*, `open_task`, `update_task` *(async)* |
| `search_commands.rs` | `search_vaults` (async — deliberate, see search), `open_search_result` |
| `mcp_commands.rs` | `get_mcp_config`, `set_mcp_config` (async), `regenerate_mcp_token` (async — both join the server thread; that wait must not sit on the main thread) |

`get_autostart`/`set_autostart` wrap launch-at-login, OS-owned state behind
`tauri-plugin-autostart`. Tray + buddy context menu live in `tray.rs`; menu
item events are handled in `lib.rs`.

### Events (Rust → webviews)

All emitted app-wide (`app.emit`); listeners noted are the windows that
actually subscribe.

| Event | Meaning | Listened to by |
| --- | --- | --- |
| `panel-shown` | Panel window just opened (the precise "opened" signal) | PanelRoot (refresh), BubbleRoot (dismiss) |
| `buddy-facing` | Buddy crossed the screen midline (deduped) | BuddyRoot |
| `bubble-anchor` | `{side, valign}` for the bubble tail | BubbleRoot |
| `bubble-message` | Text for the bubble to show | BubbleRoot |
| `buddy-toggle-animation` / `buddy-toggle-dragging` | Context-menu toggles | BuddyRoot |
| `capture:started/paused/resumed/saved/failed/warning` | Recording lifecycle | capture store (init in BuddyRoot **and** PanelRoot) |
| `capture:level` | Mic level ~5 Hz, 0–1, advisory & lossy | capture store |
| `capture:transcribing/transcribeProgress/transcribed/transcribeSkipped/transcribeFailed/transcribeCancelled` | Transcription job lifecycle (each carries the `mp3`) | capture store |
| `capture:transcribeRetargeted` | A rename moved a still-queued job `{from, to}`; the store re-keys its seeded row | capture store |
| `capture:modelDownload` / `capture:modelReady` | Whisper model download progress / ready | capture store |
| `mcp:status` | MCP server state `{state, port?, message?}` on every transition | McpSettings (panel) |
| `mcp:write` | An MCP client's successful vault write `{kind, title, vaultName}` | useBuddyAnnouncements (buddy window ONLY — exactly-once) |

### Key data flows

- **Vault open**: `VaultList.vue` → `vaults` store `runAction` →
  `invoke("open_vault", {id})` → `commands.rs::open_vault` → `core::uri`
  (by ID, never name; percent-encoded; `uri::launch` logs) → OS → Obsidian.
  Success closes the panel; failure keeps it open with the error banner.
- **Recording**: `RecordMode.vue` → capture store `start()` →
  `start_capture` (reserves names under the `CaptureState` mutex, spawns
  the named `capture-device` worker) → `capture::session` streams MP3 into
  an exclusive-created `.mp3.part` → stop finalizes via `rename_noreplace`
  + collision retry → companion note written atomically → the monitor
  thread enqueues transcription if the vault opted in.
- **Transcription**: `transcription.rs` single `transcribe-worker` thread
  pops `TranscriptionJob { mp3, vault_id, force }` one at a time →
  `ensure_model` (download + SHA-256 verify on first use) →
  `transcribe::transcribe_recording` (Symphonia decode → whisper) →
  sidecar written via `core::transcript` marker rules.
- **Search**: `Search.vue` (300 ms debounce + monotonic ticket) → async
  `search_vaults` → `spawn_blocking` → `core::search` parallel per-vault
  scoped threads → merged, capped results → `open_search_result`
  (optionally pinning the panel open for Ctrl-multi-open).
- **Task toggle**: `Tasks.vue` optimistic flip (per-row in-flight set) →
  `set_task_status` → containment gates → `core::tasks::set_status`
  surgical frontmatter edit → atomic replacing write.
- **Update**: see [Updater flow](#updater-flow-srcstoresupdatests-updatesettingsvue).

### Where state lives on disk

| State | Location |
| --- | --- |
| Vault registry (read-only input) | `%APPDATA%\obsidian\obsidian.json` |
| Per-vault capture/tasks settings + app-global `mcp` section (enabled/port/token/allowWrites) | `%APPDATA%\vault-buddy\config.json` (documented in docs/DEVELOPMENT.md; per-field defensive parse; `serialize_config` round-trips every section) |
| Whisper models | `%APPDATA%\vault-buddy\models\ggml-<tier>.bin` (pinned Hugging Face URLs + SHA-256) |
| Buddy window position | tauri-plugin-window-state file in `%APPDATA%\com.vaultbuddy.desktop` (POSITION only; panel/bubble denylisted) |
| Logs / crash records / run marker | `%LOCALAPPDATA%\com.vaultbuddy.desktop\logs` — `vault-buddy.log` (5 MB rotate), `crash.log`, `.vault-buddy.run` |
| Frontend settings | localStorage `vault-buddy.animations/.character/.dragging/.messages/.messageDuration/.checkUpdatesOnStart` |
| Recent searches | localStorage `vault-buddy:recent-searches` (cap 5) |
| Updater feed | `https://github.com/Luis85/vault-buddy/releases/latest/download/latest.json` |

## The window system (most invariant-heavy area)

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
the buddy window's transparent padding so it sits snug against the character.
`show_bubble` refuses to reveal while the buddy (`main`) is hidden and returns
whether it showed — hidden-to-tray hides ALL windows, so every announcer
(startup update check, transcription progress, the greeting's post-settle
show) is silenced at this one reveal chokepoint, and `announce` skips its
`bubble-message` emit when suppressed. While the greeting is up, the
buddy's `Moved` handler re-runs `place_beside_buddy` for the bubble
(`reposition_bubble_if_visible`, keyed on the `main` window and gated on the
bubble being visible) and re-emits the anchor, so the bubble *follows* a drag
and its tail flips live when the buddy crosses the midline or an edge — a
main-thread, lock-free `set_position` that touches no window-state cache lock,
so it cannot recreate the off-main save-vs-`Moved` deadlock. The
greeting is shown via `schedule_show_bubble`
(a short worker-thread settle, then a main-thread `show_bubble`), not
synchronously in `setup`: the window-state plugin restores the buddy's parked
position slightly after setup, and a synchronous placement would anchor the
bubble to the buddy's pre-restore default corner.

Invariants:

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
  undo, because the buddy never moves to make room. The flip side: **a sync
  command must never block** — long work belongs on a worker thread or in an
  async command.
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
  no-op. One sanctioned exception to the hide: a **Ctrl-open pin**
  (`PANEL_PIN_UNTIL`, stamped by `open_search_result` with `keep_open`)
  makes the check decline the hide for ~3 s — Obsidian grabs foreground
  focus while handling the launched `obsidian://` URI, and that grab IS the
  blur being sampled; without the pin the multi-open flow the user
  explicitly requested would collapse after the first result. The pin
  expires on its own and never shows anything, so the only-hide invariant
  stands.
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
  up. (The `GetKeyState` re-check is Windows-only; the Linux compile-gate
  build skips it.) Dragging the buddy closes the panel (`BuddyRoot` invokes
  `close_panel` on drag-start): the panel is its own window now, so it
  simply hides instead of riding the buddy along.
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

## The vault domain (core crate + `vaults` store)

Hard rule, amended by the Knowledge Intake increment: **the vault domain
never writes into a vault** — opening notes and creating daily notes is
delegated to Obsidian via `obsidian://` URIs, and every launched URI is
logged (`uri::launch`) as the audit trail. Four sanctioned write paths
exist, each documented in its own domain section below:

1. the **capture** domain — recordings and companion notes;
2. the **transcription** domain — the `<base>.transcript.md` sidecar;
3. the **tasks** domain — creating a task document (collision-safe);
4. the **tasks** domain — the surgical multi-key frontmatter field write
   (status toggle, rename, due/priority/tags edit — one generalized writer).

All four ride the same never-clobber/atomic machinery in
`core::capture_note` / `core::capture_paths` (exclusive-create temps,
`rename_noreplace`, suffix retry). Any other code touching vault contents
directly is a design change, not a patch. Design specs:
`docs/superpowers/specs/2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md`,
`docs/superpowers/specs/2026-07-04-increment-3-local-speech-to-text-design.md`,
`docs/superpowers/specs/2026-07-08-task-management-vertical-slice-design.md`,
`docs/superpowers/specs/2026-07-09-tasks-todo-list-design.md`,
`docs/superpowers/specs/2026-07-09-task-tags-design.md`,
`docs/superpowers/specs/2026-07-10-task-aggregation-design.md`.

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

## The capture domain (`src-tauri/capture/` + `capture_commands.rs` + `capture` store)

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
  One scoped exception (GAP-08): a startup-wedged reservation with no
  `.part` on disk no longer blocks hide/quit/close — nothing exists to
  strand; once a late worker reports its `.part`, the wait-forever
  posture resumes.
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
  after a successful audio move degrades to a warning (audio first). The
  rename ALSO moves `<old>.transcript.md` onto the reserved base on the
  same rails and retargets the note's `.transcript` embed alongside the
  audio embed; a transcript-side collision or failure degrades to the same
  warning, leaving the old sidecar and its old embed intact rather than
  orphaning it. The command REQUIRES canonical vault containment
  (`vault_owning_path`, GAP-01/GAP-07) before planning a rename, refuses
  outright while the recording is the ACTIVE transcription job (renaming
  underneath in-flight work would orphan the worker's terminal write), and
  retargets a still-QUEUED job to the landed name (which may carry a
  collision suffix the plan didn't predict) so a pending transcription
  follows the file instead of stranding under the old name. Config writes
  stay app-side: owned temp + REPLACING rename is correct for
  `config.json` only, serialized behind `ConfigWriteLock`.
- **Companion note & follow-up template**: the optional `.md` embeds the audio
  and carries recording metadata; with the per-vault `follow_up_template` on
  (default), `render_note` (core) also appends a `## Follow-up` scaffold (action
  items / decisions / notes). Threaded through the capture crate, same atomic
  temp write, still never clobbering an existing note.
- Per-vault settings live app-side in `%APPDATA%\vault-buddy\config.json`
  (documented in `docs/DEVELOPMENT.md`); parsing is per-field defensive so
  one malformed value can never flip a vault's mode.

## The transcription & recordings domains (`src-tauri/transcribe/` + `core/src/{transcript,recordings}.rs` + `transcription.rs`)

Local speech-to-text runs *after* a recording, never live. `vault_buddy_transcribe`
owns the pipeline: MP3→16 kHz mono PCM (Symphonia) → whisper.cpp via `whisper-rs`
(behind the `whisper` feature — the shipped engine is Windows-built; the FFI
regression tests run in the Linux `rust-core` CI job) → a rendered transcript.
The shell (`transcription.rs`) drives it through a single worker queue —
`enqueue_transcription` / `process_transcription`, one
`TranscriptionJob { mp3, vault_id, force }` at a time — so jobs never run
concurrently and the model loads once per tier. The queue dedups, supports
force/rerun and cancellation (`cancel_transcription`), and is observable via
`transcription_queue_status`; the worker yields while a recording is active.
The model downloads on demand (`ensure_model` → `download_model`, pinned
Hugging Face URL + pinned SHA-256 + size floor, progress via
`capture:modelDownload`, cancellable, `.part`-then-rename); tier + language
come from the vault config. State is surfaced as `capture:transcribing` /
`transcribeProgress` / `transcribed` / `transcribeSkipped` /
`transcribeFailed` / `transcribeCancelled` (each carries the `mp3`).
`whisper-rs` is pinned at 0.16 deliberately — `transcribe/src/engine.rs`
hand-wires abort/progress callbacks around upstream bugs; treat an upgrade
as its own tracked change (see docs/DEVELOPMENT.md).

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
  (the UI confirms before replacing a finished transcript). (Note: the panel
  currently routes all retries through `retranscribe`; see docs/Gaps.md.)
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

## The tasks domain (`core/src/tasks.rs` + `task_commands.rs` + `Tasks.vue`)

A per-vault todo list over `type: Task` markdown documents (v0.5.0). A Task is
its own document — Obsidian-Properties/Dataview-compatible frontmatter, not an
inline checklist (see CONTEXT.md for the Task / Task Tag / Todo distinction):

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

**Tags (v0.5.3, task-tags increment).** `tags: Vec<String>` (empty when none)
is the third widened field, `note_tags` (beside `note_field`) reading
Obsidian's frontmatter `tags:` property in every form it accepts — flow
`[work, home/errands]`, block `- item` list, legacy comma/space string — plus
the singular `tag:` alias, read only when `tags:` is absent (`tags:` always
wins, even present-but-empty). Read normalization is lenient, matching the
rest of the vault domain's defensive-read posture: strip an optional leading
`#`, unquote, validate against `is_valid_tag`'s charset (letters/digits/`-`/
`_`/`/`, at least one non-digit — rejects all-numeric and anything with a
space) and silently drop invalid entries, dedupe case-insensitively keeping
the first-seen casing. Writes are the opposite posture — strict and
canonical: always single-line flow style (`tags: [work, home/errands]`),
written only when non-empty, and the shell's `validated_tags` runs every tag
through the same `is_valid_tag` charset check but turns a failure into an
inline error naming the offending token instead of dropping it, so a bad tag
can never silently vanish on save. `Some([])` from the editor/patch clears —
removes the line (or block) entirely, same "absent means gone" semantics as
`due`.

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
  containment and atomicity guarantees. **Block-list consumption** is the one
  writer change tags needed: when a matched key's line has an EMPTY value
  (nothing but whitespace after the colon) and is followed by YAML list-item
  lines (`- item`, indented or not, including a mapping item's indented
  continuation lines), a rewrite or removal of that key consumes those list
  lines too — a rewrite drops the key line + items and emits the single new
  flow line, a removal drops them all; keys with an inline value are
  unaffected. This is what lets a hand-authored block-style `tags:` list
  round-trip to one canonical flow line on the next save instead of leaving
  orphaned `- item` lines behind — the round-trip stays surgical, same as
  every other field.
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
  tasks organized into subfolders are all surfaced. The recursive walk is the
  shared `core::vault_walk` helper — canonical containment (a
  symlink/junction escaping the folder is skipped), a walked-set bounding
  reparse cycles, dot-directory skips (`.obsidian`/`.trash`/`.git`) — with
  the per-file `type: Task` filter in `tasks.rs`. The sort stays clock-free:
  open tasks (`status != "done"`) first, then due ascending (no/unparseable
  due sorts last), then priority tier (high < normal < low), then newest
  `created`, then title; done tasks ignore due and sort by newest `created`
  then title. "Overdue"/"Today" need a clock, so date-bucket grouping is
  deliberately the frontend's job, not the sort's. `count_open_tasks` powers
  the vault-row badge.
- **`open_task(id, path)`** is a read-only Obsidian handoff for the row's
  title click, mirroring `open_recording`: canonicalize + require containment
  inside the vault's tasks root, compute the vault-relative path against the
  **canonical** vault path (a lexical relative path would fail `strip_prefix`
  against `list_tasks`' canonical paths, notably Windows' `\\?\` form), then
  `uri::launch(uri::open_file_uri(...))` — logged like every other vault open,
  never writes.
- **Frontend** (`Tasks.vue`, self-contained like `Recordings.vue` — no new
  store): a `tasks` panel view reached from a per-row Tasks button; an
  add-task input with an optional due/priority row (the tasks-folder setting
  lives in the per-vault Vault settings view, not here), and a
  date-bucketed list (Overdue / Today / Upcoming / No date / Done — bucket
  headers render only once a dated open task exists, so a vault that never
  uses due dates keeps the flat list it always had). A task's title is a click
  target that calls `open_task` — a successful launch closes the panel
  (best-effort `close_panel`, same as every other Obsidian handoff), a failed
  one keeps it open for the error toast. A pencil opens an inline editor
  (title, due, priority, tags) with one row editable at a time, Save sending
  only the changed fields (`clearDue: true` for an emptied date; tags follows
  the same changed-fields rule below) in a single `update_task` call.
  Toggle/archive/edit are all optimistic (revert + toast on failure) and
  **serialized per row** (a reactive in-flight Set disables the row's
  controls until its write resolves, so two concurrent writes for one task
  can't land out of order — the editor shares this guard with
  toggle/archive). A title filter appears above 5 tasks, same threshold as
  the vault list; its query applies only while the input is shown, so
  archiving below the threshold can't strand a stale, invisible filter.
  `TaskItem`/`TaskDto` fields (now including `due`/`priority`/`tags`) match
  camelCase across Rust↔TS. **Cross-vault aggregation (v0.5.4, the
  task-aggregation increment).** `Tasks.vue` takes a `vaultId: string | null`
  prop; `null` is the aggregate mode, reached via the "All tasks" bar above
  the vault list in `ActionPanel.vue` (badge = `store.taskCounts` summed) and
  the store's `openAllTasks()` action (`view = "tasks"`, `tasksVaultId =
  null`). Aggregate mode fans out `list_vaults` then a parallel per-vault
  `list_tasks`, best-effort per vault — a vault whose `list_tasks` call fails
  contributes nothing and is named in one toast, with a blocking "Couldn't
  load tasks from any vault." banner reserved for `list_vaults` failing or
  every vault failing. Zero new IPC commands: aggregation is pure frontend
  fan-out over the two commands the single-vault view already used. Both
  modes converge on one enriched shape, `AggTask = TaskItem & { vaultId,
  vaultName }`, so every row action (toggle, archive, edit, open) reads the
  row's own `task.vaultId` rather than the `vaultId` prop — the prop only
  decides how the initial fan-out happened. Aggregate-only UI stays additive
  on top of the shared list: a vault chip on each row (vault-name initial,
  full name in the tooltip), an add-row vault picker (SelectMenu, defaulting
  to the first vault) in place of the single-vault view's implicit target,
  and an "All tasks" header. The sort comparator gains a
  `vaultName.localeCompare` → `path.localeCompare` tiebreak on both arms so
  ties across vaults are stable; date buckets, tag mode, filters, and the
  per-row busy-guard serialization are unchanged and shared by both modes.
- **Tags (v0.5.3): chips, filter, inputs, and a grouping toggle.** Each row
  renders all of its tags as chips between the title and the due chip; a chip
  click activates a single component-local tag filter (no multi-tag filter
  this slice) that ANDs with the title filter (both feed `filteredTasks`,
  case-insensitive exact match — no nested-tag prefix matching) and renders
  as a dismissible chip with an always-visible ✕, independent of the >5
  title-filter threshold so it can never strand the user once activated. The
  add-options row and the inline editor each get a free-text tags input
  (comma/space separated, leading `#` optional per token, split/trimmed
  client-side into an array); the editor sends `tags` in the patch only when
  the parsed array differs from the task's current tags, and an emptied input
  sends `tags: []` (clear) — same changed-fields/optimistic-revert discipline
  as the other fields. A `Dates | Tags` segmented toggle (component-local
  state, resets to `dates` on remount) switches the SAME filtered,
  globally-sorted list from date buckets to tag sections: one alphabetical
  section per distinct tag with the task repeated under EACH of its tags,
  then "No tags" (open, untagged), then "Done" (all done tasks) — headers
  always render in tag mode. Because a task can render more than once, the
  row `:key` and the inline editor's open-row state are section-scoped
  (`` `${section}:${task.path}` ``, `editingKey`/`rowKey`), so opening the
  editor on one duplicate never expands the others, while the per-path
  `busy` guard still serializes writes for the underlying task across all its
  rendered rows.

## The search domain (`core/src/search.rs` + `search_commands.rs` + `Search.vue`)

Cross-vault, read-only, on-demand search (no index): `core::search::search_vaults`
walks every registered vault via the shared `core::vault_walk` helper
(canonical containment, cycle set, dot-dir skips, deterministic name-ordered
walk — single-sourced with the tasks scan), matching case-insensitive
substrings against note stems + note content (notes are **any-case** `.md`;
content ≤ 1 MiB UTF-8 with one whole-file-lowercase early-out — larger/binary
files match by name only) and attachment filenames. **Extensionless files
are excluded** (Obsidian doesn't index them; opening one would resolve to
the like-named note). Hard caps: 2-char minimum query (code points — the
frontend gate counts the same way), 100 hits globally (`truncated` flag →
"refine your query" footer). "Filename matches surface before content-only
matches" is a **hard guarantee**: per vault, two independently-capped class
lists; a full content list stops content *reads* but names are checked to
the vault's end. Each hit carries `is_note` and the ready-made
`obsidian://open` `file` parameter (extension dropped only for exactly-`.md`
notes, kept otherwise — a `.MD` note opens by exact path);
`open_search_result` launches it via `uri::launch` — search never writes.
`search_vaults` (command) is deliberately **async** (sync commands run on
the main thread; a content scan there would freeze window show/hide and
drags), wraps the walk in `spawn_blocking`, touches no window APIs and no
locks, and returns `Result` — an infrastructure failure rejects so the
panel keeps its previous results instead of blanking. Each call bumps a
scan-generation atomic that the core walk polls per file
(`search_vaults_with_cancel`), so superseded scans abort; per-vault scans
run in parallel on **named** scoped threads and merge in vault order
(serial-identical output). Core search types derive camelCase `Serialize`
and cross the IPC boundary directly (no DTO layer — `discovery::Vault`
precedent). The panel's `search` view (parent: the vault list) is a
self-contained `Search.vue` — 300 ms debounce, monotonic request ticket
against stale responses, vault-grouped rows with count chips and
note/attachment icons, `HighlightText` (index-based, never a RegExp from
user input), and keyboard navigation over the **visible** rows only
(collapsed groups and kind-filtered hits are skipped; arrows move a clamped
selection wired to `aria-activedescendant`, Enter opens it, Ctrl+Enter /
Ctrl+click keep the panel open for multi-open — `keep_open` travels to Rust,
which pins the panel through Obsidian's focus grab (see the focus-out check
above) — hover syncs the selection via mousemove, not mouseenter, which
would fight arrow-key scrolling).
`/` or Ctrl+F on the vault list jump into search (`ActionPanel`'s
window-keydown, gated on the list view and off text inputs). The view also
renders an aria-live match summary ("N matches in M vaults", `100+` when
truncated), per-vault collapse chevrons, All/Notes/Files filter chips
(client-side over the returned hits), and recent-search chips backed by
localStorage (`src/utils/recentSearches.ts`, capped at 5, recorded only on
successful responses).

## The MCP server domain (`src-tauri/mcp/` + `mcp_commands.rs` + `McpSettings.vue`)

An **opt-in, disabled-by-default** local MCP server embedded in the running
buddy (v0.6.0, first slice of the AI-platform PRD): MCP clients (Claude
Code/Desktop, Cursor, MCP Inspector) connect over **streamable HTTP at
`http://127.0.0.1:<port>/mcp`** (default port 22082 = 0x5642 = "VB") and get
seven tools over the same `core::services` functions the panel uses —
`list_vaults`, `list_tasks`, `list_recordings`, `open_vault`,
`open_daily_note`, `add_task`, `set_task_status`. **No new vault
capability**: MCP writes are exactly the sanctioned task writes plus the
daily-note create branch, which counts as a write here (with the grant off,
a missing daily note is a tool error — `obsidian://new` mutates the vault).
Spec: `docs/superpowers/specs/2026-07-09-local-mcp-server-design.md`.
Invariants — each exists because a review found the failure it prevents:

- **Two explicit opt-ins.** `mcp.enabled` and the `allowWrites` grant
  ("Allow vault writes" in settings) both default off; app-global `mcp`
  section in `config.json`, parsed per-field defensively (out-of-range
  ports — anything outside 1024–65535 — default to 22082 at parse time, the
  same range the settings command enforces) and **round-tripped by
  `serialize_config`** (it once emitted only `vaults`; a capture save would
  have silently deleted the section — regression-tested).
- **Guard order origin → auth → body-bound**, all before rmcp sees the
  request: absent/localhost Origin only (DNS-rebinding defense), constant
  time bearer check (an EMPTY configured token never matches), and POST —
  the only body-carrying MCP method — must present a parseable
  Content-Length ≤ 1 MiB (411/413; a chunked body can't bypass the cap).
  Bind is 127.0.0.1 only.
- **Double write gate.** Write tools enter the per-session tool router only
  when the grant is on at session construction, AND every call re-checks the
  live atomic — authoritative for sessions that straddle a flip. Any
  contract-bearing settings change (enabled/port/token/**allowWrites**)
  restarts the listener so clients re-initialize and fetch a fresh
  tools/list (no listChanged push in v1); the settings UI serializes saves
  (in-flight guard) so concurrent stop/start/persist can't interleave.
- **Audit every call, redacted.** Each tool call logs tool name, vault id,
  a STATIC outcome label (never raw service errors — they interpolate
  client-provided values), and `dur_ms` — including gate denials and failed
  lookups (audit-before-deny). The full message goes only to the client.
- **Shutdown proves the socket is gone.** `RunningServer::stop()` = cancel +
  bounded join: cancelling drops the listener (axum), ends SSE bodies
  (rmcp's `take_until`), and the per-`start()` runtime teardown kills
  stragglers — **one runtime per `start()` is the invariant** (a shared
  runtime would let a stale connection keep honoring an old token; a
  session-bound pinned-stream integration test pins this). Two supports
  make the bound real: tool handlers offload ALL synchronous work
  (registry reads, the process scan, walks, fsync'd writes, the `launch`
  call) to the blocking pool via `spawn_blocking` — run inline on the
  single-threaded runtime it would starve the drain select and stop()
  would wait on vault I/O — and teardown is `shutdown_timeout`-bounded,
  never an implicit `Runtime::drop` (which waits indefinitely for
  in-flight blocking work). A blocking task that outlives the timeout is
  LEAKED — it may fire launch/on_write late; accepted and commented. A
  slow-launch integration test pins stop() ≤ DRAIN_GRACE +
  SHUTDOWN_TIMEOUT. A bind-report timeout cancels and reaps on a named
  thread so a late-binding server can't serve as an orphan. Threads:
  `"mcp-server"`, `"mcp-server-reaper"`, blocking pool `"mcp-blocking"`.
- **Startup never fails on MCP.** `start_if_enabled` logs + surfaces
  `error` status on bind failure; an enabled config with no token
  self-heals by generating one (32 bytes, base64url, in `config.json`).
  The settings commands are async (the stop path joins the server thread —
  that wait must not sit on the main thread); config writes stay under
  `ConfigWriteLock`.
- **Frontend**: `McpSettings.vue` (Buddy-settings section, self-contained)
  owns enable/port/writes/token + status + copyable client snippets;
  successful writes emit `mcp:write`, announced by `useBuddyAnnouncements`
  in the buddy window through the existing Buddy-messages gate.

## Updater flow (`src/stores/updates.ts`, `UpdateSettings.vue`)

Check → download (panel stays open so spinner/errors are visible) →
`close_panel` (hide the panel window) → `prepare_update_install` (Rust saves
the buddy position and stamps a clean shutdown) → `install()` → `relaunch()`.
The buddy window never shifts, so there is no home position to restore. On
failure the panel reopens on the settings view via `toggle_panel`, `available`
is kept so the install button stays visible for retry, and
`rearm_crash_detection` turns the run marker back on (the prepare step latched
it off). The `Update` object is stored with `markRaw()` — a Vue reactive
proxy breaks its private-field `rid` and every real install would throw.
A quiet startup check (`useStartupUpdateCheck`, installed by PanelRoot only,
gated by the `checkUpdatesOnStart` setting, ~15 s settle for login networking)
runs `checkForUpdatesQuietly`: zero trace when current or failed (phase stays
`idle`, failures only log); on an available update the buddy announces via
bubble and `requestViewOnNextOpen("settings")` arms the next panel open to
land on the install UI without yanking an already-open panel.

## Diagnostics invariants

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

Where the artifacts land (log files, crash records, the run marker, and
what an unclean-shutdown notification means) is documented for humans in
docs/DEVELOPMENT.md § Logs & crash reporting.

## Frontend state

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
transcriptions | tasks | search`, with `captureSettingsVaultId` /
`recordingsVaultId` / `recordModeVaultId` / `tasksVaultId`) because that must
survive the panel window being hidden. Views form a fixed one-parent-per-view
tree (no history stack): the vault-row capture button `openRecordMode`s
(Meeting / Voice Note / Browse recordings), `openRecordings` opens the
read-only list, the vault-row Tasks button `openTasks` opens the per-vault
todo view, the header's magnifier `openSearch`es the cross-vault search view,
and `back()` returns to the immediate parent (`recordings` → record view,
everything else → the list) — the header renders the magnifier + cog (buddy
settings) on the list and a ← back button on every other view.

Other Pinia stores: `updates` (phase machine:
idle/checking/upToDate/available/installing/error), `settings` (buddy
character/animation/message duration, persisted to localStorage), `capture`
(recording state mirrored from Rust: `paused`, `pausedTotalMs`,
`pausedSinceMs`, `level`, `vaultId`, `lastSaved`, plus the transcription
job map and active/queued state driven by the `capture:transcribe*` events),
and `notifications` (the toast queue rendered by `NotificationHost`).

Cross-window state travels two ways: Tauri events broadcast to every window
(Rust-driven animation/dragging toggles from the menu handlers; capture
level/state; `panel-shown`), and localStorage `storage` events — a settings
change in one window fires `settings.syncFromStorage()` in the others (via the
shared `useSettingsStorageSync` composable, installed by the buddy, panel, and
bubble roots that read settings — the bubble resolves `messageDuration` at
show time) so they re-read character/animation/duration without an IPC
round-trip.

## Testing conventions

- Tests live in `tests/*.test.ts` (Vitest + happy-dom + @vue/test-utils).
  Tauri IPC is mocked with `mockIPC` from `@tauri-apps/api/mocks`; plugin
  modules are mocked with `vi.mock` + `vi.hoisted`. Tests must never require
  a real Tauri runtime.
- Rust unit tests sit next to the code in `src-tauri/core/`,
  `src-tauri/capture/`, `src-tauri/transcribe/`, and the shell
  (`src-tauri/src/transcription.rs` carries the queue's tests); keep new
  logic in the core crate whenever it doesn't need Tauri types, precisely
  so it's testable everywhere. (`capture` needs `libasound2-dev` on Linux
  for cpal — CI installs it.) The member crates' tests run in the
  `rust-core` CI job; the shell crate's own tests run in `linux-app` (they
  need the GUI libs and a built `dist/`).
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
- **Terminology:** use the CONTEXT.md ubiquitous language (a Task is a
  document; a Todo is a checklist line; a Capture is not necessarily
  audio) in code, UI copy, and docs.
- **PRs:** every PR gets an automated Codex review (chatgpt-codex-connector
  bot) plus GitGuardian secret scanning. CI = the four jobs below. Treat
  bot findings as real leads: verify against the code, fix what's
  confirmed, resolve the thread.

## CI

`.github/workflows/ci.yml` runs on every push to `main` and every PR:

| Job | Runner | Gates |
| --- | --- | --- |
| `frontend` | Linux | ESLint, LOC guard (frontend + Rust files), fallow quality ratchet, version-file agreement, `vue-tsc` typecheck + build, Vitest suite with coverage floors |
| `rust-core` | Linux | `cargo fmt --check` (whole workspace), clippy `-D warnings` + tests on `core`, `capture`, `transcribe`, `mcp` — including `--features whisper` (the only place the whisper FFI tests execute) — plus `cargo machete` (unused deps), a `cargo llvm-cov` line-coverage floor (94) over the member crates, and `cargo deny check` (RustSec advisories + license policy, `src-tauri/deny.toml`) |
| `linux-app` | Linux (after the two above) | `npx tauri build --no-bundle` — shell compile gate, never released — then **workspace clippy incl. the shell** and the **shell crate's unit tests** (`cargo test -p vault-buddy --lib`; both need the GUI libs + built `dist/` this job has) |
| `windows-app` | Windows (after the two above) | Full `npx tauri build`, MSI/NSIS installers as artifacts; leaves updater artifacts unsigned on every PR event by design (the signing secrets are injected only on push to `main`, never on PRs — GAP-36); + `cargo test` for core/capture/transcribe (incl. `--features whisper`) after the build to exercise platform-sensitive code (process detection, GetKeyState, WASAPI gates, MoveFileExW fallback) |

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
installed apps poll from Settings → Updates. CI builds unsigned artifacts
on every PR event by design — the signing secrets are populated only on push
to `main` and by this release workflow, never on PRs (GAP-36) — instead of
failing. A `validate` job gates the build: it requires the dispatch path to
come from `main`, checks the tag matches `tauri.conf.json`'s version,
requires the released commit to be an ancestor of `main` (compare API —
covers the tag-push path too), and requires a successful completed CI run
for the released commit before `windows-installer` starts.

## Known gaps

The audited backlog — correctness bugs, invariant weaknesses, security and
CI gaps, untested paths, and tech debt, each with file references and
failure scenarios — lives in [docs/Gaps.md](docs/Gaps.md). Consult it
before starting work in an area (your "new" bug may be catalogued, and its
entry names the constraint a fix must respect), and update it when you fix
an entry or find a new gap.
