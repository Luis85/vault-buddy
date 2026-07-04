# Local Speech-to-Text Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Transcribe each finished recording to text fully on-device (no cloud, no API) and surface it inline in the meeting note via an embedded transcript sidecar.

**Architecture:** A new pure-logic module `vault_buddy_core::transcript` renders/writes the transcript sidecar with the same never-clobber/atomic discipline as the audio note. A new `vault_buddy_transcribe` crate decodes our MP3 to 16 kHz mono PCM (Symphonia) and runs whisper.cpp via `whisper-rs` (static-linked, behind a `whisper` Cargo feature) behind a `Transcriber` trait. The Windows-only shell adds a per-vault opt-in config, writes a "transcribing…" placeholder at save time, and runs a bounded background worker (modelled on `run_recovery`) that decodes → transcribes → atomically replaces the placeholder, resuming after a crash via a `YYYY/MM` scan.

**Tech Stack:** Rust (workspace crates), Tauri v2, `whisper-rs` (whisper.cpp), `symphonia` (MP3 decode), `ureq` + `sha2` (model download), Vue 3 + Pinia + Vitest.

## Global Constraints

Every task's requirements implicitly include this section. Values are copied verbatim from `docs/superpowers/specs/2026-07-04-increment-3-local-speech-to-text-design.md`.

- **No network at transcription time.** The only network access is a one-time model download from Hugging Face `ggerganov/whisper.cpp`. No cloud service, no API key.
- **Never clobber a user's file.** All vault writes: exclusive-create dot-prefixed owned temps carrying `NOTE_TMP_SUFFIX` (`.vault-buddy.tmp`), `fsync`, then move; never `std::fs::rename` onto a name we don't own. The audio note is written once at finalize and **never reopened**.
- **Transcript naming.** Sidecar is `<base>.transcript.md`; the note embeds `![[<base>.transcript]]` under a `## Transcript` heading. Names are reserved **pairwise** with `.mp3`/`.md`/`.mp3.part` via the shared `candidate()` suffix scheme.
- **Ownership marker.** Every generated sidecar carries a frontmatter field `vault-buddy-transcript: pending | failed | complete`. Only `pending`/`failed` sidecars (or a missing one) may be (re)written; `complete` or a user-edited file is never overwritten.
- **Model default `small`; transcription opt-in, default off** (per vault).
- **Segment type (canonical):** `vault_buddy_core::transcript::Segment { start_ms: u64, end_ms: u64, text: String }`.
- **whisper.cpp is static-linked; no runtime DLL.** The `whisper-rs` binding lives behind the transcribe crate's `whisper` feature (off by default) so Linux CI builds/tests the crate without compiling whisper.cpp. The **`windows-app` CI job is the compile gate** for the real engine.
- **Toolchain:** Node 22, Rust stable. `cargo fmt --check` (whole `src-tauri` workspace) and `cargo clippy --all-targets -- -D warnings` must pass. Conventional Commits.
- **Failure is best-effort:** a transcription/download failure must never harm the saved MP3 or note; it degrades to a retryable `failed` sidecar + a `capture:transcribeFailed` toast, and is `log::info!`-logged.

---

## File Structure

**Create:**
- `src-tauri/core/src/transcript.rs` — pure: render placeholder/error/real transcript markdown, marker detection, sidecar path derivation, atomic write / replace-if-ours, `YYYY/MM` pending scan.
- `src-tauri/transcribe/Cargo.toml` — new crate `vault_buddy_transcribe`.
- `src-tauri/transcribe/src/lib.rs` — `Transcriber` trait, `TranscribeOptions`, `transcribe_recording` orchestration.
- `src-tauri/transcribe/src/decode.rs` — Symphonia MP3 → 16 kHz mono f32 + linear resample.
- `src-tauri/transcribe/src/model.rs` — model tier registry + `%APPDATA%` path resolution + `ureq` download.
- `src-tauri/transcribe/src/engine.rs` — `WhisperTranscriber` (behind `whisper` feature).
- `docs/superpowers/specs/2026-07-04-increment-3-windows-verification.md` — manual Windows checklist.

**Modify:**
- `src-tauri/core/src/lib.rs` — register `pub mod transcript;`.
- `src-tauri/core/src/capture_note.rs` — `NoteMeta.transcribe` field; render the `## Transcript` embed; make `yaml_quote` `pub(crate)`.
- `src-tauri/core/src/capture_paths.rs` — reserve the transcript name pairwise (`reserve_names`, `reserve_final`, `CaptureNames.transcript_md`).
- `src-tauri/core/src/capture_config.rs` — four new `VaultCaptureConfig` fields + parsing.
- `src-tauri/capture/src/session.rs` — `SessionParams.transcribe`; set `NoteMeta.transcribe`.
- `src-tauri/capture/src/recovery.rs` — `recover_root(..., transcribe)`; set `NoteMeta.transcribe`.
- `src-tauri/src/capture_commands.rs` — wire `cfg.transcribe`; placeholder+enqueue in the monitor thread; `TranscriptionState`; `run_transcription` worker; `transcribe_recording_now` command; pass `v.transcribe` to `recover_root`.
- `src-tauri/src/lib.rs` — manage `TranscriptionState`, register the command, call `run_transcription`.
- `src-tauri/Cargo.toml` — add `transcribe` workspace member + `vault_buddy_transcribe` dependency (with `whisper` feature).
- `src/types.ts`, `src/stores/capture.ts` — transcription events/state.
- `src/components/…` (the recording panel) — a minimal "transcribing…" indicator.
- `.github/workflows/ci.yml` — extend `rust-core` to cover the transcribe crate.
- `docs/DEVELOPMENT.md` — document the new config fields.

---

## Phase A — `vault_buddy_core::transcript` (pure logic, Linux-tested)

### Task 1: Transcript markdown rendering + marker

**Files:**
- Create: `src-tauri/core/src/transcript.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod transcript;`), `src-tauri/core/src/capture_note.rs` (`fn yaml_quote` → `pub(crate) fn yaml_quote`)
- Test: inline `#[cfg(test)]` in `transcript.rs`

**Interfaces:**
- Produces:
  - `pub struct Segment { pub start_ms: u64, pub end_ms: u64, pub text: String }`
  - `pub struct TranscriptMeta { pub mp3_file_name: String, pub model_label: String, pub language: Option<String>, pub duration_secs: u64, pub generated_at: String, pub timestamps: bool }`
  - `pub fn render_placeholder(mp3_file_name: &str) -> String`
  - `pub fn render_error(mp3_file_name: &str, message: &str) -> String`
  - `pub fn render_transcript(meta: &TranscriptMeta, segments: &[Segment]) -> String`
  - `pub fn is_regenerable(content: &str) -> bool`
  - `pub fn format_timestamp(ms: u64) -> String`

- [ ] **Step 1: Make `yaml_quote` reusable**

In `src-tauri/core/src/capture_note.rs`, change the signature (body unchanged):

```rust
pub(crate) fn yaml_quote(value: &str) -> String {
```

- [ ] **Step 2: Register the module**

In `src-tauri/core/src/lib.rs`, add to the module list (keep alphabetical):

```rust
pub mod transcript;
```

- [ ] **Step 3: Write the failing tests**

Create `src-tauri/core/src/transcript.rs` with only the tests first (types/functions unresolved → compile-fail is the "red"):

```rust
//! Transcript sidecar: a `<base>.transcript.md` beside the recording that
//! the meeting note embeds. Written with the same never-clobber/atomic
//! discipline as the audio note. A `vault-buddy-transcript` frontmatter
//! marker (pending/failed/complete) is how the worker tells its own
//! regenerable sidecars from a finished transcript or a user's edits.

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: u64, end_ms: u64, text: &str) -> Segment {
        Segment { start_ms, end_ms, text: text.into() }
    }

    fn meta() -> TranscriptMeta {
        TranscriptMeta {
            mp3_file_name: "2026-07-04 1405 Meeting.mp3".into(),
            model_label: "whisper-small".into(),
            language: Some("es".into()),
            duration_secs: 3723,
            generated_at: "2026-07-04T15:10:00+02:00".into(),
            timestamps: true,
        }
    }

    #[test]
    fn timestamp_is_hms() {
        assert_eq!(format_timestamp(0), "[00:00:00]");
        assert_eq!(format_timestamp(12_000), "[00:00:12]");
        assert_eq!(format_timestamp(3_723_000), "[01:02:03]");
    }

    #[test]
    fn placeholder_is_regenerable_and_names_the_audio() {
        let p = render_placeholder("2026-07-04 1405 Meeting.mp3");
        assert!(p.starts_with("---\n"));
        assert!(p.contains("vault-buddy-transcript: pending"));
        assert!(p.contains(r#"transcript-of: "2026-07-04 1405 Meeting.mp3""#));
        assert!(is_regenerable(&p));
    }

    #[test]
    fn error_is_regenerable_and_carries_message() {
        let e = render_error("x.mp3", "model download failed");
        assert!(e.contains("vault-buddy-transcript: failed"));
        assert!(e.contains("model download failed"));
        assert!(is_regenerable(&e));
    }

    #[test]
    fn real_transcript_is_complete_not_regenerable() {
        let t = render_transcript(&meta(), &[seg(0, 12_000, "Hola a todos")]);
        assert!(t.contains("vault-buddy-transcript: complete"));
        assert!(t.contains(r#"model: "whisper-small""#));
        assert!(t.contains(r#"language: "es""#));
        assert!(t.contains(r#"duration: "1:02:03""#));
        assert!(t.contains("[00:00:00] Hola a todos"));
        assert!(!is_regenerable(&t), "a finished transcript must never be overwritten");
    }

    #[test]
    fn timestamps_can_be_disabled() {
        let mut m = meta();
        m.timestamps = false;
        let t = render_transcript(&m, &[seg(0, 1000, "one"), seg(1000, 2000, "two")]);
        assert!(!t.contains("[00:00:00]"), "no timestamps when disabled");
        assert!(t.contains("one"));
        assert!(t.contains("two"));
    }

    #[test]
    fn language_none_renders_auto() {
        let mut m = meta();
        m.language = None;
        assert!(render_transcript(&m, &[]).contains(r#"language: "auto""#));
    }

    #[test]
    fn frontmatter_injection_is_escaped() {
        // mp3 name is derived from a filesystem name; a crafted name must
        // not break or inject frontmatter.
        let p = render_placeholder("evil\"\ninjected: true.mp3");
        assert!(!p.contains("\ninjected:"), "newline must not inject a field");
    }

    #[test]
    fn user_edited_sidecar_is_not_regenerable() {
        assert!(!is_regenerable("just some notes the user typed"));
    }
}
```

- [ ] **Step 4: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_core transcript::`
Expected: FAIL — `cannot find type Segment`, etc.

- [ ] **Step 5: Implement the rendering**

Prepend to `src-tauri/core/src/transcript.rs` (above the test module):

```rust
use crate::capture_note::{format_duration, yaml_quote};
use std::path::{Path, PathBuf};

