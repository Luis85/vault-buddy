# Drag Crash — Diagnostics & Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make buddy-drag crashes visible (panic hook → on-disk crash record, persistent rotating logs, frontend error bridge, an "Open logs folder" affordance) and harden the panic-prone spots on the drag path so a recoverable error degrades instead of silently aborting the app.

**Architecture:** Pure, testable logic (a poison-tolerant lock helper and a crash-record formatter) lives in the `core` crate and is unit-tested on Linux CI. The Windows-only shell crate installs a `std::panic` hook that writes a flushed `crash.log`, reconfigures `tauri-plugin-log` to persist to a rotating file, hardens the shared `PanelOffset` mutex and the 1 s background thread, and exposes an "Open logs folder" command + tray item. The Vue frontend gains a Tauri-guarded `logging.ts` bridge that funnels uncaught errors and drag/geometry breadcrumbs into the same log.

**Tech Stack:** Rust (Tauri v2, `tauri-plugin-log` 2.8.0, `chrono`, `std::backtrace`), Vue 3 + Pinia, `@tauri-apps/plugin-log` ^2, Vitest + happy-dom + `@vue/test-utils`.

## Global Constraints

- **Node 22**; install with `npm ci`. Full Vitest suite: `npm test`; single file: `npx vitest run tests/<file>.test.ts`.
- **The shell crate (`src-tauri/src/*.rs`) does not compile on Linux** (no webkit2gtk). For shell tasks there is **no local build/test**: mirror existing patterns exactly, run `cd src-tauri && cargo fmt --check` as the only local gate, and rely on CI's `windows-app` job as the compile gate. All *testable* logic is therefore extracted into the `core` crate (Tasks 1–2), which builds and tests locally.
- **Core crate** builds/tests locally: `cd src-tauri/core && cargo test` and `cargo clippy --all-targets -- -D warnings`.
- **Rust logic that doesn't need Tauri types goes in `core`** (per AGENTS.md).
- **The app never writes into a vault.** The only files this work writes are the app's own logs under the app log dir — never vault content.
- **Commits:** Conventional Commits with existing scopes (`feat(core)`, `fix(shell)`, `feat(shell)`, `feat(ui)`). Imperative subject; body explains the *why* / failure mode.
- **Comments** explain constraints the code can't show (race windows, platform quirks, ordering) — match the existing heavy-on-invariants density.
- Invoke the tauri CLI only as `npx tauri <cmd>`, never via npm script indirection.

---

### Task 1: Poison-tolerant lock helper (core)

**Files:**
- Create: `src-tauri/core/src/sync_util.rs`
- Modify: `src-tauri/core/src/lib.rs:1-7` (add `pub mod sync_util;`)
- Test: in `src-tauri/core/src/sync_util.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces: `vault_buddy_core::sync_util::lock_ignoring_poison<T>(&std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T>`

- [ ] **Step 1: Write the failing test**

Create `src-tauri/core/src/sync_util.rs`:

```rust
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Lock a mutex, recovering the guard even if a previous holder panicked and
/// poisoned it. A poisoned shared mutex must never cascade into a `.unwrap()`
/// panic on the main thread: on Windows that unwinds across the WebView2 FFI
/// boundary and aborts the whole app. Recovering the guard degrades to
/// "carry on with the last value" instead.
pub fn lock_ignoring_poison<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recovers_the_guard_after_a_poisoning_panic() {
        // Regression: a poisoned PanelOffset used to reach `.lock().unwrap()`
        // on the main thread and abort the process. The helper must return
        // the last-written value instead of panicking.
        let m = Arc::new(Mutex::new(42));
        let m2 = Arc::clone(&m);
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison the mutex while holding the lock");
        })
        .join();
        assert!(m.lock().is_err(), "precondition: the mutex is poisoned");
        assert_eq!(*lock_ignoring_poison(&m), 42);
    }
}
```

- [ ] **Step 2: Wire the module in**

Edit `src-tauri/core/src/lib.rs` — add the module declaration alphabetically among the existing `pub mod` lines (after `pub mod process;`, before `pub mod uri;`):

```rust
pub mod sync_util;
```

- [ ] **Step 3: Run the test to verify it fails first, then passes**

Run: `cd src-tauri/core && cargo test sync_util`
Expected on a clean checkout before Step 1: FAIL (module/function not found). After Steps 1–2: **PASS** (`recovers_the_guard_after_a_poisoning_panic ... ok`).

- [ ] **Step 4: Lint**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/sync_util.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): poison-tolerant mutex lock helper"
```

---

### Task 2: Crash-record formatter (core)

