# Document Import via Pandoc Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert a `.docx` / `.odt` / `.rtf` file into a vault note via a
user-installed Pandoc, triggered by dropping the file on the buddy or
picking it from the record chooser, gated behind Pandoc detection.

**Architecture:** A new Document Import domain following the repo's
pure-core / thin-shell split. Pure path/naming/frontmatter/staging logic
lives in `core::document_import` (Linux-testable). The shell adds a Pandoc
detector and an async `convert_document` command (subprocess work under
`spawn_blocking`, like `search_vaults`). The frontend adds a settings
section, a record-chooser action, and a buddy drag-drop → vault-picker
flow. No worker queue (one file at a time). Converted notes are the vault
domain's fifth sanctioned write path, reusing `capture_note`'s atomic
never-clobber writers.

**Tech Stack:** Rust (Tauri v2 shell + pure `vault_buddy_core` crate),
Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + mockIPC), Pandoc ≥ 2.15
(external, user-installed).

## Global Constraints

- **Spec:** `docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md` — read it before starting.
- **Formats:** `.docx` → reader `docx`, `.odt` → `odt`, `.rtf` → `rtf`. No other formats. Extension is authoritative (no content sniffing).
- **Pandoc version floor:** `--sandbox` requires Pandoc ≥ 2.15. A Pandoc without `--sandbox` support MUST NOT run on untrusted input — report it and refuse the conversion.
- **Pandoc invocation (exact):** working directory = the in-vault temp dir; outputs given as **relative** names; `--sandbox` always; heap cap `+RTS -M512M -RTS` always; source passed as its real absolute path. `-t gfm` output.
- **Staging on the vault's volume:** the temp working dir lives INSIDE the destination vault (dot-prefixed), so the publish `rename` is same-filesystem/atomic. Publish media directory FIRST, then the note.
- **Never clobber a vault file:** reuse `capture_note::write_note_collision_safe` / `write_note_atomic` (exclusive-create temp, `rename_noreplace`, ` (N)` suffix). Config writes (app-side `config.json`) use the REPLACING `write_config` — never for vault files.
- **Serialize conversions:** a process-wide `ImportLock` `try_lock` wraps the whole convert-and-publish body; a second concurrent import fails fast ("an import is already in progress") rather than racing the exists-reservation. Staging dirs also carry a per-invocation unique token.
- **Publish rollback:** publish media dir before the note; if the note commit fails after media is published, roll the media directory back so a failed import leaves nothing at the published path.
- **Validate the documents folder is inside the vault** with `safe_recording_root` + `assert_path_inside_vault` BOTH when saving the setting AND again in `convert_document` before staging (config.json is hand-editable — `../…`/symlink escapes must be caught at conversion time too).
- **Settings scope split:** app-global Pandoc state (status, path override) lives in `DocumentImportSettings.vue` (no vault context, `detect_pandoc`/`set_pandoc_path`); the per-vault Documents Folder lives in `CaptureSettings.vue` (`get_documents_config(id)`/`set_documents_config(id, folder)`, mirroring the Tasks Folder). `set_capture_config` must preserve `documents_folder`.
- **Dialog plugin is a prerequisite (Task 0):** `@tauri-apps/plugin-dialog` + `tauri-plugin-dialog` crate + builder `.plugin(...)` + `dialog:allow-open` capability must be wired before any `open()` file picker is used.
- **YAML values:** every frontmatter string value goes through `capture_note::yaml_quote` (doubles `\` and `"`) — never emit a raw path.
- **Config defense:** `config.json` is hand-editable; parse per-field so one bad value defaults only itself. `serialize_config` MUST round-trip the new section (regression-test it, like the `mcp` section).
- **Failure = nothing published + a toast.** Success = silent save + a toast (no auto-open). Mirrors `capture:failed` / a finished recording.
- **Threads named; no swallowed errors** (`log::warn!`/`logWarning`); sync commands never block (subprocess work is async + `spawn_blocking`).
- **Commits:** Conventional Commits, scope `feat(...)` / `test(...)`; imperative subject; body explains the *why*. Do NOT put the model identifier anywhere in commits/PRs.
- **Compile gate:** after shell changes run `npm run setup:linux` once then `npx tauri build --no-bundle`; run `cargo fmt --check` and workspace clippy `-D warnings`. Frontend: `npm test`, `npm run build`.

---

## File Structure

**Create:**
- `src-tauri/core/src/document_import.rs` — pure: format map, basename, frontmatter render, target-path resolution, staging/publish plan + helpers.
- `src-tauri/src/document_commands.rs` — shell: `detect_pandoc`, `convert_document`, `get_documents_config`, `set_documents_config`; Pandoc process invocation.
- `src/components/DocumentImportSettings.vue` — Buddy-settings section (status / recheck / install link / path override).
- `src/components/ImportVaultPicker.vue` — the "Import into which vault?" view shown after a buddy drop.
- `tests/documentImport.test.ts` — frontend Vitest for the settings section, record-chooser action, and picker/drop flow.

**Modify:**
- `src-tauri/core/src/capture_config.rs` — add `documents_folder` to `VaultCaptureConfig`; add `DocumentImportConfig` app-global section; parse/serialize/round-trip.
- `src-tauri/core/src/lib.rs` (crate root) — `pub mod document_import;`.
- `src-tauri/src/lib.rs` — register the six new commands in `generate_handler!` + `.manage(ImportLock::default())` + `.manage(DocumentImportPending::default())`.
- `src-tauri/src/commands.rs` — factor `show_panel(app)` out of `toggle_panel` (reused by `begin_document_import`).
- `src-tauri/src/main.rs` / shell `lib.rs` module list — `mod document_commands;`.
- `src-tauri/Cargo.toml` — `[target.'cfg(windows)'.dependencies] winreg` for the registry PATH read; `tauri-plugin-dialog` (Task 0).
- `package.json` — `@tauri-apps/plugin-dialog` (Task 0).
- `src-tauri/capabilities/default.json` — `dialog:allow-open` grant (Task 0).
- `src/components/CaptureSettings.vue` — add the per-vault Documents Folder control (vault-scoped, mirrors its Tasks Folder control).
- `src/types.ts` — `PandocStatus` type (incl. `configuredPath`); extend `CaptureConfig` with `documentsFolder`.
- `src/stores/vaults.ts` — new `importPicker` view + `pendingImportPath` + `openImportPicker()`.
- `src/components/RecordMode.vue` — "Import Document" action (file picker) + Pandoc gate.
- `src/components/ActionPanel.vue` — route the new `importPicker` view.
- `src/components/BuddySettings.vue` — embed `DocumentImportSettings` (app-global Pandoc only).
- `src/roots/BuddyRoot.vue` — Tauri drag-drop listener → open panel on the import picker.
- `AGENTS.md` — IPC table (+4 commands), config-state row, a Document Import domain subsection.
- `docs/use-cases/document-import-pandoc.md` — flip status once shipped.

---

## Task 0: Wire the Tauri dialog plugin (prerequisite)

The "Browse…" Pandoc-path picker (Task 7) and the "Import Document" file
picker (Task 8) both use `@tauri-apps/plugin-dialog`'s `open()`. That plugin
is **not currently in the project** (verified absent from `package.json`,
`src-tauri/Cargo.toml`, and `capabilities/default.json`), so without this
task those buttons fail to build / are rejected at invoke. Wire it once, up
front.

**Files:**
- Modify: `package.json` (+ `package-lock.json`)
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs` — builder `.plugin(...)`
- Modify: `src-tauri/capabilities/default.json` — permission grant

- [ ] **Step 1: Add the JS + Rust dependencies**

```bash
npm install @tauri-apps/plugin-dialog
```
Add to `src-tauri/Cargo.toml` `[dependencies]` (beside the other
`tauri-plugin-*` entries):
```toml
tauri-plugin-dialog = "2"
```

- [ ] **Step 2: Register the plugin in the builder**

In `src-tauri/src/lib.rs`, add beside the other `.plugin(...)` calls (order
doesn't matter except `single-instance` stays first):
```rust
.plugin(tauri_plugin_dialog::init())
```

- [ ] **Step 3: Grant the capability**

In `src-tauri/capabilities/default.json`, add to `permissions`:
```json
"dialog:allow-open"
```
(Only the open dialog is used — not save/message/ask — so grant the narrow
`dialog:allow-open`, not `dialog:default`.)

- [ ] **Step 4: Build gate**

Run: `cd src-tauri && npx tauri build --no-bundle` and `npm run build`
Expected: both succeed (the plugin resolves; the capability schema validates).

- [ ] **Step 5: Commit**

```bash
git add package.json package-lock.json src-tauri/Cargo.toml src-tauri/src/lib.rs src-tauri/capabilities/default.json
git commit -m "chore: add the Tauri dialog plugin for document-import file pickers"
```

---

## Task 1: Config — `documents_folder` + `DocumentImportConfig` section

**Files:**
- Modify: `src-tauri/core/src/capture_config.rs`
- Test: inline `#[cfg(test)]` in the same file (repo convention).

**Interfaces:**
- Produces:
  - `VaultCaptureConfig.documents_folder: Option<String>` + `fn documents_root(&self) -> &str` (default `"Documents"`).
  - `struct DocumentImportConfig { pandoc_path: Option<String> }` with `Default`.
  - `AppConfig.document_import: DocumentImportConfig`.
  - `parse_config` reads it; `serialize_config` round-trips it (emitted only when non-default).
  - `fn update_document_import_config_at(path, cfg) -> io::Result<()>` + `fn update_document_import_config(cfg) -> Result<(), String>` (mirrors the mcp updater).

- [ ] **Step 1: Write failing tests**

Add to `capture_config.rs` tests:

```rust
#[test]
fn documents_root_defaults_to_documents() {
    let mut c = VaultCaptureConfig::default();
    assert_eq!(c.documents_root(), "Documents");
    c.documents_folder = Some("Imports".into());
    assert_eq!(c.documents_root(), "Imports");
}

#[test]
fn parses_document_import_section() {
    let json = r#"{"documentImport":{"pandocPath":"C:\\pandoc\\pandoc.exe"},"vaults":{}}"#;
    let cfg = parse_config(json);
    assert_eq!(cfg.document_import.pandoc_path.as_deref(), Some("C:\\pandoc\\pandoc.exe"));
}

#[test]
fn parses_documents_folder_per_vault() {
    let json = r#"{"vaults":{"v1":{"documentsFolder":"Imports"}}}"#;
    let cfg = parse_config(json);
    assert_eq!(cfg.vaults["v1"].documents_folder.as_deref(), Some("Imports"));
}

#[test]
fn serialize_roundtrips_document_import_section() {
    // Regression: serialize_config once emitted only `vaults`; a save from
    // another surface would silently delete this section. Mirrors the mcp test.
    let mut cfg = AppConfig::default();
    cfg.document_import.pandoc_path = Some("/usr/bin/pandoc".into());
    let round = parse_config(&serialize_config(&cfg));
    assert_eq!(round.document_import, cfg.document_import);
}

#[test]
fn serialize_omits_default_document_import_section() {
    let cfg = AppConfig::default();
    assert!(!serialize_config(&cfg).contains("documentImport"));
}
```

- [ ] **Step 2: Run tests — verify they fail**

Run: `cd src-tauri/core && cargo test document`
Expected: FAIL (no `documents_folder` field / `document_import` / `documents_root`).

- [ ] **Step 3: Implement**

In `VaultCaptureConfig` add `pub documents_folder: Option<String>,`; in its `Default` add `documents_folder: None,`; add:

```rust
/// The vault-relative folder holding imported documents. None → "Documents".
pub fn documents_root(&self) -> &str {
    self.documents_folder.as_deref().unwrap_or("Documents")
}
```

Add the app-global struct near `McpConfig`:

```rust
/// App-global Document Import settings. Pandoc is one system-wide binary,
/// so its path override is app-global, not per-vault. Top-level
/// `documentImport` section beside `vaults`/`mcp`; parsed per-field
/// defensively for the same reason.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DocumentImportConfig {
    /// Manual override for a Pandoc not on PATH (a portable install).
    /// None → detect on PATH only.
    pub pandoc_path: Option<String>,
}
```

Add `pub document_import: DocumentImportConfig,` to `AppConfig`.

In `vault_entry`, add to the returned struct:
```rust
documents_folder: entry.get("documentsFolder").and_then(|v| v.as_str()).map(str::to_string),
```

In `parse_config`, after the `mcp` line:
```rust
let document_import = value
    .get("documentImport")
    .map(document_import_entry)
    .unwrap_or_default();
AppConfig { vaults, mcp, document_import }
```

Add the entry parser:
```rust
fn document_import_entry(entry: &serde_json::Value) -> DocumentImportConfig {
    DocumentImportConfig {
        pandoc_path: entry
            .get("pandocPath")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
    }
}
```

In `serialize_config`, before the `vaults` block:
```rust
if cfg.document_import != DocumentImportConfig::default() {
    let mut di = Map::new();
    if let Some(p) = &cfg.document_import.pandoc_path {
        di.insert("pandocPath".to_string(), json!(p));
    }
    root.insert("documentImport".to_string(), Value::Object(di));
}
```

In the vault serialize block, after `tasksFolder`:
```rust
if let Some(folder) = &v.documents_folder {
    entry.insert("documentsFolder".to_string(), json!(folder));
}
```

Add the updater (mirror `update_mcp_config_at`/`update_mcp_config`):
```rust
pub fn update_document_import_config_at(
    path: &Path,
    di: DocumentImportConfig,
) -> std::io::Result<()> {
    let mut cfg = match std::fs::read_to_string(path) {
        Ok(json) => parse_config(&json),
        Err(_) => AppConfig::default(),
    };
    cfg.document_import = di;
    write_config(path, &cfg)
}

pub fn update_document_import_config(di: DocumentImportConfig) -> Result<(), String> {
    let path = config_path().ok_or("Cannot resolve the config directory")?;
    update_document_import_config_at(&path, di)
        .map_err(|e| format!("Could not save document import settings: {e}"))
}
```

- [ ] **Step 4: Run tests — verify pass**

Run: `cd src-tauri/core && cargo test document && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no clippy warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/capture_config.rs
git commit -m "feat(core): add document-import config (documents folder + pandoc path)"
```

---

## Task 2: Core — format map, basename, frontmatter render, target path

**Files:**
- Create: `src-tauri/core/src/document_import.rs`
- Modify: `src-tauri/core/src/lib.rs` — add `pub mod document_import;`
- Test: inline `#[cfg(test)]`.

**Interfaces:**
- Consumes: `capture_config::VaultCaptureConfig::documents_root`, `capture_note::yaml_quote`.
- Produces:
  - `enum DocFormat { Docx, Odt, Rtf }` with `fn from_extension(&str) -> Option<DocFormat>`, `fn reader(&self) -> &'static str`, `fn label(&self) -> &'static str`.
  - `fn document_basename(original_stem: &str, today: &str) -> String` → `YYYY-MM-DD <stem>`.
  - `struct DocMeta { source_path: String, imported: String, format: DocFormat }`.
  - `fn render_frontmatter(meta: &DocMeta) -> String` (a `---`-fenced block ending with a trailing newline, no body).
  - `fn target_dir(vault_path: &Path, documents_folder: &str, year: &str, month: &str) -> PathBuf` → `<vault>/<folder>/<YYYY>/<MM>`.

- [ ] **Step 1: Write failing tests**

```rust
use super::*;
use std::path::Path;

#[test]
fn format_from_extension_is_case_insensitive_and_bounded() {
    assert_eq!(DocFormat::from_extension("docx"), Some(DocFormat::Docx));
    assert_eq!(DocFormat::from_extension("DOCX"), Some(DocFormat::Docx));
    assert_eq!(DocFormat::from_extension("odt"), Some(DocFormat::Odt));
    assert_eq!(DocFormat::from_extension("rtf"), Some(DocFormat::Rtf));
    assert_eq!(DocFormat::from_extension("pdf"), None);
    assert_eq!(DocFormat::Docx.reader(), "docx");
}

#[test]
fn basename_is_date_prefixed_original_name() {
    assert_eq!(document_basename("Quarterly Report", "2026-07-10"),
               "2026-07-10 Quarterly Report");
}

#[test]
fn frontmatter_quotes_windows_source_path() {
    let meta = DocMeta {
        source_path: r"C:\Users\me\Quarterly Report.docx".into(),
        imported: "2026-07-10".into(),
        format: DocFormat::Docx,
    };
    let fm = render_frontmatter(&meta);
    assert!(fm.starts_with("---\n"));
    assert!(fm.contains("type: Document\n"));
    assert!(fm.contains("tags: [vault-buddy-import]\n"));
    // yaml_quote doubled the backslashes — no raw backslash escape in the scalar.
    assert!(fm.contains(r#"source: "C:\\Users\\me\\Quarterly Report.docx""#));
    assert!(fm.contains("imported: 2026-07-10\n"));
    assert!(fm.contains("format: docx\n"));
    assert!(fm.trim_end().ends_with("---"));
}

#[test]
fn target_dir_is_documents_folder_dated() {
    let d = target_dir(Path::new("/vault"), "Documents", "2026", "07");
    assert_eq!(d, Path::new("/vault/Documents/2026/07"));
}
```

- [ ] **Step 2: Run — verify fail**

Run: `cd src-tauri/core && cargo test --lib document_import`
Expected: FAIL (module/type not found).

- [ ] **Step 3: Implement `document_import.rs`**

```rust
//! Document Import: convert .docx/.odt/.rtf to a vault note via Pandoc.
//! Pure filename/frontmatter/path/staging logic; the shell drives Pandoc.
//! Fifth sanctioned vault write — same never-clobber discipline as the
//! capture note. Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use crate::capture_note::yaml_quote;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocFormat {
    Docx,
    Odt,
    Rtf,
}

impl DocFormat {
    /// Extension is authoritative (Obsidian/search treat extensions the same
    /// way). Case-insensitive so `.DOCX` from Windows still maps.
    pub fn from_extension(ext: &str) -> Option<DocFormat> {
        match ext.to_ascii_lowercase().as_str() {
            "docx" => Some(DocFormat::Docx),
            "odt" => Some(DocFormat::Odt),
            "rtf" => Some(DocFormat::Rtf),
            _ => None,
        }
    }

    /// The Pandoc `-f <reader>` value.
    pub fn reader(&self) -> &'static str {
        match self {
            DocFormat::Docx => "docx",
            DocFormat::Odt => "odt",
            DocFormat::Rtf => "rtf",
        }
    }

    /// Value written to the note's `format:` frontmatter field.
    pub fn label(&self) -> &'static str {
        self.reader()
    }
}

/// `YYYY-MM-DD <Original Name>` (no extension). `today` supplied by the shell
/// so the core stays clock-free.
pub fn document_basename(original_stem: &str, today: &str) -> String {
    format!("{today} {original_stem}")
}

pub struct DocMeta {
    /// The original file's absolute path (provenance).
    pub source_path: String,
    /// Import date, `YYYY-MM-DD`.
    pub imported: String,
    pub format: DocFormat,
}

/// The `type: Document` frontmatter block (no body — Pandoc's markdown is
/// prepended by the shell after this). Every string value quoted via
/// `yaml_quote`, so a Windows source path can't emit an invalid YAML escape.
pub fn render_frontmatter(meta: &DocMeta) -> String {
    format!(
        "---\ntype: Document\ntags: [vault-buddy-import]\nsource: {}\nimported: {}\nformat: {}\ncreated-by: Vault Buddy\n---\n\n",
        yaml_quote(&meta.source_path),
        meta.imported,
        meta.format.label(),
    )
}

/// `<vault>/<documents_folder>/<YYYY>/<MM>`.
pub fn target_dir(vault_path: &Path, documents_folder: &str, year: &str, month: &str) -> PathBuf {
    vault_path.join(documents_folder).join(year).join(month)
}
```

Add `pub mod document_import;` to `src-tauri/core/src/lib.rs` (alphabetically among the module list).

- [ ] **Step 4: Run — verify pass**

Run: `cd src-tauri/core && cargo test --lib document_import && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/document_import.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): document-import format map, naming, and frontmatter"
```

---

## Task 3: Core — staging + publish helpers

**Files:**
- Modify: `src-tauri/core/src/document_import.rs`
- Test: inline `#[cfg(test)]`.

**Interfaces:**
- Consumes: `capture_note::{write_note_atomic, NOTE_TMP_SUFFIX}`, `capture_paths::candidate`, Task 2 types.
- Produces:
  - `fn reserve_basename(target_dir: &Path, basename: &str) -> String` — walks the ` (N)` suffix scheme until BOTH `<name>.md` and the `<name>/` media folder are free; returns that name. Resolved BEFORE Pandoc so the media-folder name (which Pandoc bakes into image links) is final.
  - `struct StagePlan { work_dir: PathBuf, media_name: String, note_name: String }`.
  - `fn plan_staging(target_dir: &Path, basename: &str, unique: &str) -> StagePlan` — the dot-prefixed in-vault temp dir (carrying `unique` so two imports to the same date can't collide on the temp dir) + the relative media/note names Pandoc is handed (media = `<basename>`, note = `<basename>.md`). `basename` here is the already-reserved name.
  - `fn publish(plan: &StagePlan, target_dir: &Path, frontmatter: &str) -> io::Result<PathBuf>` — prepends frontmatter to the staged note, publishes media dir first (if present & non-empty) then the note at the EXACT reserved name (non-replacing), rolling the media dir back if the note write fails. Returns the final note path. Cleans the work dir on the way out.
  - `fn cleanup_staging(work_dir: &Path)` — best-effort `remove_dir_all`.
  - `fn is_import_staging_dir(name: &str) -> bool` — matches the owned `…vault-buddy.tmp.import` marker (never another tool's dot-dir).
  - `struct StagingSweep { removed: Vec<PathBuf>, pending: usize }`.
  - `fn clean_stale_staging_at(documents_root: &Path, now: SystemTime, stale_after: Duration) -> StagingSweep` — removes every import staging dir under `documents_root`'s `YYYY/MM` tree whose mtime is older than `stale_after` (canonical containment at every level so a symlink OR Windows junction can't redirect the delete outside the vault); returns what it removed plus a count of fresh orphans still pending (so the shell janitor reschedules). `now` injected so staleness is testable clock-free.

**Design notes for the implementer:**
- The suffix is resolved up front by `reserve_basename`, NOT at write time. Pandoc bakes the media-folder name into every image link as it converts, so the final sibling-folder name must be known before Pandoc runs — a publish-time re-suffix would break the links (note says `<name>/…`, folder became `<name> (1)/…`). `publish` therefore writes the note at the exact reserved name with `write_note_atomic` (non-replacing), not the suffix-retrying `write_note_collision_safe`.
- The work dir is dot-prefixed (`.<basename>.<unique>.vault-buddy.tmp.import`) UNDER `target_dir`, so it's on the vault's volume (same-fs rename) AND auto-excluded from every `vault_walk` scan (dot-directories are skipped). It is NOT excluded from recovery — a crash mid-Pandoc leaves it behind, and the startup import janitor (Task 5b) owns that cleanup.
- Pandoc is told `--extract-media=<media_name>` and `-o <note_name>` with `cwd = work_dir` (the shell does this in Task 5), so links are written relative to the note. Publishing moves the media dir and note into `target_dir` keeping those exact names, so links stay valid with no rewriting.
- Media dir may not exist (no images). Only publish it when it exists and is non-empty.
- Publish order: media dir → note. The note is never visible pointing at not-yet-landed images. If the note write races and fails, roll the media dir back.

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn reserve_basename_avoids_both_note_and_media_collisions() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Documents/2026/07");
    std::fs::create_dir_all(&target).unwrap();
    // base free → returned as-is
    assert_eq!(reserve_basename(&target, "2026-07-10 Report"), "2026-07-10 Report");
    // a prior note claims the base → next suffix (capture_paths::candidate
    // numbers the first collision (2), matching the capture/tasks scheme)
    std::fs::write(target.join("2026-07-10 Report.md"), "x").unwrap();
    assert_eq!(reserve_basename(&target, "2026-07-10 Report"), "2026-07-10 Report (2)");
    // a prior MEDIA FOLDER (no note) also forces a suffix — both must be free
    std::fs::create_dir_all(target.join("2026-07-10 Photo")).unwrap();
    assert_eq!(reserve_basename(&target, "2026-07-10 Photo"), "2026-07-10 Photo (2)");
}

#[test]
fn plan_staging_uses_dot_prefixed_in_vault_workdir() {
    let plan = plan_staging(Path::new("/vault/Documents/2026/07"), "2026-07-10 Report", "t1");
    assert!(plan.work_dir.starts_with("/vault/Documents/2026/07"));
    assert!(plan.work_dir.file_name().unwrap().to_string_lossy().starts_with('.'));
    // the unique token keeps two same-date imports from sharing a temp dir
    let other = plan_staging(Path::new("/vault/Documents/2026/07"), "2026-07-10 Report", "t2");
    assert_ne!(plan.work_dir, other.work_dir);
    assert_eq!(plan.media_name, "2026-07-10 Report");
    assert_eq!(plan.note_name, "2026-07-10 Report.md");
}

#[test]
fn publish_moves_note_and_media_and_prepends_frontmatter() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Documents/2026/07");
    std::fs::create_dir_all(&target).unwrap();
    let plan = plan_staging(&target, "2026-07-10 Report", "u");
    std::fs::create_dir_all(&plan.work_dir).unwrap();
    // Simulate a Pandoc run: note body + a media dir with one file.
    std::fs::write(plan.work_dir.join(&plan.note_name), "# Body\n\n![img](2026-07-10 Report/image1.png)\n").unwrap();
    let media = plan.work_dir.join(&plan.media_name);
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("image1.png"), b"PNG").unwrap();

    let note = publish(&plan, &target, "---\ntype: Document\n---\n\n").unwrap();
    let published = std::fs::read_to_string(&note).unwrap();
    assert!(published.starts_with("---\ntype: Document\n---\n\n# Body"));
    // media dir landed beside the note, same name → link still resolves
    assert!(target.join("2026-07-10 Report/image1.png").exists());
    // work dir cleaned up
    assert!(!plan.work_dir.exists());
}

