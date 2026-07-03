# Increment 1: Companion Opens Daily Note — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Tauri 2 + Vue 3 desktop app: an animated companion in a transparent always-on-top window that discovers Obsidian vaults (from `obsidian.json`) and opens a vault or today's daily note via `obsidian://` URIs.

**Architecture:** All Obsidian logic (config parsing, daily-note path resolution, URI building) lives in a pure Rust crate `vault_buddy_core` at `src-tauri/core/` with zero Tauri dependencies, so it compiles and tests anywhere. The Tauri app crate at `src-tauri/` is a thin shell: three `#[tauri::command]` wrappers, a tray icon, and window config. The Vue frontend holds one Pinia store and three components; clicking the character resizes the single transparent window to reveal an action panel.

**Tech Stack:** Tauri 2, Vue 3.5, TypeScript (strict), Pinia 3, TailwindCSS 4 (via `@tailwindcss/vite`), Vitest 3 + happy-dom + `@tauri-apps/api/mocks`, Rust (edition 2021) with serde/chrono/percent-encoding/dirs/open crates.

**Spec:** `docs/superpowers/specs/2026-07-03-increment-1-companion-daily-note-design.md`

## Global Constraints

- Target platform: Windows (code must still compile and unit-test on Linux/macOS for the core crate and frontend).
- **This dev environment is Linux without webkit2gtk** — the Tauri app crate (`src-tauri/`, package `vault-buddy`) CANNOT compile here. Never run bare `cargo test`/`cargo check`/`cargo build` from `src-tauri/`; always scope to the core crate: run from `src-tauri/core/`. The app crate is compile-verified on the user's Windows machine.
- Vault Buddy never writes into a vault; all note creation is delegated to Obsidian via `obsidian://new`.
- Every launched URI is logged via `log::info!`.
- All parsing failures degrade gracefully (empty vault list / default daily-note settings); no panics on bad input.
- Supported daily-note format tokens: `YYYY`, `MM`, `DD` only. Every run of consecutive letters in the format must be exactly one supported token; any other run (e.g. `MMMM`, `dddd`, `YYYYMMDD`) means an unsupported moment format — fall back to `YYYY-MM-DD` (never point Obsidian at a wrong path). Naive string replacement is forbidden: `MMMM` must NOT be consumed as two `MM`s.
- `obsidian://` URIs address vaults by their Obsidian vault **ID** (the key in `obsidian.json`), never by name — two vaults can share a folder name; the ID is unique.
- Frontend copy for the empty state: "Obsidian not found — no vaults discovered. Is Obsidian installed and has it been opened at least once?"
- Window sizes: collapsed 140×170 logical px, expanded 440×340 logical px.
- Commit after every task with a descriptive message.

---

### Task 1: Core crate scaffold + vault discovery parsing

**Files:**
- Create: `src-tauri/core/Cargo.toml`
- Create: `src-tauri/core/src/lib.rs`
- Create: `src-tauri/core/src/discovery.rs`
- Create: `.gitignore`
- Test: inline `#[cfg(test)]` module in `src-tauri/core/src/discovery.rs`

**Interfaces:**
- Consumes: nothing (first task)
- Produces:
  - `discovery::Vault { id: String, name: String, path: String }` (derives `Debug, Clone, PartialEq, serde::Serialize`)
  - `discovery::parse_obsidian_config(json: &str) -> Vec<Vault>`
  - `discovery::obsidian_config_path() -> Option<PathBuf>`
  - `discovery::discover_vaults() -> Vec<Vault>`
  - `discovery::discover_vaults_from(config_path: &Path) -> Vec<Vault>`

- [ ] **Step 1: Create the crate scaffold**

`src-tauri/core/Cargo.toml`:

```toml
[package]
name = "vault_buddy_core"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", default-features = false, features = ["clock"] }
percent-encoding = "2"
dirs = "6"
open = "5"
log = "0.4"

[dev-dependencies]
tempfile = "3"
```

`src-tauri/core/src/lib.rs`:

```rust
pub mod discovery;
```

`.gitignore` (repo root):

