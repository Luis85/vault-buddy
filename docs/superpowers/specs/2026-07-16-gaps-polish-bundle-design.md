# Polish bundle: config-merge test, Pandoc cache, daily-notes, import queue, orphan media — Design

- **Date:** 2026-07-16
- **Status:** Approved (design only — implementation is a separate step)
- **Source:** User request to work five catalogued items from `docs/Gaps.md`
  (GAP-60, the too-old-Pandoc cache edge, GAP-09, GAP-55, GAP-54) as a
  follow-up polish bundle after the Pandoc-cache/icon PR (#62). Each fix
  ships as its own TDD commit on the same branch, appending to PR #62.

Five independent fixes, grouped only because they're a batch. Each has a
self-contained deliverable and test.

## 1. GAP-60 · Config preserve-vs-write split is untestable

**Problem.** `set_capture_config` and `set_documents_config` each own a
subset of `VaultCaptureConfig` and must carry the rest forward from the
existing config so the *other* command's settings survive.
`set_capture_config` builds a fresh struct literal that writes its own
fields and copies `existing.tasks_folder` / `documents_folder` /
`default_list` / `list_order` / `document_date_folders` /
`document_extract_images` verbatim. A transposed assignment (e.g. the
`recording_date_folders:` / `document_date_folders:` pair swapped) compiles
and passes every existing test, then silently resets a user's layout on
their next save. The logic lives behind `#[tauri::command]` /
`tauri::State`, out of reach of the core test suite.

**Design.** Two pure helpers in `core/src/vault_config.rs`, re-exported
through `capture_config` (the existing `pub use` line), using struct-update
syntax so the preserved set is explicit:

```rust
/// set_capture_config owns mode/folders/bitrate/devices/transcription/
/// recording_date_folders; every other field is preserved from `existing`.
/// `incoming` carries the owned fields (the non-owned ones are ignored — they
/// are overwritten here), so the command can no longer transpose a preserved
/// field with an owned one.
pub fn merge_capture_owned(
    existing: &VaultCaptureConfig,
    incoming: VaultCaptureConfig,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        tasks_folder: existing.tasks_folder.clone(),
        documents_folder: existing.documents_folder.clone(),
        default_list: existing.default_list.clone(),
        list_order: existing.list_order.clone(),
        document_date_folders: existing.document_date_folders,
        document_extract_images: existing.document_extract_images,
        ..incoming
    }
}

/// set_documents_config owns exactly these three fields; the rest is preserved.
pub fn merge_documents_owned(
    existing: &VaultCaptureConfig,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        documents_folder,
        document_date_folders,
        document_extract_images,
        ..existing.clone()
    }
}
```

- `set_capture_config` (`capture_config_commands.rs`): build `incoming` from
  the DTO (owned fields + `..VaultCaptureConfig::default()` for the rest) and
  return `merge_capture_owned(&existing, incoming)`. The existing
  read-inside-the-lock discipline is unchanged; only the struct construction
  moves to the helper.
- `set_documents_config` (`document_commands.rs`): replace the in-place
  mutation with `merge_documents_owned(&existing, folder, document_date_folders,
  document_extract_images)`.
- **Tests** (`vault_config.rs`): `merge_capture_owned` writes the capture-owned
  fields from `incoming` and preserves all six non-owned fields from `existing`
  (distinctive values, e.g. `document_date_folders:false`, `document_extract_images:false`,
  a `tasks_folder`, a `list_order`); `merge_documents_owned` writes its three
  and preserves `recording_date_folders` + the capture fields. A transposition
  now fails a test.

## 2. Too-old Pandoc keeps re-probing

**Problem.** `usePandocStore.ensureDetected()` caches on `status?.installed`,
so an "installed but too old (<2.15, `sandboxSupported:false`)" result is
treated as found and never re-probed — updating Pandoc isn't reflected in the
intake menu until an explicit settings Recheck.

**Design.** Change the cache-hit guard to
`if (this.status?.installed && this.status.sandboxSupported) return;`. A usable
Pandoc still caches (no re-probe); a too-old one re-probes on the next open, so
an update is picked up automatically — consistent with the not-installed
re-probe policy. One added store test: an installed-but-`sandboxSupported:false`
status re-probes on the second `ensureDetected`.

## 3. GAP-09 · Daily-note `[literal]` escapes

**Problem.** `substitute_tokens` (`daily_notes.rs`) treats every letter run as
a token and returns `None` (→ default fallback) for anything not exactly
`YYYY`/`MM`/`DD`. A moment `[literal]` escape (e.g. `YYYY-MM-DD [Daily]`,
common in Obsidian) hits that rule and falls back, and `daily_note_uri` then
`obsidian://new`s a note diverging from the user's scheme.

**Design.** Handle `[` in `substitute_tokens`: on `[`, consume to the matching
`]` and push the inner text **verbatim** (brackets stripped), skipping the
letter-run rule inside. `YYYY-MM-DD [Daily]` → `2026-07-03 Daily`. An
**unterminated** `[` (no closing `]`) returns `None` → default fallback (the
module's "when unsure, never misname" posture). Unbracketed unknown runs
(`dddd`, `MMMM`) still fall back exactly as today. New tests: a bracket literal
renders verbatim; a format mixing tokens + a literal; an unterminated `[`
falls back; the existing fallback cases still hold.

## 4. GAP-55 · Full FIFO import queue

**Problem.** The buddy-drop import stash is single-slot
(`DocumentImportPending(Mutex<Option<String>>)` + `pendingImportPath`). A
third document dropped while an import runs overwrites the second, silently
losing it. (Non-destructive — nothing is written for a lost path — but a drop
vanishes.)

**Design.** A FIFO queue end to end:

- **Rust** (`document_commands.rs`): `DocumentImportPending(Mutex<VecDeque<String>>)`.
  `begin_document_import` **pushes** the path (still calls `show_panel`).
  `take_pending_import` **drains the whole queue** and returns `Vec<String>`
  (empty when none). Lock discipline (`lock_ignoring_poison`) unchanged.
- **Store** (`vaults.ts`): replace `pendingImportPath: string | null` with
  `pendingImports: string[]` (head = the doc currently shown). `refresh()`
  drains `take_pending_import` (now `string[]`) and, if non-empty, **appends**
  to `pendingImports` and shows the `importPicker` view (a drop mid-picker adds
  to the queue rather than replacing). `showList()` clears `pendingImports`.
  `openImportPicker` is replaced by an `enqueueImports(paths: string[])` action.
  **Empty-drain guard:** when the drain is empty **and** the picker is already
  showing a non-empty queue (`view === "importPicker" && pendingImports.length`),
  `refresh()` leaves it alone rather than falling through to `showList()`. Near-
  simultaneous drops each fire a `panel-shown` → `refresh`, but JS event
  ordering can let the first refresh drain all queued paths and later refreshes
  drain empty; without this guard a later empty refresh would wipe the queue the
  first one just built. (This also hardens the pre-existing single-slot behavior,
  where a spurious re-open could abandon an un-picked drop.)
- **Picker** (`ImportVaultPicker.vue`): the source name reads
  `pendingImports[0]`; an "N more queued" indicator shows when
  `pendingImports.length > 1`. `pick(vaultId)` converts `pendingImports[0]`,
  then `shift()`s it off; if the queue is still non-empty it stays on the
  picker (next doc), else `showList()`. This **replaces** the snapshot-compare
  GAP-55 mitigation (which only survived one extra drop) — with a real queue no
  drop is lost. The per-conversion `ImportLock` (Rust) still serializes the
  actual conversions.
- **Tests**: dropping three docs then picking a vault for each converts all
  three in order; the queued-count indicator renders with >1 pending; a single
  drop behaves exactly as before.

## 5. GAP-54 · Sweep the crash-orphan media folder

**Problem.** `publish_inner` moves the media folder to the target, then writes
the note. A crash in that ~two-rename window leaves `<target>/<basename>/`
(published media) with no `<basename>.md`, and `run_import_recovery` only
sweeps `.vault-buddy.tmp.import` staging dirs. Result: a stray media folder of
our own extracted images (no user data loss).

**Design.** Extend the janitor so that when `clean_stale_staging_at` removes a
stale staging dir, it **also** removes the matching orphan media folder — but
only when provably ours and provably orphaned. The staging dir survives the
crash window (its staged note was never published, `cleanup_staging` never
ran), so its name is the anchor:

- **Basename extraction** from the staging dir name
  `.<basename>.<pid>-<seq>.vault-buddy.tmp.import`: strip the leading `.`,
  strip the trailing `STAGING_MARKER` (`.vault-buddy.tmp.import`), then
  `rsplit_once('.')` — the unique token `<pid>-<seq>` contains no `.`, so the
  last `.` separates it from `<basename>` even when the basename itself has
  dots. A name that doesn't yield a basename is skipped (no orphan sweep for
  it).
- **Removal gate** — remove `<dir>/<basename>/` only when **all** hold:
  (a) `<dir>/<basename>.md` does **not** exist (no sibling note → orphan);
  (b) the media path `canonicalize()`s to itself (a real, in-place directory —
  rejects a symlink/junction, the same discipline the staging-dir removal
  uses);
  (c) it stays inside the canonical documents root (containment).
  Removal is `remove_dir_all` of the real in-place path, never a link target.
  This runs inside the existing staleness + containment-gated `sweep_dir`, so
  it inherits those guards; it fires only for a staging dir that was itself
  stale enough to remove.
- **Tests**: a staging dir + an orphan `<basename>/` media folder with no note
  → both removed; a staging dir + `<basename>/` **with** a sibling
  `<basename>.md` → media kept (not our orphan); a symlink named like the media
  folder → skipped, target intact; basename extraction handles a dotted stem.

**Note.** This is the lowest-value item (worst case today is a cosmetic stray
folder of our own files) and it touches delete-heavy code, so the removal gate
above is deliberately strict.

## Delivery & non-goals

- Five commits on `claude/document-intake-image-text-config-lhrmsk`, appending
  to PR #62. Each fix is independent; order in the plan is by risk (GAP-60,
  Pandoc, GAP-09, GAP-55, GAP-54).
- Each fixed gap is deleted from `docs/Gaps.md` (with a regression test naming
  the failure mode) or, for the too-old-Pandoc edge (not a numbered gap),
  simply implemented.
- **Non-goals**: reworking the import UX beyond the queue; supporting daily-note
  moment tokens beyond `YYYY`/`MM`/`DD` + `[literal]` (weekday/month-name tokens
  still fall back); a crash-atomic two-phase media publish (GAP-54 stays a
  best-effort post-hoc sweep, not prevention).