**Files:**
- Create: `src-tauri/core/src/crash.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod crash;`)
- Test: in `src-tauri/core/src/crash.rs` (`#[cfg(test)]`)

**Interfaces:**
- Produces:
  - `vault_buddy_core::crash::CrashRecord<'a> { timestamp: &'a str, thread: &'a str, message: &'a str, location: Option<&'a str>, backtrace: &'a str }`
  - `vault_buddy_core::crash::format_crash_record(&CrashRecord) -> String` — a delimited block whose first line starts with the greppable marker `==== VAULT BUDDY PANIC`.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/core/src/crash.rs`:

```rust
/// The fields of a single crash, pre-rendered by the caller so this stays a
/// pure string builder (no Tauri, no std panic types) — unit-testable on any
/// platform. The shell's panic hook fills these from `PanicHookInfo`.
pub struct CrashRecord<'a> {
    pub timestamp: &'a str,
    pub thread: &'a str,
    pub message: &'a str,
    pub location: Option<&'a str>,
    pub backtrace: &'a str,
}

/// Format one crash into a delimited, human-readable block. The leading
/// marker line makes successive crashes greppable in a single file, and the
/// trailing blank line separates appended records.
pub fn format_crash_record(record: &CrashRecord) -> String {
    let location = record.location.unwrap_or("<unknown location>");
    format!(
        "==== VAULT BUDDY PANIC {timestamp} ====\n\
         thread: {thread}\n\
         location: {location}\n\
         message: {message}\n\
         backtrace:\n{backtrace}\n\n",
        timestamp = record.timestamp,
        thread = record.thread,
        location = location,
        message = record.message,
        backtrace = record.backtrace,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(location: Option<&'static str>) -> String {
        format_crash_record(&CrashRecord {
            timestamp: "2026-07-04 21:30:00.123 +0000",
            thread: "main",
            message: "called `Option::unwrap()` on a `None` value",
            location,
            backtrace: "0: some::frame",
        })
    }

    #[test]
    fn includes_every_field_under_the_marker() {
        let out = sample(Some("src-tauri/src/commands.rs:16:5"));
        assert!(out.starts_with("==== VAULT BUDDY PANIC 2026-07-04"), "got: {out}");
        assert!(out.contains("thread: main"));
        assert!(out.contains("location: src-tauri/src/commands.rs:16:5"));
        assert!(out.contains("called `Option::unwrap()` on a `None` value"));
        assert!(out.contains("backtrace:\n0: some::frame"));
        assert!(out.ends_with("\n\n"), "records must be blank-line separated");
    }

    #[test]
    fn missing_location_degrades_to_a_placeholder() {
        let out = sample(None);
        assert!(out.contains("location: <unknown location>"), "got: {out}");
    }
}
```

- [ ] **Step 2: Wire the module in**

Edit `src-tauri/core/src/lib.rs` — add at the top of the `pub mod` block (before `pub mod capture_config;`):

```rust
pub mod crash;
```

- [ ] **Step 3: Run the tests**

Run: `cd src-tauri/core && cargo test crash`
Expected: **PASS** — `includes_every_field_under_the_marker ... ok`, `missing_location_degrades_to_a_placeholder ... ok`.

- [ ] **Step 4: Lint**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/core/src/crash.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): crash record formatter"
```

---

### Task 3: Poison-tolerant `PanelOffset` access (shell)

> Shell task — cannot build on Linux. Local gate is `cargo fmt --check`; CI's `windows-app` job is the compile gate. The *behavior* is already covered by Task 1's core test.

**Files:**
- Modify: `src-tauri/src/commands.rs:11-17` (add accessors, simplify `set_panel_offset`)
- Modify: `src-tauri/src/tray.rs:30-39` (`restore_home_position` uses `take()`)

**Interfaces:**
- Consumes: `vault_buddy_core::sync_util::lock_ignoring_poison` (Task 1)
- Produces: `PanelOffset::set((i32,i32))`, `PanelOffset::take() -> (i32,i32)`, `PanelOffset::get() -> (i32,i32)`

- [ ] **Step 1: Add poison-tolerant accessors and use them in the command**

In `src-tauri/src/commands.rs`, replace the current struct + command (lines 11–17):

```rust
#[derive(Default)]
pub struct PanelOffset(pub Mutex<(i32, i32)>);
```
```rust
#[tauri::command]
pub fn set_panel_offset(state: tauri::State<PanelOffset>, x: i32, y: i32) {
    *state.0.lock().unwrap() = (x, y);
}
```

with:

```rust
#[derive(Default)]
pub struct PanelOffset(pub Mutex<(i32, i32)>);

impl PanelOffset {
    // All access goes through lock_ignoring_poison: if any thread ever
    // panics while holding this lock, `.lock().unwrap()` on the main thread
    // would abort the whole app (a panic across the WebView2 FFI boundary on
    // Windows). Recovering the poisoned guard degrades to the last value.
    pub fn set(&self, value: (i32, i32)) {
        *vault_buddy_core::sync_util::lock_ignoring_poison(&self.0) = value;
    }

    /// Read and zero the offset in one locked step (used by the restore path
    /// so the close handler and the quit path can't double-add).
    pub fn take(&self) -> (i32, i32) {
        std::mem::take(&mut *vault_buddy_core::sync_util::lock_ignoring_poison(&self.0))
    }

    pub fn get(&self) -> (i32, i32) {
        *vault_buddy_core::sync_util::lock_ignoring_poison(&self.0)
    }
}

#[tauri::command]
pub fn set_panel_offset(state: tauri::State<PanelOffset>, x: i32, y: i32) {
    state.set((x, y));
}
```

- [ ] **Step 2: Use `take()` in the tray restore path**

In `src-tauri/src/tray.rs`, replace the first line of `restore_home_position` (line 31):

```rust
    let (dx, dy) = std::mem::take(&mut *app.state::<PanelOffset>().0.lock().unwrap());
```

with:

```rust
    let (dx, dy) = app.state::<PanelOffset>().take();
```

(Leave the rest of `restore_home_position` unchanged.)

- [ ] **Step 3: Format check**

Run: `cd src-tauri && cargo fmt --check`
Expected: clean (no diff). Cannot `cargo build` here — the shell crate needs webkit2gtk; CI's `windows-app` job compiles it.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/tray.rs
git commit -m "fix(shell): poison-tolerant panel offset access"
```

---

### Task 4: Panic-safe checkpoint tick + fatal-run logging (shell)

> Shell task — `cargo fmt --check` only; CI compiles.

**Files:**
- Modify: `src-tauri/src/lib.rs` — extract the background-loop body into `checkpoint_tick`, wrap it in `catch_unwind`, use `PanelOffset::get()`, log the swallowed `set_always_on_top` failure, and log a fatal run-loop error.

**Interfaces:**
- Consumes: `PanelOffset::get()` (Task 3)

- [ ] **Step 1: Extract the tick body into a panic-isolated function**

In `src-tauri/src/lib.rs`, the `std::thread::spawn(move || { ... })` block currently holds a `loop { ... }` with the whole checkpoint body inline (lines ~107–154). Replace the loop body so each tick runs inside `catch_unwind` (a `continue` cannot cross a closure boundary, so the body moves into a function that uses `return`):

```rust
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
                // One bad tick must never permanently kill always-on-top
                // re-assertion + position checkpointing. Isolate each tick:
                // a panic here is logged (via the crash hook + this line) and
                // the loop keeps running.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    checkpoint_tick(&handle, &mut last_pos, &mut ticks, &mut saved_once);
                }));
                if outcome.is_err() {
                    log::error!("background checkpoint tick panicked; continuing");
                }
            });
