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

/// Handle to `crash.log`, opened once the real log dir is known so the
/// native crash handler never has to open a file at crash time (see
/// `install_native_crash_handler`). `Write` is implemented for `&File`, so
/// the handler writes through this shared reference without any I/O setup.
static PREOPENED_CRASH_FILE: OnceLock<std::fs::File> = OnceLock::new();

/// Record the app log dir for the panic hook. Called once from `setup`.
pub fn set_log_dir(dir: PathBuf) {
    let crash_path = dir.join("crash.log");
    let _ = LOG_DIR.set(dir);
    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&crash_path)
    {
        Ok(file) => {
            let _ = PREOPENED_CRASH_FILE.set(file);
        }
        Err(e) => log::warn!("could not pre-open crash.log for the native crash handler: {e}"),
    }
}

/// Pre-setup fallback location for crash records — app-specific name so
/// startup adoption can never grab another program's file.
pub fn stray_crash_file() -> PathBuf {
    std::env::temp_dir().join("vault-buddy-crash.log")
}

fn crash_file() -> PathBuf {
    match LOG_DIR.get() {
        Some(dir) => dir.join("crash.log"),
        None => stray_crash_file(),
    }
}

/// Serializes the marker's check-and-write in both directions: once a
/// graceful exit stamps "clean" (under this lock), no in-flight heartbeat
/// can land a stale "running" write after it — the heartbeat's check and
/// write happen under the same lock. Plain AtomicBool gating was not
/// enough: it only stopped future heartbeat invocations, not one already
/// past its check.
static MARKER_GATE: std::sync::Mutex<bool> = std::sync::Mutex::new(false);

/// Stamp the run marker "clean" and stop the heartbeat from re-arming it.
/// Called from every graceful exit path (tray/buddy quit, Alt+F4 close,
/// update install) — a marker still saying "running" at next startup is
/// therefore a crash/kill by definition. Idempotent; safe from worker
/// threads.
pub fn mark_clean_shutdown() {
    let mut shutting_down = vault_buddy_core::sync_util::lock_ignoring_poison(&MARKER_GATE);
    *shutting_down = true;
    if let Some(dir) = LOG_DIR.get() {
        if let Err(e) = vault_buddy_core::app_diagnostics::write_clean_marker(dir) {
            log::warn!("could not stamp clean shutdown: {e}");
        }
    }
}

/// Re-stamp the marker as running while the app lives — but only once the
/// gate is clear. This was once described as "self-healing" a premature
/// "clean" stamp from a failed update install, but the gate it checks
/// latches forever once tripped, so a heartbeat alone could never repair
/// that case: it would keep early-returning below forever. The actual
/// repair is explicit — `rearm_running_marker`, called by the frontend when
/// an update install fails. This function is the backstop once re-armed: it
/// keeps the marker fresh so the *next* real crash still reports correctly.
pub fn heartbeat_running_marker() {
    let shutting_down = vault_buddy_core::sync_util::lock_ignoring_poison(&MARKER_GATE);
    if *shutting_down {
        return;
    }
    if let Some(dir) = LOG_DIR.get() {
        let _ =
            vault_buddy_core::app_diagnostics::write_running_marker(dir, env!("CARGO_PKG_VERSION"));
    }
}

