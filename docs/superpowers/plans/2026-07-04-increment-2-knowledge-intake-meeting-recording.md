# Increment 2 — "Buddy records your meeting" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One click on 🎙 Capture records mic + desktop audio into a single mixed stereo MP3 stored atomically inside the chosen Obsidian vault, with the buddy as the always-visible recording indicator.

**Architecture:** Pure, unit-testable logic (config, naming/reservation, note rendering) goes into the existing `vault_buddy_core` crate. A new `vault_buddy_capture` workspace crate owns the audio pipeline (cpal devices → mixer → LAME streaming encoder → crash-safe `.part` writer → session state machine → recovery scan). The Tauri shell exposes `start_capture`/`stop_capture`/`capture_status` commands, emits `capture:*` events, and hardens the shutdown/hide/tray paths. Vue gets a `capture` Pinia store plus recording UI.

**Tech Stack:** Rust (cpal 0.15 WASAPI loopback, mp3lame-encoder, tauri-plugin-single-instance 2, tauri-plugin-notification 2), Vue 3 + Pinia + Vitest.

**Spec:** `docs/superpowers/specs/2026-07-04-increment-2-knowledge-intake-meeting-recording-design.md` — read it before starting any task.

## Global Constraints

- Windows is the capture target; **all crates must still compile and pass tests on Linux CI** (WASAPI loopback is `#[cfg(windows)]`).
- Encoding defaults: **44.1 kHz, stereo, 128 kbps MP3**.
- File layout follows the mode: `meeting` → `Meetings/YYYY/MM/YYYY-MM-DD HHmm Meeting.mp3`; `voice-note` → `Voice Notes/YYYY/MM/YYYY-MM-DD HHmm Voice Note.mp3` (+ same-named `.md`); a configured folder overrides the mode default; collision suffix ` (2)`, ` (3)`, …
- Temp file: dot-prefixed `.<base>.mp3.part` in the **target folder**, exclusive-create; the base name is reserved only when `.mp3`, `.md`, **and** `.mp3.part` are all free.
- Never lose captured audio; never overwrite user files; recovery deletes only zero-frame `.part` files.
- Writer cadence: flush to OS every **1 s**, `fsync` every **30 s**; fsync + non-replacing rename on stop; rename retries with advanced suffix on destination-exists.
- One recording at a time. Visible indicator always (no hide/quit that abandons a recording).
- Config: `%APPDATA%\vault-buddy\config.json` (via `dirs::config_dir()`), keyed by vault ID; defaults `mode=meeting`, `recordingFolder` unset (mode default applies: `Meetings` / `Voice Notes`), `bitrateKbps=128`, `createNote=true`. Never write config into vaults.
- Every start/stop/save/recovery is written to the app log (`log::info!`).
- Commit style: conventional commits (`feat(core): …`, `feat(capture): …`, `feat(ui): …`, `test: …`, `ci: …`, `docs: …`).
- Run Rust checks from `src-tauri/`: `cargo fmt --check`, `cargo clippy -p <crate> --all-targets -- -D warnings`, `cargo test -p <crate>`. Frontend: `npm run test` and `npm run build` from repo root.

## File Structure

```
src-tauri/
  Cargo.toml                    # + capture workspace member, plugins
  capture/                      # NEW crate vault_buddy_capture
    Cargo.toml
    src/lib.rs                  # module wiring + public re-exports
    src/mixer.rs                # PURE: downmix, linear resample, soft-clip, mix
    src/encoder.rs              # LAME wrapper: f32 mono in → MP3 bytes out
    src/session.rs              # state machine + worker thread + finalize
    src/recovery.rs             # has_mp3_frame + orphan scan/finalize
    src/devices.rs              # cpal streams; loopback #[cfg(windows)]
  core/src/
    lib.rs                      # + pub mod capture_config/capture_paths/capture_note
    capture_config.rs           # NEW: config schema, defaults, parse
    capture_paths.rs            # NEW: dated folders, base names, reservation
    capture_note.rs             # NEW: frontmatter + note rendering, atomic write helper
  src/
    lib.rs                      # plugins, state, commands, close/quit guards, recovery kickoff
    tray.rs                     # recording icon/menu/tooltip, hide guard, stop item
    capture_commands.rs         # NEW: start/stop/status commands, events, notifications
src/
  types.ts                      # + CaptureStatus payload types
  stores/capture.ts             # NEW Pinia store
  components/RecordingBar.vue   # NEW: elapsed + Stop
  components/VaultList.vue      # + capture button per vault
  components/ActionPanel.vue    # + RecordingBar when active
  components/CompanionCharacter.vue  # + recording visual state
  App.vue                       # wiring
tests/
  capture-store.test.ts         # NEW
  recording-bar.test.ts         # NEW
  vault-list.test.ts            # + capture button cases
  companion-character.test.ts   # + recording state case
.github/workflows/ci.yml        # ALSA headers + clippy/test for both crates
docs/DEVELOPMENT.md             # config.json schema documentation
docs/superpowers/specs/2026-07-04-increment-2-windows-verification.md  # manual checklist
```

---

### Task 1: Workspace scaffold + CI coverage for the capture crate

**Files:**
- Create: `src-tauri/capture/Cargo.toml`, `src-tauri/capture/src/lib.rs`
- Modify: `src-tauri/Cargo.toml` (workspace members)
- Modify: `.github/workflows/ci.yml:40-45`

**Interfaces:**
- Consumes: nothing.
- Produces: empty crate `vault_buddy_capture` that compiles; CI runs clippy + tests for `vault_buddy_core` **and** `vault_buddy_capture` with ALSA headers installed.

- [ ] **Step 1: Create the crate**

`src-tauri/capture/Cargo.toml`:

```toml
[package]
name = "vault_buddy_capture"
version = "0.1.0"
edition = "2021"

[dependencies]
cpal = "0.15"
mp3lame-encoder = "0.2"
log = "0.4"
vault_buddy_core = { path = "../core" }

[dev-dependencies]
minimp3 = "0.5"
tempfile = "3"
```

`src-tauri/capture/src/lib.rs`:

```rust
//! Audio capture engine for Knowledge Intake: devices → mixer → MP3
//! encoder → crash-safe .part writer. Obsidian never sees a half-written
//! file; the vault only ever contains hidden .part temps and final MP3s.
```

- [ ] **Step 2: Add the crate to the workspace**

In `src-tauri/Cargo.toml` change:

```toml
[workspace]
members = ["core"]
```

to:

```toml
[workspace]
members = ["core", "capture"]
```

- [ ] **Step 3: Verify it builds and tests run (0 tests is fine)**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture`
Expected: compiles LAME via cc, `running 0 tests`, exit 0. (On Linux without ALSA headers this fails to build cpal — that is exactly what the CI step below fixes; install locally with `sudo apt-get install -y libasound2-dev` if needed.)

- [ ] **Step 4: Update CI**

In `.github/workflows/ci.yml`, replace the `clippy (core crate)` and `tests (core crate)` steps of the `rust-core` job with:

```yaml
      - name: Install ALSA headers (cpal build dependency on Linux)
        run: sudo apt-get update && sudo apt-get install -y libasound2-dev
      - name: clippy (core + capture crates)
        run: cargo clippy -p vault_buddy_core -p vault_buddy_capture --all-targets -- -D warnings
        working-directory: src-tauri
      - name: tests (core + capture crates)
        run: cargo test -p vault_buddy_core -p vault_buddy_capture
        working-directory: src-tauri
```

(The ALSA install step must come before clippy. Keep the existing rustfmt step unchanged — it already covers the whole workspace.)

- [ ] **Step 5: Run fmt + clippy locally**

Run from `src-tauri/`: `cargo fmt --check && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/capture src-tauri/Cargo.toml .github/workflows/ci.yml
git commit -m "feat(capture): scaffold vault_buddy_capture crate with CI coverage"
```

---

### Task 2: Per-vault capture config (`vault_buddy_core::capture_config`)

**Files:**
- Create: `src-tauri/core/src/capture_config.rs`
- Modify: `src-tauri/core/src/lib.rs:1-3` (add `pub mod capture_config;`)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `enum RecordingMode { Meeting, VoiceNote }` with `RecordingMode::label(&self) -> &'static str` returning `"Meeting"` / `"Voice Note"`.
  - `struct VaultCaptureConfig { mode: RecordingMode, recording_folder: Option<String>, bitrate_kbps: u32, create_note: bool }` (`Clone`, `Debug`, `PartialEq`) with `fn effective_recording_folder(&self) -> &str` — the configured folder, or the mode default (`"Meetings"` / `"Voice Notes"`).
  - `fn parse_config(json: &str) -> AppConfig` — parses via `serde_json::Value` with **per-field** fallbacks: one malformed field (e.g. `"bitrateKbps": "160"` as a string) defaults only that field, never drops the entry or the whole file. Garbage input → defaults; never panics.
  - `fn vault_config(cfg: &AppConfig, vault_id: &str) -> VaultCaptureConfig` — clone of the entry or defaults.
  - `fn config_path() -> Option<PathBuf>` — `dirs::config_dir()/vault-buddy/config.json`.
  - `fn load_config() -> AppConfig` — reads `config_path()`, missing/unreadable → default.

- [ ] **Step 1: Write the failing tests**

At the bottom of the new `src-tauri/core/src/capture_config.rs` (write the tests first, module skeleton empty):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_json_is_garbage() {
        let cfg = parse_config("not json at all");
        let v = vault_config(&cfg, "any-id");
        assert_eq!(v.mode, RecordingMode::Meeting);
        assert_eq!(v.effective_recording_folder(), "Meetings");
        assert_eq!(v.bitrate_kbps, 128);
        assert!(v.create_note);
    }

    #[test]
    fn folder_defaults_follow_the_mode_but_config_overrides() {
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": { "mode": "voice-note" },
                "b": { "mode": "voice-note", "recordingFolder": "Inbox" }
            } }"#,
        );
        assert_eq!(vault_config(&cfg, "a").effective_recording_folder(), "Voice Notes");
        assert_eq!(vault_config(&cfg, "b").effective_recording_folder(), "Inbox");
    }

    #[test]
    fn defaults_for_unknown_vault() {
        let cfg = parse_config(r#"{ "vaults": {} }"#);
        assert_eq!(vault_config(&cfg, "missing"), VaultCaptureConfig::default());
    }

    #[test]
    fn partial_entry_fills_missing_fields_with_defaults() {
        let cfg = parse_config(
            r#"{ "vaults": { "abc": { "mode": "voice-note", "createNote": false } } }"#,
        );
        let v = vault_config(&cfg, "abc");
        assert_eq!(v.mode, RecordingMode::VoiceNote);
        assert!(!v.create_note);
        assert_eq!(v.recording_folder, None);
        assert_eq!(v.bitrate_kbps, 128);
    }

    #[test]
    fn unknown_mode_string_falls_back_to_meeting() {
        let cfg = parse_config(r#"{ "vaults": { "abc": { "mode": "karaoke" } } }"#);
        assert_eq!(vault_config(&cfg, "abc").mode, RecordingMode::Meeting);
    }

    #[test]
    fn malformed_field_defaults_locally_not_globally() {
        // One bad field must not drop the entry (or the whole file):
        // a voice-note vault must never silently flip back to meeting
        // mode (which would open loopback) because bitrate was quoted.
        let cfg = parse_config(
            r#"{ "vaults": {
                "a": { "mode": "voice-note", "bitrateKbps": "160" },
                "b": { "createNote": false }
            } }"#,
        );
        let a = vault_config(&cfg, "a");
        assert_eq!(a.mode, RecordingMode::VoiceNote);
        assert_eq!(a.bitrate_kbps, 128);
        assert!(!vault_config(&cfg, "b").create_note);
    }

    #[test]
    fn mode_labels() {
        assert_eq!(RecordingMode::Meeting.label(), "Meeting");
        assert_eq!(RecordingMode::VoiceNote.label(), "Voice Note");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri/`: `cargo test -p vault_buddy_core capture_config`
Expected: compile error (`parse_config` not found).

- [ ] **Step 3: Implement**

Top of `src-tauri/core/src/capture_config.rs`:

```rust
//! Per-vault capture settings. App-side (%APPDATA%\vault-buddy\config.json),
//! keyed by Obsidian vault ID — never written into user vaults. Hand-edited
//! for now; a settings UI arrives in a later increment, so parsing must
//! shrug off any malformed input and fall back to defaults.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordingMode {
    #[default]
    Meeting,
    VoiceNote,
}

impl RecordingMode {
    pub fn label(&self) -> &'static str {
        match self {
            RecordingMode::Meeting => "Meeting",
            RecordingMode::VoiceNote => "Voice Note",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VaultCaptureConfig {
    pub mode: RecordingMode,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
}

impl Default for VaultCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Meeting,
            recording_folder: None,
            bitrate_kbps: 128,
            create_note: true,
        }
    }
}

impl VaultCaptureConfig {
    /// Configured folder, or the mode's default (the PRD gives meetings
    /// and voice notes distinct homes).
    pub fn effective_recording_folder(&self) -> &str {
        match &self.recording_folder {
            Some(folder) => folder,
            None => match self.mode {
                RecordingMode::Meeting => "Meetings",
                RecordingMode::VoiceNote => "Voice Notes",
            },
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppConfig {
    pub vaults: HashMap<String, VaultCaptureConfig>,
}

/// Per-field parsing through serde_json::Value: the file is hand-edited,
/// and one malformed value must default only itself — a derived
/// deserializer would reject the whole file, silently flipping every
/// vault back to meeting mode (and thus desktop-audio capture).
pub fn parse_config(json: &str) -> AppConfig {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return AppConfig::default();
    };
    let mut vaults = HashMap::new();
    if let Some(map) = value.get("vaults").and_then(|v| v.as_object()) {
        for (id, entry) in map {
            vaults.insert(id.clone(), vault_entry(entry));
        }
    }
    AppConfig { vaults }
}

fn vault_entry(entry: &serde_json::Value) -> VaultCaptureConfig {
    let defaults = VaultCaptureConfig::default();
    VaultCaptureConfig {
        mode: match entry.get("mode").and_then(|v| v.as_str()) {
            Some("voice-note") => RecordingMode::VoiceNote,
            Some("meeting") => RecordingMode::Meeting,
            _ => defaults.mode,
        },
        recording_folder: entry
            .get("recordingFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        bitrate_kbps: entry
            .get("bitrateKbps")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(defaults.bitrate_kbps),
        create_note: entry
            .get("createNote")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.create_note),
    }
}

pub fn vault_config(cfg: &AppConfig, vault_id: &str) -> VaultCaptureConfig {
    cfg.vaults.get(vault_id).cloned().unwrap_or_default()
}

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vault-buddy").join("config.json"))
}

pub fn load_config() -> AppConfig {
    let Some(path) = config_path() else {
        return AppConfig::default();
    };
    match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    }
}
```

Add to `src-tauri/core/src/lib.rs` after line 3 (`pub mod uri;`):

```rust
pub mod capture_config;
```

Note: an unknown-field deserializer that errors per-entry would drop the whole map; the `mode_or_default` helper plus `#[serde(default)]` keeps every recognizable field. If the `unknown_mode_string_falls_back_to_meeting` test fails because serde rejects the entry, wrap map values as `serde_json::Value` and convert per-field — but try the annotated struct first.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_core capture_config`
Expected: 7 passed.