/// Frontmatter marker line values. `pending`/`failed` sidecars are ours to
/// (re)write; `complete` — and any file without the marker — is left alone.
const MARKER_PENDING: &str = "vault-buddy-transcript: pending";
const MARKER_FAILED: &str = "vault-buddy-transcript: failed";
const MARKER_COMPLETE: &str = "vault-buddy-transcript: complete";

pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub struct TranscriptMeta {
    pub mp3_file_name: String,
    pub model_label: String,
    pub language: Option<String>,
    pub duration_secs: u64,
    pub generated_at: String,
    pub timestamps: bool,
}

/// `[HH:MM:SS]` — meetings can exceed an hour, so always render hours.
pub fn format_timestamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    format!("[{h:02}:{m:02}:{s:02}]")
}

/// A sidecar we may (re)write: our own not-yet-finished output. A finished
/// (`complete`) transcript or a file a user has taken over must never match.
pub fn is_regenerable(content: &str) -> bool {
    content.contains(MARKER_PENDING) || content.contains(MARKER_FAILED)
}

pub fn render_placeholder(mp3_file_name: &str) -> String {
    format!(
        "---\n{MARKER_PENDING}\ntranscript-of: {}\ncreated-by: Vault Buddy\n---\n\n*Transcribing…*\n",
        yaml_quote(mp3_file_name)
    )
}

pub fn render_error(mp3_file_name: &str, message: &str) -> String {
    // Message is flattened to one line so the callout can't be broken out of.
    let flat = message.replace(['\n', '\r'], " ");
    format!(
        "---\n{MARKER_FAILED}\ntranscript-of: {}\ncreated-by: Vault Buddy\n---\n\n\
         > [!warning] Transcription failed\n> {flat}\n>\n> This will be retried automatically.\n",
        yaml_quote(mp3_file_name)
    )
}

pub fn render_transcript(meta: &TranscriptMeta, segments: &[Segment]) -> String {
    let mut out = String::from("---\n");
    out.push_str(MARKER_COMPLETE);
    out.push('\n');
    out.push_str(&format!("transcript-of: {}\n", yaml_quote(&meta.mp3_file_name)));
    out.push_str(&format!("model: {}\n", yaml_quote(&meta.model_label)));
    let lang = meta.language.as_deref().unwrap_or("auto");
    out.push_str(&format!("language: {}\n", yaml_quote(lang)));
    out.push_str(&format!(
        "duration: {}\n",
        yaml_quote(&format_duration(meta.duration_secs))
    ));
    out.push_str(&format!("generated: {}\n", yaml_quote(&meta.generated_at)));
    out.push_str("created-by: Vault Buddy\n---\n\n");
    for s in segments {
        let text = s.text.trim();
        if text.is_empty() {
            continue;
        }
        if meta.timestamps {
            out.push_str(&format!("{} {text}\n\n", format_timestamp(s.start_ms)));
        } else {
            out.push_str(&format!("{text}\n\n"));
        }
    }
    out
}
```

- [ ] **Step 6: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_core transcript::`
Expected: PASS (7 tests).

- [ ] **Step 7: fmt + clippy**

Run: `cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/core/src/transcript.rs src-tauri/core/src/lib.rs src-tauri/core/src/capture_note.rs
git commit -m "feat(core): render transcript sidecar markdown with regenerable marker"
```

---

### Task 2: Transcript path derivation + atomic write / replace-if-ours

**Files:**
- Modify: `src-tauri/core/src/transcript.rs`
- Test: inline

**Interfaces:**
- Consumes: `crate::capture_note::{write_note_atomic, NOTE_TMP_SUFFIX}`, `crate::capture_paths::rename_noreplace`
- Produces:
  - `pub fn transcript_file_name(mp3_file_name: &str) -> String`
  - `pub fn transcript_path(mp3: &Path) -> PathBuf`
  - `pub fn write_placeholder(mp3: &Path) -> std::io::Result<()>`
  - `pub enum ReplaceOutcome { Written, SkippedForeign }`
  - `pub fn replace_if_ours(transcript_path: &Path, content: &str) -> std::io::Result<ReplaceOutcome>`
  - `pub fn needs_transcription(mp3: &Path) -> bool`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `transcript.rs`:

```rust
    use std::path::Path;

    #[test]
    fn transcript_name_appends_transcript_before_md() {
        assert_eq!(
            transcript_file_name("2026-07-04 1405 Meeting.mp3"),
            "2026-07-04 1405 Meeting.transcript.md"
        );
    }

    #[test]
    fn transcript_path_sits_beside_the_mp3() {
        let p = transcript_path(Path::new("/v/Meetings/2026/07/b.mp3"));
        assert_eq!(p, Path::new("/v/Meetings/2026/07/b.transcript.md"));
    }

    #[test]
    fn write_placeholder_is_idempotent_and_needs_transcription_tracks_it() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        std::fs::write(&mp3, b"fake").unwrap();
        assert!(needs_transcription(&mp3), "no sidecar yet");
        write_placeholder(&mp3).unwrap();
        let side = transcript_path(&mp3);
        assert!(side.exists());
        assert!(needs_transcription(&mp3), "a placeholder still needs work");
        // second call must not error or clobber
        write_placeholder(&mp3).unwrap();
    }

    #[test]
    fn replace_overwrites_our_placeholder() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        std::fs::write(&mp3, b"fake").unwrap();
        write_placeholder(&mp3).unwrap();
        let side = transcript_path(&mp3);
        let real = render_transcript(&meta(), &[seg(0, 1000, "done")]);
        assert!(matches!(
            replace_if_ours(&side, &real).unwrap(),
            ReplaceOutcome::Written
        ));
        let text = std::fs::read_to_string(&side).unwrap();
        assert!(text.contains("vault-buddy-transcript: complete"));
        assert!(!needs_transcription(&mp3), "a complete transcript is done");
    }

    #[test]
    fn replace_never_touches_a_user_owned_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        let side = transcript_path(&mp3);
        std::fs::write(&side, "my own hand-written transcript").unwrap();
        assert!(matches!(
            replace_if_ours(&side, "generated").unwrap(),
            ReplaceOutcome::SkippedForeign
        ));
        assert_eq!(
            std::fs::read_to_string(&side).unwrap(),
            "my own hand-written transcript"
        );
    }

    #[test]
    fn replace_writes_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("b.mp3");
        let side = transcript_path(&mp3);
        assert!(matches!(
            replace_if_ours(&side, "generated").unwrap(),
            ReplaceOutcome::Written
        ));
        assert_eq!(std::fs::read_to_string(&side).unwrap(), "generated");
    }

    #[test]
    fn replace_leaves_no_temp_behind() {
        let dir = tempfile::tempdir().unwrap();
        let side = dir.path().join("b.transcript.md");
        replace_if_ours(&side, "generated").unwrap();
        let temps: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(temps.is_empty(), "temp not cleaned: {temps:?}");
    }
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_core transcript::`
Expected: FAIL — `cannot find function transcript_path`, etc.

- [ ] **Step 3: Implement path + write + replace**

Add to `transcript.rs` (above the tests):

```rust
use crate::capture_note::{write_note_atomic, NOTE_TMP_SUFFIX};
use std::io::Write;

pub fn transcript_file_name(mp3_file_name: &str) -> String {
    let stem = mp3_file_name.strip_suffix(".mp3").unwrap_or(mp3_file_name);
    format!("{stem}.transcript.md")
}

pub fn transcript_path(mp3: &Path) -> PathBuf {
    let dir = mp3.parent().unwrap_or_else(|| Path::new("."));
    let name = mp3.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dir.join(transcript_file_name(&name))
}

/// Missing sidecar, or one of our own not-yet-finished sidecars → work to do.
pub fn needs_transcription(mp3: &Path) -> bool {
    let path = transcript_path(mp3);
    match std::fs::read_to_string(&path) {
        Ok(content) => is_regenerable(&content),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        // Unreadable (permissions/AV lock): don't spin on it this pass.
        Err(_) => false,
    }
}

/// Create the "transcribing…" placeholder so the note's embed never shows
/// "file not found". Idempotent: an existing sidecar (placeholder or real)
/// is left untouched — the reserved name means the exclusive-create wins on
/// the common path.
pub fn write_placeholder(mp3: &Path) -> std::io::Result<()> {
    let path = transcript_path(mp3);
    if path.exists() {
        return Ok(());
    }
    let name = mp3.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match write_note_atomic(&path, &render_placeholder(&name)) {
        Ok(()) => Ok(()),
        // Raced by a concurrent writer — the sidecar exists, which is all we wanted.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

pub enum ReplaceOutcome {
    Written,
    SkippedForeign,
}

/// Atomically replace one of OUR regenerable sidecars (or write a missing
/// one). A finished transcript or a user-edited file is never overwritten.
/// Unlike the audio note, replacing here is intentional — but only ever our
/// own `pending`/`failed` output, verified before the move.
pub fn replace_if_ours(transcript_path: &Path, content: &str) -> std::io::Result<ReplaceOutcome> {
    match std::fs::read_to_string(transcript_path) {
        Ok(existing) if !is_regenerable(&existing) => return Ok(ReplaceOutcome::SkippedForeign),
        Ok(_) => {}                                              // our placeholder/error — safe
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {} // fine, create it
        Err(e) => return Err(e),
    }
    let dir = transcript_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = transcript_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Exclusive-create an owned temp (numbered on collision), carrying the
    // ownership marker so recovery's cleanup can sweep it. Mirrors
    // capture_note::write_note_atomic deliberately — kept separate so the
    // audio-note writer (which must NEVER replace) is not touched.
    let (tmp, mut f) = {
        let mut attempt = 0u32;
        loop {
            let candidate = if attempt == 0 {
                dir.join(format!(".{file_name}{NOTE_TMP_SUFFIX}"))
            } else {
                dir.join(format!(".{file_name}.{attempt}{NOTE_TMP_SUFFIX}"))
            };
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
                Ok(f) => break (candidate, f),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
                Err(e) => return Err(e),
            }
        }
    };
    f.write_all(content.as_bytes())?;
    f.sync_all()?;
    drop(f);
    // Replacing rename is correct here: we verified the destination is our
    // own regenerable sidecar (or absent) above.
    let result = std::fs::rename(&tmp, transcript_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.map(|()| ReplaceOutcome::Written)
}
```

- [ ] **Step 4: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_core transcript::`
Expected: PASS.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings
cd .. && git add src-tauri/core/src/transcript.rs
git commit -m "feat(core): atomic transcript sidecar write and replace-if-ours"
```

---

### Task 3: Pending-transcription scan over `YYYY/MM`

**Files:**
- Modify: `src-tauri/core/src/transcript.rs`
- Test: inline

