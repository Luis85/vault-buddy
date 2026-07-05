# Recordings Enhancements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the record chooser a panel view, replace the misleading cog header button with a proper parent-targeted back button, and add a per-row re-transcribe button backed by an explicit forced re-transcription.

**Architecture:** Backend adds one core primitive (`force_write_sidecar`) + a `transcript_status` classifier + a `force` flag threaded config→job→`transcribe_recording`, exposed as a `retranscribe` command; frontend converts `RecordModeDialog` into a `RecordMode` view, adds a store `back()` action and header back button, and enriches each recordings row.

**Tech Stack:** Rust (`vault_buddy_core`, `vault_buddy_transcribe` — Linux-tested; `vault-buddy` shell — Windows-only compile, CI gate), Vue 3 + Pinia + Tailwind, Vitest.

## Global Constraints

- **Only the transcript sidecar is force-written.** `force_write_sidecar` overwrites **only** `<base>.transcript.md`; the audio `.mp3` and companion `.md` note are never touched. The auto/recovery transcription paths keep the idempotent `write_placeholder` + never-clobber `replace_if_ours`.
- **Re-transcribe is an explicit opt-in:** the `retranscribe` command's forced job **bypasses the vault's `transcribe` setting** and **regenerates even a `complete` sidecar**; the frontend gates the `complete` case behind a "Replace the current transcript?" confirm.
- **Back targets (fixed parent, no history stack):** `recordings` → Record view; `recordMode`/`captureSettings`/`settings` → Vaults.
- **Config-read never blocks recording:** `RecordMode.vue` falls back to `meeting` on a config-read error.
- **Core/transcribe test on Linux; the shell compiles on Windows only (CI `windows-app` gate).** Commits: `feat(core)`, `feat(transcribe)`, `feat(shell)`, `feat(ui)`.
- **Push bracket:** Task 2 (`RecordingEntry` field) and Task 3 (`transcribe_recording` signature) break the Windows-only shell's literals/call until Task 4 restores them. **Push Task 1 alone; HOLD Tasks 2 & 3; push 2+3+4 together after Task 4.** Frontend Tasks 5–8 each keep `npm run build` green — push each.
- **Commands** — Rust (from `src-tauri/`): `cargo test -p vault_buddy_core`, `cargo test -p vault_buddy_transcribe`, `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -- -D warnings`. Frontend (repo root): `npx vitest run tests/<file>`, `npm test`, `npm run build`.

---

### Task 1: `transcript_status` + `force_write_sidecar` (core)

