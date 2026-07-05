# Increment 3 Polish Design — "Readable, tidy settings"

- **Date:** 2026-07-05
- **Status:** Approved
- **Source:** First real UI use of the transcription settings
  ([polish round 1](2026-07-05-increment-3-transcription-polish-design.md))
  surfaced readability and small UX rough edges: the native dropdown option
  lists are near-unreadable on the dark theme, the "Open in Obsidian" row has
  no way to go away, and "Saved ✓" lingers after further edits.

## Goal

Six improvements to the capture settings panel, the transcription status row,
and the vault list. Five are small; the custom dropdown (#5) is the bulk of the
work and is cleanly separable. None changes the config schema, the IPC command
surface, or any write-safety rule.

## 1. `color-scheme: dark` baseline (`src/style.css`)

**Problem.** No `color-scheme` is set anywhere, so WebView2 renders native
controls (the `<select>` option popups especially) with light-mode defaults —
on the dark panel the option text is a low-contrast gray, effectively
unreadable.

**Change.** Add `color-scheme: dark;` to the existing `html, body, #app` rule.
This is app-wide: native checkboxes (Companion note, Transcribe, Timestamps)
and the scrollbar render dark-correctly. Once #5 replaces the selects, this
still governs every remaining native control.

**Testing.** Visual/manual (a CSS property has no unit test); covered
indirectly because the existing suite still passes.

## 2. "Open in Obsidian" row — open *or* dismiss (`stores/capture.ts`, `components/TranscriptionStatus.vue`)

**Problem.** The completion row (`lastTranscribed`) never goes away: "Open in
Obsidian" opens but doesn't dismiss, and there's no way to dismiss without
opening.

**Change.** The row gets two controls:
- **Open in Obsidian** → `openTranscript()` opens the note and, on success,
  clears `lastTranscribed` so the row disappears. On failure it keeps the row
  (retry) and sets the warning.
- **✕ (dismiss)** → a new `dismissTranscribed()` action clears
  `lastTranscribed` without opening.

A new recording already clears `lastTranscribed` (via `start()`), so all three
paths converge on the same reset.

**Testing.** `capture-store.test.ts`: `openTranscript` clears `lastTranscribed`
on success and keeps it on failure; `dismissTranscribed` clears it.
`transcription-status.test.ts`: the ✕ button renders and invokes
`dismissTranscribed`.

## 3. "Saved ✓" clears on edit (`components/CaptureSettings.vue`)

**Problem.** After a save, `saveState` stays `"saved"` while the user keeps
editing, so "Saved ✓" wrongly implies the new edits are saved.

**Change.** Add a Vue `watch` over the form's reactive fields (mode,
recordingFolder, createNote, bitrateKbps, inputDevice, outputDevice,
transcribe, transcriptionModel, transcriptionLanguage, transcriptTimestamps)
that resets `saveState` to `"idle"` on any change. No load-time guard is
needed: `saveState` is `"idle"` during the initial `onMounted` load, so the
watcher firing on those assignments is an idle→idle no-op; it only becomes
visible after a save has set it to `"saved"`, which is exactly when an edit
should clear it.

**Testing.** `capture-settings.test.ts`: after a successful save shows
"Saved ✓", editing any field hides it.

## 4. Group the transcription sub-settings (`components/CaptureSettings.vue`)

**Problem.** When Transcribe is on, Model/Language/Timestamps sit at the same
visual level as unrelated settings, so their relationship to the toggle is
unclear.

**Change.** Wrap the `v-if="transcribe"` block (Model, Language, Timestamps) in
a container with a subtle left border + left padding (e.g.
`border-l border-white/10 pl-3`) and matching vertical gap, so it reads as
belonging to the Transcribe toggle above it. No behavior change.

**Testing.** Covered by the existing "controls show/hide with transcribe"
tests (the `data-testid`s are unchanged).

## 5. Custom dropdown component (`components/SelectMenu.vue` + `CaptureSettings.vue`)

Replace all five native `<select>`s (Model, Language, Bitrate, Microphone,
Desktop audio) with one reusable, accessible dropdown that matches the app's
glass theme and — unlike the native popup — stays inside the window.

