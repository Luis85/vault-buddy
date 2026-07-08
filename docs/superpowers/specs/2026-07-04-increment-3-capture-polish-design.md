# Increment 3 Design — "Capture, polished"

- **Date:** 2026-07-04
- **Status:** Approved
- **Source:** Second increment cut from [docs/PRD-knowledge-intake.md](knowledge-intake.md); builds on the shipped increment 2 ([spec](2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md), PR #4).

## Goal

Make capture configurable and comfortable: every per-vault capture setting
editable from the panel (including device choice), pause/resume, a live
input-level meter, post-save rename to human titles, and a per-vault
recording indicator.

## Scope

### In scope

1. **Per-vault capture settings view** — a ⚙ button on each vault row opens
   that vault's settings form inside the panel (back button returns to the
   list): recording mode (Meeting / Voice Note), recording folder (text
   field validated by the `safe_recording_root` rules), companion-note
   toggle, bitrate (128/160/192), input-device picker, and — in Meeting
   mode only — loopback output-device picker. "System default" is always
   the first option and the default.
2. **Config write path** — `set_capture_config` persists
   `%APPDATA%\vault-buddy\config.json` atomically (owned temp + rename;
   replacement is correct here — it is our own file, not a vault). Schema
   gains optional `inputDevice` / `outputDevice` (cpal device names;
   absent = system default). A configured device missing at record time
   falls back to the default with a warning — stale config never blocks
   recording.
3. **Device enumeration** — `list_audio_devices()` returns input devices
   and (for loopback) output devices by name with a default flag.
4. **Pause/resume** — recording-bar button plus a mirrored tray item.
   Paused: streams stay open, drained samples are discarded, nothing is
   encoded — the timeline skips the gap. Elapsed display freezes; the
   buddy/tray dot switches to steady amber; the note metadata records the
   total paused duration. The 30 s fsync cadence keeps running.
5. **Live level meter** — the worker emits the post-mix per-tick peak as
   `capture:level` (~5 Hz, normalized 0–1); the recording bar renders a
   small meter. Advisory only — nothing depends on it.
6. **Rename on save** — after a save the panel shows a one-line "Name this
   recording" prompt (prefilled with the timestamp base) for ~30 s beside
   the saved confirmation. Confirming calls `rename_capture`: the
   mp3+note pair is renamed through the pairwise reservation +
   `rename_noreplace`, and the note's `![[…]]` embed is rewritten. The
   new base keeps the `YYYY-MM-DD HHmm ` prefix (`… <title>`) so sorting,
   recovery ownership (`is_capture_base`), and collision rules still hold.
   Skipping keeps timestamp names; the prompt dismisses on new recording
   or panel close with no dangling state.
7. **Per-vault recording indicator** — the recording vault's row shows a
   pulsing red dot (the store consumes the `vaultId` the backend already
   reports).

### Out of scope (deferred)

Naming templates, other capture providers, AI pipeline, sample-rate/channel
settings (fixed 44.1 kHz stereo), PRD General-section vault icons/colors,
non-Windows capture.

## Architecture

### Core crate (pure additions)

| Addition | Responsibility |
| --- | --- |
| `capture_config::write_config` / `serialize_config` / `update_vault_config(vault_id, cfg)` | Serialize the full `AppConfig` (with the two new optional device fields) to pretty JSON; atomic write via owned `.vault-buddy.tmp` temp + rename. `update_vault_config` is read-modify-write so saving one vault preserves the others; the command layer serializes calls behind a small mutex. |
| `capture_paths::rename_plan(mp3, new_title) -> RenamePlan` | Pure planning: sanitize the title (strip path separators/control characters, trim; reject empty or longer than 120 characters), derive `YYYY-MM-DD HHmm <title>` from the existing base, return the mp3/note rename pair. Execution reuses reservation + `rename_noreplace`. |
| `capture_note::retarget_embed(note, old_mp3, new_mp3)` | Rewrite exactly the `![[…]]` embed line in an existing note. |

### Capture crate

- `session`: the stop channel generalizes to `enum Control { Stop, Pause,
  Resume }`. While paused the drain loops keep running (device loss still
  detected, buffers fresh) but samples are discarded and nothing is
  encoded; `paused_total: Duration` accumulates into the note metadata.
- Level tap: after mixing, the worker computes the tick's normalized peak
  and sends it on an optional `level_tx` every other tick (~5 Hz).