```gitignore
node_modules/
dist/
src-tauri/target/
src-tauri/core/target/
src-tauri/gen/
app-icon.png
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/core/src/discovery.rs` with an empty implementation surface and the tests (the functions don't exist yet, so this fails to compile — that is the "red" state):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
        "vaults": {
            "a1b2c3": { "path": "C:\\Users\\luis\\vaults\\Work", "ts": 1700000000000, "open": true },
            "d4e5f6": { "path": "C:\\Users\\luis\\vaults\\Personal", "ts": 1700000000001 }
        }
    }"#;

    #[test]
    fn parses_vaults_sorted_by_name() {
        let vaults = parse_obsidian_config(SAMPLE);
        assert_eq!(
            vaults,
            vec![
                Vault {
                    id: "d4e5f6".into(),
                    name: "Personal".into(),
                    path: "C:\\Users\\luis\\vaults\\Personal".into()
                },
                Vault {
                    id: "a1b2c3".into(),
                    name: "Work".into(),
                    path: "C:\\Users\\luis\\vaults\\Work".into()
                },
            ]
        );
    }

    #[test]
    fn vault_name_comes_from_last_path_component_unix_too() {
        let vaults = parse_obsidian_config(
            r#"{ "vaults": { "x": { "path": "/home/luis/vaults/Notes" } } }"#,
        );
        assert_eq!(vaults[0].name, "Notes");
    }

    #[test]
    fn malformed_json_yields_empty() {
        assert_eq!(parse_obsidian_config("not json {"), Vec::new());
    }

    #[test]
    fn missing_vaults_key_yields_empty() {
        assert_eq!(parse_obsidian_config(r#"{ "other": 1 }"#), Vec::new());
    }

    #[test]
    fn entry_without_path_is_skipped() {
        let vaults = parse_obsidian_config(
            r#"{ "vaults": { "bad": { "ts": 1 }, "good": { "path": "/v/Ok" } } }"#,
        );
        assert_eq!(vaults.len(), 1);
        assert_eq!(vaults[0].name, "Ok");
    }

    #[test]
    fn missing_config_file_yields_empty() {
        let dir = tempfile::tempdir().unwrap();
        let vaults = discover_vaults_from(&dir.path().join("nope.json"));
        assert_eq!(vaults, Vec::new());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test`
Expected: compile error — `parse_obsidian_config` and `Vault` not found.

- [ ] **Step 4: Write the implementation**

Add above the `tests` module in `src-tauri/core/src/discovery.rs`:

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Vault {
    pub id: String,
    pub name: String,
    pub path: String,
}

/// Parses the content of Obsidian's own `obsidian.json` vault registry.
/// Any malformed input degrades to an empty list — never an error.
pub fn parse_obsidian_config(json: &str) -> Vec<Vault> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    let Some(vaults) = value.get("vaults").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut result: Vec<Vault> = vaults
        .iter()
        .filter_map(|(id, entry)| {
            let path = entry.get("path")?.as_str()?;
            let name = vault_name_from_path(path)?;
            Some(Vault {
                id: id.clone(),
                name,
                path: path.to_string(),
            })
        })
        .collect();
    result.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    result
}

/// Last path component. Splits on both `/` and `\` because obsidian.json
/// on Windows stores backslash paths and this code also runs in tests on Unix.
fn vault_name_from_path(path: &str) -> Option<String> {
    path.rsplit(['/', '\\'])
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// `%APPDATA%\obsidian\obsidian.json` on Windows (`~/.config/obsidian/...` on Unix).
pub fn obsidian_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("obsidian").join("obsidian.json"))
}

pub fn discover_vaults() -> Vec<Vault> {
    let Some(path) = obsidian_config_path() else {
        return Vec::new();
    };
    discover_vaults_from(&path)
}

pub fn discover_vaults_from(config_path: &Path) -> Vec<Vault> {
    match std::fs::read_to_string(config_path) {
        Ok(json) => parse_obsidian_config(&json),
        Err(_) => Vec::new(),
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: `test result: ok. 6 passed`

- [ ] **Step 6: Commit**

```bash
git add .gitignore src-tauri/core
git commit -m "feat(core): vault discovery from obsidian.json"
```

---

### Task 2: Daily-note settings and date-format rendering

**Files:**
- Create: `src-tauri/core/src/daily_notes.rs`
- Modify: `src-tauri/core/src/lib.rs`
- Test: inline `#[cfg(test)]` module in `src-tauri/core/src/daily_notes.rs`

**Interfaces:**
- Consumes: nothing from Task 1 (independent module)
- Produces:
  - `daily_notes::DEFAULT_FORMAT: &str` (= `"YYYY-MM-DD"`)
  - `daily_notes::DailyNoteSettings { folder: String, format: String }` (derives `Debug, Clone, PartialEq`; implements `Default`)
  - `daily_notes::parse_settings(json: &str) -> DailyNoteSettings`
  - `daily_notes::load_settings(vault_path: &Path) -> DailyNoteSettings`
  - `daily_notes::render_format(format: &str, date: chrono::NaiveDate) -> String`
  - `daily_notes::daily_note_rel_path(settings: &DailyNoteSettings, date: chrono::NaiveDate) -> String` — vault-relative, **no** `.md` extension (the form `obsidian://` URIs expect)

- [ ] **Step 1: Register the module**

In `src-tauri/core/src/lib.rs`:

```rust
pub mod daily_notes;
pub mod discovery;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/core/src/daily_notes.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()
    }

    #[test]
    fn renders_default_format() {
        assert_eq!(render_format("YYYY-MM-DD", date()), "2026-07-03");
    }

    #[test]
    fn renders_format_with_subfolders() {
        assert_eq!(render_format("YYYY/MM/YYYY-MM-DD", date()), "2026/07/2026-07-03");
    }

    #[test]
    fn unsupported_tokens_fall_back_to_default() {
        // "dddd" (weekday name) is a moment token we don't support; a wrong
        // literal path would make Obsidian create a misnamed note.
        assert_eq!(render_format("YYYY-MM-DD dddd", date()), "2026-07-03");
    }

    #[test]
    fn repeated_token_runs_fall_back_instead_of_double_substituting() {
        // "MMMM" is moment's full month name. Consuming it as two "MM"s
        // would render "2026-0707-03" — a misnamed note that dodges the
        // fallback check. Letter runs must match a supported token exactly.
        assert_eq!(render_format("YYYY-MMMM-DD", date()), "2026-07-03");
        assert_eq!(render_format("YYYYMMDD", date()), "2026-07-03");
    }

    #[test]
    fn parses_settings() {
        let s = parse_settings(r#"{ "folder": "Daily Notes", "format": "YYYY-MM-DD" }"#);
        assert_eq!(s.folder, "Daily Notes");
        assert_eq!(s.format, "YYYY-MM-DD");
    }

    #[test]
    fn trims_slashes_from_folder() {
        let s = parse_settings(r#"{ "folder": "/Daily/" }"#);
        assert_eq!(s.folder, "Daily");
    }

    #[test]
    fn malformed_settings_fall_back_to_default() {
        assert_eq!(parse_settings("garbage"), DailyNoteSettings::default());
        assert_eq!(parse_settings(r#"{ "format": "" }"#), DailyNoteSettings::default());
    }

    #[test]
    fn rel_path_joins_folder_and_name() {
        let s = DailyNoteSettings { folder: "Daily".into(), format: "YYYY-MM-DD".into() };
        assert_eq!(daily_note_rel_path(&s, date()), "Daily/2026-07-03");
    }

    #[test]
    fn rel_path_without_folder_is_just_the_name() {
        assert_eq!(daily_note_rel_path(&DailyNoteSettings::default(), date()), "2026-07-03");
    }

    #[test]
    fn missing_settings_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load_settings(dir.path()), DailyNoteSettings::default());
    }

    #[test]
    fn loads_settings_from_vault() {
        let dir = tempfile::tempdir().unwrap();
        let obsidian_dir = dir.path().join(".obsidian");
        std::fs::create_dir_all(&obsidian_dir).unwrap();
        std::fs::write(
            obsidian_dir.join("daily-notes.json"),
            r#"{ "folder": "Journal", "format": "YYYY/MM-DD" }"#,
        )
        .unwrap();
        let s = load_settings(dir.path());
        assert_eq!(s.folder, "Journal");
        assert_eq!(s.format, "YYYY/MM-DD");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test daily_notes`
Expected: compile error — `render_format`, `parse_settings`, `DailyNoteSettings` not found.

- [ ] **Step 4: Write the implementation**

Add above the `tests` module in `src-tauri/core/src/daily_notes.rs`:

```rust
use chrono::NaiveDate;
use std::path::Path;

pub const DEFAULT_FORMAT: &str = "YYYY-MM-DD";

#[derive(Debug, Clone, PartialEq)]
pub struct DailyNoteSettings {
    pub folder: String,
    pub format: String,
}

impl Default for DailyNoteSettings {
    fn default() -> Self {
        Self {
            folder: String::new(),
            format: DEFAULT_FORMAT.to_string(),
        }
    }
}

/// Parses the content of a vault's `.obsidian/daily-notes.json`.
/// Any malformed input degrades to the defaults.
pub fn parse_settings(json: &str) -> DailyNoteSettings {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return DailyNoteSettings::default();
    };
    let folder = value
        .get("folder")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_matches('/')
        .to_string();
    let format = value
        .get("format")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_FORMAT)
        .to_string();
    DailyNoteSettings { folder, format }
}

pub fn load_settings(vault_path: &Path) -> DailyNoteSettings {
    let path = vault_path.join(".obsidian").join("daily-notes.json");
    match std::fs::read_to_string(path) {
        Ok(json) => parse_settings(&json),
        Err(_) => DailyNoteSettings::default(),
    }
}

/// Renders `format` for `date`, substituting the supported moment tokens
/// `YYYY`, `MM`, `DD`. Every run of consecutive letters must be exactly one
/// supported token; any other run (`dddd`, `MMMM`, `YYYYMMDD`, …) means the
/// format uses moment features we don't support — fall back to the default
/// format rather than risk telling Obsidian to create a misnamed note.
/// (Naive `str::replace` would consume `MMMM` as two `MM`s: "2026-0707-03".)
pub fn render_format(format: &str, date: NaiveDate) -> String {
    substitute_tokens(format, date).unwrap_or_else(|| {
        substitute_tokens(DEFAULT_FORMAT, date).expect("default format is valid")
    })
}

/// `None` when the format contains a letter run that is not exactly one
/// supported token.
fn substitute_tokens(format: &str, date: NaiveDate) -> Option<String> {
    let chars: Vec<char> = format.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_alphabetic() {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                i += 1;
            }
            let run: String = chars[start..i].iter().collect();
            match run.as_str() {
                "YYYY" => out.push_str(&date.format("%Y").to_string()),
                "MM" => out.push_str(&date.format("%m").to_string()),
                "DD" => out.push_str(&date.format("%d").to_string()),
                _ => return None,
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    Some(out)
}

/// Vault-relative path of the daily note, without the `.md` extension —
/// the form the `obsidian://` URI `file` parameter expects.
pub fn daily_note_rel_path(settings: &DailyNoteSettings, date: NaiveDate) -> String {
    let name = render_format(&settings.format, date);
    if settings.folder.is_empty() {
        name
    } else {
        format!("{}/{}", settings.folder, name)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: `test result: ok. 17 passed` (6 from Task 1 + 11 new)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core
git commit -m "feat(core): daily-note settings parsing and date-format rendering"
```

---

### Task 3: URI building, launching, and the open-vs-new decision

**Files:**
- Create: `src-tauri/core/src/uri.rs`
- Modify: `src-tauri/core/src/lib.rs`
- Test: inline `#[cfg(test)]` modules in `src-tauri/core/src/uri.rs` and `src-tauri/core/src/lib.rs`

**Interfaces:**
- Consumes: `daily_notes::{load_settings, daily_note_rel_path}` (Task 2)
- Produces:
  - `uri::open_vault_uri(vault_id: &str) -> String`
  - `uri::open_file_uri(vault_id: &str, file: &str) -> String`
  - `uri::new_file_uri(vault_id: &str, file: &str) -> String`
  - `uri::launch(uri: &str) -> Result<(), String>` — logs then launches via the OS opener
  - crate-root `daily_note_uri(vault_id: &str, vault_path: &Path, date: NaiveDate) -> String` — `obsidian://open` if today's note file exists, else `obsidian://new`
  - The `vault` URI parameter is always the Obsidian vault **ID** (unique key from `obsidian.json`) — the URI scheme accepts name or ID, and IDs disambiguate same-named vaults.

- [ ] **Step 1: Register the module**

In `src-tauri/core/src/lib.rs`:

```rust
pub mod daily_notes;
pub mod discovery;
pub mod uri;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/core/src/uri.rs` containing only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_open_vault_uri_with_encoding() {
        // Vault IDs from obsidian.json are hex, but the builder must encode
        // whatever it is given.
        assert_eq!(
            open_vault_uri("a1b2 c3"),
            "obsidian://open?vault=a1b2%20c3"
        );
    }

    #[test]
    fn builds_open_file_uri() {
        assert_eq!(
            open_file_uri("a1b2c3", "Daily/2026-07-03"),
            "obsidian://open?vault=a1b2c3&file=Daily%2F2026%2D07%2D03"
        );
    }

    #[test]
    fn builds_new_file_uri() {
        assert_eq!(
            new_file_uri("a1b2c3", "2026-07-03"),
            "obsidian://new?vault=a1b2c3&file=2026%2D07%2D03"
        );
    }
}
```

And append to `src-tauri/core/src/lib.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 7, 3).unwrap()
    }

    #[test]
    fn existing_note_uses_open_uri() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("2026-07-03.md"), "hello").unwrap();
        let uri = daily_note_uri("a1b2c3", dir.path(), date());
        assert!(uri.starts_with("obsidian://open?"), "got: {uri}");
    }

    #[test]
    fn missing_note_uses_new_uri() {
        let dir = tempfile::tempdir().unwrap();
        let uri = daily_note_uri("a1b2c3", dir.path(), date());
        assert!(uri.starts_with("obsidian://new?"), "got: {uri}");
    }

    #[test]
    fn respects_vault_daily_note_settings() {
        let dir = tempfile::tempdir().unwrap();
        let obsidian_dir = dir.path().join(".obsidian");
        std::fs::create_dir_all(&obsidian_dir).unwrap();
        std::fs::write(
            obsidian_dir.join("daily-notes.json"),
            r#"{ "folder": "Journal", "format": "YYYY-MM-DD" }"#,
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("Journal")).unwrap();
        std::fs::write(dir.path().join("Journal/2026-07-03.md"), "x").unwrap();
        let uri = daily_note_uri("a1b2c3", dir.path(), date());
        assert_eq!(uri, "obsidian://open?vault=a1b2c3&file=Journal%2F2026%2D07%2D03");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri/core && cargo test`
Expected: compile error — `open_vault_uri`, `daily_note_uri` not found.

- [ ] **Step 4: Write the implementation**

Add above the `tests` module in `src-tauri/core/src/uri.rs`:

```rust
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};