#[test]
fn publish_without_media_writes_only_the_note() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Documents/2026/07");
    std::fs::create_dir_all(&target).unwrap();
    let plan = plan_staging(&target, "2026-07-10 Note", "u");
    std::fs::create_dir_all(&plan.work_dir).unwrap();
    std::fs::write(plan.work_dir.join(&plan.note_name), "# Body\n").unwrap();

    let note = publish(&plan, &target, "---\ntype: Document\n---\n\n").unwrap();
    assert!(note.exists());
    // no media subfolder created when there were no images
    assert!(!target.join("2026-07-10 Note").exists());
}

#[test]
fn publish_rolls_back_media_if_note_commit_fails() {
    // The reserved note name is claimed AFTER reservation (the residual
    // post-reservation race): publish must NOT re-suffix (that would break the
    // media links) — it fails and rolls the already-published media dir back.
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Documents/2026/07");
    std::fs::create_dir_all(&target).unwrap();
    let plan = plan_staging(&target, "2026-07-10 Doc", "u");
    std::fs::create_dir_all(&plan.work_dir).unwrap();
    std::fs::write(plan.work_dir.join(&plan.note_name), "# Body\n").unwrap();
    let media = plan.work_dir.join(&plan.media_name);
    std::fs::create_dir_all(&media).unwrap();
    std::fs::write(media.join("image1.png"), b"PNG").unwrap();
    // Claim the exact reserved note name so the non-replacing write fails.
    std::fs::write(target.join("2026-07-10 Doc.md"), "SOMEONE ELSE").unwrap();

    let result = publish(&plan, &target, "---\n---\n\n");
    assert!(result.is_err());
    // original untouched (never clobbered)
    assert_eq!(std::fs::read_to_string(target.join("2026-07-10 Doc.md")).unwrap(), "SOMEONE ELSE");
    // media rolled back — no orphaned sibling folder
    assert!(!target.join("2026-07-10 Doc").exists());
}