```

and hoist the three mutable counters so they live across ticks — declare them just before the `std::thread::spawn` call:

```rust
            let mut last_pos: Option<(i32, i32)> = None;
            let mut ticks: u32 = 0;
            let mut saved_once = false;
```

Then add this free function to `src-tauri/src/lib.rs` (below `run()`), carrying over every existing comment verbatim and swapping `continue` → `return` and the offset read → `get()`. `Manager` is already in module scope via the top-level `use tauri::{Emitter, Manager};`, so `get_webview_window`/`state` resolve without an extra import:

```rust
/// One iteration of the always-on-top / position-checkpoint loop. Split out
/// of the loop so it can run inside `catch_unwind` (a `continue` can't cross a
/// closure boundary — skips become early `return`s here).
fn checkpoint_tick(
    handle: &tauri::AppHandle,
    last_pos: &mut Option<(i32, i32)>,
    ticks: &mut u32,
    saved_once: &mut bool,
) {
    use tauri_plugin_window_state::{AppHandleExt, StateFlags};

    *ticks = ticks.saturating_add(1);
    let Some(window) = handle.get_webview_window("main") else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    // Windows re-shuffles the topmost band when other topmost windows appear
    // (taskbar previews, flyouts), which can drop the buddy behind the
    // taskbar. No event reaches us, so re-assert always-on-top every tick — a
    // cheap z-order-only SetWindowPos that never moves, resizes, or steals
    // focus. Log a failure instead of swallowing it: a persistent failure is
    // how the buddy silently sinks behind the taskbar.
    if let Err(e) = window.set_always_on_top(true) {
        log::warn!("always-on-top re-assert failed: {e}");
    }
    // Never persist while the panel has the window shifted — only the
    // unshifted home position may reach disk.
    if handle.state::<commands::PanelOffset>().get() != (0, 0) {
        return;
    }
    if let Ok(pos) = window.outer_position() {
        let pos = (pos.x, pos.y);
        let moved = last_pos.is_some() && *last_pos != Some(pos);
        // The early ticks must not write: a save that lands before the
        // window-state plugin's restore would poison its cache with the
        // pre-restore default position. But a drag within that window would
        // be absorbed into the baseline and lost until the next move — so
        // once restore has certainly landed, one unconditional save persists
        // whatever the baseline is. Any successful save (moved or initial)
        // counts.
        let initial = !*saved_once && *ticks >= 3;
        if moved || initial {
            match handle.save_window_state(StateFlags::POSITION) {
                Ok(()) => *saved_once = true,
                Err(e) => log::warn!("position checkpoint failed: {e}"),
            }
        }
        *last_pos = Some(pos);
    }
}
```

Delete the now-duplicated inline body inside the `setup` closure (the old `let mut last_pos …`, `let mut ticks …`, `let mut saved_once …` declarations that were *inside* the closure, and the old `loop { … }` body) — they are replaced by the hoisted declarations + `checkpoint_tick` above.

- [ ] **Step 2: Log a fatal run-loop error instead of an opaque panic**

At the bottom of `run()`, replace:

```rust
        .run(tauri::generate_context!())
        .expect("error while running Vault Buddy");