fn encode(value: &str) -> String {
    utf8_percent_encode(value, NON_ALPHANUMERIC).to_string()
}

// The `vault` parameter is always the Obsidian vault ID (the unique key
// from obsidian.json). The URI scheme accepts name or ID; the ID
// disambiguates vaults whose folders share a name.

pub fn open_vault_uri(vault_id: &str) -> String {
    format!("obsidian://open?vault={}", encode(vault_id))
}

pub fn open_file_uri(vault_id: &str, file: &str) -> String {
    format!(
        "obsidian://open?vault={}&file={}",
        encode(vault_id),
        encode(file)
    )
}

pub fn new_file_uri(vault_id: &str, file: &str) -> String {
    format!(
        "obsidian://new?vault={}&file={}",
        encode(vault_id),
        encode(file)
    )
}

/// Logs and launches a URI via the OS default handler. The log line is the
/// audit trail required by the PRD's transparency principle.
pub fn launch(uri: &str) -> Result<(), String> {
    log::info!("launching URI: {uri}");
    open::that_detached(uri).map_err(|e| format!("failed to launch {uri}: {e}"))
}
```

Add above the `tests` module in `src-tauri/core/src/lib.rs` (below the `pub mod` lines):

```rust
use chrono::NaiveDate;
use std::path::Path;

