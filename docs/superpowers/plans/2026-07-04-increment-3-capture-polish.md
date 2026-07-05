# Increment 3 — "Capture, polished" Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make capture configurable and comfortable — per-vault settings UI with device pickers, pause/resume, a live level meter, post-save rename to human titles, and a per-vault recording indicator.

**Architecture:** Pure logic lands in the two portable crates first (`src-tauri/core`: config write path, rename planning, embed retargeting; `src-tauri/capture`: session Control channel, level tap, device enumeration, rename execution), then the Tauri shell exposes six new IPC commands and three new events, then the Vue frontend grows a `captureSettings` panel view, pause/meter/rename UI, and store state. Spec: `docs/superpowers/specs/2026-07-04-increment-3-capture-polish-design.md` (approved — implement as written).

**Tech Stack:** Rust (chrono, serde_json, cpal, mp3lame-encoder), Tauri v2, Vue 3 + Pinia + Tailwind 4, Vitest + @vue/test-utils + `mockIPC`.

## Global Constraints

- **Never write into vaults** outside the capture domain's sanctioned paths (recordings + companion notes under the recording root). `config.json` is app-side (`%APPDATA%\vault-buddy\config.json`) — replacement rename is correct THERE and only there.
- Bitrate options are exactly `128 | 160 | 192` kbps.
- Device pickers: "System default" is always the first option and the default; absent config field = system default; a configured-but-missing device falls back to the default with a warning — **stale config never blocks recording**.
- Rename keeps the `YYYY-MM-DD HHmm ` prefix (16 chars) so `is_capture_base`, sorting, recovery ownership, and collision rules keep holding. Title: sanitized, rejected when empty-after-sanitizing or > 120 chars.
- Level meter: post-mix per-tick peak, normalized 0–1, ~5 Hz (every other 100 ms tick), advisory only — a lost event only stalls the meter.
- Pause: streams stay open, drained samples are discarded, nothing is encoded, the 30 s fsync cadence keeps running; pause never blocks shutdown; stop/quit while paused finalizes normally.
- All Rust work runs from `src-tauri/`: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (all three compile on this Linux box — the shell crate builds here; only *runtime* behavior is Windows-only). Frontend: `npm run test`, `npm run build` (Node 22, run `npm ci` once first).
- Commits: Conventional Commits, imperative subject, body explains the why. Set `git config user.email noreply@anthropic.com && git config user.name Claude` once before the first commit.
- cpal::Stream is `!Send` — device streams live on the dedicated thread in `capture_commands.rs`; the monitor thread stays the sole consumer of the session outcome. Do NOT restructure that threading; the shell's stop channel generalizes to the capture crate's `Control` enum instead.
- Baseline at branch start: 86 Rust tests, 136 Vitest tests, all green.

## File map (who owns what)

| File | Task(s) | Responsibility |
| --- | --- | --- |
| `src-tauri/core/src/capture_config.rs` | 1 | +`input_device`/`output_device`, mode keys, `serialize_config`/`write_config`/`update_vault_config` |
| `src-tauri/core/src/capture_paths.rs` | 2 | +`RenamePlan`, `rename_plan`, title sanitization |
| `src-tauri/core/src/capture_note.rs` | 3, 4 | +`retarget_embed` (3); `NoteMeta.paused` (4) |
| `src-tauri/capture/src/session.rs` | 4, 5, 6 | `Control` channel + pause (4); level tap (5); `start_warning` (6) |
| `src-tauri/capture/src/devices.rs` | 6 | `list_devices`, preferred-device resolution + warnings |
| `src-tauri/capture/src/rename.rs` (new) | 7 | Collision-safe rename execution (mp3 + note + embed) |
| `src-tauri/src/capture_commands.rs` | 5, 6, 8, 9, 10 | New IPC commands, Control wiring, level forwarding |
| `src-tauri/src/tray.rs` | 9 | `TrayCaptureState` (amber paused icon, Pause ⇄ Resume item) |
| `src-tauri/src/lib.rs` | 8, 9, 10 | Command registration, `ConfigWriteLock` |
| `src/types.ts` | 11, 13 | Status/renamed/config/device DTO types |
| `src/stores/capture.ts` | 11 | paused/level/vaultId/lastSaved state + pause/resume/rename actions |
| `src/stores/vaults.ts`, `src/stores/updates.ts` | 12 | Panel view union `list \| settings \| captureSettings` |
| `src/components/ActionPanel.vue` | 12–16 | View switching, wiring of all new UI |
| `src/components/CaptureSettings.vue` (new) | 13 | The ⚙ form |
| `src/components/VaultList.vue` | 14 | ⚙ emit + recording-row dot |
| `src/components/RecordingBar.vue` | 15 | Pause/Resume, meter, frozen elapsed |
| `src/components/CompanionCharacter.vue`, `src/App.vue` | 15 | Amber buddy dot while paused |
| `src/components/RenamePrompt.vue` (new) | 16 | "Name this recording" prompt |
| `AGENTS.md`, `docs/DEVELOPMENT.md` | 17 | IPC surface + config schema docs |

Execution order is task order: core (1–3) → capture (4–7) → shell (8–10) → frontend (11–16) → docs (17). Tasks 5 and 6 include one-line shell call-site updates so `cargo test --workspace` stays green on every commit.

---

### Task 1: Config schema + atomic write path (core)

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs`

**Interfaces:**
- Consumes: nothing new.
- Produces (used by Tasks 6, 8):
  - `VaultCaptureConfig` gains `pub input_device: Option<String>`, `pub output_device: Option<String>` (both `None` in `Default`).
  - `RecordingMode::as_key(&self) -> &'static str` ("meeting" / "voice-note") and `RecordingMode::from_key(key: &str) -> Option<RecordingMode>`.
  - `serialize_config(cfg: &AppConfig) -> String` (pretty JSON, trailing newline, sorted vault ids, omits `None` fields).
  - `write_config(path: &Path, cfg: &AppConfig) -> std::io::Result<()>` (owned temp + fsync + replacing rename — our own file).
  - `update_vault_config_at(path: &Path, vault_id: &str, v: VaultCaptureConfig) -> std::io::Result<()>` (read-modify-write) and `update_vault_config(vault_id: &str, v: VaultCaptureConfig) -> Result<(), String>` (at `config_path()`).

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/core/src/capture_config.rs`:

```rust
    #[test]
    fn device_fields_parse_and_default_to_none() {
        let cfg = parse_config(
            r#"{ "vaults": { "a": {
                "inputDevice": "Headset Mic",
                "outputDevice": "Speakers"
            } } }"#,
        );
        let a = vault_config(&cfg, "a");
        assert_eq!(a.input_device.as_deref(), Some("Headset Mic"));
        assert_eq!(a.output_device.as_deref(), Some("Speakers"));
        let b = vault_config(&cfg, "missing");
        assert_eq!(b.input_device, None);
        assert_eq!(b.output_device, None);
    }

    #[test]
    fn mode_keys_round_trip() {
        for mode in [RecordingMode::Meeting, RecordingMode::VoiceNote] {
            assert_eq!(RecordingMode::from_key(mode.as_key()), Some(mode));
        }
        assert_eq!(RecordingMode::from_key("karaoke"), None);
    }

    #[test]
    fn config_round_trips_through_serialize_and_parse() {
        let mut cfg = AppConfig::default();
        cfg.vaults.insert(
            "abc".to_string(),
            VaultCaptureConfig {
                mode: RecordingMode::VoiceNote,
                recording_folder: Some("Inbox/Audio".to_string()),
                bitrate_kbps: 192,
                create_note: false,
                input_device: Some("USB Mic".to_string()),
                output_device: Some("Speakers (Realtek)".to_string()),
            },
        );
        cfg.vaults
            .insert("def".to_string(), VaultCaptureConfig::default());
        let json = serialize_config(&cfg);
        let parsed = parse_config(&json);
        assert_eq!(parsed.vaults, cfg.vaults);
    }

    #[test]
    fn serialize_omits_absent_optional_fields() {
        let mut cfg = AppConfig::default();
        cfg.vaults
            .insert("a".to_string(), VaultCaptureConfig::default());
        let json = serialize_config(&cfg);
        assert!(!json.contains("recordingFolder"), "omitted when None: {json}");
        assert!(!json.contains("inputDevice"), "omitted when None: {json}");
        assert!(!json.contains("outputDevice"), "omitted when None: {json}");
        assert!(json.ends_with('\n'), "hand-editable file ends in newline");
    }

    #[test]
    fn update_vault_config_preserves_sibling_vaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            r#"{ "vaults": { "keep": { "mode": "voice-note", "bitrateKbps": 160 } } }"#,
        )
        .unwrap();
        let updated = VaultCaptureConfig {
            input_device: Some("Mic 2".to_string()),
            ..VaultCaptureConfig::default()
        };
        update_vault_config_at(&path, "edited", updated.clone()).unwrap();
        let cfg = parse_config(&std::fs::read_to_string(&path).unwrap());
        assert_eq!(vault_config(&cfg, "edited"), updated);
        let kept = vault_config(&cfg, "keep");
        assert_eq!(kept.mode, RecordingMode::VoiceNote);
        assert_eq!(kept.bitrate_kbps, 160);
    }

    #[test]
    fn write_config_replaces_and_leaves_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("config.json");
        write_config(&path, &AppConfig::default()).unwrap();
        // second write replaces the first — our own file, replacement is intent
        let mut cfg = AppConfig::default();
        cfg.vaults
            .insert("a".to_string(), VaultCaptureConfig::default());
        write_config(&path, &cfg).unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"a\""));
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp not cleaned: {leftovers:?}");
    }

    #[test]
    fn update_on_malformed_file_starts_fresh_but_writes_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "not json at all").unwrap();
        update_vault_config_at(&path, "a", VaultCaptureConfig::default()).unwrap();
        let cfg = parse_config(&std::fs::read_to_string(&path).unwrap());
        assert!(cfg.vaults.contains_key("a"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_config`
Expected: COMPILE ERROR (`input_device` unknown field, `serialize_config` not found).

- [ ] **Step 3: Implement**

In `src-tauri/core/src/capture_config.rs`:

3a. Change the module doc comment's stale line (the settings UI is arriving now):

```rust
//! Per-vault capture settings. App-side (%APPDATA%\vault-buddy\config.json),
//! keyed by Obsidian vault ID — never written into user vaults. Read by the
//! recording path and written by the settings UI (set_capture_config), and
//! still hand-editable — parsing must shrug off any malformed input and
//! fall back to defaults.
```

3b. Add to `impl RecordingMode` (below `uses_loopback`):

```rust
    /// Stable key used in config.json and the IPC DTOs.
    pub fn as_key(&self) -> &'static str {
        match self {
            RecordingMode::Meeting => "meeting",
            RecordingMode::VoiceNote => "voice-note",
        }
    }

    pub fn from_key(key: &str) -> Option<RecordingMode> {
        match key {
            "meeting" => Some(RecordingMode::Meeting),
            "voice-note" => Some(RecordingMode::VoiceNote),
            _ => None,
        }
    }
```

3c. Extend the struct + `Default`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct VaultCaptureConfig {
    pub mode: RecordingMode,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    /// cpal device names; None = system default. A configured device
    /// missing at record time falls back to the default with a warning —
    /// stale config never blocks recording.
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

impl Default for VaultCaptureConfig {
    fn default() -> Self {
        Self {
            mode: RecordingMode::Meeting,
            recording_folder: None,
            bitrate_kbps: 128,
            create_note: true,
            input_device: None,
            output_device: None,
        }
    }
}
```

3d. In `vault_entry`, replace the `mode:` match with `from_key` and add the two device fields:

```rust
        mode: entry
            .get("mode")
            .and_then(|v| v.as_str())
            .and_then(RecordingMode::from_key)
            .unwrap_or(defaults.mode),
```

and after `create_note`:

```rust
        input_device: entry
            .get("inputDevice")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        output_device: entry
            .get("outputDevice")
            .and_then(|v| v.as_str())
            .map(str::to_string),
```

3e. Add the write path (below `load_config`), plus `use std::io::Write;` and `use std::path::Path;` at the top (keep the existing `PathBuf` import):

```rust
/// Serialize to the same schema `parse_config` reads. Vault ids are
/// sorted and optional fields omitted so the hand-editable file stays
/// stable and minimal across saves.
pub fn serialize_config(cfg: &AppConfig) -> String {
    use serde_json::{json, Map, Value};
    let mut vaults = Map::new();
    let mut ids: Vec<&String> = cfg.vaults.keys().collect();
    ids.sort();
    for id in ids {
        let v = &cfg.vaults[id];
        let mut entry = Map::new();
        entry.insert("mode".to_string(), json!(v.mode.as_key()));
        if let Some(folder) = &v.recording_folder {
            entry.insert("recordingFolder".to_string(), json!(folder));
        }
        entry.insert("bitrateKbps".to_string(), json!(v.bitrate_kbps));
        entry.insert("createNote".to_string(), json!(v.create_note));
        if let Some(device) = &v.input_device {
            entry.insert("inputDevice".to_string(), json!(device));
        }
        if let Some(device) = &v.output_device {
            entry.insert("outputDevice".to_string(), json!(device));
        }
        vaults.insert(id.clone(), Value::Object(entry));
    }
    let root = json!({ "vaults": Value::Object(vaults) });
    let mut out = serde_json::to_string_pretty(&root).unwrap_or_else(|_| "{}".to_string());
    out.push('\n');
    out
}

/// Atomic write via owned temp + rename. The REPLACING std::fs::rename is
/// correct here — config.json is our own app-side file, never a vault
/// file, and replacing the previous version is exactly the intent (the
/// capture domain's rename_noreplace rule protects vault files, not this).
pub fn write_config(path: &Path, cfg: &AppConfig) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir)?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "config.json".to_string());
    let tmp = dir.join(format!(".{file_name}.vault-buddy.tmp"));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(serialize_config(cfg).as_bytes())?;
        f.sync_all()?;
    }
    let result = std::fs::rename(&tmp, path);
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Read-modify-write so saving one vault preserves the others. No lock of
/// its own: callers that can race (the IPC command layer) must serialize
/// calls behind a mutex.
pub fn update_vault_config_at(
    path: &Path,
    vault_id: &str,
    v: VaultCaptureConfig,
) -> std::io::Result<()> {
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    };
    cfg.vaults.insert(vault_id.to_string(), v);
    write_config(path, &cfg)
}

pub fn update_vault_config(vault_id: &str, v: VaultCaptureConfig) -> Result<(), String> {
    let path = config_path().ok_or("Cannot resolve the config directory")?;
    update_vault_config_at(&path, vault_id, v)
        .map_err(|e| format!("Could not save capture settings: {e}"))
}
```

Note: `core/Cargo.toml` needs no new deps (`serde_json`, `dirs`, `tempfile` already present).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_config`
Expected: all capture_config tests PASS (7 new + 8 existing).

- [ ] **Step 5: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green. (If `fmt --check` fails, run `cargo fmt` and re-check.)

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): capture config write path with device fields

Adds optional inputDevice/outputDevice to the per-vault schema and an
atomic serialize/write/update path (owned temp + replacing rename — our
own app-side file, not a vault). update_vault_config is read-modify-write
so saving one vault preserves the others; the command layer will
serialize concurrent saves behind a mutex."
```

---

### Task 2: Rename planning + title sanitization (core)

**Files:**
- Modify: `src-tauri/core/src/capture_paths.rs`

**Interfaces:**
- Consumes: `is_capture_base` (existing, same file).
- Produces (used by Tasks 7, 10):
  - `pub struct RenamePlan { pub dir: PathBuf, pub new_base: String, pub mp3_from: PathBuf, pub note_from: PathBuf }`
  - `pub fn rename_plan(mp3: &Path, new_title: &str) -> Result<RenamePlan, String>`
  - `pub const MAX_TITLE_CHARS: usize = 120;`

Behavior contract:
- Rejects (Err) when: `mp3` lacks the `.mp3` extension or its stem fails `is_capture_base` (ownership filter — never rename an arbitrary user mp3); title empty after sanitizing; title > 120 chars after sanitizing.
- Sanitizing: remove `/ \ < > : " | ? *` and control characters, trim whitespace, trim trailing dots/spaces (Windows rejects them at the end of file names).
- If the sanitized title itself starts with a capture prefix (`is_capture_base(&title)`), the leading 16 chars are stripped — the rename prompt prefills the FULL current base ("2026-07-04 1405 Meeting"), so confirm-without-edit and full-base edits must not double the prefix.
- `new_base` = first 16 chars of the existing stem (`YYYY-MM-DD HHmm `) + title. `note_from` = sibling `.md` with the same stem.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/core/src/capture_paths.rs`:

```rust
    #[test]
    fn rename_plan_keeps_the_capture_prefix_and_sibling_note() {
        let mp3 = Path::new("/v/Meetings/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "Standup with Alice").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Standup with Alice");
        assert!(is_capture_base(&plan.new_base), "retitled base is still ours");
        assert_eq!(plan.dir, Path::new("/v/Meetings/2026/07"));
        assert_eq!(plan.mp3_from, mp3);
        assert_eq!(
            plan.note_from,
            Path::new("/v/Meetings/2026/07/2026-07-04 1405 Meeting.md")
        );
    }

    #[test]
    fn rename_plan_strips_separators_and_control_characters() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, " a/b\\c:d*e?f\"g<h>i|j\u{7}k ").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 abcdefghijk");
    }

    #[test]
    fn rename_plan_keeps_unicode_and_interior_dots() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "Café v1.2 ☕").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Café v1.2 ☕");
    }

    #[test]
    fn rename_plan_trims_trailing_dots_and_spaces() {
        // Windows rejects file names ending in dots or spaces.
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "Notes.. . ").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Notes");
    }

    #[test]
    fn rename_plan_rejects_empty_after_sanitizing_and_overlong() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        assert!(rename_plan(mp3, "").is_err());
        assert!(rename_plan(mp3, "  /\\:  ").is_err());
        let long = "x".repeat(MAX_TITLE_CHARS + 1);
        assert!(rename_plan(mp3, &long).is_err());
        let just_fits = "x".repeat(MAX_TITLE_CHARS);
        assert!(rename_plan(mp3, &just_fits).is_ok());
    }

    #[test]
    fn rename_plan_strips_a_leading_capture_prefix_from_the_title() {
        // The prompt prefills the FULL current base; confirming unedited
        // (or editing the tail of it) must not double the prefix.
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(mp3, "2026-07-04 1405 Meeting").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Meeting");
        let plan = rename_plan(mp3, "2026-07-04 1405 Meeting with Alice").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Meeting with Alice");
    }

    #[test]
    fn rename_plan_refuses_foreign_files() {
        // Ownership filter: only our capture pattern may be renamed.
        assert!(rename_plan(Path::new("/v/2026/07/holiday.mp3"), "t").is_err());
        assert!(rename_plan(Path::new("/v/2026/07/2026-07-04 1405 Meeting.wav"), "t").is_err());
        assert!(rename_plan(Path::new("/v/2026/07/2026-07-04 Meeting.mp3"), "t").is_err());
    }

    #[test]
    fn rename_plan_on_a_suffixed_base_replaces_label_and_suffix() {
        let mp3 = Path::new("/v/2026/07/2026-07-04 1405 Meeting (2).mp3");
        let plan = rename_plan(mp3, "Standup").unwrap();
        assert_eq!(plan.new_base, "2026-07-04 1405 Standup");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_paths`
Expected: COMPILE ERROR (`rename_plan` not found).

- [ ] **Step 3: Implement**

Add to `src-tauri/core/src/capture_paths.rs` (below `recovered_base`):

```rust
/// Longest accepted rename title, in characters.
pub const MAX_TITLE_CHARS: usize = 120;

/// Chars of the `YYYY-MM-DD HHmm ` prefix every capture base starts with.
const CAPTURE_PREFIX_CHARS: usize = 16;

/// Strip everything that can never reach a file name: path separators and
/// the rest of the Windows-reserved set, plus control characters. Then
/// trim whitespace and the trailing dots/spaces Windows rejects.
fn sanitize_title(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') && !c.is_control()
        })
        .collect();
    cleaned.trim().trim_end_matches(['.', ' ']).to_string()
}

