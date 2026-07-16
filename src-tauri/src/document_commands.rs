//! Document Import IPC: Pandoc detection + conversion + settings.
//! Detection re-reads PATH from the Windows registry so Recheck sees a
//! just-installed Pandoc without an app restart. Conversion runs Pandoc
//! sandboxed + heap-capped under spawn_blocking (async command, like
//! search_vaults). Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use std::path::Path;
use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery, document_import};

use crate::capture_commands::ConfigWriteLock;
use crate::pandoc::{
    pandoc_args, pandoc_command, resolve_working_pandoc, run_capturing, sandbox_supported, Capture,
    CONVERT_TIMEOUT,
};

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

/// Process-wide serialization for imports. A `try_lock` (not blocking) so a
/// second concurrent import fails fast instead of racing step 1's
/// exists-reservation into a corrupt/partial publish. The inner mutex is an
/// `Arc` so its guard can be held on the `spawn_blocking` thread (Tauri
/// `State` itself can't cross that boundary). Registered as app state in
/// lib.rs beside ConfigWriteLock: `.manage(ImportLock::default())`.
#[derive(Default, Clone)]
pub struct ImportLock(pub std::sync::Arc<std::sync::Mutex<()>>);

/// `try_lock` that treats a POISONED mutex as acquired. The guarded state is
/// `()` — there is nothing a panic-while-held could actually corrupt — so
/// poisoning must not permanently wedge every future import behind a single
/// past panic. Only `WouldBlock` (an import genuinely in progress right now)
/// means "don't proceed"; that fail-fast semantics is unchanged.
fn try_acquire(m: &std::sync::Mutex<()>) -> Option<std::sync::MutexGuard<'_, ()>> {
    match m.try_lock() {
        Ok(guard) => Some(guard),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(std::sync::TryLockError::WouldBlock) => None,
    }
}

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
        let _guard = match try_acquire(&mutex) {
            Some(g) => g,
            None => return Err("An import is already in progress.".to_string()),
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
    // Pandoc runs with cwd = the staging work_dir, not the caller's cwd — a
    // relative source_path would resolve against the WRONG directory and
    // either fail confusingly or (worse) read an unintended file. The
    // frontend always sends an absolute path from the native file picker;
    // this is a defense-in-depth guard, not the primary contract.
    if !src.is_absolute() {
        return Err("Import failed — the file path must be absolute.".into());
    }
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
        return Err(
            "Your Pandoc is too old to import safely (need 2.15+). Please update it.".into(),
        );
    }

    let cfg = capture_config::vault_config(&capture_config::load_config(), id);
    let documents_folder = cfg.documents_root().to_string();
    // Re-validate containment even though set_documents_config already did:
    // config.json is hand-editable, so a `../…` or symlink-escaping folder
    // must be caught here too before any staging dir is created (Codex
    // review). Same lexical + canonical check the save path uses.
    let vault_root = Path::new(&vault.path);
    let safe = capture_paths::safe_recording_root(vault_root, &documents_folder)?;
    capture_paths::assert_path_inside_vault(vault_root, &safe)?;
    // NEW imports land flat (no YYYY/MM) when the vault opted out of dated
    // folders — same per-vault toggle and precedent as start_capture's
    // capture_dir branch; recovery's clean_stale_staging_at sweeps both
    // layouts so a crash orphan is reaped either way.
    let dated = cfg.document_date_folders;
    let dir = if dated {
        document_import::target_dir(vault_root, &documents_folder, year, month)
    } else {
        safe.clone()
    };
    // Guard the FULLY DATED dir BEFORE creating it — the folder-root check
    // above is lexical and can't see a `Documents/2026` or `2026/07`
    // symlink/junction that escapes the vault. `assert_path_inside_vault`
    // canonicalizes the nearest EXISTING ancestor, so a pre-existing dated
    // symlink is caught here before `create_dir_all` follows it and creates
    // directories outside the vault (Codex review). Unconditional: when flat,
    // `dir` IS `safe`, so this just repeats the already-passed check above —
    // harmless, and keeps one code path instead of a branch around it.
    capture_paths::assert_path_inside_vault(vault_root, &dir)?;
    // Resolve the ` (N)` suffix for BOTH note and media folder up front — the
    // target dir must exist for the existence checks, and Pandoc bakes the
    // media-folder name into image links, so it can't be decided at publish
    // time (Codex review).
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not prepare import: {e}"))?;
    // Re-validate the now-created dir to close the race between the pre-check
    // and `create_dir_all` (a dated symlink swapped in between the two).
    // `start_capture` guards its dated folder the same way after create.
    capture_paths::assert_path_inside_vault(vault_root, &dir)?;
    // Require the dated path to be REAL in-place (no symlink/junction redirecting
    // a dated level, even one pointing elsewhere IN the vault). Containment above
    // permits such a link, but the recovery sweeper only descends real in-place
    // dated dirs — so staging through a link would strand an unrecoverable
    // orphan on a crash. Keeping import and recovery to the same layout closes
    // that gap (Codex review). Dated-only: a flat `dir` is just `safe`, already
    // asserted in-vault above — there is no dated level for a link to redirect.
    if dated && !document_import::is_real_dated_dir(&safe, &dir, year, month) {
        return Err(
            "Import destination resolves through a link; use a real Documents folder.".into(),
        );
    }
    let raw = document_import::document_basename(stem, today);
    let basename = document_import::reserve_basename(&dir, &raw);
    let plan = document_import::plan_staging(&dir, &basename, unique);

    // Fresh staging dir.
    document_import::cleanup_staging(&plan.work_dir);
    std::fs::create_dir_all(&plan.work_dir)
        .map_err(|e| format!("Could not prepare import: {e}"))?;

    let args = pandoc_args(format.reader(), &plan.media_name, &plan.note_name);
    let mut cmd = pandoc_command(&program);
    cmd.current_dir(&plan.work_dir)
        .arg(src) // absolute source
        .args(&args);

    let run = run_capturing(cmd, CONVERT_TIMEOUT, Capture::Stderr);
    match run {
        Ok((true, _)) => {}
        Ok((false, stderr)) => {
            document_import::cleanup_staging(&plan.work_dir);
            // Log WHY (bounded so a crafted doc can't flood the log); the
            // user-facing string stays generic. "No swallowed error" convention.
            let detail = stderr.trim();
            if detail.is_empty() {
                log::warn!("convert_document: pandoc exited non-zero (no stderr)");
            } else {
                let slice: String = detail.chars().take(500).collect();
                log::warn!("convert_document: pandoc failed: {slice}");
            }
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentsConfigDto {
    pub documents_folder: Option<String>,
    /// Whether NEW imports land in a dated `YYYY/MM` subfolder — the
    /// Documents settings surface for `VaultCaptureConfig::document_date_folders`.
    pub document_date_folders: bool,
    /// Whether a document import extracts images into a media folder (true) or
    /// produces a text-only note with images dropped (false) — the Documents
    /// settings surface for `VaultCaptureConfig::document_extract_images`.
    pub document_extract_images: bool,
}

/// Per-vault documents folder (or None → the frontend shows the "Documents"
/// default). Unknown vault → None, never an error. Mirrors get_tasks_config.
#[tauri::command]
pub fn get_documents_config(id: String) -> DocumentsConfigDto {
    let vault = capture_config::vault_config(&capture_config::load_config(), &id);
    DocumentsConfigDto {
        documents_folder: vault.documents_folder,
        document_date_folders: vault.document_date_folders,
        document_extract_images: vault.document_extract_images,
    }
}

/// Persist the vault's documents folder + layout toggle. Validates containment
/// BEFORE writing (the effective folder — explicit or the "Documents"
/// default — must stay in the vault), serialized behind ConfigWriteLock.
/// Read-modify-write preserves the vault's other config (recording_date_folders
/// included — that field belongs to set_capture_config). Mirrors
/// set_tasks_config exactly, plus the one extra bool field.
#[tauri::command]
pub fn set_documents_config(
    lock: tauri::State<ConfigWriteLock>,
    id: String,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
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
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), effective)?;
    capture_paths::assert_path_inside_vault(Path::new(&vault.path), &root)?;
    let _guard = lock_ignoring_poison(&lock.0);
    let mut v = capture_config::vault_config(&capture_config::load_config(), &id);
    v.documents_folder = folder;
    v.document_date_folders = document_date_folders;
    v.document_extract_images = document_extract_images;
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
    capture_config::update_document_import_config(capture_config::DocumentImportConfig {
        pandoc_path: path,
    })
}