**Interfaces:**
- Consumes: `crate::capture_paths::{base_from_part, is_capture_base}` (for the capture-name gate)
- Produces: `pub fn pending_transcriptions(root: &Path) -> Vec<PathBuf>`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    fn month_dir(root: &Path) -> std::path::PathBuf {
        let d = root.join("2026").join("07");
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn scan_finds_capture_mp3_without_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        let mp3 = month.join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&mp3, b"audio").unwrap();
        let pending = pending_transcriptions(dir.path());
        assert_eq!(pending, vec![mp3]);
    }

    #[test]
    fn scan_skips_completed_and_ignores_foreign_and_placeholders_are_pending() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        // completed → skipped
        let done = month.join("2026-07-04 1405 Meeting.mp3");
        std::fs::write(&done, b"audio").unwrap();
        std::fs::write(transcript_path(&done), render_transcript(&meta(), &[])).unwrap();
        // placeholder → still pending
        let pend = month.join("2026-07-04 1406 Meeting.mp3");
        std::fs::write(&pend, b"audio").unwrap();
        write_placeholder(&pend).unwrap();
        // foreign (not a capture base) → ignored
        std::fs::write(month.join("random.mp3"), b"audio").unwrap();

        let pending = pending_transcriptions(dir.path());
        assert_eq!(pending, vec![pend]);
    }

    #[test]
    fn scan_ignores_non_dated_and_root_level_files() {
        let dir = tempfile::tempdir().unwrap();
        // capture-named mp3 directly at the root (not under YYYY/MM)
        std::fs::write(dir.path().join("2026-07-04 1405 Meeting.mp3"), b"a").unwrap();
        let sub = dir.path().join("Project");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("2026-07-04 1405 Meeting.mp3"), b"a").unwrap();
        assert!(pending_transcriptions(dir.path()).is_empty());
    }
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_core transcript::`
Expected: FAIL — `cannot find function pending_transcriptions`.

- [ ] **Step 3: Implement the scan**

Add to `transcript.rs`. The `YYYY/MM`-only walk and symlink-safe `file_type()` mirror `capture::recovery::walk`, kept here so it stays in the pure, Linux-tested crate.

```rust
use crate::capture_paths::is_capture_base;

/// Capture MP3s under `<root>/YYYY/MM` that still need a transcript (missing
/// or one of our regenerable sidecars). Same layout discipline as recovery:
/// only `YYYY/MM`, only capture-named files, never follows symlinks.
pub fn pending_transcriptions(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (year, yft, yname) in dir_entries(root) {
        if !yft.is_dir() || !is_digit_dir(&yname, 4) {
            continue;
        }
        for (month, mft, mname) in dir_entries(&year) {
            if !mft.is_dir() || !is_digit_dir(&mname, 2) {
                continue;
            }
            for (path, fft, name) in dir_entries(&month) {
                if !fft.is_file() {
                    continue;
                }
                let Some(base) = name.strip_suffix(".mp3") else { continue };
                if !is_capture_base(base) {
                    continue;
                }
                if needs_transcription(&path) {
                    out.push(path);
                }
            }
        }
    }
    out
}

fn is_digit_dir(name: &str, len: usize) -> bool {
    name.len() == len && name.chars().all(|c| c.is_ascii_digit())
}

fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // file_type() reads the dirent WITHOUT following symlinks — a
            // symlinked dir/junction must never let the scan escape the vault.
            if let Ok(ft) = entry.file_type() {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.push((entry.path(), ft, name));
            }
        }
    }
    out
}
```

Note: `is_capture_base` is currently `pub` in `capture_paths` — it is, so no visibility change is needed.

- [ ] **Step 4: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_core transcript::`
Expected: PASS.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings
cd .. && git add src-tauri/core/src/transcript.rs
git commit -m "feat(core): scan dated folders for recordings needing transcription"
```

---

## Phase B — Pairwise reservation + note embed (pure logic in core)

### Task 4: Reserve the transcript name pairwise

**Files:**
- Modify: `src-tauri/core/src/capture_paths.rs`
- Test: inline (extend existing tests)

**Interfaces:**
- Produces: `CaptureNames.transcript_md: PathBuf`; `reserve_names`/`reserve_final` now also require `<base>.transcript.md` free.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `capture_paths.rs`:

```rust
    #[test]
    fn reserve_includes_transcript_name() {
        let dir = tempfile::tempdir().unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.transcript_md, dir.path().join("b.transcript.md"));
    }

    #[test]
    fn reserve_advances_when_transcript_exists() {
        let dir = tempfile::tempdir().unwrap();
        // a stray transcript sidecar for the plain base blocks it
        std::fs::write(dir.path().join("b.transcript.md"), "x").unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.base, "b (2)");
    }

    #[test]
    fn reserve_final_advances_past_existing_transcript() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.transcript.md"), "x").unwrap();
        let (mp3, _) = reserve_final(dir.path(), "b");
        assert_eq!(mp3, dir.path().join("b (2).mp3"));
    }
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_core capture_paths::`
Expected: FAIL — `no field transcript_md`, and the advance tests fail (transcript not yet checked).

- [ ] **Step 3: Implement**

In `capture_paths.rs`, add the field to `CaptureNames`:

```rust
pub struct CaptureNames {
    pub base: String,
    pub final_mp3: PathBuf,
    pub note_md: PathBuf,
    pub part: PathBuf,
    pub transcript_md: PathBuf,
}
```

Update `reserve_names` to reserve it pairwise:

```rust
pub fn reserve_names(dir: &Path, base: &str) -> CaptureNames {
    for attempt in 1.. {
        let b = candidate(base, attempt);
        let final_mp3 = dir.join(format!("{b}.mp3"));
        let note_md = dir.join(format!("{b}.md"));
        let transcript_md = dir.join(format!("{b}.transcript.md"));
        let part = dir.join(part_file_name(&b));
        if !final_mp3.exists()
            && !note_md.exists()
            && !transcript_md.exists()
            && !part.exists()
        {
            return CaptureNames {
                base: b,
                final_mp3,
                note_md,
                part,
                transcript_md,
            };
        }
    }
    unreachable!("suffix search always terminates")
}
```

Update `reserve_final` to also require the transcript name free (signature unchanged):

```rust
pub fn reserve_final(dir: &Path, base: &str) -> (PathBuf, PathBuf) {
    for attempt in 1.. {
        let b = candidate(base, attempt);
        let final_mp3 = dir.join(format!("{b}.mp3"));
        let note_md = dir.join(format!("{b}.md"));
        let transcript_md = dir.join(format!("{b}.transcript.md"));
        if !final_mp3.exists() && !note_md.exists() && !transcript_md.exists() {
            return (final_mp3, note_md);
        }
    }
    unreachable!("suffix search always terminates")
}
```

- [ ] **Step 4: Run to confirm pass (and existing paths still green)**

Run: `cd src-tauri && cargo test -p vault_buddy_core capture_paths::`
Expected: PASS — including the existing `reserve_*` tests.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings
cd .. && git add src-tauri/core/src/capture_paths.rs
git commit -m "feat(core): reserve the transcript sidecar name pairwise with mp3/md/part"
```

---

### Task 5: Embed the transcript in the audio note

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs`
- Test: inline (extend existing tests)

**Interfaces:**
- Produces: `NoteMeta.transcribe: bool`; `render_note` appends a `## Transcript` embed when `transcribe`.
- Consumed by: `capture::session` (Task 12) and `capture::recovery` (Task 13), which set `NoteMeta.transcribe`.

- [ ] **Step 1: Write/adjust the failing tests**

In `capture_note.rs`, add `transcribe: false` to the `meta()` test helper, and add two tests:

```rust
    // in fn meta() add the field:
    //     event: None,
    //     transcribe: false,

    #[test]
    fn note_embeds_transcript_when_enabled() {
        let mut m = meta();
        m.transcribe = true;
        let note = render_note(&m, "2026-07-04 1405 Meeting.mp3");
        assert!(note.contains("![[2026-07-04 1405 Meeting.mp3]]"), "audio embed stays");
        assert!(note.contains("## Transcript"));
        assert!(note.contains("![[2026-07-04 1405 Meeting.transcript]]"));
    }

    #[test]
    fn note_has_no_transcript_section_when_disabled() {
        let note = render_note(&meta(), "b.mp3");
        assert!(!note.contains("## Transcript"));
    }
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_core capture_note::`
Expected: FAIL — `missing field transcribe` in the `meta()` helper (and the new assertions).

- [ ] **Step 3: Implement**

Add the field to `NoteMeta`:

```rust
pub struct NoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    pub recording_type: String,
    pub input_devices: Vec<String>,
    pub event: Option<String>,
    pub transcribe: bool,
}
```

Append the transcript embed at the end of `render_note` (after the mp3 embed line, before `out`):

```rust
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
    if meta.transcribe {
        // The transcript sidecar's name is derived from the mp3 stem and was
        // reserved pairwise, so this embed resolves once the sidecar lands
        // (a "transcribing…" placeholder is written immediately so it never
        // shows "file not found").
        let stem = mp3_file_name.strip_suffix(".mp3").unwrap_or(mp3_file_name);
        out.push_str(&format!("\n## Transcript\n\n![[{stem}.transcript]]\n"));
    }
    out
```

- [ ] **Step 4: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_core capture_note::`
Expected: PASS.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings
cd .. && git add src-tauri/core/src/capture_note.rs
git commit -m "feat(core): embed the transcript sidecar in the audio note when enabled"
```

---

## Phase C — `vault_buddy_transcribe` crate

### Task 6: Crate scaffold + `Transcriber` trait

**Files:**
- Create: `src-tauri/transcribe/Cargo.toml`, `src-tauri/transcribe/src/lib.rs`
- Modify: `src-tauri/Cargo.toml` (workspace `members` + `resolver`)

**Interfaces:**
- Produces:
  - `pub trait Transcriber { fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<Vec<Segment>, String>; }`
  - `pub struct TranscribeOptions { pub language: Option<String>, pub timestamps: bool, pub model_label: String }`

- [ ] **Step 1: Add the workspace member + resolver**

Edit `src-tauri/Cargo.toml`, replacing the `[workspace]` table:

```toml
[workspace]
resolver = "2"
members = ["core", "capture", "transcribe"]
```

