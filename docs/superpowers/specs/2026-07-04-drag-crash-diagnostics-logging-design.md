# Buddy Drag Crash — Diagnostics & Logging — Design

- **Date:** 2026-07-04
- **Status:** Approved for implementation
- **Branch:** `claude/buddy-drag-crash-logging-qqo58s`

## Goal

The app has crashed several times while the user drags the buddy around.
The immediate blocker to fixing it is that **nothing survives the crash to
inspect**: `tauri-plugin-log` is initialized bare (`.build()`), so its only
target is stdout — which goes nowhere in a release GUI app — and there is no
panic handler anywhere. On Windows a Rust panic inside a Tauri command, menu
callback, or window-event handler unwinds across the wry↔WebView2 FFI
boundary (undefined behavior) and typically **aborts the process silently**.

This work does two things the user approved:

1. **Make crashes visible** — a panic hook that writes a crash record to disk
   before the process dies, persistent rotating file logs, frontend
   errors/breadcrumbs bridged into the same log, and a way to open the log
   folder.
2. **Harden the panic-prone spots on the drag path** — so a recoverable error
   degrades gracefully instead of silently aborting the app.

## Root-cause status (honest)

The exact crashing line **cannot be named yet** — precisely because no crash
record exists today; closing that gap is the point of part 1. Static analysis
of the drag path surfaced these panic-to-abort risks, which part 2 hardens:

- The **1-second background thread** in `lib.rs` (always-on-top re-assert +
  position checkpoint) runs `outer_position()`, `set_always_on_top()`,
  `save_window_state()`, and `PanelOffset.0.lock().unwrap()` **every second,
  including mid-drag** while the OS modal window-move loop owns the main
  thread. A panic there dies silently (losing always-on-top + position
  saving); if it panics while holding the shared `PanelOffset` mutex, the
  next `.lock().unwrap()` on the **main thread** (`set_panel_offset`,
  `restore_home_position`) panics → process abort.
- Poison-intolerant `.lock().unwrap()` on that shared mutex in
  `commands.rs`, `tray.rs`, and `lib.rs`.
- The top-level `.run(...).expect("error while running Vault Buddy")` and a
  swallowed `set_always_on_top` failure both lose their error.

Whatever the true cause (including a native WebView2 fault, which no Rust hook
can catch), the panic hook plus the last drag breadcrumb will bracket it in
the log for the next occurrence.

## Architecture

### Crash record — pure formatter in `core`, wiring in the shell

- **`src-tauri/core/src/crash.rs`** — pure, testable-everywhere formatter.
  `format_crash_record(fields) -> String` takes the panic message, optional
  `file:line:col` location, thread name, a captured backtrace string, and a
  pre-rendered timestamp, and returns a delimited, human-readable block
  (leading marker line so successive crashes are greppable). No Tauri or std
  panic types in the signature — it is a string builder, unit-tested on
  Linux CI. Re-exported from `core/src/lib.rs`.
- **`src-tauri/src/diagnostics.rs`** (new shell module, Windows-only compile)
  — owns the process-wide panic hook and log-folder helper.
  - `LOG_DIR: OnceLock<PathBuf>` holds the resolved app log dir. Filled in
    `.setup()` from `app.path().app_log_dir()`.
  - `install_panic_hook()` — called as the **first statement of `run()`**, so
    a panic anywhere (including during builder construction) is caught. Saves
    the previous hook and, on panic:
    1. Builds a record via `core::format_crash_record` (message from payload
       downcast to `&str`/`String`, `PanicHookInfo::location()`, current
       thread name, `std::backtrace::Backtrace::force_capture()`, local
       timestamp).
    2. Writes it **synchronously** — `OpenOptions::new().create(true)
       .append(true)` on a **separate** `crash.log` in `LOG_DIR` (falling
       back to `std::env::temp_dir()` if unset), then `write_all` + `flush`.
       A separate file avoids contending with the logger's handle on the main
       log; the flush guarantees the record is on disk before a Windows
       FFI-unwind abort. Every step is best-effort (a failing crash writer
       must never re-panic).
    3. Emits `log::error!` with the same summary (so it also lands in the main
       log / stdout), then chains the previous hook.
  - `open_log_dir(app)` — resolve `app_log_dir()`, `create_dir_all` it, then
    reveal it: `explorer.exe <dir>` via `std::process::Command::spawn()` on
    Windows, with a `#[cfg(not(windows))]` dev fallback (`xdg-open`/`open`).
    No new plugin dependency; result ignored (explorer returns nonzero even
    on success).

### Persistent logging (`tauri-plugin-log` config, `lib.rs`)

Replace the bare `.build()` with an explicit config:

- Targets: `LogDir { file_name: Some("vault-buddy".into()) }` **and**
  `Stdout` (kept for dev).
- `LevelFilter::Info`.
- Local-time timestamps (`TimezoneStrategy::UseLocal`) so lines match the
  user's clock.
- `max_file_size(5 * 1024 * 1024)` (5 MB) + `RotationStrategy::KeepOne` —
  bounded disk, and the one rotated-out file usually still holds the crash
  that preceded a restart.

