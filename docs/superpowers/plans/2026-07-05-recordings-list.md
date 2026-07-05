# Recordings List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A read-only Recordings panel view — reached from the Start Recording modal as a third option — that lists one vault's past recordings as title/date/duration rows, optionally grouped by the `type` read from each recording's companion note.

**Architecture:** Two new pure-core pieces (a minimal frontmatter reader `note_field`, and a `pending_transcriptions`-style `list_recordings` scan) feed a thin shell command; the frontend adds a `recordings` panel view (mirroring `captureSettings`) rendered by a new `Recordings.vue`, launched by a "Browse recordings" option in `RecordModeDialog`. Read-only throughout: opening a row hands off to Obsidian via the audit-logged `uri::launch`.

**Two parts.** **Part 1 (Tasks 1–7)** is the read-only Recordings list above. **Part 2 (Tasks 8–12)** folds in an independent, approved feature — a per-vault `follow_up_template` setting (default on) that appends a `## Follow-up` scaffold to each recording's companion note. It's one boolean threaded config → `SessionParams` → `NoteMeta` → `render_note`; **no new vault-write path** (the companion note is already written atomically). See the spec's Addendum. The two parts are independent — Part 2's only shared file with Part 1 is `capture_note.rs` (Task 1 adds `note_field`; Task 8 adds `follow_up`), touched in disjoint places.

**Tech Stack:** Rust (`vault_buddy_core` pure/Linux-tested; `vault-buddy` shell Windows-only compile), Vue 3 + Pinia + Tailwind, Vitest + @vue/test-utils + `mockIPC`.

## Global Constraints

- **Never writes into a vault.** The feature only reads (directory scan + frontmatter) and opens notes via `obsidian://` — the same `uri::launch` audit path as every vault open. No new write path.
- **Scan discipline (identical to `transcript::pending_transcriptions`):** `<root>/YYYY/MM` only, `is_capture_base` `.mp3`s only, via `dir_entries` (reads dirent file-type — never follows symlinks/junctions out of the vault).
- **Malformed input degrades, never errors:** an unknown vault / unreadable root → empty list; a missing or garbage companion note → `None` type+duration (→ "Ungrouped"), never a dropped recording and never a thrown error.
- **Core stays pure and Linux-tested.** The shell crate (`src-tauri/src/*.rs`) compiles on **Windows only** (CI's `windows-app` job is the gate) — mirror existing patterns exactly and run `cargo fmt --check`; do not claim a local shell compile.
- **Group toggle is per-view and NOT persisted** — component-local `ref`, default grouped.
- **Commits:** Conventional Commits — `feat(core)`, `feat(shell)`, `feat(ui)`. Imperative subject; body explains the *why*.
- **Rust commands** (from `src-tauri/`): `cargo test -p vault_buddy_core <filter>`, `cargo fmt --check`, `cargo clippy -p vault_buddy_core --all-targets -- -D warnings`.
- **Frontend commands** (from repo root `/home/user/vault-buddy`): `npx vitest run tests/<file>.test.ts`, `npm run build` (vue-tsc typecheck).

---

### Task 1: `note_field` — a minimal frontmatter reader (core)

None exists today — `capture_note.rs` only *writes* notes. Add a pure reader that extracts one top-level scalar from a note's leading `---` frontmatter, undoing `yaml_quote`'s escaping. Grouping/duration depend on it.

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs` (add `note_field` + private `unquote_yaml`; add tests in the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: the existing `render_note` / `yaml_quote` (for round-trip tests), the `meta()` test helper.
- Produces: `pub fn note_field(content: &str, key: &str) -> Option<String>`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/core/src/capture_note.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn note_field_reads_top_level_scalars() {
        // meta(): type "Meeting", duration 3723s → "1:02:03", vault "Work".
        let note = render_note(&meta(), "x.mp3");
        assert_eq!(note_field(&note, "type").as_deref(), Some("Meeting"));
        assert_eq!(note_field(&note, "duration").as_deref(), Some("1:02:03"));
        assert_eq!(note_field(&note, "vault").as_deref(), Some("Work"));
    }

    #[test]
    fn note_field_is_none_when_absent_or_no_frontmatter() {
        let note = render_note(&meta(), "x.mp3");
        assert_eq!(note_field(&note, "nope"), None);
        // an indented list item (inputs:) is not a top-level scalar
        assert_eq!(note_field(&note, "Headset Mic"), None);
        assert_eq!(note_field("no frontmatter here\ntype: X\n", "type"), None);
        assert_eq!(note_field("", "type"), None);
    }

    #[test]
    fn note_field_unescapes_quotes_and_ignores_the_body() {
        // A quoted value with an embedded quote round-trips; a `type:` mention
        // in the note body must never be picked up (search stops at the
        // closing `---`).
        let mut m = meta();
        m.recording_type = r#"A "quoted" type"#.into();
        let note = format!("{}\nprose mentioning type: fake\n", render_note(&m, "x.mp3"));
        assert_eq!(note_field(&note, "type").as_deref(), Some(r#"A "quoted" type"#));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core note_field`
Expected: FAIL to compile — `note_field` is not defined.

- [ ] **Step 3: Implement `note_field`**

In `src-tauri/core/src/capture_note.rs`, add after `yaml_quote` (before `render_note`):

```rust
/// Read one top-level `key:` scalar from a note's leading `---` frontmatter
/// block, undoing `yaml_quote`'s escaping. Returns None if the note has no
/// frontmatter block or the key is absent. Deliberately minimal — it only
/// reads back the handful of top-level fields `render_note` itself writes, so
/// it needs no general YAML parser (indented list items are skipped, and the
/// search stops at the closing `---` so the note body is never scanned).
pub fn note_field(content: &str, key: &str) -> Option<String> {
    let mut lines = content.lines();
    // Frontmatter must be the very first line.
    if lines.next()?.trim_end() != "---" {
        return None;
    }
    let prefix = format!("{key}:");
    for line in lines {
        if line.trim_end() == "---" {
            break; // end of frontmatter — never scan the body
        }
        // `strip_prefix` on the raw line matches top-level keys only: an
        // indented `  - device` list item has a leading space and can't match.
        if let Some(rest) = line.strip_prefix(&prefix) {
            return Some(unquote_yaml(rest.trim()));
        }
    }
    None
}

/// Inverse of `yaml_quote` for the double-quoted form: strip the surrounding
/// quotes and unescape `\"` then `\\` (reverse order of the escaping). An
/// unquoted scalar (older/hand-edited note) is returned as-is.
fn unquote_yaml(value: &str) -> String {
    match value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => value.to_string(),
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core note_field`
Expected: PASS (all three new tests).

- [ ] **Step 5: Format, clippy, commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_note.rs
git commit -m "feat(core): read a top-level scalar back out of a note's frontmatter

The Recordings list groups by each recording's type and shows its
duration, both of which live in the companion note's YAML frontmatter.
render_note only wrote notes; add a minimal note_field reader (top-level
scalars only, unescaping yaml_quote) to read them back."
```

---

### Task 2: `recording_roots()` — the folders to scan (core)

The Recordings scan must see EVERY past recording. A configured custom folder holds them all; without one, meetings and voice notes live in two homes and the mode may have changed over time — so scan both. This exact `match` is already open-coded twice in the shell (`run_recovery`, `scan_and_enqueue`); centralize it.

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs` (add `recording_roots` to the `impl VaultCaptureConfig` block; add a test)

**Interfaces:**
- Consumes: `VaultCaptureConfig.recording_folder` (existing field), `parse_config`/`vault_config` test helpers.
- Produces: `pub fn recording_roots(&self) -> Vec<&str>`.

- [ ] **Step 1: Write the failing test**

In `src-tauri/core/src/capture_config.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn recording_roots_are_the_custom_folder_or_both_defaults() {
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": { "mode": "voice-note" },
                "b": { "recordingFolder": "Inbox" }
            } }"#,
        );
        // No custom folder → scan both mode homes (mode may have changed).
        assert_eq!(
            vault_config(&cfg, "a").recording_roots(),
            vec!["Meetings", "Voice Notes"]
        );
        // Custom folder → it holds every recording, scan just it.
        assert_eq!(vault_config(&cfg, "b").recording_roots(), vec!["Inbox"]);
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core recording_roots`
Expected: FAIL to compile — no method `recording_roots`.

- [ ] **Step 3: Implement `recording_roots`**

In `src-tauri/core/src/capture_config.rs`, in the existing `impl VaultCaptureConfig` block (right after `effective_recording_folder`), add:

```rust
    /// Folders that may hold this vault's recordings, for scans that must see
    /// EVERY past recording (the Recordings list, recovery, transcription
    /// backfill). A configured custom folder holds them all; without one,
    /// meetings and voice notes live in their two distinct default homes and
    /// the mode may have changed over the vault's life, so scan both. This is
    /// the union of `effective_recording_folder`'s branches.
    pub fn recording_roots(&self) -> Vec<&str> {
        match &self.recording_folder {
            Some(folder) => vec![folder.as_str()],
            None => vec!["Meetings", "Voice Notes"],
        }
    }