/// Builds the URI that opens today's daily note for a vault:
/// `obsidian://open` if the note file already exists, `obsidian://new`
/// otherwise — Obsidian itself performs the creation. Vault Buddy never
/// writes into a vault. `vault_id` is the unique key from obsidian.json.
pub fn daily_note_uri(vault_id: &str, vault_path: &Path, date: NaiveDate) -> String {
    let settings = daily_notes::load_settings(vault_path);
    let rel = daily_notes::daily_note_rel_path(&settings, date);
    let exists = vault_path.join(format!("{rel}.md")).exists();
    if exists {
        uri::open_file_uri(vault_id, &rel)
    } else {
        uri::new_file_uri(vault_id, &rel)
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri/core && cargo test`
Expected: `test result: ok. 23 passed` (17 prior + 3 uri + 3 lib)

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core
git commit -m "feat(core): obsidian URI building and open-vs-new daily note decision"
```

---

### Task 4: Frontend scaffold (Vite + Vue + Tailwind + Vitest)

**Files:**
- Create: `package.json`
- Create: `vite.config.ts`
- Create: `tsconfig.json`
- Create: `index.html`
- Create: `src/main.ts`
- Create: `src/style.css`
- Create: `src/App.vue` (placeholder — replaced in Task 8)
- Create: `src/types.ts`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `Vault` TS interface in `src/types.ts`: `{ id: string; name: string; path: string }`
  - npm scripts: `dev`, `build`, `test`, `tauri`
  - Vitest configured with `happy-dom` environment

- [ ] **Step 1: Create the scaffold files**

`package.json`:

```json
{
  "name": "vault-buddy",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "pinia": "^3",
    "vue": "^3.5"
  },
  "devDependencies": {
    "@tailwindcss/vite": "^4",
    "@tauri-apps/cli": "^2",
    "@vitejs/plugin-vue": "^5",
    "@vue/test-utils": "^2",
    "happy-dom": "^17",
    "tailwindcss": "^4",
    "typescript": "^5",
    "vite": "^6",
    "vitest": "^3",
    "vue-tsc": "^2"
  }
}
```

`vite.config.ts` (note: `defineConfig` from `vitest/config` so the `test` key is typed):

```ts
import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [vue(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  test: {
    environment: "happy-dom",
  },
});
```

`tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2021",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "esModuleInterop": true,
    "skipLibCheck": true,
    "lib": ["ES2021", "DOM", "DOM.Iterable"],
    "types": ["vite/client"]
  },
  "include": ["src/**/*.ts", "src/**/*.vue", "tests/**/*.ts"]
}
```

`index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <title>Vault Buddy</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`src/main.ts`:

```ts
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import "./style.css";

createApp(App).use(createPinia()).mount("#app");
```

`src/style.css` (transparent background is what makes the frameless window invisible around the character):

```css
@import "tailwindcss";

html,
body,
#app {
  margin: 0;
  height: 100%;
  background: transparent;
  overflow: hidden;
  user-select: none;
}
```

`src/App.vue` (placeholder, replaced in Task 8):

```vue
<template>
  <main class="flex h-screen w-screen items-start gap-2 p-2">
    <p class="text-xs text-slate-500">Vault Buddy</p>
  </main>
</template>
```

`src/types.ts`:

```ts
export interface Vault {
  id: string;
  name: string;
  path: string;
}
```

- [ ] **Step 2: Install and verify the build**

Run: `npm install`
Expected: completes without errors (lockfile created).

Run: `npm run build`
Expected: `vue-tsc` passes, Vite writes `dist/`.

Run: `npm run test`
Expected: "No test files found" — exits non-zero; that's fine at this point, Task 5 adds the first test. Verify Vitest itself starts (banner prints) rather than crashing on config.

- [ ] **Step 3: Commit**

```bash
git add package.json package-lock.json vite.config.ts tsconfig.json index.html src
git commit -m "feat(ui): Vite + Vue 3 + Tailwind 4 + Vitest scaffold"
```

---

### Task 5: Vaults Pinia store

**Files:**
- Create: `src/stores/vaults.ts`
- Test: `tests/vaults-store.test.ts`

**Interfaces:**
- Consumes: `Vault` from `src/types.ts` (Task 4); Tauri commands `list_vaults`, `open_vault`, `open_daily_note` (implemented in Task 9 — mocked here via `mockIPC`)
- Produces: `useVaultsStore` with:
  - state: `vaults: Vault[]`, `loaded: boolean`, `panelOpen: boolean`, `busyVaultId: string | null`, `error: string | null`
  - actions: `loadVaults(): Promise<void>`, `togglePanel(): Promise<void>`, `runAction(command: "open_vault" | "open_daily_note", vaultId: string): Promise<void>`

- [ ] **Step 1: Write the failing tests**

`tests/vaults-store.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal" },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work" },
];