**Files:**
- Modify: `src-tauri/core/src/transcript.rs` (add `TranscriptStatus` + `transcript_status`; refactor `replace_if_ours`'s write into a shared `write_sidecar_atomic`; add `force_write_sidecar`; tests)

**Interfaces:**
- Produces: `pub enum TranscriptStatus { Missing, Pending, Failed, Complete }` with `pub fn as_dto_str(&self) -> &'static str`; `pub fn transcript_status(mp3: &Path) -> TranscriptStatus`; `pub fn force_write_sidecar(transcript_path: &Path, content: &str) -> std::io::Result<()>`.
- Consumes: existing `transcript_path`, `MARKER_PENDING`/`MARKER_FAILED`, `NOTE_TMP_SUFFIX`, `render_placeholder`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/core/src/transcript.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn transcript_status_classifies_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Missing);
        std::fs::write(transcript_path(&mp3), render_placeholder("x.mp3")).unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Pending);
        std::fs::write(transcript_path(&mp3), render_error("x.mp3", "boom")).unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Failed);
        // A finished sidecar (complete marker) — or any non-regenerable content.
        std::fs::write(transcript_path(&mp3), "---\nvault-buddy-transcript: complete\n---\nhi").unwrap();
        assert_eq!(transcript_status(&mp3), TranscriptStatus::Complete);
        assert_eq!(TranscriptStatus::Missing.as_dto_str(), "none");
        assert_eq!(TranscriptStatus::Complete.as_dto_str(), "complete");
    }

    #[test]
    fn force_write_sidecar_overwrites_a_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("2026-07-04 1405 Meeting.transcript.md");
        std::fs::write(&path, "---\nvault-buddy-transcript: complete\n---\nold").unwrap();
        // replace_if_ours refuses (never-clobbers a finished transcript)...
        assert!(matches!(
            replace_if_ours(&path, "new").unwrap(),
            ReplaceOutcome::SkippedForeign
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "---\nvault-buddy-transcript: complete\n---\nold");
        // ...but force does overwrite, and cleans its temp.
        force_write_sidecar(&path, "regenerated").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "regenerated");
        let temps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(temps.is_empty(), "temp not cleaned: {temps:?}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core transcript_status force_write_sidecar`
Expected: FAIL to compile — `TranscriptStatus`/`transcript_status`/`force_write_sidecar` undefined.

- [ ] **Step 3: Implement**

In `src-tauri/core/src/transcript.rs`:

(a) Add the status type + classifier (place after `is_regenerable`):

```rust
/// The state of a recording's transcript sidecar, for the Recordings list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptStatus {
    Missing,
    Pending,
    Failed,
    Complete,
}

impl TranscriptStatus {
    /// Lowercased wire form for the frontend (`Missing` → "none").
    pub fn as_dto_str(&self) -> &'static str {
        match self {
            TranscriptStatus::Missing => "none",
            TranscriptStatus::Pending => "pending",
            TranscriptStatus::Failed => "failed",
            TranscriptStatus::Complete => "complete",
        }
    }
}

/// Classify a recording's sidecar. A non-regenerable file (the `complete`
/// marker, or a user's hand-edit) reads as `Complete` so the re-transcribe
/// confirm fires before it is overwritten. Unreadable → `Missing` (best-effort).
pub fn transcript_status(mp3: &Path) -> TranscriptStatus {
    match std::fs::read_to_string(transcript_path(mp3)) {
        Ok(c) if c.contains(MARKER_PENDING) => TranscriptStatus::Pending,
        Ok(c) if c.contains(MARKER_FAILED) => TranscriptStatus::Failed,
        Ok(_) => TranscriptStatus::Complete,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => TranscriptStatus::Missing,
        Err(_) => TranscriptStatus::Missing,
    }
}
```

(b) Refactor `replace_if_ours`: replace its body from `let dir = …` through the end with a call to a new shared helper, and add `force_write_sidecar`. The final `replace_if_ours` + new functions read:

```rust
pub fn replace_if_ours(transcript_path: &Path, content: &str) -> std::io::Result<ReplaceOutcome> {
    match std::fs::read_to_string(transcript_path) {
        Ok(existing) if !is_regenerable(&existing) => return Ok(ReplaceOutcome::SkippedForeign),
        Ok(_) => {}                                              // our placeholder/error — safe
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // fine, create it
        Err(e) => return Err(e),
    }
    write_sidecar_atomic(transcript_path, content).map(|()| ReplaceOutcome::Written)
}

/// Forced atomic overwrite of a transcript sidecar, skipping the never-clobber
/// guard. ONLY for the explicit `retranscribe` command — the user asked to
/// regenerate this sidecar. Still touches nothing but the sidecar.
pub fn force_write_sidecar(transcript_path: &Path, content: &str) -> std::io::Result<()> {
    write_sidecar_atomic(transcript_path, content)
}

/// The atomic temp + fsync + REPLACING-rename shared by `replace_if_ours` and
/// `force_write_sidecar`. Exclusive-creates a marker-suffixed temp (numbered on
/// collision) so recovery's cleanup can sweep it; mirrors capture_note's writer
/// deliberately so the never-replace audio writer is untouched.
fn write_sidecar_atomic(transcript_path: &Path, content: &str) -> std::io::Result<()> {
    let dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = transcript_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (tmp, mut f) = {
        let mut attempt = 0u32;
        loop {
            let candidate = if attempt == 0 {
                dir.join(format!(".{file_name}{NOTE_TMP_SUFFIX}"))
            } else {
                dir.join(format!(".{file_name}.{attempt}{NOTE_TMP_SUFFIX}"))
            };
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(f) => break (candidate, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(e) => return Err(e),
            }
        }
    };
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    drop(f);
    let result = std::fs::rename(&tmp, transcript_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core transcript`
Expected: PASS (new tests + all existing transcript tests, since `replace_if_ours`'s behavior is unchanged).

- [ ] **Step 5: Format, clippy, commit, PUSH**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/transcript.rs
git commit -m "feat(core): transcript_status classifier and force_write_sidecar

Adds a sidecar-state classifier for the Recordings list and a forced atomic
overwrite (shared with replace_if_ours via write_sidecar_atomic) for the
explicit re-transcribe path. force_write_sidecar touches only the transcript
sidecar; the auto/recovery paths keep never-clobber replace_if_ours."
git push origin claude/vault-buddy-local-stt-rktgl2
```

(Task 1 is push-safe — no shell-consumed signature changed.)

---

### Task 2: `RecordingEntry.transcript_status` (core) — HOLD PUSH

**Files:**
- Modify: `src-tauri/core/src/recordings.rs` (add the field; `entry_for` reads it; tests)

**Interfaces:**
- Consumes: `transcript::{TranscriptStatus, transcript_status}` (Task 1).
- Produces: `RecordingEntry.transcript_status: TranscriptStatus`.

- [ ] **Step 1: Write the failing test**

In `src-tauri/core/src/recordings.rs`, inside `#[cfg(test)] mod tests`, add (uses the existing `write_recording` helper):

```rust
    #[test]
    fn reports_transcript_status_per_recording() {
        use crate::transcript::{transcript_path, TranscriptStatus};
        let root = tempfile::tempdir().unwrap();
        write_recording(root.path(), "2026", "07", "2026-07-04 1405 Done", Some("Meeting"));
        write_recording(root.path(), "2026", "07", "2026-07-04 1400 Raw", Some("Meeting"));
        // Give the newer one a finished sidecar.
        let done_mp3 = root.path().join("2026").join("07").join("2026-07-04 1405 Done.mp3");
        std::fs::write(
            transcript_path(&done_mp3),
            "---\nvault-buddy-transcript: complete\n---\nhi",
        )
        .unwrap();
        let list = list_recordings(&[root.path().to_path_buf()]);
        // Newest-first: "1405 Done" then "1400 Raw".
        assert_eq!(list[0].transcript_status, TranscriptStatus::Complete);
        assert_eq!(list[1].transcript_status, TranscriptStatus::Missing);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core recordings::`
Expected: FAIL to compile — no field `transcript_status`.

- [ ] **Step 3: Implement**

In `src-tauri/core/src/recordings.rs`:

(a) Import + struct field. Change the import line and add the field after `recording_type`:

```rust
use crate::transcript::{dir_entries, is_digit_dir, transcript_status, TranscriptStatus};
```
```rust
    pub recording_type: Option<String>,
    /// State of the `<base>.transcript.md` sidecar (drives the row indicator
    /// and the re-transcribe confirm).
    pub transcript_status: TranscriptStatus,
}
```

(b) In `entry_for`, set it (after computing `duration`/`recording_type`):

```rust
    RecordingEntry {
        mp3_path: mp3_path.to_path_buf(),
        title,
        recorded_at,
        duration,
        recording_type,
        transcript_status: transcript_status(mp3_path),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core recordings::`
Expected: PASS. (Note: the `vault-buddy` shell crate's `RecordingDto` map now lacks the field and won't compile on Windows — restored in Task 4. Core compiles.)

- [ ] **Step 5: Format, clippy, commit — DO NOT PUSH**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/recordings.rs
git commit -m "feat(core): report transcript status per recording

RecordingEntry carries the sidecar state so the Recordings list can show a
per-row indicator and decide whether re-transcribe needs a replace confirm."
```

Push is held (breaks the Windows-only shell's `RecordingDto` map until Task 4).

---

### Task 3: `transcribe_recording(..., force)` (transcribe) — HOLD PUSH

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs` (add the `force` param; branch the final write; extend a test)

**Interfaces:**
- Consumes: `transcript::{force_write_sidecar, replace_if_ours, ReplaceOutcome}` (Task 1).
- Produces: `transcribe_recording(mp3, transcriber, opts, generated_at, force: bool) -> Result<PathBuf, String>`.

- [ ] **Step 1: Write the failing test**

In `src-tauri/transcribe/src/lib.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn force_regenerates_a_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        write_mp3(&mp3);
        let path = transcript_path(&mp3);
        std::fs::write(&path, "---\nvault-buddy-transcript: complete\n---\nOLD").unwrap();
        // Without force, a complete transcript is left untouched...
        transcribe_recording(&mp3, &FakeOk, &opts(), "t", false).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("OLD"));
        // ...with force, it is regenerated.
        transcribe_recording(&mp3, &FakeOk, &opts(), "t", true).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("OLD"));
        assert!(text.contains("hello world"));
    }
```

Also update the existing calls in this test module — `transcribe_recording(&mp3, &FakeOk, &opts(), "…")` becomes `(…, false)` (three existing call sites: `transcribe_writes_the_sidecar`, `engine_error_leaves_no_complete_transcript`, `decode_error_leaves_no_transcript`).

- [ ] **Step 2: Run the test to verify it fails**

Run (from `src-tauri/`): `cargo test -p vault_buddy_transcribe`
Expected: FAIL to compile — `transcribe_recording` takes 4 args, calls pass 5.

- [ ] **Step 3: Implement**

In `src-tauri/transcribe/src/lib.rs`, change the signature and the final write. Signature:

```rust
pub fn transcribe_recording(
    mp3: &Path,
    transcriber: &dyn Transcriber,
    opts: &TranscribeOptions,
    generated_at: &str,
    force: bool,
) -> Result<PathBuf, String> {
```

Replace the tail (`let content = …` through the `match … ReplaceOutcome …` block) with:

```rust
    let content = transcript::render_transcript(&meta, &segments);
    let path = transcript::transcript_path(mp3);
    if force {
        // Explicit re-transcribe: overwrite even a finished sidecar.
        transcript::force_write_sidecar(&path, &content)
            .map_err(|e| format!("write transcript: {e}"))?;
    } else {
        match transcript::replace_if_ours(&path, &content)
            .map_err(|e| format!("write transcript: {e}"))?
        {
            transcript::ReplaceOutcome::Written => {}
            transcript::ReplaceOutcome::SkippedForeign => {
                log::warn!(
                    "transcribe: left an existing non-regenerable sidecar untouched (not overwritten): {}",
                    path.display()
                );
            }
        }
    }
    Ok(path)
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_transcribe`
Expected: PASS (the new force test + the three updated existing tests).

- [ ] **Step 5: Format, clippy, commit — DO NOT PUSH**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings && cd ..
git add src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): force flag to regenerate a finished transcript

transcribe_recording(force=true) writes via force_write_sidecar so an
explicit re-transcribe overwrites a complete sidecar; force=false keeps the
never-clobber replace_if_ours behavior."
```

Push is held (the shell's `transcribe_recording(...)` call is now short one arg until Task 4).

---

### Task 4: `retranscribe` command + force wiring (shell) — restores compile, PUSH 2+3+4

**Windows-only compile — `cargo fmt --check` locally; CI `windows-app` is the gate.**

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`TranscriptionJob.force`; force branch in `process_transcription`; `retranscribe` command; `RecordingDto.transcript_status`; update every `TranscriptionJob {…}` literal and the `transcribe_recording(...)` call)
- Modify: `src-tauri/src/lib.rs` (register `retranscribe`)

**Interfaces:**
- Consumes: `recordings::RecordingEntry.transcript_status` (Task 2), `transcribe_recording(..., force)` (Task 3), `transcript::{force_write_sidecar, transcript_path, render_placeholder, write_placeholder}` (Task 1).
- Produces: IPC command `retranscribe(path: String) -> Result<(), String>`; `RecordingDto.transcript_status` (camelCase `transcriptStatus`).

- [ ] **Step 1: `TranscriptionJob.force` + update every literal**

In `src-tauri/src/capture_commands.rs`, add the field:

```rust
#[derive(Clone)]
struct TranscriptionJob {
    mp3: PathBuf,
    vault_id: String,
    force: bool,
}
```

Then add `force: false,` to every existing `TranscriptionJob { … }` literal (the auto paths): in `maybe_enqueue_transcription`, in `run_recovery`, in `scan_and_enqueue`, and in `transcribe_recording_now`. (Grep `TranscriptionJob {` to find all four; each currently sets `mp3` + `vault_id` only.)

- [ ] **Step 2: Force branch in `process_transcription`**

In `process_transcription`, change the gate and the placeholder, and pass `force` to `transcribe_recording`:

Gate (was `if !cfg.transcribe { return; }`):
```rust
    // A forced (explicit) re-transcribe ignores the vault's auto-transcribe
    // setting; the automatic path still bails when disabled.
    if !cfg.transcribe && !job.force {
        return;
    }
```

Placeholder (was `let _ = vault_buddy_core::transcript::write_placeholder(&job.mp3);`):
```rust
    if job.force {
        // Overwrite a finished sidecar with the "transcribing…" placeholder so
        // the note embed reflects the in-flight regeneration.
        let name = job
            .mp3
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let _ = vault_buddy_core::transcript::force_write_sidecar(
            &vault_buddy_core::transcript::transcript_path(&job.mp3),
            &vault_buddy_core::transcript::render_placeholder(&name),
        );
    } else {
        let _ = vault_buddy_core::transcript::write_placeholder(&job.mp3);
    }
```

Final call (was `transcribe_recording(&job.mp3, transcriber, &opts, &generated_at)`):
```rust
    match transcribe_recording(&job.mp3, transcriber, &opts, &generated_at, job.force) {
```

- [ ] **Step 3: The `retranscribe` command**

In `src-tauri/src/capture_commands.rs`, after `transcribe_recording_now`, add:

```rust
/// Explicit, forced re-transcription of a specific recording: regenerates even
/// a finished transcript and ignores the vault's auto-transcribe setting.
#[tauri::command]
pub fn retranscribe(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    if !mp3.is_file() {
        return Err("Recording not found.".to_string());
    }
    let vault_id = owning_vault_id(&mp3).ok_or("Recording is not inside a known vault.")?;
    enqueue_transcription(
        &app,
        TranscriptionJob {
            mp3,
            vault_id,
            force: true,
        },
    );
    Ok(())
}
```

- [ ] **Step 4: `RecordingDto.transcript_status`**

Add the field to `RecordingDto` (after `recording_type` — mind the existing `#[serde(rename = "type")]` on that field):

```rust
    #[serde(rename = "type")]
    pub recording_type: Option<String>,
    pub transcript_status: String,
```

And in the `list_recordings` command's `.map(|e| RecordingDto { … })`, add:

```rust
            recording_type: e.recording_type,
            transcript_status: e.transcript_status.as_dto_str().to_string(),
```

- [ ] **Step 5: Register the command**

In `src-tauri/src/lib.rs`'s `generate_handler![…]`, add after `capture_commands::transcribe_recording_now,`:

```rust
            capture_commands::retranscribe,
```

- [ ] **Step 6: Format check + commit, then PUSH 2+3+4**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): retranscribe command + force wiring; transcript status DTO

TranscriptionJob carries force; process_transcription force-overwrites the
placeholder + skips the auto-transcribe gate + passes force to
transcribe_recording. New retranscribe(path) command enqueues a forced job.
RecordingDto reports transcriptStatus. Windows compile via CI."
git push origin claude/vault-buddy-local-stt-rktgl2
```

Now the pushed HEAD's shell compiles again (Tasks 2, 3, 4 land together). Watch the `windows-app` CI check.

---

### Task 5: `recordMode` view state, `back()`, and the `Recording` type (frontend store)

**Files:**
- Modify: `src/stores/vaults.ts` (`recordMode` view, `recordModeVaultId`, `openRecordMode`, `back()`; clear in `showList`)
- Modify: `src/types.ts` (`Recording.transcriptStatus`)
- Modify: `tests/vaults-store.test.ts` (tests)

**Interfaces:**
- Produces: store `view` gains `"recordMode"`; `recordModeVaultId: string | null`; `openRecordMode(vaultId)`; `back()`; `Recording.transcriptStatus: "none" | "pending" | "failed" | "complete"`.
- Consumed by: Tasks 6–8.

- [ ] **Step 1: Write the failing store tests**

In `tests/vaults-store.test.ts`, add:

```ts
  it("openRecordMode switches to the record view for a vault", () => {
    const store = useVaultsStore();
    store.openRecordMode("a1b2c3");
    expect(store.view).toBe("recordMode");
    expect(store.recordModeVaultId).toBe("a1b2c3");
  });

  it("back() returns each view to its parent", () => {
    const store = useVaultsStore();
    // recordings' parent is the record view (same vault)
    store.openRecordings("a1b2c3");
    store.back();
    expect(store.view).toBe("recordMode");
    expect(store.recordModeVaultId).toBe("a1b2c3");
    // record view's parent is the list
    store.back();
    expect(store.view).toBe("list");
    // capture settings' parent is the list
    store.openCaptureSettings("a1b2c3");
    store.back();
    expect(store.view).toBe("list");
  });
```

- [ ] **Step 2: Run to verify fail**

Run (repo root): `npx vitest run tests/vaults-store.test.ts`
Expected: FAIL — `openRecordMode`/`back` not functions.

- [ ] **Step 3: Implement the store**

In `src/stores/vaults.ts`:

(a) Widen the `view` union and add the id (after `recordingsVaultId`):
```ts
    view: "list" as
      | "list"
      | "settings"
      | "captureSettings"
      | "recordings"
      | "recordMode",
    captureSettingsVaultId: null as string | null,
    recordingsVaultId: null as string | null,
    recordModeVaultId: null as string | null,
```

(b) Clear it in `showList`:
```ts
    showList() {
      this.view = "list";
      this.captureSettingsVaultId = null;
      this.recordingsVaultId = null;
      this.recordModeVaultId = null;
    },
```

(c) Add the actions (after `openRecordings`):
```ts
    openRecordMode(vaultId: string) {
      this.view = "recordMode";
      this.recordModeVaultId = vaultId;
    },
    /** Back to the current view's fixed parent (no history stack). */
    back() {
      if (this.view === "recordings" && this.recordingsVaultId) {
        this.openRecordMode(this.recordingsVaultId);
      } else {
        this.showList();
      }
    },
```

- [ ] **Step 4: Add the `Recording.transcriptStatus` type**

In `src/types.ts`, add to the `Recording` interface (after `type`):
```ts
  type: string | null;
  /** Sidecar state — drives the row indicator + re-transcribe confirm. */
  transcriptStatus: "none" | "pending" | "failed" | "complete";
```

- [ ] **Step 5: Run to verify pass**

Run (repo root): `npx vitest run tests/vaults-store.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit + push**

```bash
git add src/stores/vaults.ts src/types.ts tests/vaults-store.test.ts
git commit -m "feat(ui): record-mode view state, back() action, transcriptStatus type"
git push origin claude/vault-buddy-local-stt-rktgl2
```

---

### Task 6: `RecordMode.vue` panel view (frontend)

**Files:**
- Create: `src/components/RecordMode.vue`
- Create: `tests/record-mode.test.ts`

**Interfaces:**
- Consumes: `get_capture_config` IPC (mocked); `useCaptureStore().start`; `useVaultsStore().openRecordings`.
- Produces: `<RecordMode :vault-id="…" />` — used by Task 7.

- [ ] **Step 1: Write the failing tests**

Create `tests/record-mode.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import RecordMode from "../src/components/RecordMode.vue";
import { useVaultsStore } from "../src/stores/vaults";
import { useCaptureStore } from "../src/stores/capture";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const mountView = async (mode: "meeting" | "voice-note" = "meeting") => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return { mode /* other fields unused here */ };
    if (cmd === "start_capture") return { recording: true, vaultId: "v1", startedAtMs: 1, paused: false, pausedTotalMs: 0, pausedSinceMs: null };
  });
  const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("RecordMode", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("highlights the vault's default mode", async () => {
    const { wrapper } = await mountView("voice-note");
    expect(wrapper.get('[data-testid="mode-voice-note"]').classes()).toContain("border-violet-400");
    expect(wrapper.get('[data-testid="mode-meeting"]').classes()).not.toContain("border-violet-400");
  });

  it("starts a recording and returns to the list", async () => {
    const { wrapper, calls } = await mountView("meeting");
    const store = useVaultsStore();
    store.openRecordMode("v1");
    await wrapper.get('[data-testid="mode-voice-note"]').trigger("click");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "start_capture")).toBe(true);
    expect(store.view).toBe("list");
  });

  it("navigates to recordings on Browse", async () => {
    const { wrapper } = await mountView("meeting");
    const store = useVaultsStore();
    await wrapper.get('[data-testid="mode-browse"]').trigger("click");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("v1");
  });

  it("falls back to meeting when the config read fails", async () => {
    clearMocks();
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") throw new Error("nope");
    });
    const wrapper = mount(RecordMode, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(wrapper.get('[data-testid="mode-meeting"]').classes()).toContain("border-violet-400");
  });
});
```

- [ ] **Step 2: Run to verify fail**

Run (repo root): `npx vitest run tests/record-mode.test.ts`
Expected: FAIL — cannot resolve `RecordMode.vue`.

- [ ] **Step 3: Implement `RecordMode.vue`**

Create `src/components/RecordMode.vue`:

```vue
<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useVaultsStore } from "../stores/vaults";
import { useCaptureStore } from "../stores/capture";
import type { CaptureConfig } from "../types";

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();
const capture = useCaptureStore();

const OPTIONS = [
  { key: "meeting", title: "Meeting", hint: "Microphone + desktop audio", testId: "mode-meeting" },
  { key: "voice-note", title: "Voice Note", hint: "Microphone only", testId: "mode-voice-note" },
] as const;

const defaultMode = ref<"meeting" | "voice-note">("meeting");

onMounted(async () => {
  // The chooser needs the vault's DEFAULT mode; a config read failure must
  // never block recording — fall back to meeting.
  try {
    const cfg = await invoke<CaptureConfig>("get_capture_config", { id: props.vaultId });
    defaultMode.value = cfg.mode;
  } catch {
    // stale config never blocks recording — mirror the backend's rule
  }
});

function start(mode: "meeting" | "voice-note") {
  void capture.start(props.vaultId, mode);
  store.showList(); // recording bar shows on the list view
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <button
      v-for="option in OPTIONS"
      :key="option.key"
      type="button"
      :data-testid="option.testId"
      :aria-label="`Start a ${option.title.toLowerCase()} recording`"
      class="w-full cursor-pointer rounded-lg border px-3 py-2 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      :class="
        option.key === defaultMode
          ? 'border-violet-400 bg-violet-500/20'
          : 'border-white/10 bg-white/5 hover:bg-white/10'
      "
      @click="start(option.key)"
    >
      <span class="block text-sm font-medium text-slate-100">{{ option.title }}</span>
      <span class="block text-xs text-slate-400">{{ option.hint }}</span>
    </button>
    <button
      type="button"
      data-testid="mode-browse"
      aria-label="Browse past recordings"
      class="mt-1 w-full cursor-pointer border-t border-white/10 pt-2 text-left text-xs text-slate-400 transition-colors hover:text-slate-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      @click="store.openRecordings(props.vaultId)"
    >
      Browse recordings…
      <span class="block text-slate-500">See past recordings in this vault</span>
    </button>
  </div>
</template>
```

- [ ] **Step 4: Run to verify pass**

Run (repo root): `npx vitest run tests/record-mode.test.ts`
Expected: PASS (all four).

- [ ] **Step 5: Commit + push**

```bash
git add src/components/RecordMode.vue tests/record-mode.test.ts
git commit -m "feat(ui): RecordMode panel view (Meeting / Voice Note / Browse)"
git push origin claude/vault-buddy-local-stt-rktgl2
```

(`RecordModeDialog.vue` is still present and used by `ActionPanel` — it's removed in Task 7.)

---

### Task 7: Header back button + record-mode wiring; remove the modal (frontend)

**Files:**
- Modify: `src/components/ActionPanel.vue` (header back button; `recordMode` slot + title; capture → `openRecordMode`; remove `RecordModeDialog` import, modal, and `recordRequest`/`openRecordDialog`/`startWithMode`/`browseRecordings`)
- Delete: `src/components/RecordModeDialog.vue`, `tests/record-mode-dialog.test.ts`
- Modify: `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `store.{openRecordMode, back, recordModeVaultId}` (Task 5), `RecordMode.vue` (Task 6).

- [ ] **Step 1: Write the failing tests**

In `tests/action-panel.test.ts`, replace the existing "navigates to the recordings view from the record dialog" test (which drove the old modal) with record-view + back-button tests:

```ts
  it("opens the record view from a vault's capture button", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") return { mode: "meeting" };
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper.get('[aria-label="Capture knowledge in Personal"]').trigger("click");
    expect(store.view).toBe("recordMode");
    expect(store.recordModeVaultId).toBe("d4e5f6");
  });

  it("shows a back button in non-list views that returns to the parent", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_recordings") return [];
      if (cmd === "get_capture_config") return { mode: "meeting" };
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.openRecordings("d4e5f6");
    const wrapper = mount(ActionPanel);
    await flushPromises();
    // no cog in a non-list view; a back button instead
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(false);
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(store.view).toBe("recordMode"); // recordings → record view
  });
```

(Keep the existing "renders the Recordings view with its title" test; it still holds.)

- [ ] **Step 2: Run to verify fail**

Run (repo root): `npx vitest run tests/action-panel.test.ts`
Expected: FAIL — capture opens the old modal, no `back-button`.

- [ ] **Step 3: Implement `ActionPanel.vue`**

(a) Imports — replace the `RecordModeDialog` import with `RecordMode`:
```ts
import RecordMode from "./RecordMode.vue";
```
(remove `import RecordModeDialog from "./RecordModeDialog.vue";`)

(b) Remove the modal machinery from `<script setup>`: delete `recordRequest`, `openRecordDialog`, `startWithMode`, and `browseRecordings` (and the now-unused `invoke`/`CaptureConfig` imports if nothing else uses them — `invoke` is unused after this; `CaptureConfig` too. Remove both imports).

(c) Header — split the single toggle into a list-view cog and a non-list back button. Replace the header title block's trailing ternary to add the record title, and replace the single `<button data-testid="settings-toggle">` with two conditionals. The header title:
```vue
        {{
          view === "settings"
            ? "Buddy settings"
            : view === "captureSettings"
              ? "Capture settings"
              : view === "recordings"
                ? "Recordings"
                : view === "recordMode"
                  ? "Record"
                  : "Vaults"
        }}
```
The header right-side controls — replace the existing `<button data-testid="settings-toggle" …>…</button>` with:
```vue
        <button
          v-if="view === 'list'"
          type="button"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          aria-label="Buddy settings"
          title="Buddy settings"
          data-testid="settings-toggle"
          @click="store.openSettings()"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <circle cx="12" cy="12" r="3" />
            <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.09a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.09a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.09a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
          </svg>
        </button>
        <button
          v-else
          type="button"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          aria-label="Back"
          title="Back"
          data-testid="back-button"
          @click="store.back()"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M19 12H5M12 19l-7-7 7-7" />
          </svg>
        </button>
```

(d) Add the `recordMode` view slot — after the `recordings` slot's closing `</div>` and before the `<div v-else>` VaultList block:
```vue
    <div
      v-else-if="view === 'recordMode' && store.recordModeVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <RecordMode
        :key="store.recordModeVaultId"
        :vault-id="store.recordModeVaultId"
      />
    </div>
```

(e) The vault-row capture handler — change `@capture="openRecordDialog($event)"` to:
```vue
        @capture="store.openRecordMode($event)"
```

(f) Remove the `<RecordModeDialog … />` element at the end of the template entirely.

- [ ] **Step 4: Delete the old modal + its test**

```bash
git rm src/components/RecordModeDialog.vue tests/record-mode-dialog.test.ts
```

- [ ] **Step 5: Run the suite + build**

Run (repo root): `npx vitest run tests/action-panel.test.ts` → PASS. Then `npm test && npm run build` → full suite green, vue-tsc clean (confirms no dangling `RecordModeDialog` import).

- [ ] **Step 6: Commit + push**

```bash
git add src/components/ActionPanel.vue tests/action-panel.test.ts
git commit -m "feat(ui): record chooser as a panel view + proper back button

The vault-row capture button opens a Record panel view instead of a modal;
non-list views get a ← back button (parent-targeted via store.back()) and the
cog is now list-only. Removes RecordModeDialog."
git push origin claude/vault-buddy-local-stt-rktgl2
```

---

### Task 8: Per-row transcript status + re-transcribe button (frontend)

**Files:**
- Modify: `src/components/Recordings.vue` (status indicator + re-transcribe button + confirm-on-complete + transient status via capture events)
- Modify: `tests/recordings.test.ts`

**Interfaces:**
- Consumes: `Recording.transcriptStatus` (Task 5), `retranscribe` IPC (Task 4), the `capture:transcribing/transcribed/transcribeFailed` events.

- [ ] **Step 1: Write the failing tests**

In `tests/recordings.test.ts`, extend the `sample` rows with `transcriptStatus` and add tests. First, add `transcriptStatus` to each sample row (e.g. the three existing rows get `"complete"`, `"none"`, `"failed"` respectively). Then add:

```ts
  it("re-transcribes immediately for a non-complete transcript", async () => {
    const { wrapper, calls } = await mountView();
    // sample[1] "Idea" has transcriptStatus "none" — its row's retranscribe button
    const rows = wrapper.findAll('[data-testid="recording-row"]');
    // find the row for sample[1] by its retranscribe button and click it
    await wrapper.findAll('[data-testid="retranscribe"]')[1].trigger("click");
    await flushPromises();
    const rt = calls.find((c) => c.cmd === "retranscribe");
    expect(rt).toBeTruthy();
    // no confirm shown for a non-complete transcript
    expect(wrapper.find('[data-testid="retranscribe-confirm"]').exists()).toBe(false);
    void rows;
  });

  it("confirms before re-transcribing a complete transcript", async () => {
    const { wrapper, calls } = await mountView();
    // sample[0] "Standup" has transcriptStatus "complete"
    await wrapper.findAll('[data-testid="retranscribe"]')[0].trigger("click");
    // no invoke yet — a confirm is shown
    expect(calls.some((c) => c.cmd === "retranscribe")).toBe(false);
    await wrapper.get('[data-testid="retranscribe-confirm"]').trigger("click");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "retranscribe")).toBe(true);
  });