(`resolver = "2"` keeps the shell's future `whisper` feature from unifying into `cargo test -p vault_buddy_transcribe`.)

- [ ] **Step 2: Create the crate manifest**

Create `src-tauri/transcribe/Cargo.toml`:

```toml
[package]
name = "vault_buddy_transcribe"
version = "0.1.0"
edition = "2021"

[features]
default = []
# whisper.cpp is compiled and static-linked only with this feature on, so
# Linux CI can build/test the crate without a C++ toolchain. The shell
# enables it; the windows-app CI job is the compile gate.
whisper = ["dep:whisper-rs"]

[dependencies]
vault_buddy_core = { path = "../core" }
log = "0.4"
symphonia = { version = "0.5", default-features = false, features = ["mp3"] }
ureq = "2"
dirs = "6"
whisper-rs = { version = "0.16", optional = true }

[dev-dependencies]
mp3lame-encoder = "0.2"
tempfile = "3"
```

- [ ] **Step 3: Create the crate root**

Create `src-tauri/transcribe/src/lib.rs`:

```rust
//! Local speech-to-text: decode our MP3 to 16 kHz mono PCM and run
//! whisper.cpp (via whisper-rs, behind the `whisper` feature) behind a
//! `Transcriber` trait so orchestration is testable without a real model.

use vault_buddy_core::transcript::Segment;

/// A speech-to-text backend. `samples` are 16 kHz mono f32 in [-1, 1];
/// `language` is an ISO code (e.g. "es") or None to auto-detect.
pub trait Transcriber {
    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<Vec<Segment>, String>;
}

pub struct TranscribeOptions {
    pub language: Option<String>,
    pub timestamps: bool,
    pub model_label: String,
}
```

- [ ] **Step 4: Build the crate**

Run: `cd src-tauri && cargo build -p vault_buddy_transcribe`
Expected: compiles (downloads symphonia/ureq; no whisper.cpp because the `whisper` feature is off).

- [ ] **Step 5: fmt + commit**

```bash
cd src-tauri && cargo fmt
cd .. && git add src-tauri/transcribe/Cargo.toml src-tauri/transcribe/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(transcribe): scaffold vault_buddy_transcribe crate with Transcriber trait"
```

---

### Task 7: MP3 → 16 kHz mono decode

**Files:**
- Create: `src-tauri/transcribe/src/decode.rs`
- Modify: `src-tauri/transcribe/src/lib.rs` (add `pub mod decode;`)
- Test: inline

**Interfaces:**
- Produces: `pub const WHISPER_RATE: u32`; `pub fn decode_to_16k_mono(path: &Path) -> Result<Vec<f32>, String>`; `pub(crate) fn resample_linear(input, from, to)`

- [ ] **Step 1: Declare the module**

Add to `src-tauri/transcribe/src/lib.rs` (top):

```rust
pub mod decode;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/transcribe/src/decode.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm};

    fn make_mp3(rate: u32, secs: f32) -> Vec<u8> {
        let frames = (rate as f32 * secs) as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16;
            pcm.push(s);
            pcm.push(s);
        }
        let mut b = Builder::new().unwrap();
        b.set_num_channels(2).unwrap();
        b.set_sample_rate(rate).unwrap();
        b.set_brate(Bitrate::Kbps128).unwrap();
        b.set_quality(mp3lame_encoder::Quality::Good).unwrap();
        let mut enc = b.build().unwrap();
        let mut out = Vec::new();
        enc.encode_to_vec(InterleavedPcm(&pcm[..]), &mut out).unwrap();
        enc.flush_to_vec::<FlushNoGap>(&mut out).unwrap();
        out
    }

    #[test]
    fn decodes_mp3_to_16k_mono() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.mp3");
        std::fs::write(&path, make_mp3(44_100, 1.0)).unwrap();
        let pcm = decode_to_16k_mono(&path).unwrap();
        let secs = pcm.len() as f32 / WHISPER_RATE as f32;
        assert!((secs - 1.0).abs() < 0.25, "expected ~1s, got {secs}s ({} samples)", pcm.len());
        assert!(pcm.iter().any(|&s| s.abs() > 0.01), "decoded audio is not silent");
    }

    #[test]
    fn resample_preserves_duration_ratio() {
        let input: Vec<f32> = (0..44_100).map(|i| (i as f32 / 100.0).sin()).collect();
        let out = resample_linear(&input, 44_100, 16_000);
        let ratio = out.len() as f32 / input.len() as f32;
        assert!((ratio - 16_000.0 / 44_100.0).abs() < 0.01, "ratio {ratio}");
    }

    #[test]
    fn resample_is_identity_when_rates_match() {
        let input = vec![0.1f32, 0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
    }

    #[test]
    fn resample_handles_empty_input() {
        assert!(resample_linear(&[], 44_100, 16_000).is_empty());
    }
}
```

- [ ] **Step 3: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_transcribe decode::`
Expected: FAIL — `cannot find function decode_to_16k_mono`.

- [ ] **Step 4: Implement the decoder**

Prepend to `src-tauri/transcribe/src/decode.rs`:

```rust
//! Decode our MP3 recording into the exact PCM shape whisper.cpp expects:
//! 16 kHz, mono, f32 in [-1, 1]. Symphonia is pure Rust, so no ffmpeg
//! binary is bundled. Resampling is linear — adequate for 16 kHz speech and
//! fully testable; rubato is a future quality upgrade.

use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const WHISPER_RATE: u32 = 16_000;

pub fn decode_to_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe audio: {e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "no audio track in recording".to_string())?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("init decoder: {e}"))?;

    let mut src_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut mono: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("read packet: {e}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let spec = *decoded.spec();
                src_rate = spec.rate;
                let channels = spec.channels.count().max(1);
                let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                buf.copy_interleaved_ref(decoded);
                for frame in buf.samples().chunks(channels) {
                    let sum: f32 = frame.iter().copied().sum();
                    mono.push(sum / channels as f32);
                }
            }
            // One corrupt frame must not abandon a whole recording.
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(format!("decode audio: {e}")),
        }
    }
    Ok(resample_linear(&mono, src_rate, WHISPER_RATE))
}

pub(crate) fn resample_linear(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if input.is_empty() || from == 0 || from == to {
        return input.to_vec();
    }
    let ratio = to as f64 / from as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let last = input.len() - 1;
    for i in 0..out_len {
        let src = i as f64 / ratio;
        let idx = src.floor() as usize;
        let frac = (src - idx as f64) as f32;
        let a = input[idx.min(last)];
        let b = input[(idx + 1).min(last)];
        out.push(a + (b - a) * frac);
    }
    out
}
```

- [ ] **Step 5: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_transcribe decode::`
Expected: PASS. If `probe audio` errors, the `mp3` Symphonia feature isn't active — confirm `features = ["mp3"]` in the manifest.

- [ ] **Step 6: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings
cd .. && git add src-tauri/transcribe/src/decode.rs src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): decode MP3 to 16 kHz mono PCM for whisper"
```

---

### Task 8: Model tier registry + download

**Files:**
- Create: `src-tauri/transcribe/src/model.rs`
- Modify: `src-tauri/transcribe/src/lib.rs` (add `pub mod model;`)
- Test: inline (registry only; the network download is exercised manually)

**Interfaces:**
- Produces: `pub enum ModelTier { Base, Small, Medium }` with `from_str`/`as_str`/`label`/`file_name`/`url`/`min_size`; `pub fn model_dir()`, `pub fn model_path(tier)`, `pub fn download_model(tier, on_progress) -> Result<PathBuf, String>`.

- [ ] **Step 1: Declare the module**

Add to `src-tauri/transcribe/src/lib.rs`:

```rust
pub mod model;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/transcribe/src/model.rs` with the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_from_str_defaults_to_small() {
        assert_eq!(ModelTier::from_str("base"), ModelTier::Base);
        assert_eq!(ModelTier::from_str("medium"), ModelTier::Medium);
        assert_eq!(ModelTier::from_str("small"), ModelTier::Small);
        assert_eq!(ModelTier::from_str("garbage"), ModelTier::Small);
    }

    #[test]
    fn tier_files_urls_and_labels() {
        assert_eq!(ModelTier::Small.file_name(), "ggml-small.bin");
        assert!(ModelTier::Small.url().ends_with("/ggml-small.bin"));
        assert!(ModelTier::Small.url().starts_with("https://huggingface.co/ggerganov/whisper.cpp"));
        assert_eq!(ModelTier::Base.label(), "whisper-base");
        assert_eq!(ModelTier::Small.as_str(), "small");
    }

    #[test]
    fn model_path_ends_with_the_tier_file() {
        if let Some(p) = model_path(ModelTier::Small) {
            assert_eq!(p.file_name().unwrap().to_string_lossy(), "ggml-small.bin");
        }
    }
}
```

- [ ] **Step 3: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_transcribe model::`
Expected: FAIL — `cannot find type ModelTier`.

- [ ] **Step 4: Implement**

Prepend to `src-tauri/transcribe/src/model.rs`:

```rust
//! Whisper ggml model registry and on-disk cache. Models are downloaded on
//! first use (never bundled) from Hugging Face into %APPDATA%\vault-buddy\
//! models — the only network access in the app.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelTier {
    Base,
    Small,
    Medium,
}

impl ModelTier {
    pub fn from_str(s: &str) -> ModelTier {
        match s {
            "base" => ModelTier::Base,
            "medium" => ModelTier::Medium,
            _ => ModelTier::Small, // small is the default tier
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTier::Base => "base",
            ModelTier::Small => "small",
            ModelTier::Medium => "medium",
        }
    }
    /// Label recorded in transcript frontmatter.
    pub fn label(&self) -> String {
        format!("whisper-{}", self.as_str())
    }
    pub fn file_name(&self) -> &'static str {
        match self {
            ModelTier::Base => "ggml-base.bin",
            ModelTier::Small => "ggml-small.bin",
            ModelTier::Medium => "ggml-medium.bin",
        }
    }
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
            self.file_name()
        )
    }
    /// A sanity floor (not a checksum): a downloaded file far below this is a
    /// partial/failed transfer. A corrupt-but-large file is caught when the
    /// engine fails to load it (retryable).
    pub fn min_size(&self) -> u64 {
        match self {
            ModelTier::Base => 100_000_000,     // ~142 MB
            ModelTier::Small => 300_000_000,    // ~466 MB
            ModelTier::Medium => 1_000_000_000, // ~1.5 GB
        }
    }
}

/// `%APPDATA%\vault-buddy\models` — app-side, never inside a vault.
pub fn model_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vault-buddy").join("models"))
}

pub fn model_path(tier: ModelTier) -> Option<PathBuf> {
    model_dir().map(|d| d.join(tier.file_name()))
}

