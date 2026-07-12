# Recording Folders, Flat/Dated Layout & Vault Settings Regroup — Design

**Date:** 2026-07-12
**Status:** Approved
**Branch:** `claude/vault-recording-folders-3eie6r`

## Problem

Three related shortcomings in the per-vault recording/capture settings:

1. **The recording folder is unified when it should be per-mode.** The config
   carries a single `recording_folder: Option<String>`. Distinct defaults
   exist (`Meetings` / `Voice Notes`), but the moment a user sets any custom
   folder, **both** Meeting and Voice Note recordings are forced into that one
   folder. Meeting and Voice Note need independently configurable folders.
2. **The dated `YYYY/MM` layout is mandatory.** Every recording, transcript,
   and imported document lands under `<folder>/YYYY/MM/`. Some users want a
   **flat** folder (all notes directly in the folder) instead of the
   year/month hierarchy. This must be a toggle, per domain.
3. **The Vault settings screen is a flat pile of sections** mixing three
   domains (capture, tasks, documents) under one Save button. It should read
   as grouped domains.

## Shape

**One design, a four-phase plan**, each phase independently green (all gates
pass at every phase boundary) and independently shippable/reviewable. Two of
the phases are structural refactors the repo's shrink-only LOC caps *force* —
both target files are grandfathered **at** their caps and already flagged
"split when next touched" (GAP-45/47), so the splits are sanctioned, not
incidental. Order puts the enabling refactors first so later phases have
headroom and never churn freshly-reviewed code.

| Phase | What | Risk |
| --- | --- | --- |
| **1** | Structural splits: `vault_config.rs` (core) + a capture-config shell command module. Pure refactor, **zero behavior change**. | Low (mechanical) |
| **2** | Regroup Vault settings into Recording / Tasks / Documents super-groups; extract `RecordingSettings.vue`. UI only. | Low |
| **3** | Distinct Meeting / Voice Note folders + legacy migration. | Medium |
| **4** | Per-domain flat/dated layout toggle: write branches + layout-agnostic scanners/recovery. | Medium (invariant-heavy) |

**Bookkeeping (every phase):**

- TDD: failing test first, named for the behavior/failure mode. Windows-only
  behavior that can't run on Linux gets its logic in a pure, testable function
  where feasible.
- All gates green at each task boundary: Vitest + coverage floors, `vue-tsc`,
  ESLint, LOC guard, fallow ratchet, `cargo fmt`/`clippy -D warnings`/`test`
  (core, capture, transcribe, mcp), and the Linux shell compile gate for shell
  changes.
- When a change *improves* a ratcheted metric (LOC, fallow, coverage), re-run
  the gate with `--update` and commit the baseline in the same commit.
- Docs that describe the touched invariants (`AGENTS.md`, `docs/DEVELOPMENT.md`
  config reference, `CONTEXT.md`) are updated in the same phase as the code.

## Domain vocabulary (CONTEXT.md additions)

- **Dated layout** — Vault Buddy's default on-disk layout: captures and
  imports live under `<folder>/YYYY/MM/`. The timestamped **base name** still
  encodes the full date, so the folders are organizational, not identifying.
- **Flat layout** — the opt-in alternative: files live directly in `<folder>`,
  no year/month subfolders. A per-domain, per-vault choice; it changes only
  where **new** files are written, never where existing ones are found.

## Config schema (final state)

Per-vault entry in `%APPDATA%\vault-buddy\config.json`. New/changed keys in
**bold**; every field stays per-field defensively parsed and omitted-when-
default (the hand-editable file stays minimal).

```jsonc
{
  "mode": "meeting",
  "meetingFolder": "Meetings",       // NEW  optional; omit → "Meetings"
  "voiceNoteFolder": "Voice Notes",  // NEW  optional; omit → "Voice Notes"
  "recordingDateFolders": true,      // NEW  optional; omit → true; written only when false
  "documentDateFolders": true,       // NEW  optional; omit → true; written only when false
  "bitrateKbps": 128,
  "createNote": true,
  "followUpTemplate": true,
  "inputDevice": null,
  "outputDevice": null,
  "transcribe": false,
  "transcriptionModel": "small",
  "transcriptionLanguage": null,
  "transcriptTimestamps": true,
  "tasksFolder": "Tasks",
  "documentsFolder": "Documents",
  "defaultList": null,
  "listOrder": []
  // REMOVED: "recordingFolder" — retired; see migration below
}
```