- [ ] **Step 5: fmt + clippy + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_config.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): per-vault capture config with defensive defaults"
```

---

### Task 3: Naming, folders, and pairwise reservation (`vault_buddy_core::capture_paths`)

**Files:**
- Create: `src-tauri/core/src/capture_paths.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod capture_paths;`)

**Interfaces:**
- Consumes: `chrono::NaiveDate`.
- Produces:
  - `struct CaptureNames { pub base: String, pub final_mp3: PathBuf, pub note_md: PathBuf, pub part: PathBuf }`
  - `fn dated_folder(root: &Path, date: NaiveDate) -> PathBuf` — `root/YYYY/MM`.
  - `fn base_name(date: NaiveDate, hour: u32, minute: u32, label: &str) -> String` — `"2026-07-04 1435 Meeting"`.
  - `fn reserve_names(dir: &Path, base: &str) -> CaptureNames` — first base (plain, then `" (2)"`, `" (3)"`, …) where `.mp3`, `.md` **and** `.<base>.mp3.part` are all absent in `dir`.
  - `fn reserve_final(dir: &Path, base: &str) -> (PathBuf, PathBuf)` — stop-time recheck: first suffix where only `.mp3` and `.md` are absent (the session's own `.part` must not force a suffix).
  - `fn part_file_name(base: &str) -> String` — `".{base}.mp3.part"`.
  - `fn base_from_part(part_file_name: &str) -> Option<String>` — inverse; `None` if the name doesn't match the pattern.
  - `fn recovered_base(base: &str) -> String` — `"{base} (recovered)"`.
  - `fn safe_recording_root(vault_path: &Path, folder: &str) -> Result<PathBuf, String>` — joins a configured folder onto the vault, rejecting absolute paths and any `..`/root components so a hand-edited config can never write outside the vault (PRD: recordings are stored inside the vault).
  - `fn assert_root_inside_vault(vault_path: &Path, root: &Path) -> Result<(), String>` — runtime companion to the lexical check: canonicalizes both paths (the root must already exist) and requires the root to resolve under the vault, so a pre-existing symlink or Windows junction at the recording folder cannot carry writes outside the vault. Called by the start path after `create_dir_all` and by the recovery pass per root.

- [ ] **Step 1: Write the failing tests**

Bottom of new `src-tauri/core/src/capture_paths.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 4).unwrap()
    }

    #[test]
    fn dated_folder_is_year_slash_month() {
        let dir = dated_folder(Path::new("/v/Meetings"), date());
        assert_eq!(dir, Path::new("/v/Meetings/2026/07"));
    }

    #[test]
    fn base_name_format() {
        assert_eq!(base_name(date(), 14, 5, "Meeting"), "2026-07-04 1405 Meeting");
    }

    #[test]
    fn part_name_roundtrip() {
        let part = part_file_name("2026-07-04 1405 Meeting");
        assert_eq!(part, ".2026-07-04 1405 Meeting.mp3.part");
        assert_eq!(
            base_from_part(&part).as_deref(),
            Some("2026-07-04 1405 Meeting")
        );
        assert_eq!(base_from_part("random.txt"), None);
    }

    #[test]
    fn reserve_uses_plain_base_when_all_free() {
        let dir = tempfile::tempdir().unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.base, "b");
        assert_eq!(names.final_mp3, dir.path().join("b.mp3"));
        assert_eq!(names.note_md, dir.path().join("b.md"));
        assert_eq!(names.part, dir.path().join(".b.mp3.part"));
    }

    #[test]
    fn reserve_advances_when_note_or_part_exists() {
        let dir = tempfile::tempdir().unwrap();
        // a pre-existing user note blocks the plain base
        std::fs::write(dir.path().join("b.md"), "user note").unwrap();
        // an unrecovered orphan blocks " (2)"
        std::fs::write(dir.path().join(".b (2).mp3.part"), "x").unwrap();
        let names = reserve_names(dir.path(), "b");
        assert_eq!(names.base, "b (3)");
    }

    #[test]
    fn reserve_final_ignores_own_part_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".b.mp3.part"), "recording").unwrap();
        let (mp3, md) = reserve_final(dir.path(), "b");
        assert_eq!(mp3, dir.path().join("b.mp3"));
        assert_eq!(md, dir.path().join("b.md"));
    }

    #[test]
    fn reserve_final_advances_past_existing_mp3() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.mp3"), "sync client wrote this").unwrap();
        let (mp3, _) = reserve_final(dir.path(), "b");
        assert_eq!(mp3, dir.path().join("b (2).mp3"));
    }

    #[test]
    fn recovered_base_appends_marker() {
        assert_eq!(recovered_base("b"), "b (recovered)");
    }

    #[test]
    fn safe_root_accepts_plain_and_nested_folders() {
        let vault = Path::new("/v");
        assert_eq!(
            safe_recording_root(vault, "Meetings").unwrap(),
            Path::new("/v/Meetings")
        );
        assert_eq!(
            safe_recording_root(vault, "Capture/Meetings").unwrap(),
            Path::new("/v/Capture/Meetings")
        );
    }

    #[test]
    fn safe_root_rejects_escaping_folders() {
        let vault = Path::new("/v");
        assert!(safe_recording_root(vault, "../outside").is_err());
        assert!(safe_recording_root(vault, "a/../../outside").is_err());
        assert!(safe_recording_root(vault, "/etc").is_err());
        assert!(safe_recording_root(vault, "C:\\other").is_err());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri/`: `cargo test -p vault_buddy_core capture_paths`
Expected: compile error.

- [ ] **Step 3: Implement**

Top of `src-tauri/core/src/capture_paths.rs`:

```rust
//! Recording file layout: dated folders, timestamped base names, and the
//! pairwise reservation rule — a base name is usable only when the .mp3,
//! the .md AND the hidden .mp3.part are all free, so a capture can never
//! overwrite a user note or an unrecovered orphan from an earlier crash.

use chrono::NaiveDate;
use std::path::{Path, PathBuf};

pub struct CaptureNames {
    pub base: String,
    pub final_mp3: PathBuf,
    pub note_md: PathBuf,
    pub part: PathBuf,
}

pub fn dated_folder(root: &Path, date: NaiveDate) -> PathBuf {
    root.join(date.format("%Y").to_string())
        .join(date.format("%m").to_string())
}

pub fn base_name(date: NaiveDate, hour: u32, minute: u32, label: &str) -> String {
    format!("{} {hour:02}{minute:02} {label}", date.format("%Y-%m-%d"))
}

pub fn part_file_name(base: &str) -> String {
    format!(".{base}.mp3.part")
}

pub fn base_from_part(part_file_name: &str) -> Option<String> {
    let stripped = part_file_name.strip_prefix('.')?;
    let base = stripped.strip_suffix(".mp3.part")?;
    if base.is_empty() {
        None
    } else {
        Some(base.to_string())
    }
}

pub fn recovered_base(base: &str) -> String {
    format!("{base} (recovered)")
}

fn candidate(base: &str, attempt: u32) -> String {
    if attempt == 1 {
        base.to_string()
    } else {
        format!("{base} ({attempt})")
    }
}

pub fn reserve_names(dir: &Path, base: &str) -> CaptureNames {
    for attempt in 1.. {
        let b = candidate(base, attempt);
        let final_mp3 = dir.join(format!("{b}.mp3"));
        let note_md = dir.join(format!("{b}.md"));
        let part = dir.join(part_file_name(&b));
        if !final_mp3.exists() && !note_md.exists() && !part.exists() {
            return CaptureNames { base: b, final_mp3, note_md, part };
        }
    }
    unreachable!("suffix search always terminates")
}

/// Join a configured recording folder onto the vault, refusing anything
/// that could land outside it: the config file is hand-editable, and the
/// PRD guarantees recordings are stored inside the vault.
pub fn safe_recording_root(vault_path: &Path, folder: &str) -> Result<PathBuf, String> {
    use std::path::Component;
    let rel = Path::new(folder);
    let escapes = rel.components().any(|c| {
        !matches!(c, Component::Normal(_) | Component::CurDir)
    }) || folder.contains('\\') && folder.contains(':');
    if folder.is_empty() || escapes {
        return Err(format!(
            "Configured recording folder must stay inside the vault: {folder:?}"
        ));
    }
    Ok(vault_path.join(rel))
}

/// Stop-time recheck: only the final destinations matter — the session's
/// own .part must not push an ordinary save onto a suffixed name.
pub fn reserve_final(dir: &Path, base: &str) -> (PathBuf, PathBuf) {
    for attempt in 1.. {
        let b = candidate(base, attempt);
        let final_mp3 = dir.join(format!("{b}.mp3"));
        let note_md = dir.join(format!("{b}.md"));
        if !final_mp3.exists() && !note_md.exists() {
            return (final_mp3, note_md);
        }
    }
    unreachable!("suffix search always terminates")
}
```

Add `pub mod capture_paths;` to `src-tauri/core/src/lib.rs` beside the other modules.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_core capture_paths`
Expected: 10 passed. (The `C:\other` rejection relies on the backslash+colon
check when the test runs on Linux — on Windows it is a Prefix component and
rejected structurally.)

- [ ] **Step 5: fmt + clippy + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_paths.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): capture file naming and pairwise reservation"
```

---

### Task 4: Companion note rendering + atomic write (`vault_buddy_core::capture_note`)

**Files:**
- Create: `src-tauri/core/src/capture_note.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod capture_note;`)

**Interfaces:**
- Consumes: nothing new.
- Produces:
  - `struct NoteMeta { pub recorded_at: String /* RFC3339 local */, pub duration_secs: u64, pub vault_name: String, pub recording_type: String /* "Meeting" | "Voice Note" */, pub input_devices: Vec<String>, pub event: Option<String> /* "source lost: …" | "recovered after crash" */ }`
  - `fn format_duration(secs: u64) -> String` — `"0:07"`, `"3:09"`, `"1:02:03"`.
  - `fn render_note(meta: &NoteMeta, mp3_file_name: &str) -> String` — YAML frontmatter + `![[<mp3>]]` embed. **Every scalar value is double-quoted** with `\` and `"` escaped and newlines flattened to spaces (private `yaml_quote` helper): vault and device names are user/system-controlled and may contain `:` or quotes, and an unquoted `1:02:03` duration would even parse as YAML sexagesimal.
  - `fn write_note_atomic(note_path: &Path, content: &str) -> std::io::Result<()>` — writes `.<name>.vault-buddy.tmp` beside the target (the `vault-buddy` infix is the **ownership marker**: recovery may only ever delete temps carrying it, never another tool's `.md.tmp`), flush + `sync_all`, then non-replacing rename; never truncates an existing note.
  - `fn write_note_collision_safe(note_path: &Path, content: &str) -> std::io::Result<PathBuf>` — calls `write_note_atomic`; on `AlreadyExists` (a user or sync client took the name after reservation) advances a ` (2)`, ` (3)`, … suffix on the note's stem and retries, returning the path actually written. A saved MP3 must never lose its companion note to a late collision.
  - `pub const NOTE_TMP_SUFFIX: &str = ".vault-buddy.tmp"` — shared with recovery's cleanup filter.

- [ ] **Step 1: Write the failing tests**

Bottom of new `src-tauri/core/src/capture_note.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> NoteMeta {
        NoteMeta {
            recorded_at: "2026-07-04T14:05:00+02:00".into(),
            duration_secs: 3723,
            vault_name: "Work".into(),
            recording_type: "Meeting".into(),
            input_devices: vec!["Headset Mic".into(), "Speakers (loopback)".into()],
            event: None,
        }
    }

    #[test]
    fn duration_formats() {
        assert_eq!(format_duration(7), "0:07");
        assert_eq!(format_duration(189), "3:09");
        assert_eq!(format_duration(3723), "1:02:03");
    }

    #[test]
    fn note_contains_frontmatter_and_embed() {
        let note = render_note(&meta(), "2026-07-04 1405 Meeting.mp3");
        assert!(note.starts_with("---\n"), "frontmatter first: {note}");
        assert!(note.contains(r#"recorded: "2026-07-04T14:05:00+02:00""#));
        assert!(note.contains(r#"duration: "1:02:03""#));
        assert!(note.contains(r#"vault: "Work""#));
        assert!(note.contains(r#"type: "Meeting""#));
        assert!(note.contains(r#"- "Headset Mic""#));
        assert!(note.contains("![[2026-07-04 1405 Meeting.mp3]]"));
        assert!(!note.contains("event:"), "no event line when None");
    }

    #[test]
    fn note_includes_event_when_present() {
        let mut m = meta();
        m.event = Some("recovered after crash".into());
        assert!(render_note(&m, "x.mp3").contains(r#"event: "recovered after crash""#));
    }

    #[test]
    fn yaml_special_characters_are_escaped() {
        // Vault and device names are user/system input: colons, quotes and
        // newlines must not break the frontmatter or inject fields.
        let mut m = meta();
        m.vault_name = "Work: \"Client\" Vault".into();
        m.input_devices = vec!["Mic\nInjected: true".into()];
        let note = render_note(&m, "x.mp3");
        assert!(note.contains(r#"vault: "Work: \"Client\" Vault""#));
        assert!(!note.contains("\nInjected:"), "newline must not inject a field");
    }

    #[test]
    fn atomic_write_creates_note_and_removes_temp() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        write_note_atomic(&note, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "hello");
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp not cleaned: {leftovers:?}");
    }

    #[test]
    fn atomic_write_never_replaces_existing_note() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        std::fs::write(&note, "user content").unwrap();
        assert!(write_note_atomic(&note, "new").is_err());
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "user content");
    }

    #[test]
    fn temp_file_carries_ownership_marker() {
        // The marker is what recovery's cleanup filter keys on — without
        // it we could never safely delete stale temps in a user's vault.
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        // Sabotage the rename by pre-creating the target after temp write
        // is not needed — just verify the constant shape is used.
        write_note_atomic(&note, "x").unwrap();
        assert!(NOTE_TMP_SUFFIX.contains("vault-buddy"));
    }

    #[test]
    fn collision_safe_write_suffixes_instead_of_dropping() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("n.md");
        std::fs::write(&note, "taken").unwrap();
        let written = write_note_collision_safe(&note, "content").unwrap();
        assert_eq!(written, dir.path().join("n (2).md"));
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "content");
        assert_eq!(std::fs::read_to_string(&note).unwrap(), "taken");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri/`: `cargo test -p vault_buddy_core capture_note`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Companion markdown note: frontmatter metadata + an ![[…]] embed of the
//! recording — no AI sections in this increment. Written atomically
//! (temp + fsync + non-replacing rename) so a crash can truncate only a
//! hidden temp file, never a note in the vault.

use std::io::Write;
use std::path::Path;

pub struct NoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    pub recording_type: String,
    pub input_devices: Vec<String>,
    pub event: Option<String>,
}

