# Document Import via Pandoc — Design

- **Date:** 2026-07-10
- **Status:** Approved (design only — no implementation yet)
- **Source:** User request to enhance the Knowledge Intake PRD with a
  docx/odt/rtf → Markdown import capability built on Pandoc, brainstormed
  end-to-end before any code is written. This spec documents the *why*
  behind the shape below; implementation is a separate, later increment.

## Goal

Let a user turn a Word/OpenDocument/RTF file into a vault note in one or
two clicks, without Vault Buddy bundling or shipping Pandoc itself.

## Why not bundle Pandoc

Vault Buddy already bundles one heavy local dependency (whisper.cpp via
`whisper-rs`, compiled directly into the transcribe crate). Pandoc has no
equivalent "just link it in" path — it's a separate GPL-2 executable, not
a Rust crate. Bundling a sidecar binary is legally fine (GPL doesn't
"infect" a separately-invoked subprocess) but costs ~150–200MB of
installer weight and an ongoing vendored-binary update burden. A
pure-Rust docx parser avoids both the size and the license question but
trades away Pandoc's mature handling of tracked changes, complex tables,
footnotes, and styles — exactly the fidelity a "convert my real-world
Word doc" feature needs.

**Decision:** require a user-installed Pandoc, detected on `PATH` (with a
manual override for non-`PATH` installs), gated behind explicit in-app
guidance. This keeps the installer light, keeps Vault Buddy MIT-clean,
and treats Pandoc the same way the app already treats other optional
capabilities — opt-in, gated, degrading gracefully when absent (compare:
transcription models download on demand, the MCP server is disabled by
default).

## Architecture

A new **Document Import** domain, following the existing pure-core /
thin-shell split used by every other domain:

- **`core::document_import`** (pure, unit-tested on Linux): filename/path
  resolution (dated `YYYY/MM` layout), frontmatter rendering, Pandoc
  argument construction per source format, and a collision-safe atomic
  writer built on the same primitives `capture_note` already provides
  (exclusive-create temp, `rename_noreplace`, ` (N)`-suffix retry). No new
  write-discipline is invented here — this domain becomes the vault
  domain's fifth sanctioned write path, riding the same never-clobber
  machinery as capture/transcript/tasks.
- **`document_commands.rs`** (shell): two **async** IPC commands —
  `detect_pandoc()` and `convert_document(vault_id, source_path)` — both
  wrapping their subprocess work in `spawn_blocking`, for the identical
  reason `search_vaults` is async rather than sync: a sync command runs on
  the main thread, and spawning an external process with unbounded
  runtime would freeze window show/hide/drag for as long as Pandoc takes.
  `convert_document` bounds the subprocess with a timeout that kills the
  child on expiry — there is no queue/cancel-button infrastructure here
  (see "Why no worker queue" below), so the timeout is the only backstop
  against a hung process.
- **No persistent worker thread or job queue.** The transcription domain
  needs one because whisper jobs run for minutes, must never run
  concurrently (one model loaded at a time), and need cancel/dedup against
  a recording in progress. None of that applies here: conversions are
  one-at-a-time (see "Non-goals" below) and typically complete in well
  under a second to a few seconds. Building queue machinery for that would
  be unused ceremony — the `search_vaults`-style "async command,
  `spawn_blocking`, no queue" shape is the correct fit.
- **Config** — mirrors the `mcp` app-global-section precedent:
  - App-global `document_import` section in `config.json`: manual Pandoc
    path override (`pandoc_path: Option<String>`), parsed per-field
    defensively and round-tripped by `serialize_config` like every other
    section.
  - Per-vault `documents_folder: String` (default `Documents`), stored
    alongside `tasks_folder` / capture folder settings.

## Trigger flows

Two entry points, both converging on the same `convert_document` command
— there is exactly one place vault-write logic for this feature lives.

1. **Drag-and-drop onto the buddy.** `BuddyRoot` listens for Tauri's
   drag-drop event. A drop is filtered to `.docx` / `.odt` / `.rtf`; an
   unsupported extension is rejected with a toast before anything else
   happens. A supported drop is **ambiguous about target vault** (the
   buddy is one fixed 88×88 icon, not a vault row), so it opens the panel
   to a new "Import into which vault?" picker view; picking a vault fires
   `convert_document(vaultId, path)`.
2. **File picker via a new "Import Document" action** in the record
   chooser (`RecordMode.vue`), alongside Meeting / Voice Note / Browse
   recordings. Opens a native file dialog filtered to the same three
   extensions. The vault is already known from context (the user opened
   this chooser from a specific vault row), so `convert_document` fires
   directly with no extra picker step.

Both paths are **feature-gated on Pandoc detection**: if `detect_pandoc`
reports not-installed, the "Import Document" action is disabled with a
hint pointing at Settings, and a buddy drop of a supported extension is
rejected with a toast pointing at the same place — never a silent no-op.

**Non-goal for this version:** batch/multi-file import. Both triggers
accept exactly one file; a queue is exactly the kind of complexity that
batch would justify, and it's explicitly deferred.

## Pandoc settings UI

A new "Document Import" section in Buddy settings, structurally a peer of
`McpSettings.vue`:

- **Status line**: Not Installed / Installed vX.Y.Z, from `detect_pandoc`.
- **Recheck button**: detection is on-demand only — runs once when the
  settings view opens and again on explicit click. No background polling
  (Pandoc's presence doesn't change while the app is running, except right
  after the user installs it, which is exactly when they'll click Recheck).
- **Install link**: opens Pandoc's install page in the OS's default
  browser. Vault Buddy does not download or run an installer itself —
  that would blur back into "bundling," with all the trust and
  installer-execution complexity that implies, for a feature explicitly
  scoped to *not* do that.
- **Manual path override**: a text field + "Browse…" native file picker,
  for installs not on `PATH` (e.g. a portable Pandoc). Stored in the new
  app-global `document_import` config section; `detect_pandoc` checks the
  override first, falling back to `PATH` lookup (`pandoc --version`).

## File organization, naming, frontmatter

- Path: `<vault>/<DocumentsFolder>/YYYY/MM/YYYY-MM-DD <Original Name>.md`
  — the same dated-folder convention Recordings uses, date-prefixed (no
  time component — unlike a recording, a document import isn't a live
  timed session, so only the date is meaningful) and named after the
  source file's stem.
- Embedded images, if the source document has any, extract to a
  same-named sibling folder next to the note (via Pandoc's
  `--extract-media`); **zero images means zero folder** — nothing is
  created that doesn't need to be.
- Frontmatter, following the `type:`-as-identity convention Tasks
  established:
  ```yaml
  ---
  type: Document
  tags: [vault-buddy-import]
  source: "C:\Users\...\Quarterly Report.docx"
  imported: 2026-07-10
  format: docx
  ---
  ```
  `source` is the original file's absolute path (provenance — where this
  came from), `imported` is the conversion date, `format` is the original
  extension. The `vault-buddy-import` tag exists for the same reason the
  transcript sidecar carries a marker: it's a machine-legible signal that
  this file is Vault Buddy's regenerable output rather than a
  hand-authored note, useful for any future "Documents" browser view.
- Collision handling: the same exclusive-create + `(N)`-suffix retry
  pattern as Tasks/Recordings — no new discipline invented.

## Conversion mechanics

`convert_document`:

1. Resolves the vault's `documents_folder`, builds the dated target path,
   and reserves the note filename (and media folder name, if the doc
   turns out to have images) the same way capture reserves `.mp3`/`.md`
   pairs.
2. Invokes Pandoc against a **temp file**, not the final vault path
   directly: `pandoc <source> -f <docx|odt|rtf> -t gfm
   --extract-media=<mediaDir> -o <tempFile>`. GFM (GitHub-Flavored
   Markdown) is the output target rather than Pandoc's native Markdown
   dialect, because Obsidian's renderer is much closer to GFM than to
   Pandoc's extension-heavy default (footnote/definition-list/etc. syntax
   Obsidian doesn't understand).
3. Vault Buddy — not Pandoc — prepends the frontmatter block and performs
   the atomic write (temp + fsync + `rename_noreplace`), so the write
   discipline stays uniform with every other domain instead of trusting
   Pandoc's own file output as the final artifact.

Format-to-reader mapping is a fixed three-entry table: `.docx` → `docx`,
`.odt` → `odt`, `.rtf` → `rtf`. No format sniffing beyond the extension —
consistent with how the rest of the app treats file extensions as
authoritative (e.g. search's `.md`-only note matching).

## Error handling

- Unsupported extension: rejected before Pandoc is ever invoked.
- Pandoc missing at call time (a TOCTOU race — uninstalled or PATH
  changed between detection and this call): command errors, toast shown,
  nothing written.
- Non-zero exit, or the timeout kills the child: toast shown, temp files
  cleaned up, **nothing lands in the vault** — mirrors `capture:failed`'s
  "no partial/garbage output" guarantee.
- Success: silent save + toast, no auto-open — mirrors how a completed
  recording finishes (the user finds the note later; nothing yanks focus
  or opens Obsidian on their behalf).

## Testing (for the implementation phase)

- `core::document_import` unit tests: naming/path resolution, frontmatter
  rendering, collision handling — all pure, no Pandoc needed, run
  anywhere.
- Recommended (not required for this design): install real Pandoc in the
  Linux `rust-core` CI job and run a genuine conversion against a small
  fixture `.docx`/`.odt`/`.rtf`, the same precedent as the `--features
  whisper` tests being the one place the whisper FFI runs for real.
- Frontend Vitest: buddy drag-drop handling, the new record-chooser
  action, the vault-picker view, and the settings section's status/
  recheck/override UI — all against mocked IPC, no real Pandoc needed.

## Roadmap placement

Deliberately **not** folded into the existing generic "File Import"
Version 3 placeholder (which stays a vague future item for non-conversion
file drops — e.g. dropping an already-Markdown or plain-text file in
as-is). This is fully specified and nearer-term, so it's tracked as its
own capability, sequenced alongside the existing "Version 2 — planned"
items (Summaries / Task Extraction / AI-enriched Meeting Notes).

## Non-goals (this version)

- Batch/multi-file import.
- Formats beyond `.docx` / `.odt` / `.rtf` (no `.pdf`, `.html`, `.epub` —
  those would need per-format quirks this spec doesn't cover).
- Bundling or auto-installing Pandoc.
- A watched "Inbox" folder or OS file-association integration — only
  buddy drag-drop and the record-chooser file picker.
- Auto-opening the converted note in Obsidian after conversion.
- Any AI pipeline step on the imported content (out of scope, same as
  the rest of Knowledge Intake's AI processing, which remains a separate
  future pipeline).