#[test]
fn janitor_removes_stale_orphan_staging_dirs_only() {
    use std::time::{Duration, SystemTime};
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("Documents");
    let month = root.join("2026/07");
    std::fs::create_dir_all(&month).unwrap();
    // an orphaned staging dir + a real note + an unrelated dot-dir
    let orphan = month.join(".2026-07-10 Doc.123-4.vault-buddy.tmp.import");
    std::fs::create_dir_all(&orphan).unwrap();
    std::fs::write(orphan.join("partial.md"), "x").unwrap();
    std::fs::write(month.join("2026-07-10 Real.md"), "keep").unwrap();
    let foreign = month.join(".obsidian-cache");
    std::fs::create_dir_all(&foreign).unwrap();

    // now = far future so the orphan is definitely stale
    let now = SystemTime::now() + Duration::from_secs(3600);
    let sweep = clean_stale_staging_at(&root, now, Duration::from_secs(60));
    assert_eq!(sweep.removed.len(), 1);
    assert_eq!(sweep.pending, 0);
    assert!(!orphan.exists());              // owned orphan gone
    assert!(month.join("2026-07-10 Real.md").exists()); // real note kept
    assert!(foreign.exists());             // foreign dot-dir untouched
}

#[test]
fn janitor_keeps_fresh_staging_dirs() {
    use std::time::{Duration, SystemTime};
    let tmp = tempfile::tempdir().unwrap();
    let month = tmp.path().join("Documents/2026/07");
    std::fs::create_dir_all(&month).unwrap();
    let fresh = month.join(".2026-07-10 Doc.9-9.vault-buddy.tmp.import");
    std::fs::create_dir_all(&fresh).unwrap();
    // now ≈ creation time → not yet stale
    let sweep = clean_stale_staging_at(&tmp.path().join("Documents"), SystemTime::now(), Duration::from_secs(600));
    assert!(sweep.removed.is_empty());
    assert_eq!(sweep.pending, 1); // fresh orphan seen → caller must reschedule
    assert!(fresh.exists());
}

#[test]
fn publish_writes_at_the_exact_reserved_name() {
    // publish does NOT suffix — the reserved basename is final so Pandoc's
    // baked-in media links stay valid. A free target writes at the exact name.
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Documents/2026/07");
    std::fs::create_dir_all(&target).unwrap();
    let plan = plan_staging(&target, "2026-07-10 Note", "u");
    std::fs::create_dir_all(&plan.work_dir).unwrap();
    std::fs::write(plan.work_dir.join(&plan.note_name), "NEW\n").unwrap();

    let note = publish(&plan, &target, "---\n---\n\n").unwrap();
    assert_eq!(note, target.join("2026-07-10 Note.md"));
    assert!(std::fs::read_to_string(&note).unwrap().contains("NEW"));
}
```

Add `tempfile` to `[dev-dependencies]` in `src-tauri/core/Cargo.toml` if not already present (it is used by other core tests — confirm; if absent, add `tempfile = "3"`).

- [ ] **Step 2: Run — verify fail**

Run: `cd src-tauri/core && cargo test --lib document_import`
Expected: FAIL (`plan_staging`/`publish` undefined).

- [ ] **Step 3: Implement**

```rust
use crate::capture_note::{write_note_atomic, NOTE_TMP_SUFFIX};
use crate::capture_paths::candidate;

/// Resolve a collision-free basename BEFORE Pandoc runs: walk the ` (N)`
/// suffix scheme until BOTH `<name>.md` and the `<name>/` media folder are
/// free, and use that one name for both. Up-front (not at write time) because
/// Pandoc bakes the media-folder name into image links as it converts — a
/// publish-time re-suffix would leave the note pointing at the wrong folder.
pub fn reserve_basename(target_dir: &Path, basename: &str) -> String {
    for attempt in 1u32.. {
        let name = candidate(basename, attempt);
        let note_free = !target_dir.join(format!("{name}.md")).exists();
        let media_free = !target_dir.join(&name).exists();
        if note_free && media_free {
            return name;
        }
    }
    unreachable!("suffix search always terminates")
}

pub struct StagePlan {
    pub work_dir: PathBuf,
    pub media_name: String,
    pub note_name: String,
}

/// Dot-prefixed temp working dir under `target_dir` (same volume → atomic
/// publish rename; dot-dir → excluded from every vault_walk scan and
/// recovery). `unique` (a per-invocation token from the shell) keeps two
/// imports to the same date from colliding on the temp dir. Media/note names
/// are the FINAL names, so Pandoc's relative-to-note image links stay correct
/// after the publish move.
pub fn plan_staging(target_dir: &Path, basename: &str, unique: &str) -> StagePlan {
    let work_dir = target_dir.join(format!(".{basename}.{unique}{NOTE_TMP_SUFFIX}.import"));
    StagePlan {
        work_dir,
        media_name: basename.to_string(),
        note_name: format!("{basename}.md"),
    }
}

pub fn cleanup_staging(work_dir: &Path) {
    let _ = std::fs::remove_dir_all(work_dir);
}

/// The owned staging-dir marker. Matched by the janitor so it removes ONLY
/// our own crash-orphaned temp dirs, never another tool's dot-directory.
const STAGING_MARKER: &str = ".vault-buddy.tmp.import";

pub fn is_import_staging_dir(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(STAGING_MARKER)
}

/// Outcome of one janitor sweep.
#[derive(Debug, Default, PartialEq)]
pub struct StagingSweep {
    /// Stale orphan staging dirs removed this pass (for logging).
    pub removed: Vec<PathBuf>,
    /// Staging dirs seen that were too FRESH to remove yet. >0 means the
    /// shell janitor must reschedule — a crash-then-immediate-restart leaves
    /// an orphan younger than the staleness window (Codex review).
    pub pending: usize,
}