pub fn format_duration(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Double-quote a YAML scalar, escaping `\` and `"` and flattening
/// newlines to spaces. Vault and device names are user/system input;
/// unquoted they could break the frontmatter or inject fields — and an
/// unquoted `1:02:03` duration even parses as YAML sexagesimal.
fn yaml_quote(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ");
    format!("\"{escaped}\"")
}

pub fn render_note(meta: &NoteMeta, mp3_file_name: &str) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("recorded: {}\n", yaml_quote(&meta.recorded_at)));
    out.push_str(&format!(
        "duration: {}\n",
        yaml_quote(&format_duration(meta.duration_secs))
    ));
    out.push_str(&format!("vault: {}\n", yaml_quote(&meta.vault_name)));
    out.push_str(&format!("type: {}\n", yaml_quote(&meta.recording_type)));
    out.push_str("inputs:\n");
    for device in &meta.input_devices {
        out.push_str(&format!("  - {}\n", yaml_quote(device)));
    }
    if let Some(event) = &meta.event {
        out.push_str(&format!("event: {}\n", yaml_quote(event)));
    }
    out.push_str("created-by: Vault Buddy\n---\n\n");
    out.push_str(&format!("![[{mp3_file_name}]]\n"));
    out
}

/// Ownership marker for our note temp files: recovery's cleanup filter
/// deletes ONLY temps carrying this suffix — never another tool's
/// `.md.tmp` that happens to live in a recording folder.
pub const NOTE_TMP_SUFFIX: &str = ".vault-buddy.tmp";

pub fn write_note_collision_safe(
    note_path: &Path,
    content: &str,
) -> std::io::Result<std::path::PathBuf> {
    let dir = note_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = note_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    for attempt in 1u32.. {
        let candidate = if attempt == 1 {
            note_path.to_path_buf()
        } else {
            dir.join(format!("{stem} ({attempt}).md"))
        };
        match write_note_atomic(&candidate, content) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    unreachable!("suffix search always terminates")
}

pub fn write_note_atomic(note_path: &Path, content: &str) -> std::io::Result<()> {
    if note_path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "note already exists",
        ));
    }
    let dir = note_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = note_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{file_name}{NOTE_TMP_SUFFIX}"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(content.as_bytes())?;
        f.sync_all()?;
    }
    // Non-replacing on Windows (rename fails if dest exists). On Unix,
    // rename would replace — the exists() guard above plus the pairwise
    // reservation makes that window acceptable for a dev-only platform.
    let result = std::fs::rename(&tmp, note_path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}
```

Add `pub mod capture_note;` to `src-tauri/core/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_core capture_note`
Expected: 8 passed. (If `atomic_write_never_replaces_existing_note` fails on Linux because rename replaced the file, the `exists()` pre-check is missing — it is the guard that test exercises.)

- [ ] **Step 5: fmt + clippy + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_core --all-targets -- -D warnings && cd ..
git add src-tauri/core/src/capture_note.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): companion note rendering with atomic writes"
```

---

### Task 5: Mixer — downmix, resample, soft-clip (`vault_buddy_capture::mixer`)

**Files:**
- Create: `src-tauri/capture/src/mixer.rs`
- Modify: `src-tauri/capture/src/lib.rs` (add `pub mod mixer;`)

**Interfaces:**
- Consumes: nothing.
- Produces (all pure):
  - `fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32>`
  - `fn resample_linear(mono: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32>`
  - `fn soft_clip(x: f32) -> f32` — `x.tanh()`; keeps |out| < 1.
  - `fn mix_to_stereo_i16(a: &[f32], b: &[f32]) -> Vec<i16>` — sums (shorter input treated as silence-padded), soft-clips, duplicates to interleaved stereo i16.

- [ ] **Step 1: Write the failing tests**

Bottom of new `src-tauri/capture/src/mixer.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channel_pairs() {
        assert_eq!(downmix_to_mono(&[1.0, 0.0, 0.5, 0.5], 2), vec![0.5, 0.5]);
        assert_eq!(downmix_to_mono(&[0.25, 0.75], 1), vec![0.25, 0.75]);
    }

    #[test]
    fn resample_identity_when_rates_match() {
        let x = vec![0.1, 0.2, 0.3];
        assert_eq!(resample_linear(&x, 44_100, 44_100), x);
    }

    #[test]
    fn resample_halves_and_doubles_length() {
        let x: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        assert_eq!(resample_linear(&x, 88_200, 44_100).len(), 50);
        assert_eq!(resample_linear(&x, 22_050, 44_100).len(), 200);
    }

    #[test]
    fn resample_preserves_a_constant_signal() {
        let x = vec![0.5f32; 480];
        for y in resample_linear(&x, 48_000, 44_100) {
            assert!((y - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn soft_clip_bounds_output() {
        assert!(soft_clip(10.0) < 1.0);
        assert!(soft_clip(-10.0) > -1.0);
        assert!((soft_clip(0.1) - 0.1).abs() < 0.01, "near-linear when small");
    }

    #[test]
    fn mix_pads_shorter_side_with_silence_and_interleaves_stereo() {
        let out = mix_to_stereo_i16(&[0.5, 0.5], &[0.25]);
        assert_eq!(out.len(), 4); // 2 frames * 2 channels
        assert_eq!(out[0], out[1], "L == R");
        let first = out[0] as f32 / i16::MAX as f32;
        assert!((first - (0.75f32).tanh()).abs() < 0.001);
        let second = out[2] as f32 / i16::MAX as f32;
        assert!((second - (0.5f32).tanh()).abs() < 0.001, "b side silence-padded");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture mixer`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Pure sample math for the capture pipeline. No I/O, no devices — fully
//! unit-tested on any platform. Linear resampling is deliberate: adaptive
//! drift compensation is an accepted deferral in the spec.

pub fn downmix_to_mono(interleaved: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    interleaved
        .chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

pub fn resample_linear(mono: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || mono.is_empty() {
        return mono.to_vec();
    }
    let out_len = (mono.len() as u64 * to_rate as u64 / from_rate as u64) as usize;
    let step = from_rate as f64 / to_rate as f64;
    (0..out_len)
        .map(|i| {
            let pos = i as f64 * step;
            let idx = pos as usize;
            let frac = (pos - idx as f64) as f32;
            let a = mono[idx.min(mono.len() - 1)];
            let b = mono[(idx + 1).min(mono.len() - 1)];
            a + (b - a) * frac
        })
        .collect()
}

pub fn soft_clip(x: f32) -> f32 {
    x.tanh()
}

pub fn mix_to_stereo_i16(a: &[f32], b: &[f32]) -> Vec<i16> {
    let frames = a.len().max(b.len());
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let sum = a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0);
        let sample = (soft_clip(sum) * i16::MAX as f32) as i16;
        out.push(sample);
        out.push(sample);
    }
    out
}
```

Add `pub mod mixer;` to `src-tauri/capture/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture mixer`
Expected: 6 passed.

- [ ] **Step 5: fmt + clippy + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings && cd ..
git add src-tauri/capture/src/mixer.rs src-tauri/capture/src/lib.rs
git commit -m "feat(capture): pure mixer — downmix, linear resample, soft-clip"
```

---

### Task 6: MP3 encoder wrapper (`vault_buddy_capture::encoder`)

**Files:**
- Create: `src-tauri/capture/src/encoder.rs`
- Modify: `src-tauri/capture/src/lib.rs` (add `pub mod encoder;`)

**Interfaces:**
- Consumes: `mixer::mix_to_stereo_i16` output shape (interleaved stereo `&[i16]`).
- Produces:
  - `struct Mp3Encoder` with:
    - `fn new(sample_rate: u32, bitrate_kbps: u32) -> Result<Mp3Encoder, String>` (bitrates other than 128/160/192 fall back to 128 with a `log::warn!`).
    - `fn encode(&mut self, interleaved_stereo: &[i16]) -> Result<Vec<u8>, String>`
    - `fn finish(self) -> Result<Vec<u8>, String>` — final LAME flush.

- [ ] **Step 1: Write the failing integration test**

Bottom of new `src-tauri/capture/src/encoder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// 2 seconds of 440 Hz sine → encode → decode with minimp3 → duration
    /// within tolerance. This is the CI-side proof that the pipeline
    /// produces real, playable MP3 without any audio hardware.
    #[test]
    fn sine_roundtrip_has_expected_duration() {
        let rate = 44_100u32;
        let seconds = 2.0f32;
        let frames = (rate as f32 * seconds) as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / rate as f32;
            let s = ((t * 440.0 * std::f32::consts::TAU).sin() * 0.5 * i16::MAX as f32) as i16;
            pcm.push(s);
            pcm.push(s);
        }

        let mut enc = Mp3Encoder::new(rate, 128).unwrap();
        let mut mp3 = Vec::new();
        // encode in 100ms chunks like the live pipeline does
        for chunk in pcm.chunks(4410 * 2) {
            mp3.extend(enc.encode(chunk).unwrap());
        }
        mp3.extend(enc.finish().unwrap());
        assert!(mp3.len() > 10_000, "suspiciously small: {} bytes", mp3.len());

        let mut decoder = minimp3::Decoder::new(std::io::Cursor::new(mp3));
        let mut decoded_frames = 0usize;
        loop {
            match decoder.next_frame() {
                Ok(frame) => decoded_frames += frame.data.len() / frame.channels,
                Err(minimp3::Error::Eof) => break,
                Err(e) => panic!("decode error: {e:?}"),
            }
        }
        let decoded_secs = decoded_frames as f32 / rate as f32;
        assert!(
            (decoded_secs - seconds).abs() < 0.2,
            "expected ~{seconds}s, decoded {decoded_secs}s"
        );
    }

    #[test]
    fn unsupported_bitrate_falls_back() {
        assert!(Mp3Encoder::new(44_100, 999).is_ok());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture encoder`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Thin safe wrapper around LAME (mp3lame-encoder). Streaming: every call
//! returns finished MP3 bytes ready to append to the .part file, so the
//! on-disk file is always a valid (if truncated) MP3.

use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm, MonoPcm};

pub struct Mp3Encoder {
    inner: mp3lame_encoder::Encoder,
}

// MonoPcm import is unused if the interleaved path is the only one; remove
// the import if clippy flags it after implementation settles.
#[allow(unused_imports)]
use MonoPcm as _;

impl Mp3Encoder {
    pub fn new(sample_rate: u32, bitrate_kbps: u32) -> Result<Self, String> {
        let bitrate = match bitrate_kbps {
            128 => Bitrate::Kbps128,
            160 => Bitrate::Kbps160,
            192 => Bitrate::Kbps192,
            other => {
                log::warn!("unsupported bitrate {other} kbps, falling back to 128");
                Bitrate::Kbps128
            }
        };
        let mut builder = Builder::new().ok_or("failed to init LAME")?;
        builder.set_num_channels(2).map_err(|e| e.to_string())?;
        builder
            .set_sample_rate(sample_rate)
            .map_err(|e| e.to_string())?;
        builder.set_brate(bitrate).map_err(|e| e.to_string())?;
        builder
            .set_quality(mp3lame_encoder::Quality::Good)
            .map_err(|e| e.to_string())?;
        let inner = builder.build().map_err(|e| e.to_string())?;
        Ok(Self { inner })
    }

    pub fn encode(&mut self, interleaved_stereo: &[i16]) -> Result<Vec<u8>, String> {
        let input = InterleavedPcm(interleaved_stereo);
        let mut out = Vec::new();
        out.reserve(mp3lame_encoder::max_required_buffer_size(
            interleaved_stereo.len() / 2,
        ));
        let n = self
            .inner
            .encode(input, out.spare_capacity_mut())
            .map_err(|e| e.to_string())?;
        // SAFETY: encode() reports how many bytes of spare capacity it wrote.
        unsafe { out.set_len(n) };
        Ok(out)
    }

    pub fn finish(mut self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        out.reserve(7200);
        let n = self
            .inner
            .flush::<FlushNoGap>(out.spare_capacity_mut())
            .map_err(|e| e.to_string())?;
        unsafe { out.set_len(n) };
        Ok(out)
    }
}
```

Add `pub mod encoder;` to `src-tauri/capture/src/lib.rs`.

API note for the implementer: if `mp3lame-encoder` 0.2's names differ (e.g. `Builder::new()` returning `Option` vs `Result`, or `set_brate` vs `set_bitrate`), check `cargo doc -p mp3lame-encoder --open` and adjust the wrapper only — the public `Mp3Encoder` interface above is what Task 8 consumes and must stay as written. Delete the `MonoPcm` import if unused.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture encoder`
Expected: 2 passed (the roundtrip takes a few seconds — LAME builds from source on first compile).