```

- [ ] **Step 4: Run the test to verify it passes**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core recording_roots`
Expected: PASS.

- [ ] **Step 5: Format, clippy, commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): add recording_roots — every folder a vault's recordings may live in

A scan that must find all past recordings (the new Recordings list) needs
the custom folder, or BOTH mode defaults when there's no override since the
mode may have changed. Centralize the match the shell already open-codes in
recovery and transcription backfill."
```

---

### Task 3: `list_recordings` scan (core)

The read-only enumerator: walk each root's `YYYY/MM`, take capture `.mp3`s, pair each with its companion note's type+duration, sort newest-first. Mirrors `transcript::pending_transcriptions`.

**Files:**
- Create: `src-tauri/core/src/recordings.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod recordings;`)
- Modify: `src-tauri/core/src/transcript.rs` (make `dir_entries` + `is_digit_dir` `pub(crate)` so the new scan reuses them)

**Interfaces:**
- Consumes: `transcript::dir_entries`, `transcript::is_digit_dir` (made `pub(crate)` here), `capture_paths::is_capture_base`, `capture_note::note_field`.
- Produces: `pub struct RecordingEntry { mp3_path: PathBuf, title: String, recorded_at: String, duration: Option<String>, recording_type: Option<String> }` and `pub fn list_recordings(roots: &[PathBuf]) -> Vec<RecordingEntry>`.

- [ ] **Step 1: Share the two scan helpers**

In `src-tauri/core/src/transcript.rs`, change the two private helper signatures (they're currently `fn`):

```rust
pub(crate) fn is_digit_dir(name: &str, len: usize) -> bool {
```
```rust
pub(crate) fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
```

(Only the visibility keyword changes; bodies stay identical.)

- [ ] **Step 2: Declare the module**

In `src-tauri/core/src/lib.rs`, add to the module list (keep alphabetical — after `process`):

```rust
pub mod recordings;
```

- [ ] **Step 3: Write the failing tests + the module skeleton**

Create `src-tauri/core/src/recordings.rs` with the doc header, imports, the struct, a `todo!()` body, and the tests:

```rust
//! Read-only enumeration of a vault's past recordings for the in-app
//! Recordings list. Scans the same `<root>/YYYY/MM` layout capture writes
//! (capture-named `.mp3`s only, no symlink follow — identical discipline to
//! `transcript::pending_transcriptions`) and pairs each recording with the
//! `type`/`duration` read back from its companion note. Never writes.

use crate::capture_note::note_field;
use crate::capture_paths::is_capture_base;
use crate::transcript::{dir_entries, is_digit_dir};
use std::path::{Path, PathBuf};

/// One recording surfaced in the list. `title`/`recorded_at` come from the
/// capture base name (always parseable — it passed `is_capture_base`);
/// `duration`/`recording_type` are best-effort from the companion note and
/// are None when there's no note or the field is absent (→ "Ungrouped").
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingEntry {
    pub mp3_path: PathBuf,
    pub title: String,
    pub recorded_at: String,
    pub duration: Option<String>,
    pub recording_type: Option<String>,
}

/// Chars of the `YYYY-MM-DD HHmm ` prefix (10 date + space + 4 time + space).
const PREFIX_CHARS: usize = 16;

pub fn list_recordings(roots: &[PathBuf]) -> Vec<RecordingEntry> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_note::{render_note, NoteMeta};

    /// Write `<base>.mp3` under `root/YYYY/MM`, plus a companion note with the
    /// given `type` (and a 65s duration → "1:05") when `note_type` is Some.
    fn write_recording(root: &Path, year: &str, month: &str, base: &str, note_type: Option<&str>) {
        let dir = root.join(year).join(month);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{base}.mp3")), b"id3").unwrap();
        if let Some(t) = note_type {
            let meta = NoteMeta {
                recorded_at: "2026-07-04T14:05:00+02:00".into(),
                duration_secs: 65,
                vault_name: "Work".into(),
                recording_type: t.into(),
                paused: None,
                input_devices: vec![],
                event: None,
                transcribe: false,
            };
            std::fs::write(
                dir.join(format!("{base}.md")),
                render_note(&meta, &format!("{base}.mp3")),
            )
            .unwrap();
        }
    }

    #[test]
    fn lists_a_recording_with_type_and_duration_from_the_note() {
        let root = tempfile::tempdir().unwrap();
        write_recording(root.path(), "2026", "07", "2026-07-04 1405 Standup", Some("Meeting"));
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Standup");
        assert_eq!(list[0].recorded_at, "2026-07-04 14:05");
        assert_eq!(list[0].duration.as_deref(), Some("1:05"));
        assert_eq!(list[0].recording_type.as_deref(), Some("Meeting"));
    }

    #[test]
    fn type_comes_from_the_note_not_the_folder() {
        // A recording physically under one root whose note says "Voice Note".
        let root = tempfile::tempdir().unwrap();
        write_recording(root.path(), "2026", "07", "2026-07-04 1405 Chat", Some("Voice Note"));
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list[0].recording_type.as_deref(), Some("Voice Note"));
    }

    #[test]
    fn recording_without_a_note_has_no_type_or_duration() {
        let root = tempfile::tempdir().unwrap();
        write_recording(root.path(), "2026", "07", "2026-07-04 1405 Orphan", None);
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Orphan");
        assert_eq!(list[0].duration, None);
        assert_eq!(list[0].recording_type, None);
    }

    #[test]
    fn sorts_newest_first_and_merges_multiple_roots() {
        let meetings = tempfile::tempdir().unwrap();
        let voice = tempfile::tempdir().unwrap();
        write_recording(meetings.path(), "2026", "07", "2026-07-04 0900 Early", Some("Meeting"));
        write_recording(voice.path(), "2026", "07", "2026-07-04 1700 Late", Some("Voice Note"));
        let list = list_recordings(&[
            meetings.path().to_path_buf(),
            voice.path().to_path_buf(),
        ]);
        let titles: Vec<_> = list.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, vec!["Late", "Early"]); // 17:00 before 09:00
    }

    #[test]
    fn ignores_non_capture_files_and_in_progress_parts() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("2026").join("07");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("holiday.mp3"), b"x").unwrap(); // not a capture base
        std::fs::write(dir.join(".2026-07-04 1405 Live.mp3.part"), b"x").unwrap(); // in-progress
        write_recording(root.path(), "2026", "07", "2026-07-04 1405 Real", Some("Meeting"));
        let list = list_recordings(&[root.path().to_path_buf()]);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Real");
    }

    #[test]
    fn a_missing_root_yields_no_entries() {
        let list = list_recordings(&[PathBuf::from("/no/such/place")]);
        assert!(list.is_empty());
    }
}
```

- [ ] **Step 4: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core recordings::`
Expected: FAIL — `list_recordings` panics with `not yet implemented` (`todo!()`).

