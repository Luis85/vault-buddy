# AGENTS.md

Operating guide for coding agents working in this repository. The
human-facing equivalents are [README.md](README.md) (what the product does),
[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) (contributor setup, CI, releases)
and [docs/PRD.md](docs/PRD.md) (vision, principles, roadmap). Design specs
live in [docs/superpowers/specs/](docs/superpowers/specs/).

Vault Buddy is a Windows desktop companion for Obsidian: a Tauri v2 shell
(Rust) hosting a Vue 3 + Pinia + Tailwind 4 frontend. A tiny always-on-top
transparent window shows an animated buddy; clicking it expands a panel that
lists Obsidian vaults and opens them via `obsidian://` URIs. The app never
writes into vaults.

## What compiles where (read this first)

The Rust code is deliberately split so agents can work outside Windows:

| Path | What it is | Compiles on |
| --- | --- | --- |
| `src-tauri/core/` | Pure crate: obsidian.json parsing, daily-note resolution, URI building, process detection. No GUI deps. | Anywhere — test and lint locally |
| `src-tauri/transcribe/` | Pure-ish crate: MP3→PCM decode (Symphonia), model registry/download, and whisper.cpp via `whisper-rs` behind the `whisper` feature. | Anywhere with default features (no whisper.cpp); the `whisper` feature + real engine build on **Windows** (CI gate). |
| `src-tauri/` (root crate) | Tauri shell: window, tray, IPC commands, plugins. | **Windows only** (Linux lacks webkit2gtk); CI's Windows job is the compile gate |
| `src/` + `tests/` | Vue frontend + Vitest suite (happy-dom, no Tauri runtime needed) | Anywhere |

When you change the shell crate (`src-tauri/src/*.rs`), you cannot compile it
in a Linux container. Mirror existing patterns exactly, run
`cargo fmt --check`, and let CI's `windows-app` job verify the build.

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
`set_panel_offset`, `set_window_geometry`, `show_buddy_menu` — all defined in
`src-tauri/src/commands.rs`. Tray + buddy context menu live in
`src-tauri/src/tray.rs`; menu item events are handled in `lib.rs`.

The app is single-instance (`tauri-plugin-single-instance`, registered
FIRST in the builder — keep it first): a second launch exits immediately
and the surviving instance reveals the buddy instead.

### The window geometry system (most invariant-heavy area)

The transparent window is 88×88 collapsed and grows to 440×340 when the
panel opens (`useCompanionWindow.ts`), so the invisible window never blocks
desktop clicks. Near screen edges the window is *shifted* so the panel
unfolds toward free space; the shift is tracked as an offset that must be
undone before any position is persisted. Invariants:

- Position + size change in ONE native call (`set_window_geometry`) —
  two IPC round-trips paint an intermediate geometry (buddy flashes).
- The frontend mirrors the offset to Rust (`set_panel_offset`) so exit paths
  that bypass the frontend (tray quit, Alt+F4, updater install) can restore
  the unshifted home position before `tauri-plugin-window-state` saves it.
  The plugin persists POSITION only — size is always managed dynamically.
- Panel open/close transitions are serialized in a queue;
  `panelTransitionsSettled()` exposes its tail. The updater awaits it before
  installing — never replace that with a sleep (it races; see git history).
- The 1s background loop in `lib.rs` (always-on-top re-assert) also
  checkpoints the parked position to the window-state file whenever it
  changed — but only while the offset is zero. Exit-time saves alone proved
  lossy (the updater kills the process via `std::process::exit`).

### Updater flow (`src/stores/updates.ts`, `UpdateSettings.vue`)

Check → download (panel stays open so spinner/errors are visible) → close
panel + await transitions settle → `prepare_update_install` (Rust restores
home position and saves window state) → `install()` → `relaunch()`. On
failure the panel reopens on the settings view and `available` is kept so
the install button stays visible for retry. The `Update` object is stored
with `markRaw()` — a Vue reactive proxy breaks its private-field `rid` and
every real install would throw.

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
- Per-vault settings live app-side in `%APPDATA%\vault-buddy\config.json`
  (documented in `docs/DEVELOPMENT.md`); parsing is per-field defensive so
  one malformed value can never flip a vault's mode.

### Frontend state

Pinia stores: `vaults` (list, panel open/closed, which panel view is showing
— view state lives in the store because the panel component is destroyed
while closed), `updates` (phase machine: idle/checking/upToDate/available/
installing/error), `settings` (buddy character/animation, persisted to
localStorage). Rust-driven toggles (animation, dragging) arrive as Tauri
events emitted from menu handlers.

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
`src-tauri/Cargo.toml` (+ both lockfiles) on `main`, then either push a
`v*` tag **or** dispatch the Release workflow with the tag as input
(`gh workflow run release.yml -f tag=vX.Y.Z` / the Actions UI). The
dispatch path exists because remote agent sessions can push branches but
not tags (the git proxy 403s tag refs); `tauri-action` creates the tag and
the GitHub release itself either way. The workflow signs updater artifacts
(`TAURI_SIGNING_PRIVATE_KEY` secrets) and attaches `latest.json`, which
installed apps poll from Settings → Updates. CI builds without updater
artifacts when the signing secrets are absent (forked PRs) instead of
failing.
