# Recordings List Design — "Browse past recordings, grouped by type"

- **Date:** 2026-07-05
- **Status:** Approved
- **Source:** Follow-up to the capture/transcription increments. Once a vault
  has several recordings they're scattered across dated `YYYY/MM` folders and
  (in meeting/voice-note mode) two different roots. There's no in-app way to see
  what you've recorded. Add a read-only Recordings list, reachable from the
  Start Recording modal, that lists a vault's recordings and optionally groups
  them by type.

## Goal

A new **Recordings** panel view that lists one vault's past recordings as
`title — date — duration` rows, optionally grouped by the recording's **type**
(read from its companion note's frontmatter). Opening a row opens that
recording's note in Obsidian. Strictly read-only — it never writes into a vault.

## Entry point & shape (the two decisions)

- **Reached from the Start Recording modal, as a third option.** `RecordModeDialog`
  gains a "Browse recordings" action below the two mode buttons. It does **not**
  start a recording — it navigates to the Recordings view for the same vault and
  dismisses the dialog. No new vault-row icon.
- **It is a panel VIEW, not a fullscreen modal.** This mirrors the existing
  `captureSettings` view exactly (store-held `view` + a scoping vault id, an
  `v-else-if` slot in `ActionPanel`, the shared header toggle acting as "back to
  vaults"). The 440×340 window wants a full-height scrollable list, which the
  view slots already provide; a modal overlay would be cramped and fight the
  scroll. The dialog stays a focused confirm-or-switch chooser.

## Scope: one vault

The list shows recordings for **the vault whose Start Recording modal opened it**.
Per-vault scope keeps the query cheap, gives the header a concrete title
("Recordings — <vault>"), and sidesteps cross-vault aggregation entirely.

## What counts as a "recording" and where we look

A recording is a capture-named `<base>.mp3` under a recording root's dated
`YYYY/MM` layout — the same definition `transcript::pending_transcriptions`
already scans. The roots to scan for a vault:

```rust
// VaultCaptureConfig
/// Folders that may hold this vault's recordings. A configured custom folder
/// holds every recording regardless of mode; without one, meetings and voice
/// notes live in their two distinct default homes, so scan both.
pub fn recording_roots(&self) -> Vec<&str> {
    match &self.recording_folder {
        Some(folder) => vec![folder.as_str()],
        None => vec!["Meetings", "Voice Notes"],
    }
}
```

This is the union of `effective_recording_folder`'s branches: mode can change
over a vault's life, so past recordings can sit in either default home.

## Grouping by type (derived from the note, not the folder)

Grouping is by the **recording's type**, read from the companion note
`<base>.md`'s YAML frontmatter `type:` field (what `render_note` writes:
`type: "Meeting"` / `type: "Voice Note"`). Rules:

- **Optional**, toggled **per-view**, **not persisted** — a component-local
  `ref`, default grouped. Flat mode is one flat newest-first list.
- **Derived from the note, never the folder.** A recording under `Meetings/`
  whose note says `type: "Voice Note"` groups under Voice Note.
- **Un-derivable → Ungrouped.** No companion note, no `type:` field, or an
  unparseable value → the recording still appears, under an **"Ungrouped"**
  section (sorted last).

## Row content: title — date — duration

Every field comes from data we already have on disk — no new writes.

| Field | Source | Fallback |
| --- | --- | --- |
| **title** | capture base name with the `YYYY-MM-DD HHmm ` prefix stripped (the user's rename, or the default "Meeting"/"Voice Note" label) | the whole base name if the strip leaves nothing |
| **date** | the base name's `YYYY-MM-DD HHmm` prefix → `YYYY-MM-DD HH:MM` | — (always present; base is validated by `is_capture_base`) |
| **duration** | the companion note's `duration:` frontmatter | `—` when there's no note / no field |

The base name is the reliable source (guaranteed to parse — it's a capture
base); the note is best-effort for `type` + `duration`.

## Architecture

### Core (`vault_buddy_core`) — pure, Linux-tested

- **`capture_config.rs`: `VaultCaptureConfig::recording_roots()`** (above).
- **`capture_note.rs`: a tiny frontmatter reader.** None exists today —
  `render_note` only writes. Add a pure helper that extracts a single top-level
  scalar from a note's frontmatter block, unquoting a `yaml_quote`-style value:

  ```rust
  /// Read one top-level `key:` scalar from a note's leading `---` frontmatter
  /// block, undoing `yaml_quote`'s escaping. Returns None if there's no
  /// frontmatter or the key is absent. Deliberately minimal — we only read
  /// back the handful of fields we ourselves wrote.
  pub fn note_field(content: &str, key: &str) -> Option<String>;
  ```

  Round-trips with `render_note` (write `type: "Voice Note"` → read
  `Some("Voice Note")`); tolerates missing frontmatter, missing key, and
  quoted/unquoted values.
- **`recordings.rs` (new module): `list_recordings(roots: &[PathBuf]) -> Vec<RecordingEntry>`.**
  Mirrors `pending_transcriptions`' scan (walk each root's `YYYY/MM`, `is_capture_base`
  mp3s only, `dir_entries` so symlinks are never followed). For each mp3, read the
  sibling `<base>.md` (if present) via `note_field` for `type` + `duration`. Returns:

  ```rust
  pub struct RecordingEntry {
      pub mp3_path: PathBuf,          // absolute — the open command's input
      pub title: String,             // base minus the timestamp prefix
      pub recorded_at: String,       // "YYYY-MM-DD HH:MM" from the base name
      pub duration: Option<String>,  // note frontmatter, else None
      pub recording_type: Option<String>, // note frontmatter, else None → Ungrouped
  }
  ```

  Sorted **newest-first** (base names begin with a lexically-sortable
  `YYYY-MM-DD HHmm`, so a reverse string sort is chronological). A malformed or
  missing note degrades that entry's `type`/`duration` to `None` — it never
  drops the recording and never errors the scan.

### Shell (`src-tauri`) — thin command layer

- **`list_recordings(id: String) -> Vec<RecordingDto>`** (in `capture_commands.rs`,
  registered in `lib.rs`): resolve the vault via `discovery::discover_vaults()`,
  read its config, build absolute roots with `capture_paths::safe_recording_root`
  for each `recording_roots()` entry, call `core::list_recordings`, and map to a
  camelCase DTO `{ mp3, title, recordedAt, duration, type }`. A missing vault or
  unreadable roots yields an empty list, not an error (mirrors discovery's
  degrade-to-empty rule).
- **`open_recording(mp3: String)`**: open the recording's companion note
  (`<base>.md`) via `uri::open_file_uri` + `uri::launch`, using the same
  vault-resolution + `vault_relative_no_ext` path `open_transcript` uses. Never
  opens the `.mp3` (Obsidian can't render it) and never writes. If no note
  exists it falls back to the transcript sidecar, else returns an error the view
  surfaces.

### Frontend (Vue + Pinia)

- **`stores/vaults.ts`**: `view` gains `'recordings'`; add `recordingsVaultId:
  string | null`, an `openRecordings(vaultId)` action, and clear it in
  `showList()` (exactly like `captureSettingsVaultId`).
- **`components/RecordModeDialog.vue`**: add a `browse` emit and a "Browse
  recordings" button (visually secondary — a full-width row below the two mode
  buttons, separated by a hairline divider, so it doesn't read as a third
  recording *mode*). Hint: "See past recordings in this vault."
- **`components/ActionPanel.vue`**: extend the header-title switch with a
  `recordings` case ("Recordings"); add an `v-else-if="view === 'recordings' &&
  store.recordingsVaultId"` slot rendering `<Recordings :vault-id=… />`; handle
  the dialog's `browse` by `store.openRecordings(recordRequest.vaultId)` then
  clearing `recordRequest`.
- **`components/Recordings.vue` (new)**: on mount, `invoke('list_recordings', {
  id })`; hold a local `grouped` toggle (default true). Grouped mode renders a
  section per `type` (Ungrouped last) with a small count; flat mode renders one
  newest-first list. Each row is a button: title, date, duration; click →
  `invoke('open_recording', { mp3 })` then `store.panelOpen = false` (Obsidian
  takes over, like `open_vault`). Empty state: "No recordings yet." A failed
  `list_recordings` shows an inline error and an empty list — it never blanks
  the panel.

No new global store: the list is view-scoped and ephemeral, fetched on mount
like `CaptureSettings` fetches its config.

## Invariants preserved

- **Never writes into a vault.** The view only reads (scan + frontmatter) and
  hands off opening to Obsidian via `obsidian://` — the same audit-logged
  `uri::launch` path everything else uses.
- **Scan touches only our files.** `YYYY/MM` only, `is_capture_base` only, via
  `dir_entries` (no symlink follow) — identical discipline to
  `pending_transcriptions`/recovery.
- **Malformed input degrades, never errors.** A garbage note → Ungrouped, no
  duration; an unreadable root → skipped; a failed command → inline error, list
  stays a working panel.

## Edge cases

- **Recording with no note** (edge recovery case): still listed, title/date from
  the base name, `duration —`, Ungrouped.
- **Mode changed over time / custom folder set:** `recording_roots` scans both
  defaults (or the single custom folder), so nothing is missed and nothing is
  double-counted (a recording lives in exactly one root).
- **In-progress `.mp3.part`:** excluded — the scan matches `…\.mp3` suffix,
  which `.mp3.part` fails (and it's dot-hidden anyway).
- **Very long lists:** acceptable for v1 (per-vault scope keeps it bounded); the
  scroll area handles it. In-view search is a deferred non-goal.

## Testing

- **Core (`recordings.rs`, `capture_note.rs`, `capture_config.rs`):**
  - `note_field`: round-trips a `render_note` output for `type`/`duration`;
    returns None for missing frontmatter, missing key; handles quoted values.
  - `recording_roots`: custom folder → `[folder]`; no override → `[Meetings,
    Voice Notes]`.
  - `list_recordings` over a tempdir tree: groups by note `type` (incl. a
    `Meetings/` mp3 whose note says Voice Note), missing note → `None`
    type/duration, newest-first ordering, non-capture files and `.part` ignored,
    a second root merged in. All pure/Linux.
- **Frontend (Vitest + `mockIPC`):**
  - `RecordModeDialog` emits `browse` from the new option and still emits
    `start`/`cancel`.
  - `Recordings.vue`: renders grouped sections + Ungrouped fallback, toggles to
    a flat list, empty state, row click invokes `open_recording` and closes the
    panel, `list_recordings` failure shows the inline error.
  - `vaults` store: `openRecordings` sets `view`/`recordingsVaultId`; `showList`
    clears both.

## Non-goals / scope guards

- **No playback, no delete, no rename** from this view (rename stays the
  post-save prompt). Read-only.
- **No cross-vault aggregation** — per-vault by construction.
- **No in-view search/filter** in v1 (small window, bounded per-vault list).
- **No persistence of the group toggle** — per-view, resets each open.
- **No new frontmatter fields** — we only read back what `render_note` writes.

---

## Addendum — Follow-up template in the companion note

Folded into this increment (approved 2026-07-05). Originally floated as an
auto-created "task note" in a configurable Tasks folder; refined to a scaffold
**inside the existing companion note**, which collapses the risk: it's extra
content in the `<base>.md` the capture path already writes atomically — **no new
vault-write path, no new never-clobber logic.**

**What it is:** a per-vault setting `follow_up_template` (default **on**,
opt-out) that appends a `## Follow-up` scaffold to each recording's companion
note, above the `## Transcript` embed so the actionable part is visible without
scrolling past a long transcript. One template for all recordings:

```markdown
![[2026-07-04 1405 Meeting.mp3]]

## Follow-up

### Action items
- [ ] 

### Decisions

### Notes

## Transcript

![[2026-07-04 1405 Meeting.transcript]]
```

**Architecture — one boolean threaded through the existing note pipeline:**

| Layer | Change |
| --- | --- |
| `core/capture_note.rs` | `NoteMeta.follow_up: bool`; `render_note` emits the `## Follow-up` block (before `## Transcript`) when true. Pure, Linux-tested — the heart. |
| `core/capture_config.rs` | `VaultCaptureConfig.follow_up_template: bool` (default **true**), parsed per-field defensively, round-tripped in `serialize_config` (`followUpTemplate`). |
| `capture/session.rs` | `SessionParams.follow_up`; set on the finalize `NoteMeta`; test helper updated. |
| `capture/recovery.rs` | Recovered notes stay minimal — `follow_up: false` hardcoded (they already omit `recorded_at`, devices, duration). No `recover_root` signature change. |
| `src/capture_commands.rs` | `CaptureConfigDto.follow_up_template`; `get`/`set_capture_config`; `start_capture` sets `SessionParams.follow_up = cfg.follow_up_template`. |
| `src/types.ts` + `CaptureSettings.vue` | `followUpTemplate` toggle, **nested under "Companion note"** (shown only when `createNote` is on — no note, nowhere to put the scaffold), mirroring the transcription sub-options' nesting under Transcribe. |

**Invariants preserved:**
- **No new vault-write path** — same atomic, never-clobber companion-note write
  (`write_note_collision_safe`), just more content.
- **Gated by `create_note`** — `render_note` runs only when a note is written,
  so `follow_up_template=true` + `create_note=false` is a no-op, and the UI
  hides the toggle then.
- **Rename-safe** — `retarget_embed` rewrites only the `![[…]]` embed line; the
  static scaffold is untouched.

**Scope guards (YAGNI):** one template for all recordings (no per-mode
variants); default on ("toggled off" framing = opt-out); follow-up above the
transcript; **recovered notes stay minimal** (no scaffold — a defensible edge,
trivially threadable later if wanted); the original Tasks-folder / separate-note
idea is **parked**, not built.
