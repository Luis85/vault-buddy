# Transcription Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the German→English transcription bug, turn the language field into a curated dropdown, and add a manual "Open in Obsidian" action when a transcript finishes.

**Architecture:** Three small, independent slices. (1) One whisper param in the `transcribe` crate. (2) A `<select>` swap in the Vue settings form. (3) A read-only "open" path: a pure core URI helper (Linux-tested) + a thin shell command + a store flag and a button in the status component.

**Tech Stack:** Rust (whisper-rs behind the `whisper` feature; `vault_buddy_core`), Vue 3 + Pinia + Tailwind, Vitest.

## Global Constraints

- **Read-only open.** The new "open" path never writes into a vault. It launches an `obsidian://open` URI via `vault_buddy_core::uri::launch` (which logs the URI — the audit trail) exactly like `open_vault`/`open_daily_note`.
- **Address vaults by ID; percent-encode every URI parameter.** Reuse `uri::open_file_uri` (already does both).
- **Backend config stays permissive.** `set_capture_config` must NOT start validating `transcriptionLanguage` against the curated list — `config.json` is hand-editable and Whisper supports ~99 languages. The dropdown is a UI convenience only.
- **Whisper always transcribes.** `translate = false` unconditionally; no translate-to-English feature.
- **What compiles where:** `core/` + `src/` (frontend) build and test on Linux. The shell crate (`src-tauri/src/*`) compiles on Windows only — verify it with `cargo fmt --check` locally; CI's `windows-app` job is the compile gate. The `transcribe` crate's `whisper` feature builds on Linux when libclang + cmake are present.
- **Commits:** Conventional Commits — `fix(transcribe)`, `feat(ui)`, `feat(core)`, `feat(shell)`.

---

### Task 1: Always transcribe, never translate (`engine.rs`)

**Files:**
- Modify: `src-tauri/transcribe/src/engine.rs` (inside `transcribe()`)

**Interfaces:**
- Consumes: nothing new.
- Produces: no signature change — `WhisperTranscriber::transcribe` behaves the same, but the engine now pins the whisper task to transcription.

**Why no unit test:** whisper inference can't be exercised without a model + audio fixture, and `FullParams` exposes no getter for the translate flag. This task is verified by compiling with the `whisper` feature and by the user's real re-record (German in → German out). This is the one non-TDD task in the plan, per the spec.

- [ ] **Step 1: Add the translate flag**

In `src-tauri/transcribe/src/engine.rs`, inside `transcribe()`, add `params.set_translate(false);` immediately before the language block. Result:

```rust
        params.set_n_threads(n_threads);
        // Always transcribe in the spoken/selected language — never translate
        // to English. The multilingual models (small especially) otherwise
        // drift to English translation on auto-detect; pinning the task off is
        // the reliable fix, and a pinned language (settings dropdown) removes
        // the drift entirely.
        params.set_translate(false);
        if let Some(lang) = language {
            params.set_language(Some(lang));
        }
```

- [ ] **Step 2: Verify it compiles with the whisper feature**