```

with:

```rust
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            // The run loop failing to start is fatal, but `.expect` would
            // panic with no persisted reason — log it first so the cause
            // survives in the app log.
            log::error!("fatal: Tauri run loop exited: {e}");
            std::process::exit(1);
        });
```

- [ ] **Step 3: Format check**

Run: `cd src-tauri && cargo fmt --check`
Expected: clean. CI's `windows-app` job is the compile gate.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "fix(shell): panic-safe checkpoint tick; log fatal run error"
```

---

### Task 5: Persist logs to a rotating file (shell)

> Shell task — `cargo fmt --check` only; CI compiles.

**Files:**
- Modify: `src-tauri/src/lib.rs` (the `tauri_plugin_log` plugin registration, ~line 23)

- [ ] **Step 1: Replace the bare log-plugin init**

In `src-tauri/src/lib.rs`, replace:

```rust
        .plugin(tauri_plugin_log::Builder::new().build())
```

with:

```rust
        // Persist to a rotating file in the app log dir — the bare `.build()`
        // logged only to stdout, which is invisible in a release GUI build,
        // so crashes left no trail. `LogDir` writes `vault-buddy.log`; the
        // default stdout target is kept for `tauri dev`. 5 MB + KeepOne
        // bounds disk while keeping the one rotated-out file that usually
        // holds the crash preceding a restart. Local timestamps so lines
        // match the user's clock.
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("vault-buddy".into()),
                    },
                ))
                .level(log::LevelFilter::Info)
                .max_file_size(5 * 1024 * 1024)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepOne)
                .timezone_strategy(tauri_plugin_log::TimezoneStrategy::UseLocal)
                .build(),
        )
```

- [ ] **Step 2: Format check**

Run: `cd src-tauri && cargo fmt --check`
Expected: clean. If CI reports an API-name mismatch for `tauri-plugin-log` 2.8.0 (e.g. `RotationStrategy`/`TimezoneStrategy` path), adjust the path against the compiled version — these are the v2.8 names.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(shell): persist logs to a rotating file"
```

---

### Task 6: Panic hook writes a crash record to disk (shell)

> Shell task — `cargo fmt --check` only; CI compiles.

**Files:**
- Create: `src-tauri/src/diagnostics.rs`
- Modify: `src-tauri/src/lib.rs` (declare `mod diagnostics;`, call `install_panic_hook()` first in `run()`, fill `LOG_DIR` in `setup`)

**Interfaces:**
- Consumes: `vault_buddy_core::crash::{CrashRecord, format_crash_record}` (Task 2)
- Produces: `diagnostics::install_panic_hook()`, `diagnostics::set_log_dir(PathBuf)`, `diagnostics::open_log_dir(&AppHandle)` (the last used by Task 7)

- [ ] **Step 1: Create the diagnostics module**

Create `src-tauri/src/diagnostics.rs`:

```rust
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};
use vault_buddy_core::crash::{format_crash_record, CrashRecord};

// The panic hook has no AppHandle, so the resolved app log dir is stashed
// here once `setup` can compute it. Until then the hook falls back to the
// temp dir — a panic in that tiny pre-setup window is still captured.
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