- [ ] **Step 5: Implement the scan**

In `src-tauri/core/src/recordings.rs`, replace the `todo!()` body and add the two private helpers below it:

```rust
/// Every capture recording under the given roots' `YYYY/MM` layout, newest
/// first. Read-only: reads each companion `<base>.md` for type/duration but
/// never writes. Missing/unreadable roots and notes degrade silently.
pub fn list_recordings(roots: &[PathBuf]) -> Vec<RecordingEntry> {
    let mut out = Vec::new();
    for root in roots {
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
                    let Some(base) = name.strip_suffix(".mp3") else {
                        continue;
                    };
                    if !is_capture_base(base) {
                        continue;
                    }
                    out.push(entry_for(&path, base));
                }
            }
        }
    }
    // Newest first. Base names begin with a lexically-sortable
    // `YYYY-MM-DD HHmm`, so a reverse compare on recorded_at is chronological;
    // same-minute ties fall back to the path for a stable order.
    out.sort_by(|a, b| {
        b.recorded_at
            .cmp(&a.recorded_at)
            .then_with(|| b.mp3_path.cmp(&a.mp3_path))
    });
    out
}

fn entry_for(mp3_path: &Path, base: &str) -> RecordingEntry {
    let (title, recorded_at) = split_base(base);
    // Companion note is best-effort: read type + duration when it's there.
    let (duration, recording_type) = match std::fs::read_to_string(mp3_path.with_extension("md")) {
        Ok(content) => (note_field(&content, "duration"), note_field(&content, "type")),
        Err(_) => (None, None),
    };
    RecordingEntry {
        mp3_path: mp3_path.to_path_buf(),
        title,
        recorded_at,
        duration,
        recording_type,
    }
}

/// `"2026-07-04 1405 Standup"` → (title "Standup", recorded_at
/// "2026-07-04 14:05"). The base already passed `is_capture_base`, so the
/// fixed-width prefix is safe to slice; a base with nothing after the prefix
/// (shouldn't happen for our names) falls back to the whole base as the title.
fn split_base(base: &str) -> (String, String) {
    let chars: Vec<char> = base.chars().collect();
    let date: String = chars[0..10].iter().collect();
    let hh: String = chars[11..13].iter().collect();
    let mm: String = chars[13..15].iter().collect();
    let recorded_at = format!("{date} {hh}:{mm}");
    let title: String = chars.iter().skip(PREFIX_CHARS).collect();
    let title = if title.trim().is_empty() {
        base.to_string()
    } else {
        title
    };
    (title, recorded_at)
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core recordings::`
Expected: PASS (all six).

- [ ] **Step 7: Format, clippy, commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/recordings.rs src-tauri/core/src/lib.rs src-tauri/core/src/transcript.rs
git commit -m "feat(core): list a vault's recordings, grouped-ready by note type

Scan each recording root's YYYY/MM (capture .mp3s only, no symlink follow —
same discipline as pending_transcriptions), pairing each with its companion
note's type + duration and sorting newest-first. Reuses the transcript
scan's dir_entries/is_digit_dir helpers (now pub(crate)). Read-only."
```

---

### Task 4: `list_recordings` + `open_recording` commands (shell)

Wire the core scan to IPC, add the note-opening command, and DRY the two open-coded root matches onto `recording_roots()`. **Windows-only compile — verified by CI's `windows-app` job; run `cargo fmt --check` locally.**

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (add `recordings` to the core `use`; add `RecordingDto` + `list_recordings` command; extract `open_recording_note` helper and add the `open_recording` command; refactor two root-building loops)
- Modify: `src-tauri/src/lib.rs` (register `list_recordings`, `open_recording`)

**Interfaces:**
- Consumes: `vault_buddy_core::recordings::{RecordingEntry, list_recordings}`, `capture_config::VaultCaptureConfig::recording_roots`, existing `discovery`, `capture_paths::safe_recording_root`, `uri`, `transcript`.
- Produces: IPC commands `list_recordings(id: String) -> Vec<RecordingDto>` and `open_recording(path: String) -> Result<(), String>`; `RecordingDto { mp3, title, recordedAt, duration, type }` (camelCase, `type` via serde rename).

- [ ] **Step 1: Import the core module**

In `src-tauri/src/capture_commands.rs`, extend the existing core `use` (line ~10) to include `recordings`:

```rust
use vault_buddy_core::{capture_config, capture_paths, discovery, recordings, transcript, uri};
```

- [ ] **Step 2: Add the DTO and the `list_recordings` command**

In `src-tauri/src/capture_commands.rs`, add near the other DTO commands (e.g. after `list_audio_devices`):

```rust
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDto {
    pub mp3: String,
    pub title: String,
    pub recorded_at: String,
    pub duration: Option<String>,
    // `type` is a Rust keyword — expose the camelCase `type` the frontend wants.
    #[serde(rename = "type")]
    pub recording_type: Option<String>,
}