pub struct RenamePlan {
    pub dir: PathBuf,
    pub new_base: String,
    pub mp3_from: PathBuf,
    pub note_from: PathBuf,
}

/// Pure planning for the post-save rename: validates ownership and the
/// title, derives the new base. Execution (reservation + rename_noreplace
/// + embed retarget) lives in the capture crate so the safety rails are
/// shared with the save path.
pub fn rename_plan(mp3: &Path, new_title: &str) -> Result<RenamePlan, String> {
    let stem = mp3
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let is_mp3 = mp3
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mp3"));
    if !is_mp3 || !is_capture_base(&stem) {
        // Ownership filter: rename only files carrying our capture
        // pattern — never an arbitrary user mp3 handed in by mistake.
        return Err("Not a Vault Buddy capture file".to_string());
    }
    let mut title = sanitize_title(new_title);
    // The prompt prefills the full current base; a title that itself
    // starts with a capture prefix must not end up double-prefixed.
    if is_capture_base(&title) {
        title = title.chars().skip(CAPTURE_PREFIX_CHARS).collect();
    }
    if title.is_empty() {
        return Err("Title is empty after removing unusable characters".to_string());
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(format!("Title is too long (max {MAX_TITLE_CHARS} characters)"));
    }
    let prefix: String = stem.chars().take(CAPTURE_PREFIX_CHARS).collect();
    let dir = mp3.parent().unwrap_or_else(|| Path::new(".")).to_path_buf();
    let note_from = dir.join(format!("{stem}.md"));
    Ok(RenamePlan {
        dir,
        new_base: format!("{prefix}{title}"),
        mp3_from: mp3.to_path_buf(),
        note_from,
    })
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_paths`
Expected: PASS (8 new + existing).

- [ ] **Step 5: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/core/src/capture_paths.rs
git commit -m "feat(core): rename planning with title sanitization

rename_plan validates ownership (capture pattern only), sanitizes the
title (Windows-reserved chars, control chars, trailing dots/spaces;
empty/overlong rejected), strips a doubled capture prefix from prefilled
titles, and keeps the YYYY-MM-DD HHmm prefix so sorting, recovery
ownership and collision rules keep holding."
```

---

### Task 3: Embed retargeting (core)

**Files:**
- Modify: `src-tauri/core/src/capture_note.rs`

**Interfaces:**
- Produces (used by Task 7): `pub fn retarget_embed(note: &str, old_mp3: &str, new_mp3: &str) -> String` — rewrites exactly the `![[old_mp3]]` embed line(s); everything else (including prose mentions of the old name) is untouched; line endings preserved.

- [ ] **Step 1: Write the failing tests**

Append inside `mod tests` in `src-tauri/core/src/capture_note.rs`:

```rust
    #[test]
    fn retarget_rewrites_only_the_embed_line() {
        let note = "---\nvault: \"W\"\n---\n\nSee old.mp3 in prose.\n![[old.mp3]]\n";
        let out = retarget_embed(note, "old.mp3", "new.mp3");
        assert!(out.contains("![[new.mp3]]"));
        assert!(!out.contains("![[old.mp3]]"));
        assert!(
            out.contains("See old.mp3 in prose."),
            "prose mention untouched: {out}"
        );
    }

    #[test]
    fn retarget_preserves_crlf_line_endings() {
        let note = "a\r\n![[old.mp3]]\r\nb\r\n";
        let out = retarget_embed(note, "old.mp3", "new.mp3");
        assert_eq!(out, "a\r\n![[new.mp3]]\r\nb\r\n");
    }

    #[test]
    fn retarget_without_a_match_returns_the_note_unchanged() {
        let note = "no embed here\n![[other.mp3]]\n";
        assert_eq!(retarget_embed(note, "old.mp3", "new.mp3"), note);
    }

    #[test]
    fn retarget_handles_a_note_without_trailing_newline() {
        let out = retarget_embed("![[old.mp3]]", "old.mp3", "new.mp3");
        assert_eq!(out, "![[new.mp3]]");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_note`
Expected: COMPILE ERROR (`retarget_embed` not found).

- [ ] **Step 3: Implement**

Add to `src-tauri/core/src/capture_note.rs` (below `render_note`):

```rust
/// Rewrite exactly the `![[old]]` embed line(s) to point at the new file
/// name. Line-anchored on purpose: the user may have written prose
/// mentioning the old name, and only the embed our own render_note wrote
/// may change.
pub fn retarget_embed(note: &str, old_mp3: &str, new_mp3: &str) -> String {
    let old_line = format!("![[{old_mp3}]]");
    let new_line = format!("![[{new_mp3}]]");
    let mut out = String::with_capacity(note.len());
    for line in note.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        if body == old_line {
            out.push_str(&new_line);
            out.push_str(&line[body.len()..]);
        } else {
            out.push_str(line);
        }
    }
    out
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_note`
Expected: PASS (4 new + existing).

- [ ] **Step 5: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/core/src/capture_note.rs
git commit -m "feat(core): retarget note embeds for post-save rename

retarget_embed rewrites exactly the ![[...]] embed line render_note
wrote — line-anchored so prose mentions of the old file name survive;
CRLF and missing trailing newlines are preserved byte-for-byte."
```

---

### Task 4: Pause/resume via a session Control channel (capture)

**Files:**
- Modify: `src-tauri/capture/src/session.rs`
- Modify: `src-tauri/core/src/capture_note.rs` (NoteMeta gains `paused`)
- Modify: `src-tauri/capture/src/recovery.rs` (one NoteMeta initializer)

**Interfaces:**
- Consumes: `NoteMeta`, `render_note`, `format_duration` (core).
- Produces (used by Task 9):
  - `pub enum Control { Stop, Pause, Resume }` in `session.rs`.
  - `CaptureSession::pause(&self)`, `CaptureSession::resume(&self)` (fire-and-forget sends), `stop(self)` unchanged in signature (now sends `Control::Stop`).
  - `Outcome` gains `pub paused_secs: u64`.
  - `NoteMeta` gains `pub paused: Option<String>` (rendered as a `paused:` frontmatter line when `Some`).

Behavior contract (spec): while paused the drain loops keep running (device loss still detected) but drained samples are discarded and nothing is encoded — the timeline skips the gap; the flush/fsync cadence keeps running; stop-while-paused closes the open pause span and finalizes normally; `duration_secs` (frames-based) never includes paused wall time; the note records total paused duration when > 0.

- [ ] **Step 1: Write the failing tests**

1a. Append inside `mod tests` in `src-tauri/capture/src/session.rs`:

```rust
    #[test]
    fn pause_excludes_the_gap_and_the_note_records_it() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        // ~0.4 s of real audio while recording
        for chunk in sine_chunks(44_100, 0.4) {
            tx.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(400));
        session.pause();
        // 2.2 s of wall time that must NOT appear in the duration; samples
        // arriving while paused are discarded (the gap is skipped, not
        // recorded as silence)
        std::thread::sleep(Duration::from_millis(200));
        for chunk in sine_chunks(44_100, 0.5) {
            tx.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(2_000));
        session.resume();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert!(
            outcome.paused_secs >= 2,
            "paused span accumulated: {}",
            outcome.paused_secs
        );
        // active wall time is ~0.7 s + scheduling slack; if the paused span
        // leaked into the timeline this would be >= 2
        assert!(
            outcome.duration_secs < 2,
            "paused wall time excluded: {}",
            outcome.duration_secs
        );
        assert!(outcome.mp3.exists());
        let note = std::fs::read_to_string(outcome.note.expect("note written")).unwrap();
        assert!(note.contains("paused: \"0:0"), "paused metadata: {note}");
    }

    #[test]
    fn stop_while_paused_finalizes_and_closes_the_open_span() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        session.pause();
        std::thread::sleep(Duration::from_millis(1_100));
        // pause never blocks shutdown: stop while paused saves normally
        let outcome = session.stop().unwrap();
        assert!(outcome.mp3.exists());
        assert!(
            outcome.paused_secs >= 1,
            "open pause span closed at stop: {}",
            outcome.paused_secs
        );
    }

    #[test]
    fn resume_without_pause_and_double_pause_are_harmless() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let session = CaptureSession::start(
            params(dir.path()),
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        session.resume(); // no-op
        session.pause();
        session.pause(); // second pause must not restart the span
        std::thread::sleep(Duration::from_millis(200));
        session.resume();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert!(outcome.mp3.exists());
    }