```

(Adjust the `mountView` mock to accept `retranscribe`: add `if (cmd === "retranscribe") return null;` to its `mockIPC`.)

- [ ] **Step 2: Run to verify fail**

Run (repo root): `npx vitest run tests/recordings.test.ts`
Expected: FAIL — no `retranscribe` button/confirm.

- [ ] **Step 3: Implement `Recordings.vue`**

(a) `<script setup>` — add imports, transient state, a confirm target, and the re-transcribe handler. After the existing `openError` ref, add:

```ts
import { listen } from "@tauri-apps/api/event";
```
```ts
// mp3 currently being (re)transcribed → row shows a spinner. Seeded on click
// and by capture:transcribing; cleared by transcribed/transcribeFailed.
const transcribingMp3 = ref<Set<string>>(new Set());
// mp3 awaiting a "replace the current transcript?" confirm (complete only).
const confirmMp3 = ref<string | null>(null);

function statusLabel(r: Recording): string {
  if (transcribingMp3.value.has(r.mp3)) return "Transcribing…";
  return { none: "", pending: "Transcribing…", failed: "Transcript failed", complete: "Transcribed ✓" }[r.transcriptStatus];
}

async function runRetranscribe(mp3: string) {
  confirmMp3.value = null;
  transcribingMp3.value = new Set(transcribingMp3.value).add(mp3);
  try {
    await invoke("retranscribe", { path: mp3 });
  } catch (e) {
    transcribingMp3.value = new Set([...transcribingMp3.value].filter((m) => m !== mp3));
    openError.value = String(e);
    logWarning(`retranscribe rejected: ${String(e)}`);
  }
}