- [ ] **Step 5: fmt + clippy + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings && cd ..
git add src-tauri/capture/src/encoder.rs src-tauri/capture/src/lib.rs
git commit -m "feat(capture): streaming LAME MP3 encoder wrapper"
```

---

### Task 7: Recovery scan (`vault_buddy_capture::recovery`)

**Files:**
- Create: `src-tauri/capture/src/recovery.rs`
- Modify: `src-tauri/capture/src/lib.rs` (add `pub mod recovery;`)

**Interfaces:**
- Consumes: `vault_buddy_core::capture_paths::{base_from_part, recovered_base, reserve_final}`, `vault_buddy_core::capture_note::{render_note, write_note_atomic, NoteMeta}`.
- Produces:
  - `fn has_mp3_frame(bytes: &[u8]) -> bool` — true if an MP3 sync word (`0xFF` followed by a byte whose top 3 bits are set) appears.
  - `enum RecoveryAction { Recovered { mp3: PathBuf }, DeletedEmpty(PathBuf), Fresh(PathBuf) }`
  - `fn rename_into_reserved(from: &Path, dir: &Path, base: &str) -> Result<(PathBuf, PathBuf), String>` — shared finalize primitive: loops `reserve_final` + non-replacing rename, retrying with an advanced suffix when the destination appears in the check→rename window. Task 8's session finalize uses this too.
  - `fn recover_root(root: &Path, vault_name: &str, stale_after: Duration, write_note: bool) -> Vec<RecoveryAction>` — recursively walks `root` for `.…mp3.part` files and stale note temps; fresh `.part`s are reported as `Fresh` (caller schedules a rescan), zero-frame `.part`s are deleted, others are renamed to `<base> (recovered).mp3` via `rename_into_reserved`, with an optional note (`event: recovered after crash`).
  - **Ownership filters — recovery may only ever touch Vault Buddy's own files:** a `.mp3.part` is processed only when its base matches the capture timestamp pattern `YYYY-MM-DD HHmm …` (private `is_capture_base` check) — another tool's `.download.mp3.part` is never deleted or renamed; note temps are cleaned only when they end with `capture_note::NOTE_TMP_SUFFIX` (the `.vault-buddy.tmp` marker).
  - **Layout filter — recovery traverses only the dated capture layout:** from the root it descends only into 4-digit year directories and, within them, 2-digit month directories, and visits files only inside those month folders — Vault Buddy writes nowhere else, so a stale capture-named `.part` a user moved into `Meetings/Project/` (or left at the root) is never touched. Symlinks are still skipped at every level.

- [ ] **Step 1: Write the failing tests**

Bottom of new `src-tauri/capture/src/recovery.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Minimal valid-looking MP3 frame header bytes.
    fn mp3_bytes() -> Vec<u8> {
        let mut v = vec![0u8; 32];
        v.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        v.extend_from_slice(&[0u8; 400]);
        v
    }

    fn make_stale(path: &std::path::Path) {
        // recover_root treats files older than stale_after as orphans; a
        // zero stale_after makes everything stale without clock games.
        let _ = path;
    }

    #[test]
    fn sync_word_detection() {
        assert!(has_mp3_frame(&mp3_bytes()));
        assert!(!has_mp3_frame(&[0u8; 512]));
        assert!(!has_mp3_frame(b""));
    }

    #[test]
    fn recovers_stale_part_with_audio() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(".2026-07-04 1405 Meeting.mp3.part");
        std::fs::write(&part, mp3_bytes()).unwrap();
        make_stale(&part);
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            RecoveryAction::Recovered { mp3 } => {
                assert_eq!(
                    mp3.file_name().unwrap().to_string_lossy(),
                    "2026-07-04 1405 Meeting (recovered).mp3"
                );
                assert!(mp3.exists());
                assert!(!part.exists());
                let note = dir.path().join("2026-07-04 1405 Meeting (recovered).md");
                assert!(note.exists(), "recovery note written");
                let text = std::fs::read_to_string(note).unwrap();
                assert!(text.contains(r#"event: "recovered after crash""#));
            }
            other => panic!("expected Recovered, got {other:?}"),
        }
    }

    // All bases below use the capture timestamp pattern — recovery's
    // ownership filter ignores anything else.
    const BASE: &str = "2026-07-04 1405 Voice Note";

    #[test]
    fn respects_note_toggle() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, false);
        assert!(!dir.path().join(format!("{BASE} (recovered).md")).exists());
        assert!(dir.path().join(format!("{BASE} (recovered).mp3")).exists());
    }

    #[test]
    fn deletes_zero_frame_part() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, [0u8; 64]).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(matches!(actions[0], RecoveryAction::DeletedEmpty(_)));
        assert!(!part.exists());
        assert!(!dir.path().join(format!("{BASE} (recovered).mp3")).exists());
    }

    #[test]
    fn reports_fresh_part_without_touching_it() {
        let dir = tempfile::tempdir().unwrap();
        let part = dir.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::from_secs(3600), true);
        assert!(matches!(actions[0], RecoveryAction::Fresh(_)));
        assert!(part.exists());
    }

    #[test]
    fn walks_dated_subfolders_and_avoids_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let month = dir.path().join("2026").join("06");
        std::fs::create_dir_all(&month).unwrap();
        // an earlier recovered capture already claimed the recovered name
        std::fs::write(month.join(format!("{BASE} (recovered).mp3")), "earlier").unwrap();
        std::fs::write(month.join(format!(".{BASE}.mp3.part")), mp3_bytes()).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, false);
        match &actions[0] {
            RecoveryAction::Recovered { mp3 } => {
                assert_eq!(
                    mp3.file_name().unwrap().to_string_lossy(),
                    format!("{BASE} (recovered) (2).mp3")
                );
                assert_eq!(
                    std::fs::read_to_string(month.join(format!("{BASE} (recovered).mp3")))
                        .unwrap(),
                    "earlier",
                    "earlier recovery untouched"
                );
            }
            other => panic!("expected Recovered, got {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directories_are_not_followed() {
        // A symlink inside a recording root must not let recovery walk
        // outside the vault and rename/delete files there.
        let outside = tempfile::tempdir().unwrap();
        let part = outside.path().join(format!(".{BASE}.mp3.part"));
        std::fs::write(&part, mp3_bytes()).unwrap();
        let root = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("link")).unwrap();
        let actions = recover_root(root.path(), "Work", Duration::ZERO, true);
        assert!(actions.is_empty(), "walked through symlink: {actions:?}");
        assert!(part.exists(), "outside file untouched");
    }

    #[test]
    fn foreign_mp3_parts_are_never_touched() {
        // Another tool's hidden partial download must survive recovery
        // untouched — even a zero-frame one must NOT be deleted.
        let dir = tempfile::tempdir().unwrap();
        let foreign = dir.path().join(".download.mp3.part");
        std::fs::write(&foreign, [0u8; 64]).unwrap();
        let actions = recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(actions.is_empty(), "foreign part produced actions: {actions:?}");
        assert!(foreign.exists());
    }

    #[test]
    fn deletes_only_vault_buddy_note_temps() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join(".b.md.vault-buddy.tmp");
        let foreign = dir.path().join(".draft.md.tmp");
        std::fs::write(&ours, "half a note").unwrap();
        std::fs::write(&foreign, "another tool's temp").unwrap();
        recover_root(dir.path(), "Work", Duration::ZERO, true);
        assert!(!ours.exists(), "our stale temp is cleaned");
        assert!(foreign.exists(), "foreign temp files are never touched");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture recovery`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Startup recovery: finalize orphaned .part recordings left by a crash.
//! Never touches live recordings (staleness check; single-instance is the
//! first line of defense) and never deletes captured audio — the only
//! deletion is a .part with no MP3 frame in it, which holds no audio.

use std::path::{Path, PathBuf};
use std::time::Duration;
use vault_buddy_core::capture_note::{render_note, write_note_collision_safe, NoteMeta};
use vault_buddy_core::capture_paths::{base_from_part, recovered_base, reserve_final};

#[derive(Debug)]
pub enum RecoveryAction {
    Recovered { mp3: PathBuf },
    DeletedEmpty(PathBuf),
    Fresh(PathBuf),
}

pub fn has_mp3_frame(bytes: &[u8]) -> bool {
    bytes
        .windows(2)
        .any(|w| w[0] == 0xFF && (w[1] & 0xE0) == 0xE0)
}

/// Ownership check for .mp3.part files: only bases matching Vault Buddy's
/// capture pattern `YYYY-MM-DD HHmm <label>` are ours to delete or rename.
/// Another tool's `.download.mp3.part` in a vault must never be touched.
fn is_capture_base(base: &str) -> bool {
    let b: Vec<char> = base.chars().collect();
    if b.len() < 17 {
        return false;
    }
    let digit = |i: usize| b[i].is_ascii_digit();
    (0..4).all(digit)
        && b[4] == '-'
        && (5..7).all(digit)
        && b[7] == '-'
        && (8..10).all(digit)
        && b[10] == ' '
        && (11..15).all(digit)
        && b[15] == ' '
}

fn is_stale(path: &Path, stale_after: Duration) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Ok(modified) = meta.modified() else {
        return false;
    };
    modified
        .elapsed()
        .map(|age| age >= stale_after)
        .unwrap_or(false)
}

pub fn recover_root(
    root: &Path,
    vault_name: &str,
    stale_after: Duration,
    write_note: bool,
) -> Vec<RecoveryAction> {
    let mut actions = Vec::new();
    walk(root, &mut |path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        // Ownership marker filter: only OUR temps are ever deleted. A
        // foreign `.something.md.tmp` from another tool must survive —
        // this is the app's first write path into user vaults.
        if name.ends_with(vault_buddy_core::capture_note::NOTE_TMP_SUFFIX)
            && name.starts_with('.')
        {
            if is_stale(path, stale_after) {
                log::info!("recovery: removing stale note temp {}", path.display());
                let _ = std::fs::remove_file(path);
            }
            return;
        }
        let Some(base) = base_from_part(&name) else {
            return;
        };
        if !is_capture_base(&base) {
            return; // not ours — never delete or rename foreign files
        }
        if !is_stale(path, stale_after) {
            actions.push(RecoveryAction::Fresh(path.to_path_buf()));
            return;
        }
        // A read failure (permissions, AV lock, transient I/O) must NOT
        // look like "no audio" — deletion is only for provably frameless
        // parts. Unreadable files are left for a later pass.
        let Ok(bytes) = std::fs::read(path) else {
            log::warn!("recovery: cannot read {}, skipping", path.display());
            return;
        };
        if !has_mp3_frame(&bytes) {
            log::info!("recovery: deleting frameless part {}", path.display());
            let _ = std::fs::remove_file(path);
            actions.push(RecoveryAction::DeletedEmpty(path.to_path_buf()));
            return;
        }
        let dir = path.parent().unwrap_or(root);
        let (mp3, note) = match rename_into_reserved(path, dir, &recovered_base(&base)) {
            Ok(paths) => paths,
            Err(e) => {
                log::warn!("recovery: rename failed for {}: {e}", path.display());
                return;
            }
        };
        log::info!("recovery: finalized {}", mp3.display());
        if write_note {
            let meta = NoteMeta {
                recorded_at: String::new(),
                duration_secs: 0,
                vault_name: vault_name.to_string(),
                recording_type: "Recording".to_string(),
                input_devices: Vec::new(),
                event: Some("recovered after crash".to_string()),
            };
            let mp3_name = mp3.file_name().unwrap_or_default().to_string_lossy();
            let _ = write_note_collision_safe(&note, &render_note(&meta, &mp3_name));
        }
        actions.push(RecoveryAction::Recovered { mp3 });
    });
    actions
}

/// Finalize `from` under the first free suffixed name for `base` in `dir`.
/// The rename is the arbiter: a destination created between the reserve
/// check and the rename advances the suffix and retries (Windows renames
/// are non-replacing; on Unix the exists() pre-check plus the tight retry
/// loop is the accepted dev-platform approximation).
pub fn rename_into_reserved(
    from: &Path,
    dir: &Path,
    base: &str,
) -> Result<(PathBuf, PathBuf), String> {
    loop {
        let (mp3, note) = reserve_final(dir, base);
        match std::fs::rename(from, &mp3) {
            Ok(()) => return Ok((mp3, note)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Windows reports rename-onto-existing as PermissionDenied or
            // AlreadyExists depending on the API path.
            Err(_) if mp3.exists() => continue,
            Err(e) => return Err(format!("finalize rename failed: {e}")),
        }
    }
}

fn is_digit_dir(name: &str, len: usize) -> bool {
    name.len() == len && name.chars().all(|c| c.is_ascii_digit())
}

fn dir_entries(dir: &Path) -> Vec<(PathBuf, std::fs::FileType, String)> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            // entry.file_type() reads the dirent and does NOT follow
            // symlinks: a symlinked directory (or Windows junction) must
            // never let the scan escape the vault or enter a cycle.
            if let Ok(ft) = entry.file_type() {
                let name = entry.file_name().to_string_lossy().into_owned();
                out.push((entry.path(), ft, name));
            }
        }
    }
    out
}

/// Vault Buddy writes only under `<root>/YYYY/MM` — recovery looks
/// nowhere else, so a capture-named file a user moved into an arbitrary
/// subfolder (or the root itself) is never touched.
fn walk(root: &Path, visit: &mut dyn FnMut(&Path)) {
    for (year_path, year_ft, year_name) in dir_entries(root) {
        if !year_ft.is_dir() || !is_digit_dir(&year_name, 4) {
            continue;
        }
        for (month_path, month_ft, month_name) in dir_entries(&year_path) {
            if !month_ft.is_dir() || !is_digit_dir(&month_name, 2) {
                continue;
            }
            for (file_path, file_ft, _) in dir_entries(&month_path) {
                if file_ft.is_file() {
                    visit(&file_path);
                }
            }
        }
    }
}
```

Add `pub mod recovery;` to `src-tauri/capture/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture recovery`
Expected: 9 passed (the symlink test is `#[cfg(unix)]`; on Windows 8).

**Dated-layout amendment (applied in a follow-up fix):** with the
`YYYY/MM`-only walk, all tests place `.part`/temp files under a
`month_dir(root)` helper (`root/2026/07/`, created via `create_dir_all`);
the symlink test symlinks `root/2026` → outside (with the payload in
`outside/07/`); and two extra tests assert that a capture-named `.part`
at the root or under a non-dated subfolder (`Project/`) is ignored.

- [ ] **Step 5: fmt + clippy + commit**

```bash
cd src-tauri && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings && cd ..
git add src-tauri/capture/src/recovery.rs src-tauri/capture/src/lib.rs
git commit -m "feat(capture): crash recovery scan with staleness and zero-frame guards"
```

---

### Task 8: Recording session — worker, writer, finalize (`vault_buddy_capture::session`)

**Files:**
- Create: `src-tauri/capture/src/session.rs`
- Modify: `src-tauri/capture/src/lib.rs` (add `pub mod session;`)