```

1b. In `src-tauri/core/src/capture_note.rs` tests: add `paused: None,` to the `meta()` helper's `NoteMeta { ... }` initializer (after `recording_type`), and append:

```rust
    #[test]
    fn note_records_paused_duration_when_present() {
        let mut m = meta();
        m.paused = Some(format_duration(65));
        let note = render_note(&m, "x.mp3");
        assert!(note.contains(r#"paused: "1:05""#), "{note}");
        let plain = render_note(&meta(), "x.mp3");
        assert!(!plain.contains("paused:"), "no paused line when None");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core capture_note && cargo test -p vault_buddy_capture session`
Expected: COMPILE ERROR (`NoteMeta` has no field `paused`; `CaptureSession::pause` not found).

- [ ] **Step 3: Implement — core NoteMeta**

In `src-tauri/core/src/capture_note.rs`:

Add to the struct (after `recording_type`):

```rust
pub struct NoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    pub recording_type: String,
    /// Total paused time (pre-formatted, e.g. "1:05"); None when the
    /// recording was never paused — the line is omitted entirely then.
    pub paused: Option<String>,
    pub input_devices: Vec<String>,
    pub event: Option<String>,
}
```

In `render_note`, after the `duration:` line and before the `vault:` line:

```rust
    if let Some(paused) = &meta.paused {
        out.push_str(&format!("paused: {}\n", yaml_quote(paused)));
    }
```

In `src-tauri/capture/src/recovery.rs`, in the `NoteMeta` initializer inside `recover_root` (the one with `recording_type: "Recording"`), add `paused: None,` after `recording_type`.

- [ ] **Step 4: Implement — session Control channel**

In `src-tauri/capture/src/session.rs`:

4a. Below `SourceMsg`, add:

```rust
/// Session control messages. One channel carries all three so the shell's
/// device thread (which owns the !Send cpal streams) stays the single
/// forwarding point and no second signalling path can race the stop.
pub enum Control {
    Stop,
    Pause,
    Resume,
}
```

4b. Replace the `CaptureSession` struct + impl's channel plumbing:

```rust
pub struct CaptureSession {
    control_tx: Sender<Control>,
    handle: JoinHandle<Result<Outcome, String>>,
}
```

In `start`: replace `let (stop_tx, stop_rx) = mpsc::channel();` with `let (control_tx, control_rx) = mpsc::channel();`, pass `control_rx` to `run_worker`, and return `CaptureSession { control_tx, handle }`.

Replace `stop` and add `pause`/`resume`:

```rust
    /// Fire-and-forget: a dead worker (already finalizing) makes these
    /// no-ops, which is exactly right — pause must never block or fail
    /// shutdown.
    pub fn pause(&self) {
        let _ = self.control_tx.send(Control::Pause);
    }

    pub fn resume(&self) {
        let _ = self.control_tx.send(Control::Resume);
    }

    pub fn stop(self) -> Result<Outcome, String> {
        let _ = self.control_tx.send(Control::Stop);
        self.handle
            .join()
            .map_err(|_| "capture worker panicked".to_string())?
    }
```

4c. `Outcome` gains the field:

```rust
pub struct Outcome {
    pub mp3: PathBuf,
    pub note: Option<PathBuf>,
    pub duration_secs: u64,
    /// Total time spent paused (excluded from duration_secs and from the
    /// encoded timeline).
    pub paused_secs: u64,
    pub warning: Option<String>,
    pub ended_early: bool,
}
```

4d. `run_worker` signature: last parameter becomes `control_rx: Receiver<Control>`.

4e. In `run_worker`, add state above the loop (next to `let mut next_tick = ...`):

```rust
    let mut paused = false;
    let mut paused_total = Duration::ZERO;
    let mut pause_started: Option<Instant> = None;
```

Replace the loop head's stop check:

```rust
        let wait = next_tick.saturating_duration_since(Instant::now());
        let mut stopped = false;
        match control_rx.recv_timeout(wait) {
            Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => stopped = true,
            Ok(Control::Pause) => {
                if !paused {
                    paused = true;
                    pause_started = Some(Instant::now());
                }
            }
            Ok(Control::Resume) => {
                if paused {
                    paused = false;
                    if let Some(started) = pause_started.take() {
                        paused_total += started.elapsed();
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        next_tick += TICK;
```

4f. In the source drain loop, discard samples while paused — the `Samples` arm becomes:

```rust
                    Ok(SourceMsg::Samples(raw)) => {
                        // Paused: keep draining (device loss stays
                        // detectable, channels never back up) but discard —
                        // the encoded timeline skips the gap entirely.
                        if paused {
                            continue;
                        }
                        let mono = mixer::downmix_to_mono(&raw, s.input.channels);
                        let mono = mixer::resample_linear(&mono, s.input.rate, TARGET_RATE);
                        s.buffer.extend(mono);
                        if s.buffer.len() > BUFFER_CAP {
                            let drop = s.buffer.len() - BUFFER_CAP;
                            log::warn!(
                                "capture: dropping {drop} overflowed samples ({})",
                                s.input.name
                            );
                            s.buffer.drain(..drop);
                        }
                    }
```

4g. The `take` computation gains a paused arm (encode nothing while paused; pre-pause remainder in the buffers is real audio and is encoded on resume or at finish):

```rust
        let take = if finish {
            states.iter().map(|s| s.buffer.len()).max().unwrap_or(0)
        } else if paused {
            0
        } else {
            tick_frames
        };
```

(The flush/fsync blocks below stay exactly where they are — the fsync cadence keeps running while paused by design.)

4h. Immediately after the loop (before the finalize comment), close an open span:

```rust
    // Stop while paused: close the open span so the note records it and
    // pause can never block or distort shutdown.
    if let Some(started) = pause_started.take() {
        paused_total += started.elapsed();
    }
```

4i. In the `NoteMeta` initializer inside `run_worker`, add after `recording_type`:

```rust
            paused: (paused_total.as_secs() > 0)
                .then(|| vault_buddy_core::capture_note::format_duration(paused_total.as_secs())),
```

4j. In the final `Ok(Outcome { ... })`, add `paused_secs: paused_total.as_secs(),`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test --workspace`
Expected: all green including the 3 new session tests and 1 new note test (the pause test takes ~3 s wall time). The shell crate still compiles: it only uses `CaptureSession::start/stop/is_running`, all unchanged in signature.

- [ ] **Step 6: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/capture/src/session.rs src-tauri/core/src/capture_note.rs src-tauri/capture/src/recovery.rs
git commit -m "feat(capture): pause/resume via session control channel

The stop channel generalizes to Control { Stop, Pause, Resume }. While
paused the drain loops keep running (device loss stays detectable) but
samples are discarded and nothing is encoded — the timeline skips the
gap; the fsync cadence keeps running. Stop-while-paused closes the open
span and finalizes normally, so pause can never block shutdown. The note
frontmatter records total paused time when > 0."
```

---

### Task 5: Live level tap (capture + one shell line)

**Files:**
- Modify: `src-tauri/capture/src/session.rs`
- Modify: `src-tauri/src/capture_commands.rs` (one field in the `SessionParams` initializer)

**Interfaces:**
- Produces (used by Task 9): `SessionParams` gains `pub level_tx: Option<Sender<f32>>` — post-mix per-tick peak, normalized 0–1, sent every other tick (~5 Hz), lossy/advisory.

- [ ] **Step 1: Write the failing test**

Append inside `mod tests` in `src-tauri/capture/src/session.rs`:

```rust
    #[test]
    fn level_tap_reports_normalized_peaks_for_a_known_sine() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let (level_tx, level_rx) = mpsc::channel();
        let mut p = params(dir.path());
        p.level_tx = Some(level_tx);
        let session = CaptureSession::start(
            p,
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        for chunk in sine_chunks(44_100, 1.0) {
            tx.send(SourceMsg::Samples(chunk)).unwrap();
        }
        std::thread::sleep(Duration::from_millis(600));
        let outcome = session.stop().unwrap();
        assert!(outcome.mp3.exists());
        let levels: Vec<f32> = level_rx.try_iter().collect();
        assert!(!levels.is_empty(), "levels were emitted");
        assert!(
            levels.iter().all(|l| (0.0..=1.0).contains(l)),
            "normalized 0-1: {levels:?}"
        );
        // 0.4-amplitude sine through soft_clip peaks near tanh(0.4) ≈ 0.38
        let max = levels.iter().cloned().fold(0.0f32, f32::max);
        assert!(max > 0.2, "peak tracks the signal: {max}");
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_capture level_tap`
Expected: COMPILE ERROR (`SessionParams` has no field `level_tx`).

- [ ] **Step 3: Implement**

3a. In `SessionParams` (after `warn_tx`):

```rust
    /// Advisory live level meter: post-mix per-tick peak (0–1), sent every
    /// other tick (~5 Hz). Lossy by design — a gone receiver must never
    /// slow or fail the encode path.
    pub level_tx: Option<Sender<f32>>,
```

3b. In `run_worker`, add a counter next to the pause state:

```rust
    let mut level_tick: u32 = 0;
```

3c. In the encode block, right after `let stereo = mixer::mix_to_stereo_i16(a, b);`:

```rust
            if let Some(level_tx) = &params.level_tx {
                level_tick = level_tick.wrapping_add(1);
                if level_tick % 2 == 0 {
                    let peak = stereo
                        .iter()
                        .map(|s| (*s as f32 / i16::MAX as f32).abs())
                        .fold(0.0f32, f32::max);
                    let _ = level_tx.send(peak);
                }
            }
```

3d. In the session tests' `params()` helper, add `level_tx: None,` after `warn_tx: None,`.

3e. In `src-tauri/src/capture_commands.rs`, in the `SessionParams { ... }` initializer inside `start_capture`, add `level_tx: None,` after `warn_tx: Some(warn_tx),` (Task 9 replaces it with a real channel).

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_capture level_tap`
Expected: PASS.

- [ ] **Step 5: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/capture/src/session.rs src-tauri/src/capture_commands.rs
git commit -m "feat(capture): live level tap on the session worker

Optional level_tx on SessionParams: the worker sends the post-mix
per-tick peak (normalized 0-1) every other 100 ms tick (~5 Hz). Advisory
only — sends are fire-and-forget so a slow or gone receiver can never
stall the encode path."
```

---

### Task 6: Device enumeration + preferred devices + start warning (capture + shell call site)

**Files:**
- Modify: `src-tauri/capture/src/devices.rs`
- Modify: `src-tauri/capture/src/session.rs` (`start_warning` param)
- Modify: `src-tauri/src/capture_commands.rs` (call site)

**Interfaces:**
- Produces (used by Tasks 8, 9):
  - `pub struct DeviceInfo { pub name: String, pub is_default: bool }`
  - `pub struct DeviceList { pub inputs: Vec<DeviceInfo>, pub outputs: Vec<DeviceInfo> }`
  - `pub fn list_devices() -> DeviceList` (never errors; empty lists on device-less CI)
  - `open_sources(meeting_mode: bool, preferred_input: Option<&str>, preferred_output: Option<&str>) -> Result<OpenSources, String>`; `OpenSources` gains `pub warnings: Vec<String>` (configured-but-missing device → default + warning; **never a start failure**)
  - `SessionParams` gains `pub start_warning: Option<String>` — seeds the worker's warning so stale-device fallbacks reach the note's `event:` metadata and `Outcome.warning`.

- [ ] **Step 1: Write the failing tests**

1a. Replace the existing `open_sources_never_panics` test in `src-tauri/capture/src/devices.rs` and add two more:

```rust
    /// CI runners have no audio devices; these assert the error paths are
    /// clean human-readable Errs (or graceful fallbacks), never panics. On
    /// a dev machine with devices they exercise the success paths instead.
    #[test]
    fn open_sources_never_panics() {
        match open_sources(true, None, None) {
            Ok(open) => {
                assert!(!open.inputs.is_empty());
                assert!(!open.inputs[0].name.is_empty(), "mic source is named");
            }
            Err(message) => {
                assert!(!message.is_empty());
            }
        }
    }

    #[test]
    fn missing_preferred_input_falls_back_with_a_warning() {
        // Stale config must never block recording: an unplugged configured
        // device degrades to the default plus a warning naming it.
        match open_sources(false, Some("No Such Device 9000"), None) {
            Ok(open) => {
                assert!(
                    open.warnings.iter().any(|w| w.contains("No Such Device 9000")),
                    "warning names the missing device: {:?}",
                    open.warnings
                );
            }
            Err(message) => assert!(!message.is_empty()), // device-less CI
        }
    }

    #[test]
    fn list_devices_is_clean_on_any_machine() {
        let list = list_devices();
        // No panic, and every entry is named; at most one default per side.
        assert!(list.inputs.iter().all(|d| !d.name.is_empty()));
        assert!(list.outputs.iter().all(|d| !d.name.is_empty()));
        assert!(list.inputs.iter().filter(|d| d.is_default).count() <= 1);
        assert!(list.outputs.iter().filter(|d| d.is_default).count() <= 1);
    }
```

1b. Append inside `mod tests` in `src-tauri/capture/src/session.rs`:

```rust
    #[test]
    fn start_warning_reaches_outcome_and_note_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let mut p = params(dir.path());
        p.start_warning = Some("Configured microphone \"X\" not found".into());
        let session = CaptureSession::start(
            p,
            vec![SourceInput {
                name: "mic".into(),
                rate: 44_100,
                channels: 1,
                rx,
            }],
        )
        .unwrap();
        tx.send(SourceMsg::Samples(vec![0.1f32; 4410])).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let outcome = session.stop().unwrap();
        assert!(
            outcome.warning.as_deref().unwrap_or("").contains("not found"),
            "warning surfaced: {:?}",
            outcome.warning
        );
        assert!(!outcome.ended_early, "a fallback is not an early end");
        let note = std::fs::read_to_string(outcome.note.expect("note")).unwrap();
        assert!(note.contains("event:"), "note metadata event: {note}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_capture`
Expected: COMPILE ERROR (`open_sources` takes 1 argument, `list_devices` not found, `start_warning` unknown field).

- [ ] **Step 3: Implement — devices.rs**

3a. Add above `OpenSources`:

```rust
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

pub struct DeviceList {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}

/// Enumerate capture-relevant devices by name. Never errors: an
/// enumeration failure (or a device-less CI box) yields empty lists —
/// the settings UI shows "System default" alone in that case.
pub fn list_devices() -> DeviceList {
    let host = cpal::default_host();
    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    let default_out = host.default_output_device().and_then(|d| d.name().ok());
    let name_list = |devices: Result<
        Vec<String>,
        cpal::DevicesError,
    >,
                     default: &Option<String>|
     -> Vec<DeviceInfo> {
        devices
            .unwrap_or_default()
            .into_iter()
            .map(|name| DeviceInfo {
                is_default: Some(&name) == default.as_ref(),
                name,
            })
            .collect()
    };
    let inputs = host
        .input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>());
    let outputs = host
        .output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect::<Vec<_>>());
    DeviceList {
        inputs: name_list(inputs, &default_in),
        outputs: name_list(outputs, &default_out),
    }
}
```

3b. `OpenSources` gains warnings:

```rust
pub struct OpenSources {
    pub inputs: Vec<SourceInput>,
    pub streams: Vec<cpal::Stream>,
    /// Configured-but-missing device fallbacks — recording proceeded on
    /// defaults; the caller surfaces these (stale config never blocks).
    pub warnings: Vec<String>,
}
```

3c. Add a by-name resolver and rework `open_sources`:

```rust
/// Resolve a configured device by exact name against the live device set;
/// None = not found (caller falls back to the default with a warning).
fn find_by_name<I: Iterator<Item = cpal::Device>>(devices: I, name: &str) -> Option<cpal::Device> {
    let mut devices = devices;
    devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

pub fn open_sources(
    meeting_mode: bool,
    preferred_input: Option<&str>,
    preferred_output: Option<&str>,
) -> Result<OpenSources, String> {
    let host = cpal::default_host();
    let mut inputs = Vec::new();
    let mut streams = Vec::new();
    let mut warnings = Vec::new();
    #[cfg(not(windows))]
    let _ = preferred_output; // loopback (and its device pick) is Windows-only

    let mic = match preferred_input {
        Some(name) => match host.input_devices().ok().and_then(|it| find_by_name(it, name)) {
            Some(device) => Some(device),
            None => {
                warnings.push(format!(
                    "Configured microphone \"{name}\" not found — using the default input device"
                ));
                None
            }
        },
        None => None,
    };
    let mic = match mic {
        Some(device) => device,
        None => host
            .default_input_device()
            .ok_or("No microphone found — check Windows sound settings.")?,
    };
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
        let output = match preferred_output {
            Some(name) => match host.output_devices().ok().and_then(|it| find_by_name(it, name)) {
                Some(device) => Some(device),
                None => {
                    warnings.push(format!(
                        "Configured output device \"{name}\" not found — using the default output device"
                    ));
                    None
                }
            },
            None => None,
        };
        let output = match output {
            Some(device) => device,
            None => host
                .default_output_device()
                .ok_or("Desktop audio (loopback) unavailable: no default output device")?,
        };
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

    Ok(OpenSources {
        inputs,
        streams,
        warnings,
    })
}
```

(If clippy objects to the `name_list` closure's formatting or the `Result<Vec<String>, cpal::DevicesError>` shape, simplify to two small `fn` helpers — behavior over form; keep the public signatures exactly as specified.)

- [ ] **Step 4: Implement — session `start_warning`**

4a. `SessionParams` gains (after `level_tx`):

```rust
    /// Warning that predates the session (e.g. a configured device missing
    /// at start): seeds the worker's warning so it reaches the note's
    /// event metadata and the final Outcome exactly like a live warning.
    pub start_warning: Option<String>,
```

4b. In `run_worker`, change the warning initializer:

```rust
    let mut warning: Option<String> = params.start_warning.clone();
```

4c. In the session tests' `params()` helper, add `start_warning: None,` after `level_tx: None,`.

- [ ] **Step 5: Implement — shell call site**

In `src-tauri/src/capture_commands.rs` inside the device thread in `start_capture`:

Replace:

```rust
        let open = match vault_buddy_capture::devices::open_sources(uses_loopback) {
```

with (the config values ride into the thread via the existing `cfg` move — `cfg` is already captured):

```rust
        let open = match vault_buddy_capture::devices::open_sources(
            uses_loopback,
            cfg.input_device.as_deref(),
            cfg.output_device.as_deref(),
        ) {
```

And below the `let open = ...` block (before `let now = chrono::Local::now();`), forward fallback warnings live and into the note:

```rust
        // Stale-device fallbacks: surface live (capture:warning via the
        // forwarder) AND seed the session so the note metadata records it.
        for w in &open.warnings {
            let _ = warn_tx.send(w.clone());
        }
        let start_warning = (!open.warnings.is_empty()).then(|| open.warnings.join("; "));
```

In the `SessionParams { ... }` initializer, add `start_warning,` after `level_tx: None,`.

- [ ] **Step 6: Run tests to verify they pass**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test --workspace`
Expected: all green (new devices tests tolerate device-less CI; `start_warning_reaches_outcome_and_note_metadata` passes).

- [ ] **Step 7: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/capture/src/devices.rs src-tauri/capture/src/session.rs src-tauri/src/capture_commands.rs
git commit -m "feat(capture): device enumeration and preferred-device selection

list_devices() enumerates inputs/outputs by name with a default flag
(empty lists, never errors, on device-less machines). open_sources gains
preferred input/output names resolved against the live device set; a
configured-but-missing device falls back to the default with a warning
that reaches capture:warning live and the note's event metadata — stale
config never blocks recording."
```

---

### Task 7: Rename execution (capture)

**Files:**
- Create: `src-tauri/capture/src/rename.rs`
- Modify: `src-tauri/capture/src/lib.rs` (register module)

**Interfaces:**
- Consumes: `RenamePlan` (Task 2), `retarget_embed` (Task 3), `rename_into_reserved` (existing, `recovery.rs`), `write_note_collision_safe` (core).
- Produces (used by Task 10): `pub struct RenameOutcome { pub mp3: PathBuf, pub note: Option<PathBuf>, pub warning: Option<String> }`, `pub fn execute(plan: &RenamePlan) -> Result<RenameOutcome, String>`.

Behavior contract: the mp3 move is the arbiter (reservation + `rename_noreplace`, suffix retry — a taken target advances the suffix, never clobbers). The note follows: read old text → retarget embed → collision-safe write at the reserved note name → remove the old note. Any note-side failure after a successful mp3 move degrades to a warning reporting both paths (audio first — the note is repairable by hand, the audio is not).

- [ ] **Step 1: Write the new module with failing tests**

Create `src-tauri/capture/src/rename.rs`:

```rust
//! Post-save rename: retitle a finished capture (mp3 + companion note)
//! under the same safety rails as the save path — pairwise reservation,
//! non-replacing renames, ownership filters. Audio first: the mp3 move is
//! the arbiter, and a note failure after a successful mp3 move degrades
//! to a warning (the note is repairable by hand; the audio is not).

use std::path::PathBuf;
use vault_buddy_core::capture_note::retarget_embed;
use vault_buddy_core::capture_note::write_note_collision_safe;
use vault_buddy_core::capture_paths::RenamePlan;

pub struct RenameOutcome {
    pub mp3: PathBuf,
    pub note: Option<PathBuf>,
    pub warning: Option<String>,
}

pub fn execute(plan: &RenamePlan) -> Result<RenameOutcome, String> {
    let old_mp3_name = plan
        .mp3_from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Read the note BEFORE moving anything: the embed rewrite needs the
    // old text, and a read failure should not strand a half-done pair.
    let note_read = match std::fs::read_to_string(&plan.note_from) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read the companion note: {e}")),
    };

    let (mp3_to, note_to) =
        crate::recovery::rename_into_reserved(&plan.mp3_from, &plan.dir, &plan.new_base)?;
    let new_mp3_name = mp3_to
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let (note, note_error) = match note_read {
        Ok(Some(text)) => {
            let retargeted = retarget_embed(&text, &old_mp3_name, &new_mp3_name);
            // Collision-safe exclusive create at the reserved name: a
            // sync-client race costs a suffix, never a clobbered file.
            match write_note_collision_safe(&note_to, &retargeted) {
                Ok(written) => match std::fs::remove_file(&plan.note_from) {
                    Ok(()) => (Some(written), None),
                    Err(e) => (
                        Some(written),
                        Some(format!("the old note could not be removed: {e}")),
                    ),
                },
                Err(e) => (None, Some(format!("the note could not be rewritten: {e}"))),
            }
        }
        Ok(None) => (None, None),
        Err(e) => (None, Some(e)),
    };

    let warning = note_error.map(|e| {
        format!(
            "Recording renamed, but its note needs attention ({e}). \
             Audio: {}; note: {}",
            mp3_to.display(),
            plan.note_from.display()
        )
    });

    Ok(RenameOutcome {
        mp3: mp3_to,
        note,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_core::capture_paths::{is_capture_base, rename_plan};

    fn seed(dir: &std::path::Path) -> (PathBuf, PathBuf) {
        let mp3 = dir.join("2026-07-04 1405 Meeting.mp3");
        let note = dir.join("2026-07-04 1405 Meeting.md");
        std::fs::write(&mp3, "mp3 bytes").unwrap();
        std::fs::write(
            &note,
            "---\nvault: \"W\"\n---\n\n![[2026-07-04 1405 Meeting.mp3]]\n",
        )
        .unwrap();
        (mp3, note)
    }

    #[test]
    fn renames_pair_and_retargets_embed() {
        let dir = tempfile::tempdir().unwrap();
        let (mp3, note) = seed(dir.path());
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        assert_eq!(outcome.mp3, dir.path().join("2026-07-04 1405 Standup.mp3"));
        assert_eq!(
            outcome.note.as_deref(),
            Some(dir.path().join("2026-07-04 1405 Standup.md").as_path())
        );
        assert!(outcome.warning.is_none(), "{:?}", outcome.warning);
        assert!(!mp3.exists(), "old mp3 moved");
        assert!(!note.exists(), "old note moved");
        let text = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(text.contains("![[2026-07-04 1405 Standup.mp3]]"), "{text}");
        assert!(!text.contains("Meeting.mp3"), "old embed gone: {text}");
        // recovery must still recognize the retitled files as ours
        let stem = outcome.mp3.file_stem().unwrap().to_string_lossy();
        assert!(is_capture_base(&stem));
    }

    #[test]
    fn collision_on_the_new_name_advances_the_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let (mp3, _note) = seed(dir.path());
        std::fs::write(dir.path().join("2026-07-04 1405 Standup.mp3"), "taken").unwrap();
        let plan = rename_plan(&mp3, "Standup").unwrap();
        let outcome = execute(&plan).unwrap();
        assert_eq!(
            outcome.mp3,
            dir.path().join("2026-07-04 1405 Standup (2).mp3")
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("2026-07-04 1405 Standup.mp3")).unwrap(),
            "taken",
            "never clobbers"
        );
        let text = std::fs::read_to_string(outcome.note.unwrap()).unwrap();
        assert!(
            text.contains("![[2026-07-04 1405 Standup (2).mp3]]"),
            "embed targets the suffixed name: {text}"
        );
    }

    #[test]
    fn mp3_without_note_renames_audio_only() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Voice Note.mp3");
        std::fs::write(&mp3, "mp3 bytes").unwrap();
        let plan = rename_plan(&mp3, "Idea").unwrap();
        let outcome = execute(&plan).unwrap();
        assert_eq!(outcome.mp3, dir.path().join("2026-07-04 1405 Idea.mp3"));
        assert!(outcome.note.is_none());
        assert!(outcome.warning.is_none());
    }

    #[test]
    fn missing_mp3_is_a_clean_error() {
        let dir = tempfile::tempdir().unwrap();
        let mp3 = dir.path().join("2026-07-04 1405 Meeting.mp3");
        let plan = rename_plan(&mp3, "Standup").unwrap();
        assert!(execute(&plan).is_err());
    }
}
```

- [ ] **Step 2: Register the module and verify red first**

Add `pub mod rename;` to `src-tauri/capture/src/lib.rs` (alphabetical: between `mixer` and `recovery`). For a strict red: create the file with the tests above but `execute` stubbed as `todo!()`:

```rust
pub fn execute(plan: &RenamePlan) -> Result<RenameOutcome, String> {
    let _ = plan;
    todo!("implemented in the next step")
}
```

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_capture rename`
Expected: 4 FAIL (panicked at 'not yet implemented'). Then replace the stub with the real body from Step 1.

- [ ] **Step 3: Run tests to verify they pass**

Run: `cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_capture rename`
Expected: 4 PASS.

- [ ] **Step 4: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green.

- [ ] **Step 5: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/capture/src/rename.rs src-tauri/capture/src/lib.rs
git commit -m "feat(capture): collision-safe capture rename execution

execute(plan) moves the mp3 through the same reservation +
rename_noreplace + suffix-retry loop as the save path, then rewrites the
companion note's embed at the reserved note name (collision-safe
exclusive create) and removes the old note. Audio first: a note failure
after a successful mp3 move degrades to a warning reporting both paths."
```

---

### Task 8: Shell — config and device commands

**Files:**
- Modify: `src-tauri/src/capture_commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `RecordingMode::from_key/as_key`, `update_vault_config` (Task 1), `safe_recording_root` (existing), `list_devices` (Task 6).
- Produces (used by Task 13's frontend):
  - Command `get_capture_config(id: String) -> CaptureConfigDto` — camelCase JSON `{ mode, recordingFolder, bitrateKbps, createNote, inputDevice, outputDevice }`.
  - Command `set_capture_config(id: String, cfg: CaptureConfigDto) -> Result<(), String>` — validates mode key, bitrate ∈ {128,160,192}, folder via `safe_recording_root` against the real vault path; empty-string folder/devices normalize to `None`; writes behind `ConfigWriteLock`.
  - Command `list_audio_devices() -> DeviceListDto` — `{ inputs: [{ name, isDefault }], outputs: [...] }`.
  - `pub struct ConfigWriteLock(pub Mutex<()>)` managed in the builder.

- [ ] **Step 1: Implement the commands**

Add to `src-tauri/src/capture_commands.rs` (below `capture_status`):

```rust
/// Serializes set_capture_config's read-modify-write of config.json —
/// concurrent saves for different vaults must not lose each other's
/// fields (the write path itself is lock-free by design).
#[derive(Default)]
pub struct ConfigWriteLock(pub Mutex<()>);

pub const BITRATES_KBPS: [u32; 3] = [128, 160, 192];

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfigDto {
    pub mode: String,
    pub recording_folder: Option<String>,
    pub bitrate_kbps: u32,
    pub create_note: bool,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
}

impl CaptureConfigDto {
    fn from_config(v: &capture_config::VaultCaptureConfig) -> Self {
        Self {
            mode: v.mode.as_key().to_string(),
            recording_folder: v.recording_folder.clone(),
            bitrate_kbps: v.bitrate_kbps,
            create_note: v.create_note,
            input_device: v.input_device.clone(),
            output_device: v.output_device.clone(),
        }
    }
}

#[tauri::command]
pub fn get_capture_config(id: String) -> CaptureConfigDto {
    // Unknown vaults return the defaults — exactly what a fresh form shows.
    CaptureConfigDto::from_config(&capture_config::vault_config(
        &capture_config::load_config(),
        &id,
    ))
}

#[tauri::command]
pub fn set_capture_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    cfg: CaptureConfigDto,
) -> Result<(), String> {
    let mode = capture_config::RecordingMode::from_key(&cfg.mode)
        .ok_or_else(|| format!("Unknown recording mode: {}", cfg.mode))?;
    if !BITRATES_KBPS.contains(&cfg.bitrate_kbps) {
        return Err(format!("Bitrate must be one of {BITRATES_KBPS:?} kbps"));
    }
    // Validate the folder against the real vault path BEFORE writing —
    // an invalid folder is an inline field error, nothing gets written.
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let folder = cfg
        .recording_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    if let Some(folder) = &folder {
        capture_paths::safe_recording_root(Path::new(&vault.path), folder)?;
    }
    let value = capture_config::VaultCaptureConfig {
        mode,
        recording_folder: folder,
        bitrate_kbps: cfg.bitrate_kbps,
        create_note: cfg.create_note,
        input_device: cfg.input_device.clone().filter(|d| !d.is_empty()),
        output_device: cfg.output_device.clone().filter(|d| !d.is_empty()),
    };
    let _guard = lock.0.lock().unwrap();
    capture_config::update_vault_config(&id, value)
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoDto {
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceListDto {
    pub inputs: Vec<DeviceInfoDto>,
    pub outputs: Vec<DeviceInfoDto>,
}

#[tauri::command]
pub fn list_audio_devices() -> DeviceListDto {
    let list = vault_buddy_capture::devices::list_devices();
    let map = |d: vault_buddy_capture::devices::DeviceInfo| DeviceInfoDto {
        name: d.name,
        is_default: d.is_default,
    };
    DeviceListDto {
        inputs: list.inputs.into_iter().map(map).collect(),
        outputs: list.outputs.into_iter().map(map).collect(),
    }
}
```

- [ ] **Step 2: Register in the builder**

In `src-tauri/src/lib.rs`:
- After `.manage(capture_commands::CaptureState::default())` add:

```rust
        .manage(capture_commands::ConfigWriteLock::default())
```

- In `tauri::generate_handler![...]`, after `capture_commands::capture_status` add:

```rust
            capture_commands::get_capture_config,
            capture_commands::set_capture_config,
            capture_commands::list_audio_devices
```

- [ ] **Step 3: Full Rust gate (this IS the test for shell code — plus Windows CI)**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green (the shell crate compiles here; its runtime is exercised by the Windows checklist).

- [ ] **Step 4: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): capture config and device commands

get/set_capture_config round-trip the per-vault settings; set validates
the mode key, the 128/160/192 bitrate set and the recording folder (via
safe_recording_root against the real vault path) before anything is
written, and serializes read-modify-writes behind a ConfigWriteLock so
concurrent saves for different vaults cannot lose each other.
list_audio_devices exposes cpal device names with default flags."
```

---

### Task 9: Shell — pause/resume, level events, paused tray state

**Files:**
- Modify: `src-tauri/src/capture_commands.rs`
- Modify: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `Control`, `CaptureSession::pause/resume` (Task 4), `SessionParams.level_tx` (Task 5).
- Produces (used by Tasks 11, 15):
  - Commands `pause_capture()` / `resume_capture()` → `Result<(), String>` (typed errors: no recording / still starting / already in that state).
  - Events: `capture:paused { atMs }`, `capture:resumed { pausedTotalMs }`, `capture:level { peak }`.
  - `StatusPayload` gains `paused: bool`, `paused_total_ms: u64`, `paused_since_ms: Option<u64>` (camelCase over IPC: `paused`, `pausedTotalMs`, `pausedSinceMs`).
  - `tray::TrayCaptureState { Idle, Recording, Paused }` + `tray::set_capture_state(app, state)` (replaces `set_recording`); tray menu shows "⏸ Pause recording" ⇄ "▶ Resume recording" while active; paused icon dot is steady amber.

- [ ] **Step 1: Generalize the shell channel to `Control`**

In `src-tauri/src/capture_commands.rs`:

1a. Replace the import of `SessionParams` line and drop `StopReason`:

```rust
use vault_buddy_capture::session::{CaptureSession, Control, Outcome, SessionParams};
```

Delete `pub enum StopReason { User }`.

1b. `ActiveCapture` becomes:

```rust
pub struct ActiveCapture {
    pub control_tx: Sender<Control>,
    pub vault_id: String,
    pub started_at_ms: u64,
    /// Pause bookkeeping mirrors the session (which owns the truth for the
    /// encoded timeline) so capture_status can resync a reloaded webview's
    /// frozen-elapsed display exactly.
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    /// The .part file the live session owns, once the worker has reserved
    /// it — None while devices are still being set up (and for a timed-out
    /// start whose worker never reported back).
    pub part: Option<PathBuf>,
}
```

1c. `StatusPayload` becomes:

```rust
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub recording: bool,
    pub vault_id: Option<String>,
    pub started_at_ms: Option<u64>,
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
}
```

`capture_status` fills the new fields from `ActiveCapture` (`paused: active.paused, paused_total_ms: active.paused_total_ms, paused_since_ms: active.paused_since_ms`); the `None` arm returns `paused: false, paused_total_ms: 0, paused_since_ms: None`.

1d. In `start_capture`: the channel becomes `let (control_tx, control_rx) = mpsc::channel::<Control>();` (rename `stop_tx`/`stop_rx` accordingly); the reservation initializer sets `control_tx: control_tx.clone(), paused: false, paused_total_ms: 0, paused_since_ms: None`; the `payload` construction after ready adds `paused: false, paused_total_ms: 0, paused_since_ms: None`; the timeout path sends `Control::Stop` instead of `StopReason::User`.

1e. The device-thread poll loop forwards pause/resume to the session (streams stay alive on this thread — cpal::Stream is !Send, do not move them):

```rust
        // Own the streams here; poll for control or self-finalization.
        let streams = open.streams;
        loop {
            match control_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(Control::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(Control::Pause) => session.pause(),
                Ok(Control::Resume) => session.resume(),
                Err(RecvTimeoutError::Timeout) => {
                    if !session.is_running() {
                        break; // sources died; worker self-finalized
                    }
                }
            }
        }
```

1f. `request_stop_and_wait` sends `Control::Stop` (`let _ = active.control_tx.send(Control::Stop);`).

- [ ] **Step 2: Level forwarding**

In `start_capture`, next to the warn forwarder thread, add:

```rust
    // Advisory level meter: forward the worker's ~5 Hz peaks to the panel.
    let (level_tx, level_rx) = mpsc::channel::<f32>();
    let app_level = app.clone();
    std::thread::spawn(move || {
        while let Ok(peak) = level_rx.recv() {
            let _ = app_level.emit("capture:level", serde_json::json!({ "peak": peak }));
        }
    });
```

and in the `SessionParams { ... }` initializer replace `level_tx: None,` with `level_tx: Some(level_tx),`.

- [ ] **Step 3: pause/resume commands + menu helpers**

Add to `src-tauri/src/capture_commands.rs`:

```rust
/// Shared by the IPC commands and the tray menu items. Errors are typed
/// for the UI (which disables the buttons in starting/saving states) —
/// but the tray can always race, so every precondition re-checks here.
fn set_paused(app: &AppHandle, pause: bool) -> Result<(), String> {
    let state = app.state::<CaptureState>();
    let mut guard = state.0.lock().unwrap();
    let Some(active) = guard.as_mut() else {
        return Err("No recording is running.".to_string());
    };
    if active.part.is_none() {
        return Err("Recording is still starting.".to_string());
    }
    if pause == active.paused {
        return Err(if pause {
            "Recording is already paused."
        } else {
            "Recording is not paused."
        }
        .to_string());
    }
    let now = now_ms();
    if pause {
        active.paused = true;
        active.paused_since_ms = Some(now);
        let _ = active.control_tx.send(Control::Pause);
    } else {
        active.paused = false;
        active.paused_total_ms += now.saturating_sub(active.paused_since_ms.take().unwrap_or(now));
        let _ = active.control_tx.send(Control::Resume);
    }
    let paused_total_ms = active.paused_total_ms;
    drop(guard);
    if pause {
        let _ = app.emit("capture:paused", serde_json::json!({ "atMs": now }));
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Paused);
    } else {
        let _ = app.emit(
            "capture:resumed",
            serde_json::json!({ "pausedTotalMs": paused_total_ms }),
        );
        crate::tray::set_capture_state(app, crate::tray::TrayCaptureState::Recording);
    }
    Ok(())
}

#[tauri::command]
pub fn pause_capture(app: AppHandle) -> Result<(), String> {
    set_paused(&app, true)
}

#[tauri::command]
pub fn resume_capture(app: AppHandle) -> Result<(), String> {
    set_paused(&app, false)
}

/// Tray menu variants: failures only log — there is no panel to show them.
pub fn pause_from_menu(app: &AppHandle) {
    if let Err(e) = set_paused(app, true) {
        log::warn!("pause from tray: {e}");
    }
}

pub fn resume_from_menu(app: &AppHandle) {
    if let Err(e) = set_paused(app, false) {
        log::warn!("resume from tray: {e}");
    }
}
```

- [ ] **Step 4: Tray paused state**

In `src-tauri/src/tray.rs`:

4a. Add the state enum:

```rust
/// Recording indicator states the tray can show. Paused is deliberately
/// its own visual (steady amber vs. red) — a user glancing at the tray
/// must be able to tell "capturing audio right now" from "not capturing,
/// but a session is open".
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayCaptureState {
    Idle,
    Recording,
    Paused,
}
```

4b. `buddy_icon` takes the enum; the dot turns amber when paused:

```rust
fn buddy_icon(state: TrayCaptureState) -> tauri::image::Image<'static> {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE / 2) as i32;
    let dot: Option<[u8; 4]> = match state {
        TrayCaptureState::Idle => None,
        TrayCaptureState::Recording => Some([0xe0, 0x2e, 0x2e, 0xff]), // red
        TrayCaptureState::Paused => Some([0xf5, 0x9e, 0x0b, 0xff]),    // amber
    };
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let idx = ((y as u32 * SIZE + x as u32) * 4) as usize;
            let dx = x - center;
            let dy = y - center;
            if dx * dx + dy * dy <= (center - 2) * (center - 2) {
                rgba[idx..idx + 4].copy_from_slice(&[0x7c, 0x5c, 0xff, 0xff]);
            }
            if let Some(color) = dot {
                // dot bottom-right
                let rx = x - (SIZE as i32 - 9);
                let ry = y - (SIZE as i32 - 9);
                if rx * rx + ry * ry <= 36 {
                    rgba[idx..idx + 4].copy_from_slice(&color);
                }
            }
        }
    }
    tauri::image::Image::new_owned(rgba, SIZE, SIZE)
}
```

4c. `tray_menu` takes the enum and adds the pause/resume item while active:

```rust
fn tray_menu(app: &AppHandle, state: TrayCaptureState) -> tauri::Result<Menu<tauri::Wry>> {
    let active = state != TrayCaptureState::Idle;
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", !active, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    if active {
        let pause_resume = if state == TrayCaptureState::Paused {
            MenuItem::with_id(
                app,
                "tray-resume-recording",
                "▶ Resume recording",
                true,
                None::<&str>,
            )?
        } else {
            MenuItem::with_id(
                app,
                "tray-pause-recording",
                "⏸ Pause recording",
                true,
                None::<&str>,
            )?
        };
        let stop = MenuItem::with_id(
            app,
            "tray-stop-recording",
            "⏹ Stop recording",
            true,
            None::<&str>,
        )?;
        Menu::with_items(app, &[&pause_resume, &stop, &toggle, &quit_item])
    } else {
        Menu::with_items(app, &[&toggle, &quit_item])
    }
}
```

4d. Replace `set_recording` with:

```rust
/// Swap the tray icon, tooltip, and menu to reflect capture state. Called
/// on start, pause, resume, and finish (successful or not).
pub fn set_capture_state(app: &AppHandle, state: TrayCaptureState) {
    if let Some(tray) = app.tray_by_id("main-tray") {
        let _ = tray.set_icon(Some(buddy_icon(state)));
        let _ = tray.set_tooltip(Some(match state {
            TrayCaptureState::Idle => "Vault Buddy",
            TrayCaptureState::Recording => "Vault Buddy — recording",
            TrayCaptureState::Paused => "Vault Buddy — paused",
        }));
        if let Ok(menu) = tray_menu(app, state) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}
```

4e. `create_tray`: `let menu = tray_menu(app, TrayCaptureState::Idle)?;` and `.icon(buddy_icon(TrayCaptureState::Idle))`; add the two menu arms next to `tray-stop-recording` (pause/resume take the state lock only briefly — no finalize wait — so unlike stop they can run inline in the callback):

```rust
            "tray-pause-recording" => crate::capture_commands::pause_from_menu(app),
            "tray-resume-recording" => crate::capture_commands::resume_from_menu(app),
```

4f. Update ALL former `set_recording` call sites in `capture_commands.rs`:
- monitor thread: `crate::tray::set_capture_state(&app3, crate::tray::TrayCaptureState::Idle);`
- janitor thread (timeout path): `crate::tray::set_capture_state(&app4, crate::tray::TrayCaptureState::Idle);`
- after successful start: `crate::tray::set_capture_state(&app, crate::tray::TrayCaptureState::Recording);`

- [ ] **Step 5: Register commands**

In `src-tauri/src/lib.rs` `generate_handler!`, after `capture_commands::list_audio_devices` add:

```rust
            capture_commands::pause_capture,
            capture_commands::resume_capture
```

- [ ] **Step 6: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green. Grep-check no `StopReason` or `set_recording` references remain: `grep -rn "StopReason\|set_recording" src-tauri/src/` → no output.

- [ ] **Step 7: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/src/capture_commands.rs src-tauri/src/tray.rs src-tauri/src/lib.rs
git commit -m "feat(shell): pause/resume, level events, paused tray state

The shell stop channel generalizes to the session's Control enum; the
device thread forwards Pause/Resume to the session so the !Send streams
never move. pause/resume commands keep authoritative pause bookkeeping
on ActiveCapture (resynced via capture_status after a webview reload),
emit capture:paused/resumed, and flip the tray between the red recording
dot, a steady amber paused dot, and a Pause <-> Resume menu item. A
forwarder thread relays the worker's ~5 Hz peaks as capture:level."
```

---

### Task 10: Shell — rename_capture command

**Files:**
- Modify: `src-tauri/src/capture_commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `rename_plan` (Task 2), `rename::execute` (Task 7).
- Produces (used by Task 11): command `rename_capture(mp3: String, title: String) -> Result<RenamedPayload, String>` with camelCase payload `{ mp3, note, warning }`. Confirming the unedited prefilled title is a no-op success (same paths back, nothing moved).

- [ ] **Step 1: Implement**

Add to `src-tauri/src/capture_commands.rs`:

```rust
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenamedPayload {
    pub mp3: String,
    pub note: Option<String>,
    pub warning: Option<String>,
}

#[tauri::command]
pub fn rename_capture(
    state: tauri::State<CaptureState>,
    mp3: String,
    title: String,
) -> Result<RenamedPayload, String> {
    // The prompt dismisses on a new recording (UI rule); this is the
    // backend guard for the same thing — never shuffle files next to a
    // directory a live session is writing into.
    if state.0.lock().unwrap().is_some() {
        return Err("Cannot rename while a recording is running.".to_string());
    }
    // rename_plan re-validates ownership (capture-pattern stems only), so
    // an arbitrary user mp3 can never be renamed through this command.
    let plan = capture_paths::rename_plan(Path::new(&mp3), &title)?;
    if !plan.mp3_from.is_file() {
        return Err("Recording file not found — was it moved?".to_string());
    }
    let stem = plan
        .mp3_from
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if plan.new_base == stem {
        // Confirming the unedited prefill: nothing to do, and running the
        // reservation anyway would mint a pointless " (2)" suffix (the
        // source itself occupies the target name).
        return Ok(RenamedPayload {
            note: plan
                .note_from
                .is_file()
                .then(|| plan.note_from.to_string_lossy().into_owned()),
            mp3,
            warning: None,
        });
    }
    let outcome = vault_buddy_capture::rename::execute(&plan)?;
    Ok(RenamedPayload {
        mp3: outcome.mp3.to_string_lossy().into_owned(),
        note: outcome.note.map(|p| p.to_string_lossy().into_owned()),
        warning: outcome.warning,
    })
}
```

- [ ] **Step 2: Register**

In `src-tauri/src/lib.rs` `generate_handler!`, after `capture_commands::resume_capture` add:

```rust
            capture_commands::rename_capture
```

- [ ] **Step 3: Full Rust gate**

Run: `cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: clean, all green.

- [ ] **Step 4: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): rename_capture command

Plans via rename_plan (ownership + title validation), executes via the
capture crate's reservation + rename_noreplace path, and returns the
final mp3/note paths plus an optional note-side warning. Renaming is
refused while a recording runs, and confirming the unedited prefill is
a no-op success instead of minting a pointless suffix."
```

---

### Task 11: Frontend — capture store: pause/level/vaultId/rename window

**Files:**
- Modify: `src/types.ts`
- Modify: `src/stores/capture.ts`
- Modify: `tests/capture-store.test.ts`

**Interfaces:**
- Consumes (IPC, mocked in tests): `pause_capture`, `resume_capture`, `rename_capture`, extended `capture_status`; events `capture:paused { atMs }`, `capture:resumed { pausedTotalMs }`, `capture:level { peak }`, extended `capture:saved { mp3, note, endedEarly }`.
- Produces (used by Tasks 13–16): store state `paused: boolean`, `pausedTotalMs: number`, `pausedSinceMs: number | null`, `level: number`, `vaultId: string | null`, `lastSaved: { mp3: string; note: string | null } | null`, `renameError: string | null`; actions `pause()`, `resume()`, `rename(title)`, `dismissRename()`; the rename window auto-dismisses after 30 s (`RENAME_PROMPT_MS = 30_000`).

- [ ] **Step 1: Extend types**

In `src/types.ts`, replace `CaptureStatus` and add `CaptureRenamed`:

```ts
export interface CaptureStatus {
  recording: boolean;
  vaultId: string | null;
  startedAtMs: number | null;
  paused: boolean;
  pausedTotalMs: number;
  pausedSinceMs: number | null;
}

export interface CaptureRenamed {
  mp3: string;
  note: string | null;
  warning: string | null;
}
```

- [ ] **Step 2: Write the failing tests**

Append to `tests/capture-store.test.ts` (inside the existing `describe`; the existing mocks in this file that return `capture_status` objects keep working — extra fields are additive; extend the two resync mocks you touch below as shown):

```ts
  it("pause and resume flow through IPC and mirror events", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1_000 };
      }
    });
    const store = useCaptureStore();
    await store.start("v1");
    await store.pause();
    expect(calls).toContain("pause_capture");
    // Rust confirms via event — the store mirrors it, not the invoke
    expect(store.paused).toBe(false);
    state.eventHandlers["capture:paused"]!({ payload: { atMs: 5_000 } });
    expect(store.paused).toBe(true);
    expect(store.pausedSinceMs).toBe(5_000);
    await store.pause(); // already paused: no second IPC call
    expect(calls.filter((c) => c === "pause_capture")).toHaveLength(1);
    await store.resume();
    expect(calls).toContain("resume_capture");
    state.eventHandlers["capture:resumed"]!({
      payload: { pausedTotalMs: 2_500 },
    });
    expect(store.paused).toBe(false);
    expect(store.pausedSinceMs).toBeNull();
    expect(store.pausedTotalMs).toBe(2_500);
  });

  it("level events update the meter value, clamped to 0..1", async () => {
    mockIPC(() => undefined);
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:level"]!({ payload: { peak: 0.42 } });
    expect(store.level).toBeCloseTo(0.42);
    state.eventHandlers["capture:level"]!({ payload: { peak: 7 } });
    expect(store.level).toBe(1);
  });

  it("saved event opens the rename window and clears recording state", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v1", startedAtMs: 1 };
      }
    });
    const store = useCaptureStore();
    await store.start("v1");
    expect(store.vaultId).toBe("v1");
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/M/2026/07/2026-07-04 1405 Meeting.mp3", note: "/v/M/2026/07/2026-07-04 1405 Meeting.md", endedEarly: false },
    });
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.paused).toBe(false);
    expect(store.level).toBe(0);
    expect(store.lastSaved).toEqual({
      mp3: "/v/M/2026/07/2026-07-04 1405 Meeting.mp3",
      note: "/v/M/2026/07/2026-07-04 1405 Meeting.md",
    });
  });

  it("rename window expires after 30s", async () => {
    vi.useFakeTimers();
    mockIPC(() => undefined);
    const store = useCaptureStore();
    await store.init();
    state.eventHandlers["capture:saved"]!({
      payload: { mp3: "/v/m.mp3", note: null, endedEarly: false },
    });
    expect(store.lastSaved).not.toBeNull();
    vi.advanceTimersByTime(29_000);
    expect(store.lastSaved).not.toBeNull();
    vi.advanceTimersByTime(2_000);
    expect(store.lastSaved).toBeNull();
    vi.useRealTimers();
  });

  it("rename calls rename_capture and updates the saved file", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "rename_capture") {
        return { mp3: "/v/2026-07-04 1405 Standup.mp3", note: null, warning: null };
      }
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.rename("Standup");
    expect(calls).toEqual([
      {
        cmd: "rename_capture",
        args: { mp3: "/v/2026-07-04 1405 Meeting.mp3", title: "Standup" },
      },
    ]);
    expect(store.lastSavedFile).toBe("/v/2026-07-04 1405 Standup.mp3");
    expect(store.lastSaved).toBeNull();
    expect(store.renameError).toBeNull();
  });

  it("rename failure keeps the prompt up with the error", async () => {
    mockIPC(() => {
      throw "Title is too long";
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await store.rename("x");
    expect(store.lastSaved).not.toBeNull();
    expect(store.renameError).toContain("Title is too long");
  });

  it("a new recording dismisses the rename window", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_capture") {
        return { recording: true, vaultId: "v2", startedAtMs: 9 };
      }
    });
    const store = useCaptureStore();
    store.lastSaved = { mp3: "/v/old.mp3", note: null };
    await store.start("v2");
    expect(store.lastSaved).toBeNull();
  });

  it("init resyncs paused state from capture_status", async () => {
    mockIPC((cmd) => {
      if (cmd === "capture_status") {
        return {
          recording: true,
          vaultId: "v9",
          startedAtMs: 7,
          paused: true,
          pausedTotalMs: 1_500,
          pausedSinceMs: 9_000,
        };
      }
    });
    const store = useCaptureStore();
    await store.init();
    expect(store.status).toBe("recording");
    expect(store.vaultId).toBe("v9");
    expect(store.paused).toBe(true);
    expect(store.pausedTotalMs).toBe(1_500);
    expect(store.pausedSinceMs).toBe(9_000);
  });
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: FAIL — `store.pause is not a function`, `lastSaved` undefined, etc.

- [ ] **Step 4: Implement the store**

Replace `src/stores/capture.ts` with:

```ts
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CaptureRenamed, CaptureSaved, CaptureStatus } from "../types";

/** How long the post-save "Name this recording" window stays open. */
export const RENAME_PROMPT_MS = 30_000;

export const useCaptureStore = defineStore("capture", {
  state: () => ({
    status: "idle" as "idle" | "starting" | "recording" | "saving",
    startedAtMs: null as number | null,
    /** Which vault is recording — drives the per-vault row indicator. */
    vaultId: null as string | null,
    paused: false,
    /** Accumulated pause time (authoritative value from Rust events). */
    pausedTotalMs: 0,
    /** Start of the current pause span; null while not paused. */
    pausedSinceMs: null as number | null,
    /** Advisory level meter, 0..1 (~5 Hz from capture:level). */
    level: 0,
    error: null as string | null,
    warning: null as string | null,
    lastSavedFile: null as string | null,
    /** Post-save rename window; null once renamed/dismissed/expired. */
    lastSaved: null as { mp3: string; note: string | null } | null,
    renameError: null as string | null,
    renameTimer: null as ReturnType<typeof setTimeout> | null,
  }),
  actions: {
    resetRecordingState() {
      this.status = "idle";
      this.startedAtMs = null;
      this.vaultId = null;
      this.paused = false;
      this.pausedTotalMs = 0;
      this.pausedSinceMs = null;
      this.level = 0;
    },
    async init() {
      await listen<CaptureSaved>("capture:saved", (event) => {
        this.resetRecordingState();
        this.lastSavedFile = event.payload.mp3;
        // A previous stop/failure may have left a stale banner up —
        // a fresh successful save means neither is still relevant.
        this.error = null;
        this.warning = null;
        this.lastSaved = { mp3: event.payload.mp3, note: event.payload.note };
        this.renameError = null;
        this.armRenameExpiry();
      });
      await listen<{ message: string }>("capture:failed", (event) => {
        this.resetRecordingState();
        this.error = event.payload.message;
      });
      await listen<{ message: string }>("capture:warning", (event) => {
        this.warning = event.payload.message;
      });
      await listen<{ atMs: number }>("capture:paused", (event) => {
        this.paused = true;
        this.pausedSinceMs = event.payload.atMs ?? Date.now();
      });
      await listen<{ pausedTotalMs: number }>("capture:resumed", (event) => {
        this.paused = false;
        this.pausedSinceMs = null;
        this.pausedTotalMs = event.payload.pausedTotalMs ?? this.pausedTotalMs;
      });
      await listen<{ peak: number }>("capture:level", (event) => {
        this.level = Math.min(1, Math.max(0, event.payload.peak ?? 0));
      });
      // Resync: the webview can reload while Rust keeps recording.
      try {
        const s = await invoke<CaptureStatus>("capture_status");
        if (s.recording) {
          this.status = "recording";
          this.startedAtMs = s.startedAtMs;
          this.vaultId = s.vaultId;
          this.paused = s.paused;
          this.pausedTotalMs = s.pausedTotalMs ?? 0;
          this.pausedSinceMs = s.pausedSinceMs ?? null;
        }
      } catch {
        // not running under Tauri (unit tests without a status mock)
      }
    },
    async start(vaultId: string) {
      // Synchronous guard + "starting" state: without it a double-click
      // fires start_capture twice during device setup, and the second
      // call's "already running" rejection would reset the UI to idle
      // while Rust keeps recording.
      if (this.status !== "idle") return;
      this.status = "starting";
      this.error = null;
      this.warning = null;
      // New recording: the previous save's rename window is over.
      this.dismissRename();
      try {
        const s = await invoke<CaptureStatus>("start_capture", { id: vaultId });
        this.status = "recording";
        this.startedAtMs = s.startedAtMs;
        this.vaultId = s.vaultId;
        this.paused = false;
        this.pausedTotalMs = 0;
        this.pausedSinceMs = null;
        this.level = 0;
      } catch (e) {
        // Only downgrade if this attempt still owns the state — an event
        // may have moved it on in the meantime.
        if (this.status === "starting") this.status = "idle";
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
    async pause() {
      if (this.status !== "recording" || this.paused) return;
      try {
        await invoke("pause_capture");
        // capture:paused flips the state — Rust owns the truth.
      } catch (e) {
        this.error = String(e);
      }
    },
    async resume() {
      if (this.status !== "recording" || !this.paused) return;
      try {
        await invoke("resume_capture");
      } catch (e) {
        this.error = String(e);
      }
    },
    armRenameExpiry() {
      if (this.renameTimer) clearTimeout(this.renameTimer);
      this.renameTimer = setTimeout(() => this.dismissRename(), RENAME_PROMPT_MS);
    },
    dismissRename() {
      if (this.renameTimer) {
        clearTimeout(this.renameTimer);
        this.renameTimer = null;
      }
      this.lastSaved = null;
      this.renameError = null;
    },
    async rename(title: string) {
      if (!this.lastSaved) return;
      this.renameError = null;
      try {
        const r = await invoke<CaptureRenamed>("rename_capture", {
          mp3: this.lastSaved.mp3,
          title,
        });
        this.lastSavedFile = r.mp3;
        if (r.warning) this.warning = r.warning;
        this.dismissRename();
      } catch (e) {
        // Prompt stays up so the user can fix the title and retry.
        this.renameError = String(e);
      }
    },
  },
});
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: all PASS (8 new + 7 existing).

- [ ] **Step 6: Full frontend gate**

Run: `npm run test && npm run build`
Expected: all suites green, typecheck+build clean.

- [ ] **Step 7: Commit**

```bash
cd /home/user/vault-buddy
git add src/types.ts src/stores/capture.ts tests/capture-store.test.ts
git commit -m "feat(ui): capture store pause/level/vaultId/rename state

Pause state mirrors Rust events (capture:paused/resumed carry the
authoritative timestamps) and resyncs via capture_status after a webview
reload. capture:level drives an advisory 0..1 meter value. A saved
capture opens a 30s rename window; rename() calls rename_capture and a
failure keeps the prompt up for a retry, while a new recording or
dismissal closes it with no dangling timer."
```

---

### Task 12: Frontend — panel view state becomes a union

**Files:**
- Modify: `src/stores/vaults.ts`
- Modify: `src/stores/updates.ts`
- Modify: `src/components/ActionPanel.vue`
- Modify: `tests/vaults-store.test.ts`, `tests/action-panel.test.ts`, `tests/updates-store.test.ts`

**Interfaces:**
- Produces (used by Tasks 13, 14): vaults store state `view: "list" | "settings" | "captureSettings"` (replaces `showSettings: boolean`), `captureSettingsVaultId: string | null`; actions `showList()`, `openSettings()`, `openCaptureSettings(vaultId: string)`. Panel open always resets to `list`. View state stays in the store because the panel component is destroyed while closed.

- [ ] **Step 1: Update the failing tests first**

1a. `tests/vaults-store.test.ts` — the existing test that sets `store.showSettings = true` and expects it reset on open becomes:

```ts
  it("reopening the panel always lands on the vault list", async () => {
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    store.openSettings();
    expect(store.view).toBe("settings");
    await store.togglePanel(); // open
    expect(store.view).toBe("list");
    store.openCaptureSettings("v1");
    expect(store.view).toBe("captureSettings");
    expect(store.captureSettingsVaultId).toBe("v1");
    store.showList();
    expect(store.view).toBe("list");
    expect(store.captureSettingsVaultId).toBeNull();
  });
```

(Replace the old `showSettings` assertions in that file with this; keep the rest of the file untouched.)

1b. `tests/action-panel.test.ts` — replace `store.showSettings = true;` (line ~157) with `store.view = "settings";`.

1c. `tests/updates-store.test.ts` — replace `vaults.showSettings = false;` with `vaults.view = "list";`, `vaults.showSettings = true;` with `vaults.view = "settings";`, and `expect(vaults.showSettings).toBe(true);` with `expect(vaults.view).toBe("settings");`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/vaults-store.test.ts tests/action-panel.test.ts tests/updates-store.test.ts`
Expected: FAIL (`openSettings is not a function`, `view` undefined).

- [ ] **Step 3: Implement the store**

In `src/stores/vaults.ts`:

Replace the `showSettings: false,` state line (keep its comment, updated) with:

```ts
    // Which panel view is showing. Lives here (not in ActionPanel) because
    // the panel is destroyed while closed — a failed update install must be
    // able to reopen it directly on settings, where the error UI lives.
    view: "list" as "list" | "settings" | "captureSettings",
    // Which vault the captureSettings view edits.
    captureSettingsVaultId: null as string | null,
```

In `togglePanel`, replace `this.showSettings = false;` with `this.showList();`.

Add actions:

```ts
    showList() {
      this.view = "list";
      this.captureSettingsVaultId = null;
    },
    openSettings() {
      this.view = "settings";
    },
    openCaptureSettings(vaultId: string) {
      this.view = "captureSettings";
      this.captureSettingsVaultId = vaultId;
    },
```

In `src/stores/updates.ts`, replace `vaults.showSettings = true;` with `vaults.openSettings();`.

- [ ] **Step 4: Update ActionPanel**

In `src/components/ActionPanel.vue` `<script setup>`:

```ts
const { view } = storeToRefs(store); // replaces showSettings
```

and

```ts
const showFilter = computed(
  () => view.value === "list" && store.vaults.length > FILTER_THRESHOLD,
);
```

Template — mechanical replacements:
- `{{ showSettings ? "Buddy settings" : "Vaults" }}` → `{{ view === "settings" ? "Buddy settings" : "Vaults" }}` (Task 13 adds the captureSettings title)
- `v-if="!showSettings && store.vaults.length > 0"` → `v-if="view === 'list' && store.vaults.length > 0"`
- Gear button: `:class="{ 'text-violet-300': view === 'settings' }"`, `:aria-label="view === 'list' ? 'Buddy settings' : 'Back to vaults'"`, `:aria-pressed="view === 'settings'"`, `:title="view === 'list' ? 'Buddy settings' : 'Back to vaults'"`, `@click="view === 'list' ? store.openSettings() : store.showList()"`
- `v-if="!showSettings && store.error"` → `v-if="view === 'list' && store.error"`
- RecordingBar `v-if="!showSettings && capture.status !== 'idle'"` → `v-if="view === 'list' && capture.status !== 'idle'"`
- capture.error `v-if="!showSettings && capture.error"` → `v-if="view === 'list' && capture.error"`
- Body: `<div v-if="showSettings" ...>` → `<div v-if="view === 'settings'" ...>`

- [ ] **Step 5: Run tests to verify they pass**

Run: `npm run test`
Expected: all green (`grep -rn "showSettings" src/ tests/` → no output).

- [ ] **Step 6: Build + commit**

Run: `npm run build` — expected clean.

```bash
cd /home/user/vault-buddy
git add src/stores/vaults.ts src/stores/updates.ts src/components/ActionPanel.vue tests/vaults-store.test.ts tests/action-panel.test.ts tests/updates-store.test.ts
git commit -m "refactor(ui): panel view state as a union

showSettings grows a third state (the per-vault capture settings view),
so the boolean becomes view: list | settings | captureSettings plus
captureSettingsVaultId. Still store-backed: the panel component is
destroyed while closed and a failed update install must reopen straight
onto the settings view. Opening the panel always lands on the list."
```

---

### Task 13: Frontend — CaptureSettings view

**Files:**
- Create: `src/components/CaptureSettings.vue`
- Modify: `src/types.ts`, `src/components/ActionPanel.vue`
- Test: `tests/capture-settings.test.ts` (new)

**Interfaces:**
- Consumes: `get_capture_config` / `set_capture_config` / `list_audio_devices` (Task 8 DTO shapes), `view`/`captureSettingsVaultId` (Task 12).
- Produces: `CaptureSettings.vue` with prop `vaultId: string`; types `CaptureConfig`, `AudioDevice`, `AudioDevices` in `src/types.ts`.

- [ ] **Step 1: Add types**

Append to `src/types.ts`:

```ts
export interface CaptureConfig {
  mode: "meeting" | "voice-note";
  recordingFolder: string | null;
  bitrateKbps: number;
  createNote: boolean;
  inputDevice: string | null;
  outputDevice: string | null;
}

export interface AudioDevice {
  name: string;
  isDefault: boolean;
}

export interface AudioDevices {
  inputs: AudioDevice[];
  outputs: AudioDevice[];
}
```

- [ ] **Step 2: Write the failing tests**

Create `tests/capture-settings.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import CaptureSettings from "../src/components/CaptureSettings.vue";

const config = {
  mode: "meeting",
  recordingFolder: "Meetings",
  bitrateKbps: 160,
  createNote: true,
  inputDevice: "USB Mic",
  outputDevice: null,
};

const devices = {
  inputs: [
    { name: "USB Mic", isDefault: false },
    { name: "Built-in Mic", isDefault: true },
  ],
  outputs: [{ name: "Speakers", isDefault: true }],
};

const mountLoaded = async (
  overrides: {
    config?: Partial<typeof config>;
    devices?: typeof devices;
    onSet?: (args: unknown) => unknown;
  } = {},
) => {
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "get_capture_config") return { ...config, ...overrides.config };
    if (cmd === "list_audio_devices") return overrides.devices ?? devices;
    if (cmd === "set_capture_config") return overrides.onSet?.(args);
  });
  const wrapper = mount(CaptureSettings, { props: { vaultId: "v1" } });
  await flushPromises();
  return { wrapper, calls };
};

describe("CaptureSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => clearMocks());

  it("loads the config into the form", async () => {
    const { wrapper, calls } = await mountLoaded();
    expect(calls.map((c) => c.cmd)).toContain("get_capture_config");
    expect(calls.map((c) => c.cmd)).toContain("list_audio_devices");
    const folder = wrapper.get<HTMLInputElement>('[data-testid="folder-input"]');
    expect(folder.element.value).toBe("Meetings");
    const bitrate = wrapper.get<HTMLSelectElement>('[data-testid="bitrate-select"]');
    expect(bitrate.element.value).toBe("160");
    const input = wrapper.get<HTMLSelectElement>('[data-testid="input-device-select"]');
    expect(input.element.value).toBe("USB Mic");
  });

  it("System default is the first option in both device pickers", async () => {
    const { wrapper } = await mountLoaded();
    for (const testid of ["input-device-select", "output-device-select"]) {
      const options = wrapper.get(`[data-testid="${testid}"]`).findAll("option");
      expect(options[0]!.text()).toBe("System default");
      expect(options[0]!.attributes("value")).toBe("");
    }
  });

  it("marks a configured-but-absent device as not connected instead of dropping it", async () => {
    const { wrapper } = await mountLoaded({
      config: { inputDevice: "Unplugged Headset" },
    });
    const select = wrapper.get<HTMLSelectElement>('[data-testid="input-device-select"]');
    expect(select.element.value).toBe("Unplugged Headset");
    expect(select.text()).toContain("Unplugged Headset (not connected)");
  });

  it("hides the output picker in voice-note mode", async () => {
    const { wrapper } = await mountLoaded({ config: { mode: "voice-note" } });
    expect(wrapper.find('[data-testid="output-device-select"]').exists()).toBe(false);
  });

  it("saves the edited form through set_capture_config", async () => {
    const { wrapper, calls } = await mountLoaded();
    await wrapper.get('[data-testid="folder-input"]').setValue("Inbox/Audio");
    await wrapper.get('[data-testid="bitrate-select"]').setValue("192");
    await wrapper.get('[data-testid="input-device-select"]').setValue("");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_capture_config");
    expect(set?.args).toEqual({
      id: "v1",
      cfg: {
        mode: "meeting",
        recordingFolder: "Inbox/Audio",
        bitrateKbps: 192,
        createNote: true,
        inputDevice: null,
        outputDevice: null,
      },
    });
    expect(wrapper.text()).toContain("Saved");
  });

  it("shows a folder error inline and keeps the form state", async () => {
    const { wrapper } = await mountLoaded({
      onSet: () => {
        throw "Configured recording folder must stay inside the vault: \"../x\"";
      },
    });
    await wrapper.get('[data-testid="folder-input"]').setValue("../x");
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[data-testid="folder-error"]').text()).toContain(
      "must stay inside the vault",
    );
    const folder = wrapper.get<HTMLInputElement>('[data-testid="folder-input"]');
    expect(folder.element.value).toBe("../x");
  });

  it("shows non-folder save failures as a form error", async () => {
    const { wrapper } = await mountLoaded({
      onSet: () => {
        throw "Could not save capture settings: disk full";
      },
    });
    await wrapper.get("form").trigger("submit");
    await flushPromises();
    expect(wrapper.get('[data-testid="save-error"]').text()).toContain("disk full");
  });
});
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `npx vitest run tests/capture-settings.test.ts`
Expected: FAIL (component does not exist).

- [ ] **Step 4: Implement the component**

Create `src/components/CaptureSettings.vue`:

```vue
<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { AudioDevice, AudioDevices, CaptureConfig } from "../types";

const props = defineProps<{ vaultId: string }>();

const BITRATES = [128, 160, 192];

const loading = ref(true);
const loadError = ref<string | null>(null);
const saveState = ref<"idle" | "saving" | "saved">("idle");
const saveError = ref<string | null>(null);
const folderError = ref<string | null>(null);

const mode = ref<"meeting" | "voice-note">("meeting");
const recordingFolder = ref("");
const createNote = ref(true);
const bitrateKbps = ref(128);
const inputDevice = ref(""); // "" = system default
const outputDevice = ref("");
const devices = ref<AudioDevices>({ inputs: [], outputs: [] });

// A configured device that is not currently connected must stay
// selectable (unplugging a headset must not silently rewrite the
// config) — it is surfaced with a "(not connected)" suffix instead.
function withConfigured(list: AudioDevice[], configured: string) {
  const options = list.map((d) => ({ value: d.name, label: d.name }));
  if (configured && !list.some((d) => d.name === configured)) {
    options.push({ value: configured, label: `${configured} (not connected)` });
  }
  return options;
}
const inputOptions = computed(() =>
  withConfigured(devices.value.inputs, inputDevice.value),
);
const outputOptions = computed(() =>
  withConfigured(devices.value.outputs, outputDevice.value),
);

const folderPlaceholder = computed(() =>
  mode.value === "meeting" ? "Meetings" : "Voice Notes",
);

onMounted(async () => {
  try {
    const [cfg, devs] = await Promise.all([
      invoke<CaptureConfig>("get_capture_config", { id: props.vaultId }),
      invoke<AudioDevices>("list_audio_devices"),
    ]);
    mode.value = cfg.mode;
    recordingFolder.value = cfg.recordingFolder ?? "";
    createNote.value = cfg.createNote;
    bitrateKbps.value = cfg.bitrateKbps;
    inputDevice.value = cfg.inputDevice ?? "";
    outputDevice.value = cfg.outputDevice ?? "";
    devices.value = devs;
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

async function save() {
  saveState.value = "saving";
  saveError.value = null;
  folderError.value = null;
  const folder = recordingFolder.value.trim();
  try {
    await invoke("set_capture_config", {
      id: props.vaultId,
      cfg: {
        mode: mode.value,
        recordingFolder: folder ? folder : null,
        bitrateKbps: bitrateKbps.value,
        createNote: createNote.value,
        inputDevice: inputDevice.value || null,
        outputDevice: outputDevice.value || null,
      },
    });
    saveState.value = "saved";
  } catch (e) {
    saveState.value = "idle";
    // Folder rejections are field-level; everything else is form-level.
    // Form state is preserved either way so the user can correct and retry.
    const message = String(e);
    if (message.toLowerCase().includes("folder")) folderError.value = message;
    else saveError.value = message;
  }
}
</script>

<template>
  <p v-if="loading" class="text-xs text-slate-400">Loading…</p>
  <p v-else-if="loadError" class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200">
    {{ loadError }}
  </p>
  <form v-else class="flex flex-col gap-3" @submit.prevent="save">
    <section>
      <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400">
        Recording mode
      </h2>
      <div class="flex gap-1" role="radiogroup" aria-label="Recording mode">
        <button
          v-for="m in [
            { key: 'meeting', label: 'Meeting' },
            { key: 'voice-note', label: 'Voice Note' },
          ] as const"
          :key="m.key"
          type="button"
          role="radio"
          class="cursor-pointer rounded-lg border px-2 py-0.5 text-xs transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            mode === m.key
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          :aria-checked="mode === m.key"
          :data-testid="`mode-${m.key}`"
          @click="mode = m.key"
        >
          {{ m.label }}
        </button>
      </div>
    </section>
    <section>
      <label class="mb-1 block text-sm text-slate-200" for="capture-folder">
        Recording folder
        <span class="block text-xs text-slate-500">Inside the vault</span>
      </label>
      <input
        id="capture-folder"
        v-model="recordingFolder"
        data-testid="folder-input"
        type="text"
        :placeholder="folderPlaceholder"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      />
      <p
        v-if="folderError"
        data-testid="folder-error"
        class="mt-1 text-xs text-red-300"
      >
        {{ folderError }}
      </p>
    </section>
    <section class="flex items-center justify-between">
      <label for="capture-note-toggle" class="text-sm text-slate-200">
        Companion note
        <span class="block text-xs text-slate-500">.md with metadata + embed</span>
      </label>
      <input
        id="capture-note-toggle"
        v-model="createNote"
        data-testid="note-toggle"
        type="checkbox"
        class="h-4 w-4 accent-violet-500"
      />
    </section>
    <section class="flex items-center justify-between gap-2">
      <label for="capture-bitrate" class="text-sm text-slate-200">Bitrate</label>
      <select
        id="capture-bitrate"
        v-model.number="bitrateKbps"
        data-testid="bitrate-select"
        class="rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none"
      >
        <option v-for="b in BITRATES" :key="b" :value="b">{{ b }} kbps</option>
      </select>
    </section>
    <section>
      <label class="mb-1 block text-sm text-slate-200" for="capture-input-device">
        Microphone
      </label>
      <select
        id="capture-input-device"
        v-model="inputDevice"
        data-testid="input-device-select"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none"
      >
        <option value="">System default</option>
        <option v-for="o in inputOptions" :key="o.value" :value="o.value">
          {{ o.label }}
        </option>
      </select>
    </section>
    <section v-if="mode === 'meeting'">
      <label class="mb-1 block text-sm text-slate-200" for="capture-output-device">
        Desktop audio from
        <span class="block text-xs text-slate-500">Loopback output device</span>
      </label>
      <select
        id="capture-output-device"
        v-model="outputDevice"
        data-testid="output-device-select"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 focus:border-violet-400 focus:outline-none"
      >
        <option value="">System default</option>
        <option v-for="o in outputOptions" :key="o.value" :value="o.value">
          {{ o.label }}
        </option>
      </select>
    </section>
    <p
      v-if="saveError"
      data-testid="save-error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ saveError }}
    </p>
    <div class="flex items-center gap-2">
      <button
        type="submit"
        data-testid="save-button"
        class="cursor-pointer rounded-lg bg-violet-600/80 px-3 py-1 text-xs font-semibold text-white hover:bg-violet-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
        :disabled="saveState === 'saving'"
      >
        {{ saveState === "saving" ? "Saving…" : "Save" }}
      </button>
      <span v-if="saveState === 'saved'" class="text-xs text-emerald-300">
        Saved ✓
      </span>
    </div>
  </form>
</template>
```

Note on the submit trigger in tests: trigger `submit` on the `form` element (the form's `@submit.prevent="save"` handles it); the save button is `type="submit"` for real keyboard/click submission.

- [ ] **Step 5: Wire into ActionPanel**

In `src/components/ActionPanel.vue`:
- `import CaptureSettings from "./CaptureSettings.vue";`
- Title: replace the `<h1>` content with:

```vue
        {{
          view === "settings"
            ? "Buddy settings"
            : view === "captureSettings"
              ? "Capture settings"
              : "Vaults"
        }}
```

- Body: after the settings `<div v-if="view === 'settings'" ...>` block add:

```vue
    <div
      v-else-if="view === 'captureSettings' && store.captureSettingsVaultId"
      class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1"
    >
      <CaptureSettings
        :key="store.captureSettingsVaultId"
        :vault-id="store.captureSettingsVaultId"
      />
    </div>
```

(`:key` forces a fresh load when switching between vaults' gears.)

- [ ] **Step 6: Run tests, build, commit**

Run: `npx vitest run tests/capture-settings.test.ts` then `npm run test && npm run build`
Expected: all green.

```bash
cd /home/user/vault-buddy
git add src/components/CaptureSettings.vue src/components/ActionPanel.vue src/types.ts tests/capture-settings.test.ts
git commit -m "feat(ui): per-vault capture settings view

Gear view bound to get/set_capture_config and list_audio_devices: mode,
folder (validated Rust-side; rejection surfaces as an inline field
error with form state preserved), note toggle, 128/160/192 bitrate,
and device pickers where System default is always first and a
configured-but-absent device stays selectable as '(not connected)'.
The loopback output picker only exists in Meeting mode."
```

---

### Task 14: Frontend — vault row gear + recording indicator

**Files:**
- Modify: `src/components/VaultList.vue`
- Modify: `src/components/ActionPanel.vue`
- Modify: `tests/vault-list.test.ts`, `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `openCaptureSettings` (Task 12), `capture.vaultId` (Task 11).
- Produces: `VaultList` props gain `recordingVaultId: string | null`; new emit `(e: "capture-settings", id: string)`; the recording vault's row shows a pulsing red dot (`title="Recording…"`).

- [ ] **Step 1: Write the failing tests**

1a. In `tests/vault-list.test.ts`, extend the mount helper:

```ts
const mountList = (
  vaults: Array<{ id: string; name: string; path: string; open: boolean }>,
  busyVaultId: string | null = null,
  busyCommand: Busy = null,
  captureDisabled = false,
  recordingVaultId: string | null = null,
) =>
  mount(VaultList, {
    props: { vaults, busyVaultId, busyCommand, captureDisabled, recordingVaultId },
  });
```

Update the button-count test: `expect(buttons.length).toBe(6);` → `expect(buttons.length).toBe(8);` (a gear per row).

Append:

```ts
  it("emits capture-settings with the vault id from the gear", async () => {
    const wrapper = mountList(sample);
    await wrapper
      .find('[aria-label="Capture settings for Work"]')
      .trigger("click");
    expect(wrapper.emitted("capture-settings")).toEqual([["bbb222"]]);
  });

  it("marks the recording vault's row with a red dot", () => {
    const wrapper = mountList(sample, null, null, true, "bbb222");
    const dots = wrapper.findAll('[title="Recording…"]');
    expect(dots).toHaveLength(1);
    // the dot sits on the Work row
    const workRow = wrapper.findAll("li").find((li) => li.text().includes("Work"))!;
    expect(workRow.find('[title="Recording…"]').exists()).toBe(true);
  });

  it("shows no recording dot when nothing records", () => {
    const wrapper = mountList(sample);
    expect(wrapper.find('[title="Recording…"]').exists()).toBe(false);
  });
```

1b. In `tests/action-panel.test.ts`, append (matching the file's existing mount/store setup style):

```ts
  it("opens capture settings when a vault gear is clicked", async () => {
    const { wrapper, store } = await mountPanel(); // adapt to this file's helper
    await wrapper
      .find('[aria-label^="Capture settings for"]')
      .trigger("click");
    expect(store.view).toBe("captureSettings");
    expect(store.captureSettingsVaultId).not.toBeNull();
  });
```

(Read the top of `tests/action-panel.test.ts` first and reuse its actual mount helper and store access — the file already mounts ActionPanel with mocked IPC returning sample vaults.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/vault-list.test.ts tests/action-panel.test.ts`
Expected: FAIL (missing prop/emit/gear button).

- [ ] **Step 3: Implement VaultList**

In `src/components/VaultList.vue`:

Props/emits:

```ts
const props = defineProps<{
  vaults: Vault[];
  busyVaultId: string | null;
  busyCommand: "open_vault" | "open_daily_note" | null;
  captureDisabled: boolean;
  recordingVaultId: string | null;
}>();
defineEmits<{
  (e: "open-vault", id: string): void;
  (e: "open-daily-note", id: string): void;
  (e: "capture", id: string): void;
  (e: "capture-settings", id: string): void;
}>();
```

Template: next to the open-in-Obsidian dot (inside the name row's flex span, after the `vault.open` dot), add:

```vue
              <span
                v-if="vault.id === recordingVaultId"
                class="h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-red-500"
                title="Recording…"
                aria-hidden="true"
              ></span>
```

After the capture (mic) button, add the gear button:

```vue
        <button
          type="button"
          class="mr-1 shrink-0 cursor-pointer rounded-lg p-1.5 text-slate-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null"
          :aria-label="`Capture settings for ${accessibleName(vault)}`"
          title="Capture settings"
          @click="$emit('capture-settings', vault.id)"
        >
          <svg
            width="16"
            height="16"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <circle cx="12" cy="12" r="3" />
            <path
              d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.09a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51h.09a1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.09a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
            />
          </svg>
        </button>
```

- [ ] **Step 4: Wire in ActionPanel**

In `src/components/ActionPanel.vue`'s `<VaultList ...>` usage, add:

```vue
        :recording-vault-id="capture.vaultId"
        @capture-settings="store.openCaptureSettings($event)"
```

- [ ] **Step 5: Run tests, build, commit**

Run: `npm run test && npm run build`
Expected: all green.

```bash
cd /home/user/vault-buddy
git add src/components/VaultList.vue src/components/ActionPanel.vue tests/vault-list.test.ts tests/action-panel.test.ts
git commit -m "feat(ui): vault row gear and per-vault recording indicator

Each vault row gains a gear that opens that vault's capture settings
view, and the recording vault's row shows a pulsing red dot driven by
the vaultId the backend reports (survives webview reloads via the
capture_status resync)."
```

---

### Task 15: Frontend — RecordingBar pause/meter/frozen elapsed + paused buddy

**Files:**
- Modify: `src/components/RecordingBar.vue`
- Modify: `src/components/CompanionCharacter.vue`
- Modify: `src/App.vue`
- Modify: `src/components/ActionPanel.vue`
- Modify: `tests/recording-bar.test.ts`, `tests/companion-character.test.ts`

**Interfaces:**
- Consumes: store fields `paused`, `pausedTotalMs`, `pausedSinceMs`, `level` (Task 11).
- Produces: `RecordingBar` props gain `paused: boolean`, `pausedTotalMs: number`, `pausedSinceMs: number | null`, `level: number`; emits gain `pause` / `resume`. `CompanionCharacter` prop gains `paused?: boolean` (steady amber dot instead of blinking red).

Elapsed contract: `elapsed = now − startedAtMs − pausedTotalMs − (paused ? now − pausedSinceMs : 0)` — freezes while paused, excludes all prior pauses.

- [ ] **Step 1: Write the failing tests**

1a. Append to `tests/recording-bar.test.ts` (extend every existing `props:` object in this file with `paused: false, pausedTotalMs: 0, pausedSinceMs: null, level: 0` so it keeps compiling):

```ts
  it("freezes elapsed while paused and excludes prior pauses", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(200_000));
    const wrapper = mount(RecordingBar, {
      props: {
        // started 100s ago, 20s of past pauses, paused again 10s ago:
        // elapsed = 100 - 20 - 10 = 70s
        startedAtMs: 100_000,
        saving: false,
        starting: false,
        warning: null,
        paused: true,
        pausedTotalMs: 20_000,
        pausedSinceMs: 190_000,
        level: 0,
      },
    });
    expect(wrapper.text()).toContain("Paused 1:10");
  });

  it("emits pause while recording and resume while paused", async () => {
    const base = {
      startedAtMs: Date.now(),
      saving: false,
      starting: false,
      warning: null,
      pausedTotalMs: 0,
      pausedSinceMs: null,
      level: 0,
    };
    const recording = mount(RecordingBar, { props: { ...base, paused: false } });
    await recording.get("button[aria-label='Pause recording']").trigger("click");
    expect(recording.emitted("pause")).toHaveLength(1);
    expect(recording.find("button[aria-label='Resume recording']").exists()).toBe(false);
    const paused = mount(RecordingBar, { props: { ...base, paused: true } });
    await paused.get("button[aria-label='Resume recording']").trigger("click");
    expect(paused.emitted("resume")).toHaveLength(1);
  });

  it("disables pause while starting or saving", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: null,
        saving: false,
        starting: true,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0,
      },
    });
    expect(
      wrapper.get("button[aria-label='Pause recording']").attributes("disabled"),
    ).toBeDefined();
  });

  it("renders the level meter width from the level prop", () => {
    const wrapper = mount(RecordingBar, {
      props: {
        startedAtMs: Date.now(),
        saving: false,
        starting: false,
        warning: null,
        paused: false,
        pausedTotalMs: 0,
        pausedSinceMs: null,
        level: 0.4,
      },
    });
    const meter = wrapper.get('[data-testid="level-meter"]');
    expect(meter.attributes("style")).toContain("width: 40%");
  });
```

1b. Append to `tests/companion-character.test.ts` (match the file's existing mount style — read its helper first):

```ts
  it("shows a steady amber dot while paused", () => {
    const wrapper = mount(CompanionCharacter, {
      props: { working: false, recording: true, paused: true },
    });
    const dot = wrapper.get(".rec-dot");
    expect(dot.classes()).toContain("bg-amber-400");
    expect(dot.classes()).not.toContain("bg-red-500");
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/recording-bar.test.ts tests/companion-character.test.ts`
Expected: FAIL (missing props/buttons/meter).

- [ ] **Step 3: Implement RecordingBar**

Replace `src/components/RecordingBar.vue` with:

```vue
<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{
  startedAtMs: number | null;
  saving: boolean;
  starting: boolean;
  warning: string | null;
  paused: boolean;
  pausedTotalMs: number;
  pausedSinceMs: number | null;
  level: number;
}>();
defineEmits<{ (e: "stop"): void; (e: "pause"): void; (e: "resume"): void }>();

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;
onMounted(() => {
  timer = setInterval(() => (now.value = Date.now()), 1000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

// Wall time minus accumulated pauses (and the still-open span while
// paused) — the display freezes during a pause and never counts the gap.
const elapsed = computed(() => {
  if (props.startedAtMs === null) return "0:00";
  const openPause =
    props.paused && props.pausedSinceMs !== null
      ? now.value - props.pausedSinceMs
      : 0;
  const total = Math.max(
    0,
    Math.floor(
      (now.value - props.startedAtMs - props.pausedTotalMs - openPause) / 1000,
    ),
  );
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${m}:${String(s).padStart(2, "0")}`;
});

const label = computed(() => {
  if (props.starting) return "Starting…";
  if (props.saving) return "Saving…";
  if (props.paused) return `Paused ${elapsed.value}`;
  return `Recording ${elapsed.value}`;
});

const meterWidth = computed(
  () => `${Math.round(Math.min(1, Math.max(0, props.level)) * 100)}%`,
);
</script>

<template>
  <div
    class="rounded-lg px-2 py-1.5"
    :class="paused ? 'bg-amber-500/15' : 'bg-red-500/15'"
  >
    <div class="flex items-center gap-2">
      <span
        class="h-2.5 w-2.5 shrink-0 rounded-full"
        :class="paused ? 'bg-amber-400' : 'animate-pulse bg-red-500'"
        aria-hidden="true"
      ></span>
      <span
        class="flex-1 text-sm font-medium"
        :class="paused ? 'text-amber-100' : 'text-red-100'"
        role="status"
      >
        {{ label }}
      </span>
      <button
        v-if="!paused"
        type="button"
        class="cursor-pointer rounded-lg bg-white/10 px-2 py-1 text-xs font-semibold text-white hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Pause recording"
        :disabled="saving || starting"
        @click="$emit('pause')"
      >
        ⏸ Pause
      </button>
      <button
        v-else
        type="button"
        class="cursor-pointer rounded-lg bg-white/10 px-2 py-1 text-xs font-semibold text-white hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Resume recording"
        :disabled="saving || starting"
        @click="$emit('resume')"
      >
        ▶ Resume
      </button>
      <button
        type="button"
        class="cursor-pointer rounded-lg bg-red-500/80 px-2 py-1 text-xs font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Stop recording"
        :disabled="saving || starting"
        @click="$emit('stop')"
      >
        ⏹ Stop
      </button>
    </div>
    <div
      v-if="!starting"
      class="mt-1.5 h-1 overflow-hidden rounded-full bg-white/10"
      aria-hidden="true"
    >
      <div
        data-testid="level-meter"
        class="h-full rounded-full bg-emerald-400 transition-[width] duration-100"
        :style="{ width: meterWidth }"
      ></div>
    </div>
    <p v-if="warning" class="mt-1 text-xs text-amber-200">{{ warning }}</p>
  </div>
</template>
```

- [ ] **Step 4: Implement paused buddy dot**

In `src/components/CompanionCharacter.vue`:
- Add `paused?: boolean;` to the props interface and `paused: false,` to the defaults.
- The rec-dot span becomes:

```vue
        <span
          v-if="recording"
          class="rec-dot absolute -right-1 -top-1 h-3 w-3 rounded-full ring-2 ring-slate-900"
          :class="paused ? 'bg-amber-400' : 'bg-red-500'"
          aria-hidden="true"
        ></span>
```

- Add `paused` to the button's class bindings object: `{ working, still: !animated, recording, paused }`.
- In the scoped CSS, keep the blink rule and stop it while paused (steady amber):

```css
.buddy.recording.paused .rec-dot {
  animation: none;
}
```

In `src/App.vue`, add to the `<CompanionCharacter ...>` bindings:

```vue
        :paused="capture.paused"
```

- [ ] **Step 5: Wire in ActionPanel**

The `<RecordingBar ...>` usage gains:

```vue
      :paused="capture.paused"
      :paused-total-ms="capture.pausedTotalMs"
      :paused-since-ms="capture.pausedSinceMs"
      :level="capture.level"
      @pause="capture.pause()"
      @resume="capture.resume()"
```

- [ ] **Step 6: Run tests, build, commit**

Run: `npm run test && npm run build`
Expected: all green (including the extended existing RecordingBar tests).

```bash
cd /home/user/vault-buddy
git add src/components/RecordingBar.vue src/components/CompanionCharacter.vue src/App.vue src/components/ActionPanel.vue tests/recording-bar.test.ts tests/companion-character.test.ts
git commit -m "feat(ui): recording bar pause/resume, level meter, frozen elapsed

Pause <-> Resume button mirrored from store state, an advisory level
meter driven by capture:level, and an elapsed display computed as wall
time minus accumulated pauses (frozen mid-pause). Paused state reads as
steady amber on the bar and on the buddy's dot — visually distinct from
live recording at a glance."
```

---

### Task 16: Frontend — post-save rename prompt

**Files:**
- Create: `src/components/RenamePrompt.vue`
- Modify: `src/components/ActionPanel.vue`
- Test: `tests/rename-prompt.test.ts` (new), `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `capture.lastSaved`, `capture.renameError`, `capture.rename(title)`, `capture.dismissRename()` (Task 11).
- Produces: `RenamePrompt.vue` with props `savedMp3: string`, `error: string | null`; emits `(e: "rename", title: string)`, `(e: "dismiss")`. Prefilled with the saved base name (file name without `.mp3` — `rename_plan` strips the doubled prefix server-side, so confirming unedited is a safe no-op).

- [ ] **Step 1: Write the failing tests**

Create `tests/rename-prompt.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import RenamePrompt from "../src/components/RenamePrompt.vue";

describe("RenamePrompt", () => {
  it("prefills the input with the saved base name", () => {
    const wrapper = mount(RenamePrompt, {
      props: {
        savedMp3: "C:\\v\\Meetings\\2026\\07\\2026-07-04 1405 Meeting.mp3",
        error: null,
      },
    });
    const input = wrapper.get<HTMLInputElement>("input");
    expect(input.element.value).toBe("2026-07-04 1405 Meeting");
  });

  it("emits rename with the edited title on submit", async () => {
    const wrapper = mount(RenamePrompt, {
      props: { savedMp3: "/v/2026-07-04 1405 Meeting.mp3", error: null },
    });
    await wrapper.get("input").setValue("2026-07-04 1405 Standup");
    await wrapper.get("form").trigger("submit");
    expect(wrapper.emitted("rename")).toEqual([["2026-07-04 1405 Standup"]]);
  });

  it("emits dismiss from the keep-name button", async () => {
    const wrapper = mount(RenamePrompt, {
      props: { savedMp3: "/v/2026-07-04 1405 Meeting.mp3", error: null },
    });
    await wrapper
      .get("button[aria-label='Keep the timestamp name']")
      .trigger("click");
    expect(wrapper.emitted("dismiss")).toHaveLength(1);
  });

  it("shows a rename error", () => {
    const wrapper = mount(RenamePrompt, {
      props: {
        savedMp3: "/v/2026-07-04 1405 Meeting.mp3",
        error: "Title is too long (max 120 characters)",
      },
    });
    expect(wrapper.text()).toContain("Title is too long");
  });
});
```

And append to `tests/action-panel.test.ts` (again matching that file's existing helper):

```ts
  it("shows the rename prompt after a save and hides it on dismiss", async () => {
    const { wrapper, capture } = await mountPanel(); // adapt to the helper
    capture.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("Name this recording");
    capture.lastSaved = null;
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).not.toContain("Name this recording");
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run tests/rename-prompt.test.ts`
Expected: FAIL (component does not exist).

- [ ] **Step 3: Implement**

Create `src/components/RenamePrompt.vue`:

```vue
<script setup lang="ts">
import { computed, ref, watch } from "vue";

const props = defineProps<{ savedMp3: string; error: string | null }>();
const emit = defineEmits<{
  (e: "rename", title: string): void;
  (e: "dismiss"): void;
}>();

// Prefill with the saved base (file name without .mp3) so confirming
// unedited is a no-op and edits can start from the real name. The
// backend strips the duplicated timestamp prefix, so editing the tail
// of the full base is safe too.
const baseName = computed(() => {
  const name = props.savedMp3.split(/[\\/]/).pop() ?? "";
  return name.replace(/\.mp3$/i, "");
});
const title = ref(baseName.value);
watch(baseName, (value) => (title.value = value));
</script>

<template>
  <form
    class="rounded-lg bg-emerald-500/10 px-2 py-1.5"
    @submit.prevent="emit('rename', title)"
  >
    <label class="text-xs font-medium text-emerald-200" for="rename-input">
      Saved ✓ — name this recording?
    </label>
    <div class="mt-1 flex items-center gap-1">
      <input
        id="rename-input"
        v-model="title"
        type="text"
        aria-label="Recording name"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
      />
      <button
        type="submit"
        class="cursor-pointer rounded-lg bg-emerald-600/80 px-2 py-1 text-xs font-semibold text-white hover:bg-emerald-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
      >
        Rename
      </button>
      <button
        type="button"
        class="cursor-pointer rounded-lg p-1 text-slate-400 hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
        aria-label="Keep the timestamp name"
        title="Keep the timestamp name"
        @click="emit('dismiss')"
      >
        ✕
      </button>
    </div>
    <p v-if="error" class="mt-1 text-xs text-red-300">{{ error }}</p>
  </form>
</template>
```

- [ ] **Step 4: Wire into ActionPanel**

In `src/components/ActionPanel.vue`:
- `import RenamePrompt from "./RenamePrompt.vue";`
- After the capture.error `<p>` block (still above the body `<div>`s), add:

```vue
    <RenamePrompt
      v-if="view === 'list' && capture.lastSaved"
      class="mb-2"
      :saved-mp3="capture.lastSaved.mp3"
      :error="capture.renameError"
      @rename="capture.rename($event)"
      @dismiss="capture.dismissRename()"
    />
```

- Panel close must not leave a stale prompt for the next open (spec: "the prompt dismisses on new recording or panel close with no dangling state"). Add to the script:

```ts
import { onUnmounted } from "vue";
// The panel component is destroyed on close — that IS the close signal.
onUnmounted(() => capture.dismissRename());
```

(Merge the `onUnmounted` import into the existing `vue` import statement.)

- [ ] **Step 5: Run tests, build, commit**

Run: `npm run test && npm run build`
Expected: all green.

```bash
cd /home/user/vault-buddy
git add src/components/RenamePrompt.vue src/components/ActionPanel.vue tests/rename-prompt.test.ts tests/action-panel.test.ts
git commit -m "feat(ui): post-save rename prompt

One-line prompt beside the saved confirmation, prefilled with the
timestamp base; confirming calls rename_capture (failure keeps the
prompt up with the error for a retry), the X keeps timestamp names, and
panel close dismisses via the component teardown so no stale prompt can
greet the next open. The store's 30s expiry covers the walk-away case."
```

---

### Task 17: Docs + final verification

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/DEVELOPMENT.md`

- [ ] **Step 1: Update AGENTS.md**

In the **IPC surface** paragraph, replace the command list sentence with:

```
`list_vaults`, `open_vault`, `open_daily_note`, `prepare_update_install`,
`set_panel_offset`, `set_window_geometry`, `show_buddy_menu`, plus the
capture surface: `capture_status`, `start_capture`, `stop_capture`,
`pause_capture`, `resume_capture`, `get_capture_config`,
`set_capture_config`, `list_audio_devices`, `rename_capture` — commands
live in `src-tauri/src/commands.rs` and `src-tauri/src/capture_commands.rs`.
```

In the **capture domain** section, append two invariant bullets:

```
- **Pause is a session Control message** (`Control { Stop, Pause, Resume }`
  on the one channel the shell's device thread forwards): streams stay
  open, drained samples are discarded, nothing is encoded, the fsync
  cadence keeps running — and pause can never block shutdown
  (stop-while-paused finalizes normally). Level metering (`capture:level`,
  ~5 Hz, 0–1) is advisory and lossy by design.
- **Rename never breaks the capture contract**: `rename_plan` (core)
  keeps the `YYYY-MM-DD HHmm ` prefix and refuses non-capture files;
  execution reuses the reservation + `rename_noreplace` + suffix-retry
  loop, retargets exactly the note's embed line, and a note-side failure
  after a successful audio move degrades to a warning (audio first).
  Config writes stay app-side: owned temp + REPLACING rename is correct
  for `config.json` only, serialized behind `ConfigWriteLock`.
```

In the **frontend state** paragraph, update the vaults-store sentence: panel view state is now `view: list | settings | captureSettings` (+ `captureSettingsVaultId`), and the capture store carries `paused/pausedTotalMs/level/vaultId/lastSaved` mirrored from Rust events.

- [ ] **Step 2: Update docs/DEVELOPMENT.md**

Extend the config.json example (Capture configuration section):

```json
{
  "vaults": {
    "<vault-id>": {
      "mode": "meeting",          // "meeting" (mic + desktop audio) | "voice-note" (mic only)
      "recordingFolder": "Meetings", // optional — omit for the mode default ("Meetings" / "Voice Notes")
      "bitrateKbps": 128,          // 128 | 160 | 192
      "createNote": true,          // companion .md with metadata + embed
      "inputDevice": "USB Mic",    // optional — cpal device name; omit for system default
      "outputDevice": "Speakers"   // optional — loopback source (Meeting mode); omit for system default
    }
  }
}
```

Below the example, note: the file is written by the panel's per-vault ⚙ form (atomic temp + rename); it stays hand-editable and malformed fields still degrade per-field to defaults; a configured device that is missing at record time falls back to the system default with a warning.

- [ ] **Step 3: Final whole-branch verification**

```bash
cd /home/user/vault-buddy/src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
npm run test && npm run build
```

Expected: everything green (Rust ≈ 86 + ~20 new tests, Vitest ≈ 136 + ~25 new).

- [ ] **Step 4: Commit**

```bash
cd /home/user/vault-buddy
git add AGENTS.md docs/DEVELOPMENT.md
git commit -m "docs: increment 3 — IPC surface, capture invariants, config schema

AGENTS.md gains the six new capture commands and the pause/rename
invariants (Control channel semantics, rename safety rails, app-side
config write rules); DEVELOPMENT.md documents the inputDevice/
outputDevice config fields and the settings-UI write path."
```

---

## Windows manual checklist (post-merge, from the spec — not automatable here)

Real device pick honored; stale-device fallback warning; pause gap absent from playback with frozen elapsed and amber dot (bar, buddy, tray); rename produces correct files + working Obsidian embed; per-vault dot on the correct row; meter tracks speech and sits near zero on silence.