/// Re-arm crash detection after an aborted shutdown: a failed update
/// install keeps the app running after prepare_update_install already
/// stamped "clean" and latched the gate. Clearing the latch and
/// re-stamping under the same lock restores the exact state a normal
/// running session has.
pub fn rearm_running_marker() {
    let mut shutting_down = vault_buddy_core::sync_util::lock_ignoring_poison(&MARKER_GATE);
    *shutting_down = false;
    if let Some(dir) = LOG_DIR.get() {
        let _ =
            vault_buddy_core::app_diagnostics::write_running_marker(dir, env!("CARGO_PKG_VERSION"));
    }
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
        let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        let record = format_crash_record(&CrashRecord {
            timestamp: &timestamp,
            thread: &thread,
            message: &message,
            location: location.as_deref(),
            backtrace: &backtrace,
            app_version: env!("CARGO_PKG_VERSION"),
            os: &os,
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

// Keeps the OS-level handler registered for the whole process lifetime —
// dropping it would silently unregister the hooks.
static NATIVE_CRASH_HANDLER: OnceLock<crash_handler::CrashHandler> = OnceLock::new();

/// `install_native_crash_handler` runs before the Tauri builder — and so
/// before the log plugin exists — so a `log::warn!` on install failure goes
/// nowhere. Stash the message here instead; `report_startup_diagnostics`
/// replays it once real logging is up.
static NATIVE_HANDLER_ERROR: OnceLock<String> = OnceLock::new();

/// Log a native-crash-handler install failure, if one happened. Call once
/// from `setup`, right after the startup banner (the first point logging
/// actually reaches a file).
pub fn report_startup_diagnostics() {
    if let Some(err) = NATIVE_HANDLER_ERROR.get() {
        log::warn!("{err}");
    }
}

/// Catch what the panic hook cannot: native faults (SEH exceptions on
/// Windows — WebView2, GPU or audio-driver crashes — and fatal signals on
/// Unix). The handler runs in a crashed process, possibly with the heap
/// lock still held by whatever corrupted it — a single allocation there
/// (format!, String, Vec growth) can deadlock a process that's also holding
/// the single-instance lock. So every byte written at crash time is either
/// preformatted here at install time or a fixed-size stack buffer; the
/// crash-time closure below does no formatting and no heap allocation on
/// the path that matters. Returning Handled(false) lets the
/// previous/default handling (WER dumps on Windows, default signal
/// disposition on Unix) still run afterward.
pub fn install_native_crash_handler() {
    // Preformatted once, now — never at crash time. The fault time can't be
    // known in advance, so the record says so explicitly and points at the
    // log tail instead.
    let install_ts = chrono::Local::now()
        .format("%Y-%m-%d %H:%M:%S%.3f %z")
        .to_string();
    let timestamp = format!(
        "(session start {install_ts}; fault time unrecorded — see the tail of vault-buddy.log)"
    );
    let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let record: Vec<u8> = format_crash_record(&CrashRecord {
        timestamp: &timestamp,
        thread: "<native fault>",
        message: "native crash (exception/signal code appended after this record)",
        location: None,
        backtrace: "<unavailable for native faults — enable WER LocalDumps for a dump>",
        app_version: env!("CARGO_PKG_VERSION"),
        os: &os,
    })
    .into_bytes();

    let result = crash_handler::CrashHandler::attach(unsafe {
        crash_handler::make_crash_event(move |context: &crash_handler::CrashContext| {
            #[cfg(windows)]
            let code = context.exception_code as u32;
            // `ssi_signo` is already `u32` on this target, but the field's
            // width isn't a cross-platform guarantee — keep the cast so
            // this still type-checks if that ever changes.
            #[cfg(target_os = "linux")]
            #[allow(clippy::unnecessary_cast)]
            let code = context.siginfo.ssi_signo as u32;
            // Fixed-size stack buffer — `hex_u32` writes into it with no
            // allocation, unlike `format!("{:#010x}", ...)`.
            #[cfg(any(windows, target_os = "linux"))]
            let mut hex_buf = [0u8; 10];
            #[cfg(any(windows, target_os = "linux"))]
            let code_hex = vault_buddy_core::crash::hex_u32(code, &mut hex_buf);

            // Separate write_all calls are deliberate: combining would require
            // crash-time allocation. The panic hook writes through its own handle,
            // and the docs already tell readers to treat same-moment records as one
            // crash, so a torn interleave is an accepted residual risk.
            let write_record = |mut file: &std::fs::File| {
                let _ = file.write_all(&record);
                let _ = file.write_all(b"code: ");
                #[cfg(any(windows, target_os = "linux"))]
                let _ = file.write_all(code_hex);
                #[cfg(target_os = "macos")]
                let _ = file.write_all(b"(mach exception)");
                let _ = file.write_all(b"\n\n");
                let _ = file.flush();
            };

            match PREOPENED_CRASH_FILE.get() {
                Some(file) => write_record(file),
                None => {
                    // Pre-setup window: the handler was installed (this
                    // closure exists) but `set_log_dir` hasn't opened the
                    // handle yet — a fault in the first instants of startup.
                    // Best-effort fall back to the stray temp file; unlike
                    // the pre-opened-handle path above, this may still
                    // allocate (OpenOptions, path join), but the window it
                    // covers is a few milliseconds wide.
                    if let Ok(f) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(stray_crash_file())
                    {
                        write_record(&f);
                    }
                }
            }
            crash_handler::CrashEventResult::Handled(false)
        })
    });
    match result {
        Ok(handler) => {
            let _ = NATIVE_CRASH_HANDLER.set(handler);
        }
        Err(e) => {
            let msg = format!("native crash handler unavailable: {e}");
            log::warn!("{msg}");
            let _ = NATIVE_HANDLER_ERROR.set(msg);
        }
    }
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