/// Startup janitor: remove crash-orphaned import staging dirs under a vault's
/// Documents folder (walking its `YYYY/MM` subtree — that's where staging
/// dirs live). Staleness-gated with an injected `now` so a clock jump giving
/// a live dir a future mtime can't make it look stale (mirrors capture's
/// `is_stale_at`). Returns what was removed AND how many fresh orphans remain
/// (so the caller can reschedule).
///
/// **Canonical containment at every level** (Codex review): a symlinked OR
/// junctioned dated subfolder (`Documents/2026`, `2026/07`) must never let the
/// sweep descend or `remove_dir_all` outside the vault. `is_symlink()` alone is
/// insufficient — a Windows directory junction is a reparse point, NOT a
/// symlink. So each descended dir is `canonicalize()`d and required to stay
/// under the canonicalized `documents_root`; anything that fails to canonicalize
/// or escapes is skipped. The caller (the shell janitor) additionally
/// canonical-checks `documents_root` is inside the vault before calling.
pub fn clean_stale_staging_at(
    documents_root: &Path,
    now: std::time::SystemTime,
    stale_after: std::time::Duration,
) -> StagingSweep {
    let mut sweep = StagingSweep::default();
    let Ok(canon_root) = documents_root.canonicalize() else {
        return sweep; // no Documents folder yet → nothing to do
    };
    // Real subdir whose canonical path stays under canon_root — resolves BOTH
    // symlinks and Windows junctions, so neither can redirect the walk out.
    // Returns the CANONICAL path so a real in-place child canonicalizes to
    // itself (the leaf uses that self-equality to reject a symlink/junction
    // named like a staging dir).
    let contained_subdirs = |dir: &Path| -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.path().canonicalize().ok())
            .filter(|c| c.is_dir() && c.starts_with(&canon_root))
            .collect()
    };
    // Documents/<YYYY>/<MM>/.<name>.<unique>.vault-buddy.tmp.import
    for year in contained_subdirs(&canon_root) {
        for month in contained_subdirs(&year) {
            let Ok(entries) = std::fs::read_dir(&month) else { continue };
            for entry in entries.flatten() {
                let path = entry.path(); // <month_canon>/<name>
                let name = entry.file_name().to_string_lossy().into_owned();
                if !is_import_staging_dir(&name) {
                    continue;
                }
                // Delete ONLY a REAL owned in-place staging dir — never a link's
                // target (Codex review). An owned staging dir is a real dir we
                // created, so it canonicalizes to ITSELF (its parent `month` is
                // canonical). A symlink/junction named like one canonicalizes
                // ELSEWHERE — the containment check would still pass for an
                // in-vault target, so remove_dir_all on the resolved target
                // would erase real vault data. `canon == path` rejects it, and
                // we remove `path` (the entry in place), never a resolved target.
                let Ok(canon) = path.canonicalize() else { continue };
                if canon != path || !canon.is_dir() {
                    continue; // symlink/junction/reparse redirect → skip
                }
                // Staleness from the entry's own mtime (no-follow), guarding a
                // future mtime (clock jump).
                let stale = std::fs::symlink_metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|mtime| match now.duration_since(mtime) {
                        Ok(age) => age >= stale_after,
                        Err(_) => false, // mtime in the future → treat as fresh
                    })
                    .unwrap_or(false);
                if stale {
                    if std::fs::remove_dir_all(&path).is_ok() {
                        sweep.removed.push(path);
                    }
                } else {
                    sweep.pending += 1; // fresh orphan → caller reschedules
                }
            }
        }
    }
    sweep
}

/// Publish a completed staging dir into `target_dir`. Prepends `frontmatter`
/// to the staged note, then moves the media dir (if non-empty) then the note,
/// both at the EXACT names reserved up front (no re-suffixing — the suffix was
/// already resolved by `reserve_basename` and Pandoc pinned the links to it).
/// The note write is non-replacing; on failure the already-published media dir
/// is rolled back. Always cleans the work dir before returning.
pub fn publish(plan: &StagePlan, target_dir: &Path, frontmatter: &str) -> std::io::Result<PathBuf> {
    let result = publish_inner(plan, target_dir, frontmatter);
    cleanup_staging(&plan.work_dir);
    result
}

fn publish_inner(
    plan: &StagePlan,
    target_dir: &Path,
    frontmatter: &str,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(target_dir)?;
    let staged_note = plan.work_dir.join(&plan.note_name);
    let body = std::fs::read_to_string(&staged_note)?;
    let full = format!("{frontmatter}{body}");

    // Media first, so the note never resolves to missing images. Only when
    // Pandoc actually extracted something.
    let staged_media = plan.work_dir.join(&plan.media_name);
    let media_has_files = staged_media
        .read_dir()
        .map(|mut it| it.next().is_some())
        .unwrap_or(false);
    let mut published_media: Option<PathBuf> = None;
    if media_has_files {
        let dest = target_dir.join(&plan.media_name);
        // The basename was reserved (note + media dir both free) up front, so
        // dest should be free; a directory here means the name was claimed
        // AFTER reservation — refuse rather than merge/clobber.
        if dest.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "media directory already exists at destination",
            ));
        }
        std::fs::rename(&staged_media, &dest)?;
        published_media = Some(dest);
    }

    // Write the note at the EXACT reserved name (non-replacing). NOT
    // write_note_collision_safe — re-suffixing here would break Pandoc's
    // baked-in media links. If the name was claimed after reservation the
    // write fails; roll the already-published media dir back so a failed
    // import never leaves an orphaned media folder (Codex review).
    let note_path = target_dir.join(&plan.note_name);
    match write_note_atomic(&note_path, &full) {
        Ok(()) => Ok(note_path),
        Err(e) => {
            if let Some(media) = published_media {
                let _ = std::fs::remove_dir_all(&media);
            }
            Err(e)
        }
    }
}
```

**Note for implementer:** `reserve_basename` (Step 3) already guaranteed both
`<name>.md` and `<name>/` were free, so the common "same document, same date,
again" case gets its own ` (N)` name up front and never reaches the
`AlreadyExists` guards here. Those guards are the backstop for the residual
post-reservation race only (a sync client / hand-created file landing between
reservation and publish), which fails cleanly with a rollback.

- [ ] **Step 4: Run — verify pass**

Run: `cd src-tauri/core && cargo test --lib document_import && cargo clippy --all-targets -- -D warnings`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/document_import.rs src-tauri/core/Cargo.toml
git commit -m "feat(core): document-import staging + collision-safe publish"
```

---

## Task 4: Shell — Pandoc detection

**Files:**
- Create: `src-tauri/src/document_commands.rs` (detection half)
- Modify: `src-tauri/Cargo.toml` — Windows-only `winreg`
- Modify: shell module list (`src-tauri/src/lib.rs` or `main.rs`) — `mod document_commands;`
- Test: inline `#[cfg(test)]` for the pure helpers (version parse, sandbox threshold, PATH merge).

**Interfaces:**
- Produces:
  - `#[derive(Serialize)] struct PandocStatus { installed: bool, version: Option<String>, path: Option<String>, sandbox_supported: bool }` (camelCase).
  - `#[tauri::command] async fn detect_pandoc() -> PandocStatus`.
  - helpers: `fn parse_pandoc_version(stdout: &str) -> Option<(u32,u32)>`, `fn sandbox_supported(major: u32, minor: u32) -> bool` (≥ 2.15), `fn merged_path(base: &str, extra: &[String]) -> String`, `fn pandoc_candidates() -> Vec<String>` (override→PATH, deduped), `fn probe_pandoc(program) -> Option<(String,u32,u32)>`, `fn resolve_working_pandoc() -> Option<(String,u32,u32,String)>` (first candidate that runs — the override→PATH fallback both `detect_pandoc` and `convert_document` share).

- [ ] **Step 1: Write failing tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pandoc_version_line() {
        assert_eq!(parse_pandoc_version("pandoc 3.1.9\nCompiled with..."), Some((3, 1)));
        assert_eq!(parse_pandoc_version("pandoc.exe 2.14.2"), Some((2, 14)));
        assert_eq!(parse_pandoc_version("not pandoc"), None);
    }

    #[test]
    fn sandbox_requires_2_15_or_newer() {
        assert!(!sandbox_supported(2, 14));
        assert!(sandbox_supported(2, 15));
        assert!(sandbox_supported(3, 1));
        assert!(sandbox_supported(2, 20));
    }

    #[test]
    fn merged_path_appends_registry_entries_without_dupes() {
        let merged = merged_path("/usr/bin:/bin", &["/usr/bin".into(), "/opt/pandoc".into()]);
        assert!(merged.contains("/opt/pandoc"));
        // existing entry not duplicated
        assert_eq!(merged.matches("/usr/bin").count(), 1);
    }
}
```

- [ ] **Step 2: Run — verify fail**

Run: `cd src-tauri && cargo test -p vault-buddy --lib document_commands`
Expected: FAIL (module absent). (If the shell lib test target isn't built yet, this fails to compile — that's the failing state.)

- [ ] **Step 3: Implement the detection half**

```rust
//! Document Import IPC: Pandoc detection + conversion + settings.
//! Detection re-reads PATH from the Windows registry so Recheck sees a
//! just-installed Pandoc without an app restart. Conversion runs Pandoc
//! sandboxed + heap-capped under spawn_blocking (async command, like
//! search_vaults). Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use std::process::Command;
use vault_buddy_core::capture_config;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PandocStatus {
    pub installed: bool,
    pub version: Option<String>,
    pub path: Option<String>,
    pub sandbox_supported: bool,
    /// The raw configured override (None → using PATH), so the settings
    /// field can seed itself without a second command.
    pub configured_path: Option<String>,
}

/// First line of `pandoc --version` is `pandoc <x.y.z>`; return (major, minor).
fn parse_pandoc_version(stdout: &str) -> Option<(u32, u32)> {
    let first = stdout.lines().next()?;
    let ver = first.split_whitespace().nth(1)?;
    let mut parts = ver.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

/// `--sandbox` landed in Pandoc 2.15.
fn sandbox_supported(major: u32, minor: u32) -> bool {
    (major, minor) >= (2, 15)
}

/// Append `extra` PATH entries not already present (case-insensitive on the
/// separator platform). Keeps the process PATH first.
fn merged_path(base: &str, extra: &[String]) -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut out: Vec<String> = base.split(sep).map(str::to_string).collect();
    for e in extra {
        if !out.iter().any(|p| p.eq_ignore_ascii_case(e)) {
            out.push(e.clone());
        }
    }
    out.join(&sep.to_string())
}

/// Windows: read user + machine PATH from the registry so a just-installed
/// Pandoc is visible without restarting (a running process keeps its launch
/// PATH snapshot). Non-Windows: nothing extra (the compile gate + tests).
#[cfg(windows)]
fn registry_path_entries() -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut entries = Vec::new();
    let reads = [
        (HKEY_CURRENT_USER, "Environment"),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ];
    for (hive, sub) in reads {
        if let Ok(key) = RegKey::predef(hive).open_subkey(sub) {
            if let Ok(path) = key.get_value::<String, _>("Path") {
                entries.extend(path.split(';').map(str::to_string));
            }
        }
    }
    entries
}

#[cfg(not(windows))]
fn registry_path_entries() -> Vec<String> {
    Vec::new()
}

/// Ordered pandoc candidates to try: the configured override FIRST (if
/// non-empty), then the bare `pandoc` PATH lookup. Both are probed in order so
/// a stale/mistyped override does NOT hide a valid Pandoc on PATH — detection
/// falls through to PATH before reporting Not Installed (the settings contract
/// promises the override is checked first, *falling back* to PATH). Deduped so
/// an override literally equal to `pandoc` isn't probed twice.
fn pandoc_candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = capture_config::load_config()
        .document_import
        .pandoc_path
        .filter(|p| !p.trim().is_empty())
    {
        out.push(p);
    }
    if !out.iter().any(|c| c == "pandoc") {
        out.push("pandoc".to_string());
    }
    out
}

/// Build a Command with the registry-augmented PATH so PATH lookup sees a
/// fresh install.
fn pandoc_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    let base = std::env::var("PATH").unwrap_or_default();
    let extra = registry_path_entries();
    if !extra.is_empty() {
        cmd.env("PATH", merged_path(&base, &extra));
    }
    cmd
}

