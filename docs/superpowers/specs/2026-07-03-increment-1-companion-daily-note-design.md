# Increment 1 Design — "Buddy opens your daily note"

- **Date:** 2026-07-03
- **Status:** Approved
- **Source:** First increment cut from [docs/PRD.md](PRD%20-%20Product%20Vision.md) (Phase 1 — Foundation)

## Goal

Ship the thinnest end-to-end vertical slice of Vault Buddy: a running
Tauri 2 + Vue 3 desktop app that puts an animated companion on the Windows
desktop which can discover the user's Obsidian vaults and open a vault or
today's daily note.

This proves the PRD's core loop — companion → intent → safe action →
Obsidian — across every layer of the stack (Vue UI, Tauri IPC, Rust native
layer, Obsidian integration) with one genuinely useful action.

## Scope

### In scope

1. **Project scaffold** — Tauri 2, Vue 3, TypeScript, Pinia, TailwindCSS,
   Vitest for frontend tests, `cargo test` for Rust tests.
2. **Companion window** — transparent, frameless, always-on-top, draggable
   across the desktop; system tray icon with Show/Hide and Quit.
3. **Character** — placeholder art with three animated states rendered via
   CSS/sprite animation (no Rive):
   - `idle` — default resting loop
   - `greeting` — plays on hover
   - `working` — plays while an action is in flight
4. **Vault discovery** — Rust reads Obsidian's own config at
   `%APPDATA%\obsidian\obsidian.json` and returns the list of vaults
   (id, display name, filesystem path).
5. **Action panel** — clicking the character grows the window and shows a
   panel beside the character listing discovered vaults; each vault offers:
   - **Open vault** — launches `obsidian://open?vault=<id>`
   - **Open today's daily note** — resolves today's note (see below) and
     launches the appropriate URI

   URIs address vaults by their Obsidian vault **ID** (the unique key in
   `obsidian.json`), not by name — two vaults can share a folder name, and
   the URI scheme accepts either.
6. **Daily note resolution** — Rust reads the vault's
   `.obsidian/daily-notes.json` (folder + moment-style date format,
   defaulting to `YYYY-MM-DD` in the vault root), computes today's note
   path, then:
   - file exists → `obsidian://open?vault=<id>&file=<path>`
   - file missing → `obsidian://new?vault=<id>&file=<path>`

   Obsidian performs all file creation; Vault Buddy never writes into a
   vault.

### Out of scope (deferred)

Chat / natural language, tasks, search, templates, workflow engine, Rive
animation runtime, global hotkeys, start-with-Windows, settings UI,
multi-monitor management, macOS/Linux support.

## Key decisions

| Decision | Choice | Why |
| --- | --- | --- |
| First slice shape | Thin vertical slice (companion + one real Obsidian action) | Proves the whole stack with something useful; validates the product idea earliest |
| Obsidian integration | `obsidian.json` + `obsidian://` URI scheme | Zero user setup, works with stock Obsidian, read-only — safest start |
| Interaction model | Click character → popup panel | Direct, simple; makes the companion itself functional, not decorative |
| Character fidelity | Simple animated states (placeholder art, CSS/sprite) | Feels alive without an art/tooling pipeline |
| Window strategy | One transparent window that resizes when the panel opens/closes | One webview, trivial IPC, panel always positioned correctly; avoids a permanent invisible click-blocking region |

## Architecture

### Rust (`src-tauri/src/obsidian/`)

| Module | Responsibility |
| --- | --- |
| `discovery.rs` | Parse `obsidian.json` into a vault list. Pure parsing separated from file I/O so it is unit-testable. |
| `daily_notes.rs` | Parse per-vault `daily-notes.json`; render today's date using a supported subset of moment tokens (`YYYY`, `MM`, `DD` — each letter run must be exactly one token); fall back to defaults on any parse failure or unsupported format. |
| `uri.rs` | Build `obsidian://` URIs (proper percent-encoding) and launch them via the OS opener. URI construction is pure and unit-testable; launching is a thin shell. |

Exposed Tauri commands: `list_vaults()`, `open_vault(id)`,
`open_daily_note(id)`.

### Vue (`src/`)

- **Store:** one Pinia store `vaults` — discovered vaults, panel open/closed,
  per-action loading and error state.
- **Components:** `CompanionCharacter.vue` (animation states + drag),
  `ActionPanel.vue` (popup container), `VaultList.vue` (vault rows with the
  two actions).
- **Composable:** `useCompanionWindow` — resizes the Tauri window when the
  panel opens/closes so the transparent window never leaves a large
  invisible area that blocks clicks on the desktop beneath it.

## Error handling

- **Obsidian not installed / `obsidian.json` missing** — panel shows a
  friendly "Obsidian not found" state with a hint; character stays idle;
  no crash.
- **Malformed `obsidian.json`** — treated as no vaults found; same friendly
  state.
- **Malformed / missing `daily-notes.json`** — that vault falls back to
  defaults (`YYYY-MM-DD` in vault root).
- **URI launch failure** — surfaced as an inline panel error.
- **Auditability** — every launched URI is written to the app log, honoring
  the PRD's transparency principle.

## Known limitations (accepted for this increment)

1. When today's note does not exist, `obsidian://new` creates it **empty**:
   the user's daily-note template is not applied (would require the
   Advanced URI plugin or Obsidian's own daily-note command). Candidate for
   increment 2.
2. Only the common moment tokens `YYYY`, `MM`, `DD` are supported in the
   daily-note date format. Every run of consecutive letters in the format
   must be exactly one supported token; any other run (`MMMM`, `dddd`,
   `YYYYMMDD`, …) means an unsupported moment format, and rendering falls
   back to `YYYY-MM-DD` — safer than pointing Obsidian at a misnamed path.
   (Folder prefixes belong in the `folder` setting, which is fully
   supported; digits and punctuation in the format string are fine.)
3. Windows only. The code compiles and unit-tests cross-platform, but
   vault discovery paths and desktop behavior target Windows.

## Testing

- **Rust unit tests** — `obsidian.json` parsing, `daily-notes.json`
  parsing, date-format token rendering, URI construction. All pure
  functions; no OS calls in tests.
- **Vitest component tests** — panel rendering from store state, vault list
  actions dispatching store calls, error/empty states.
- **Manual verification on Windows** — transparent window, always-on-top,
  drag, tray, and real Obsidian round-trip. The development environment is
  Linux, so desktop behavior is verified on the user's machine.

## Success criteria

Increment 1 is done when, on a Windows machine with Obsidian installed:

1. Launching Vault Buddy shows the animated companion on the desktop
   (transparent, always-on-top, draggable), with a tray icon.
2. Clicking the companion opens the panel listing the user's real vaults.
3. "Open vault" brings that vault up in Obsidian.
4. "Open today's daily note" opens today's note in the right vault —
   creating it via Obsidian if it didn't exist.
5. With Obsidian absent, the app degrades to the friendly empty state.
6. All Rust and Vitest tests pass.