- `devices`: `list_devices() -> DeviceList { inputs, outputs }` with
  `DeviceInfo { name, is_default }`; `open_sources` gains
  `preferred_input` / `preferred_output: Option<&str>` resolved by name,
  falling back to the default with a returned warning when missing.

### Shell

New commands: `get_capture_config(id)`, `set_capture_config(id, cfg)`
(validates the folder via `safe_recording_root` before writing),
`list_audio_devices()`, `pause_capture()` / `resume_capture()`,
`rename_capture(mp3, title)`. New events: `capture:level { peak }`,
`capture:paused`, `capture:resumed`. `ActiveCapture` tracks `paused`;
the tray menu item flips Pause ⇄ Resume and the icon builder gains the
amber paused variant.

### Frontend

- `capture` store: adds `paused`, `level`, `vaultId` (consumed by the
  vault list), `lastSaved { mp3, note } | null` with a rename-pending
  window; actions `pause / resume / rename(title) / dismissRename`.
- New `CaptureSettings.vue` (gear view: form bound to get/set commands,
  device dropdowns, inline validation and save feedback). `VaultList.vue`
  gains the ⚙ emit and the recording-row dot. `RecordingBar.vue` gains
  Pause/Resume, the level meter, and elapsed frozen while paused (wall
  time minus accumulated pause, mirrored from pause/resume events).
  A small inline `RenamePrompt` appears under the list after a save.
- Panel view state grows to `list | settings | captureSettings(vaultId)`
  in the `vaults` store (view state lives in the store because the panel
  component is destroyed while closed).

### Data flow (settings save)

Gear click → `get_capture_config(id)` fills the form → edit →
`set_capture_config` validates + writes atomically → the next
`start_capture` reads the fresh file (config stays deliberately uncached).

## Error handling

- **Settings save:** invalid folder → inline field error, nothing written.
  Write failure → form-level error, form state preserved. Concurrent saves
  for different vaults are safe (serialized read-modify-write).
- **Stale devices:** missing at start → default + `capture:warning` +
  note-metadata event; never a start failure. Pickers suffix
  "(not connected)" for configured-but-absent devices.
- **Pause:** pausing during `starting`/`saving` → typed error (UI disables
  the button in those states). Source loss while paused still finalizes.
  Stop/quit while paused finalizes normally — pause never blocks shutdown.
- **Rename:** empty-after-sanitizing → prompt error, no call. Target taken
  → reservation advances the suffix (never clobbers). Mp3 renamed but note
  retarget fails → warning reports both paths (audio first; the note is
  repairable). Prompt expiry leaves timestamp names — no dangling state.
- **Level:** advisory; a lost event only stalls the meter.

## Testing

- **Core:** config round-trip with device fields and absent-field
  defaults; `update_vault_config` preserves sibling vaults; `rename_plan`
  sanitization matrix (separators, dots, unicode, empty, overlong);
  retitled bases still satisfy `is_capture_base`; `retarget_embed`
  touches only the embed line.
- **Capture:** pause/resume via control channel on synthesized sources —
  paused span excluded from duration, resume continues, stop-while-paused
  finalizes; level values in 0–1 for a known sine; `list_devices` clean on
  device-less CI.
- **Vitest:** store pause/resume/level/rename transitions; CaptureSettings
  load-edit-save incl. validation errors; VaultList gear emit +
  recording-row dot; RecordingBar paused/meter/frozen-elapsed rendering.
- **Windows checklist additions:** real device pick honored; stale-device
  fallback warning; pause gap absent from playback with frozen elapsed and
  amber dot; rename produces correct files + working Obsidian embed;
  per-vault dot on the correct row; meter tracks speech.

## Success criteria

1. Every field in the ⚙ form round-trips to config.json and visibly
   affects the next recording (mode, folder, note, bitrate, devices).
2. Unplugged configured devices degrade to defaults with a visible warning.
3. Pause produces a gap-free recording whose duration excludes the pause;
   UI/tray/buddy reflect paused state; note metadata shows paused time.
4. The meter moves with speech and sits near zero on silence.
5. Rename yields `YYYY-MM-DD HHmm <title>.mp3/.md` with a working embed,
   collision-safe, and recovery still recognizes the files as ours.
6. The recording vault's row is visibly marked while recording.
7. All Rust and Vitest suites green in CI; fmt/clippy clean.