Run (from `src-tauri/`): `cargo build -p vault_buddy_transcribe --features whisper`
Expected: `Finished` with no errors. (First build compiles whisper.cpp via cmake — a few minutes; needs libclang + cmake present. If the toolchain is absent, skip and rely on CI's `windows-app` job.)

- [ ] **Step 3: Format check**

Run (from `src-tauri/`): `cargo fmt --check`
Expected: exit 0.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/transcribe/src/engine.rs
git commit -m "fix(transcribe): always transcribe, never translate to English"
```

---

### Task 2: Language dropdown (`CaptureSettings.vue`)

**Files:**
- Modify: `src/components/CaptureSettings.vue`
- Test: `tests/capture-settings.test.ts`

**Interfaces:**
- Consumes: the existing `transcriptionLanguage` ref and its null↔"" mapping (load `cfg.transcriptionLanguage ?? ""`; save `transcriptionLanguage.value.trim() || null`) — both unchanged.
- Produces: the language control's `data-testid` changes from `transcription-language-input` to `transcription-language-select`.

- [ ] **Step 1: Update the two affected tests to expect a select (RED)**

In `tests/capture-settings.test.ts`, in the test **"shows the model/language/timestamps controls, loaded correctly, once transcribe is on"**, change the language lookup from an input to a select:

```ts
    const language = wrapper.get<HTMLSelectElement>(
      '[data-testid="transcription-language-select"]',
    );
    expect(language.element.value).toBe("es");
```

In the test **"saves transcription settings after enabling transcribe and picking a model/language"**, change the language interaction target:

```ts
    await wrapper.get('[data-testid="transcription-language-select"]').setValue("es");
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: FAIL — `Unable to get [data-testid="transcription-language-select"]` (the select doesn't exist yet).

- [ ] **Step 3: Add the `LANGUAGES` const**

In `src/components/CaptureSettings.vue`, beside the existing `const MODELS = [...] as const;`, add:

```ts
const LANGUAGES = [
  { code: "", name: "Auto-detect" },
  { code: "en", name: "English" },
  { code: "de", name: "German" },
  { code: "es", name: "Spanish" },
  { code: "fr", name: "French" },
  { code: "it", name: "Italian" },
  { code: "pt", name: "Portuguese" },
  { code: "nl", name: "Dutch" },
  { code: "pl", name: "Polish" },
  { code: "zh", name: "Chinese" },
  { code: "ja", name: "Japanese" },
  { code: "ru", name: "Russian" },
  { code: "ar", name: "Arabic" },
] as const;
```

- [ ] **Step 4: Replace the language text input with a select**

In `src/components/CaptureSettings.vue`, replace the whole language `<section>` (the one containing `data-testid="transcription-language-input"`) with a select matching the existing Model-select markup:

```html
    <section class="flex items-center justify-between gap-2">
      <label for="capture-transcription-language" class="text-sm text-slate-200">Language</label>
      <select
        id="capture-transcription-language"
        v-model="transcriptionLanguage"
        data-testid="transcription-language-select"
        class="rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none"
      >
        <option v-for="l in LANGUAGES" :key="l.code" :value="l.code">{{ l.name }}</option>
      </select>
    </section>
```

(The `transcriptionLanguage` ref, its `onMounted` load, and the `save()` payload are unchanged — `""` still maps to `null` on save.)

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: PASS (all tests, including the two edited).

- [ ] **Step 6: Typecheck + full suite**

Run: `npm run build` (vue-tsc must pass) and `npm test`
Expected: both green.

- [ ] **Step 7: Commit**

```bash
git add src/components/CaptureSettings.vue tests/capture-settings.test.ts
git commit -m "feat(ui): pick transcription language from a dropdown"
```

---

### Task 3: Core URI helper `vault_relative_no_ext` (`uri.rs`)

**Files:**
- Modify: `src-tauri/core/src/uri.rs`
- Test: `src-tauri/core/src/uri.rs` (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub fn vault_relative_no_ext(file: &std::path::Path, vault_root: &std::path::Path) -> Option<String>` — the `file`-parameter value for `open_file_uri`. Task 4 consumes this.

- [ ] **Step 1: Write the failing test (RED)**

In `src-tauri/core/src/uri.rs`, add to the existing `mod tests`:

```rust
    #[test]
    fn vault_relative_drops_the_md_extension_and_normalizes_separators() {
        use std::path::Path;
        let root = Path::new("/vault");
        // a note: drop `.md`
        assert_eq!(
            vault_relative_no_ext(Path::new("/vault/2026/07/Meeting.md"), root).as_deref(),
            Some("2026/07/Meeting")
        );
        // a sidecar: only the final `.md` goes, the inner `.transcript` stays
        assert_eq!(
            vault_relative_no_ext(Path::new("/vault/2026/07/Meeting.transcript.md"), root)
                .as_deref(),
            Some("2026/07/Meeting.transcript")
        );
        // a file outside the vault → None
        assert_eq!(
            vault_relative_no_ext(Path::new("/elsewhere/x.md"), root),
            None
        );
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core uri::`
Expected: FAIL — `cannot find function vault_relative_no_ext`.

- [ ] **Step 3: Implement the helper**

In `src-tauri/core/src/uri.rs`, add (after `new_file_uri`, before the tests):

```rust
/// The `file` value for an `obsidian://open?file=` URI: `file`'s location
/// under `vault_root`, `/`-separated, with the final extension dropped —
/// Obsidian resolves `2026/07/Meeting` to `Meeting.md`, and a sidecar
/// `Meeting.transcript.md` to `Meeting.transcript`. Returns `None` when `file`
/// is not inside `vault_root`.
pub fn vault_relative_no_ext(
    file: &std::path::Path,
    vault_root: &std::path::Path,
) -> Option<String> {
    let rel = file.strip_prefix(vault_root).ok()?;
    let rel = rel.with_extension("");
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core uri::`
Expected: PASS (all `uri::` tests).

- [ ] **Step 5: Format + clippy**

Run (from `src-tauri/`): `cargo fmt --check` and `cargo clippy -p vault_buddy_core --all-targets -- -D warnings`
Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/uri.rs
git commit -m "feat(core): add vault_relative_no_ext for obsidian open URIs"
```

---

### Task 4: `open_transcript` command (`capture_commands.rs` + `lib.rs`)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (add the command; extend the `use vault_buddy_core::{...}` on line 10)
- Modify: `src-tauri/src/lib.rs` (register the command in `generate_handler!`)

**Interfaces:**
- Consumes: `uri::vault_relative_no_ext` (Task 3), `uri::open_file_uri`, `uri::launch`, `transcript::transcript_path`, `discovery::discover_vaults` (all in `vault_buddy_core`).
- Produces: Tauri command `open_transcript(path: String) -> Result<(), String>`. Task 5's store invokes `"open_transcript"` with `{ path }`.

**No Linux unit test:** the shell crate compiles on Windows only. The pure URI logic is covered by Task 3; this is a thin wrapper. Verify with `cargo fmt --check`; CI's `windows-app` job is the compile gate.

- [ ] **Step 1: Extend the core import**

In `src-tauri/src/capture_commands.rs`, change line 10 to add `transcript` and `uri`:

```rust
use vault_buddy_core::{capture_config, capture_paths, discovery, transcript, uri};
```

- [ ] **Step 2: Add the command**

In `src-tauri/src/capture_commands.rs`, add near the other `#[tauri::command]` functions:

```rust
/// Open a finished recording's note (or its transcript sidecar) in Obsidian.
/// Given the recording's `.mp3` path, resolve the owning vault and launch an
/// `obsidian://open` URI for the companion note `<base>.md` when it exists (it
/// embeds the transcript and the audio player — the richest view), otherwise
/// the `<base>.transcript.md` sidecar. Read-only: never writes into the vault;
/// the launch is logged by `uri::launch`, the same audit trail as every other
/// vault open.
#[tauri::command]
pub fn open_transcript(path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| mp3.starts_with(&v.path))
        .ok_or_else(|| format!("no vault owns {path}"))?;
    let note = mp3.with_extension("md");
    let target = if note.exists() {
        note
    } else {
        transcript::transcript_path(&mp3)
    };
    let rel = uri::vault_relative_no_ext(&target, Path::new(&vault.path))
        .ok_or_else(|| format!("recording is outside its vault: {}", target.display()))?;
    uri::launch(&uri::open_file_uri(&vault.id, &rel))
}
```

- [ ] **Step 3: Register the command**

In `src-tauri/src/lib.rs`, add `capture_commands::open_transcript,` to the `tauri::generate_handler![...]` list (next to the other `capture_commands::` entries, e.g. right after `capture_commands::transcribe_recording_now,`).

- [ ] **Step 4: Format check (shell can't compile on Linux)**

Run (from `src-tauri/`): `cargo fmt --check`
Expected: exit 0. (Compilation is verified by CI's `windows-app` job.)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): open a finished transcript's note in Obsidian"
```

---

### Task 5: Store flag + "Open in Obsidian" row (`capture.ts` + `TranscriptionStatus.vue`)

**Files:**
- Modify: `src/stores/capture.ts`
- Modify: `src/components/TranscriptionStatus.vue`
- Test: `tests/capture-store.test.ts`, `tests/transcription-status.test.ts`

**Interfaces:**
- Consumes: `open_transcript` command (Task 4); the `capture:transcribed` event `{ mp3, transcript }`.
- Produces: `capture` store gains `lastTranscribed: { mp3: string } | null` and `openTranscript()`.

- [ ] **Step 1: Write the failing store tests (RED)**

In `tests/capture-store.test.ts`, add:

```ts
  it("transcribed event records the file for the Open action", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/m.mp3", transcript: "/v/m.transcript.md" },
    });
    expect(store.lastTranscribed).toEqual({ mp3: "/v/m.mp3" });
  });

  it("a new recording clears the last transcribed marker", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") return { recording: true, vaultId: "v2", startedAtMs: 9 };
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/old.mp3" };
    await store.start("v2");
    expect(store.lastTranscribed).toBeNull();
  });

  it("openTranscript invokes open_transcript with the recording path", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    await store.openTranscript();
    expect(calls).toContainEqual({ cmd: "open_transcript", args: { path: "/v/m.mp3" } });
  });
```

- [ ] **Step 2: Run to verify they fail**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: FAIL — `lastTranscribed` / `openTranscript` don't exist.

- [ ] **Step 3: Add store state, event wiring, clear, and action**

In `src/stores/capture.ts`:

(a) Add to `state`, next to `lastSaved`:

```ts
    /** Most recent finished transcription; drives the "Open in Obsidian" row. */
    lastTranscribed: null as { mp3: string } | null,
```

(b) Set it in the `capture:transcribed` listener:

```ts
      await listen<CaptureTranscribed>("capture:transcribed", (event) => {
        this.transcribing = false;
        this.modelDownload = null;
        this.lastTranscribed = { mp3: event.payload.mp3 };
      });
```

(c) In `start()`, clear it where `dismissRename()` is already called:

```ts
      // New recording: the previous save's rename window is over.
      this.dismissRename();
      this.lastTranscribed = null;
```

(d) Add the action (e.g. after `retryTranscription`):

```ts
    async openTranscript() {
      if (!this.lastTranscribed) return;
      try {
        await invoke("open_transcript", { path: this.lastTranscribed.mp3 });
      } catch (e) {
        // A failed open (recording moved, launch error) is non-fatal — warn.
        this.warning = String(e);
        logWarning(`open transcript rejected: ${String(e)}`);
      }
    },
```

- [ ] **Step 4: Run store tests to verify they pass**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: PASS.

- [ ] **Step 5: Write the failing status-component test (RED)**

In `tests/transcription-status.test.ts`, add:

```ts
  it("offers an Open in Obsidian button after a transcription finishes", () => {
    const store = useCaptureStore();
    store.lastTranscribed = { mp3: "/v/m.mp3" };
    const w = mount(TranscriptionStatus);
    const btn = w.get('[data-testid="open-transcript"]');
    expect(btn.text()).toContain("Open in Obsidian");
  });
```

- [ ] **Step 6: Run to verify it fails**

Run: `npx vitest run tests/transcription-status.test.ts`
Expected: FAIL — `Unable to get [data-testid="open-transcript"]`.

- [ ] **Step 7: Add the completion row to the component**

In `src/components/TranscriptionStatus.vue`, replace the whole `<template>` with (root `v-if` gains `lastTranscribed`; the error branch becomes `v-else-if`; a new emerald "done" branch is added):

```html
<template>
  <div v-if="capture.transcribing || capture.transcriptError || capture.lastTranscribed">
    <div
      v-if="capture.transcribing"
      class="rounded-lg bg-violet-500/15 px-2 py-1.5 text-xs text-violet-100"
      role="status"
    >
      <span v-if="capture.modelDownload">
        Downloading {{ capture.modelDownload.model }} model<span v-if="downloadPct !== null">
          — {{ downloadPct }}%</span
        >…
      </span>
      <span v-else>Transcribing…</span>
    </div>
    <div
      v-else-if="capture.transcriptError"
      class="flex items-center justify-between gap-2 rounded-lg bg-red-500/20 px-2 py-1.5 text-xs text-red-200"
    >
      <span>Transcription failed: {{ capture.transcriptError }}</span>
      <button
        v-if="capture.transcriptFailedMp3"
        type="button"
        class="cursor-pointer rounded bg-red-500/80 px-2 py-0.5 font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300"
        @click="capture.retryTranscription()"
      >
        Retry
      </button>
    </div>
    <div
      v-else
      class="flex items-center justify-between gap-2 rounded-lg bg-emerald-500/15 px-2 py-1.5 text-xs text-emerald-100"
      role="status"
    >
      <span>✓ Transcribed</span>
      <button
        type="button"
        data-testid="open-transcript"
        class="cursor-pointer rounded bg-emerald-500/80 px-2 py-0.5 font-semibold text-white hover:bg-emerald-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
        @click="capture.openTranscript()"
      >
        Open in Obsidian
      </button>
    </div>
  </div>
</template>
```

- [ ] **Step 8: Run to verify it passes + full suite + typecheck**

Run: `npx vitest run tests/transcription-status.test.ts tests/capture-store.test.ts`
Expected: PASS.
Run: `npm test` and `npm run build`
Expected: both green.

- [ ] **Step 9: Commit**

```bash
git add src/stores/capture.ts src/components/TranscriptionStatus.vue tests/capture-store.test.ts tests/transcription-status.test.ts
git commit -m "feat(ui): offer Open in Obsidian when a transcript finishes"
```

---

## Self-Review

**Spec coverage:**
- §1 translate fix → Task 1. ✅
- §2 language dropdown (curated list, config mapping unchanged, backend permissive) → Task 2 (no `set_capture_config` change → backend stays permissive). ✅
- §3 open-after-finish (manual button, `lastTranscribed`, `open_transcript` command, note-or-sidecar, core URI helper, error surface) → Tasks 3–5. ✅
- Invariants (obsidian:// delegation + logged, no vault write, address by ID) → Task 4 uses `uri::open_file_uri` + `uri::launch`, read-only. ✅
- Non-goals (no auto-open, no translate feature, no backend lockdown) → honored (manual button; `set_translate(false)`; no config validation added). ✅

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** `vault_relative_no_ext(&Path, &Path) -> Option<String>` defined in Task 3, consumed in Task 4. `open_transcript(path: String)` defined in Task 4, invoked as `{ path }` in Task 5. `lastTranscribed: { mp3: string } | null` and `openTranscript()` defined and used consistently in Task 5. `data-testid="transcription-language-select"` / `"open-transcript"` match between component and tests. ✅