/// Download the tier's ggml model with progress, `.part`-then-rename. Skips
/// if already present. `on_progress(received, total)` is called per chunk.
pub fn download_model(
    tier: ModelTier,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<PathBuf, String> {
    let dir = model_dir().ok_or("cannot resolve model directory")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create model dir: {e}"))?;
    let dest = dir.join(tier.file_name());
    if dest.exists() {
        return Ok(dest);
    }
    let resp = ureq::get(&tier.url())
        .call()
        .map_err(|e| format!("request model: {e}"))?;
    let total: Option<u64> = resp.header("Content-Length").and_then(|v| v.parse().ok());
    let part = dir.join(format!("{}.part", tier.file_name()));
    let mut reader = resp.into_reader();
    let mut file = std::fs::File::create(&part).map_err(|e| format!("create model temp: {e}"))?;
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    loop {
        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| format!("read stream: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("write model: {e}"))?;
        received += n as u64;
        on_progress(received, total);
    }
    let _ = std::io::Write::flush(&mut file);
    let _ = file.sync_all();
    drop(file);
    if received < tier.min_size() {
        let _ = std::fs::remove_file(&part);
        return Err(format!("model download incomplete: {received} bytes"));
    }
    // We own `part` and `dest` didn't exist above — a plain rename is fine.
    std::fs::rename(&part, &dest).map_err(|e| format!("finalize model: {e}"))?;
    let _ = on_progress; // (kept in signature for the shell's progress events)
    let _ = Path::new("");
    Ok(dest)
}
```

Remove the two throwaway `let _ =` lines at the end if clippy flags them; they exist only to keep imports used — delete `use std::path::Path;` if `Path` ends up unused and drop the `let _ = Path::new("")` line.

- [ ] **Step 5: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_transcribe model::`
Expected: PASS.

- [ ] **Step 6: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings
cd .. && git add src-tauri/transcribe/src/model.rs src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): whisper model tier registry and on-demand download"
```

---

### Task 9: `transcribe_recording` orchestration

**Files:**
- Modify: `src-tauri/transcribe/src/lib.rs`
- Test: inline

**Interfaces:**
- Produces: `pub fn transcribe_recording(mp3: &Path, transcriber: &dyn Transcriber, opts: &TranscribeOptions, generated_at: &str) -> Result<PathBuf, String>`

- [ ] **Step 1: Write the failing tests**

Add to `src-tauri/transcribe/src/lib.rs` a test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_core::transcript::{transcript_path, Segment};

    struct FakeOk;
    impl Transcriber for FakeOk {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>) -> Result<Vec<Segment>, String> {
            Ok(vec![Segment { start_ms: 0, end_ms: 1000, text: "hello world".into() }])
        }
    }
    struct FakeErr;
    impl Transcriber for FakeErr {
        fn transcribe(&self, _s: &[f32], _l: Option<&str>) -> Result<Vec<Segment>, String> {
            Err("engine exploded".into())
        }
    }

    fn write_mp3(path: &std::path::Path) {
        use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm};
        let rate = 44_100u32;
        let frames = rate as usize / 2;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = ((i as f32 / rate as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.4
                * i16::MAX as f32) as i16;
            pcm.push(s);
            pcm.push(s);
        }
        let mut b = Builder::new().unwrap();
        b.set_num_channels(2).unwrap();
        b.set_sample_rate(rate).unwrap();
        b.set_brate(Bitrate::Kbps128).unwrap();
        b.set_quality(mp3lame_encoder::Quality::Good).unwrap();
        let mut enc = b.build().unwrap();
        let mut out = Vec::new();
        enc.encode_to_vec(InterleavedPcm(&pcm[..]), &mut out).unwrap();
        enc.flush_to_vec::<FlushNoGap>(&mut out).unwrap();
        std::fs::write(path, out).unwrap();
    }

    fn opts() -> TranscribeOptions {
        TranscribeOptions {
            language: Some("en".into()),
            timestamps: true,
            model_label: "whisper-small".into(),
        }
    }

    #[test]
    fn transcribe_writes_the_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        write_mp3(&mp3);
        let path =
            transcribe_recording(&mp3, &FakeOk, &opts(), "2026-07-04T15:00:00+00:00").unwrap();
        assert_eq!(path, transcript_path(&mp3));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("vault-buddy-transcript: complete"));
        assert!(text.contains("[00:00:00] hello world"));
    }

    #[test]
    fn engine_error_leaves_no_complete_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        write_mp3(&mp3);
        let err = transcribe_recording(&mp3, &FakeErr, &opts(), "t").unwrap_err();
        assert!(err.contains("engine exploded"));
        assert!(!transcript_path(&mp3).exists());
    }
}
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_transcribe --lib`
Expected: FAIL — `cannot find function transcribe_recording`.

- [ ] **Step 3: Implement the orchestration**

Add to `src-tauri/transcribe/src/lib.rs` (below `TranscribeOptions`):

```rust
use std::path::{Path, PathBuf};
use vault_buddy_core::transcript::{self, TranscriptMeta};

/// Decode → transcribe → atomically replace the sidecar with the finished
/// transcript. `generated_at` (RFC3339) is passed in so this stays
/// clock-free and testable. On any error the sidecar is left as-is (the
/// caller writes a retryable `failed` note); a `complete` transcript is only
/// ever written on success.
pub fn transcribe_recording(
    mp3: &Path,
    transcriber: &dyn Transcriber,
    opts: &TranscribeOptions,
    generated_at: &str,
) -> Result<PathBuf, String> {
    let samples = decode::decode_to_16k_mono(mp3)?;
    let duration_secs = samples.len() as u64 / decode::WHISPER_RATE as u64;
    let segments = transcriber.transcribe(&samples, opts.language.as_deref())?;
    let mp3_file_name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let meta = TranscriptMeta {
        mp3_file_name,
        model_label: opts.model_label.clone(),
        language: opts.language.clone(),
        duration_secs,
        generated_at: generated_at.to_string(),
        timestamps: opts.timestamps,
    };
    let content = transcript::render_transcript(&meta, &segments);
    let path = transcript::transcript_path(mp3);
    transcript::replace_if_ours(&path, &content).map_err(|e| format!("write transcript: {e}"))?;
    Ok(path)
}
```

- [ ] **Step 4: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_transcribe --lib`
Expected: PASS.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_transcribe --all-targets -- -D warnings
cd .. && git add src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): orchestrate decode, inference, and sidecar write"
```

---

### Task 10: whisper.cpp engine (feature-gated, CI-built)

**Files:**
- Create: `src-tauri/transcribe/src/engine.rs`
- Modify: `src-tauri/transcribe/src/lib.rs` (feature-gated `pub mod engine;`)

**Interfaces:**
- Produces (under `#[cfg(feature = "whisper")]`): `pub struct WhisperTranscriber` with `pub fn load(model_path: &Path) -> Result<Self, String>` and `impl Transcriber`.

**Note:** This module is compiled only with `--features whisper` and is **not** unit-tested on Linux (compiling whisper.cpp needs a C++ toolchain). The **windows-app CI job is the compile gate**. The exact segment-accessor names below target `whisper-rs` 0.16 (`WhisperSegment` via `state.as_iter()`); if the pinned version differs, adjust against docs.rs (older 0.14/0.15 expose flat `full_get_segment_text(i)` / `full_get_segment_t0(i)` getters). Timestamps from whisper are in **centiseconds**.

- [ ] **Step 1: Declare the module (feature-gated)**

Add to `src-tauri/transcribe/src/lib.rs`:

```rust
#[cfg(feature = "whisper")]
pub mod engine;
```

- [ ] **Step 2: Implement the binding**

Create `src-tauri/transcribe/src/engine.rs`:

```rust
//! whisper.cpp binding (whisper-rs), compiled only with the `whisper`
//! feature. Static-linked — no runtime DLL. Not Linux-tested; the
//! windows-app CI job is the compile gate.

use crate::Transcriber;
use std::path::Path;
use vault_buddy_core::transcript::Segment;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

pub struct WhisperTranscriber {
    ctx: WhisperContext,
}

impl WhisperTranscriber {
    pub fn load(model_path: &Path) -> Result<Self, String> {
        let ctx = WhisperContext::new_with_params(
            &model_path.to_string_lossy(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("load model {}: {e}", model_path.display()))?;
        Ok(Self { ctx })
    }
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&self, samples: &[f32], language: Option<&str>) -> Result<Vec<Segment>, String> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| format!("whisper state: {e}"))?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        if let Some(lang) = language {
            params.set_language(Some(lang));
        }
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_special(false);
        state
            .full(params, samples)
            .map_err(|e| format!("whisper inference: {e}"))?;

        // whisper-rs 0.16: iterate WhisperSegment objects; timestamps are in
        // centiseconds. See the module note for older-version accessors.
        let mut out = Vec::new();
        for segment in state.as_iter() {
            let text = segment.to_str_lossy().unwrap_or_default().trim().to_string();
            if text.is_empty() {
                continue;
            }
            let t0 = segment.start_timestamp().max(0) as u64;
            let t1 = segment.end_timestamp().max(0) as u64;
            out.push(Segment {
                start_ms: t0 * 10,
                end_ms: t1 * 10,
                text,
            });
        }
        Ok(out)
    }
}
```

- [ ] **Step 3: Confirm the default (Linux) build is unaffected**

Run: `cd src-tauri && cargo build -p vault_buddy_transcribe`
Expected: compiles — the `engine` module is `cfg`-gated out without `--features whisper`, so whisper.cpp is not built.

- [ ] **Step 4: fmt, then commit**

```bash
cd src-tauri && cargo fmt
cd .. && git add src-tauri/transcribe/src/engine.rs src-tauri/transcribe/src/lib.rs
git commit -m "feat(transcribe): whisper.cpp Transcriber behind the whisper feature"
```

**Windows build risk (record for the windows-app job):** `whisper-rs-sys` compiles whisper.cpp via CMake + bindgen; a documented MSVC failure emits glibc-flavoured bindings (`_G_fpos_t` size overflow). Ensure `LIBCLANG_PATH` is set on the runner. If it proves flaky, adopt the "prebuilt static libs pinned by commit, built in CI" recipe (the `sona`/`vibe` pattern) instead of building from source. This is validated in Task 15's Windows build.

---

## Phase D — Per-vault config

### Task 11: Transcription config fields

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs`
- Test: inline (extend existing tests)

**Interfaces:**
- Produces on `VaultCaptureConfig`: `transcribe: bool`, `transcription_model: String`, `transcription_language: Option<String>`, `transcript_timestamps: bool`.

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module in `capture_config.rs`:

```rust
    #[test]
    fn transcription_defaults_are_opt_in_small_timestamped() {
        let v = vault_config(&parse_config("{}"), "any");
        assert!(!v.transcribe, "opt-in: off by default");
        assert_eq!(v.transcription_model, "small");
        assert_eq!(v.transcription_language, None);
        assert!(v.transcript_timestamps);
    }

    #[test]
    fn transcription_fields_parse() {
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "transcribe": true,
                "transcriptionModel": "medium",
                "transcriptionLanguage": "es",
                "transcriptTimestamps": false
            } } }"#,
        );
        let v = vault_config(&cfg, "a");
        assert!(v.transcribe);
        assert_eq!(v.transcription_model, "medium");
        assert_eq!(v.transcription_language.as_deref(), Some("es"));
        assert!(!v.transcript_timestamps);
    }

    #[test]
    fn malformed_transcribe_defaults_locally() {
        // A quoted bool must not enable transcription, nor drop the entry.
        let cfg = parse_config(r#"{ "vaults": { "a": { "transcribe": "yes", "mode": "voice-note" } } }"#);
        let v = vault_config(&cfg, "a");
        assert!(!v.transcribe);
        assert_eq!(v.mode, RecordingMode::VoiceNote);
    }
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_core capture_config::`
Expected: FAIL — `no field transcribe`.

- [ ] **Step 3: Implement**

Add the fields to the struct:

```rust
pub struct VaultCaptureConfig {
    pub mode: RecordingMode,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    pub transcribe: bool,
    pub transcription_model: String,
    pub transcription_language: Option<String>,
    pub transcript_timestamps: bool,
}
```

Extend the `Default` impl:

```rust
impl Default for VaultCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Meeting,
            recording_folder: None,
            bitrate_kbps: 128,
            create_note: true,
            transcribe: false,
            transcription_model: "small".to_string(),
            transcription_language: None,
            transcript_timestamps: true,
        }
    }
}
```

Extend `vault_entry` (after the `create_note` field), keeping the per-field-defensive pattern:

```rust
        transcribe: entry
            .get("transcribe")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.transcribe),
        transcription_model: entry
            .get("transcriptionModel")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| defaults.transcription_model.clone()),
        transcription_language: entry
            .get("transcriptionLanguage")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        transcript_timestamps: entry
            .get("transcriptTimestamps")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.transcript_timestamps),
```

- [ ] **Step 4: Run to confirm pass (existing config tests still green)**

Run: `cd src-tauri && cargo test -p vault_buddy_core capture_config::`
Expected: PASS.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings
cd .. && git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): add per-vault transcription config fields (opt-in, small)"
```