/// One-shot pending buddy-drop import path, consumed by the panel's refresh.
/// Rust-owned (not an emit-then-toggle) because the buddy and panel windows
/// have SEPARATE Pinia stores — the buddy can't set the panel store
/// directly, and `toggle_panel` would HIDE an already-open panel instead of
/// routing it to the picker. Registered as app state in lib.rs beside
/// ImportLock: `.manage(DocumentImportPending::default())`.
#[derive(Default)]
pub struct DocumentImportPending(pub std::sync::Mutex<Option<String>>);

/// A buddy drop: stash the path, then SHOW the panel (idempotent — never
/// toggles it hidden) so the panel's `refresh()` lands and consumes the
/// pending import via `take_pending_import`. Sync command → runs on the main
/// thread, where the window getters/show/focus are valid (same rule as
/// `toggle_panel`). Reuses `commands::show_panel`, the panel-show helper
/// factored out of `toggle_panel`'s show branch, so this doesn't duplicate
/// window logic.
#[tauri::command]
pub fn begin_document_import(app: tauri::AppHandle, path: String) {
    {
        let state = app.state::<DocumentImportPending>();
        *lock_ignoring_poison(&state.0) = Some(path);
    }
    crate::commands::show_panel(&app);
}

/// Take (and clear) the pending buddy-drop import path. The panel's
/// `refresh()` calls this on every open and routes to the import picker
/// when it returns `Some` — a one-shot consume, same idiom as the
/// `pendingView` request the failed-update-install reopen uses.
#[tauri::command]
pub fn take_pending_import(app: tauri::AppHandle) -> Option<String> {
    let state = app.state::<DocumentImportPending>();
    let mut guard = lock_ignoring_poison(&state.0);
    guard.take()
}

/// Open a freshly-imported note in Obsidian — the success toast's "Open in
/// Obsidian" action. `path` is what `convert_document` returned (vault-relative
/// on success, an absolute fallback otherwise); resolve the vault by id and
/// launch `obsidian://open` for it. Read-only: never writes into the vault; the
/// launch is logged (uri::launch), the same audit trail as `open_recording`.
#[tauri::command]
pub fn open_imported_document(id: String, path: String) -> Result<(), String> {
    let vault = discovery::discover_vaults()
        .into_iter()
        .find(|v| v.id == id)
        .ok_or("Vault not found — was it removed from Obsidian?")?;
    let uri = vault_buddy_core::imported_note_uri(&vault.id, Path::new(&vault.path), &path)
        .ok_or_else(|| format!("imported note is outside its vault: {path}"))?;
    vault_buddy_core::uri::launch(&uri)
}

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
                let Some(_guard) = try_acquire(&lock.0) else {
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
                    let sweep = document_import::clean_stale_staging_at(
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