describe("vaults store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loads vaults via the list_vaults command", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);
    expect(store.loaded).toBe(true);
  });

  it("opening the panel triggers the first load", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    expect(store.panelOpen).toBe(false);
    await store.togglePanel();
    expect(store.panelOpen).toBe(true);
    expect(store.vaults).toEqual(sampleVaults);
  });

  it("runAction passes the vault id and tracks busy state", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    await store.runAction("open_daily_note", "a1b2c3");
    expect(calls).toEqual([{ cmd: "open_daily_note", args: { id: "a1b2c3" } }]);
    expect(store.busyVaultId).toBe(null);
    expect(store.error).toBe(null);
  });

  it("runAction surfaces command errors", async () => {
    mockIPC(() => {
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    await store.runAction("open_vault", "nope");
    expect(store.error).toContain("vault not found");
    expect(store.busyVaultId).toBe(null);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test`
Expected: FAIL — cannot resolve `../src/stores/vaults`.

- [ ] **Step 3: Write the implementation**

`src/stores/vaults.ts`:

```ts
import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import type { Vault } from "../types";

export const useVaultsStore = defineStore("vaults", {
  state: () => ({
    vaults: [] as Vault[],
    loaded: false,
    panelOpen: false,
    busyVaultId: null as string | null,
    error: null as string | null,
  }),
  actions: {
    async loadVaults() {
      this.vaults = await invoke<Vault[]>("list_vaults");
      this.loaded = true;
    },
    async togglePanel() {
      this.panelOpen = !this.panelOpen;
      if (this.panelOpen && !this.loaded) {
        await this.loadVaults();
      }
    },
    async runAction(
      command: "open_vault" | "open_daily_note",
      vaultId: string,
    ) {
      this.busyVaultId = vaultId;
      this.error = null;
      try {
        await invoke(command, { id: vaultId });
      } catch (e) {
        this.error = String(e);
      } finally {
        this.busyVaultId = null;
      }
    },
  },
});
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test`
Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/stores/vaults.ts tests/vaults-store.test.ts
git commit -m "feat(ui): vaults store with load, toggle, and action dispatch"
```

---

### Task 6: Companion character component

**Files:**
- Create: `src/components/CompanionCharacter.vue`
- Test: `tests/companion-character.test.ts`

**Interfaces:**
- Consumes: nothing
- Produces: `CompanionCharacter.vue` — props `{ working: boolean }`, emits `toggle`. Three visual states: idle (bob animation, default), greeting (wiggle, via CSS `:hover`), working (pulse, via `working` prop). Contains the `data-tauri-drag-region` drag handle.

- [ ] **Step 1: Write the failing tests**

`tests/companion-character.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import CompanionCharacter from "../src/components/CompanionCharacter.vue";

describe("CompanionCharacter", () => {
  it("emits toggle when the character is clicked", async () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    await wrapper.find("button.buddy").trigger("click");
    expect(wrapper.emitted("toggle")).toHaveLength(1);
  });

  it("applies the working class while an action runs", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: true } });
    expect(wrapper.find("button.buddy").classes()).toContain("working");
  });

  it("has a drag region handle for moving the window", () => {
    const wrapper = mount(CompanionCharacter, { props: { working: false } });
    expect(wrapper.find("[data-tauri-drag-region]").exists()).toBe(true);
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test`
Expected: FAIL — cannot resolve `../src/components/CompanionCharacter.vue`.

- [ ] **Step 3: Write the implementation**

`src/components/CompanionCharacter.vue`:

```vue
<script setup lang="ts">
defineProps<{ working: boolean }>();
defineEmits<{ (e: "toggle"): void }>();
</script>

<template>
  <div class="flex flex-col items-center">
    <div
      data-tauri-drag-region
      class="cursor-move px-2 py-1 text-xs leading-none text-slate-400"
      title="Drag to move Vault Buddy"
    >
      ⠿
    </div>
    <button
      type="button"
      class="buddy block focus:outline-none"
      :class="{ working }"
      aria-label="Vault Buddy — click to open the panel"
      @click="$emit('toggle')"
    >
      <svg width="96" height="96" viewBox="0 0 96 96" aria-hidden="true">
        <ellipse cx="48" cy="52" rx="34" ry="32" fill="#7c5cff" />
        <circle class="eye" cx="38" cy="46" r="5" fill="#fff" />
        <circle class="eye" cx="58" cy="46" r="5" fill="#fff" />
        <path
          d="M40 62 Q48 70 56 62"
          stroke="#fff"
          stroke-width="3"
          fill="none"
          stroke-linecap="round"
        />
      </svg>
    </button>
  </div>
</template>

<style scoped>
/* idle */
.buddy {
  animation: bob 3s ease-in-out infinite;
}
/* greeting */
.buddy:hover:not(.working) {
  animation: wiggle 0.6s ease-in-out infinite;
}
/* working */
.buddy.working {
  animation: pulse 0.9s ease-in-out infinite;
}
.buddy .eye {
  animation: blink 4s infinite;
  transform-origin: center;
  transform-box: fill-box;
}
@keyframes bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-4px);
  }
}
@keyframes wiggle {
  0%,
  100% {
    transform: rotate(-4deg);
  }
  50% {
    transform: rotate(4deg);
  }
}
@keyframes pulse {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(0.94);
  }
}
@keyframes blink {
  0%,
  92%,
  100% {
    transform: scaleY(1);
  }
  96% {
    transform: scaleY(0.1);
  }
}
</style>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test`
Expected: 7 tests pass (4 store + 3 character).

- [ ] **Step 5: Commit**

```bash
git add src/components/CompanionCharacter.vue tests/companion-character.test.ts
git commit -m "feat(ui): animated companion character with idle/greeting/working states"
```

---

### Task 7: Vault list and action panel components

**Files:**
- Create: `src/components/VaultList.vue`
- Create: `src/components/ActionPanel.vue`
- Test: `tests/action-panel.test.ts`

**Interfaces:**
- Consumes: `useVaultsStore` (Task 5), `Vault` type (Task 4)
- Produces:
  - `VaultList.vue` — props `{ vaults: Vault[]; busyVaultId: string | null }`, emits `open-vault(id: string)` and `open-daily-note(id: string)`
  - `ActionPanel.vue` — no props; reads the store, renders `VaultList`, the error banner, and the "Obsidian not found" empty state

- [ ] **Step 1: Write the failing tests**

`tests/action-panel.test.ts`:

```ts
import { beforeEach, afterEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import ActionPanel from "../src/components/ActionPanel.vue";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal" },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work" },
];

describe("ActionPanel", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("lists each vault with both actions", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("Personal");
    expect(wrapper.text()).toContain("Work");
    const buttons = wrapper.findAll("button");
    expect(buttons).toHaveLength(4); // 2 vaults × 2 actions
  });

  it("dispatches open_daily_note with the vault id", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const dailyButtons = wrapper
      .findAll("button")
      .filter((b) => b.text().includes("daily note"));
    await dailyButtons[0].trigger("click");
    expect(calls).toEqual([{ cmd: "open_daily_note", args: { id: "d4e5f6" } }]);
  });

  it("shows the friendly empty state when no vaults were found", () => {
    const store = useVaultsStore();
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("Obsidian not found");
  });

  it("shows the error banner when an action failed", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.error = "failed to launch obsidian://open";
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("failed to launch");
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npm run test`
Expected: FAIL — cannot resolve `../src/components/ActionPanel.vue`.

- [ ] **Step 3: Write the implementation**

`src/components/VaultList.vue`:

```vue
<script setup lang="ts">
import type { Vault } from "../types";

defineProps<{ vaults: Vault[]; busyVaultId: string | null }>();
defineEmits<{
  (e: "open-vault", id: string): void;
  (e: "open-daily-note", id: string): void;
}>();
</script>

<template>
  <ul class="space-y-2">
    <li
      v-for="vault in vaults"
      :key="vault.id"
      class="rounded-lg bg-white/90 px-3 py-2 shadow"
    >
      <div class="text-sm font-semibold text-slate-800">{{ vault.name }}</div>
      <div class="mt-1 flex gap-2">
        <button
          type="button"
          class="rounded bg-violet-600 px-2 py-1 text-xs text-white hover:bg-violet-500 disabled:opacity-50"
          :disabled="busyVaultId !== null"
          @click="$emit('open-vault', vault.id)"
        >
          Open vault
        </button>
        <button
          type="button"
          class="rounded bg-violet-600 px-2 py-1 text-xs text-white hover:bg-violet-500 disabled:opacity-50"
          :disabled="busyVaultId !== null"
          @click="$emit('open-daily-note', vault.id)"
        >
          Open today's daily note
        </button>
      </div>
    </li>
  </ul>
</template>
```

`src/components/ActionPanel.vue`:

```vue
<script setup lang="ts">
import { useVaultsStore } from "../stores/vaults";
import VaultList from "./VaultList.vue";

const store = useVaultsStore();
</script>

<template>
  <div class="h-full w-64 overflow-y-auto rounded-xl bg-slate-100/95 p-3 shadow-xl">
    <h1 class="mb-2 text-sm font-bold text-slate-700">Your vaults</h1>
    <p
      v-if="store.error"
      class="mb-2 rounded bg-red-100 px-2 py-1 text-xs text-red-700"
    >
      {{ store.error }}
    </p>
    <VaultList
      v-if="store.vaults.length > 0"
      :vaults="store.vaults"
      :busy-vault-id="store.busyVaultId"
      @open-vault="store.runAction('open_vault', $event)"
      @open-daily-note="store.runAction('open_daily_note', $event)"
    />
    <p v-else-if="store.loaded" class="text-xs text-slate-600">
      Obsidian not found — no vaults discovered. Is Obsidian installed and has
      it been opened at least once?
    </p>
  </div>
</template>
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npm run test`
Expected: 11 tests pass (4 store + 3 character + 4 panel).

- [ ] **Step 5: Commit**

```bash
git add src/components/VaultList.vue src/components/ActionPanel.vue tests/action-panel.test.ts
git commit -m "feat(ui): action panel with vault list, error banner, and empty state"
```

---

### Task 8: App assembly and window-resize composable

**Files:**
- Create: `src/composables/useCompanionWindow.ts`
- Modify: `src/App.vue` (replace the Task 4 placeholder entirely)

**Interfaces:**
- Consumes: `CompanionCharacter` (Task 6), `ActionPanel` (Task 7), `useVaultsStore` (Task 5)
- Produces: `useCompanionWindow(panelOpen: Ref<boolean>): void` and the assembled `App.vue`. The composable is a thin Tauri-window wrapper — intentionally not unit-tested; it is covered by the Windows manual verification in Task 10.

- [ ] **Step 1: Write the composable**

`src/composables/useCompanionWindow.ts`:

```ts
import { watch, type Ref } from "vue";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

export const COLLAPSED = { width: 140, height: 170 };
export const EXPANDED = { width: 440, height: 340 };

/**
 * Grows the transparent window when the panel opens and shrinks it back when
 * it closes, so the invisible window never blocks clicks on the desktop
 * beneath it.
 */
export function useCompanionWindow(panelOpen: Ref<boolean>): void {
  watch(panelOpen, async (open) => {
    const size = open ? EXPANDED : COLLAPSED;
    await getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
  });
}
```

- [ ] **Step 2: Replace App.vue**

`src/App.vue` (full new content):

```vue
<script setup lang="ts">
import { computed } from "vue";
import { storeToRefs } from "pinia";
import CompanionCharacter from "./components/CompanionCharacter.vue";
import ActionPanel from "./components/ActionPanel.vue";
import { useCompanionWindow } from "./composables/useCompanionWindow";
import { useVaultsStore } from "./stores/vaults";

const store = useVaultsStore();
const { panelOpen, busyVaultId } = storeToRefs(store);
const working = computed(() => busyVaultId.value !== null);

useCompanionWindow(panelOpen);
</script>

<template>
  <main class="flex h-screen w-screen items-start gap-2 p-2">
    <CompanionCharacter :working="working" @toggle="store.togglePanel()" />
    <ActionPanel v-if="panelOpen" />
  </main>
</template>
```

- [ ] **Step 3: Verify build and full test suite**

Run: `npm run build`
Expected: `vue-tsc` passes, Vite build succeeds.

Run: `npm run test`
Expected: 11 tests pass (unchanged).

- [ ] **Step 4: Commit**

```bash
git add src/App.vue src/composables/useCompanionWindow.ts
git commit -m "feat(ui): assemble companion app with panel-driven window resizing"
```

---

### Task 9: Tauri shell — app crate, window config, tray, commands, icons

The app crate cannot compile in this Linux environment (no webkit2gtk). The automated check for this task is that the **core** crate still tests green inside the new workspace and the frontend still builds; the app crate itself is compile-verified on Windows (Task 10 checklist).

**Files:**
- Create: `src-tauri/Cargo.toml`
- Create: `src-tauri/build.rs`
- Create: `src-tauri/tauri.conf.json`
- Create: `src-tauri/capabilities/default.json`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/commands.rs`
- Create: `src-tauri/src/tray.rs`
- Create: `scripts/make-icon.mjs`
- Create: `src-tauri/icons/*` (generated by `npx tauri icon`)

**Interfaces:**
- Consumes: `vault_buddy_core::{discovery, uri, daily_note_uri}` (Tasks 1–3); frontend `dist/` and dev server (Task 4)
- Produces: Tauri commands `list_vaults() -> Vec<Vault>`, `open_vault(id: String) -> Result<(), String>`, `open_daily_note(id: String) -> Result<(), String>` — exactly the names/args the store (Task 5) invokes.

- [ ] **Step 1: Create the app crate manifest and build script**

`src-tauri/Cargo.toml`:

```toml
[package]
name = "vault-buddy"
version = "0.1.0"
description = "AI-native desktop companion for Obsidian"
edition = "2021"

[lib]
name = "vault_buddy_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
tauri-plugin-log = "2"
serde = { version = "1", features = ["derive"] }
chrono = { version = "0.4", default-features = false, features = ["clock"] }
vault_buddy_core = { path = "core" }

[workspace]
members = ["core"]
```

`src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build()
}
```

- [ ] **Step 2: Create the Tauri config and capabilities**

`src-tauri/tauri.conf.json`:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Vault Buddy",
  "version": "0.1.0",
  "identifier": "com.vaultbuddy.desktop",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "Vault Buddy",
        "width": 140,
        "height": 170,
        "transparent": true,
        "decorations": false,
        "alwaysOnTop": true,
        "resizable": false,
        "skipTaskbar": true,
        "shadow": false
      }
    ],
    "security": { "csp": null }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

`src-tauri/capabilities/default.json`:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Companion window: drag anywhere, resize when the panel opens",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:window:allow-start-dragging",
    "core:window:allow-set-size"
  ]
}
```

- [ ] **Step 3: Write the Rust shell**

`src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vault_buddy_lib::run();
}
```

`src-tauri/src/lib.rs`:

```rust
mod commands;
mod tray;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::list_vaults,
            commands::open_vault,
            commands::open_daily_note
        ])
        .setup(|app| {
            tray::create_tray(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Vault Buddy");
}
```

`src-tauri/src/commands.rs`:

```rust
use chrono::Local;
use std::path::Path;
use vault_buddy_core::{daily_note_uri, discovery, uri};

fn find_vault(id: &str) -> Result<discovery::Vault, String> {
    discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or_else(|| format!("vault not found: {id}"))
}

#[tauri::command]
pub fn list_vaults() -> Vec<discovery::Vault> {
    discovery::discover_vaults()
}

#[tauri::command]
pub fn open_vault(id: String) -> Result<(), String> {
    let vault = find_vault(&id)?;
    // Address the vault by ID, not name — names can collide across vaults.
    uri::launch(&uri::open_vault_uri(&vault.id))
}

#[tauri::command]
pub fn open_daily_note(id: String) -> Result<(), String> {
    let vault = find_vault(&id)?;
    let today = Local::now().date_naive();
    let target = daily_note_uri(&vault.id, Path::new(&vault.path), today);
    uri::launch(&target)
}
```

`src-tauri/src/tray.rs`:

```rust
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

pub fn create_tray(app: &AppHandle) -> tauri::Result<()> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&toggle, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().cloned().expect("bundled icon missing"))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "toggle" => {
                if let Some(window) = app.get_webview_window("main") {
                    let visible = window.is_visible().unwrap_or(true);
                    let _ = if visible { window.hide() } else { window.show() };
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
```

- [ ] **Step 4: Generate placeholder icons**

`scripts/make-icon.mjs` — dependency-free 1024×1024 solid-purple PNG generator (Tauri's `icon` command derives all platform icons from it):

```js
// Generates a solid-color placeholder app icon (1024x1024 PNG) with no deps.
import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";

const SIZE = 1024;
const [R, G, B, A] = [124, 92, 255, 255]; // Vault Buddy purple #7c5cff

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const chunk = (type, data) => {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
};

const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // color type: RGBA
const row = Buffer.alloc(1 + SIZE * 4); // filter byte + pixels
for (let x = 0; x < SIZE; x++) row.set([R, G, B, A], 1 + x * 4);
const raw = Buffer.concat(Array.from({ length: SIZE }, () => row));
const png = Buffer.concat([
  Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  chunk("IHDR", ihdr),
  chunk("IDAT", deflateSync(raw)),
  chunk("IEND", Buffer.alloc(0)),
]);
writeFileSync("app-icon.png", png);
console.log("wrote app-icon.png");
```

Run:

```bash
node scripts/make-icon.mjs
npx tauri icon app-icon.png
```

Expected: `src-tauri/icons/` now contains `32x32.png`, `128x128.png`, `128x128@2x.png`, `icon.icns`, `icon.ico`, and more. (`app-icon.png` itself is gitignored.)

- [ ] **Step 5: Verify what this environment can verify**

Run: `cd src-tauri/core && cargo test`
Expected: `test result: ok. 23 passed` — the core crate still tests green as a workspace member. (Cargo will download the tauri dependency graph to update the lockfile; it will not compile the app crate.)

Run: `npm run build && npm run test`
Expected: build succeeds, 11 tests pass.

Do NOT run `cargo build`/`cargo check` from `src-tauri/` — it will fail on missing webkit2gtk here; Windows verification happens in Task 10.

- [ ] **Step 6: Commit**

```bash
git add src-tauri scripts
git commit -m "feat(app): Tauri 2 shell with transparent companion window, tray, and vault commands"
```

---

### Task 10: Docs and the Windows verification checklist

**Files:**
- Modify: `README.md` (add a Development section)
- Create: `docs/superpowers/specs/2026-07-03-increment-1-windows-verification.md`

**Interfaces:**
- Consumes: everything prior
- Produces: developer setup docs and the manual verification checklist that gates increment 1 as "done" per the spec's success criteria.

- [ ] **Step 1: Add a Development section to README.md**

Insert after the intro block (before "## Development with Superpowers"):

```markdown
## Development

Prerequisites: [Node 22+](https://nodejs.org), [Rust stable](https://rustup.rs),
and on Windows the [Tauri prerequisites](https://tauri.app/start/prerequisites/)
(WebView2 is preinstalled on Windows 11).

```bash
npm install          # frontend dependencies
npm run test         # Vitest component/store tests
npm run build        # typecheck + production frontend build
cd src-tauri/core && cargo test   # pure Rust core tests (run anywhere)
npm run tauri dev    # run the desktop app (needs a desktop OS with WebView)
```

The Rust code is split in two: `src-tauri/core/` is a pure crate with all
Obsidian logic (config parsing, daily-note resolution, URI building) and no
GUI dependencies — it tests on any machine, including CI containers.
`src-tauri/` is the thin Tauri shell (window, tray, command wrappers) and
needs platform WebView libraries to compile.
```

- [ ] **Step 2: Write the Windows verification checklist**

`docs/superpowers/specs/2026-07-03-increment-1-windows-verification.md`:

```markdown
# Increment 1 — Windows Manual Verification Checklist

Run on a Windows machine with Obsidian installed. This is the manual gate for
the spec's success criteria (the automated gates are `cargo test` in
`src-tauri/core` and `npm run test` / `npm run build`).

Setup: `npm install`, then `npm run tauri dev`.

- [ ] App launches; the companion appears in a transparent window (no frame,
      no background rectangle), always on top, with a tray icon.
- [ ] Character idles (bobbing), wiggles on hover, and the ⠿ handle drags the
      window around the desktop.
- [ ] Clicking the character grows the window and shows the panel listing your
      real vaults (names match Obsidian's vault switcher).
- [ ] "Open vault" brings that vault up in Obsidian.
- [ ] "Open today's daily note" with an existing note opens it in the right
      vault.
- [ ] Delete/rename today's note, retry: Obsidian creates it (empty — template
      not applied is a known limitation).
- [ ] Vault with a custom daily-note folder/format (e.g. `Journal`,
      `YYYY/MM-DD`) resolves the correct file.
- [ ] While an action runs the character pulses (working state); afterwards the
      panel stays responsive.
- [ ] Rename `%APPDATA%\obsidian\obsidian.json` temporarily and restart: the
      panel shows the "Obsidian not found" message; no crash. Restore the file.
- [ ] Tray "Show / Hide" toggles the companion; "Quit Vault Buddy" exits the
      app.
- [ ] `tauri dev` console shows a `launching URI: obsidian://...` log line for
      every action performed.
```

- [ ] **Step 3: Commit**

```bash
git add README.md docs/superpowers/specs/2026-07-03-increment-1-windows-verification.md
git commit -m "docs: development setup and increment 1 Windows verification checklist"
```
