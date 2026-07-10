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
   `convert_document(vaultId, path)`. **The dropped path is armed as a
   pending import that survives the panel-open, not raced against it.**
   The buddy and panel windows have separate Pinia stores, so the path
   can't be written into the panel store directly; and the panel is
   revealed through the same `panel-shown` → `PanelRoot.refresh()` flow
   every open uses, whose default is `showList()`. If the buddy merely
   emitted an event and toggled the panel, the toggle could *hide* an
   already-open panel, and `refresh()`'s `showList()` would clobber a
   direct `openImportPicker` call. So the drop instead calls a Rust
   command (`begin_document_import(path)`) that stashes the path in
   shell-side state and **shows** the panel idempotently (never toggles
   it hidden); `refresh()` then *consumes* that pending path (via
   `take_pending_import`) and routes to the picker in place of the list —
   exactly the pending-view idiom the failed-update-install reopen
   already uses, sourced from Rust because the trigger is cross-window.
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
  For Recheck to actually see a fresh install without the user having to
  restart Vault Buddy, `detect_pandoc` cannot rely solely on
  `std::env::var("PATH")` / a subprocess's inherited environment: on
  Windows, an installer updates the user/machine `PATH` in the registry,
  but a process already running (Vault Buddy) keeps the `PATH` snapshot it
  started with — Windows only broadcasts the change, it doesn't rewrite
  other processes' environment blocks. So `detect_pandoc` re-reads the
  current `PATH` from the two Windows environment-registry keys — the user
  key `HKCU\Environment` and the machine key (kept unbroken; note there is
  no space in `CurrentControlSet`):
  ```
  HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment
  ```
  merges those with the process's own `PATH`, and searches *that* combined
  value for `pandoc.exe`, rather than trusting the environment Vault Buddy
  was launched with.
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
  established, with every string value run through the existing
  `core::capture_note::yaml_quote` helper (already reused by
  `tasks.rs`/`transcript.rs` for exactly this reason: an unescaped
  Windows path's backslashes are invalid escape sequences inside a
  double-quoted YAML scalar — `yaml_quote` doubles `\` and `"` before
  quoting, so this is a rendering detail, not a new helper to write):
  ```yaml
  ---
  type: Document
  tags: [vault-buddy-import]
  source: "C:\\Users\\...\\Quarterly Report.docx"
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
- Collision handling: the same exclusive-create + `(N)`-suffix scheme as
  Tasks/Recordings, but the suffix is resolved **up front** (jointly for the
  note and its media folder — see Conversion mechanics step 1) rather than at
  write time, because Pandoc bakes the media folder name into image links
  before the note is written. A second import of the same image-bearing
  document on the same date takes `<name> (1)` for both the note and the
  media folder, links intact — it does not fail.

## Conversion mechanics

**Conversions are serialized process-wide.** `convert_document` is async and
its subprocess work runs under `spawn_blocking`, so Tauri is free to run two
invocations concurrently — and the UI *can* deliver two in quick succession
(a double-click on the action, or two files dropped near-simultaneously if
batch is ever relaxed). Two concurrent conversions targeting the same date
would both pass step 1's `exists`-based reservation before either publishes,
pick the *same* note/media names and the same staging dir, and then race into
the media publish + `rename_noreplace`, corrupting each other's staging or
leaving a partial media folder. The exists-check reservation is therefore
**not** sufficient on its own. A single process-wide guard — a `try_lock` on
a dedicated mutex (an `ImportLock` app state, mirroring `ConfigWriteLock`) —
wraps the whole convert-and-publish body: a second conversion that can't take
the lock fails fast with a "an import is already in progress" toast rather
than racing. (A `try_lock`, not a blocking lock, so a hung Pandoc can't make
the UI's second attempt hang too.) Each staging dir also carries a
per-invocation unique suffix so even two imports of *different* files to the
same date can't collide on the temp dir.

`convert_document`:

1. Resolves the vault's `documents_folder` and **re-validates that it stays
   inside the vault** — the same lexical + canonical containment check the
   save path uses (`capture_paths::safe_recording_root` +
   `assert_path_inside_vault`), run again here, not just when the setting is
   saved. `documents_folder` is hand-editable in `config.json`, so a value
   like `../../elsewhere` or a folder that resolves through a
   symlink/junction out of the vault must be caught before any staging dir
   is created or any file is written — otherwise Pandoc output and the
   published note would land *outside* the selected vault. Validation failure
   is a normal conversion error (toast, nothing written). Then it builds the
   dated target path and **resolves a collision-free basename BEFORE Pandoc
   runs**: it walks the ` (N)` suffix sequence (`capture_paths::candidate`,
   the same scheme capture uses) until it finds a name for which *both*
   `<basename>.md` AND the `<basename>/` media folder are free, and that one
   name is used for both the note and the media folder. This up-front joint
   reservation is not the usual "write, and let the writer suffix on
   collision" — it *can't* be, because Pandoc bakes the media folder name into
   every image link as it converts, so the final sibling-folder name has to
   be known before Pandoc is invoked. Deferring the suffix to publish time (as
   the note writer normally does) would leave the note's links pointing at
   `<basename>/…` while the folder had to become `<basename> (1)/…` — broken
   links. The media folder name is therefore fixed here — it's exactly the
   reserved basename — even though it's about to be staged under a temp
   parent, not the final path yet. **That temp
   working directory lives inside the destination vault**, not the OS
   temp dir (e.g. a hidden `.vault-buddy.tmp`-style staging dir under the
   target `YYYY/MM` folder, matching the owned-temp convention capture
   already uses). This is deliberate: the final step moves the staged
   media directory into place with a `rename`, and a `rename` across
   filesystems fails with a cross-device error — vaults routinely live on
   a different drive than the OS temp dir (a synced folder on `D:`, a
   network/removable drive). Staging on the *same volume as the vault*
   keeps the publish step a same-filesystem rename, so it's atomic and
   can't half-publish. (The temp dir is dot-prefixed and its basename is
   excluded from the recordings/tasks/search walks, so it's never
   surfaced even in the window between staging and publish.)
2. Invokes Pandoc **with its working directory set to `tempWorkDir`** and
   every output argument given as a **plain relative name**, never an
   absolute path:
   ```
   cwd = <tempWorkDir>
   pandoc <source> -f <docx|odt|rtf> -t gfm --sandbox \
     --extract-media=<reservedMediaName> \
     -o <reservedNoteName>.md \
     +RTS -M512M -RTS
   ```
   The relative `--extract-media` target is load-bearing, not cosmetic:
   Pandoc rewrites the markdown's image links to whatever string it was
   handed as the extraction target, so passing an absolute
   `<tempWorkDir>/<reservedMediaName>` would bake the staging path into
   every image link and those links would still point back into the temp
   dir after the file is published. Running with `cwd = tempWorkDir` and a
   bare relative `--extract-media=<reservedMediaName>` makes Pandoc write
   links relative to the note (`<reservedMediaName>/…`), which stay correct
   unchanged once the note and its sibling media folder are moved into the
   vault together (step 3 relocates only the shared parent). `<source>` is
   still passed as its real absolute path — only the *outputs* are
   relativized.
   - `--sandbox` is not optional: the source document is untrusted (email,
     a download, the web), and Pandoc's readers can be steered to *read
     arbitrary local files or fetch remote resources* while resolving
     linked media. `--sandbox` restricts I/O to the given input/output, so
     a malicious `.docx`/`.odt`/`.rtf` can't exfiltrate a local file into
     the vault or trigger network access during what the user was told is
     a purely local import. `detect_pandoc` records the version and
     `convert_document` gates on `--sandbox` support (Pandoc ≥ 2.15; some
     minimal builds lacking embedded data files degrade it) — an
     unsupporting Pandoc is reported in settings and the conversion is
     refused, never run unsandboxed on untrusted input.
   - `+RTS -M512M -RTS` caps Pandoc's GHC-runtime heap. A wall-clock
     timeout alone does not bound *memory*: Pandoc's own security note
     warns of pathological parser performance, and a crafted or merely
     enormous document can allocate enough to freeze the desktop or hit
     OOM well before the timeout fires. The heap cap makes Pandoc die with
     a memory error (caught as a normal non-zero exit → toast, nothing
     published) instead of taking the machine down. The timeout remains as
     the orthogonal bound on *time*.

   Both outputs are staged under the *same* temp parent, using the *exact*
   names and the *exact* sibling relationship (media folder named after
   and next to the note) they'll have in their final vault location. This
   matters for two reasons: (a) staging the media directory outside the
   final tree means a non-zero exit or timeout can never leave a partial
   media folder at the published path, since Pandoc creates
   `--extract-media`'s target directory and writes into it as it runs; (b)
   the relative-link property above only holds because that relationship
   is identical in both places, so step 3's move changes only the *shared
   parent* directory and the links Pandoc wrote stay correct with no
   rewriting. GFM (GitHub-Flavored Markdown) is the output target rather
   than Pandoc's native Markdown dialect, because Obsidian's renderer is
   much closer to GFM than to Pandoc's extension-heavy default
   (footnote/definition-list/etc. syntax Obsidian doesn't understand).
3. Only once Pandoc exits successfully: Vault Buddy prepends the
   frontmatter block to the temp markdown, then publishes both the note
   and the temp media directory (if non-empty) to their final paths — a
   same-volume `rename` for each (guaranteed same-filesystem by step 1's
   in-vault staging), the note via the same atomic
   temp+fsync+`rename_noreplace` write every other domain uses, both
   keeping the exact names reserved in step 1. Both are written at those
   *exact* reserved names — **not** re-suffixed at publish time, because the
   suffix was already resolved jointly in step 1 and Pandoc's image links are
   pinned to that media-folder name; re-suffixing the note here would break
   them. Publish order is **media directory first, then the note**, so the
   note is never visible pointing at images that haven't landed yet — but
   that ordering opens a partial-write window the publish step must close:
   the note write is non-replacing (`rename_noreplace` at the exact name), so
   it can still fail with `AlreadyExists` if the reserved name was claimed
   *after* step 1's reservation (a concurrent import — improbable under the
   process lock — a sync client, or the user hand-creating the same dated
   name while Pandoc ran). If the note commit fails for any reason after the
   media directory has already been published, **the media directory is
   rolled back** (removed from the published path) before the error returns,
   so a failed import never leaves an orphaned media folder behind. This
   residual post-reservation race fails the import cleanly (toast, nothing
   left behind) rather than re-suffixing — the ordinary "same document,
   same date, again" case never reaches it because step 1 already gave that
   import its own ` (N)` name. A failure at any point before this step leaves
   only the temp locations to clean up and **nothing at the published path**,
   matching the error-handling section's guarantee.

Format-to-reader mapping is a fixed three-entry table: `.docx` → `docx`,
`.odt` → `odt`, `.rtf` → `rtf`. No format sniffing beyond the extension —
consistent with how the rest of the app treats file extensions as
authoritative (e.g. search's `.md`-only note matching).

## Error handling

- Unsupported extension: rejected before Pandoc is ever invoked.
- Documents folder escapes the vault (hand-edited `../…` or a
  symlink/junction): rejected before any staging dir is created — toast,
  nothing written.
- A conversion is already in progress: the second call fails fast on the
  `try_lock` — toast ("an import is already in progress"), nothing written.
- Reserved name claimed between reservation and publish (the residual
  post-reservation race): the note write **fails at the exact reserved
  name** — it does NOT re-suffix, because Pandoc already baked links to the
  reserved media-folder name and the media may already be published under
  it; re-suffixing would break those links or orphan the folder. Any
  already-published media directory is rolled back — toast, nothing left at
  the published path. (The ordinary "same document, same date, again" case
  never reaches this — reservation gave it its own ` (N)` name up front.)
- Pandoc missing at call time (a TOCTOU race — uninstalled or PATH
  changed between detection and this call): command errors, toast shown,
  nothing written.
- Non-zero exit, or the timeout kills the child: toast shown, temp files
  cleaned up, **nothing lands in the vault** — mirrors `capture:failed`'s
  "no partial/garbage output" guarantee.
- Process killed / crash / power loss mid-conversion: `cleanup_staging`
  never runs, so the in-vault staging dir survives. A startup **import
  janitor** owns that garbage — see Recovery below.
- Success: silent save + toast, no auto-open — mirrors how a completed
  recording finishes (the user finds the note later; nothing yanks focus
  or opens Obsidian on their behalf).

## Recovery

The in-vault staging dir is normally removed by `cleanup_staging` on both
the success and the failure path. But a hard kill, crash, or power loss
while Pandoc is mid-write leaves it behind — partial markdown/media sitting
in a hidden `.…vault-buddy.tmp.import` dir under the vault's Documents
folder — and `cleanup_staging` never gets to run. The vault-walk scans
(recordings/tasks/search) already skip it because it's dot-prefixed, so it's
invisible in the UI, but invisible is not the same as cleaned up.

So a startup **import janitor** owns these orphans, exactly as the capture
recovery pass owns stale `.part` files: a scan over each discovered vault's
Documents folder removes any import staging dir (matched by the owned
`…vault-buddy.tmp.import` marker — never another tool's dot-dir) whose mtime
is older than a staleness threshold. It is **postponed while an import is
active** (it tries the same `ImportLock`; if it can't take the lock, a
conversion is running and its fresh staging dir must not be touched — retry
later) and **staleness-gated** (a clock jump giving a live dir a future
mtime must not make it look stale, mirroring capture recovery's exact
guard). This is the one place these owned temp dirs are *not* excluded from
recovery — they are its whole subject.

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