**Legacy migration (no data loss).** `recordingFolder` is read as a fallback:
`meeting_folder = meetingFolder ?? recordingFolder`, `voice_note_folder =
voiceNoteFolder ?? recordingFolder`. A user who set the old unified folder
keeps both modes pointing at it until they change one; their old recordings
stay findable. `serialize_config` writes only the two new keys, so
`recordingFolder` disappears on the next save. No forced rewrite — a passive
read-time fallback, equivalent for the user and simpler than a migration pass.

**Toggle serialization.** `recordingDateFolders`/`documentDateFolders` default
`true`; parse absent → `true`; serialize omits them when `true`, writes them
only when `false`. (Deliberately unlike `createNote`/`followUpTemplate`, which
are always written — a brand-new default-on field should not appear in every
existing user's file on first save.)

---

## Phase 1 — Structural splits (LOC headroom, zero behavior change)

Both files are at their shrink-only caps; the later phases' additions would
fail CI without this. Pure moves + re-exports — no behavior change, existing
tests move with the code and keep passing.

### 1a. `core/src/vault_config.rs` (new)

Move out of `capture_config.rs`, mirroring the existing `mcp_config.rs` /
`document_import_config.rs` split-outs:

- `RecordingMode` enum + its impls.
- `VaultCaptureConfig` struct + its `impl` (`effective_recording_folder`,
  `recording_roots`, `tasks_root`, `documents_root`).
- `vault_entry(&Value) -> VaultCaptureConfig` (the per-vault parser).
- The per-vault serialize helper (extract the vault-entry serialization from
  `serialize_config`'s loop into `serialize_vault_entry(&VaultCaptureConfig)
  -> Map`).
- All vault-config-specific unit tests.

`capture_config.rs` keeps `AppConfig`, top-level `parse_config` /
`serialize_config` (now calling `vault_config::vault_entry` /
`serialize_vault_entry`), file IO (`load_*`/`write_config`/`update_*`),
`app_config_dir`/`config_path`, and the IO/mcp/documents tests. It
**re-exports** `pub use crate::vault_config::{RecordingMode,
VaultCaptureConfig}` so every existing `capture_config::RecordingMode` /
`capture_config::VaultCaptureConfig` caller compiles unchanged.

### 1b. Capture-config shell command module (new)

Extract from `capture_commands.rs` (at 1219/1219) into a new sibling shell
module (e.g. `capture_config_commands.rs`): `CaptureConfigDto`, its
`from_config`, and the `get_capture_config` / `set_capture_config` commands
(with `BITRATES_KBPS` / `TRANSCRIPTION_MODELS` if only used there, else leave
shared). Register the two commands from the new module in `lib.rs`'s
`generate_handler!` (the IPC surface is unchanged; only the defining module
moves — update the AGENTS.md IPC table's "Defined in" column accordingly).
This offsets Phase 3/4's DTO + write-branch growth on that hotspot.

**Gate:** after Phase 1, regenerate `scripts/loc-baseline.json` with
`--update` — `capture_config.rs` and `capture_commands.rs` shrink (ratchet
their caps down); the two new files are below the base caps.

---

## Phase 2 — Regroup Vault settings (UI only)

`CaptureSettings.vue` becomes a thin **composer** that owns load/save
orchestration (the single Save button, the `set_capture_config` invoke, the
independent tasks/documents folder saves, save state). It renders three
domain super-groups; the Recording group's markup moves into a new controlled
component.

### `RecordingSettings.vue` (new)

A controlled component following the existing `TranscriptionSettings.vue`
pattern: a single `v-model` bundle of the recording-domain capture-config
fields (folders, bitrate, createNote, followUpTemplate, inputDevice,
outputDevice, transcription) + a `devices` prop + a `folderError` prop.
Emits merged updates back; the composer reads the bundle in `save()` to build
the `set_capture_config` payload. This drops `CaptureSettings.vue` from
474 nonblank back under the 500 cap and gives Phase 3/4 room to add the second
folder input + the toggle.

### Layout

Three `<section>` super-groups, one Save button, unchanged save semantics:

- **Recording** → `RecordingSettings.vue` (Folders · Audio · Companion note ·
  Transcription).
- **Tasks** → the existing `VaultFolderSetting` (Tasks folder) +
  `TaskListSettings` (unchanged), under a Tasks header.
- **Documents** → the existing `VaultFolderSetting` (Documents folder), under a
  Documents header.

No behavior change to what saves or how; this is regrouping + extraction. The
Phase 3 folder inputs and Phase 4 toggles slot into the Recording and
Documents groups.

---

## Phase 3 — Distinct Meeting / Voice Note folders

### Model (`vault_config.rs`)

Replace `recording_folder: Option<String>` with:

```rust
pub meeting_folder: Option<String>,     // None → "Meetings"
pub voice_note_folder: Option<String>,  // None → "Voice Notes"

pub fn folder_for(&self, mode: RecordingMode) -> &str {
    match mode {
        RecordingMode::Meeting => self.meeting_folder.as_deref().unwrap_or("Meetings"),
        RecordingMode::VoiceNote => self.voice_note_folder.as_deref().unwrap_or("Voice Notes"),
    }
}
pub fn effective_recording_folder(&self) -> &str { self.folder_for(self.mode) }
pub fn recording_roots(&self) -> Vec<&str> {
    // Deduped union of both modes' effective folders — scans exactly the
    // folders recordings can live in (e.g. ["Audio", "Voice Notes"] when
    // only Meeting is customized).
    let m = self.folder_for(RecordingMode::Meeting);
    let v = self.folder_for(RecordingMode::VoiceNote);
    if m == v { vec![m] } else { vec![m, v] }
}
```

The four consumers of `effective_recording_folder()` /`recording_roots()`
(`start_capture`, `services::list_recordings`, `transcription.rs` backfill,
capture recovery) call the **same method names** and compile unchanged.
`recording_roots()` becomes strictly more correct.

Parse (`vault_entry`): `meetingFolder ?? recordingFolder`, `voiceNoteFolder ??
recordingFolder` (the migration). Serialize (`serialize_vault_entry`): each
folder key written only when `Some`; never write `recordingFolder`.

### IPC + frontend

- `CaptureConfigDto` (now in the Phase 1b module): `recording_folder` →
  `meeting_folder` + `voice_note_folder`. Update `from_config` and
  `set_capture_config` (validate **both** folders via
  `capture_paths::safe_recording_root` before writing; either failure is the
  field-level folder error). `set_capture_config` continues to preserve the
  fields it doesn't own.
- `types.ts` `CaptureConfig`: `recordingFolder` → `meetingFolder` +
  `voiceNoteFolder`.
- `RecordingSettings.vue`: two folder inputs (placeholders `Meetings` /
  `Voice Notes`), one shared folder-error line beneath them. `RecordMode.vue`:
  update its default-seed config object's field names (it round-trips the
  config; no new UI there).

---

## Phase 4 — Flat vs. dated layout toggle (per-domain)

### Model

Two per-vault booleans on `VaultCaptureConfig`, default `true` (dated):
`recording_date_folders`, `document_date_folders`. Parse absent → `true`;
serialize only when `false` (see schema above).

### Write paths branch on the toggle

A small pure helper keeps the branch in one place, e.g. in `capture_paths`:

```rust
pub fn capture_dir(root: &Path, date: NaiveDate, dated: bool) -> PathBuf {
    if dated { dated_folder(root, date) } else { root.to_path_buf() }
}
```

- **Recordings** — `capture_commands.rs` (the `dated_folder(&root, date)` call
  site) uses `capture_dir(&root, date, cfg.recording_date_folders)`. The
  companion note + transcript already reserve/write in the same `dir`, so they
  follow automatically.
- **Documents** — `document_commands.rs` (the `<documents_root>/YYYY/MM` target
  computation) branches identically on `document_date_folders`. The staging
  dir and `publish` target sit in the same chosen dir.

### Reads + recovery become **layout-agnostic**

This is the crux and the reason no migration is needed: the toggle changes only
where *new* files land; **all** read/recovery paths find files in **either**
layout, so existing recordings/documents are always surfaced and old + new
coexist.

- `recordings::list_recordings` and `transcript::pending_transcriptions`
  (near-identical `<root>/YYYY/MM` walkers): also scan capture-named `.mp3`s
  **directly at the root level** (flat), in addition to descending the
  `YYYY`/`MM` digit-dirs (dated). Factor the shared walk into one helper that
  yields capture-named mp3s from both levels (removes the current duplication
  and keeps both scanners in lockstep).
- **Capture recovery** (`capture/src/recovery.rs`): sweep orphan `.mp3.part` /
  owned-temp files at the flat root **and** the `YYYY/MM` levels. Ownership
  gates are unchanged — `is_capture_base`, `base_from_part`, owned-temp markers
  — so scanning the flat level only ever touches our own files.
- **Import recovery** (`document_import::clean_stale_staging_at`): sweep owned
  `*.vault-buddy.tmp.import` staging dirs at the flat root **and** `YYYY/MM`.
  The existing "delete only a REAL, owned, in-place staging dir, never a link's
  target" canonical + name gate is unchanged and still applies at both levels.

Pushing the both-layout logic **into the core scanner functions** means the
at-cap shell callers (`transcription.rs`, the `capture_commands.rs` recovery
loop) keep calling the same functions and don't have to change.

### No bulk move of existing files

Switching a toggle **never moves or rewrites** existing files — that would be a
mass vault mutation, against "the vault is sacred," and the layout-agnostic
scanners make it unnecessary. Explicitly out of scope (see below).

### Frontend

- **Recording** super-group (in `RecordingSettings.vue`): a checkbox
  "Organize into year/month folders" bound to `recordingDateFolders`, riding
  the capture-config save.
- **Documents** super-group (in `CaptureSettings.vue`): a matching checkbox
  bound to `documentDateFolders`, riding the independent
  `set_documents_config` save alongside the Documents folder. Extend the
  `DocumentsConfig` DTO/type with `documentDateFolders`, and extend
  `set_capture_config`'s preserve-list so a capture save can't reset it (the
  same clobber-prevention discipline that already guards `documents_folder`).

---

## Testing

- **Rust (core):** `folder_for` per mode + defaults; `recording_roots` dedup
  union (both custom, one custom, none); legacy `recordingFolder` seeds both
  fields and round-trips away on re-serialize; toggle parse/serialize
  (absent→true, omit-when-true, write-when-false); `capture_dir` flat vs
  dated; both scanners find flat **and** dated recordings and ignore foreign
  files at both levels; `pending_transcriptions` both-layout; recovery /
  staging sweep both-layout with ownership gates intact.
- **Rust (shell):** `set_capture_config` validates both folders and preserves
  `document_date_folders`; the Phase 1b module's command move keeps the IPC
  contract.
- **Frontend (Vitest):** regrouped `CaptureSettings` renders three groups and
  saves unchanged; `RecordingSettings` two folder inputs + recording toggle
  round-trip; Documents toggle round-trips; `record-mode` uses the new field
  names. Update `tests/capture-settings.test.ts`, `tests/record-mode.test.ts`.

## Docs

`AGENTS.md` (capture domain: two folders, `recording_roots` semantics,
layout-agnostic scanners + the flat/dated toggle as a sanctioned behavior;
document-import domain: the toggle; the IPC table's "Defined in" for the moved
commands), `docs/DEVELOPMENT.md` config reference (new keys + legacy fallback),
`CONTEXT.md` (dated/flat layout terms), `docs/Gaps.md` if any edge is
discovered, and `scripts/loc-baseline.json` regenerated as files shrink.

## Out of scope (YAGNI / safety)

- **Bulk-moving existing files** when a toggle flips (mass vault mutation;
  layout-agnostic scanning makes it unneeded).
- **Per-mode layout** (a separate dated/flat choice for Meeting vs Voice Note)
  — one recording-domain toggle covers both modes.
- **Per-mode bitrate/device/transcription** — unchanged; still one set per
  vault.

## Invariants to respect

- The vault is sacred: the only writes remain the sanctioned capture /
  transcript / tasks / document-import paths; never-clobber (`rename_noreplace`
  + suffix retry, exclusive-create temps) is untouched.
- Recovery/scan ownership gates (`is_capture_base`, owned-temp markers,
  canonical containment, no symlink follow) apply identically at the flat root
  level — the flat scan must never touch a non-capture file.
- Sync commands stay non-blocking; the async capture/config commands stay
  async; window/thread invariants are not in scope and must not be disturbed.
- LOC/quality/coverage baselines only ratchet **down** (or with a justified
  bump); every phase leaves all gates green.