/// Install the process-wide panic hook. MUST run before the Tauri builder so
/// a panic anywhere — including builder construction and background threads —
/// is captured. On Windows a panic on the main thread unwinds across the
/// WebView2 FFI boundary and aborts almost immediately, so an async logger
/// would lose it: the hook writes the record synchronously and flushes it to
/// its own file (separate from the plugin's rotating log to avoid contending
/// for the same handle). Every step is best-effort — the hook must never
/// re-panic.
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
/// spawn/exit failures are ignored (explorer returns nonzero even on
/// success). No `tauri-plugin-opener` dependency — a one-shot spawn is enough.
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
```

- [ ] **Step 2: Declare the module and install the hook first**

In `src-tauri/src/lib.rs`, add to the module declarations at the top (with `mod capture_commands; mod commands; mod tray;`):

```rust
mod diagnostics;
```

Make `install_panic_hook()` the **first statement** of `run()`:

```rust
pub fn run() {
    // Before anything else: a panic during builder construction or in any
    // thread should still be captured on disk.
    diagnostics::install_panic_hook();

    tauri::Builder::default()
```

- [ ] **Step 3: Fill the log dir in `setup`**

Inside the `.setup(|app| { ... })` closure, as the first statements (before `tray::create_tray(...)`):

```rust
            // Give the panic hook the real log dir; until now it falls back
            // to the temp dir.
            if let Ok(dir) = app.path().app_log_dir() {
                diagnostics::set_log_dir(dir);
            }
```

(`app.path()` resolves via `tauri::Manager`, already imported in `lib.rs`.)

- [ ] **Step 4: Format check**

Run: `cd src-tauri && cargo fmt --check`
Expected: clean. CI's `windows-app` job compiles it.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/diagnostics.rs src-tauri/src/lib.rs
git commit -m "feat(shell): panic hook writes a crash record to disk"
```

---

### Task 7: Open logs folder from tray + command (shell)

> Shell task — `cargo fmt --check` only; CI compiles.

**Files:**
- Modify: `src-tauri/src/commands.rs` (add `open_logs_folder` command)
- Modify: `src-tauri/src/lib.rs` (register the command in `invoke_handler`)
- Modify: `src-tauri/src/tray.rs` (`tray_menu` gains an "Open logs folder" item; handle it in `on_menu_event`)

**Interfaces:**
- Consumes: `diagnostics::open_log_dir` (Task 6)
- Produces: IPC command `open_logs_folder` (invoked by Task 10)

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, append:

```rust
/// Reveal the app log folder (holding `vault-buddy.log` and `crash.log`) in
/// the OS file manager, so a user can attach logs after a crash.
#[tauri::command]
pub fn open_logs_folder(app: tauri::AppHandle) {
    crate::diagnostics::open_log_dir(&app);
}
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, add `commands::open_logs_folder` to the `tauri::generate_handler![ ... ]` list (after `commands::show_buddy_menu,`):

```rust
            commands::show_buddy_menu,
            commands::open_logs_folder,
```

- [ ] **Step 3: Add the tray menu item + handler**

In `src-tauri/src/tray.rs`, in `tray_menu`, add an item and include it in both menu variants:

```rust
fn tray_menu(app: &AppHandle, recording: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let toggle = MenuItem::with_id(app, "toggle", "Show / Hide", !recording, None::<&str>)?;
    let logs = MenuItem::with_id(app, "open-logs", "Open logs folder", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit Vault Buddy", true, None::<&str>)?;
    if recording {
        let stop = MenuItem::with_id(
            app,
            "tray-stop-recording",
            "⏹ Stop recording",
            true,
            None::<&str>,
        )?;
        Menu::with_items(app, &[&stop, &toggle, &logs, &quit_item])
    } else {
        Menu::with_items(app, &[&toggle, &logs, &quit_item])
    }
}
```

In the `TrayIconBuilder ... .on_menu_event(|app, event| match event.id.as_ref() { ... })` block, add an arm (before `"quit" => quit(app),`):

```rust
            "open-logs" => crate::diagnostics::open_log_dir(app),
```

- [ ] **Step 4: Format check**

Run: `cd src-tauri && cargo fmt --check`
Expected: clean. CI's `windows-app` job compiles it.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/tray.rs
git commit -m "feat(shell): open logs folder from tray and command"
```

---

### Task 8: Frontend log bridge (frontend)

**Files:**
- Modify: `package.json` (add `@tauri-apps/plugin-log`)
- Modify: `src-tauri/capabilities/default.json` (add `log:default`)
- Create: `src/logging.ts`
- Modify: `src/main.ts` (call `initLogging()`)
- Test: `tests/logging.test.ts`

**Interfaces:**
- Produces:
  - `initLogging(): void` — installs `window` error/rejection forwarders (no-op outside Tauri)
  - `logBreadcrumb(message: string): void` — info-level lifecycle marker
  - `logWarning(message: string): void` — warn-level marker for swallowed failures

- [ ] **Step 1: Add the dependency**

Edit `package.json` — add to `dependencies` (keep alphabetical among the `@tauri-apps/*` entries):

```json
    "@tauri-apps/plugin-log": "^2",
```

Then install:

Run: `npm install`
Expected: `@tauri-apps/plugin-log` resolves to a 2.x version; lockfile updates.

- [ ] **Step 2: Grant the log permission**

Edit `src-tauri/capabilities/default.json` — add to the `permissions` array (after `"core:default",`):

```json
    "log:default",
```

- [ ] **Step 3: Write the failing test**

Create `tests/logging.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from "vitest";

const logMocks = vi.hoisted(() => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-log", () => ({
  info: logMocks.info,
  warn: logMocks.warn,
  error: logMocks.error,
}));

import { initLogging, logBreadcrumb, logWarning } from "../src/logging";

describe("logging bridge", () => {
  beforeEach(() => {
    logMocks.info.mockReset().mockResolvedValue(undefined);
    logMocks.warn.mockReset().mockResolvedValue(undefined);
    logMocks.error.mockReset().mockResolvedValue(undefined);
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  });

  it("no-ops outside Tauri", () => {
    logBreadcrumb("hi");
    logWarning("uh oh");
    expect(logMocks.info).not.toHaveBeenCalled();
    expect(logMocks.warn).not.toHaveBeenCalled();
  });

  it("forwards breadcrumbs and warnings under Tauri", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    logBreadcrumb("drag start @ 10,20");
    logWarning("panel transition failed: boom");
    expect(logMocks.info).toHaveBeenCalledWith("drag start @ 10,20");
    expect(logMocks.warn).toHaveBeenCalledWith("panel transition failed: boom");
  });

  it("forwards uncaught window errors to the log", () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    initLogging();
    window.dispatchEvent(
      new ErrorEvent("error", {
        message: "boom",
        filename: "a.js",
        lineno: 1,
        colno: 2,
      }),
    );
    expect(logMocks.error).toHaveBeenCalledWith(
      "window error: boom @ a.js:1:2",
    );
  });
});
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `npx vitest run tests/logging.test.ts`
Expected: FAIL — cannot resolve `../src/logging`.

- [ ] **Step 5: Implement the bridge**

Create `src/logging.ts`:

```ts
// The single place the app funnels diagnostics through. Every export is a
// no-op unless we're running inside Tauri, so the Vitest/happy-dom suite
// (no Tauri runtime) neither throws nor needs the log plugin. Under Tauri the
// calls reach `@tauri-apps/plugin-log`, whose Rust side writes them into the
// same rotating file the panic hook writes `crash.log` beside.
import {
  error as pluginError,
  warn as pluginWarn,
  info as pluginInfo,
} from "@tauri-apps/plugin-log";

function underTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

// Fire-and-forget: a logging failure must never break the UI or surface as an
// unhandled rejection (which our own handler would then re-log in a loop).
function emit(fn: (message: string) => Promise<void>, message: string): void {
  if (!underTauri()) return;
  try {
    void fn(message).catch(() => {});
  } catch {
    // plugin unavailable — logging must stay invisible to the app
  }
}

/** Info-level lifecycle marker, e.g. "drag start @ 1920,12". */
export function logBreadcrumb(message: string): void {
  emit(pluginInfo, message);
}

/** Warn-level marker for a failure the app otherwise swallows. */
export function logWarning(message: string): void {
  emit(pluginWarn, message);
}

/**
 * Route uncaught frontend errors into the persistent log so a webview fault
 * during a drag leaves a trail alongside the Rust crash record. Idempotent
 * enough for one startup call.
 */
export function initLogging(): void {
  if (!underTauri()) return;
  window.addEventListener("error", (event) => {
    emit(
      pluginError,
      `window error: ${event.message} @ ${event.filename}:${event.lineno}:${event.colno}`,
    );
  });
  window.addEventListener("unhandledrejection", (event) => {
    emit(pluginError, `unhandled rejection: ${String(event.reason)}`);
  });
}
```

- [ ] **Step 6: Call it at startup**

Edit `src/main.ts`:

```ts
import { createApp } from "vue";
import { createPinia } from "pinia";
import App from "./App.vue";
import { initLogging } from "./logging";
import "./style.css";

initLogging();
createApp(App).use(createPinia()).mount("#app");
```

- [ ] **Step 7: Run the test to verify it passes**

Run: `npx vitest run tests/logging.test.ts`
Expected: PASS (all three cases).

- [ ] **Step 8: Commit**

```bash
git add package.json package-lock.json src/logging.ts src/main.ts \
  src-tauri/capabilities/default.json tests/logging.test.ts
git commit -m "feat(ui): frontend log bridge into the app log"
```

---

### Task 9: Drag + geometry breadcrumbs; log swallowed transitions (frontend)

**Files:**
- Modify: `src/App.vue` (`onDragStart` breadcrumb)
- Modify: `src/composables/useCompanionWindow.ts` (`setGeometry` breadcrumb; log the swallowed open/close/queue catches)
- Modify: `tests/companion-window.test.ts` (mock `@tauri-apps/plugin-log`)
- Modify: `tests/app-layout.test.ts` (mock `@tauri-apps/plugin-log`)

**Interfaces:**
- Consumes: `logBreadcrumb`, `logWarning` (Task 8)

- [ ] **Step 1: Breadcrumb the drag start**

In `src/App.vue`, add the import (with the other imports in `<script setup>`):

```ts
import { logBreadcrumb } from "./logging";
```

Update `onDragStart`:

```ts
function onDragStart() {
  dragStartedAt = Date.now();
  dragBlurPending = true;
  logBreadcrumb("buddy drag start");
}
```

- [ ] **Step 2: Breadcrumb geometry changes and log the swallowed catches**

In `src/composables/useCompanionWindow.ts`, add the import at the top:

```ts
import { logBreadcrumb, logWarning } from "../logging";
```

In `setGeometry`, add a breadcrumb before the invoke:

```ts
  function setGeometry(
    pos: { x: number; y: number },
    size: { width: number; height: number },
  ): Promise<void> {
    logBreadcrumb(
      `geometry → ${pos.x},${pos.y} ${size.width}×${size.height}`,
    );
    return invoke("set_window_geometry", {
      x: pos.x,
      y: pos.y,
      width: size.width,
      height: size.height,
    });
  }
```

In `applyOpen`'s `catch` block, replace the comment-only body's silence by logging first (keep the existing fallback behavior):

```ts
    } catch (e) {
      // No window/monitor info — grow right/down in place. Leave any
      // recorded offset untouched so a pending shift is still undone on
      // close.
      logWarning(`applyOpen fell back: ${String(e)}`);
      side.value = "right";
      valign.value = "down";
      await win
        .setSize(new LogicalSize(EXPANDED.width, EXPANDED.height))
        .catch(() => {});
    }
```

In `applyClose`'s `catch` block:

```ts
    } catch (e) {
      // window may be gone during shutdown — best-effort collapse
      logWarning(`applyClose fell back: ${String(e)}`);
      await win
        .setSize(new LogicalSize(COLLAPSED.width, COLLAPSED.height))
        .catch(() => {});
    }