/// Probe one candidate: run `<program> --version`. On success, return the
/// program string with its parsed (major, minor). None if it can't run or
/// exits non-zero (so the caller falls through to the next candidate).
fn probe_pandoc(program: &str) -> Option<(String, u32, u32)> {
    let out = pandoc_command(program).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (major, minor) = parse_pandoc_version(&stdout)?;
    Some((program.to_string(), major, minor))
}

/// The first `--version` line of a program known to run, for display.
fn pandoc_version_line(program: &str) -> String {
    pandoc_command(program)
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8_lossy(&o.stdout).lines().next().map(|l| l.trim().to_string()))
        .unwrap_or_default()
}

/// Resolve a pandoc to use across the ordered candidates (override → PATH).
/// **Prefer a sandbox-capable (≥ 2.15) candidate** (Codex review): a
/// working-but-old override must not shadow a supported Pandoc on PATH —
/// returning the old one would make convert_document reject it at the sandbox
/// gate and never probe PATH. Keep probing past a too-old runnable candidate
/// and return the first sandbox-capable one; only if NONE is sandbox-capable
/// return the first runnable (old) one, so detect_pandoc reports an accurate
/// "installed but too old" (and convert_document still rejects it).
fn resolve_working_pandoc() -> Option<(String, u32, u32, String)> {
    let mut too_old: Option<(String, u32, u32)> = None;
    for program in pandoc_candidates() {
        if let Some((prog, major, minor)) = probe_pandoc(&program) {
            if sandbox_supported(major, minor) {
                let line = pandoc_version_line(&prog);
                return Some((prog, major, minor, line));
            }
            too_old.get_or_insert((prog, major, minor));
        }
    }
    too_old.map(|(prog, major, minor)| {
        let line = pandoc_version_line(&prog);
        (prog, major, minor, line)
    })
}

/// Detect Pandoc on demand (settings-open + Recheck). Async + spawn_blocking:
/// spawning a subprocess is blocking I/O and must stay off the main thread.
#[tauri::command]
pub async fn detect_pandoc() -> PandocStatus {
    tauri::async_runtime::spawn_blocking(|| {
        let configured = capture_config::load_config()
            .document_import
            .pandoc_path
            .filter(|p| !p.trim().is_empty());
        // Try the override, then PATH — a stale override must not hide a valid
        // PATH Pandoc (Codex review).
        match resolve_working_pandoc() {
            Some((program, major, minor, version_line)) => PandocStatus {
                installed: true,
                version: Some(version_line),
                path: Some(program),
                sandbox_supported: sandbox_supported(major, minor),
                configured_path: configured,
            },
            None => PandocStatus {
                installed: false,
                version: None,
                path: None,
                sandbox_supported: false,
                configured_path: configured,
            },
        }
    })
    .await
    .unwrap_or(PandocStatus {
        installed: false,
        version: None,
        path: None,
        sandbox_supported: false,
        configured_path: None,
    })
}
```

Add a unit test for the fallback ordering (pure — no real Pandoc needed):
```rust
#[test]
fn candidates_try_override_then_path_deduped() {
    // With no override configured, only "pandoc" is probed.
    // (pandoc_candidates reads config; in a test env with no config file it
    // returns just ["pandoc"].)
    assert_eq!(pandoc_candidates(), vec!["pandoc".to_string()]);
}
```
(If `pandoc_candidates` can't be made deterministic under test because it
reads the real config path, extract the ordering into a pure
`candidate_order(override: Option<&str>) -> Vec<String>` helper and test that
instead — the override→PATH→dedup logic is the part worth pinning.)

Add to `src-tauri/Cargo.toml`:
```toml
[target.'cfg(windows)'.dependencies]
winreg = "0.52"
```
(If a `[target.'cfg(windows)'.dependencies]` table already exists, add `winreg` to it.)

Add `mod document_commands;` beside the other `mod` lines in the shell crate root.

- [ ] **Step 4: Run — verify pass + compile gate**

Run: `cd src-tauri && cargo test -p vault-buddy --lib document_commands`
Then the Linux compile gate (once): `npm run setup:linux && npx tauri build --no-bundle`
Also: `cargo machete .` (winreg is used only under cfg(windows) — machete may flag it; if so, it's a known false positive for target-gated deps — verify machete passes, and if it flags winreg, add an `ignored` entry in the `[package.metadata.cargo-machete]` list with a comment).
Expected: tests PASS; build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/document_commands.rs src-tauri/Cargo.toml src-tauri/src/lib.rs
git commit -m "feat(shell): detect Pandoc (version, sandbox support, registry PATH)"
```

---

## Task 5: Shell — `convert_document` command

**Files:**
- Modify: `src-tauri/src/document_commands.rs`
- Test: inline `#[cfg(test)]` for the arg-builder helper (pure).

**Interfaces:**
- Consumes: Task 2/3 core fns; `document_import::{DocFormat, DocMeta, document_basename, render_frontmatter, target_dir, plan_staging, publish, cleanup_staging}`; `capture_paths::{safe_recording_root, assert_path_inside_vault}`; `discovery::discover_vaults`; `capture_config`.
- Produces:
  - `struct ImportLock(Arc<Mutex<()>>)` app state (process-wide conversion serialization), registered in lib.rs via `.manage(...)`.
  - `#[tauri::command] async fn convert_document(lock: State<ImportLock>, id: String, source_path: String) -> Result<String, String>` — takes the lock via `try_lock` (fail-fast on a concurrent import), re-validates folder containment, returns the published note's vault-relative path.
  - pure `fn pandoc_args(reader: &str, media_name: &str, note_name: &str) -> Vec<String>` returning the exact arg vector (sans program), so the sandbox/heap/relative-output contract is unit-tested.

- [ ] **Step 1: Write failing test**

```rust
#[test]
fn pandoc_args_are_sandboxed_relative_and_heap_capped() {
    let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md");
    // reader
    assert!(args.windows(2).any(|w| w == ["-f", "docx"]));
    assert!(args.windows(2).any(|w| w == ["-t", "gfm"]));
    // sandbox always present
    assert!(args.iter().any(|a| a == "--sandbox"));
    // relative extract-media + output (no temp path baked in)
    assert!(args.iter().any(|a| a == "--extract-media=2026-07-10 Report"));
    assert!(args.windows(2).any(|w| w == ["-o", "2026-07-10 Report.md"]));
    // heap cap
    let joined = args.join(" ");
    assert!(joined.contains("+RTS -M512M -RTS"));
}
```

- [ ] **Step 2: Run — verify fail**

Run: `cd src-tauri && cargo test -p vault-buddy --lib document_commands`
Expected: FAIL (`pandoc_args` undefined).

- [ ] **Step 3: Implement**

```rust
use std::path::Path;
use std::time::Duration;
use vault_buddy_core::{discovery, document_import};

/// Pandoc argument vector (program excluded). Source is added by the caller as
/// an absolute path; every OUTPUT here is relative (Pandoc runs with cwd =
/// work dir) so rewritten image links stay valid after publish.
fn pandoc_args(reader: &str, media_name: &str, note_name: &str) -> Vec<String> {
    vec![
        "-f".into(),
        reader.into(),
        "-t".into(),
        "gfm".into(),
        "--sandbox".into(),
        format!("--extract-media={media_name}"),
        "-o".into(),
        note_name.into(),
        // GHC RTS heap cap: a timeout bounds time, not memory; a crafted doc
        // could OOM before it fires. Pandoc dies with a memory error instead.
        "+RTS".into(),
        "-M512M".into(),
        "-RTS".into(),
    ]
}

/// Max wall-clock for a single conversion before the child is killed.
const CONVERT_TIMEOUT: Duration = Duration::from_secs(120);

/// Process-wide serialization for imports. A `try_lock` (not blocking) so a
/// second concurrent import fails fast instead of racing step 1's
/// exists-reservation into a corrupt/partial publish. The inner mutex is an
/// `Arc` so its guard can be held on the `spawn_blocking` thread (Tauri
/// `State` itself can't cross that boundary). Registered as app state in
/// lib.rs beside ConfigWriteLock: `.manage(ImportLock::default())`.
#[derive(Default, Clone)]
pub struct ImportLock(pub std::sync::Arc<std::sync::Mutex<()>>);

/// Monotonic per-invocation token so two same-date imports can't collide on
/// the staging dir name even across the (lock-serialized) boundary.
static IMPORT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[tauri::command]
pub async fn convert_document(
    lock: tauri::State<'_, ImportLock>,
    id: String,
    source_path: String,
) -> Result<String, String> {
    // Take the process-wide import lock BEFORE spawning the blocking job. A
    // failed try_lock means another import is mid-flight — fail fast rather
    // than race. The guard is moved into the blocking closure so it's held
    // for the whole convert-and-publish body and dropped when it returns.
    // (State can't cross the spawn_blocking boundary, so clone the inner Arc
    // via a dedicated Arc<Mutex<()>> — see the lib.rs wiring note below.)
    let today = chrono::Local::now().date_naive();
    let today_str = today.format("%Y-%m-%d").to_string();
    let year = today.format("%Y").to_string();
    let month = today.format("%m").to_string();
    let seq = IMPORT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let unique = format!("{}-{}", std::process::id(), seq);

    // The lock is an Arc<Mutex<()>> so its guard can live on the blocking
    // thread; ImportLock stores that Arc (see the struct note).
    let mutex = lock.0.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = match mutex.try_lock() {
            Ok(g) => g,
            Err(_) => return Err("An import is already in progress.".to_string()),
        };
        convert_blocking(&id, &source_path, &today_str, &year, &month, &unique)
    })
    .await
    .map_err(|e| {
        log::warn!("convert_document: task failed: {e}");
        "Import failed — see the logs for details.".to_string()
    })?
}

fn convert_blocking(
    id: &str,
    source_path: &str,
    today: &str,
    year: &str,
    month: &str,
    unique: &str,
) -> Result<String, String> {
    let src = Path::new(source_path);
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .ok_or("Unsupported file — expected .docx, .odt, or .rtf")?;
    let format = document_import::DocFormat::from_extension(ext)
        .ok_or("Unsupported file — expected .docx, .odt, or .rtf")?;
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Could not read the file name")?;

    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;

    // Resolve Pandoc synchronously here (we're already on a blocking thread):
    // override first, then PATH — a stale override must not hide a valid PATH
    // Pandoc (Codex review). This is the SAME resolution detect_pandoc uses.
    let (program, major, minor, _) = resolve_working_pandoc()
        .ok_or("Pandoc is not installed. Install it from Settings → Document Import.")?;
    if !sandbox_supported(major, minor) {
        return Err("Your Pandoc is too old to import safely (need 2.15+). Please update it.".into());
    }

    let cfg = capture_config::vault_config(&capture_config::load_config(), id);
    let documents_folder = cfg.documents_root().to_string();
    // Re-validate containment even though set_documents_config already did:
    // config.json is hand-editable, so a `../…` or symlink-escaping folder
    // must be caught here too before any staging dir is created (Codex
    // review). Same lexical + canonical check the save path uses.
    let vault_root = Path::new(&vault.path);
    let safe = vault_buddy_core::capture_paths::safe_recording_root(vault_root, &documents_folder)?;
    vault_buddy_core::capture_paths::assert_path_inside_vault(vault_root, &safe)?;
    let dir = document_import::target_dir(vault_root, &documents_folder, year, month);
    // Resolve the ` (N)` suffix for BOTH note and media folder up front — the
    // target dir must exist for the existence checks, and Pandoc bakes the
    // media-folder name into image links, so it can't be decided at publish
    // time (Codex review).
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not prepare import: {e}"))?;
    // Re-validate the FULLY DATED dir after creating it — the folder-root
    // check above is lexical and can't see a `Documents/2026` or `2026/07`
    // symlink/junction that escapes the vault. `start_capture` guards its
    // dated folder the same way after create_dir_all (Codex review): a
    // canonical containment check on the concrete path so staging + publish
    // can't land outside the vault through a nested date-folder link.
    vault_buddy_core::capture_paths::assert_path_inside_vault(vault_root, &dir)?;
    let raw = document_import::document_basename(stem, today);
    let basename = document_import::reserve_basename(&dir, &raw);
    let plan = document_import::plan_staging(&dir, &basename, unique);

    // Fresh staging dir.
    document_import::cleanup_staging(&plan.work_dir);
    std::fs::create_dir_all(&plan.work_dir).map_err(|e| format!("Could not prepare import: {e}"))?;

    let args = pandoc_args(format.reader(), &plan.media_name, &plan.note_name);
    let mut cmd = pandoc_command(&program);
    cmd.current_dir(&plan.work_dir)
        .arg(src) // absolute source
        .args(&args);

    let run = run_with_timeout(cmd, CONVERT_TIMEOUT);
    match run {
        Ok(true) => {}
        Ok(false) => {
            document_import::cleanup_staging(&plan.work_dir);
            return Err("Pandoc could not convert this document.".into());
        }
        Err(e) => {
            document_import::cleanup_staging(&plan.work_dir);
            log::warn!("convert_document: pandoc run failed: {e}");
            return Err("Pandoc could not convert this document.".into());
        }
    }

    let meta = document_import::DocMeta {
        source_path: source_path.to_string(),
        imported: today.to_string(),
        format,
    };
    let frontmatter = document_import::render_frontmatter(&meta);
    let note = document_import::publish(&plan, &dir, &frontmatter)
        .map_err(|e| format!("Could not save the imported note: {e}"))?;

    // Vault-relative path for the caller (best-effort; absolute on failure).
    let rel = note
        .strip_prefix(&vault.path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| note.to_string_lossy().to_string());
    Ok(rel)
}

/// Spawn + wait with a wall-clock kill. Returns Ok(true) on success exit,
/// Ok(false) on non-zero/killed, Err on spawn failure.
fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::io::Result<bool> {
    let mut child = cmd.spawn()?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}
```