---

## Phase E — Capture crate wiring

### Task 12: Thread `transcribe` through the session

**Files:**
- Modify: `src-tauri/capture/src/session.rs`
- Test: inline (extend existing tests)

**Interfaces:**
- Consumes: `NoteMeta.transcribe` (Task 5).
- Produces: `SessionParams.transcribe: bool`; the finalize note now reflects it.

- [ ] **Step 1: Write/adjust the tests**

In `session.rs`, in the `params()` test helper add `transcribe: false,` to the `SessionParams { … }`. Then add a test:

```rust
    #[test]
    fn note_embeds_transcript_when_transcribe_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut p = params(dir.path());
        p.transcribe = true;
        let session = CaptureSession::start(
            p,
            vec![SourceInput { name: "mic".into(), rate: 44_100, channels: 1, rx }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        let note = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(note.contains("## Transcript"));
        assert!(note.contains(".transcript]]"));
    }
```

- [ ] **Step 2: Run and confirm failure**

Run: `cd src-tauri && cargo test -p vault_buddy_capture session::`
Expected: FAIL — `missing field transcribe` in `params()` / `SessionParams`.

- [ ] **Step 3: Implement**

Add the field to `SessionParams`:

```rust
pub struct SessionParams {
    pub dir: PathBuf,
    pub base: String,
    pub part: PathBuf,
    pub bitrate_kbps: u32,
    pub vault_name: String,
    pub recording_type: String,
    pub create_note: bool,
    pub transcribe: bool,
    pub recorded_at: String,
    pub flush_every: Duration,
    pub fsync_every: Duration,
    pub warn_tx: Option<Sender<String>>,
}
```

In `run_worker`'s finalize, set the new `NoteMeta` field:

```rust
        let meta = NoteMeta {
            recorded_at: params.recorded_at.clone(),
            duration_secs,
            vault_name: params.vault_name.clone(),
            recording_type: params.recording_type.clone(),
            input_devices: device_names,
            event: warning.clone(),
            transcribe: params.transcribe,
        };
```

- [ ] **Step 4: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_capture session::`
Expected: PASS.

- [ ] **Step 5: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings
cd .. && git add src-tauri/capture/src/session.rs
git commit -m "feat(capture): thread the transcribe flag into the finalize note"
```

---

### Task 13: Embed transcript in recovered notes

**Files:**
- Modify: `src-tauri/capture/src/recovery.rs`
- Test: inline (update existing call sites + one new test)

**Interfaces:**
- Produces: `recover_root(root, vault_name, stale_after, write_note, transcribe)` — a new trailing `transcribe: bool` param; sets `NoteMeta.transcribe`.
- Consumed by: `capture_commands::run_recovery` (Task 14 updates the call site).

- [ ] **Step 1: Update the signature and the NoteMeta**

In `recover_root`, add the parameter and set the field:

```rust
pub fn recover_root(
    root: &Path,
    vault_name: &str,
    stale_after: Duration,
    write_note: bool,
    transcribe: bool,
) -> Vec<RecoveryAction> {
```

and in the note block inside it:

```rust
            let meta = NoteMeta {
                recorded_at: String::new(),
                duration_secs: 0,
                vault_name: vault_name.to_string(),
                recording_type: "Recording".to_string(),
                input_devices: Vec::new(),
                event: Some("recovered after crash".to_string()),
                transcribe,
            };
```

- [ ] **Step 2: Fix existing test call sites + add the new test**