```

In the `watch(panelOpen, …)` queue's `.catch`:

```ts
      .catch((e) => {
        // a failed transition must not wedge the queue
        logWarning(`panel transition failed: ${String(e)}`);
      });
```

- [ ] **Step 3: Keep the transitive-import tests clean**

`useCompanionWindow.ts` and `App.vue` now import `logging.ts`, which imports `@tauri-apps/plugin-log`. Under `mockIPC` these tests count as "under Tauri", so mock the plugin to keep IPC noise out.

In `tests/companion-window.test.ts`, add near the other `vi.mock` calls (top of file):

```ts
vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
```

In `tests/app-layout.test.ts`, add the same block near its other `vi.mock` calls.

- [ ] **Step 4: Run the affected suites**

Run: `npx vitest run tests/companion-window.test.ts tests/app-layout.test.ts tests/companion-character.test.ts`
Expected: PASS — existing behavior unchanged (breadcrumbs are inert with the plugin mocked).

- [ ] **Step 5: Full suite**

Run: `npm test`
Expected: PASS (whole suite green).

- [ ] **Step 6: Commit**

```bash
git add src/App.vue src/composables/useCompanionWindow.ts \
  tests/companion-window.test.ts tests/app-layout.test.ts
git commit -m "feat(ui): drag + geometry breadcrumbs; log swallowed transitions"
```

---

### Task 10: Open logs folder from settings (frontend)

**Files:**
- Create: `src/components/DiagnosticsSettings.vue`
- Modify: `src/components/BuddySettings.vue` (render `<DiagnosticsSettings />` after `<UpdateSettings />`)
- Test: `tests/diagnostics-settings.test.ts`

**Interfaces:**
- Consumes: IPC command `open_logs_folder` (Task 7)

- [ ] **Step 1: Write the failing test**

Create `tests/diagnostics-settings.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import DiagnosticsSettings from "../src/components/DiagnosticsSettings.vue";