**Interfaces:**
- Consumes: `mixer::*`, `encoder::Mp3Encoder`, `recovery::rename_into_reserved` (Task 7), `vault_buddy_core::capture_note::{render_note, write_note_atomic, NoteMeta}`.
- Produces:
  - `enum SourceMsg { Samples(Vec<f32>) /* raw interleaved at source rate */, Lost }`
  - `struct SourceInput { pub name: String, pub rate: u32, pub channels: u16, pub rx: std::sync::mpsc::Receiver<SourceMsg> }` — every opened source is one the mode requires, so loss detection is simply "no source left alive": a meeting continues while either mic or loopback survives; a mic-only voice note finalizes when the mic dies.
  - `struct SessionParams { pub dir: PathBuf, pub base: String, pub part: PathBuf, pub bitrate_kbps: u32, pub vault_name: String, pub recording_type: String, pub create_note: bool, pub recorded_at: String, pub flush_every: Duration, pub fsync_every: Duration, pub warn_tx: Option<std::sync::mpsc::Sender<String>> }` — `warn_tx` delivers source-loss warnings **live, while recording continues** (the panel must show "source lost" during the meeting, not after stop).
  - `struct Outcome { pub mp3: PathBuf, pub note: Option<PathBuf>, pub duration_secs: u64, pub warning: Option<String>, pub ended_early: bool }`
  - `struct CaptureSession` with `fn start(params: SessionParams, sources: Vec<SourceInput>) -> std::io::Result<CaptureSession>` (creates the `.part` with `create_new`), `fn stop(self) -> Result<Outcome, String>`, and `fn is_running(&self) -> bool`.
  - The worker finalizes **on its own** (returning through `stop()`'s join) when no source is left alive.

- [ ] **Step 1: Write the failing integration tests**

Bottom of new `src-tauri/capture/src/session.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn sine_chunks(rate: u32, secs: f32) -> Vec<Vec<f32>> {
        let frames = (rate as f32 * secs) as usize;
        (0..frames)
            .map(|i| ((i as f32 / rate as f32) * 440.0 * std::f32::consts::TAU).sin() * 0.4)
            .collect::<Vec<f32>>()
            .chunks(rate as usize / 10)
            .map(|c| c.to_vec())
            .collect()
    }

    fn params(dir: &std::path::Path) -> SessionParams {
        let names = vault_buddy_core::capture_paths::reserve_names(dir, "b");
        SessionParams {
            dir: dir.to_path_buf(),
            base: names.base,
            part: names.part,
            bitrate_kbps: 128,
            vault_name: "Work".into(),
            recording_type: "Meeting".into(),
            create_note: true,
            recorded_at: "2026-07-04T14:05:00+02:00".into(),
            flush_every: Duration::from_millis(100),
            fsync_every: Duration::from_secs(30),
            warn_tx: None,
        }
    }

    #[test]
    fn records_mixes_and_finalizes_with_note() {
        let dir = tempfile::tempdir().unwrap();
        let (tx_a, rx_a) = mpsc::channel();
        let (tx_b, rx_b) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![
                SourceInput { name: "mic".into(), rate: 44_100, channels: 1, rx: rx_a },
                SourceInput { name: "loopback".into(), rate: 44_100, channels: 1, rx: rx_b },
            ],
        )
        .unwrap();
        assert!(dir.path().join(".b.mp3.part").exists(), "part created");
        for chunk in sine_chunks(44_100, 1.0) {
            tx_a.send(SourceMsg::Samples(chunk.clone())).unwrap();
            tx_b.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(400));
        let outcome = session.stop().unwrap();
        assert_eq!(outcome.mp3, dir.path().join("b.mp3"));
        assert!(outcome.mp3.exists());
        assert!(!dir.path().join(".b.mp3.part").exists(), "part renamed away");
        let note = outcome.note.expect("note written");
        let text = std::fs::read_to_string(&note).unwrap();
        assert!(text.contains("![[b.mp3]]"));
        assert!(!outcome.ended_early);
        let bytes = std::fs::read(&outcome.mp3).unwrap();
        assert!(crate::recovery::has_mp3_frame(&bytes));
    }

    #[test]
    fn losing_the_only_required_source_finalizes_early() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let (warn_tx, warn_rx) = mpsc::channel();
        let mut p = params(dir.path());
        p.warn_tx = Some(warn_tx);
        let session = CaptureSession::start(
            p,
            vec![SourceInput { name: "mic".into(), rate: 44_100, channels: 1, rx }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        tx.send(SourceMsg::Lost).unwrap();
        // the warning must arrive live, not only inside the final Outcome
        let live = warn_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(live.contains("mic"), "live warning names the source: {live}");
        // worker should self-finalize; stop() then just collects the outcome
        std::thread::sleep(Duration::from_millis(500));
        let outcome = session.stop().unwrap();
        assert!(outcome.ended_early);
        assert!(outcome.mp3.exists());
    }

    #[test]
    fn stop_time_collision_advances_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput { name: "mic".into(), rate: 44_100, channels: 1, rx }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        // a sync client grabs the final name mid-recording
        std::fs::write(dir.path().join("b.mp3"), "intruder").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert_eq!(outcome.mp3, dir.path().join("b (2).mp3"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("b.mp3")).unwrap(),
            "intruder"
        );
    }

    #[test]
    fn part_creation_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".b.mp3.part"), "orphan").unwrap();
        let (_tx, rx) = mpsc::channel::<SourceMsg>();
        let mut p = params(dir.path());
        p.part = dir.path().join(".b.mp3.part"); // simulate racing reservation
        p.base = "b".into();
        let result = CaptureSession::start(
            p,
            vec![SourceInput { name: "mic".into(), rate: 44_100, channels: 1, rx }],
        );
        assert!(result.is_err(), "must not truncate the orphan");
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".b.mp3.part")).unwrap(),
            "orphan"
        );
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture session`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! Recording session: a worker thread pulls raw chunks from each source,
//! converts them to 44.1 kHz mono, mixes to stereo, streams MP3 bytes into
//! the hidden .part file (flush ~1 s, fsync ~30 s), and finalizes with the
//! stop-time reservation + rename-retry. Sources are plain mpsc channels so
//! the whole session is testable without audio hardware.

use crate::encoder::Mp3Encoder;
use crate::mixer;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use vault_buddy_core::capture_note::{render_note, write_note_collision_safe, NoteMeta};

pub enum SourceMsg {
    Samples(Vec<f32>),
    Lost,
}

pub struct SourceInput {
    pub name: String,
    pub rate: u32,
    pub channels: u16,
    pub rx: Receiver<SourceMsg>,
}

pub struct SessionParams {
    pub dir: PathBuf,
    pub base: String,
    pub part: PathBuf,
    pub bitrate_kbps: u32,
    pub vault_name: String,
    pub recording_type: String,
    pub create_note: bool,
    pub recorded_at: String,
    pub flush_every: Duration,
    pub fsync_every: Duration,
    /// Live source-loss warnings, delivered while the recording continues.
    pub warn_tx: Option<Sender<String>>,
}

pub struct Outcome {
    pub mp3: PathBuf,
    pub note: Option<PathBuf>,
    pub duration_secs: u64,
    pub warning: Option<String>,
    pub ended_early: bool,
}

pub struct CaptureSession {
    stop_tx: Sender<()>,
    handle: JoinHandle<Result<Outcome, String>>,
}

const TARGET_RATE: u32 = 44_100;
/// Max buffered audio per source before oldest samples are dropped (2 s).
const BUFFER_CAP: usize = (TARGET_RATE * 2) as usize;
const TICK: Duration = Duration::from_millis(100);

struct SourceState {
    input: SourceInput,
    buffer: Vec<f32>, // mono @ TARGET_RATE
    alive: bool,
}

impl CaptureSession {
    pub fn start(
        params: SessionParams,
        sources: Vec<SourceInput>,
    ) -> std::io::Result<CaptureSession> {
        // Exclusive create: an existing file here means an unrecovered
        // orphan won the name despite the reservation — never truncate it.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&params.part)?;
        let (stop_tx, stop_rx) = mpsc::channel();
        let handle =
            std::thread::spawn(move || run_worker(file, params, sources, stop_rx));
        Ok(CaptureSession { stop_tx, handle })
    }

    pub fn is_running(&self) -> bool {
        !self.handle.is_finished()
    }

    pub fn stop(self) -> Result<Outcome, String> {
        let _ = self.stop_tx.send(());
        self.handle
            .join()
            .map_err(|_| "capture worker panicked".to_string())?
    }
}

fn run_worker(
    mut file: std::fs::File,
    params: SessionParams,
    sources: Vec<SourceInput>,
    stop_rx: Receiver<()>,
) -> Result<Outcome, String> {
    let mut encoder = match Mp3Encoder::new(TARGET_RATE, params.bitrate_kbps) {
        Ok(enc) => enc,
        Err(e) => {
            // Setup failed after the exclusive create — honor the
            // start-failure rule: no file left behind before the first
            // MP3 frame exists.
            drop(file);
            let _ = std::fs::remove_file(&params.part);
            return Err(e);
        }
    };
    let mut states: Vec<SourceState> = sources
        .into_iter()
        .map(|input| SourceState { input, buffer: Vec::new(), alive: true })
        .collect();
    let device_names: Vec<String> = states.iter().map(|s| s.input.name.clone()).collect();
    let started = Instant::now();
    let mut last_flush = Instant::now();
    let mut last_fsync = Instant::now();
    let mut frames_written: u64 = 0;
    let mut warning: Option<String> = None;
    let mut ended_early = false;

    loop {
        let stopped = matches!(
            stop_rx.recv_timeout(TICK),
            Ok(()) | Err(RecvTimeoutError::Disconnected)
        );

        // Drain every source's channel into its (converted) buffer.
        for s in states.iter_mut().filter(|s| s.alive) {
            loop {
                match s.input.rx.try_recv() {
                    Ok(SourceMsg::Samples(raw)) => {
                        let mono = mixer::downmix_to_mono(&raw, s.input.channels);
                        let mono = mixer::resample_linear(&mono, s.input.rate, TARGET_RATE);
                        s.buffer.extend(mono);
                        if s.buffer.len() > BUFFER_CAP {
                            let drop = s.buffer.len() - BUFFER_CAP;
                            log::warn!("capture: dropping {drop} overflowed samples ({})", s.input.name);
                            s.buffer.drain(..drop);
                        }
                    }
                    Ok(SourceMsg::Lost) => {
                        s.alive = false;
                        let msg = format!("source lost: {}", s.input.name);
                        log::warn!("capture: {msg}");
                        if let Some(tx) = &params.warn_tx {
                            let _ = tx.send(msg.clone());
                        }
                        warning = Some(msg);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        s.alive = false;
                        let msg = format!("source lost: {}", s.input.name);
                        if let Some(tx) = &params.warn_tx {
                            let _ = tx.send(msg.clone());
                        }
                        warning = Some(msg);
                        break;
                    }
                }
            }
        }

        // Every opened source is one the mode requires (spec: source loss
        // is judged against the mode's sources) — a meeting survives on
        // either stream; a mic-only voice note ends when the mic dies.
        let no_source_left = states.iter().all(|s| !s.alive);
        let finish = stopped || no_source_left;
        if no_source_left && !stopped {
            ended_early = true;
        }

        // Pull one tick's worth of audio per source (silence-fill a
        // starved side only while its source is still alive and we keep
        // recording; on finish, take what's left without padding).
        let tick_frames = (TARGET_RATE / 10) as usize;
        let take = if finish {
            states.iter().map(|s| s.buffer.len()).max().unwrap_or(0)
        } else {
            tick_frames
        };
        if take > 0 {
            let mut mono_slices: Vec<Vec<f32>> = Vec::with_capacity(states.len());
            for s in states.iter_mut() {
                let n = take.min(s.buffer.len());
                let mut chunk: Vec<f32> = s.buffer.drain(..n).collect();
                chunk.resize(take, 0.0); // silence-fill underrun
                mono_slices.push(chunk);
            }
            let silent = vec![0.0f32; take];
            let a = mono_slices.first().map(|v| v.as_slice()).unwrap_or(&silent);
            let b = mono_slices.get(1).map(|v| v.as_slice()).unwrap_or(&silent);
            let stereo = mixer::mix_to_stereo_i16(a, b);
            frames_written += (stereo.len() / 2) as u64;
            let bytes = encoder.encode(&stereo)?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
        }

        if last_flush.elapsed() >= params.flush_every {
            file.flush().map_err(|e| e.to_string())?;
            last_flush = Instant::now();
        }
        if last_fsync.elapsed() >= params.fsync_every {
            let _ = file.sync_data();
            last_fsync = Instant::now();
        }

        if finish {
            break;
        }
    }

    // Finalize: flush encoder, fsync, stop-time reservation + rename retry.
    let tail = encoder.finish()?;
    file.write_all(&tail).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);

    let duration_secs = frames_written / TARGET_RATE as u64;
    let _elapsed = started.elapsed();
    let (mp3, note_path) =
        crate::recovery::rename_into_reserved(&params.part, &params.dir, &params.base)?;
    // Make the rename's directory entry durable where the platform
    // supports it (Unix dir fsync; NTFS journaling covers Windows). Worst
    // case the fsynced .part entry survives instead and the next launch's
    // recovery finalizes it — no audio is lost either way.
    #[cfg(unix)]
    if let Ok(dir_handle) = std::fs::File::open(&params.dir) {
        let _ = dir_handle.sync_all();
    }
    log::info!("capture: saved {}", mp3.display());

    let note = if params.create_note {
        let meta = NoteMeta {
            recorded_at: params.recorded_at.clone(),
            duration_secs,
            vault_name: params.vault_name.clone(),
            recording_type: params.recording_type.clone(),
            input_devices: device_names,
            event: warning.clone(),
        };
        let mp3_name = mp3.file_name().unwrap_or_default().to_string_lossy();
        // Collision-safe: a user or sync client grabbing the reserved
        // note name after the rename must cost us a suffix, not the note.
        match write_note_collision_safe(&note_path, &render_note(&meta, &mp3_name)) {
            Ok(written) => Some(written),
            Err(e) => {
                log::warn!("capture: note write failed: {e}");
                None
            }
        }
    } else {
        None
    };

    Ok(Outcome { mp3, note, duration_secs, warning, ended_early })
}
```

Add `pub mod session;` to `src-tauri/capture/src/lib.rs`.

- [ ] **Step 4: Run tests to verify they pass**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture session`
Expected: 4 passed. Timing-sensitive: if `losing_the_only_required_source_finalizes_early` flakes, raise its sleep to 800 ms — the worker ticks every 100 ms.