**Note:** `chrono` is already a shell dependency (used by `task_commands`/capture). No new dep.

- [ ] **Step 4: Run — verify pass + gate**

Run: `cd src-tauri && cargo test -p vault-buddy --lib document_commands && cargo clippy -p vault-buddy --all-targets -- -D warnings`
Then compile gate: `npx tauri build --no-bundle`
Expected: PASS + build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/document_commands.rs
git commit -m "feat(shell): convert_document — sandboxed, heap-capped, staged import"
```

---

## Task 5b: Shell — startup import janitor (crash-orphan cleanup)

**Files:**
- Modify: `src-tauri/src/document_commands.rs` — `run_import_recovery`
- Modify: `src-tauri/src/lib.rs` — call it in `setup` (near `run_recovery`)
- Test: none new (the pure sweep is tested in Task 3; this is thin wiring — the build gate covers it).

**Interfaces:**
- Consumes: `document_import::{clean_stale_staging_at, StagingSweep}`, `discovery::discover_vaults`, `capture_config`, `capture_paths::{safe_recording_root, assert_path_inside_vault}`, `ImportLock`.
- Produces: `fn run_import_recovery(app: &AppHandle)` — a named background thread that sweeps each vault's Documents folder for crash-orphaned staging dirs, postponed while an import is active, **and reschedules while fresh orphans age** (mirrors capture's retry loop).

**Why:** a hard kill / crash / power loss mid-Pandoc leaves the in-vault
`.…vault-buddy.tmp.import` dir behind (`cleanup_staging` never ran). The
vault-walk scans skip it (dot-dir) so it's invisible, but nothing removes
it — capture has an analogous `.part` recovery pass; imports need the same
(Codex review). And like capture, one pass isn't enough: a crash-then-
immediate-restart leaves an orphan younger than the staleness window, so the
pass must retry as fresh work ages, not exit after a single sweep (Codex
review).

- [ ] **Step 1: Implement**

```rust
use tauri::{AppHandle, Manager};

/// Staleness floor: only sweep staging dirs older than this, so a live
/// conversion's fresh dir is never touched even if the ImportLock check
/// somehow raced. 10 min is comfortably longer than any real conversion.
const IMPORT_STAGING_STALE: std::time::Duration = std::time::Duration::from_secs(600);
/// Retry cadence while work is pending (a postponed pass, or a fresh orphan
/// not yet stale) — mirrors capture recovery's 90s retry.
const IMPORT_RECOVERY_RETRY: std::time::Duration = std::time::Duration::from_secs(90);
/// Bound the retries (~24h), so a permanently-fresh anomaly can't loop forever.
const IMPORT_RECOVERY_MAX_PASSES: u32 = 960;

/// Startup janitor for crash-orphaned import staging dirs. Named background
/// thread. One `pass()` returns whether work is still pending (postponed, or a
/// fresh orphan seen); while pending, it retries every IMPORT_RECOVERY_RETRY
/// so an orphan younger than the staleness window at boot is still reaped once
/// it ages — exactly the capture-recovery shape.
pub fn run_import_recovery(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("import-recovery".into())
        .spawn(move || {
            let pass = || -> bool {
                // Postpone the WHOLE pass while a conversion runs: try the same
                // lock convert takes. If we can't get it, an import is mid-flight
                // and its fresh staging dir must not be swept — retry later.
                let lock = app.state::<ImportLock>();
                let Ok(_guard) = lock.0.try_lock() else {
                    log::info!("import-recovery: postponed while an import is active");
                    return true; // pending → retry
                };
                let cfg = capture_config::load_config();
                let mut pending = false;
                for vault in discovery::discover_vaults() {
                    let v = capture_config::vault_config(&cfg, &vault.id);
                    let folder = v.documents_root();
                    let vault_root = std::path::Path::new(&vault.path);
                    let Ok(root) = capture_paths::safe_recording_root(vault_root, folder) else {
                        continue;
                    };
                    if !root.is_dir() {
                        continue;
                    }
                    // Canonical containment before we DELETE anything: the
                    // safe_recording_root check is lexical, so a symlinked/
                    // junctioned Documents folder could point the sweep outside
                    // the vault. (clean_stale_staging_at also canonical-checks
                    // every dated level — symlinks AND Windows junctions.)
                    if capture_paths::assert_path_inside_vault(vault_root, &root).is_err() {
                        log::warn!("import-recovery: skipping root outside vault: {root:?}");
                        continue;
                    }
                    let sweep = vault_buddy_core::document_import::clean_stale_staging_at(
                        &root,
                        std::time::SystemTime::now(),
                        IMPORT_STAGING_STALE,
                    );
                    for dir in sweep.removed {
                        log::info!("import-recovery: removed orphaned staging dir {dir:?}");
                    }
                    if sweep.pending > 0 {
                        pending = true; // fresh orphan → retry after it ages
                    }
                }
                pending
            };
            // Retry while work is pending, bounded. A clean pass (no orphans,
            // not postponed) ends the thread.
            for _ in 0..IMPORT_RECOVERY_MAX_PASSES {
                if !pass() {
                    return;
                }
                std::thread::sleep(IMPORT_RECOVERY_RETRY);
            }
            log::warn!("import-recovery: gave up after max passes with work still pending");
        })
        .map(|_| ())
        .unwrap_or_else(|e| log::warn!("import-recovery: could not spawn thread: {e}"));
}
```

In `src-tauri/src/lib.rs` `setup`, after the existing capture `run_recovery(app)` call:
```rust
document_commands::run_import_recovery(app.handle());
```

- [ ] **Step 2: Build gate**

Run: `cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings && npx tauri build --no-bundle`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/document_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): startup janitor for crash-orphaned import staging dirs"
```

---

## Task 6: Shell — settings commands + register all commands

**Design correction (Codex review):** the per-vault Documents Folder and the
app-global Pandoc path are **separate scopes** and must have separate
commands, or the folder control ends up in a view with no vault id. So this
task produces a **per-vault** `get_documents_config`/`set_documents_config`
pair — mirroring `get_tasks_config`/`set_tasks_config` *exactly* (id-scoped,
folder only, no Pandoc) — and an **app-global** `set_pandoc_path`. The
configured override is read back via `detect_pandoc().configuredPath` (Task
4), so no separate getter is needed.

**Files:**
- Modify: `src-tauri/src/document_commands.rs` (config get/set)
- Modify: `src-tauri/src/capture_commands.rs` — `set_capture_config` must PRESERVE `documents_folder` (read-modify-write), exactly as it already preserves `tasks_folder`; and the `CaptureConfig` DTO get/set must round-trip `documentsFolder`.
- Modify: `src-tauri/src/lib.rs` — register in `generate_handler!` + `.manage`.
- Test: none new (logic is thin over Task 1, already tested); register-and-build is the gate.

**Interfaces:**
- Consumes: `capture_config`, `ConfigWriteLock`, `capture_paths` for folder validation.
- Produces:
  - `#[derive(Serialize)] struct DocumentsConfigDto { documents_folder: Option<String> }` (per-vault, camelCase).
  - `#[tauri::command] fn get_documents_config(id: String) -> DocumentsConfigDto`.
  - `#[tauri::command] fn set_documents_config(lock, id: String, documents_folder: Option<String>) -> Result<(), String>` — validates the folder inside the vault (like `set_tasks_config`), writes only the per-vault `documents_folder`.
  - `#[tauri::command] fn set_pandoc_path(lock, pandoc_path: Option<String>) -> Result<(), String>` — app-global; writes `document_import.pandoc_path`.

- [ ] **Step 1: Implement (no new unit test; covered by Task 1 + build)**

```rust
use std::path::Path as StdPath;
use vault_buddy_core::capture_paths;
use vault_buddy_core::sync_util::lock_ignoring_poison;
use crate::capture_commands::ConfigWriteLock;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentsConfigDto {
    pub documents_folder: Option<String>,
}

/// Per-vault documents folder (or None → the frontend shows the "Documents"
/// default). Unknown vault → None, never an error. Mirrors get_tasks_config.
#[tauri::command]
pub fn get_documents_config(id: String) -> DocumentsConfigDto {
    let vault = capture_config::vault_config(&capture_config::load_config(), &id);
    DocumentsConfigDto {
        documents_folder: vault.documents_folder,
    }
}

/// Persist the vault's documents folder. Validates containment BEFORE writing
/// (the effective folder — explicit or the "Documents" default — must stay in
/// the vault), serialized behind ConfigWriteLock. Read-modify-write preserves
/// the vault's other config. Mirrors set_tasks_config exactly.
#[tauri::command]
pub fn set_documents_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    documents_folder: Option<String>,
) -> Result<(), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let folder = documents_folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .map(str::to_string);
    let effective = folder.as_deref().unwrap_or("Documents");
    let root = capture_paths::safe_recording_root(StdPath::new(&vault.path), effective)?;
    capture_paths::assert_path_inside_vault(StdPath::new(&vault.path), &root)?;
    let _guard = lock_ignoring_poison(&lock.0);
    let mut v = capture_config::vault_config(&capture_config::load_config(), &id);
    v.documents_folder = folder;
    capture_config::update_vault_config(&id, v)
}

/// App-global Pandoc path override (None → PATH lookup). Serialized behind
/// ConfigWriteLock; round-tripped by serialize_config (Task 1).
#[tauri::command]
pub fn set_pandoc_path(
    lock: tauri::State<ConfigWriteLock>,
    pandoc_path: Option<String>,
) -> Result<(), String> {
    let path = pandoc_path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    let _guard = lock_ignoring_poison(&lock.0);
    capture_config::update_document_import_config(
        vault_buddy_core::capture_config::DocumentImportConfig { pandoc_path: path },
    )
}
```

