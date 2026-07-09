# Record View Improvements Design — actions first, browse card with count, no default mode, one Save

- **Date:** 2026-07-09
- **Status:** Approved
- **Source:** User feedback on the Record view (v0.5.1): the two recording
  actions should lead the view, browsing past recordings should look like a
  third action (with a count), the "default recording mode" vault setting has
  outlived its purpose, and the tasks folder shouldn't need its own Save.

## Goals

1. **Reorder the Record view** — the recording actions come first; the
   transcription settings become the last section.
2. **"Browse recordings" becomes a real card button**, visually aligned with
   the Meeting / Voice Note options, showing **how many recordings** the vault
   has.
3. **Remove the "Default recording mode" setting** from Vault settings — the
   mode is picked per recording in the Record view, so a stored default no
   longer drives anything the user can see.
4. **Fold the tasks folder into the main Vault-settings Save** — no dedicated
   Save button for one field.

## Record view (`RecordMode.vue`)

New layout, top to bottom:

1. **Meeting** and **Voice Note** cards — unchanged structure (title + hint),
   but **no default highlight**: with the default-mode concept gone, neither
   option is pre-selected. Both use the neutral card style
   (`border-white/10 bg-white/5 hover:bg-white/10`).
2. **Browse recordings** card — same card style and structure as the two
   action cards (title "Browse recordings", hint "See past recordings in this
   vault"), replacing today's small text link. A **count pill** on the card's
   right side shows the vault's recording count. Keeps
   `data-testid="mode-browse"`; the count is `data-testid="recording-count"`.
3. A divider, then **`TranscriptionSettings`** as the last section (today it
   is first).

**Recording count:** fetched on mount via the existing read-only
`list_recordings` command (`.length` of the returned list — the Recordings
view already runs this identical scan on its own mount, so cost and staleness
behavior match). A failed invoke hides the pill and logs through `logWarning`
(the `taskCounts` degrade pattern); a successful `0` **is** shown — knowing a
vault has no recordings before clicking is the point of the count. The
button's aria-label includes the count when known.

`defaultMode` (the ref driving the highlight) is removed. The full-config
load stays: `config` still seeds `TranscriptionSettings` and `persist()` still
round-trips the untouched fields (including `mode`) exactly as today — the
loading/`loaded` gate is unchanged.

## Vault settings (`CaptureSettings.vue`)

- **Remove the "Default recording mode" radio group.** The `mode` value keeps
  flowing: it is loaded from `get_capture_config` and sent back **unchanged**
  in the save payload. The IPC contract and `config.json` schema stay as they
  are — `mode` becomes a pass-through the UI can no longer edit. (It only ever
  supplied the Record view's highlight and the `start_capture(mode: None)`
  fallback, which no frontend caller uses.)
- **Recording-folder placeholder** was mode-dependent ("Meetings" / "Voice
  Notes"). With no mode control it becomes the static
  `"Meetings or Voice Notes"` — honest about the per-type defaults without
  reading as a nested path (this input accepts `/` paths).
- **"Desktop audio from" (output device) is always visible** — it was gated on
  meeting mode. Its hint says what the gate used to imply: used for meeting
  recordings (loopback).
- **Tasks folder joins the form's single Save.** The dedicated Save button and
  its Enter handler go away (Enter in the input now submits the form
  naturally). `save()` persists both configs as **independent invokes** —
  `set_capture_config` first, then `set_tasks_config` — so one failure can't
  block the other (the reason the command pair is separate, preserved).
  A tasks-folder failure surfaces inline under the field
  (`tasks-folder-error`, as today) and suppresses "Saved ✓"; `tasksFolder`
  joins the watch list that invalidates a shown "Saved ✓".
- **The tasks write is gated on loaded-or-edited** (found in review): the form
  is submittable before the `get_tasks_config` read resolves (that read runs
  after the capture `loading` gate on purpose) and stays usable after a failed
  read — an unconditional write would send the default-seeded `""` (→ `null`)
  and clear a configured folder the form never saw. So `save()` writes the
  tasks config only once its value has loaded, or after an explicit user edit
  (typed input is explicit intent even when the read failed) — and a
  late-resolving read never clobbers a field the user already edited. This is
  `RecordMode.vue`'s `loaded` persist gate, applied to the second config.

## Rust

**No changes.** The count reuses `list_recordings`; `mode` stays in
`CaptureConfigDto` and `config.json` (per-field-defensive parsing keeps old
files valid either way).

### Alternatives considered

- *Remove `mode` from the DTO + config entirely:* cleaner end state, but it
  touches the invariant-heavy config write path, the IPC contract in two
  frontend consumers, and `start_capture`'s fallback for zero user-visible
  gain. Pass-through wins.
- *A dedicated `count_recordings` command:* duplicates the scan
  `list_recordings` already does, to save bytes on an IPC response measured
  in tens of rows. Reuse wins.
- *Auto-save the tasks folder on blur:* a second, implicit save style on a
  form whose every other field saves via the one button. Merging into Save
  wins.

## Testing

- **`record-mode.test.ts`:**
  - The mode cards render **before** the transcription section (DOM order).
  - Neither mode card carries the default highlight (`border-violet-400`),
    regardless of the config's stored `mode`.
  - The browse card shares the action-card classes and shows the count pill
    from `list_recordings` (including `0`); a failed `list_recordings` hides
    the pill and logs a warning.
  - Existing persist/start/navigate tests updated (highlight assertions
    dropped; `list_recordings` added to the mocks).
- **`capture-settings.test.ts`:**
  - No default-mode radio group renders; the output-device picker shows even
    when the stored mode is `voice-note` (replaces the "hides in voice-note
    mode" test).
  - Submitting the form saves the tasks folder via `set_tasks_config`
    (trimmed; `""` → `null`) — and there is no `tasks-folder-save` button.
  - A tasks-folder save failure shows `tasks-folder-error`, withholds
    "Saved ✓", and still saves the capture config (and the reverse: a capture
    failure doesn't skip `set_tasks_config`).
  - The save payload still carries the loaded `mode` unchanged.

## Out of scope

Removing `mode` from the Rust config/DTO; a Rust-side recordings counter;
restyling the Recordings list itself; any change to how recordings start or
where they land.