describe("DiagnosticsSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => clearMocks());

  it("invokes open_logs_folder when the button is clicked", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return undefined;
    });
    const wrapper = mount(DiagnosticsSettings);
    expect(wrapper.text()).toContain("Diagnostics");
    await wrapper.find('[data-testid="open-logs"]').trigger("click");
    expect(calls).toContain("open_logs_folder");
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `npx vitest run tests/diagnostics-settings.test.ts`
Expected: FAIL — cannot resolve `../src/components/DiagnosticsSettings.vue`.

- [ ] **Step 3: Implement the component**

Create `src/components/DiagnosticsSettings.vue` (styling mirrors `UpdateSettings.vue`):

```vue
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";

// Reveal the folder holding vault-buddy.log + crash.log so the user can
// attach logs after a crash. Guarded: unit tests run without a Tauri runtime.
function openLogs() {
  void invoke("open_logs_folder").catch(() => {
    // not running under Tauri (unit tests) — nothing to open
  });
}
</script>

<template>
  <section>
    <h2
      class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-slate-400"
    >
      Diagnostics
    </h2>
    <div class="rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <span class="text-sm text-slate-200">Logs</span>
        <button
          type="button"
          class="cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          data-testid="open-logs"
          @click="openLogs"
        >
          Open logs folder
        </button>
      </div>
      <p class="mt-1.5 text-xs text-slate-500">
        Share these if the buddy crashes.
      </p>
    </div>
  </section>
</template>
```

