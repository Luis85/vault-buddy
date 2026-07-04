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

### Updater flow (`src/stores/updates.ts`, `UpdateSettings.vue`)

Check → download (panel stays open so spinner/errors are visible) → close
panel + await transitions settle → `prepare_update_install` (Rust restores
home position and saves window state) → `install()` → `relaunch()`. On
failure the panel reopens on the settings view and `available` is kept so
the install button stays visible for retry. The `Update` object is stored
with `markRaw()` — a Vue reactive proxy breaks its private-field `rid` and
every real install would throw.

### Vault discovery (`src-tauri/core/src/discovery.rs`, `process.rs`)

Vaults come from Obsidian's own registry
(`%APPDATA%\obsidian\obsidian.json`), re-read on every panel open. The
`open` flag in that file survives a full Obsidian quit (it's how Obsidian
restores vaults on relaunch), so `list_vaults` clears all open flags when no
Obsidian process is running — otherwise the "Open now" group shows stale
entries. Malformed config always degrades to an empty list, never an error.

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
- Rust unit tests sit next to the code in `src-tauri/core/`; keep new logic
  in the core crate whenever it doesn't need Tauri types, precisely so it's
  testable everywhere.
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
