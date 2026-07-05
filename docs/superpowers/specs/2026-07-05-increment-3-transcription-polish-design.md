# Increment 3 Polish Design — "Transcription: fix, pin, open"

- **Date:** 2026-07-05
- **Status:** Approved
- **Source:** First real-world use of the local speech-to-text vertical slice
  ([Increment 3](2026-07-04-increment-3-local-speech-to-text-design.md)).
  The slice works end to end (real build, `small` model, real recording),
  and three rough edges surfaced in that first run.

## Goal

Three focused improvements, driven by using the feature for real:

1. **Fix the translate bug.** German speech came out as English text with the
   language on Auto — the model *translated* instead of *transcribing*.
2. **Language as a dropdown**, not a free-text field, so language codes can't
   be mistyped (and pinning a language avoids the drift in #1).
3. **"Open in Obsidian" after a transcript finishes**, so the result is one
   click away instead of hunting for it in the vault.

Each is small and independently shippable. None changes the transcription
architecture, the write-safety contract, or the `obsidian://` delegation rule.

## 1. Fix the translate bug

**Cause.** `transcribe/src/engine.rs::transcribe()` sets the language only when
one is supplied and **never sets the translate flag**. Whisper's multilingual
models — the `small` tier especially — drift to English *translation* on
auto-detect when the task isn't pinned. So two gaps compounded: no explicit
"transcribe, don't translate", and Auto left the door open.

**Fix.** Call `params.set_translate(false)` unconditionally in `transcribe()`,
so the engine always produces verbatim text in the spoken (or selected)
language. Item #2 (pinning a language) further removes the auto-detect drift.

**Non-goal.** No "translate to English" feature. If that's ever wanted it's a
separate, explicit option — not the default behavior.

**Testing.** Whisper inference can't be unit-tested without a model + audio
fixture, so correctness is verified by re-recording German and confirming
German out. The change is a single, safe parameter; it is compile-checked with
the `whisper` feature on Linux (toolchain available) and by CI's Windows job.

## 2. Language dropdown

**Change.** In `components/CaptureSettings.vue`, replace the free-text language
input (`transcription-language-input`) with a `<select>`
(`transcription-language-select`). Options — a `const LANGUAGES` of
`{ code, name }`, mirroring the existing `MODELS`/`BITRATES` consts:

| Value (`code`) | Label |
| --- | --- |
| `""` | Auto-detect (default) |
| `en` | English |
| `de` | German |
| `es` | Spanish |
| `fr` | French |
| `it` | Italian |
| `pt` | Portuguese |
| `nl` | Dutch |
| `pl` | Polish |
| `zh` | Chinese |
| `ja` | Japanese |
| `ru` | Russian |
| `ar` | Arabic |

**Config mapping is unchanged.** The `transcriptionLanguage` ref keeps its
existing null↔"" mapping (load: `null` → `""`; save: `"" ` → `null`), and
`set_capture_config` keeps `.filter(|l| !l.is_empty())`. Only the control type
changes from `input` to `select`.

**Backend stays permissive.** `set_capture_config` does **not** start
validating the language against this list. `config.json` is hand-editable and
Whisper supports ~99 languages, so a hand-set `sv` (Swedish) must still work.
The dropdown solves the *mistyped-key* problem for the UI; it does not lock the
backend down. This mirrors the existing per-field-defensive config rule.

**Testing.** `tests/capture-settings.test.ts`: the control is now a select —
assert it renders with the options (including Auto-detect), reflects a loaded
code (e.g. `de` selected), saves the chosen code (`transcriptionLanguage: "de"`),
and saves `null` when Auto-detect is chosen.

## 3. "Open in Obsidian" after finish

Manual, not automatic (recordings often finish in the background — auto-launch
would yank the user out of what they're doing).

### Data flow

`process_transcription` already emits `capture:transcribed` with `{ mp3,
transcript }` on success. The frontend records the most-recent completion and
offers a button that opens it in Obsidian via a new command.

- **`stores/capture.ts`** — add `lastTranscribed: { mp3: string } | null`.
  Set it on `capture:transcribed` (store the `mp3` path — the canonical base
  identity). Clear it on `capture:started` (a new recording supersedes it).
  Add an `openTranscript()` action that invokes the command and surfaces any
  error the existing way.
- **`components/TranscriptionStatus.vue`** — while `lastTranscribed` is set and
  nothing is actively transcribing/downloading, show a persistent
  "✓ Transcribed — Open in Obsidian" row whose button calls
  `openTranscript()`.

### The `open_transcript` command (shell)

A thin wrapper in `capture_commands.rs`, registered in `lib.rs`:

1. Take the recording's `mp3` path.
2. Find the owning vault (reuse `owning_vault_id` / `discover_vaults` to get the
   vault **id + path**).
3. Choose the target: the **companion note** `<base>.md` if it exists (it
   embeds the transcript and the audio player — the richest view), otherwise
   the transcript sidecar `<base>.transcript.md` (for the `createNote: false`
   case).
4. Build an `obsidian://open?vault=<id>&file=<vault-relative>` URI and launch
   it, logging via `uri::launch` — the same delegate-and-audit path as
   `open_vault` / `open_daily_note`. **No vault write.**
5. Return `Result<(), String>`; the frontend shows the error (vault not found —
   e.g. the recording was moved — or launch failure) via the existing
   toast/banner path.

### Keep the logic testable on Linux

The pure part lives in **core** and is unit-tested there; the shell command is
a thin wrapper (the shell only compiles on Windows):

- `uri.rs` — an `open_note_uri(vault_id, vault_relative_file)` builder
  (address by ID, percent-encode), reusing the construction `daily_note_uri`
  already uses for `obsidian://open`.
- A helper deriving the `<base>.md` / `<base>.transcript.md` candidates and the
  vault-relative path from an absolute recording path + vault root. The
  existence check (note vs sidecar) stays in the shell (filesystem); the path
  math and URI building are pure and tested. The vault-relative `file` value is
  produced **without the `.md` extension** — the form `daily_note_uri` already
  uses, and one Obsidian resolves for existing notes (so the sidecar
  `<base>.transcript.md` is passed as `<base>.transcript`).

**Testing.** Core unit tests for the path derivation + URI builder (Linux).
Frontend Vitest: the store sets/clears `lastTranscribed` and `openTranscript`
invokes the command; `TranscriptionStatus` renders the button only when
appropriate and wires it to the action.

## Invariants preserved

- **`obsidian://` delegation** — opening is a launched, logged URI, never a
  direct file open; addressed by vault **id**, every parameter percent-encoded.
- **No vault write** — the "open" path is read-only; the never-clobber capture
  contract is untouched.
- **Per-field-defensive config** — the dropdown constrains the UI only; the
  backend keeps accepting any hand-edited language string.

## Non-goals / scope guards

- No auto-open (manual button only).
- No translate-to-English feature.
- No backend language validation lockdown.
- No change to the model registry, download flow, or the transcript sidecar
  format.
