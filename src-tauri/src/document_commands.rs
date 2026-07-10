//! Document Import IPC: Pandoc detection + conversion + settings.
//! Detection re-reads PATH from the Windows registry so Recheck sees a
//! just-installed Pandoc without an app restart. Conversion runs Pandoc
//! sandboxed + heap-capped under spawn_blocking (async command, like
//! search_vaults). Spec:
//! docs/superpowers/specs/2026-07-10-document-import-pandoc-design.md

use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths, discovery, document_import};

use crate::capture_commands::ConfigWriteLock;

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

/// Merge the freshly-read registry PATH (`extra`) with the process PATH
/// (`base`), **registry entries FIRST** — deduped case-insensitively. The
/// registry is preferred because `base` is the process's launch-time PATH
/// snapshot, which is STALE: when a Windows user upgrades or relocates Pandoc
/// while Vault Buddy is running, the new location lands in the registry but not
/// in the process env. Searching the stale process PATH first would keep
/// resolving the OLD `pandoc.exe`, so Recheck would report the stale/unsupported
/// version until restart (Codex review). Registry-first fixes that; any
/// process-only entries (session additions not in the registry) still follow.
fn merged_path(base: &str, extra: &[String]) -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut out: Vec<String> = extra.to_vec();
    for p in base.split(sep) {
        if !p.is_empty() && !out.iter().any(|e| e.eq_ignore_ascii_case(p)) {
            out.push(p.to_string());
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
/// non-empty), then the bare `pandoc` PATH lookup, deduped. Pure so it's
/// testable without touching the real config file — `pandoc_candidates`
/// below is the impure wrapper that feeds it the real config.
fn candidate_order(override_path: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = override_path {
        if !p.trim().is_empty() {
            out.push(p.to_string());
        }
    }
    if !out.iter().any(|c| c == "pandoc") {
        out.push("pandoc".to_string());
    }
    out
}

/// Ordered pandoc candidates to try: the configured override FIRST (if
/// non-empty), then the bare `pandoc` PATH lookup. Both are probed in order so
/// a stale/mistyped override does NOT hide a valid Pandoc on PATH — detection
/// falls through to PATH before reporting Not Installed (the settings contract
/// promises the override is checked first, *falling back* to PATH). Deduped so
/// an override literally equal to `pandoc` isn't probed twice.
fn pandoc_candidates() -> Vec<String> {
    candidate_order(
        capture_config::load_config()
            .document_import
            .pandoc_path
            .as_deref(),
    )
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
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .next()
                .map(|l| l.trim().to_string())
        })
        .unwrap_or_default()
}

/// Resolve a pandoc to use across the ordered candidates (override → PATH),
/// with its full first `--version` line for display. None if no candidate
/// runs.
///
/// **Prefer a candidate that meets the sandbox minimum** (Codex review): a
/// working-but-old (< 2.15) override must not shadow a supported Pandoc on
/// PATH. Returning the old override here would make `convert_document` reject
/// it at the sandbox gate and never probe PATH, so imports stay broken until
/// the user clears the override. So we keep probing past a too-old runnable
/// candidate and return the first sandbox-capable one; only if NONE is
/// sandbox-capable do we return the first runnable (old) one, so
/// `detect_pandoc` can still report an accurate "installed but too old"
/// status (and `convert_document` still rejects it — nothing usable exists).
fn resolve_working_pandoc() -> Option<(String, u32, u32, String)> {
    let mut too_old: Option<(String, u32, u32)> = None;
    for program in pandoc_candidates() {
        if let Some((prog, major, minor)) = probe_pandoc(&program) {
            if sandbox_supported(major, minor) {
                let line = pandoc_version_line(&prog);
                return Some((prog, major, minor, line));
            }
            // Runnable but too old — remember the FIRST such one and keep
            // looking for a newer candidate.
            too_old.get_or_insert((prog, major, minor));
        }
    }
    too_old.map(|(prog, major, minor)| {
        let line = pandoc_version_line(&prog);
        (prog, major, minor, line)
    })
}

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
    capture_paths::assert_path_inside_vault(vault_root, &dir)?;
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
    let root = capture_paths::safe_recording_root(Path::new(&vault.path), effective)?;
    capture_paths::assert_path_inside_vault(Path::new(&vault.path), &root)?;
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
    capture_config::update_document_import_config(capture_config::DocumentImportConfig {
        pandoc_path: path,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pandoc_version_line() {
        assert_eq!(
            parse_pandoc_version("pandoc 3.1.9\nCompiled with..."),
            Some((3, 1))
        );
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
    fn merged_path_prefers_registry_over_stale_process_path_without_dupes() {
        // Registry entries (extra) come FIRST so a just-upgraded Pandoc wins
        // over the stale process-PATH snapshot; process-only entries follow;
        // no duplicates.
        let merged = merged_path("/usr/bin:/bin", &["/opt/pandoc".into(), "/usr/bin".into()]);
        assert_eq!(merged, "/opt/pandoc:/usr/bin:/bin");
        assert_eq!(merged.matches("/usr/bin").count(), 1);
    }

    #[test]
    fn pandoc_args_are_sandboxed_relative_and_heap_capped() {
        let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md");
        // reader
        assert!(args.windows(2).any(|w| w == ["-f", "docx"]));
        assert!(args.windows(2).any(|w| w == ["-t", "gfm"]));
        // sandbox always present
        assert!(args.iter().any(|a| a == "--sandbox"));
        // relative extract-media + output (no temp path baked in)
        assert!(args
            .iter()
            .any(|a| a == "--extract-media=2026-07-10 Report"));
        assert!(args.windows(2).any(|w| w == ["-o", "2026-07-10 Report.md"]));
        // heap cap
        let joined = args.join(" ");
        assert!(joined.contains("+RTS -M512M -RTS"));
    }

    #[test]
    fn candidates_try_override_then_path_deduped() {
        // No override: just PATH.
        assert_eq!(candidate_order(None), vec!["pandoc".to_string()]);
        // Override present: override first, then PATH.
        assert_eq!(
            candidate_order(Some("/custom/pandoc")),
            vec!["/custom/pandoc".to_string(), "pandoc".to_string()]
        );
        // Blank override is treated as unset.
        assert_eq!(candidate_order(Some("   ")), vec!["pandoc".to_string()]);
        // Override literally "pandoc" is deduped, not probed twice.
        assert_eq!(candidate_order(Some("pandoc")), vec!["pandoc".to_string()]);
    }
}