The many existing `log::*` calls (capture, uri, commands, tray) begin
persisting for free.

### Frontend log bridge (`src/logging.ts` + `@tauri-apps/plugin-log`)

- Add the `@tauri-apps/plugin-log` dependency and the `log:default`
  permission to `capabilities/default.json`.
- **`src/logging.ts`** — `initLogging()` (called from `main.ts`):
  - Detects Tauri (`'__TAURI_INTERNALS__' in window`); when absent every
    export is a **no-op**, so the Vitest/happy-dom suite stays silent and
    needs no Tauri runtime.
  - Installs `window` `error` and `unhandledrejection` listeners that forward
    message + stack into the same log file via the plugin's `error()`.
  - Exports `logBreadcrumb(msg)` for lifecycle markers.
  - Does **not** blanket-wrap `console.*` (loop/noise risk — out of scope).
- Breadcrumbs added at `App.vue onDragStart` ("drag start @ x,y") and
  `useCompanionWindow.setGeometry` ("geometry → pos/size"). Several
  currently-silent `catch {}` swallows in the panel open/close hot path
  (`applyOpen`, `applyClose`, the transition `queue.catch`) become
  `warn`-level breadcrumbs instead of vanishing.

### Harden the panic sites (Rust)

- A generic poison-tolerant lock helper
  **`core::lock_ignoring_poison(&Mutex<T>) -> MutexGuard<'_, T>`**
  (`.lock().unwrap_or_else(std::sync::PoisonError::into_inner)`) lives in the
  core crate so its behavior is unit-tested on Linux CI. **`PanelOffset`**
  gains accessors built on it — `set((i32,i32))`, `take() -> (i32,i32)`,
  `get() -> (i32,i32)` — so a poisoned mutex can never cascade into a
  process-aborting `.unwrap()`. `set_panel_offset`, `restore_home_position`,
  and the background loop switch to these; no `.lock().unwrap()` remains.
- The **1 s background tick body** is wrapped in
  `std::panic::catch_unwind(AssertUnwindSafe(|| { ... }))`; a caught panic is
  logged and the loop continues, so one bad tick can never permanently kill
  always-on-top re-assertion + position checkpointing.
- The swallowed `set_always_on_top` failure logs on `Err`; the top-level
  `.run(...).expect(...)` becomes a `match` that `log::error!`s the runtime
  error before exiting non-zero (so the reason reaches disk).

### "Open logs folder" affordance

- New command **`open_logs_folder(app)`** in `commands.rs`, delegating to
  `diagnostics::open_log_dir`; registered in `invoke_handler`.
- **Tray menu**: an "Open logs folder" item (id `open-logs`), handled in the
  tray `on_menu_event`.
- **Settings**: a new **Diagnostics** `<section>` in `BuddySettings.vue`
  (after `<UpdateSettings />`) with an "Open logs folder" button that
  `invoke`s the command, styled to mirror `UpdateSettings.vue`.

## Data flow

Panic anywhere → panic hook → synchronous `crash.log` write + `log::error!` →
`LogDir` file. Frontend error/breadcrumb → `logging.ts` → plugin → same
`LogDir` file. User clicks tray "Open logs folder" or the Settings button →
`open_log_dir` reveals the folder holding both `vault-buddy` log and
`crash.log`.

## Error handling

- The crash writer is fully best-effort: an unresolved log dir falls back to
  the temp dir, and any I/O failure is swallowed — it must never re-panic
  inside the panic hook.
- `logging.ts` no-ops entirely outside Tauri; individual forward calls are
  `try`/`catch`-guarded so a logging failure never breaks the UI.
- `open_log_dir` ignores spawn/exit failures (best-effort reveal).
- Poison-tolerant accessors always return the last-written offset even after
  a poisoning panic elsewhere.

## Testing

- **Core (Linux CI):**
  - `crash::format_crash_record` — includes message, location, thread, and
    backtrace; the marker line is present; a `None` location degrades to a
    readable placeholder rather than panicking.
  - `lock_ignoring_poison` — spawn a thread that panics while holding a test
    mutex, then assert the helper still returns a guard with the stored value
    (regression test named for the abort-cascade it prevents). `PanelOffset`
    accessors inherit this behavior by construction.
- **Frontend (Vitest):** `logging.ts` — exports no-op without Tauri (no throw,
  no plugin import side effects); with `@tauri-apps/plugin-log` mocked
  (`vi.mock`), an emitted `window` `error`/`unhandledrejection` forwards to
  the plugin, and `logBreadcrumb` forwards its message. Existing
  CompanionCharacter/App drag tests stay green (breadcrumbs no-op under
  happy-dom).
- **Shell (`diagnostics.rs`, tray + command wiring):** Windows-only compile —
  mirror existing patterns exactly, `cargo fmt --check`, and let CI's
  `windows-app` job verify the build.

## Out of scope (possible future)

A full crash-reporting backend (Sentry / minidump capture + remote upload)
would also symbolicate native WebView2 access violations, but carries a heavy
dependency and telemetry/privacy/network implications — deferred.