- [ ] **Step 4: Render it in the settings view**

Edit `src/components/BuddySettings.vue` — add the import:

```ts
import UpdateSettings from "./UpdateSettings.vue";
import DiagnosticsSettings from "./DiagnosticsSettings.vue";
```

and render it after `<UpdateSettings />` (still inside the closing `</div>`):

```vue
    <UpdateSettings />
    <DiagnosticsSettings />
  </div>
</template>
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `npx vitest run tests/diagnostics-settings.test.ts`
Expected: PASS.

- [ ] **Step 6: Guard the existing BuddySettings suite**

`BuddySettings` now also mounts `DiagnosticsSettings`, which is inert without IPC.

Run: `npx vitest run tests/buddy-settings.test.ts`
Expected: PASS (unchanged — no new IPC is triggered on mount).

- [ ] **Step 7: Full suite + typecheck**

Run: `npm test`
Expected: PASS.

Run: `npm run build`
Expected: `vue-tsc` typecheck passes and the production bundle builds.

- [ ] **Step 8: Commit**

```bash
git add src/components/DiagnosticsSettings.vue src/components/BuddySettings.vue \
  tests/diagnostics-settings.test.ts
git commit -m "feat(ui): open logs folder from settings"
```

---

## Final verification (after all tasks)

- [ ] `npm test` — full Vitest suite green.
- [ ] `npm run build` — `vue-tsc` typecheck + production build succeed.
- [ ] `cd src-tauri && cargo fmt --check` — whole workspace formatted.
- [ ] `cd src-tauri/core && cargo test && cargo clippy --all-targets -- -D warnings` — core green + lint-clean.
- [ ] Push the branch and open a PR; **CI's `windows-app` job is the compile gate for every shell (`src-tauri/src/*.rs`) change** — watch it, since none of Tasks 3–7 can be built locally.

## Post-merge manual verification (Windows, by the maintainer)

The real payoff can only be confirmed on Windows:

1. Launch the app, drag the buddy around, open/close the panel near screen edges.
2. Tray → **Open logs folder** (and Settings → Diagnostics → **Open logs folder**) both reveal the folder; confirm `vault-buddy.log` exists and accrues `drag start` / `geometry →` breadcrumbs.
3. If a crash reproduces, confirm `crash.log` now holds a `==== VAULT BUDDY PANIC …` record with thread, location, and backtrace — the artifact to attach to the bug.

## Spec coverage self-check

- Panic hook → flushed `crash.log`: Task 6. ✓
- Crash-record formatter in core, tested on Linux: Task 2. ✓
- Persistent rotating file log (LogDir, 5 MB, KeepOne, local time): Task 5. ✓
- Frontend error/breadcrumb bridge, no-op outside Tauri: Tasks 8–9. ✓
- Poison-tolerant `PanelOffset` + core-tested helper: Tasks 1, 3. ✓
- `catch_unwind`-wrapped background tick + logged always-on-top failure + logged fatal run error: Task 4. ✓
- "Open logs folder" via command + tray + Settings: Tasks 7, 10. ✓
- `log:default` capability + `@tauri-apps/plugin-log` dep: Task 8. ✓