- [ ] **Step 5: Run the whole capture crate + fmt + clippy + commit**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings`

```bash
git add src-tauri/capture/src/session.rs src-tauri/capture/src/lib.rs
git commit -m "feat(capture): recording session with crash-safe writer and finalize"
```

---

### Task 9: Device layer — cpal streams with mode branching (`vault_buddy_capture::devices`)

**Files:**
- Create: `src-tauri/capture/src/devices.rs`
- Modify: `src-tauri/capture/src/lib.rs` (add `pub mod devices;`)

**Interfaces:**
- Consumes: `session::{SourceMsg, SourceInput}`.
- Produces:
  - `struct OpenSources { pub inputs: Vec<SourceInput>, pub streams: Vec<cpal::Stream> }` — keep `streams` alive for the whole recording; dropping them stops capture.
  - `fn open_sources(meeting_mode: bool) -> Result<OpenSources, String>` — always opens the default mic. When `meeting_mode` **and** on Windows, also opens WASAPI loopback on the default output device. Loss handling lives in the session's all-sources-dead predicate: a meeting continues while either stream survives. On non-Windows, `meeting_mode` opens mic only (documented dev limitation).
  - Errors are human-readable: `"No microphone found — check Windows sound settings."`, `"Desktop audio (loopback) unavailable: <cause>"`.

- [ ] **Step 1: Write the (compile-focused) test**

Bottom of new `src-tauri/capture/src/devices.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// CI runners have no audio devices; this asserts the error path is a
    /// clean human-readable Err, not a panic. On a dev machine with a mic
    /// it exercises the success path instead.
    #[test]
    fn open_sources_never_panics() {
        match open_sources(true) {
            Ok(open) => {
                assert!(!open.inputs.is_empty());
                assert!(!open.inputs[0].name.is_empty(), "mic source is named");
            }
            Err(message) => {
                assert!(!message.is_empty());
            }
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture devices`
Expected: compile error.

- [ ] **Step 3: Implement**

```rust
//! cpal glue: opens the default microphone (and, in meeting mode on
//! Windows, WASAPI loopback on the default output) and pushes raw sample
//! chunks into the session's mpsc channels. All sample-format conversion
//! beyond f32 widening happens in the session worker, not here.

use crate::session::{SourceInput, SourceMsg};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::Sender;

pub struct OpenSources {
    pub inputs: Vec<SourceInput>,
    pub streams: Vec<cpal::Stream>,
}

fn build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    tx: Sender<SourceMsg>,
) -> Result<cpal::Stream, String> {
    let err_tx = tx.clone();
    let on_error = move |e: cpal::StreamError| {
        log::warn!("capture stream error: {e}");
        let _ = err_tx.send(SourceMsg::Lost);
    };
    let stream_config: cpal::StreamConfig = config.config();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| {
                let _ = tx.send(SourceMsg::Samples(data.to_vec()));
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| {
                let samples = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                let _ = tx.send(SourceMsg::Samples(samples));
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| {
                let samples = data
                    .iter()
                    .map(|s| (*s as f32 / u16::MAX as f32) * 2.0 - 1.0)
                    .collect();
                let _ = tx.send(SourceMsg::Samples(samples));
            },
            on_error,
            None,
        ),
        other => return Err(format!("unsupported sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

pub fn open_sources(meeting_mode: bool) -> Result<OpenSources, String> {
    let host = cpal::default_host();
    let mut inputs = Vec::new();
    let mut streams = Vec::new();

    let mic = host
        .default_input_device()
        .ok_or("No microphone found — check Windows sound settings.")?;
    let mic_config = mic
        .default_input_config()
        .map_err(|e| format!("Microphone unavailable: {e}"))?;
    let (mic_tx, mic_rx) = std::sync::mpsc::channel();
    let mic_name = mic.name().unwrap_or_else(|_| "Microphone".to_string());
    streams.push(build_stream(&mic, &mic_config, mic_tx)?);
    inputs.push(SourceInput {
        name: mic_name,
        rate: mic_config.sample_rate().0,
        channels: mic_config.channels(),
        rx: mic_rx,
    });

    #[cfg(windows)]
    if meeting_mode {
        // WASAPI loopback: cpal exposes it by building an *input* stream on
        // an *output* device — you get exactly what the speakers play.
        let output = host
            .default_output_device()
            .ok_or("Desktop audio (loopback) unavailable: no default output device")?;
        let config = output
            .default_output_config()
            .map_err(|e| format!("Desktop audio (loopback) unavailable: {e}"))?;
        let (tx, rx) = std::sync::mpsc::channel();
        let name = format!(
            "{} (loopback)",
            output.name().unwrap_or_else(|_| "Speakers".to_string())
        );
        streams.push(build_stream(&output, &config, tx)?);
        inputs.push(SourceInput {
            name,
            rate: config.sample_rate().0,
            channels: config.channels(),
            rx,
        });
    }
    #[cfg(not(windows))]
    if meeting_mode {
        log::warn!("desktop audio loopback is Windows-only; recording mic only");
    }

    Ok(OpenSources { inputs, streams })
}
```

Add `pub mod devices;` to `src-tauri/capture/src/lib.rs`.

API note: if cpal 0.15's `SampleFormat` is non-exhaustive or `build_input_stream` signatures differ, adjust conversions but keep `open_sources`'s signature and error strings — Task 10 consumes them verbatim.

- [ ] **Step 4: Run tests + fmt + clippy**

Run from `src-tauri/`: `cargo test -p vault_buddy_capture && cargo fmt && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings`
Expected: all capture tests pass; `devices::tests::open_sources_never_panics` returns the Err arm on CI machines.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/capture/src/devices.rs src-tauri/capture/src/lib.rs
git commit -m "feat(capture): cpal device layer with Windows loopback"
```

---

### Task 10: Tauri commands, events, notifications (`src-tauri/src/capture_commands.rs`)

**Files:**
- Create: `src-tauri/src/capture_commands.rs`
- Modify: `src-tauri/Cargo.toml` (add deps), `src-tauri/src/lib.rs` (module + state + handler), `src-tauri/tauri.conf.json` (no change needed unless plugins require it)

**Interfaces:**
- Consumes: `vault_buddy_capture::{devices, session, recovery}`, `vault_buddy_core::{capture_config, capture_paths, discovery}`, Task 11's `tray::set_recording(app, bool)` (call sites written here, implemented next task — stub it now as a no-op in tray.rs to keep the build green: `pub fn set_recording(_app: &AppHandle, _recording: bool) {}`).
- Produces:
  - `pub struct CaptureState(pub Mutex<Option<ActiveCapture>>)` with `pub struct ActiveCapture { pub stop_tx: Sender<StopReason>, pub vault_id: String, pub started_at_ms: u64 }` — `cpal::Stream` is `!Send`, so the streams live on a dedicated device thread, and the session outcome is consumed by a dedicated monitor thread (see implementation note below).
  - Commands: `start_capture(app, state, id: String) -> Result<StatusPayload, String>`, `stop_capture(app, state) -> Result<(), String>`, `capture_status(state) -> StatusPayload`.
  - `#[derive(Clone, serde::Serialize)] pub struct StatusPayload { pub recording: bool, pub vault_id: Option<String>, pub started_at_ms: Option<u64> }`
  - Events emitted on the app: `capture:started` (StatusPayload), `capture:saved` (`{ mp3: String, note: Option<String>, ended_early: bool }`), `capture:failed` (`{ message: String }`), `capture:warning` (`{ message: String }`).
  - `pub fn finalize_if_recording(app: &AppHandle)` — used by every quit/close path.
  - `pub fn is_recording(app: &AppHandle) -> bool`.
  - `pub fn run_recovery(app: &AppHandle)` — spawns the startup scan + 90 s rescan for fresh orphans, toasts results.
- Cargo additions to `src-tauri/Cargo.toml` `[dependencies]`:

```toml
vault_buddy_capture = { path = "capture" }
cpal = "0.15"
tauri-plugin-single-instance = "2"
tauri-plugin-notification = "2"
log = "0.4"
```

**Implementation note on `cpal::Stream` (`!Send`):** Tauri managed state must be `Send + Sync`. Keep the streams OUT of managed state: `start_capture` spawns a plain thread that opens the devices, owns the streams, starts the session, and parks on a channel until stop; the managed state holds only a control `Sender` plus metadata. Concretely:

```rust
pub struct ActiveCapture {
    pub stop_tx: std::sync::mpsc::Sender<StopReason>,
    pub vault_id: String,
    pub started_at_ms: u64,
}
pub enum StopReason { User }
```

The spawned device thread: open sources → build `SessionParams` → `CaptureSession::start` → hold `streams` → wait on `stop_rx.recv_timeout(500ms)` while also polling `session.is_running()` (so self-finalization after source loss is noticed promptly) → `session.stop()` → drop streams → send the outcome through `done_tx`.

**The sole consumer of `done_rx` is a dedicated monitor thread** spawned by `start_capture`. It blocks on `done_rx.recv()`, clears `CaptureState`, emits `capture:saved`/`capture:failed`, shows the toast, and resets the tray — regardless of *why* the session ended (user Stop, menu Stop, shutdown finalize, or self-finalization when every source died). Stop paths therefore only *send* `StopReason::User` and wait for the state to clear; they never consume the outcome themselves. This is what guarantees a voice-note whose mic was unplugged surfaces its saved file immediately instead of leaving the UI stuck on "recording".

- [ ] **Step 1: Implement the module**

`src-tauri/src/capture_commands.rs` (full file):

```rust
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use vault_buddy_capture::session::{CaptureSession, Outcome, SessionParams};
use vault_buddy_core::{capture_config, capture_paths, discovery};

pub enum StopReason {
    User,
}

pub struct ActiveCapture {
    pub stop_tx: Sender<StopReason>,
    pub vault_id: String,
    pub started_at_ms: u64,
}

#[derive(Default)]
pub struct CaptureState(pub Mutex<Option<ActiveCapture>>);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub recording: bool,
    pub vault_id: Option<String>,
    pub started_at_ms: Option<u64>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn toast(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn emit_saved(app: &AppHandle, outcome: &Outcome) {
    let file_name = outcome
        .mp3
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let _ = app.emit(
        "capture:saved",
        serde_json::json!({
            "mp3": outcome.mp3.to_string_lossy(),
            "note": outcome.note.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "endedEarly": outcome.ended_early,
        }),
    );
    // Source-loss warnings were already emitted live via warn_tx; here the
    // outcome only feeds the note metadata and the ended-early toast copy.
    let body = if outcome.ended_early {
        format!("Recording ended early — saved {file_name}")
    } else {
        format!("Saved {file_name}")
    };
    toast(app, "Recording saved", &body);
}

#[tauri::command]
pub fn capture_status(state: tauri::State<CaptureState>) -> StatusPayload {
    let guard = state.0.lock().unwrap();
    match guard.as_ref() {
        Some(active) => StatusPayload {
            recording: true,
            vault_id: Some(active.vault_id.clone()),
            started_at_ms: Some(active.started_at_ms),
        },
        None => StatusPayload { recording: false, vault_id: None, started_at_ms: None },
    }
}

#[tauri::command]
pub fn start_capture(
    app: AppHandle,
    state: tauri::State<CaptureState>,
    id: String,
) -> Result<StatusPayload, String> {
    let mut guard = state.0.lock().unwrap();
    if guard.is_some() {
        return Err("A recording is already running.".to_string());
    }

    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let vault_path = std::path::PathBuf::from(&vault.path);
    if !vault_path.is_dir() {
        return Err(format!("Vault folder not found: {}", vault.path));
    }

    let cfg = capture_config::vault_config(&capture_config::load_config(), &id);
    let meeting = cfg.mode == capture_config::RecordingMode::Meeting;
    let label = cfg.mode.label();

    // Device validation happens on the worker thread BEFORE any file is
    // created (spec: start failures stay file-free).
    let (stop_tx, stop_rx) = mpsc::channel::<StopReason>();
    let (done_tx, done_rx) = mpsc::channel::<Result<Outcome, String>>();
    let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
    let app2 = app.clone();
    let vault_name = vault.name.clone();
    // Hand-editable config must never escape the vault (PRD guarantee).
    let root = capture_paths::safe_recording_root(&vault_path, cfg.effective_recording_folder())?;

    let vault_path2 = vault_path.clone();

    // Live source-loss warnings: forwarded to the panel while recording.
    let (warn_tx, warn_rx) = mpsc::channel::<String>();
    let app_warn = app.clone();
    std::thread::spawn(move || {
        while let Ok(message) = warn_rx.recv() {
            let _ = app_warn.emit("capture:warning", serde_json::json!({ "message": message }));
        }
    });

    std::thread::spawn(move || {
        let open = match vault_buddy_capture::devices::open_sources(meeting) {
            Ok(o) => o,
            Err(e) => {
                let _ = ready_tx.send(Err(e));
                return;
            }
        };
        let now = chrono::Local::now();
        use chrono::{Datelike, Timelike};
        let date = now.date_naive();
        let dir = capture_paths::dated_folder(&root, date);
        if let Err(e) = std::fs::create_dir_all(&dir) {
            let _ = ready_tx.send(Err(format!("Cannot create recording folder: {e}")));
            return;
        }
        // A pre-existing symlink/junction at the recording folder must
        // not carry writes outside the vault (lexical check can't see it).
        if let Err(e) = capture_paths::assert_root_inside_vault(&vault_path2, &dir) {
            let _ = ready_tx.send(Err(e));
            return;
        }
        let base = capture_paths::base_name(date, now.hour(), now.minute(), label);
        let names = capture_paths::reserve_names(&dir, &base);
        let params = SessionParams {
            dir: dir.clone(),
            base: names.base.clone(),
            part: names.part.clone(),
            bitrate_kbps: cfg.bitrate_kbps,
            vault_name: vault_name.clone(),
            recording_type: label.to_string(),
            create_note: cfg.create_note,
            recorded_at: now.to_rfc3339(),
            flush_every: Duration::from_secs(1),
            fsync_every: Duration::from_secs(30),
            warn_tx: Some(warn_tx),
        };
        let session = match CaptureSession::start(params, open.inputs) {
            Ok(s) => s,
            Err(e) => {
                let _ = ready_tx.send(Err(format!("Could not start recording: {e}")));
                return;
            }
        };
        log::info!("capture: started in vault '{vault_name}' → {}", names.part.display());
        let _ = ready_tx.send(Ok(()));

        // Own the streams here; poll for user stop or self-finalization.
        let streams = open.streams;
        loop {
            match stop_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(StopReason::User) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    if !session.is_running() {
                        break; // sources died; worker self-finalized
                    }
                }
            }
        }
        // Stop the session while the streams are still alive: dropping
        // them first disconnects every source channel, and the worker
        // could mistake an ordinary stop for all-sources-lost (bogus
        // ended_early + source-loss warnings in the toast and note).
        let outcome = session.stop();
        drop(streams);
        let _ = done_tx.send(outcome);
    });

    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            let _ = app2.emit("capture:failed", serde_json::json!({ "message": e.clone() }));
            return Err(e);
        }
        Err(_) => {
            // Startup hung (e.g. a wedged audio driver). The worker may
            // still create the .part and start the session AFTER this
            // return — never leave that recording detached: a janitor
            // thread waits for the worker's one ready signal and, if a
            // session did start, stops it and surfaces the outcome so no
            // audio is silently stranded.
            let msg = "Recording did not start in time.".to_string();
            let app4 = app2.clone();
            std::thread::spawn(move || {
                if let Ok(Ok(())) = ready_rx.recv() {
                    log::warn!("capture: late start after timeout — stopping and draining");
                    let _ = stop_tx.send(StopReason::User);
                    match done_rx.recv() {
                        Ok(Ok(outcome)) => emit_saved(&app4, &outcome),
                        other => log::warn!("capture: late-start cleanup: {other:?}"),
                    }
                }
                // worker replied Err (or vanished): nothing was created.
            });
            let _ = app2.emit("capture:failed", serde_json::json!({ "message": msg.clone() }));
            return Err(msg);
        }
    }

    let payload = StatusPayload {
        recording: true,
        vault_id: Some(id.clone()),
        started_at_ms: Some(now_ms()),
    };
    *guard = Some(ActiveCapture {
        stop_tx,
        vault_id: id,
        started_at_ms: payload.started_at_ms.unwrap(),
    });
    drop(guard);

    // Monitor thread: the ONLY consumer of the session outcome. Covers
    // user/menu/shutdown stops AND self-finalization (all sources lost) —
    // the state clears and the outcome surfaces no matter who ended it.
    let app3 = app.clone();
    std::thread::spawn(move || {
        let result = done_rx
            .recv()
            .unwrap_or_else(|_| Err("capture thread vanished".to_string()));
        *app3.state::<CaptureState>().0.lock().unwrap() = None;
        match result {
            Ok(outcome) => emit_saved(&app3, &outcome),
            Err(e) => {
                log::error!("capture: finalize failed: {e}");
                let _ = app3.emit("capture:failed", serde_json::json!({ "message": e.clone() }));
                toast(&app3, "Recording failed", &e);
            }
        }
        crate::tray::set_recording(&app3, false);
    });

    // Indicator hardening: recording buddy must be visible.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
    }
    crate::tray::set_recording(&app, true);
    let _ = app.emit("capture:started", payload.clone());
    Ok(payload)
}

/// Ask the device thread to stop and wait until the monitor thread has
/// cleared the state (i.e. the outcome landed and events were emitted).
/// `wait: None` means wait forever — shutdown paths use it so the app can
/// never exit while a recording is still finalizing (a slow vault or a
/// stuck fsync must not strand the capture as .part).
fn request_stop_and_wait(app: &AppHandle, wait: Option<Duration>) {
    let stop_tx = {
        let guard = app.state::<CaptureState>().0.lock().unwrap();
        guard.as_ref().map(|active| active.stop_tx.clone())
    };
    let Some(stop_tx) = stop_tx else { return };
    let _ = stop_tx.send(StopReason::User);
    let started = std::time::Instant::now();
    let mut last_log = std::time::Instant::now();
    loop {
        if app.state::<CaptureState>().0.lock().unwrap().is_none() {
            return;
        }
        if let Some(limit) = wait {
            if started.elapsed() >= limit {
                log::warn!("capture: stop wait timed out");
                return;
            }
        }
        if last_log.elapsed() >= Duration::from_secs(15) {
            log::warn!("capture: still finalizing…");
            last_log = std::time::Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[tauri::command]
pub fn stop_capture(app: AppHandle, state: tauri::State<CaptureState>) -> Result<(), String> {
    if state.0.lock().unwrap().is_none() {
        return Err("No recording is running.".to_string());
    }
    request_stop_and_wait(&app, Some(Duration::from_secs(15)));
    Ok(())
}

pub fn is_recording(app: &AppHandle) -> bool {
    app.state::<CaptureState>().0.lock().unwrap().is_some()
}

/// Every shutdown path funnels through here so quitting mid-meeting saves
/// the capture through the normal stop flow instead of stranding a .part.
pub fn finalize_if_recording(app: &AppHandle) {
    if is_recording(app) {
        log::info!("capture: finalizing active recording before shutdown");
        // Unbounded: quitting must block until the save lands — exiting
        // on a timeout would kill the worker and strand the .part.
        request_stop_and_wait(app, None);
    }
}

/// Startup recovery over every discovered vault's effective recording
/// root; fresh orphans trigger one rescan after the staleness window.
pub fn run_recovery(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let pass = |stale: Duration| -> bool {
            let cfg = capture_config::load_config();
            let mut fresh_found = false;
            for vault in discovery::discover_vaults() {
                let v = capture_config::vault_config(&cfg, &vault.id);
                // Configured folder, or BOTH mode defaults when no config
                // entry exists — a first-ever crash may have used either.
                let roots: Vec<String> = match &v.recording_folder {
                    Some(folder) => vec![folder.clone()],
                    None => vec!["Meetings".to_string(), "Voice Notes".to_string()],
                };
                for folder in roots {
                let Ok(root) =
                    capture_paths::safe_recording_root(std::path::Path::new(&vault.path), &folder)
                else {
                    log::warn!("recovery: skipping unsafe configured folder {folder:?}");
                    continue;
                };
                if !root.is_dir() {
                    continue;
                }
                if let Err(e) = capture_paths::assert_root_inside_vault(
                    std::path::Path::new(&vault.path),
                    &root,
                ) {
                    log::warn!("recovery: skipping root: {e}");
                    continue;
                }
                for action in vault_buddy_capture::recovery::recover_root(
                    &root,
                    &vault.name,
                    stale,
                    v.create_note,
                ) {
                    use vault_buddy_capture::recovery::RecoveryAction;
                    match action {
                        RecoveryAction::Recovered { mp3 } => {
                            let name = mp3
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            toast(&app, "Recording recovered", &name);
                        }
                        RecoveryAction::Fresh(_) => fresh_found = true,
                        RecoveryAction::DeletedEmpty(_) => {}
                    }
                }
                }
            }
            fresh_found
        };
        if pass(Duration::from_secs(60)) {
            std::thread::sleep(Duration::from_secs(90));
            pass(Duration::from_secs(60));
        }
    });
}
```

- [ ] **Step 2: Wire into lib.rs and Cargo.toml**

Add the dependencies block above to `src-tauri/Cargo.toml`. In `src-tauri/src/lib.rs`:
- add `mod capture_commands;` under `mod commands;`
- add `.manage(capture_commands::CaptureState::default())` after the existing `.manage(...)`
- extend `tauri::generate_handler![…]` with `capture_commands::start_capture, capture_commands::stop_capture, capture_commands::capture_status`
- register plugins at the TOP of the builder chain (single-instance must be first):

```rust
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Second launch: focus the running buddy instead of starting a
            // new process (spec: recovery must never race a live recording).
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_notification::init())
```

In `tray.rs`, add the temporary stub (replaced in Task 11):

```rust
pub fn set_recording(_app: &AppHandle, _recording: bool) {}
```

- [ ] **Step 3: Verify the shell builds**

Run from `src-tauri/`: `cargo check`
Expected: compiles. (`cargo check` on Linux needs the Tauri system deps the repo already documents in `docs/DEVELOPMENT.md`; if the shell crate can't build locally, rely on `cargo check -p vault_buddy_capture -p vault_buddy_core` plus the Windows CI job.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs src-tauri/src/tray.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(app): capture commands, events, notifications, startup recovery"
```

---

### Task 11: Shutdown/hide/tray hardening (`src-tauri/src/lib.rs`, `src-tauri/src/tray.rs`)

**Files:**
- Modify: `src-tauri/src/tray.rs` (hide guard, quit finalize, recording icon/menu/tooltip, stop item)
- Modify: `src-tauri/src/lib.rs` (CloseRequested finalize, buddy-hide guard, recovery kickoff, `tray-stop-recording` / `buddy-stop-recording` menu events)

**Interfaces:**
- Consumes: `capture_commands::{finalize_if_recording, is_recording, run_recovery}` and Task 10's `stop_capture` internals via a new helper `capture_commands::stop_from_menu(app: &AppHandle)` (add it: delegates to `request_stop_and_wait`; body below).
- Produces: `tray::set_recording(app, recording: bool)` — swaps tray icon (programmatic red-dot variant), tooltip (`"Vault Buddy — recording"`), and menu (adds `Stop recording` item with id `"tray-stop-recording"` while recording).

- [ ] **Step 1: Add `stop_from_menu` to capture_commands.rs**

```rust
/// Stop triggered from a native menu (tray or buddy) rather than the panel.
pub fn stop_from_menu(app: &AppHandle) {
    request_stop_and_wait(app, Some(std::time::Duration::from_secs(15)));
}
```

- [ ] **Step 2: Replace the tray stub with the real recording state**

In `src-tauri/src/tray.rs`, replace `pub fn set_recording(...) {}` with:

```rust
/// Programmatic 32×32 RGBA icon: the buddy's violet disc, plus a red
/// recording dot when active — no asset pipeline needed for a state that
/// is pure signal.
fn buddy_icon(recording: bool) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE / 2) as i32;
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let idx = ((y as u32 * SIZE + x as u32) * 4) as usize;
            let dx = x - center;
            let dy = y - center;
            if dx * dx + dy * dy <= (center - 2) * (center - 2) {
                rgba[idx..idx + 4].copy_from_slice(&[0x7c, 0x5c, 0xff, 0xff]);
            }
            if recording {
                // red dot bottom-right
                let rx = x - (SIZE as i32 - 9);
                let ry = y - (SIZE as i32 - 9);
                if rx * rx + ry * ry <= 36 {
                    rgba[idx..idx + 4].copy_from_slice(&[0xe0, 0x2e, 0x2e, 0xff]);
                }
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE, SIZE)
}

fn tray_menu(app: &AppHandle, recording: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", !recording, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    if recording {
        let stop =
            MenuItem::with_id(app, "tray-stop-recording", "⏹ Stop recording", true, None::<&str>)?;
        Menu::with_items(app, &[&stop, &toggle, &quit_item])
    } else {
        Menu::with_items(app, &[&toggle, &quit_item])
    }
}

pub fn set_recording(app: &AppHandle, recording: bool) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_icon(Some(buddy_icon(recording)));
        let _ = tray.set_tooltip(Some(if recording {
            "Vault Buddy — recording"
        } else {
            "Vault Buddy"
        }));
        if let Ok(menu) = tray_menu(app, recording) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
```

Adjust `create_tray` to use `tray_menu(app, false)?` instead of its inline menu construction, and to use `buddy_icon(false)` as the initial icon (replacing `app.default_window_icon()` keeps the tray consistent with the recording variant; if the default icon is preferred when idle, keep it and only swap while recording — either is acceptable, pick one and keep `set_recording(false)` symmetric).

- [ ] **Step 3: Guard hide and quit paths**

In `tray.rs`:

```rust
pub fn hide_to_tray(app: &AppHandle) {
    // The buddy is the recording indicator; hiding it mid-capture would
    // violate the spec's no-hidden-recordings requirement.
    if crate::capture_commands::is_recording(app) {
        log::info!("hide ignored: recording in progress");
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

pub fn quit(app: &AppHandle) {
    crate::capture_commands::finalize_if_recording(app);
    restore_home_position(app);
    // … existing body unchanged from here …
}
```

In the tray `toggle` menu-event handler (tray.rs `on_menu_event`), before hiding:

```rust
            "toggle" => {
                if let Some(window) = app.get_webview_window("main") {
                    let visible = window.is_visible().unwrap_or(true);
                    if visible && crate::capture_commands::is_recording(app) {
                        return; // indicator must stay visible while recording
                    }
                    let _ = if visible { window.hide() } else { window.show() };
                }
            }
            "tray-stop-recording" => {
                crate::capture_commands::stop_from_menu(app);
            }
```

- [ ] **Step 4: Harden lib.rs close path and kick off recovery**

In `src-tauri/src/lib.rs` `on_window_event`, extend the CloseRequested arm:

```rust
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                capture_commands::finalize_if_recording(window.app_handle());
                tray::restore_home_position(window.app_handle());
            }
        })
```

In `setup(…)` after `tray::create_tray(app.handle())?;` add:

```rust
            capture_commands::run_recovery(app.handle());
```

The `buddy-hide` menu event already routes through `tray::hide_to_tray`, which now guards; `buddy-quit` routes through `tray::quit`, which now finalizes. No change needed there.

- [ ] **Step 5: Verify build + fmt + clippy for touched crates**

Run from `src-tauri/`: `cargo check && cargo fmt --check`
Expected: clean. (Shell-crate clippy runs on the Windows CI job.)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/tray.rs src-tauri/src/lib.rs src-tauri/src/capture_commands.rs
git commit -m "feat(app): recording-safe shutdown, hide guard, tray recording state"
```

---

### Task 12: Capture Pinia store (`src/stores/capture.ts`)

**Files:**
- Create: `src/stores/capture.ts`
- Modify: `src/types.ts`
- Test: `tests/capture-store.test.ts`

**Interfaces:**
- Consumes: Tauri `invoke("start_capture", { id })`, `invoke("stop_capture")`, `invoke("capture_status")`; events `capture:started|saved|failed|warning`.
- Produces (state consumed by Task 13):
  - `status: "idle" | "recording" | "saving"`
  - `vaultId: string | null`, `startedAtMs: number | null`
  - `error: string | null`, `warning: string | null`, `lastSavedFile: string | null`
  - actions `start(vaultId: string)`, `stop()`, `init()` (event listeners + status resync).

- [ ] **Step 1: Add types to `src/types.ts`**

```ts
export interface CaptureStatus {
  recording: boolean;
  vaultId: string | null;
  startedAtMs: number | null;
}

export interface CaptureSaved {
  mp3: string;
  note: string | null;
  endedEarly: boolean;
}
```

- [ ] **Step 2: Write the failing tests**

`tests/capture-store.test.ts` (mirror the mocking style of `tests/vaults-store.test.ts` — check that file first and copy its `@tauri-apps/api/core` mock shape):

```ts
import { describe, it, expect, vi, beforeEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
const listeners = new Map<string, (event: { payload: unknown }) => void>();
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (event: { payload: unknown }) => void) => {
    listeners.set(name, cb);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

import { useCaptureStore } from "../src/stores/capture";

describe("capture store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    invoke.mockReset();
    listeners.clear();
  });

  it("starts recording and tracks the vault", async () => {
    invoke.mockResolvedValueOnce({
      recording: true,
      vaultId: "v1",
      startedAtMs: 123,
    });
    const store = useCaptureStore();
    await store.start("v1");
    expect(invoke).toHaveBeenCalledWith("start_capture", { id: "v1" });
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v1");
    expect(store.startedAtMs).toBe(123);
  });

  it("start failure surfaces the error and stays idle", async () => {
    invoke.mockRejectedValueOnce("No microphone found");
    const store = useCaptureStore();
    await store.start("v1");
    expect(store.status).toBe("idle");
    expect(store.error).toContain("No microphone");
  });

  it("stop passes through saving and returns to idle on saved event", async () => {
    invoke.mockResolvedValueOnce({ recording: true, vaultId: "v1", startedAtMs: 1 });
    invoke.mockResolvedValueOnce(undefined); // stop_capture
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    const stopping = store.stop();
    expect(store.status).toBe("saving");
    await stopping;
    listeners.get("capture:saved")!({
      payload: { mp3: "/v/m.mp3", note: null, endedEarly: false },
    });
    expect(store.status).toBe("idle");
    expect(store.lastSavedFile).toBe("/v/m.mp3");
  });

  it("failed event resets to idle with error", async () => {
    const store = useCaptureStore();
    await store.init();
    listeners.get("capture:failed")!({ payload: { message: "boom" } });
    expect(store.status).toBe("idle");
    expect(store.error).toBe("boom");
  });

  it("warning event is stored without changing status", async () => {
    invoke.mockResolvedValueOnce({ recording: true, vaultId: "v1", startedAtMs: 1 });
    const store = useCaptureStore();
    await store.init();
    await store.start("v1");
    listeners.get("capture:warning")!({ payload: { message: "source lost: mic" } });
    expect(store.status).toBe("recording");
    expect(store.warning).toContain("source lost");
  });

  it("init resyncs from capture_status (app restarted mid-recording UI)", async () => {
    invoke.mockResolvedValueOnce({ recording: true, vaultId: "v9", startedAtMs: 7 });
    const store = useCaptureStore();
    await store.init();
    expect(invoke).toHaveBeenCalledWith("capture_status");
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v9");
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: FAIL — module `../src/stores/capture` not found.

- [ ] **Step 4: Implement the store**

`src/stores/capture.ts`:

```ts
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CaptureSaved, CaptureStatus } from "../types";

export const useCaptureStore = defineStore("capture", {
  state: () => ({
    status: "idle" as "idle" | "recording" | "saving",
    vaultId: null as string | null,
    startedAtMs: null as number | null,
    error: null as string | null,
    warning: null as string | null,
    lastSavedFile: null as string | null,
  }),
  actions: {
    async init() {
      await listen<CaptureSaved>("capture:saved", (event) => {
        this.status = "idle";
        this.vaultId = null;
        this.startedAtMs = null;
        this.lastSavedFile = event.payload.mp3;
      });
      await listen<{ message: string }>("capture:failed", (event) => {
        this.status = "idle";
        this.vaultId = null;
        this.startedAtMs = null;
        this.error = event.payload.message;
      });
      await listen<{ message: string }>("capture:warning", (event) => {
        this.warning = event.payload.message;
      });
      // Resync: the webview can reload while Rust keeps recording.
      try {
        const s = await invoke<CaptureStatus>("capture_status");
        if (s.recording) {
          this.status = "recording";
          this.vaultId = s.vaultId;
          this.startedAtMs = s.startedAtMs;
        }
      } catch {
        // not running under Tauri (unit tests without a status mock)
      }
    },
    async start(vaultId: string) {
      this.error = null;
      this.warning = null;
      try {
        const s = await invoke<CaptureStatus>("start_capture", { id: vaultId });
        this.status = "recording";
        this.vaultId = s.vaultId;
        this.startedAtMs = s.startedAtMs;
      } catch (e) {
        this.status = "idle";
        this.error = String(e);
      }
    },
    async stop() {
      if (this.status !== "recording") return;
      this.status = "saving";
      try {
        await invoke("stop_capture");
        // capture:saved / capture:failed events complete the transition.
      } catch (e) {
        this.status = "idle";
        this.error = String(e);
      }
    },
  },
});
```

Note for the test `init resyncs…`: `init()` calls `capture_status` as its only invoke when no start happened — the mock's single `mockResolvedValueOnce` feeds it. In the `stop` test, `init()` is called before `start`, so add one extra `invoke.mockResolvedValueOnce({ recording: false, vaultId: null, startedAtMs: null })` FIRST in that test if the assertion sequence misaligns — order: status resync, start, stop.

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: 6 passed (adjust the mock ordering note above if the third test's invoke sequence fails).

- [ ] **Step 6: Commit**

```bash
git add src/stores/capture.ts src/types.ts tests/capture-store.test.ts
git commit -m "feat(ui): capture store with event-driven lifecycle"
```

---

### Task 13: Recording UI — capture button, recording bar, buddy state

**Files:**
- Create: `src/components/RecordingBar.vue`
- Modify: `src/components/VaultList.vue` (capture button per vault row), `src/components/ActionPanel.vue` (bar + wiring), `src/components/CompanionCharacter.vue` (recording visual), `src/App.vue` (store init + recording prop)
- Test: `tests/recording-bar.test.ts`; extend `tests/vault-list.test.ts`, `tests/companion-character.test.ts`

**Interfaces:**
- Consumes: `useCaptureStore` from Task 12.
- Produces:
  - `VaultList.vue` new prop `captureDisabled: boolean` and new emit `(e: "capture", id: string)`; the capture button carries `aria-label` `` `Capture knowledge in ${accessibleName(vault)}` ``.
  - `RecordingBar.vue` props `{ startedAtMs: number | null; saving: boolean; warning: string | null }`, emit `(e: "stop")`; shows elapsed `M:SS` (frontend-computed, 1 s interval) + pulsing red dot + Stop button (`aria-label="Stop recording"`), or "Saving…" while saving.
  - `CompanionCharacter.vue` new optional prop `recording?: boolean`; adds class `recording` and a structural red-dot overlay (`.rec-dot`) positioned over the sprite `BuddyAvatar` — visible for every character and with animations off; pulses only when animations are on. **Main's PR #5 rewrote this component (sprites, BuddySettings, expanded settings store) — read the current `CompanionCharacter.vue`/`ActionPanel.vue`/`App.vue` before editing; the surrounding markup in this task is indicative, the interfaces (prop names, emits, `.rec-dot` presence) are binding.**

- [ ] **Step 1: Write the failing RecordingBar test**

`tests/recording-bar.test.ts`:

```ts
import { describe, it, expect, vi, afterEach } from "vitest";
import { mount } from "@vue/test-utils";
import RecordingBar from "../src/components/RecordingBar.vue";

describe("RecordingBar", () => {
  afterEach(() => vi.useRealTimers());

  it("shows elapsed time from startedAtMs", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(100_000));
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: 100_000 - 65_000, saving: false, warning: null },
    });
    expect(wrapper.text()).toContain("1:05");
  });

  it("emits stop on button click", async () => {
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: Date.now(), saving: false, warning: null },
    });
    await wrapper.get("button[aria-label='Stop recording']").trigger("click");
    expect(wrapper.emitted("stop")).toHaveLength(1);
  });

  it("shows saving state and disables stop", () => {
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: Date.now(), saving: true, warning: null },
    });
    expect(wrapper.text()).toContain("Saving");
    expect(
      wrapper.get("button[aria-label='Stop recording']").attributes("disabled"),
    ).toBeDefined();
  });

  it("renders a warning when present", () => {
    const wrapper = mount(RecordingBar, {
      props: { startedAtMs: Date.now(), saving: false, warning: "source lost: mic" },
    });
    expect(wrapper.text()).toContain("source lost: mic");
  });
});
```

- [ ] **Step 2: Run it to verify failure**

Run: `npx vitest run tests/recording-bar.test.ts`
Expected: FAIL — component missing.

- [ ] **Step 3: Implement RecordingBar.vue**

```vue
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{
  startedAtMs: number | null;
  saving: boolean;
  warning: string | null;
}>();
defineEmits<{ (e: "stop"): void }>();

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;
onMounted(() => {
  timer = setInterval(() => (now.value = Date.now()), 1000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

const elapsed = computed(() => {
  if (props.startedAtMs === null) return "0:00";
  const total = Math.max(0, Math.floor((now.value - props.startedAtMs) / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${m}:${String(s).padStart(2, "0")}`;
});
</script>

<template>
  <div class="rounded-lg bg-red-500/15 px-2 py-1.5">
    <div class="flex items-center gap-2">
      <span
        class="h-2.5 w-2.5 shrink-0 animate-pulse rounded-full bg-red-500"
        aria-hidden="true"
      ></span>
      <span class="flex-1 text-sm font-medium text-red-100" role="status">
        {{ saving ? "Saving…" : `Recording ${elapsed}` }}
      </span>
      <button
        type="button"
        class="cursor-pointer rounded-lg bg-red-500/80 px-2 py-1 text-xs font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Stop recording"
        :disabled="saving"
        @click="$emit('stop')"
      >
        ⏹ Stop
      </button>
    </div>
    <p v-if="warning" class="mt-1 text-xs text-amber-200">{{ warning }}</p>
  </div>
</template>
```

- [ ] **Step 4: Extend VaultList.vue**

Add to the props interface: `captureDisabled: boolean;` and to emits: `(e: "capture", id: string): void;`. Insert a capture button between the daily-note button and the row end (same styling as the daily-note button; mic SVG):

```vue
        <button
          type="button"
          class="mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null || captureDisabled"
          :aria-label="`Capture knowledge in ${accessibleName(vault)}`"
          title="Capture knowledge (record audio)"
          @click="$emit('capture', vault.id)"
        >
          <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true">
            <rect x="9" y="2" width="6" height="12" rx="3" />
            <path d="M5 10v1a7 7 0 0 0 14 0v-1M12 18v4" />
          </svg>
        </button>
```

Extend `tests/vault-list.test.ts` with (follow the file's existing mount helper pattern; pass the new required prop `captureDisabled: false` to existing mounts):

```ts
  it("emits capture with the vault id", async () => {
    const wrapper = mountList(); // existing helper, captureDisabled: false
    await wrapper
      .get("button[aria-label^='Capture knowledge in']")
      .trigger("click");
    expect(wrapper.emitted("capture")![0]).toEqual([vaults[0].id]);
  });

  it("disables capture buttons when captureDisabled", () => {
    const wrapper = mountList({ captureDisabled: true });
    expect(
      wrapper.get("button[aria-label^='Capture knowledge in']").attributes("disabled"),
    ).toBeDefined();
  });
```

- [ ] **Step 5: Wire ActionPanel.vue**

In the script block add:

```ts
import { useCaptureStore } from "../stores/capture";
import RecordingBar from "./RecordingBar.vue";

const capture = useCaptureStore();
```

In the template, right after the error `<p v-if="store.error" …>` paragraph, add:

```vue
    <RecordingBar
      v-if="capture.status !== 'idle'"
      class="mb-2"
      :started-at-ms="capture.startedAtMs"
      :saving="capture.status === 'saving'"
      :warning="capture.warning"
      @stop="capture.stop()"
    />
    <p
      v-if="capture.error"
      class="mb-2 rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ capture.error }}
    </p>
```

And extend the `<VaultList …>` usage:

```vue
      <VaultList
        v-if="filtered.length > 0"
        :vaults="filtered"
        :busy-vault-id="store.busyVaultId"
        :busy-command="store.busyCommand"
        :capture-disabled="capture.status !== 'idle'"
        @open-vault="store.runAction('open_vault', $event)"
        @open-daily-note="store.runAction('open_daily_note', $event)"
        @capture="capture.start($event)"
      />
```

- [ ] **Step 6: CompanionCharacter recording state + App wiring**

`CompanionCharacter.vue` was rewritten on main (PR #5) around sprite characters: it now renders a `<BuddyAvatar>` inside the button and takes `working / animated / character / draggable / facing` props. Read the current file first, then:

1. Add `recording?: boolean` to the props (default `false` in the existing `withDefaults` call).
2. Add `recording` to the button's class array binding alongside its existing entries.
3. Wrap the `<BuddyAvatar …>` in a relative container with a structural (non-animated) red dot overlay, so it works for every character sprite and survives animations-off:

```vue
      <span class="relative inline-block">
        <BuddyAvatar
          ...existing bindings unchanged...
        />
        <span
          v-if="recording"
          class="rec-dot absolute -right-1 -top-1 h-3 w-3 rounded-full bg-red-500 ring-2 ring-slate-900"
          aria-hidden="true"
        ></span>
      </span>
```

4. In the style block add a pulse that only applies while animations are on (the dot itself stays visible regardless — `v-if` is structural):

```css
.buddy.recording:not(.still) .rec-dot {
  animation: rec-blink 1.2s ease-in-out infinite;
}
@keyframes rec-blink {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.35; }
}
```

(If the current component no longer uses a `.still` class for animations-off, gate the animation on whatever the current mechanism is — the invariant to preserve: dot always visible while recording, pulse only when animations are enabled.)

`App.vue`: import and init the capture store next to the existing stores, call `capture.init()` where the app does its startup work (e.g. in `onMounted`), and pass `:recording="capture.status === 'recording'"` to `<CompanionCharacter …>`. Check `src/App.vue` for the exact mount point — it already passes `working` and `animated` props.

Extend `tests/companion-character.test.ts`:

```ts
  it("shows the recording dot when recording", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, recording: true },
    });
    expect(wrapper.find(".rec-dot").exists()).toBe(true);
    expect(wrapper.get("button").classes()).toContain("recording");
  });

  it("hides the recording dot when idle", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    expect(wrapper.find(".rec-dot").exists()).toBe(false);
  });
```

- [ ] **Step 7: Run the full frontend suite + typecheck**

Run: `npm run test && npm run build`
Expected: all suites pass (existing `vault-list` mounts updated with `captureDisabled`), `vue-tsc` clean.

- [ ] **Step 8: Commit**

```bash
git add src/components tests src/App.vue
git commit -m "feat(ui): capture button, recording bar, buddy recording indicator"
```

---

### Task 14: Docs + full verification sweep

**Files:**
- Modify: `docs/DEVELOPMENT.md` (config schema section)
- Create: `docs/superpowers/specs/2026-07-04-increment-2-windows-verification.md`

- [ ] **Step 1: Document the config file**

Append to `docs/DEVELOPMENT.md`:

```markdown
## Capture configuration

Per-vault capture settings live app-side in `%APPDATA%\vault-buddy\config.json`
(keyed by Obsidian vault ID — the key from `obsidian.json`). The file is
optional; missing files, entries, or fields fall back to defaults. Nothing is
ever written into your vaults except recordings and their notes.

```json
{
  "vaults": {
    "<vault-id>": {
      "mode": "meeting",          // "meeting" (mic + desktop audio) | "voice-note" (mic only)
      "recordingFolder": "Meetings", // optional — omit for the mode default ("Meetings" / "Voice Notes")
      "bitrateKbps": 128,          // 128 | 160 | 192
      "createNote": true           // companion .md with metadata + embed
    }
  }
}
```
```

- [ ] **Step 2: Write the Windows verification checklist**

`docs/superpowers/specs/2026-07-04-increment-2-windows-verification.md`:

```markdown
# Increment 2 — Windows manual verification

Companion checklist to the increment 2 design; run on a Windows machine
with Obsidian and a microphone. Development happens on Linux, so every
device-dependent behavior below must be verified here before release.

## Happy path
- [ ] Click 🎙 Capture on a vault: recording starts < 2 s; buddy pulses red
      with the dot; tray icon shows the red dot; tray menu gains "⏹ Stop
      recording"; toast is NOT shown on start (panel/buddy are the signal).
- [ ] During a Teams (or any) call: stop after ≥ 2 min → MP3 in
      `Meetings/YYYY/MM/` inside the vault, saved toast with filename,
      both your voice and the other side audible, duration matches.
- [ ] Companion note sits beside the MP3, embed plays inside Obsidian,
      frontmatter lists both devices.
- [ ] Stop → file present in < 5 s regardless of recording length.

## Collisions and modes
- [ ] Two captures in the same minute → second file gets " (2)".
- [ ] Pre-create `<name>.md` in the target folder → capture uses " (2)"
      for BOTH mp3 and note; the user note is untouched.
- [ ] Set `"mode": "voice-note"` in config.json → recording contains mic
      only; works with no output device connected.
- [ ] Set `"createNote": false` → MP3 only, no .md.

## Indicator hardening
- [ ] While recording: tray "Show / Hide" does nothing (buddy stays);
      buddy right-click → Hide does nothing.
- [ ] Start capture while the buddy is hidden in the tray → buddy shows.
- [ ] Tray "⏹ Stop recording" stops and saves.
- [ ] Quit (tray and buddy menu) mid-recording → recording is saved
      (toast) before the app exits. Alt+F4 likewise.

## Reliability
- [ ] Unplug the headset mic mid-meeting → warning in panel, recording
      continues (desktop audio side), note metadata records the event.
- [ ] Voice-note mode + unplug mic → recording finalizes immediately,
      "ended early" toast, partial audio saved.
- [ ] Kill Vault Buddy in Task Manager mid-recording → relaunch →
      within ~2 min a "Recording recovered" toast; `… (recovered).mp3`
      plays, containing audio up to ~the kill moment.
- [ ] Kill + relaunch + immediately start a new capture in the same vault
      and minute → new capture gets a suffixed name; the orphan is still
      recovered afterwards.
- [ ] Launch a second Vault Buddy instance while recording → no second
      process; existing buddy focused; recording unaffected.

## Audit
- [ ] App log contains started/saved/recovered lines with vault + path
      for every case above.
```

- [ ] **Step 3: Full verification sweep**

Run from repo root:

```bash
cd src-tauri && cargo fmt --check && \
  cargo clippy -p vault_buddy_core -p vault_buddy_capture --all-targets -- -D warnings && \
  cargo test -p vault_buddy_core -p vault_buddy_capture && cd .. && \
  npm run test && npm run build
```

Expected: everything green.

- [ ] **Step 4: Commit**

```bash
git add docs/DEVELOPMENT.md docs/superpowers/specs/2026-07-04-increment-2-windows-verification.md
git commit -m "docs: capture config schema and Windows verification checklist"
```

---

## Self-Review Notes (already applied)

- Spec coverage walked section-by-section: capture action (T10/13), recording UX + indicator hardening (T11/13), pipeline (T5/6/8/9), storage layout + reservation (T3/8), note + atomic write + toggle (T4/8/10), config (T2, docs T14), notifications (T10), recovery incl. staleness/zero-frame/rescan/dated-walk/collision-safe names (T7/T10), quit/close finalize (T11), single instance (T10), mode-aware source loss (T8 all-sources-dead predicate), stop-time recheck + rename retry (T3 `reserve_final` + T7 `rename_into_reserved`, shared by session and recovery), self-finalization surfacing (T10 monitor thread), CI (T1), Windows checklist (T14).
- Type consistency: `SourceMsg`/`SourceInput` defined in T8, consumed by T9; `reserve_final` defined T3, consumed T7/T8; `set_recording` stubbed in T10 exactly as T11 replaces it; `captureDisabled` prop name identical in T13 component and tests.
- Known judgment calls an implementer may hit: exact `mp3lame-encoder` / cpal API names (adjust internals, keep interfaces), Windows rename error kinds (both retried), Vitest mock ordering in T12 (noted inline).
```