**`set_capture_config` must preserve `documents_folder`.** In
`capture_commands.rs`, `set_capture_config` reconstructs a `VaultCaptureConfig`
from its DTO; it already re-reads `tasks_folder` under the lock so a capture
save can't reset it (AGENTS.md invariant). Add `documents_folder` to that same
preserve step. Also add `documentsFolder` to the `CaptureConfig` DTO's get/set
mapping so the field round-trips (Task 7's `CaptureConfig` type adds the TS
side); the vault-scoped `CaptureSettings.vue` reads it via `get_documents_config`
and writes it via `set_documents_config` (its own command pair, like the Tasks
folder), so the capture DTO carrying it is only for round-trip preservation.

Register in `src-tauri/src/lib.rs` `generate_handler!` (after the `mcp_commands`
block) the commands that exist by now — `begin_document_import` /
`take_pending_import` are added in Task 9 where they're implemented:
```rust
document_commands::detect_pandoc,
document_commands::convert_document,
document_commands::get_documents_config,
document_commands::set_documents_config,
document_commands::set_pandoc_path,
```

Also register the import lock app state in the builder (beside the other
`.manage(...)` — `DocumentImportPending` is added in Task 9):
```rust
.manage(document_commands::ImportLock::default())
```

- [ ] **Step 2: Build gate**

Run: `cd src-tauri && cargo fmt && cargo clippy -p vault-buddy --all-targets -- -D warnings && npx tauri build --no-bundle`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/document_commands.rs src-tauri/src/capture_commands.rs src-tauri/src/lib.rs
git commit -m "feat(shell): document-import settings commands + preserve documents_folder"
```

---

## Task 7: Frontend — Pandoc settings (app-global) + Documents Folder (per-vault)

**Two surfaces, matching the two scopes (Codex review):** app-global Pandoc
status/path lives in `DocumentImportSettings.vue` (Buddy settings, no vault
context); the per-vault Documents Folder lives in `CaptureSettings.vue`
(vault-scoped, already has `vaultId`), beside its existing Tasks Folder
control.

**Files:**
- Modify: `src/types.ts` — `PandocStatus` type; add `documentsFolder` to `CaptureConfig`.
- Create: `src/components/DocumentImportSettings.vue` — app-global Pandoc only.
- Modify: `src/components/BuddySettings.vue` — embed `DocumentImportSettings`.
- Modify: `src/components/CaptureSettings.vue` — add the per-vault Documents Folder input.
- Test: `tests/documentImport.test.ts` (settings portion).

**Interfaces:**
- `DocumentImportSettings.vue` consumes: `detect_pandoc` → `PandocStatus` (incl. `configuredPath`), `set_pandoc_path`. No vault id — none of its state is per-vault.
- `CaptureSettings.vue` consumes (added): `get_documents_config(id)`, `set_documents_config(id, documentsFolder)` — the same shape as its existing `get_tasks_config`/`set_tasks_config` calls.

- [ ] **Step 1: Write failing test**

```ts
import { mount } from "@vue/test-utils";
import { mockIPC } from "@tauri-apps/api/mocks";
import { describe, expect, it } from "vitest";
import DocumentImportSettings from "../src/components/DocumentImportSettings.vue";

describe("DocumentImportSettings", () => {
  it("shows Not Installed and re-detects on Recheck", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return calls === 1
          ? { installed: false, version: null, path: null, sandboxSupported: false, configuredPath: null }
          : { installed: true, version: "pandoc 3.1.9", path: "pandoc", sandboxSupported: true, configuredPath: null };
      }
      return undefined;
    });
    const wrapper = mount(DocumentImportSettings);
    await new Promise((r) => setTimeout(r));
    expect(wrapper.text()).toContain("Not installed");
    await wrapper.get('[data-testid="pandoc-recheck"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(wrapper.text()).toContain("3.1.9");
  });
});
```

- [ ] **Step 2: Run — verify fail**

Run: `npx vitest run tests/documentImport.test.ts`
Expected: FAIL (component missing).

- [ ] **Step 3: Implement types + components**

In `src/types.ts` add:
```ts
export type PandocStatus = {
  installed: boolean;
  version: string | null;
  path: string | null;
  sandboxSupported: boolean;
  configuredPath: string | null;
};
```
And add `documentsFolder: string | null;` to the existing `CaptureConfig`
type (round-tripped by get/set_capture_config, mirroring `tasksFolder`).

Create `src/components/DocumentImportSettings.vue` (app-global only) mirroring
`McpSettings.vue`:
- `onMounted`: `detect_pandoc` → status; seed the path-override field from `status.configuredPath`.
- `recheck()`: re-run `detect_pandoc` (in-flight guarded), `data-testid="pandoc-recheck"`.
- Status line: `!installed` → "Not installed"; installed but `!sandboxSupported` → "Installed (version) — too old for safe import (need 2.15+)"; else "Installed (version)".
- Install link: anchor to `https://pandoc.org/installing.html`, opened via `@tauri-apps/plugin-shell`'s `open` if present, else `<a target="_blank">`.
- Path override input + "Browse…" using `@tauri-apps/plugin-dialog`'s `open({ multiple:false, filters:[{ name:"Pandoc", extensions:["exe",""] }] })` (Task 0 wired the plugin); on change call `set_pandoc_path` (in-flight guarded), then re-`detect_pandoc` so the new path resolves. **No folder input here.**
- On failure: `logWarning` + inline error line (mirror McpSettings).

Embed in `BuddySettings.vue` below the MCP section.

In `CaptureSettings.vue`, add a **Documents Folder** text input beside the
existing Tasks Folder control (it already has `props.vaultId`): load via
`get_documents_config({ id: props.vaultId })` in the same place it loads the
tasks config, and save via `set_documents_config({ id: props.vaultId,
documentsFolder })` in its save flow — copy the Tasks Folder control's
markup, load, save, and error handling verbatim, swapping the command names
and label. Placeholder "Documents".

- [ ] **Step 4: Run — verify pass + typecheck**

Run: `npx vitest run tests/documentImport.test.ts && npm run build`
Expected: PASS + typecheck clean.

- [ ] **Step 5: Commit**

```bash
git add src/types.ts src/components/DocumentImportSettings.vue src/components/BuddySettings.vue src/components/CaptureSettings.vue tests/documentImport.test.ts
git commit -m "feat(ui): app-global Pandoc settings + per-vault Documents Folder"
```

---

## Task 8: Frontend — "Import Document" action in the record chooser

**Files:**
- Modify: `src/components/RecordMode.vue`
- Modify: `tests/documentImport.test.ts` (add a case)

**Interfaces:**
- Consumes IPC: `detect_pandoc`, `convert_document`; `@tauri-apps/plugin-dialog` `open`.
- Produces: an "Import Document" button below "Browse recordings", disabled with a Settings hint when Pandoc is absent; on click opens a file picker (filters docx/odt/rtf), calls `convert_document`, toasts success/failure.

- [ ] **Step 1: Write failing test**

```ts
it("imports a picked document and toasts success", async () => {
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(cmd);
    if (cmd === "detect_pandoc") return { installed: true, version: "pandoc 3.1", path: "pandoc", sandboxSupported: true };
    if (cmd === "get_capture_config") return { /* CaptureConfig defaults */ };
    if (cmd === "list_recordings") return [];
    if (cmd === "convert_document") return "Documents/2026/07/2026-07-10 Report.md";
    return undefined;
  });
  // mock plugin-dialog open() to return a fake path — see vi.mock pattern in existing tests
  // assert convert_document was called with { id, sourcePath } and a success toast enqueued.
});
```
(Follow the `vi.mock("@tauri-apps/plugin-dialog", ...)` + `vi.hoisted` pattern already used in the suite for plugin mocks.)

- [ ] **Step 2: Run — verify fail**

Run: `npx vitest run tests/documentImport.test.ts`
Expected: FAIL (button/flow absent).

- [ ] **Step 3: Implement**

In `RecordMode.vue`:
- On mount, `detect_pandoc()` → a `pandoc` ref (guard: swallow errors → treated as not installed, `logWarning`).
- Add an "Import Document" button (`data-testid="import-document"`) below the Browse card. When `!pandoc.installed` (or `!pandoc.sandboxSupported`), render it disabled with subtext "Install Pandoc in Settings to import documents" (or "Update Pandoc (2.15+ needed)").
- `importDocument()`: `open({ multiple: false, filters: [{ name: "Documents", extensions: ["docx", "odt", "rtf"] }] })`; if a path returned, `await invoke("convert_document", { id: props.vaultId, sourcePath: path })`, then `notifications.success("Imported <name>")` and optionally `store.showList()`. On throw: `notifications.error(...)` + `logWarning`.

- [ ] **Step 4: Run — verify pass**

Run: `npx vitest run tests/documentImport.test.ts && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/RecordMode.vue tests/documentImport.test.ts
git commit -m "feat(ui): Import Document action in the record chooser"
```

---

## Task 9: Frontend — buddy drag-drop → vault picker

**Files:**
- Modify: `src/stores/vaults.ts` — `importPicker` view + `pendingImportPath` + `openImportPicker()` + `refresh()` consumes `take_pending_import`
- Create: `src/components/ImportVaultPicker.vue`
- Modify: `src/components/ActionPanel.vue` — route the view
- Modify: `src/roots/BuddyRoot.vue` — drag-drop listener → `begin_document_import`
- Modify: `src-tauri/src/document_commands.rs` — `DocumentImportPending` state + `begin_document_import` / `take_pending_import`
- Modify: `src-tauri/src/commands.rs` — factor `show_panel(app)` out of `toggle_panel`
- Modify: `src-tauri/src/lib.rs` — register the two commands + `.manage(DocumentImportPending::default())`
- Modify: `tests/documentImport.test.ts` (picker + refresh case)

**Interfaces:**
- Consumes: `getCurrentWebview().onDragDropEvent` (Tauri v2 drag-drop), `begin_document_import`, `take_pending_import`, `convert_document`, the vaults list.
- Produces:
  - Rust: `DocumentImportPending(Mutex<Option<String>>)` state; `begin_document_import(app, path)` (stash + `show_panel`); `take_pending_import(app) -> Option<String>`; `commands::show_panel(app)` factored from `toggle_panel`.
  - vaults store: `view` union gains `"importPicker"`; state `pendingImportPath: string | null`; action `openImportPicker(path)`; `refresh()` consumes `take_pending_import` before the `pendingView`/`showList` branch; `showList()` clears `pendingImportPath`.
  - `ImportVaultPicker.vue`: lists `store.vaults`, each row calls `convert_document({ id, sourcePath: store.pendingImportPath })`, toasts, returns to list; Pandoc-gated on mount.

- [ ] **Step 1: Write failing tests**

