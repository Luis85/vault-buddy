use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use vault_buddy_core::crash::{format_crash_record, CrashRecord};

// The panic hook has no AppHandle, so the resolved app log dir is stashed here
// once `setup` can compute it. Until then the hook falls back to the temp dir
// — a panic in that tiny pre-setup window is still captured.
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Record the app log dir for the panic hook. Called once from `setup`.
pub fn set_log_dir(dir: PathBuf) {
    let _ = LOG_DIR.set(dir);
}

fn crash_file() -> PathBuf {
    LOG_DIR
        .get()
        .cloned()
        .unwrap_or_else(std::env::temp_dir)
        .join("crash.log")
}

/// Install the process-wide panic hook. MUST run before the Tauri builder so a
/// panic anywhere — including builder construction and background threads — is
/// captured. On Windows a panic on the main thread unwinds across the WebView2
/// FFI boundary and aborts almost immediately, so an async logger would lose
/// it: the hook writes the record synchronously and flushes it to its own file
/// (separate from the plugin's rotating log to avoid contending for the same
/// handle). Every step is best-effort — the hook must never re-panic.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let thread = std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string();
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        let timestamp = chrono::Local::now()
            .format("%Y-%m-%d %H:%M:%S%.3f %z")
            .to_string();
        let record = format_crash_record(&CrashRecord {
            timestamp: &timestamp,
            thread: &thread,
            message: &message,
            location: location.as_deref(),
            backtrace: &backtrace,
        });
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(crash_file())
        {
            let _ = file.write_all(record.as_bytes());
            let _ = file.flush();
        }
        // Also into the rotating log/stdout, so the two views agree.
        log::error!(
            "panic: {message} at {}",
            location.as_deref().unwrap_or("<unknown location>")
        );
        previous(info);
    }));
}

/// Reveal the folder holding `vault-buddy.log` and `crash.log`. Best-effort:
/// spawn/exit failures are ignored (explorer returns nonzero even on success).
/// No `tauri-plugin-opener` dependency — a one-shot spawn is enough.
pub fn open_log_dir(app: &AppHandle) {
    let Ok(dir) = app.path().app_log_dir() else {
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Dev fallback so the path is exercisable off Windows.
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener).arg(&dir).spawn();
    }
}