function onRetranscribeClick(r: Recording) {
  // A finished (or hand-edited) transcript needs a confirm before we clobber it.
  if (r.transcriptStatus === "complete") confirmMp3.value = r.mp3;
  else void runRetranscribe(r.mp3);
}

function clearTranscribing(mp3: string) {
  transcribingMp3.value = new Set([...transcribingMp3.value].filter((m) => m !== mp3));
}

onMounted(async () => {
  await listen<{ mp3: string }>("capture:transcribing", (e) => {
    transcribingMp3.value = new Set(transcribingMp3.value).add(e.payload.mp3);
  });
  await listen<{ mp3: string }>("capture:transcribed", (e) => {
    clearTranscribing(e.payload.mp3);
    const row = recordings.value.find((r) => r.mp3 === e.payload.mp3);
    if (row) row.transcriptStatus = "complete";
  });
  await listen<{ mp3: string }>("capture:transcribeFailed", (e) => {
    clearTranscribing(e.payload.mp3);
    const row = recordings.value.find((r) => r.mp3 === e.payload.mp3);
    if (row) row.transcriptStatus = "failed";
  });
});
```

Note: keep the EXISTING `onMounted` that fetches `list_recordings` — Vue runs multiple `onMounted` hooks in order, so add this as a SECOND `onMounted` (or fold the `listen` calls into the existing one after the fetch). Simplest: fold the three `listen` calls into the existing `onMounted`, after the `finally` block completes — i.e. put them right before the closing of the existing `onMounted`'s `try/finally` is not possible; instead register them first, then fetch. Reorder the existing `onMounted` to register listeners then fetch:

```ts
onMounted(async () => {
  await listen<{ mp3: string }>("capture:transcribing", (e) => {
    transcribingMp3.value = new Set(transcribingMp3.value).add(e.payload.mp3);
  });
  await listen<{ mp3: string }>("capture:transcribed", (e) => {
    clearTranscribing(e.payload.mp3);
    const row = recordings.value.find((r) => r.mp3 === e.payload.mp3);
    if (row) row.transcriptStatus = "complete";
  });
  await listen<{ mp3: string }>("capture:transcribeFailed", (e) => {
    clearTranscribing(e.payload.mp3);
    const row = recordings.value.find((r) => r.mp3 === e.payload.mp3);
    if (row) row.transcriptStatus = "failed";
  });
  try {
    recordings.value = await invoke<Recording[]>("list_recordings", { id: props.vaultId });
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});
```

(b) Template — the row `<button>` currently wraps the whole row and calls `open`. Split it: keep the open-row as a button, and add a **status label** + a **re-transcribe button** as siblings inside a row container. Replace the row `<button …>…</button>` (the `data-testid="recording-row"` element) with:

```vue
        <div
          v-for="r in section.items"
          :key="r.mp3"
          class="flex items-center gap-1"
        >
          <button
            type="button"
            data-testid="recording-row"
            class="flex min-w-0 flex-1 items-baseline justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
            @click="open(r.mp3)"
          >
            <span class="min-w-0 flex-1 truncate text-sm text-slate-100" :title="r.title">
              {{ r.title }}
            </span>
            <span class="shrink-0 text-xs text-slate-400">{{ r.recordedAt }}</span>
            <span class="shrink-0 text-xs text-slate-500">{{ r.duration ?? "—" }}</span>
          </button>
          <span
            v-if="statusLabel(r)"
            class="shrink-0 text-[10px] text-slate-500"
            :title="statusLabel(r)"
          >{{ transcribingMp3.has(r.mp3) || r.transcriptStatus === "pending" ? "…" : r.transcriptStatus === "failed" ? "⚠" : "✓" }}</span>
          <button
            type="button"
            data-testid="retranscribe"
            :disabled="transcribingMp3.has(r.mp3)"
            :aria-label="`Re-transcribe ${r.title}`"
            title="Re-transcribe"
            class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
            @click="onRetranscribeClick(r)"
          >
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M23 4v6h-6M1 20v-6h6" />
              <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
            </svg>
          </button>
        </div>
```

(c) Template — the confirm. Add, inside the populated `<div v-else …>` block right after the `openError` banner:

```vue
      <div
        v-if="confirmMp3"
        data-testid="retranscribe-confirm-row"
        class="flex items-center justify-between gap-2 rounded-lg border border-amber-400/30 bg-amber-500/10 px-2 py-1 text-xs text-amber-100"
      >
        <span>Replace the current transcript?</span>
        <span class="flex gap-1">
          <button
            type="button"
            data-testid="retranscribe-confirm"
            class="cursor-pointer rounded bg-amber-500/30 px-2 py-0.5 hover:bg-amber-500/40 focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-300"
            @click="runRetranscribe(confirmMp3)"
          >Replace</button>
          <button
            type="button"
            data-testid="retranscribe-cancel"
            class="cursor-pointer rounded bg-white/10 px-2 py-0.5 hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
            @click="confirmMp3 = null"
          >Cancel</button>
        </span>
      </div>
```

- [ ] **Step 4: Run the target test, then the suite + build**

Run (repo root): `npx vitest run tests/recordings.test.ts` → PASS. Then `npm test && npm run build` → full suite green, vue-tsc clean.

- [ ] **Step 5: Commit + push**

```bash
git add src/components/Recordings.vue tests/recordings.test.ts
git commit -m "feat(ui): per-row transcript status + re-transcribe button

Each recording row shows its sidecar status and a re-transcribe button:
force-regenerates immediately for none/failed, behind a Replace confirm for a
complete transcript. Transient transcribing state tracks the capture events."
git push origin claude/vault-buddy-local-stt-rktgl2
```

---

## Self-Review

**Spec coverage:**
- Part 1 (record chooser → view): Task 5 (`recordMode` state), Task 6 (`RecordMode.vue`), Task 7 (ActionPanel wiring + remove modal). ✅
- Part 2 (proper back button): Task 5 (`back()`), Task 7 (header ← / cog split, parent targets). ✅
- Part 3 (re-transcribe): Task 1 (`force_write_sidecar`, `transcript_status`), Task 2 (`RecordingEntry.transcript_status`), Task 3 (`transcribe_recording(force)`), Task 4 (`retranscribe` command + DTO + force wiring), Task 5 (`Recording.transcriptStatus`), Task 8 (row status + button + confirm + transient). ✅
- Force touches only the sidecar; auto/recovery unchanged → Task 1 (`force_write_sidecar` vs `replace_if_ours`), Task 4 (force branch, auto literals `force:false`). ✅
- Bypass gate + confirm-on-complete → Task 4 (`!cfg.transcribe && !job.force`), Task 8 (`transcriptStatus === 'complete'` → confirm). ✅
- Config-read never blocks recording → Task 6 (`catch` → meeting). ✅

**Placeholder scan:** none — every step has concrete code/commands.

**Type consistency:** `TranscriptStatus` + `as_dto_str` (T1) → `RecordingEntry.transcript_status` (T2) → `RecordingDto.transcript_status`/`transcriptStatus` (T4) → `Recording.transcriptStatus` TS (T5) → `Recordings.vue` (T8). `transcribe_recording(…, force)` (T3) called with `job.force` (T4). `force`/`TranscriptionJob.force` (T4) consistent across literals. `retranscribe` command (T4) ↔ `invoke("retranscribe", { path })` (T8). Store `openRecordMode`/`back`/`recordModeVaultId` (T5) ↔ ActionPanel (T7) ↔ RecordMode (T6). `data-testid` hooks (`mode-browse`, `back-button`, `retranscribe`, `retranscribe-confirm`) consistent between tests and templates. ✅