```ts
it("openImportPicker sets the pending path and view", () => {
  setActivePinia(createPinia());
  const store = useVaultsStore();
  store.openImportPicker("C:/x/Report.docx");
  expect(store.view).toBe("importPicker");
  expect(store.pendingImportPath).toBe("C:/x/Report.docx");
});

it("refresh routes to the import picker when Rust has a pending import", async () => {
  setActivePinia(createPinia());
  mockIPC((cmd) => {
    if (cmd === "take_pending_import") return "C:/x/Report.docx";
    if (cmd === "list_vaults") return [];
    if (cmd === "count_open_tasks") return 0;
    return undefined;
  });
  const store = useVaultsStore();
  await store.refresh();
  expect(store.view).toBe("importPicker");
  expect(store.pendingImportPath).toBe("C:/x/Report.docx");
});
```

- [ ] **Step 2: Run — verify fail**

Run: `npx vitest run tests/documentImport.test.ts`
Expected: FAIL.

- [ ] **Step 3: Implement**

**Why not emit-then-toggle:** the buddy and panel windows have separate
Pinia stores, so the buddy can't set the panel store directly. An earlier
sketch (emit `import-document`, then `toggle_panel`, PanelRoot calls
`openImportPicker`) is racy (Codex review): `toggle_panel` *hides* an
already-open panel, and every open runs `panel-shown` → `refresh()` whose
default is `showList()`, which would clobber a direct `openImportPicker`.
The robust design makes the pending import **Rust-owned** and consumed
*inside* `refresh()`, exactly like the failed-update-install `pendingView`
idiom — but sourced from Rust because the trigger is cross-window, and the
panel is **shown** (never toggled).

**Rust (`document_commands.rs`):**
```rust
/// One-shot pending buddy-drop import path, consumed by the panel's refresh.
#[derive(Default)]
pub struct DocumentImportPending(pub std::sync::Mutex<Option<String>>);

/// A buddy drop: stash the path, then SHOW the panel (idempotent — never
/// toggles it hidden) so refresh() lands and consumes the pending import.
/// Sync command → main thread, where the window getters/show/focus are valid
/// (same rule as toggle_panel). Reuses the panel-show helper toggle_panel
/// uses (position while hidden, show, focus, emit `panel-shown`).
#[tauri::command]
pub fn begin_document_import(app: tauri::AppHandle, path: String) {
    {
        let state = app.state::<DocumentImportPending>();
        *vault_buddy_core::sync_util::lock_ignoring_poison(&state.0) = Some(path);
    }
    crate::commands::show_panel(&app); // factor out of toggle_panel's show branch
}

/// Take (and clear) the pending buddy-drop import path. Panel refresh calls
/// this and routes to the picker when it returns Some.
#[tauri::command]
pub fn take_pending_import(app: tauri::AppHandle) -> Option<String> {
    let state = app.state::<DocumentImportPending>();
    vault_buddy_core::sync_util::lock_ignoring_poison(&state.0).take()
}
```
Register both in `generate_handler!`, `.manage(DocumentImportPending::default())`,
and **factor `toggle_panel`'s show branch into `commands::show_panel(app)`**
(position-while-hidden + show + focus + emit `panel-shown` + hide bubble) so
`begin_document_import` reuses it without duplicating window logic. `toggle_panel`
keeps its hide branch and calls `show_panel` for the show branch.

**Vaults store (`src/stores/vaults.ts`):**
- Add `"importPicker"` to the `view` union.
- Add state `pendingImportPath: null as string | null,`.
- Add action `openImportPicker(path: string)`:
  ```ts
  openImportPicker(path: string) {
    this.view = "importPicker";
    this.pendingImportPath = path;
  },
  ```
- In `showList()` add `this.pendingImportPath = null;`.
- In `refresh()`, **consume the Rust pending import FIRST** (before the
  `pendingView`/`showList` branch), so a buddy drop always wins over the
  list default:
  ```ts
  const dropped = await invoke<string | null>("take_pending_import").catch(() => null);
  if (dropped) {
    this.openImportPicker(dropped);
  } else if (this.pendingView) {
    /* existing pendingView branch */
  } else {
    this.showList();
  }
  this.shownNonce++;
  await this.loadVaults();
  await this.loadTaskCounts();
  ```

**`ImportVaultPicker.vue`:** header "Import into which vault?", a list of
`store.vaults` rows; row click → `invoke("convert_document", { id: v.id,
sourcePath: store.pendingImportPath })`, success/error toast, then
`store.showList()`. (Extension already validated by the drop handler.)

**`ActionPanel.vue`:** add `v-else-if="view === 'importPicker'"` rendering
`<ImportVaultPicker />`; add its header title (`importPicker: "Import
document"`); import the component.

**`BuddyRoot.vue`:** register a drag-drop listener in `onMounted`:
```ts
import { getCurrentWebview } from "@tauri-apps/api/webview";
const SUPPORTED = ["docx", "odt", "rtf"];
unlistenDrop = await getCurrentWebview().onDragDropEvent(async (event) => {
  if (event.payload.type !== "drop") return;
  const path = event.payload.paths.find((p) => {
    const ext = p.split(".").pop()?.toLowerCase();
    return ext ? SUPPORTED.includes(ext) : false;
  });
  if (!path) return; // unsupported drop — ignore
  invokeQuiet("begin_document_import", { path });
});
```
Store `unlistenDrop` and call it in `onUnmounted`. No `toggle_panel`, no
event emit — `begin_document_import` shows the panel and arms the pending
import itself.

**Pandoc gate for drops:** `ImportVaultPicker` runs `detect_pandoc` on mount;
if not installed (or too old), it shows an inline "Install Pandoc in
Settings" message with a button that routes to `settings` instead of a vault
list — so a drop while Pandoc is absent lands somewhere actionable rather
than failing on convert.

- [ ] **Step 4: Run — verify pass**

Run: `npx vitest run tests/documentImport.test.ts && npm run build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/stores/vaults.ts src/components/ImportVaultPicker.vue src/components/ActionPanel.vue src/roots/BuddyRoot.vue src-tauri/src/document_commands.rs src-tauri/src/commands.rs src-tauri/src/lib.rs tests/documentImport.test.ts
git commit -m "feat: buddy drag-drop imports a document via a vault picker"
```

---

## Task 10: Docs — AGENTS.md + use-case status

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/use-cases/document-import-pandoc.md`

- [ ] **Step 1: Update AGENTS.md**

- IPC surface table: add a `document_commands.rs` row — `detect_pandoc`, `convert_document` (async — external subprocess), `get_documents_config`, `set_documents_config`, `set_pandoc_path`, `begin_document_import`, `take_pending_import`. Update the "All N commands" count (43 → 50).
- Add a "The document-import domain" subsection under the capture-domain area, summarizing: Pandoc-gated, `--sandbox` + heap cap, up-front joint note+media reservation, in-vault staging, publish-media-then-note with rollback, process-wide `ImportLock`, containment re-validation, startup `import-recovery` janitor for crash-orphaned staging dirs, fifth sanctioned vault write.
- Diagnostics/threads: note the new named threads `import-recovery` (and, from Task 4, that detection/conversion offload to `spawn_blocking`).
- Note the `commands::show_panel` refactor (factored from `toggle_panel`; also used by `begin_document_import`) in the window-system section.
- Note the new `tauri-plugin-dialog` + `dialog:allow-open` capability grant.
- "Where state lives on disk" row: note the app-global `documentImport` section + per-vault `documentsFolder` in `config.json`, and that `set_capture_config` preserves `documents_folder` (like `tasks_folder`).
- Frontend-state section: mention the `importPicker` view and the Rust-owned pending-import consumed in `refresh()` (via `take_pending_import`).

- [ ] **Step 2: Flip the use-case status**

In `docs/use-cases/document-import-pandoc.md`, change `status: planned` → `status: shipped`, add `shipped_in:` and the plan under `related_specs`, and update the "Status" heading.

- [ ] **Step 3: Commit**

```bash
git add AGENTS.md docs/use-cases/document-import-pandoc.md
git commit -m "docs: record the shipped Document Import via Pandoc domain"
```

---

## Task 11: Full verification pass

- [ ] **Step 1: Rust gates**

Run:
```bash
cd src-tauri && cargo fmt --check
cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test
cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings && cargo test -p vault-buddy --lib
cd src-tauri && cargo machete .
```
Expected: all pass.

- [ ] **Step 2: Frontend gates**

Run:
```bash
npm run lint && npm run check:loc && npm run check:quality && npm test && npm run build
```
Expected: all pass. If LOC/quality baselines shifted favorably, re-run with `--update` and commit the baseline.

- [ ] **Step 3: Compile gate (shell)**

Run: `npm run setup:linux && npx tauri build --no-bundle`
Expected: builds.

- [ ] **Step 4: Manual smoke (if on Windows / a machine with Pandoc)**

Install Pandoc, drop a `.docx` on the buddy, pick a vault, confirm a
dated `Documents/YYYY/MM/YYYY-MM-DD <name>.md` note with `type: Document`
frontmatter and (if the doc had images) a sibling media folder with
resolving links. Repeat via the record-chooser "Import Document" action.

- [ ] **Step 5: Commit any baseline updates**

```bash
git add scripts/loc-baseline.json scripts/quality-baseline.json
git commit -m "chore: update shrink-only baselines for document import"
```

---

## Self-Review Notes

- **Spec coverage:** trigger flows (Tasks 8/9), Pandoc gate + settings (Tasks 4/6/7), format scope (Task 2), file org/naming/frontmatter (Tasks 2/3), sandbox + heap cap + relative paths + cross-volume staging (Tasks 3/5), registry PATH refresh (Task 4), conversion serialization via ImportLock (Task 5), up-front joint note+media reservation (Tasks 3/5), media-publish rollback (Task 3), documents-folder containment revalidation (Tasks 5/6), crash-orphan staging recovery (Tasks 3/5b), buddy-drop pending-import surviving panel-shown (Task 9), failure = nothing published + toast (Tasks 3/5/8/9), success = silent save + toast (Tasks 8/9), config round-trip (Task 1), never-clobber (Task 3). All mapped.
- **Type consistency:** `PandocStatus` (incl. `configuredPath`)/`DocumentsConfigDto` camelCase across Rust↔TS; `convert_document(id, sourcePath)`, `set_documents_config(id, documentsFolder)` (per-vault, folder only), and `set_pandoc_path(pandocPath)` (app-global) arg names match the Vue call sites; `DocFormat`/`DocMeta`/`StagePlan`/`reserve_basename` used consistently across Tasks 2/3/5; `plan_staging(target_dir, basename, unique)` and `publish` write at the exact reserved name (no re-suffix); `ImportLock` holds an `Arc<Mutex<()>>` so its guard survives `spawn_blocking`; the buddy drop routes through `begin_document_import`/`take_pending_import` + `commands::show_panel`, not an emit-then-toggle.
- **Scope split:** app-global Pandoc (status/path) in `DocumentImportSettings.vue` (no vault id); per-vault Documents Folder in `CaptureSettings.vue` (has `vaultId`), via its own `get/set_documents_config` pair mirroring the Tasks Folder; `set_capture_config` preserves `documents_folder`.
- **Prereqs:** `tauri-plugin-dialog` (Task 0) must be wired before Tasks 7/8 use `open()`; the file pickers are otherwise rejected at invoke.
- **Known deviations to watch:** `winreg` is Windows-only (cfg-gated) — machete/deny may need a documented ignore; the ImportLock `try_lock` intentionally rejects (not queues) a concurrent import; the residual post-reservation name race errors + rolls back rather than re-suffixing (would break Pandoc's baked-in links); factoring `show_panel` out of `toggle_panel` must preserve every existing `toggle_panel` invariant (position-while-hidden, focus, `panel-shown`, hide bubble).