### Component API

```
SelectMenu.vue
  props:
    modelValue: string | number          // v-model
    options: { value: string | number; label: string }[]
    id?: string                          // for the trigger button + label association
    ariaLabel?: string
    dataTestid?: string                  // applied to the trigger button
  emits:
    update:modelValue(value)
```

A drop-in for the current selects, including the numeric Bitrate (values may be
`string | number`; the component compares and emits them as-is, preserving the
existing `v-model.number` semantics via `number` option values).

### Behavior

- **Trigger:** a button styled like the current select boxes — shows the
  selected option's label + a chevron. `data-testid` and `id` pass through so
  the existing labels/`for=` associations and tests keep working.
- **Popup:** a list styled to match (dark, rounded, `white/10` border), each
  row a `role="option"`; the selected row is violet-accented and
  `aria-selected`.
- **Escapes the scroll clip:** the settings sit in an `overflow-y-auto` panel
  (`ActionPanel.vue`) that would clip an in-flow popup, so the popup is
  rendered via `<Teleport to="body">` and positioned `fixed` from the
  trigger's `getBoundingClientRect()`. It is capped to the window height with
  internal scroll for long lists (13 languages), and opens **upward** when
  there isn't room below.
- **Accessible & dismissible:** `role="listbox"`, `aria-expanded` on the
  trigger, `aria-activedescendant` for the highlighted option; keyboard nav
  ↑/↓ to move, Enter to select, Esc to close; click-outside closes; focus
  returns to the trigger on close. **No type-ahead** (YAGNI).
- **Reposition safety:** recompute position on open; close on scroll/resize
  rather than tracking (the popup is short-lived and the panel is small).

### Testing

- New `tests/select-menu.test.ts`: renders the selected label; opening shows
  the options; clicking an option emits `update:modelValue` with its value and
  closes; Esc and click-outside close; ↑/↓/Enter select via keyboard.
- `capture-settings.test.ts`: dropdown interactions move from native
  `.setValue()` / `element.value` to the custom component (open the
  `SelectMenu`, click the target option). The **saved payload assertions are
  unchanged** — the point is that swapping the control doesn't change what gets
  saved.

## 6. Vault-row transcription indicator (`capture_commands.rs`, `stores/capture.ts`, `components/VaultList.vue`)

**Problem.** While a recording transcribes in the background, nothing on the
vault list shows *which* vault it's for — unlike recording, which already
pulses a red dot on its row (`VaultList.vue`).

**Change.** Mirror the recording indicator with a transcription one:
- **Backend:** `process_transcription` already emits `capture:transcribing`;
  add `"vaultId": job.vault_id` to that payload. No new events — the single
  transcription worker means exactly one vault transcribes at a time.
- **Store:** add `transcribingVaultId: string | null`, set from the
  `capture:transcribing` payload and cleared on both `capture:transcribed` and
  `capture:transcribeFailed` (whichever ends the current job).
- **VaultList:** beside the existing open/recording dots, show a pulsing
  **violet** dot (matching the transcribing status color) titled
  "Transcribing…" when `vault.id === capture.transcribingVaultId`.

**Testing.** `capture-store.test.ts`: `capture:transcribing` with a `vaultId`
sets `transcribingVaultId`; `transcribed`/`transcribeFailed` clear it.
`vault-list.test.ts`: the transcribing dot renders on the matching vault row.
The backend one-field addition is compiled by CI's Windows job (shell); an
event field has no unit test.

## Invariants preserved

- **Config schema + IPC unchanged.** `SelectMenu` is a presentation swap; the
  `get/set_capture_config` payloads and the `transcriptionLanguage` null↔""
  mapping are untouched.
- **Minimal backend touch.** The only Rust change is adding a `vaultId` field
  to the existing `capture:transcribing` event (#6) — no new events, no new
  writes, no vault interaction, no schema or IPC-command change. Everything
  else is frontend (`src/` + `style.css`).

## Non-goals / scope guards

- No type-ahead in `SelectMenu`.
- No auto-open of transcripts (still manual).
- No redesign of the panel layout beyond the grouped transcription block.
- No change to the mode selector (already custom buttons) or text inputs.