/// Read-only list of a vault's past recordings for the Recordings view.
/// Scans the vault's recording roots (custom folder, or both mode defaults)
/// and reads each recording's companion note for type/duration. An unknown
/// vault or unreadable roots yield an empty list — never an error (mirrors
/// discovery's degrade-to-empty rule). Never writes into the vault.
#[tauri::command]
pub fn list_recordings(id: String) -> Vec<RecordingDto> {
    let Some(vault) = discovery::discover_vaults().into_iter().find(|v| v.id == id) else {
        return Vec::new();
    };
    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let roots: Vec<PathBuf> = cfg
        .recording_roots()
        .into_iter()
        .filter_map(|folder| {
            capture_paths::safe_recording_root(Path::new(&vault.path), folder).ok()
        })
        .collect();
    recordings::list_recordings(&roots)
        .into_iter()
        .map(|e| RecordingDto {
            mp3: e.mp3_path.to_string_lossy().into_owned(),
            title: e.title,
            recorded_at: e.recorded_at,
            duration: e.duration,
            recording_type: e.recording_type,
        })
        .collect()
}
```

- [ ] **Step 3: Extract `open_recording_note` and add the `open_recording` command**

In `src-tauri/src/capture_commands.rs`, replace the existing `open_transcript` command (the last item in the file) with a shared helper plus two thin wrappers:

```rust
/// Shared by `open_transcript` and `open_recording`: launch an
/// `obsidian://open` for a recording's companion note `<base>.md` when it
/// exists (the richest view — it embeds the transcript and the audio player),
/// otherwise the `<base>.transcript.md` sidecar. Read-only: never writes into
/// the vault; the launch is logged by `uri::launch`, the same audit trail as
/// every other vault open.
fn open_recording_note(path: &str) -> Result<(), String> {
    let mp3 = PathBuf::from(path);
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

/// Open a finished recording's note (or transcript sidecar) — the
/// TranscriptionStatus "Open in Obsidian" row.
#[tauri::command]
pub fn open_transcript(path: String) -> Result<(), String> {
    open_recording_note(&path)
}

/// Open a recording's note from the Recordings list row.
#[tauri::command]
pub fn open_recording(path: String) -> Result<(), String> {
    open_recording_note(&path)
}
```

- [ ] **Step 4: DRY the two open-coded root matches onto `recording_roots()`**

In `src-tauri/src/capture_commands.rs`, in **`run_recovery`** replace:

```rust
                    let roots: Vec<String> = match &v.recording_folder {
                        Some(folder) => vec![folder.clone()],
                        None => vec!["Meetings".to_string(), "Voice Notes".to_string()],
                    };
                    for folder in roots {
                        let Ok(root) =
                            capture_paths::safe_recording_root(Path::new(&vault.path), &folder)
                        else {
                            log::warn!("recovery: skipping unsafe configured folder {folder:?}");
                            continue;
                        };
```

with:

```rust
                    for folder in v.recording_roots() {
                        let Ok(root) =
                            capture_paths::safe_recording_root(Path::new(&vault.path), folder)
                        else {
                            log::warn!("recovery: skipping unsafe configured folder {folder:?}");
                            continue;
                        };
```

And in **`scan_and_enqueue`** replace:

```rust
        let roots: Vec<String> = match &v.recording_folder {
            Some(folder) => vec![folder.clone()],
            None => vec!["Meetings".to_string(), "Voice Notes".to_string()],
        };
        for folder in roots {
            let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), &folder)
            else {
                continue;
            };
```

with:

```rust
        for folder in v.recording_roots() {
            let Ok(root) = capture_paths::safe_recording_root(Path::new(&vault.path), folder)
            else {
                continue;
            };
```

(Both now take `folder: &str` borrowed from `v`, which outlives the loop — `safe_recording_root` accepts `&str`.)

- [ ] **Step 5: Register the commands**

In `src-tauri/src/lib.rs`, in the `tauri::generate_handler![…]` list, add after `capture_commands::open_transcript,`:

```rust
            capture_commands::list_recordings,
            capture_commands::open_recording,
```

- [ ] **Step 6: Format check + commit**

`cargo build`/`clippy` for the shell crate can't run here (no webkit2gtk on Linux) — CI's `windows-app` job is the compile gate. Only fmt is checkable locally:

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): list_recordings + open_recording IPC commands

list_recordings scans a vault's recording roots (via the new
recording_roots) and returns title/date/duration/type DTOs for the
Recordings view; open_recording opens a row's companion note in Obsidian,
sharing open_transcript's read-only note-launch helper. Also folds recovery
and transcription backfill onto recording_roots. Windows compile via CI."
```

---

### Task 5: `recordings` panel view state + `Recording` type (frontend store)

Add the store view (mirroring `captureSettings`) and the IPC row type. Small, isolated, unit-tested.

**Files:**
- Modify: `src/types.ts` (add `Recording`)
- Modify: `src/stores/vaults.ts` (extend `view`, add `recordingsVaultId`, `openRecordings`, clear in `showList`)
- Modify: `tests/vaults-store.test.ts` (add view-transition tests)

**Interfaces:**
- Produces: `Recording` interface; store state `view` gains `"recordings"`, new `recordingsVaultId: string | null`, action `openRecordings(vaultId: string)`; `showList()` clears `recordingsVaultId`.
- Consumed by: Task 6 (`Recordings.vue`), Task 7 (`ActionPanel.vue`).

- [ ] **Step 1: Write the failing store tests**

In `tests/vaults-store.test.ts`, inside `describe("vaults store", …)`, add:

```rust
  it("openRecordings switches to the recordings view for a vault", () => {
    const store = useVaultsStore();
    store.openRecordings("a1b2c3");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("a1b2c3");
  });

  it("showList clears the recordings vault id", () => {
    const store = useVaultsStore();
    store.openRecordings("a1b2c3");
    store.showList();
    expect(store.view).toBe("list");
    expect(store.recordingsVaultId).toBe(null);
  });
```

(Ignore the ```rust fence — this is TypeScript; it's inside a `.ts` test file.)

- [ ] **Step 2: Run the tests to verify they fail**

Run (from repo root): `npx vitest run tests/vaults-store.test.ts`
Expected: FAIL — `openRecordings` is not a function.

- [ ] **Step 3: Add the `Recording` type**

In `src/types.ts`, append:

```ts
export interface Recording {
  mp3: string;
  title: string;
  recordedAt: string;
  /** From the companion note's frontmatter; null when there's no note. */
  duration: string | null;
  /** Recording type from the companion note; null → "Ungrouped". */
  type: string | null;
}
```

- [ ] **Step 4: Extend the store**

In `src/stores/vaults.ts`:

(a) Widen the `view` union and add `recordingsVaultId` (right after `captureSettingsVaultId`):

```ts
    view: "list" as "list" | "settings" | "captureSettings" | "recordings",
    // Which vault the captureSettings view edits.
    captureSettingsVaultId: null as string | null,
    // Which vault the recordings view lists.
    recordingsVaultId: null as string | null,
```

(b) Clear it in `showList`:

```ts
    showList() {
      this.view = "list";
      this.captureSettingsVaultId = null;
      this.recordingsVaultId = null;
    },
```

(c) Add the action (after `openCaptureSettings`):

```ts
    openRecordings(vaultId: string) {
      this.view = "recordings";
      this.recordingsVaultId = vaultId;
    },
```

- [ ] **Step 5: Run the tests to verify they pass**

Run (from repo root): `npx vitest run tests/vaults-store.test.ts`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/types.ts src/stores/vaults.ts tests/vaults-store.test.ts
git commit -m "feat(ui): add the recordings panel view state and Recording type

Mirrors the captureSettings view: a store-held view + recordingsVaultId,
openRecordings action, and showList clears it. Plus the Recording IPC row
type the list command returns."
```

---

### Task 6: `Recordings.vue` — the list view (frontend)

The component: fetch `list_recordings` on mount, group by type (Ungrouped last) with a per-view flat/grouped toggle, and open a row's note (closing the panel). One row markup, driven by a `sections` computed so grouped and flat share it.

**Files:**
- Create: `src/components/Recordings.vue`
- Create: `tests/recordings.test.ts`

**Interfaces:**
- Consumes: `list_recordings` / `open_recording` IPC (mocked in tests); `Recording` type; `useVaultsStore` (`panelOpen`).
- Produces: `<Recordings :vault-id="…" />` (single required prop `vaultId: string`). Used by Task 7.

- [ ] **Step 1: Write the failing component tests**

Create `tests/recordings.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import Recordings from "../src/components/Recordings.vue";
import { useVaultsStore } from "../src/stores/vaults";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const sample = [
  { mp3: "C:/v/Meetings/2026/07/a.mp3", title: "Standup", recordedAt: "2026-07-04 14:05", duration: "1:05", type: "Meeting" },
  { mp3: "C:/v/Voice Notes/2026/07/b.mp3", title: "Idea", recordedAt: "2026-07-04 09:00", duration: "0:30", type: "Voice Note" },
  { mp3: "C:/v/Meetings/2026/07/c.mp3", title: "Orphan", recordedAt: "2026-07-03 10:00", duration: null as string | null, type: null as string | null },
];

const mountView = async (
  opts: { list?: unknown; onOpen?: (args: unknown) => unknown } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "list_recordings") return opts.list ?? sample;
    if (cmd === "open_recording") return opts.onOpen?.(args);
  });
  const wrapper = mount(Recordings, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("Recordings", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("fetches list_recordings for the vault", async () => {
    const { calls } = await mountView();
    expect(calls[0]).toEqual({ cmd: "list_recordings", args: { id: "v1" } });
  });

  it("groups by type with Ungrouped last", async () => {
    const { wrapper } = await mountView();
    const headers = wrapper.findAll("h2").map((h) => h.text());
    expect(headers[0]).toContain("Meeting");
    expect(headers[1]).toContain("Voice Note");
    expect(headers[headers.length - 1]).toContain("Ungrouped");
    expect(wrapper.text()).toContain("Standup");
    expect(wrapper.text()).toContain("—"); // null duration renders a dash
  });

  it("toggles to a flat list, hiding the type headers", async () => {
    const { wrapper } = await mountView();
    await wrapper.get('[data-testid="group-toggle"]').trigger("click");
    expect(wrapper.findAll("h2")).toHaveLength(0);
    expect(wrapper.findAll('[data-testid="recording-row"]')).toHaveLength(3);
  });

  it("shows an empty state when there are no recordings", async () => {
    const { wrapper } = await mountView({ list: [] });
    expect(wrapper.text()).toContain("No recordings yet.");
  });

  it("opens a recording and closes the panel", async () => {
    const { wrapper, calls } = await mountView();
    const store = useVaultsStore();
    store.panelOpen = true;
    await wrapper.findAll('[data-testid="recording-row"]')[0].trigger("click");
    await flushPromises();
    const open = calls.find((c) => c.cmd === "open_recording");
    expect(open?.args).toEqual({ mp3: sample[0].mp3 }); // first row = Meeting/Standup
    expect(store.panelOpen).toBe(false);
  });

  it("surfaces a load error", async () => {
    const { wrapper } = await mountView({ list: undefined });
    // override with a throwing mock
    clearMocks();
    mockIPC((cmd) => {
      if (cmd === "list_recordings") throw new Error("scan boom");
    });
    const w = mount(Recordings, { props: { vaultId: "v1" } });
    await flushPromises();
    expect(w.text()).toContain("scan boom");
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from repo root): `npx vitest run tests/recordings.test.ts`
Expected: FAIL — cannot resolve `../src/components/Recordings.vue`.

- [ ] **Step 3: Implement `Recordings.vue`**

Create `src/components/Recordings.vue`:

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { useVaultsStore } from "../stores/vaults";
import { logWarning } from "../logging";
import type { Recording } from "../types";

const props = defineProps<{ vaultId: string }>();
const store = useVaultsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const recordings = ref<Recording[]>([]);
// Per-view, not persisted: resets to grouped every time the view opens.
const grouped = ref(true);

const UNGROUPED = "Ungrouped";

// One shape drives both modes: flat mode is a single header-less section;
// grouped mode is one section per type with Ungrouped forced last. Recordings
// arrive newest-first, so each section's rows stay newest-first.
const sections = computed<Array<{ type: string | null; items: Recording[] }>>(() => {
  if (!grouped.value) return [{ type: null, items: recordings.value }];
  const map = new Map<string, Recording[]>();
  for (const r of recordings.value) {
    const key = r.type ?? UNGROUPED;
    const list = map.get(key);
    if (list) list.push(r);
    else map.set(key, [r]);
  }
  return [...map.entries()]
    .sort(([a], [b]) => (a === UNGROUPED ? 1 : b === UNGROUPED ? -1 : 0))
    .map(([type, items]) => ({ type, items }));
});

onMounted(async () => {
  try {
    recordings.value = await invoke<Recording[]>("list_recordings", {
      id: props.vaultId,
    });
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

async function open(mp3: string) {
  try {
    await invoke("open_recording", { mp3 });
    store.panelOpen = false; // Obsidian takes over — get out of the way
  } catch (e) {
    // A failed open (recording moved, launch error) is non-fatal — surface it
    // and keep the list so the user can pick another.
    loadError.value = String(e);
    logWarning(`open recording rejected: ${String(e)}`);
  }
}
</script>

<template>
  <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
  <p
    v-else-if="loadError"
    class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
  >
    {{ loadError }}
  </p>
  <p v-else-if="recordings.length === 0" class="text-xs text-slate-400">
    No recordings yet.
  </p>
  <div v-else class="flex flex-col gap-2">
    <div class="flex items-center justify-between">
      <span class="text-xs text-slate-400">
        {{ recordings.length }} recording{{ recordings.length === 1 ? "" : "s" }}
      </span>
      <button
        type="button"
        data-testid="group-toggle"
        class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :aria-pressed="grouped"
        @click="grouped = !grouped"
      >
        {{ grouped ? "Grouped by type" : "Flat list" }}
      </button>
    </div>
    <section
      v-for="(section, i) in sections"
      :key="section.type ?? `flat-${i}`"
    >
      <h2
        v-if="section.type"
        class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-400"
      >
        {{ section.type }}
        <span class="text-slate-500">· {{ section.items.length }}</span>
      </h2>
      <div class="flex flex-col gap-1">
        <button
          v-for="r in section.items"
          :key="r.mp3"
          type="button"
          data-testid="recording-row"
          class="flex w-full items-baseline justify-between gap-2 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="open(r.mp3)"
        >
          <span class="min-w-0 flex-1 truncate text-sm text-slate-100">
            {{ r.title }}
          </span>
          <span class="shrink-0 text-xs text-slate-400">{{ r.recordedAt }}</span>
          <span class="shrink-0 text-xs text-slate-500">{{ r.duration ?? "—" }}</span>
        </button>
      </div>
    </section>
  </div>
</template>
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from repo root): `npx vitest run tests/recordings.test.ts`
Expected: PASS (all six).

- [ ] **Step 5: Commit**

```bash
git add src/components/Recordings.vue tests/recordings.test.ts
git commit -m "feat(ui): Recordings list view — grouped by type, read-only

Fetches list_recordings on mount, groups by the note-derived type (Ungrouped
last) with a per-view flat/grouped toggle that isn't persisted, and opens a
row's note in Obsidian (closing the panel). One row markup drives both modes
via a sections computed."
```

---

### Task 7: Wire the entry point (frontend)

The "Browse recordings" option in `RecordModeDialog` and the `ActionPanel` plumbing (title, view slot, `browse` handler) that connect the dialog to the view.

**Files:**
- Modify: `src/components/RecordModeDialog.vue` (add `browse` emit + button)
- Modify: `src/components/ActionPanel.vue` (import `Recordings`; title case; view slot; `browse` handler)
- Modify: `tests/record-mode-dialog.test.ts` (assert `browse` emit)
- Modify: `tests/action-panel.test.ts` (assert dialog→recordings navigation + the view renders)

**Interfaces:**
- Consumes: `store.openRecordings` (Task 5), `Recordings.vue` (Task 6), the existing `recordRequest` dialog flow.
- Produces: end-to-end entry — record dialog "Browse recordings" → `view: 'recordings'` rendering `<Recordings>`.

- [ ] **Step 1: Write the failing tests**

In `tests/record-mode-dialog.test.ts`, add inside the `describe`:

```ts
  it("emits browse when the Browse recordings option is clicked", async () => {
    const wrapper = mount(RecordModeDialog, {
      props: { vaultName: "Personal", defaultMode: "meeting" },
    });
    await wrapper.get('[data-testid="mode-browse"]').trigger("click");
    expect(wrapper.emitted("browse")).toHaveLength(1);
    // browsing is not starting a recording
    expect(wrapper.emitted("start")).toBeUndefined();
  });
```

In `tests/action-panel.test.ts`, add (the file already imports `mount`, `mockIPC`, `useVaultsStore`; add `flushPromises` to the `@vue/test-utils` import if not present):

```ts
  it("navigates to the recordings view from the record dialog", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_capture_config")
        return {
          mode: "meeting",
          recordingFolder: null,
          bitrateKbps: 128,
          createNote: true,
          inputDevice: null,
          outputDevice: null,
          transcribe: false,
          transcriptionModel: "small",
          transcriptionLanguage: null,
          transcriptTimestamps: true,
        };
      if (cmd === "list_recordings") return [];
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper
      .get('[aria-label="Capture knowledge in Personal"]')
      .trigger("click");
    await flushPromises(); // openRecordDialog awaits get_capture_config
    await wrapper.get('[data-testid="mode-browse"]').trigger("click");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("d4e5f6");
  });

  it("renders the Recordings view with its title", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_recordings") return [];
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.openRecordings("d4e5f6");
    const wrapper = mount(ActionPanel);
    await flushPromises();
    expect(wrapper.get("h1").text()).toBe("Recordings");
    expect(wrapper.text()).toContain("No recordings yet.");
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from repo root):
`npx vitest run tests/record-mode-dialog.test.ts tests/action-panel.test.ts`
Expected: FAIL — no `[data-testid="mode-browse"]`; `store.view` never becomes `recordings`.

- [ ] **Step 3: Add the `browse` option to `RecordModeDialog.vue`**

In `src/components/RecordModeDialog.vue`:

(a) Add `browse` to the emits:

```ts
const emit = defineEmits<{
  (e: "start", mode: "meeting" | "voice-note"): void;
  (e: "browse"): void;
  (e: "cancel"): void;
}>();
```

(b) Add the button immediately after the closing `</div>` of the options `<div class="flex flex-col gap-2">` block (still inside the `<div class="w-64 …">` card):

```vue
      <button
        type="button"
        data-testid="mode-browse"
        aria-label="Browse past recordings"
        class="mt-2 w-full cursor-pointer border-t border-white/10 pt-2 text-left text-xs text-slate-400 transition-colors hover:text-slate-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="emit('browse')"
      >
        Browse recordings…
        <span class="block text-slate-500">See past recordings in this vault</span>
      </button>
```

- [ ] **Step 4: Wire `ActionPanel.vue`**

In `src/components/ActionPanel.vue`:

(a) Import the view component (after the other component imports, ~line 13):

```ts
import Recordings from "./Recordings.vue";
```

(b) Add the `browse` handler (after `startWithMode`, ~line 78):

```ts
function browseRecordings() {
  const request = recordRequest.value;
  recordRequest.value = null;
  if (request) store.openRecordings(request.vaultId);
}
```

(c) Extend the header title (the `<h1>` ternary, ~lines 87–93):

```vue
        {{
          view === "settings"
            ? "Buddy settings"
            : view === "captureSettings"
              ? "Capture settings"
              : view === "recordings"
                ? "Recordings"
                : "Vaults"
        }}
```

(d) Add the view slot — insert between the `captureSettings` block and the `<div v-else>` VaultList block (after the closing `</div>` at ~line 195):

```vue
    <div
      v-else-if="view === 'recordings' && store.recordingsVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <Recordings
        :key="store.recordingsVaultId"
        :vault-id="store.recordingsVaultId"
      />
    </div>
```

(e) Handle the dialog's `browse` — on the `<RecordModeDialog>` element (~line 218) add the listener:

```vue
    <RecordModeDialog
      v-if="recordRequest"
      :vault-name="recordRequest.vaultName"
      :default-mode="recordRequest.defaultMode"
      @start="startWithMode($event)"
      @browse="browseRecordings"
      @cancel="recordRequest = null"
    />
```

- [ ] **Step 5: Run the tests to verify they pass**

Run (from repo root):
`npx vitest run tests/record-mode-dialog.test.ts tests/action-panel.test.ts`
Expected: PASS.

- [ ] **Step 6: Full suite + typecheck**

Run (from repo root): `npm test && npm run build`
Expected: full Vitest suite green; `vue-tsc` typecheck clean.

- [ ] **Step 7: Commit**

```bash
git add src/components/RecordModeDialog.vue src/components/ActionPanel.vue tests/record-mode-dialog.test.ts tests/action-panel.test.ts
git commit -m "feat(ui): reach the Recordings view from the Start Recording modal

Adds a Browse recordings option to RecordModeDialog that navigates to the
new recordings panel view (rather than starting a recording), and wires
ActionPanel to render it with the Recordings title. Entry point lives in the
record modal, not a new vault-row icon."
```

---

## Part 2 — Follow-up template in the companion note

An approved, independent feature folded into this increment (see the spec's
Addendum). A per-vault `follow_up_template` setting (default **on**, opt-out)
appends a `## Follow-up` scaffold — Action items / Decisions / Notes — to each
recording's companion note, above the `## Transcript` embed.

**Part 2 constraints:**
- **No new vault-write path** — the scaffold is extra content in the companion
  note the capture path already writes atomically (`write_note_collision_safe`).
- **Gated by `create_note`** — `render_note` runs only when a note is written.
- **Default on**; **recovered notes stay minimal** (`follow_up: false` hardcoded
  in recovery — no `recover_root` signature change).
- **Cross-crate compile note:** Task 8 adds a required `NoteMeta.follow_up`
  field, so the `capture` crate stops compiling until Task 10 sets it at its two
  literals. Tasks 8–9 verify with `cargo test -p vault_buddy_core` (core builds
  standalone); the workspace is whole again after Task 10.

---

### Task 8: `## Follow-up` scaffold in `render_note` (core)

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs` (add `NoteMeta.follow_up`; emit the block in `render_note`; update the `meta()` test helper; add tests)

**Interfaces:**
- Produces: `NoteMeta.follow_up: bool`; `render_note` emits `## Follow-up` (Action items / Decisions / Notes) before any `## Transcript` block when true.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/core/src/capture_note.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn note_includes_follow_up_template_when_enabled() {
        let mut m = meta();
        m.follow_up = true;
        let note = render_note(&m, "2026-07-04 1405 Meeting.mp3");
        assert!(note.contains("## Follow-up"));
        assert!(note.contains("### Action items"));
        assert!(note.contains("- [ ]"));
        assert!(note.contains("### Decisions"));
        assert!(note.contains("### Notes"));
        // the audio embed still sits above the scaffold
        assert!(note.contains("![[2026-07-04 1405 Meeting.mp3]]"));
    }

    #[test]
    fn note_has_no_follow_up_when_disabled() {
        // meta() sets follow_up: false (Step 3d), so the default note omits it.
        assert!(!render_note(&meta(), "x.mp3").contains("## Follow-up"));
    }

    #[test]
    fn follow_up_sits_above_the_transcript() {
        let mut m = meta();
        m.follow_up = true;
        m.transcribe = true;
        let note = render_note(&m, "2026-07-04 1405 Meeting.mp3");
        let fu = note.find("## Follow-up").unwrap();
        let tr = note.find("## Transcript").unwrap();
        assert!(fu < tr, "follow-up must render above the transcript embed: {note}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core capture_note`
Expected: FAIL to compile — `NoteMeta` has no field `follow_up`.

- [ ] **Step 3: Implement**

In `src-tauri/core/src/capture_note.rs`:

(a) Add the field to `NoteMeta` (after `transcribe`):

```rust
    pub transcribe: bool,
    /// Append a `## Follow-up` scaffold (Action items / Decisions / Notes)
    /// above the transcript embed. Per-vault opt-out; recovery leaves it off.
    pub follow_up: bool,
```

(b) In `render_note`, insert the block between the audio embed and the transcript block. Change the tail from:

```rust
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
    if meta.transcribe {
```

to:

```rust
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
    if meta.follow_up {
        // A follow-up scaffold above the (possibly long) transcript embed so
        // the actionable part is visible without scrolling. Static text — the
        // rename retarget only rewrites the ![[…]] embed line, never this.
        out.push_str(
            "\n## Follow-up\n\n### Action items\n\n- [ ] \n\n### Decisions\n\n### Notes\n",
        );
    }
    if meta.transcribe {
```

(c) Update the `meta()` test helper — add `follow_up: false` (after `transcribe: false`), so every existing `render_note` assertion (which doesn't expect the scaffold) still holds:

```rust
            transcribe: false,
            follow_up: false,
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core capture_note`
Expected: PASS (new + existing note tests). The `capture` crate won't build until Task 10 — that's expected; do not run a workspace-wide build here.

- [ ] **Step 5: Format, clippy, commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_note.rs
git commit -m "feat(core): add an optional follow-up scaffold to the companion note

render_note appends a ## Follow-up section (Action items / Decisions / Notes)
above the transcript embed when NoteMeta.follow_up is set. Static text, so
the rename embed-retarget never touches it; gated by the caller so a note
without create_note never gets one."
```

---

### Task 9: `follow_up_template` config field (core)

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs` (struct field, `Default`, `vault_entry` parse, `serialize_config`; add tests)

**Interfaces:**
- Produces: `VaultCaptureConfig.follow_up_template: bool` (default `true`), parsed from/serialized to `followUpTemplate`.

- [ ] **Step 1: Write the failing tests**

In `src-tauri/core/src/capture_config.rs`, inside `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn follow_up_template_defaults_on_and_parses() {
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": {},
                "b": { "followUpTemplate": false }
            } }"#,
        );
        assert!(vault_config(&cfg, "a").follow_up_template, "default on");
        assert!(!vault_config(&cfg, "b").follow_up_template);
    }

    #[test]
    fn follow_up_template_survives_a_round_trip() {
        let mut v = VaultCaptureConfig::default();
        v.follow_up_template = false;
        let mut cfg = AppConfig::default();
        cfg.vaults.insert("a".into(), v);
        let reparsed = parse_config(&serialize_config(&cfg));
        assert!(!vault_config(&reparsed, "a").follow_up_template);
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core follow_up_template`
Expected: FAIL to compile — no field `follow_up_template`.

- [ ] **Step 3: Implement**

In `src-tauri/core/src/capture_config.rs`:

(a) Add the field to `VaultCaptureConfig` (after `transcript_timestamps`):

```rust
    pub transcript_timestamps: bool,
    pub follow_up_template: bool,
```

(b) Add to `Default` (after `transcript_timestamps: true,`):

```rust
            transcript_timestamps: true,
            follow_up_template: true,
```

(c) Add to `vault_entry` (after the `transcript_timestamps` field):

```rust
        transcript_timestamps: entry
            .get("transcriptTimestamps")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.transcript_timestamps),
        follow_up_template: entry
            .get("followUpTemplate")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.follow_up_template),
```

(d) Add to `serialize_config` (after the `transcriptTimestamps` insert):

```rust
        entry.insert(
            "transcriptTimestamps".to_string(),
            json!(v.transcript_timestamps),
        );
        entry.insert(
            "followUpTemplate".to_string(),
            json!(v.follow_up_template),
        );
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test -p vault_buddy_core capture_config`
Expected: PASS (new + existing config tests).

- [ ] **Step 5: Format, clippy, commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): per-vault follow_up_template setting (default on)

Parsed per-field defensively (followUpTemplate) and round-tripped in
serialize_config, like every other capture setting. Drives whether the
companion note gets a ## Follow-up scaffold."
```

---

### Task 10: thread `follow_up` through the capture crate

Makes the workspace whole again (Task 8 added the required `NoteMeta.follow_up`) and wires the recording save path. The `capture` crate compiles/tests on Linux **with `libasound2-dev`** (CI installs it; locally `sudo apt-get install -y libasound2-dev` if `cargo` reports a missing `alsa`).

**Files:**
- Modify: `src-tauri/capture/src/session.rs` (`SessionParams.follow_up`; finalize `NoteMeta`; `params()` test helper)
- Modify: `src-tauri/capture/src/recovery.rs` (recovery `NoteMeta`: `follow_up: false`)

**Interfaces:**
- Consumes: `NoteMeta.follow_up` (Task 8).
- Produces: `SessionParams.follow_up: bool` (consumed by the shell in Task 11).

- [ ] **Step 1: Add the field to `SessionParams`**

In `src-tauri/capture/src/session.rs`, in `pub struct SessionParams` (after `pub transcribe: bool,`):

```rust
    pub transcribe: bool,
    pub follow_up: bool,
```

- [ ] **Step 2: Set it on the finalize `NoteMeta`**

In `src-tauri/capture/src/session.rs`, in the `if params.create_note` block, the `NoteMeta` literal — after `transcribe: params.transcribe,`:

```rust
            transcribe: params.transcribe,
            follow_up: params.follow_up,
```

- [ ] **Step 3: Set the two remaining literals**

In `src-tauri/capture/src/session.rs`, the `params()` test helper (after `transcribe: false,`) — neutral `false` so existing note-content assertions are unchanged:

```rust
            transcribe: false,
            follow_up: false,
```

In `src-tauri/capture/src/recovery.rs`, the recovery `NoteMeta` literal (after `transcribe,`) — recovered notes stay minimal:

```rust
                transcribe,
                // Recovered notes are intentionally minimal (no recorded_at,
                // devices, or duration); skip the follow-up scaffold too.
                follow_up: false,
```

- [ ] **Step 4: Build + run the capture tests**

Run (from `src-tauri/`): `cargo test -p vault_buddy_capture`
Expected: PASS (the crate compiles with the new field and all existing session/recovery tests pass). If `cargo` errors that `alsa`/`libasound2` is missing, install `libasound2-dev` first (CI already has it).

- [ ] **Step 5: Format, commit**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/capture/src/session.rs src-tauri/capture/src/recovery.rs
git commit -m "feat(capture): thread the follow-up flag into the saved note

SessionParams carries follow_up into the finalize NoteMeta so a normally
saved recording honors the vault setting; recovered notes stay minimal
(follow_up: false)."
```

---

### Task 11: config DTO + save/start wiring (shell)

**Windows-only compile — verified by CI's `windows-app` job; run `cargo fmt --check` locally.**

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`CaptureConfigDto` field + `from_config`; `set_capture_config` `VaultCaptureConfig` literal; `start_capture` `SessionParams` literal)

**Interfaces:**
- Consumes: `VaultCaptureConfig.follow_up_template` (Task 9), `SessionParams.follow_up` (Task 10).
- Produces: `CaptureConfigDto.follow_up_template` (camelCase `followUpTemplate` over IPC).

- [ ] **Step 1: DTO field + mapping**

In `src-tauri/src/capture_commands.rs`, in `pub struct CaptureConfigDto` (after `transcript_timestamps: bool,`):

```rust
    pub transcript_timestamps: bool,
    pub follow_up_template: bool,
```

And in `CaptureConfigDto::from_config` (after `transcript_timestamps: v.transcript_timestamps,`):

```rust
            transcript_timestamps: v.transcript_timestamps,
            follow_up_template: v.follow_up_template,
```

- [ ] **Step 2: Persist it in `set_capture_config`**

In `src-tauri/src/capture_commands.rs`, in `set_capture_config`, the `VaultCaptureConfig { … }` value (after `transcript_timestamps: cfg.transcript_timestamps,`):

```rust
        transcript_timestamps: cfg.transcript_timestamps,
        follow_up_template: cfg.follow_up_template,
```

- [ ] **Step 3: Pass it into the recording session**

In `src-tauri/src/capture_commands.rs`, in `start_capture`, the `SessionParams { … }` literal (after `transcribe: cfg.transcribe,`):

```rust
                transcribe: cfg.transcribe,
                follow_up: cfg.follow_up_template,
```

- [ ] **Step 4: Format check + commit**

```bash
cd src-tauri && cargo fmt --check && cd ..
git add src-tauri/src/capture_commands.rs
git commit -m "feat(shell): carry follow_up_template through config IPC and into capture

get/set_capture_config round-trip the setting; start_capture passes it to
SessionParams so a new recording's note gets the scaffold. Windows compile
via CI."
```

---

### Task 12: settings toggle (frontend)

The UI: a "Follow-up template" toggle nested under "Companion note" (shown only when the note is enabled), default on.

**Files:**
- Modify: `src/types.ts` (`CaptureConfig.followUpTemplate`)
- Modify: `src/components/CaptureSettings.vue` (ref + load + save + watch + nested toggle)
- Modify: `tests/capture-settings.test.ts` (fixture field + a save test)

**Interfaces:**
- Consumes: `list`/`get`/`set_capture_config` IPC (mocked); the `followUpTemplate` DTO field (Task 11).

- [ ] **Step 1: Write the failing test**

In `tests/capture-settings.test.ts`, add `followUpTemplate: true,` to the `config` fixture (after `transcriptTimestamps: true,`), then add:

```ts
  it("saves the follow-up template toggle", async () => {
    let saved: { cfg: { followUpTemplate: boolean } } | undefined;
    const { wrapper } = await mountLoaded({
      onSet: (args) => {
        saved = args as typeof saved;
      },
    });
    await wrapper.get('[data-testid="follow-up-toggle"]').setValue(false);
    await wrapper.get('[data-testid="save-button"]').trigger("click");
    await flushPromises();
    expect(saved?.cfg.followUpTemplate).toBe(false);
  });
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from repo root): `npx vitest run tests/capture-settings.test.ts`
Expected: FAIL — no `[data-testid="follow-up-toggle"]`.

- [ ] **Step 3: Implement**

(a) In `src/types.ts`, add to `CaptureConfig` (after `transcriptTimestamps: boolean;`):

```ts
  transcriptTimestamps: boolean;
  followUpTemplate: boolean;
```

(b) In `src/components/CaptureSettings.vue` `<script setup>`:

Add the ref (after `const createNote = ref(true);`):

```ts
const createNote = ref(true);
const followUpTemplate = ref(true);
```

Add to the edit-invalidates-Saved watch array (after `createNote,`):

```ts
    createNote,
    followUpTemplate,
```

Load it in `onMounted` (after `createNote.value = cfg.createNote;`):

```ts
    createNote.value = cfg.createNote;
    followUpTemplate.value = cfg.followUpTemplate;
```

Save it in `save()`'s `cfg` payload (after `createNote: createNote.value,`):

```ts
        createNote: createNote.value,
        followUpTemplate: followUpTemplate.value,
```

(c) In the `<template>`, add the nested toggle right after the "Companion note" `</section>` (the `create-note` block) and before the "Transcribe recordings" section:

```vue
    <div
      v-if="createNote"
      class="flex items-center justify-between border-l border-white/10 pl-3"
    >
      <label for="capture-follow-up-toggle" class="text-sm text-slate-200">
        Follow-up template
        <span class="block text-xs text-slate-500">Action items · Decisions · Notes</span>
      </label>
      <input
        id="capture-follow-up-toggle"
        v-model="followUpTemplate"
        data-testid="follow-up-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
      />
    </div>
```

- [ ] **Step 4: Run the test, then the full suite + typecheck**

Run (from repo root): `npx vitest run tests/capture-settings.test.ts` → PASS.
Then: `npm test && npm run build` → full Vitest suite green; `vue-tsc` clean.

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/components/CaptureSettings.vue tests/capture-settings.test.ts
git commit -m "feat(ui): follow-up template toggle in capture settings

Nested under Companion note (shown only when the note is enabled, since the
scaffold lives in that note), default on — mirrors how transcription
sub-options nest under Transcribe."
```

---

## Self-Review

**Spec coverage:**
- Entry = third option in RecordModeDialog, navigates (not records) → Task 7 (`browse` emit + `browseRecordings`). ✅
- New panel view, not modal → Task 5 (`view: 'recordings'`) + Task 7 (ActionPanel slot). ✅
- Per-vault scope → Task 4 (`list_recordings(id)` resolves one vault) + Task 5 (`recordingsVaultId`). ✅
- Group by note `type`, Ungrouped fallback → Task 1 (`note_field`), Task 3 (`recording_type`), Task 6 (`sections`). ✅
- Toggle per-view, not persisted → Task 6 (`grouped` local ref). ✅
- Rows title—date—duration → Task 3 (`split_base`, note duration) + Task 6 row markup. ✅
- Open row → note via `obsidian://`, close panel, read-only → Task 4 (`open_recording`) + Task 6 (`open`). ✅
- `recording_roots` (custom or both defaults) → Task 2, used Task 4. ✅
- Scan discipline / degrade-not-error → Task 3 (dir_entries, is_capture_base) + Task 4 (empty-on-missing). ✅
- Empty state / load error → Task 6 tests + template. ✅
- *(Part 2)* `## Follow-up` scaffold above transcript → Task 8 (`render_note`). ✅
- *(Part 2)* `follow_up_template` setting, default on, defensive parse + round-trip → Task 9. ✅
- *(Part 2)* Threaded into the saved note; recovered notes minimal → Task 10. ✅
- *(Part 2)* Config IPC + `start_capture` wiring → Task 11. ✅
- *(Part 2)* Settings toggle nested under Companion note, default on → Task 12. ✅
- *(Part 2)* No new vault-write path / gated by `create_note` → Task 8 emits only when a note is written; Task 12 hides the toggle when `createNote` is off. ✅

**Placeholder scan:** none — every code step is concrete. The `todo!()` in Task 3 Step 3 is deliberate red-phase scaffolding, replaced in Step 5.

**Type consistency:** `note_field(&str,&str)->Option<String>` (T1) consumed in T3 `entry_for`. `recording_roots(&self)->Vec<&str>` (T2) consumed in T4 (`filter_map(|folder| safe_recording_root(_, folder))`) and the two refactors. `RecordingEntry` fields (T3) map 1:1 to `RecordingDto` camelCase (T4) → `Recording` TS interface (T5: `mp3,title,recordedAt,duration,type`) consumed by `Recordings.vue` (T6). Store `openRecordings`/`recordingsVaultId`/`view:'recordings'` (T5) consumed by `ActionPanel` (T7) and `panelOpen` by `Recordings.vue` (T6). `browse` emit (T7 dialog) handled by `browseRecordings` (T7 panel). Command names `list_recordings`/`open_recording` consistent shell (T4) ↔ frontend invokes (T6). *(Part 2)* `follow_up` bool chain: `NoteMeta.follow_up` (T8) ← `SessionParams.follow_up` (T10) ← `cfg.follow_up_template` in `start_capture` (T11); `VaultCaptureConfig.follow_up_template` (T9) ↔ `CaptureConfigDto.follow_up_template` (T11) ↔ `CaptureConfig.followUpTemplate` TS (T12); config key `followUpTemplate` consistent parse/serialize (T9). ✅