Every existing `recover_root(dir.path(), "Work", Duration::…, <write_note>)` call in the `tests` module gains a trailing `, false` (recovered captures don't embed a transcript in these existing cases). The compiler lists each site. Then add:

```rust
    #[test]
    fn recovered_note_embeds_transcript_when_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let month = month_dir(dir.path());
        std::fs::write(month.join(format!(".{BASE}.mp3.part")), mp3_bytes()).unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, true, true);
        let note = std::fs::read_to_string(month.join(format!("{BASE} (recovered).md"))).unwrap();
        assert!(note.contains("## Transcript"));
    }
```

- [ ] **Step 3: Run to confirm pass**

Run: `cd src-tauri && cargo test -p vault_buddy_capture recovery::`
Expected: PASS (existing recovery tests unchanged behaviour + the new embed test).

- [ ] **Step 4: fmt + clippy, then commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings
cd .. && git add src-tauri/capture/src/recovery.rs
git commit -m "feat(capture): embed transcript in recovered notes when enabled"
```

Note: after this task, `cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe` must be fully green — this is the exact set the CI `rust-core` job will run (Task 18).

---

## Phase F — Shell wiring (Windows-only; `windows-app` CI is the compile gate)

> These tasks touch `src-tauri/src/*.rs`, which **cannot be compiled on Linux** (no webkit2gtk). Per AGENTS.md: mirror the existing patterns exactly, run `cargo fmt --check`, and let the `windows-app` CI job verify the build. The behavioural "tests" are the manual Windows checklist (Task 19).

### Task 14: Enable transcription in the capture flow

**Files:**
- Modify: `src-tauri/src/capture_commands.rs`
- Verify: `cargo fmt --check`; windows-app CI; manual checklist

**Interfaces:**
- Consumes: `SessionParams.transcribe` (Task 12), `recover_root(..., transcribe)` (Task 13), `TranscriptionState` + `enqueue_transcription` (Task 15), `vault_buddy_core::transcript::write_placeholder`.

- [ ] **Step 1: Pass the config flag into the session**

In `start_capture`, in the `SessionParams { … }` construction, add:

```rust
            create_note: cfg.create_note,
            transcribe: cfg.transcribe,
```

- [ ] **Step 2: Capture the vault id for the monitor thread**

`id` is moved into the final `StatusPayload`, so clone it for the monitor closure. Just before `let payload = StatusPayload { … vault_id: Some(id) … };`, add:

```rust
    let monitor_vault_id = id.clone();
```

- [ ] **Step 3: Enqueue transcription after a successful save**

In the monitor thread, change the success arm:

```rust
        match result {
            Ok(outcome) => {
                emit_saved(&app3, &outcome);
                maybe_enqueue_transcription(&app3, &monitor_vault_id, &outcome.mp3);
            }
            Err(e) => {
                log::error!("capture: finalize failed: {e}");
                emit_failed(&app3, &e);
            }
        }
```

Add the helper (it writes the placeholder immediately so the note's embed resolves at once, then queues the work):

```rust
/// After a save, if the vault opted into transcription, drop the
/// "transcribing…" placeholder (so the note's embed resolves instantly) and
/// queue the recording. Config is re-read here so a toggle mid-session is
/// respected.
fn maybe_enqueue_transcription(app: &AppHandle, vault_id: &str, mp3: &Path) {
    let cfg = capture_config::vault_config(&capture_config::load_config(), vault_id);
    if !cfg.transcribe {
        return;
    }
    let _ = vault_buddy_core::transcript::write_placeholder(mp3);
    enqueue_transcription(
        app,
        TranscriptionJob {
            mp3: mp3.to_path_buf(),
            vault_id: vault_id.to_string(),
        },
    );
}
```

- [ ] **Step 4: Pass `transcribe` to recovery, and queue late recoveries**

In `run_recovery`, update the `recover_root` call and enqueue recovered captures:

```rust
                    for action in vault_buddy_capture::recovery::recover_root(
                        &root,
                        &vault.name,
                        stale,
                        v.create_note,
                        v.transcribe,
                    ) {
                        use vault_buddy_capture::recovery::RecoveryAction;
                        match action {
                            RecoveryAction::Recovered { mp3 } => {
                                let name = mp3
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default();
                                toast(&app, "Recording recovered", &name);
                                if v.transcribe {
                                    let _ = vault_buddy_core::transcript::write_placeholder(&mp3);
                                    enqueue_transcription(
                                        &app,
                                        TranscriptionJob {
                                            mp3,
                                            vault_id: vault.id.clone(),
                                        },
                                    );
                                }
                            }
                            RecoveryAction::Fresh(_) => fresh_found = true,
                            RecoveryAction::DeletedEmpty(_) => {}
                        }
                    }
```

- [ ] **Step 5: fmt-check + commit**

```bash
cd src-tauri && cargo fmt
cd .. && git add src-tauri/src/capture_commands.rs
git commit -m "feat(shell): enqueue transcription after save and recovery"
```

(`capture_commands.rs` will not fully compile until Task 15 adds `TranscriptionState`/`enqueue_transcription`/`TranscriptionJob`; commit both together if your editor's checker complains, or proceed straight to Task 15 before pushing.)

---

### Task 15: Transcription worker, command, and app wiring

**Files:**
- Modify: `src-tauri/src/capture_commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`
- Verify: `cargo fmt --check`; windows-app CI (this is where whisper.cpp actually compiles + static-links); manual checklist

**Interfaces:**
- Produces: `pub struct TranscriptionState`; `pub fn run_transcription(app: &AppHandle)`; `#[tauri::command] pub fn transcribe_recording_now(app, path)`; internal `TranscriptionJob`, `enqueue_transcription`.
- Events: `capture:transcribing {mp3}`, `capture:transcribed {mp3, transcript}`, `capture:transcribeFailed {mp3, message}`, `capture:modelDownload {model, received, total}`.

- [ ] **Step 1: Depend on the transcribe crate (with whisper)**

In `src-tauri/Cargo.toml` `[dependencies]`, add:

```toml
vault_buddy_transcribe = { path = "transcribe", features = ["whisper"] }
```

- [ ] **Step 2: Add imports + state to `capture_commands.rs`**

At the top of `src-tauri/src/capture_commands.rs`, extend the imports:

```rust
use std::collections::{HashSet, VecDeque};
use vault_buddy_transcribe::engine::WhisperTranscriber;
use vault_buddy_transcribe::model::{download_model, model_path, ModelTier};
use vault_buddy_transcribe::{transcribe_recording, TranscribeOptions};
```

Add the state types (near `CaptureState`):

```rust
#[derive(Clone)]
struct TranscriptionJob {
    mp3: PathBuf,
    vault_id: String,
}

#[derive(Default)]
struct TranscriptionQueue {
    pending: VecDeque<TranscriptionJob>,
    /// Paths currently queued or in flight — dedupes the save-time enqueue
    /// against the startup/late-recovery scans.
    known: HashSet<PathBuf>,
}

/// Background transcription queue. One worker (see `run_transcription`)
/// drains it, yielding to any active recording so inference never steals
/// CPU from live capture.
#[derive(Default)]
pub struct TranscriptionState {
    inner: Mutex<TranscriptionQueue>,
    cv: Condvar,
}

fn enqueue_transcription(app: &AppHandle, job: TranscriptionJob) {
    let state = app.state::<TranscriptionState>();
    let mut guard = state.inner.lock().unwrap();
    if guard.known.insert(job.mp3.clone()) {
        log::info!("transcribe: queued {}", job.mp3.display());
        guard.pending.push_back(job);
        state.cv.notify_all();
    }
}
```

- [ ] **Step 3: Add the worker + helpers to `capture_commands.rs`**

```rust
/// Startup + on-demand worker: drains the transcription queue, postponing
/// while a recording is active. The loaded whisper model is cached across
/// jobs of the same tier. Mirrors `run_recovery`'s shape (own thread, coarse
/// is-recording gate).
pub fn run_transcription(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        // Backfill: transcribe anything already on disk missing a transcript
        // (previous-session saves, crash-recovered captures, freshly enabled
        // vaults).
        scan_and_enqueue(&app);
        let mut loaded: Option<(ModelTier, WhisperTranscriber)> = None;
        loop {
            // Block until a job is available; peek without claiming it.
            let job = {
                let state = app.state::<TranscriptionState>();
                let mut guard = state.inner.lock().unwrap();
                while guard.pending.is_empty() {
                    guard = state.cv.wait(guard).unwrap();
                }
                guard.pending.front().cloned().unwrap()
            };
            // Never contend with a live recording for CPU — re-check soon.
            if is_recording(&app) {
                std::thread::sleep(Duration::from_secs(30));
                continue;
            }
            {
                let state = app.state::<TranscriptionState>();
                state.inner.lock().unwrap().pending.pop_front();
            }
            process_transcription(&app, &job, &mut loaded);
            // Drop from the dedupe set: success leaves a `complete` sidecar
            // (won't rescan); failure leaves a `failed` one (a later launch's
            // scan or a manual retry re-queues it).
            {
                let state = app.state::<TranscriptionState>();
                state.inner.lock().unwrap().known.remove(&job.mp3);
            }
        }
    });
}

/// Enqueue every capture recording still needing a transcript, across all
/// vaults that opted in. Same root discipline as `run_recovery`.
fn scan_and_enqueue(app: &AppHandle) {
    let cfg = capture_config::load_config();
    for vault in discovery::discover_vaults() {
        let v = capture_config::vault_config(&cfg, &vault.id);
        if !v.transcribe {
            continue;
        }
        let roots: Vec<String> = match &v.recording_folder {
            Some(folder) => vec![folder.clone()],
            None => vec!["Meetings".to_string(), "Voice Notes".to_string()],
        };
        for folder in roots {
            let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), &folder) else {
                continue;
            };
            if !root.is_dir() {
                continue;
            }
            if capture_paths::assert_root_inside_vault(Path::new(&vault.path), &root).is_err() {
                continue;
            }
            for mp3 in vault_buddy_core::transcript::pending_transcriptions(&root) {
                enqueue_transcription(
                    app,
                    TranscriptionJob {
                        mp3,
                        vault_id: vault.id.clone(),
                    },
                );
            }
        }
    }
}

fn process_transcription(
    app: &AppHandle,
    job: &TranscriptionJob,
    loaded: &mut Option<(ModelTier, WhisperTranscriber)>,
) {
    let cfg = capture_config::vault_config(&capture_config::load_config(), &job.vault_id);
    if !cfg.transcribe {
        return; // disabled since it was queued
    }
    let tier = ModelTier::from_str(&cfg.transcription_model);
    let _ = app.emit(
        "capture:transcribing",
        serde_json::json!({ "mp3": job.mp3.to_string_lossy() }),
    );
    let _ = vault_buddy_core::transcript::write_placeholder(&job.mp3);

    let model = match ensure_model(app, tier) {
        Ok(p) => p,
        Err(e) => return fail_transcription(app, &job.mp3, &format!("model unavailable: {e}")),
    };
    if loaded.as_ref().map(|(t, _)| *t) != Some(tier) {
        match WhisperTranscriber::load(&model) {
            Ok(w) => *loaded = Some((tier, w)),
            Err(e) => return fail_transcription(app, &job.mp3, &e),
        }
    }
    let transcriber = &loaded.as_ref().unwrap().1;
    let opts = TranscribeOptions {
        language: cfg.transcription_language.clone(),
        timestamps: cfg.transcript_timestamps,
        model_label: tier.label(),
    };
    let generated_at = chrono::Local::now().to_rfc3339();
    match transcribe_recording(&job.mp3, transcriber, &opts, &generated_at) {
        Ok(path) => {
            log::info!("transcribe: wrote {}", path.display());
            let _ = app.emit(
                "capture:transcribed",
                serde_json::json!({
                    "mp3": job.mp3.to_string_lossy(),
                    "transcript": path.to_string_lossy(),
                }),
            );
        }
        Err(e) => fail_transcription(app, &job.mp3, &e),
    }
}

/// Ensure the tier's model is on disk, downloading with progress if not.
fn ensure_model(app: &AppHandle, tier: ModelTier) -> Result<PathBuf, String> {
    if let Some(p) = model_path(tier) {
        if p.exists() {
            return Ok(p);
        }
    }
    log::info!("transcribe: downloading model {}", tier.as_str());
    let app = app.clone();
    let mut last_emit: u64 = 0;
    download_model(tier, &mut |received, total| {
        // Throttle: an event every ~4 MB (and the final byte).
        if received.saturating_sub(last_emit) >= 4_000_000 || Some(received) == total {
            last_emit = received;
            let _ = app.emit(
                "capture:modelDownload",
                serde_json::json!({ "model": tier.as_str(), "received": received, "total": total }),
            );
        }
    })
}

/// Best-effort failure: leave the audio + note untouched, replace the
/// sidecar with a retryable `failed` note, and surface it.
fn fail_transcription(app: &AppHandle, mp3: &Path, message: &str) {
    log::warn!("transcribe: {} failed: {message}", mp3.display());
    let name = mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = vault_buddy_core::transcript::transcript_path(mp3);
    let content = vault_buddy_core::transcript::render_error(&name, message);
    let _ = vault_buddy_core::transcript::replace_if_ours(&path, &content);
    let _ = app.emit(
        "capture:transcribeFailed",
        serde_json::json!({ "mp3": mp3.to_string_lossy(), "message": message }),
    );
    toast(app, "Transcription failed", message);
}

/// The vault whose folder contains `mp3` (for the retry command).
fn owning_vault_id(mp3: &Path) -> Option<String> {
    discovery::discover_vaults()
        .into_iter()
        .find(|v| mp3.starts_with(&v.path))
        .map(|v| v.id)
}

/// Retry / on-demand transcription of a specific recording.
#[tauri::command]
pub fn transcribe_recording_now(app: AppHandle, path: String) -> Result<(), String> {
    let mp3 = PathBuf::from(&path);
    if !mp3.is_file() {
        return Err("Recording not found.".to_string());
    }
    let vault_id = owning_vault_id(&mp3).ok_or("Recording is not inside a known vault.")?;
    enqueue_transcription(&app, TranscriptionJob { mp3, vault_id });
    Ok(())
}
```

- [ ] **Step 4: Register state, command, and worker in `lib.rs`**

In `src-tauri/src/lib.rs`:

Add the managed state next to `CaptureState`:

```rust
        .manage(capture_commands::CaptureState::default())
        .manage(capture_commands::TranscriptionState::default())
```

Add the command to the handler list:

```rust
            capture_commands::start_capture,
            capture_commands::stop_capture,
            capture_commands::capture_status,
            capture_commands::transcribe_recording_now
        ])
```

Start the worker in `setup`, right after recovery:

```rust
            tray::create_tray(app.handle())?;
            capture_commands::run_recovery(app.handle());
            capture_commands::run_transcription(app.handle());
```

- [ ] **Step 5: fmt-check + commit**

```bash
cd src-tauri && cargo fmt --check
cd .. && git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(shell): background transcription worker, model download, and retry command"
```

- [ ] **Step 6: Windows compile gate**

Push the branch and confirm the **windows-app** CI job builds — this is where `whisper-rs` compiles and static-links whisper.cpp. If it fails on the MSVC/bindgen glibc-bindings issue, set `LIBCLANG_PATH` on the runner or switch to the prebuilt-static-libs recipe (see Task 10's note). Expected on success: the installer builds with no new runtime DLLs bundled.

---

## Phase G — Frontend

### Task 16: Transcription state in the capture store

**Files:**
- Modify: `src/types.ts`, `src/stores/capture.ts`
- Test: `tests/capture-store.test.ts`

**Interfaces:**
- Produces on the store: `transcribing`, `transcriptError`, `transcriptFailedMp3`, `modelDownload`, and `retryTranscription()`.

- [ ] **Step 1: Add event payload types**

Append to `src/types.ts`:

```ts
export interface CaptureTranscribed {
  mp3: string;
  transcript: string;
}

export interface CaptureTranscribeFailed {
  mp3: string;
  message: string;
}

export interface ModelDownload {
  model: string;
  received: number;
  total: number | null;
}
```

- [ ] **Step 2: Write the failing tests**

Add to `tests/capture-store.test.ts`:

```ts
  it("transcribing event sets the transcribing flag", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3" } });
    expect(store.transcribing).toBe(true);
  });

  it("model download progress is tracked, then cleared on transcribed", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribing"]!({ payload: { mp3: "/v/m.mp3" } });
    state.eventHandlers["capture:modelDownload"]!({
      payload: { model: "small", received: 5, total: 10 },
    });
    expect(store.modelDownload).toEqual({ model: "small", received: 5, total: 10 });
    state.eventHandlers["capture:transcribed"]!({
      payload: { mp3: "/v/m.mp3", transcript: "/v/m.transcript.md" },
    });
    expect(store.transcribing).toBe(false);
    expect(store.modelDownload).toBeNull();
  });

  it("transcribeFailed surfaces an error and the mp3 for retry", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribeFailed"]!({
      payload: { mp3: "/v/m.mp3", message: "model unavailable" },
    });
    expect(store.transcribing).toBe(false);
    expect(store.transcriptError).toBe("model unavailable");
    expect(store.transcriptFailedMp3).toBe("/v/m.mp3");
  });

  it("retryTranscription re-invokes the command for the failed file", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "capture_status") return { recording: false, vaultId: null, startedAtMs: null };
    });
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:transcribeFailed"]!({
      payload: { mp3: "/v/m.mp3", message: "boom" },
    });
    await store.retryTranscription();
    expect(calls).toContainEqual({
      cmd: "transcribe_recording_now",
      args: { path: "/v/m.mp3" },
    });
    expect(store.transcriptError).toBeNull();
  });
```

- [ ] **Step 3: Run and confirm failure**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: FAIL — `transcribing` etc. undefined.

- [ ] **Step 4: Implement the store changes**

In `src/stores/capture.ts`, add to `state`:

```ts
    transcribing: false as boolean,
    transcriptError: null as string | null,
    transcriptFailedMp3: null as string | null,
    modelDownload: null as { model: string; received: number; total: number | null } | null,
```

Add listeners in `init()` (after the existing ones, before the resync):

```ts
      await listen<{ mp3: string }>("capture:transcribing", () => {
        this.transcribing = true;
        this.transcriptError = null;
      });
      await listen<{ mp3: string; transcript: string }>("capture:transcribed", () => {
        this.transcribing = false;
        this.modelDownload = null;
      });
      await listen<{ mp3: string; message: string }>("capture:transcribeFailed", (event) => {
        this.transcribing = false;
        this.modelDownload = null;
        this.transcriptError = event.payload.message;
        this.transcriptFailedMp3 = event.payload.mp3;
      });
      await listen<{ model: string; received: number; total: number | null }>(
        "capture:modelDownload",
        (event) => {
          this.modelDownload = event.payload;
        },
      );
```

Add the action (after `stop()`):

```ts
    async retryTranscription() {
      if (!this.transcriptFailedMp3) return;
      const path = this.transcriptFailedMp3;
      this.transcriptError = null;
      try {
        await invoke("transcribe_recording_now", { path });
        this.transcribing = true;
      } catch (e) {
        this.transcriptError = String(e);
      }
    },
```

- [ ] **Step 5: Run to confirm pass**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: PASS (existing + 4 new).

- [ ] **Step 6: Commit**

```bash
git add src/types.ts src/stores/capture.ts tests/capture-store.test.ts
git commit -m "feat(ui): track transcription state and retry in the capture store"
```

---

### Task 17: Transcription status indicator

**Files:**
- Create: `src/components/TranscriptionStatus.vue`
- Modify: `src/components/ActionPanel.vue`
- Test: `tests/transcription-status.test.ts`

**Interfaces:**
- Consumes: the capture store's `transcribing` / `modelDownload` / `transcriptError` / `transcriptFailedMp3` / `retryTranscription`.

- [ ] **Step 1: Write the failing component test**

Create `tests/transcription-status.test.ts`:

```ts
import { describe, expect, it, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import { setActivePinia, createPinia } from "pinia";
import TranscriptionStatus from "../src/components/TranscriptionStatus.vue";
import { useCaptureStore } from "../src/stores/capture";

describe("TranscriptionStatus", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("is empty when idle", () => {
    const w = mount(TranscriptionStatus);
    expect(w.text()).toBe("");
  });

  it("shows a transcribing message", () => {
    const store = useCaptureStore();
    store.transcribing = true;
    const w = mount(TranscriptionStatus);
    expect(w.text()).toContain("Transcribing");
  });

  it("shows model download progress", () => {
    const store = useCaptureStore();
    store.transcribing = true;
    store.modelDownload = { model: "small", received: 5, total: 10 };
    const w = mount(TranscriptionStatus);
    expect(w.text()).toContain("small");
    expect(w.text()).toContain("50%");
  });

  it("shows an error with a retry button", () => {
    const store = useCaptureStore();
    store.transcriptError = "model unavailable";
    store.transcriptFailedMp3 = "/v/m.mp3";
    const w = mount(TranscriptionStatus);
    expect(w.text()).toContain("model unavailable");
    expect(w.find("button").exists()).toBe(true);
  });
});
```

- [ ] **Step 2: Run and confirm failure**

Run: `npx vitest run tests/transcription-status.test.ts`
Expected: FAIL — cannot resolve `TranscriptionStatus.vue`.

- [ ] **Step 3: Implement the component**

Create `src/components/TranscriptionStatus.vue`:

```vue
<script setup lang="ts">
import { computed } from "vue";
import { useCaptureStore } from "../stores/capture";

const capture = useCaptureStore();

const downloadPct = computed(() => {
  const d = capture.modelDownload;
  if (!d || !d.total) return null;
  return Math.min(100, Math.round((d.received / d.total) * 100));
});
</script>

<template>
  <div v-if="capture.transcribing || capture.transcriptError">
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
      v-else
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
  </div>
</template>
```

- [ ] **Step 4: Wire it into the panel**

In `src/components/ActionPanel.vue`, import it (with the other component imports):

```ts
import TranscriptionStatus from "./TranscriptionStatus.vue";
```

and render it just after the `capture.error` paragraph (after line ~116):

```html
    <TranscriptionStatus v-if="!showSettings" class="mb-2" />
```

- [ ] **Step 5: Run tests + typecheck**

Run: `npx vitest run tests/transcription-status.test.ts && npm run build`
Expected: tests PASS; `vue-tsc` typecheck + build succeed.

- [ ] **Step 6: Commit**

```bash
git add src/components/TranscriptionStatus.vue src/components/ActionPanel.vue tests/transcription-status.test.ts
git commit -m "feat(ui): show transcribing / model-download / retry status in the panel"
```

---

## Phase H — CI + docs

### Task 18: Cover the transcribe crate in CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the crate to the rust-core job**

In `.github/workflows/ci.yml`, extend the two `rust-core` commands to include the new crate (default features → no whisper.cpp compile):

```yaml
      - name: clippy (core + capture + transcribe crates)
        run: cargo clippy -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --all-targets -- -D warnings
        working-directory: src-tauri
      - name: tests (core + capture + transcribe crates)
        run: cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe
        working-directory: src-tauri
```

No new system dependency is needed: `symphonia`/`ureq` are pure Rust, and `mp3lame-encoder` (already built for the capture crate) compiles with the runner's C toolchain.

- [ ] **Step 2: Verify locally**

Run: `cd src-tauri && cargo clippy -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --all-targets -- -D warnings && cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe`
Expected: all green.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: run clippy and tests for the transcribe crate"
```

---

### Task 19: Documentation + Windows verification checklist

**Files:**
- Modify: `docs/DEVELOPMENT.md`, `AGENTS.md`
- Create: `docs/superpowers/specs/2026-07-04-increment-3-windows-verification.md`

- [ ] **Step 1: Document the config fields**

In `docs/DEVELOPMENT.md`, in the section that documents `%APPDATA%\vault-buddy\config.json`, add the new per-vault keys:

```markdown
- `transcribe` (bool, default `false`) — opt in to local speech-to-text. Enabling it downloads a Whisper model on the next recording (or backfills existing recordings) and writes a `<name>.transcript.md` sidecar the note embeds.
- `transcriptionModel` (`"base"` | `"small"` | `"medium"`, default `"small"`) — accuracy/speed/size trade-off. Models download to `%APPDATA%\vault-buddy\models`.
- `transcriptionLanguage` (string or omit, default auto-detect) — e.g. `"es"`; omit to auto-detect per recording.
- `transcriptTimestamps` (bool, default `true`) — prefix each segment with `[HH:MM:SS]`.
```

- [ ] **Step 2: Update the agent guide**

In `AGENTS.md`, add `transcribe` to the "What compiles where" table and note the new write path. Under the crate table add:

```markdown
| `src-tauri/transcribe/` | Pure-ish crate: MP3→PCM decode (Symphonia), model registry/download, and whisper.cpp via `whisper-rs` behind the `whisper` feature. | Anywhere with default features (no whisper.cpp); the `whisper` feature + real engine build on **Windows** (CI gate). |
```

And amend the vault-domain write-path note to record the transcript sidecar as a second sanctioned write, produced by the transcription worker under the same never-clobber/atomic rules (marker field `vault-buddy-transcript`, replace-only-if-ours).

- [ ] **Step 3: Create the Windows verification checklist**

Create `docs/superpowers/specs/2026-07-04-increment-3-windows-verification.md`:

```markdown
# Increment 3 — Windows Verification Checklist

Local speech-to-text. Run on a Windows machine after the `windows-app` build.

## Setup
1. In `%APPDATA%\vault-buddy\config.json`, set a vault's `transcribe: true` (leave `transcriptionModel` default `small`).

## Happy path
2. Record a short (~30 s) clip with some Spanish and English speech; Stop.
3. Confirm the MP3 still saves within ~5 s (the save toast fires first).
4. Confirm the first transcription downloads the `small` model once, with a visible progress indicator in the panel; later recordings reuse it with **no network**.
5. Open the meeting note in Obsidian: the `## Transcript` section renders the transcript **inline** (embedded sidecar), with `[HH:MM:SS]` timestamps that line up with the audio player.
6. Confirm `<name>.transcript.md` exists beside the MP3 and carries `vault-buddy-transcript: complete`.

## Embed resolution (spec risk)
7. Confirm the dotted embed `![[<name>.transcript]]` resolves in Obsidian (no "file not found"). If it does not, rename the sidecar scheme to `<name> transcript.md` / `![[<name> transcript]]` (see the design spec).

## Resilience + failure
8. Start a recording, Stop, and quit the app **while "Transcribing…" is showing**; relaunch → the transcript completes (startup scan resumes it).
9. Kill the app mid-recording; relaunch → the recording is recovered **and** transcribed.
10. Temporarily point the model URL at nothing (or go offline before the first download): confirm the audio + note are untouched, the transcript embed shows a retryable "failed" note, a toast fires, and the panel's **Retry** works once back online.
11. Hand-edit a completed `.transcript.md`; trigger a rescan (relaunch): confirm your edits are **not** overwritten.

## No-cloud audit
12. With the model already downloaded, transcribe with the network disconnected → succeeds fully offline.
```

- [ ] **Step 4: Commit**

```bash
git add docs/DEVELOPMENT.md AGENTS.md docs/superpowers/specs/2026-07-04-increment-3-windows-verification.md
git commit -m "docs: document transcription config, crate topology, and Windows checklist"
```

---

## Self-Review — spec coverage

| Spec requirement | Task(s) |
| --- | --- |
| Auto batch transcription after finalize, off the event loop | 14, 15 |
| In-process whisper.cpp via `whisper-rs`, static-linked, feature-gated | 6, 10, 15 |
| MP3 → 16 kHz mono decode (Symphonia) | 7 |
| Multilingual `small` default, per-vault model tier | 8, 11 |
| Model downloaded on demand to `%APPDATA%`, progress, no bundling | 8, 15 |
| Embedded sidecar `<base>.transcript.md`, `![[…transcript]]` under `## Transcript` | 5, 15 |
| Placeholder → real content; marker `pending`/`failed`/`complete` | 1, 2, 15 |
| Pairwise name reservation with mp3/md/part | 4 |
| Opt-in, default off; config fields parsed defensively | 11 |
| Recovery-style worker: postpone while recording, startup rescan, resumes after crash | 15 |
| Recovered recordings transcribed too | 13, 14 |
| Never clobber; atomic writes; replace only our own sidecar; never reopen the note | 1, 2, 5 |
| Best-effort failure → retryable `failed` note + toast, audio/note untouched | 15, 16, 17 |
| Events `transcribing`/`transcribed`/`transcribeFailed`/`modelDownload`; retry command | 15, 16 |
| Frontend status + retry | 16, 17 |
| CI covers the transcribe crate; Windows job is the whisper compile gate | 18, 15 |
| Docs + Windows verification checklist | 19 |

**Placeholder scan:** no `TBD`/`TODO`/"handle appropriately" left; every code step carries real code. The only intentionally-deferred items are documented deviations: linear resampler in place of rubato (Task 7 note), size-sanity instead of pinned SHA-256 (Task 8), and the whisper-rs 0.16 segment-accessor names to confirm on the Windows build (Task 10 note).

**Type consistency:** the canonical `Segment { start_ms: u64, end_ms: u64, text: String }` (core) is used by the trait, engine, and orchestration; `TranscribeOptions`, `TranscriptMeta`, `ModelTier`, and `TranscriptionJob` names match across producing/consuming tasks; `render_note` / `NoteMeta.transcribe`, `SessionParams.transcribe`, `recover_root(..., transcribe)`, and the `vault-buddy-transcript` marker are used consistently.
