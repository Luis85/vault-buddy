# Polish Sub-pass B — Main-Thread Responsiveness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move every long-blocking IPC command off the main thread (GAP-20 `stop_capture`, GAP-21 `start_capture`, GAP-22 the four read-only list commands), following the `search_vaults` async + `spawn_blocking` precedent, with a typed "still saving" stop result instead of a lying bare `Ok`.

**Architecture:** Each command keeps its name, wire shape (except `stop_capture`'s new result DTO), and semantics; only the threading changes — the blocking body runs under `tauri::async_runtime::spawn_blocking`, and the one main-thread-only side effect (`start_capture`'s buddy-window `show`) is marshalled back via `run_on_main_thread`. The frontend already treats `invoke` as a promise, so changes there are limited to typing the new stop result and pinning rejection/still-saving behavior in Vitest.

**Tech Stack:** Rust (Tauri v2 async commands), Vitest + mockIPC for the store tests.

## Global Constraints

- **Branch:** `claude/task-management-vertical-slice-ikeuly`. Never push elsewhere; never amend/rebase existing commits.
- **Bookkeeping (umbrella spec):** each task's Gaps.md entry is tombstoned **in the same commit as its fix**, GAP-40 format: `### GAP-NN · ~~Severity~~ FIXED 2026-07-10 · <original title>`, entry kept with a one-or-two-line what-changed.
- **TDD:** failing/pinning test first for every behavioral change; regression tests name the GAP in a comment. The spec's B-specific evidence: compile-level assertions that the moved commands are `async` (a `#[cfg(test)]` fn-pointer bound that only compiles when the command returns a `Future`), the typed still-saving mapping, and frontend cases for the new rejection/still-saving timing. Threading itself (main-thread freeze vs not) is not unit-observable on Linux — say so, never claim it.
- **Reservation semantics unchanged (spec B2):** names are still reserved under the `CaptureState` mutex before the worker spawns; the double-start guard, timeout/janitor branch, GAP-08 wedged stamp, and GAP-24 spawn degrades move verbatim into the blocking body.
- **Window-thread invariant (AGENTS.md):** window show/hide and window getters run on the MAIN thread only. `start_capture`'s tail (`get_webview_window("main")` + `show`) must be marshalled via `run_on_main_thread`; tray updates off-main are already precedented (the `capture-monitor` thread calls `set_capture_state`).
- **Events stay the source of truth for capture UI state** — `capture:started/saved/failed` drive the store; command return values only add the typed still-saving hint.
- **Gates at every task boundary:** `cd src-tauri && cargo fmt && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib`, `npx tauri build --no-bundle` (repo root), `npm test` (or the touched Vitest files), `npm run check:loc` — `capture_commands.rs` (allowlisted 1073) and `transcription.rs` may grow: `--update` in the same commit **+ a justification line in the commit body** (mandatory — this was missed once in sub-pass A).
- **Commits:** Conventional Commits (`fix(shell)`, `docs(agents)`), one commit per task, ending with the two trailers:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU`

---

### Task 1: `stop_capture` goes async with a typed still-saving result (B1 · GAP-20)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`request_stop_and_wait` return type, `stop_capture`, new `StopOutcomeDto`, tests module)
- Modify: `src/stores/capture.ts` (`stop()` types the result; still-saving keeps the saving UI)
- Modify: `docs/Gaps.md` (GAP-20 tombstone)
- Test: `src-tauri/src/capture_commands.rs` tests module + `tests/capture-store.test.ts`

**Interfaces:**
- Consumes: `request_stop_and_wait(app, wait)` (existing, returns `()` today), `is_recording(&AppHandle) -> bool`, the GAP-08 bypass inside the wait.
- Produces (Task 4 documents these):
  - `#[derive(Clone, Copy, PartialEq, Eq, Debug)] pub enum StopWait { Cleared, TimedOut }` — `request_stop_and_wait` now returns it (early-return-no-active and the GAP-08 bypass both map to `Cleared`; only the bounded-deadline expiry maps to `TimedOut`).
  - `StopOutcomeDto { still_saving: bool }` (camelCase → `stillSaving` on the wire).
  - `pub async fn stop_capture(app: AppHandle) -> Result<StopOutcomeDto, String>` — the `state: tauri::State<CaptureState>` parameter is DROPPED (a `State` borrow can't cross the `spawn_blocking` boundary; the precondition check uses `is_recording`).

- [ ] **Step 1: Write the failing Rust tests**

In `capture_commands.rs`'s existing `#[cfg(test)] mod tests`, add:

```rust
    // GAP-20: the moved commands must be async — this only compiles when
    // stop_capture returns a Future (fn-pointer bound, no runtime needed).
    #[allow(dead_code)]
    fn stop_capture_is_async() {
        fn takes_async<F: std::future::Future>(_: fn(AppHandle) -> F) {}
        takes_async(stop_capture);
    }

    #[test]
    fn stop_outcome_maps_timeout_to_still_saving() {
        // GAP-20 (related-low): the sync command returned a bare Ok(()) on
        // the 15 s timeout, so the frontend saw success while the recording
        // was still finalizing. The typed mapping is the fix's contract.
        assert!(StopOutcomeDto::from_wait(StopWait::TimedOut).still_saving);
        assert!(!StopOutcomeDto::from_wait(StopWait::Cleared).still_saving);
    }
```

- [ ] **Step 2: Run to verify RED**

Run: `cd src-tauri && cargo test -p vault-buddy --lib stop_outcome`
Expected: compile failure — `StopWait`, `StopOutcomeDto`, and the async signature don't exist yet.

- [ ] **Step 3: Implement**

(a) Change `request_stop_and_wait`'s signature and returns (body otherwise unchanged — including the Stop send and the GAP-08 bypass):

```rust
/// Outcome of a stop wait: `Cleared` = the reservation was released (the
/// save landed, there was nothing to wait for, or a startup-wedged
/// reservation was bypassed); `TimedOut` = the bounded deadline expired
/// while finalize was still running — the caller must not report success.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopWait {
    Cleared,
    TimedOut,
}

fn request_stop_and_wait(app: &AppHandle, wait: Option<Duration>) -> StopWait {
```

with `return StopWait::Cleared;` at the no-active early return and the GAP-08 bypass, `return StopWait::TimedOut;` at the deadline-expiry branch (keep its existing `log::warn!`), and `StopWait::Cleared` after the loop exits. `stop_from_menu` and `finalize_if_recording` discard the result with `let _ =` (their logging already covers the timeout case).

(b) The DTO + mapping:

```rust
/// Wire result for stop_capture. `stillSaving` = the bounded wait expired
/// while finalize was still running; the frontend keeps its saving UI and
/// lets capture:saved/failed finish the story (GAP-20).
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopOutcomeDto {
    pub still_saving: bool,
}

impl StopOutcomeDto {
    fn from_wait(wait: StopWait) -> Self {
        Self {
            still_saving: wait == StopWait::TimedOut,
        }
    }
}
```

(c) The command (replaces the sync one; note the dropped `State` param — the handler registration in `lib.rs` needs no edit, the name is unchanged):

```rust
/// ASYNC (GAP-20): the wait is on CaptureState's condvar — up to 15 s of
/// LAME flush + fsync + rename on a slow vault — which froze the whole UI
/// when this ran as a sync command on the main thread. It touches no
/// window APIs and no window-state locks, so the window-thread invariant
/// doesn't pin it; the condvar wait runs on the blocking pool.
#[tauri::command]
pub async fn stop_capture(app: AppHandle) -> Result<StopOutcomeDto, String> {
    if !is_recording(&app) {
        return Err("No recording is running.".to_string());
    }
    let waiter = app.clone();
    let wait = tauri::async_runtime::spawn_blocking(move || {
        request_stop_and_wait(&waiter, Some(Duration::from_secs(15)))
    })
    .await
    .map_err(|e| {
        log::warn!("stop_capture: wait task failed: {e}");
        "Stop failed — see the logs for details.".to_string()
    })?;
    Ok(StopOutcomeDto::from_wait(wait))
}
```

- [ ] **Step 4: Run Rust gates**

Run: `cd src-tauri && cargo fmt && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS (new tests green; no remaining caller passes a `State` to `stop_capture`).

- [ ] **Step 5: Write the failing frontend test**

In `tests/capture-store.test.ts`, next to the existing stop tests (match their mockIPC/setup idiom exactly — read the neighboring cases first):

```ts
it("keeps the saving UI when stop resolves stillSaving (GAP-20)", async () => {
  // A 15s finalize timeout used to resolve as a bare success; the typed
  // result must NOT flip the store out of "saving" — capture:saved/failed
  // events own that transition.
  mockIPC((cmd) => {
    if (cmd === "stop_capture") return { stillSaving: true };
    throw new Error(`unexpected ${cmd}`);
  });
  const store = useCaptureStore();
  store.status = "recording";
  await store.stop();
  expect(store.status).toBe("saving");
  expect(store.error).toBeNull();
});
```

(Adapt the store-construction/mock helpers to the file's existing pattern; the assertion set — status stays `"saving"`, no error — is the contract.)

- [ ] **Step 6: Run to verify it fails or passes honestly**

Run: `npx vitest run tests/capture-store.test.ts`
Expected: the new test may already PASS (the old `stop()` ignores the resolved value, which is the correct behavior) — that is a pinning test, not a driving one; say so in the report. The driving change is Step 7's typing + breadcrumb.

- [ ] **Step 7: Type the result in the store**

In `src/stores/capture.ts` `stop()`:

```ts
    async stop() {
      if (this.status !== "recording") return;
      this.status = "saving";
      try {
        logBreadcrumb("capture: stop requested");
        const r = await invoke<{ stillSaving: boolean }>("stop_capture");
        if (r.stillSaving) {
          // Finalize outlived the bounded wait (slow/network vault). Stay in
          // the saving UI; capture:saved / capture:failed complete the
          // transition (GAP-20 — the old bare Ok looked like success).
          logBreadcrumb("capture: stop still saving after bounded wait");
        }
      } catch (e) {
        this.status = "idle";
        this.error = String(e);
        useNotificationsStore().error(String(e));
        logWarning(`capture stop rejected: ${String(e)}`);
      }
    },
```

- [ ] **Step 8: Run the frontend gates**

Run: `npx vitest run tests/capture-store.test.ts && npm run build`
Expected: PASS (typecheck included via `npm run build`'s vue-tsc).

- [ ] **Step 9: Tombstone GAP-20, LOC baseline, commit**

Gaps.md:

```markdown
### GAP-20 · ~~High~~ FIXED 2026-07-10 · `stop_capture` blocks the main thread for up to 15 s
Now an async command: the condvar wait runs under `spawn_blocking`, and the
15 s expiry returns a typed `{ stillSaving: true }` instead of a bare Ok —
the store keeps its saving UI and the capture events finish the story.
`request_stop_and_wait` returns `StopWait` so no caller can misread a
timeout as success.
```

Run `npm run check:loc`; `--update` + commit-body justification if `capture_commands.rs` grew.

```bash
git add src-tauri/src/capture_commands.rs src/stores/capture.ts tests/capture-store.test.ts docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(shell): make stop_capture async with a typed still-saving result" -m "GAP-20: the sync command held the main thread on CaptureState's condvar for up to 15 s of finalize (LAME flush, fsync, rename) — the exact freeze the tray path avoids by spawning tray-stop — and its timeout still returned Ok, so the frontend saw success mid-finalize. The wait now runs on the blocking pool and the expiry maps to stillSaving, which the store keeps in the saving UI until capture:saved/failed land." -m "LOC baseline: <fill actual numbers if --update ran>." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 2: `start_capture` goes async; the buddy-show tail marshals to the main thread (B2 · GAP-21)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`start_capture` split into async shell + `start_capture_blocking`)
- Modify: `docs/Gaps.md` (GAP-21 tombstone)
- Test: `capture_commands.rs` tests module + `tests/capture-store.test.ts` (rejection path re-verified)

**Interfaces:**
- Consumes: the entire existing `start_capture` body — reservation under the mutex, `capture-warn`/`capture-level`/`capture-device` spawns with their GAP-24 degrades, the 10 s `ready_rx.recv_timeout`, the GAP-08 wedged stamp + janitor, the `capture-monitor` spawn. **All of it moves verbatim** into the blocking fn; this task adds no behavior.
- Produces: `pub async fn start_capture(app: AppHandle, id: String, mode: Option<String>) -> Result<StatusPayload, String>` (the `state: tauri::State<CaptureState>` param is DROPPED — the blocking body binds `let state = app.state::<CaptureState>();` where the old body used the param); private `fn start_capture_blocking(app: &AppHandle, id: String, mode: Option<String>) -> Result<StatusPayload, String>`.

- [ ] **Step 1: Write the failing compile-level test**

In the tests module:

```rust
    // GAP-21: start_capture must be async — compiles only when the command
    // returns a Future.
    #[allow(dead_code)]
    fn start_capture_is_async() {
        fn takes_async<F: std::future::Future>(_: fn(AppHandle, String, Option<String>) -> F) {}
        takes_async(start_capture);
    }
```

- [ ] **Step 2: Run to verify RED**

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: compile failure (`start_capture` is not async / signature mismatch).

- [ ] **Step 3: Implement the split**

(a) Rename the current function to `start_capture_blocking`, remove `#[tauri::command]`, change the signature to `fn start_capture_blocking(app: &AppHandle, id: String, mode: Option<String>) -> Result<StatusPayload, String>`, replace the `state` param uses with a local `let state = app.state::<CaptureState>();` right before the reservation block (NOT earlier — the doc comment's "everything fallible-but-cheap runs before the state lock is touched" ordering stays), and DELETE the success tail (window show + tray + emit + `Ok(payload)` becomes just `Ok(payload)`). Every error path (including the janitor branch's `emit_failed` + `return Err`) stays inside unchanged.

(b) The new async command:

```rust
/// ASYNC (GAP-21): the 10 s device-ready wait (`ready_rx.recv_timeout`)
/// froze the whole UI when this ran as a sync command on the main thread —
/// a wedged audio driver is the timeout's own premise. The body runs on
/// the blocking pool; reservation semantics are unchanged (names reserved
/// under the CaptureState mutex before the worker spawns, double-starts
/// rejected). The one main-thread-only side effect — showing the buddy,
/// the recording indicator — is marshalled back via run_on_main_thread
/// (window show/hide is main-thread-only; tray updates off-main are the
/// capture-monitor precedent).
#[tauri::command]
pub async fn start_capture(
    app: AppHandle,
    id: String,
    mode: Option<String>,
) -> Result<StatusPayload, String> {
    let worker = app.clone();
    let payload = tauri::async_runtime::spawn_blocking(move || {
        start_capture_blocking(&worker, id, mode)
    })
    .await
    .map_err(|e| {
        log::warn!("start_capture: task failed: {e}");
        "Recording start failed — see the logs for details.".to_string()
    })??;
    // Indicator hardening: recording buddy must be visible. Best-effort,
    // same as before — a failed post just loses the show, never the start.
    let shower = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = shower.get_webview_window("main") {
            let _ = window.show();
        }
    });
    crate::tray::set_capture_state(&app, crate::tray::TrayCaptureState::Recording);
    let _ = app.emit("capture:started", payload.clone());
    Ok(payload)
}
```

(The `??` unwraps `Result<Result<_,_>, JoinError>` — map the outer join error as shown, then propagate the inner.)

- [ ] **Step 4: Run the gates**

Run: `cd src-tauri && cargo fmt && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings`
Then: `npx tauri build --no-bundle` (repo root).
Expected: green. In your report, verify point-by-point: (1) reservation/guard code byte-identical inside the blocking fn; (2) no window API remains in the blocking body; (3) the emit still fires AFTER the reservation is live (same as before); (4) `capture_status` and every other consumer of `CaptureState` unchanged.

- [ ] **Step 5: Re-verify the frontend start rejection path**

Read `src/stores/capture.ts` `start()` and its existing Vitest failure case in `tests/capture-store.test.ts`. The rejection timing is unchanged (`invoke` was already awaited); if a rejection case is missing for `start()`, add one mirroring the file's stop-rejection test (status resets, error toast recorded). Run: `npx vitest run tests/capture-store.test.ts`.

- [ ] **Step 6: Tombstone GAP-21, LOC baseline, commit**

Gaps.md:

```markdown
### GAP-21 · ~~High~~ FIXED 2026-07-10 · `start_capture` blocks the main thread for up to 10 s
Now an async command: the whole start body (device-ready wait included)
runs under `spawn_blocking` with reservation semantics unchanged; the
buddy-show indicator tail is marshalled back to the main thread
(window show is main-thread-only).
```

```bash
git add src-tauri/src/capture_commands.rs docs/Gaps.md scripts/loc-baseline.json tests/capture-store.test.ts
git commit -m "fix(shell): make start_capture async; marshal the indicator show to the main thread" -m "GAP-21: the sync command held the main thread through ready_rx.recv_timeout(10s) while the capture-device worker opened WASAPI devices — a slow or wedged driver (the timeout's own premise) froze the UI for the duration. The body now runs on the blocking pool; reservation-under-mutex semantics and the GAP-08/GAP-24 branches move verbatim; the buddy-show tail runs via run_on_main_thread per the window-thread invariant." -m "LOC baseline: <fill if --update ran>." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 3: The four read-only list commands go async (B3 · GAP-22)

**Files:**
- Modify: `src-tauri/src/capture_commands.rs` (`list_recordings`, `list_audio_devices`)
- Modify: `src-tauri/src/task_commands.rs` (`list_tasks`, `count_open_tasks`)
- Modify: `docs/Gaps.md` (GAP-22 tombstone)
- Test: compile-level async assertions in each file's tests module (`task_commands.rs` has none yet — create one)

**Interfaces:**
- Consumes: `services::list_recordings/list_tasks/count_open_tasks` and `vault_buddy_capture::devices::list_devices` (unchanged).
- Produces: same names, same wire shapes, now `pub async fn`. Degrade-to-empty on a panicked task (these commands already degrade to empty on every other failure; an error would newly blank working UI lists).

Per-command window-API verification (the spec demands it be stated in the plan — the implementer re-verifies and reports):
- `list_recordings` → `services::list_recordings`: pure filesystem walk + frontmatter reads. No window APIs, no window-state locks.
- `list_tasks` / `count_open_tasks` → `services::*`: recursive `vault_walk` + frontmatter reads. Same.
- `list_audio_devices` → `vault_buddy_capture::devices::list_devices`: cpal/WASAPI enumeration only. COM on a blocking-pool thread is fine — cpal initializes COM per calling thread, and device work already runs off-main on the `capture-device` worker.

- [ ] **Step 1: Write the failing compile-level tests**

In `capture_commands.rs`'s tests module:

```rust
    // GAP-22: the read-only list commands must be async (blocking fs/COM
    // work belongs on the blocking pool, not the main thread).
    #[allow(dead_code)]
    fn list_commands_are_async() {
        fn takes_async1<F: std::future::Future>(_: fn(String) -> F) {}
        fn takes_async0<F: std::future::Future>(_: fn() -> F) {}
        takes_async1(list_recordings);
        takes_async0(list_audio_devices);
    }
```

Create `#[cfg(test)] mod tests` at the bottom of `task_commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // GAP-22: list_tasks/count_open_tasks must be async — the recursive
    // tasks-folder walk ran on the main thread on every panel open.
    #[allow(dead_code)]
    fn task_list_commands_are_async() {
        fn takes_async<T, F: std::future::Future>(_: fn(String) -> F) {}
        fn is_future<F: std::future::Future>(_: fn(String) -> F) {}
        is_future(list_tasks);
        is_future(count_open_tasks);
    }
}
```

(Drop the unused `takes_async` helper if clippy flags it — one helper suffices.)

- [ ] **Step 2: Run to verify RED**

Run: `cd src-tauri && cargo test -p vault-buddy --lib`
Expected: compile failure (the four are not async).

- [ ] **Step 3: Implement**

`capture_commands.rs`:

```rust
/// ASYNC (GAP-22): COM/WASAPI enumeration commonly takes hundreds of ms;
/// on the main thread that stalled the settings view. cpal initializes COM
/// per calling thread, so the blocking pool is fine (the capture-device
/// worker already enumerates off-main).
#[tauri::command]
pub async fn list_audio_devices() -> DeviceListDto {
    tauri::async_runtime::spawn_blocking(|| {
        let list = vault_buddy_capture::devices::list_devices();
        let map = |d: vault_buddy_capture::devices::DeviceInfo| DeviceInfoDto {
            name: d.name,
            is_default: d.is_default,
        };
        DeviceListDto {
            inputs: list.inputs.into_iter().map(map).collect(),
            outputs: list.outputs.into_iter().map(map).collect(),
        }
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("list_audio_devices: task failed: {e}");
        DeviceListDto {
            inputs: Vec::new(),
            outputs: Vec::new(),
        }
    })
}
```

```rust
#[tauri::command]
pub async fn list_recordings(id: String) -> Vec<RecordingDto> {
    // ASYNC (GAP-22): scans dated folders and reads every companion note's
    // frontmatter — a large archive stalled the UI on every panel open.
    tauri::async_runtime::spawn_blocking(move || services::list_recordings(&ServicePaths::real(), &id))
        .await
        .unwrap_or_else(|e| {
            log::warn!("list_recordings: task failed: {e}");
            Vec::new()
        })
}
```

`task_commands.rs` (same shape):

```rust
#[tauri::command]
pub async fn list_tasks(id: String) -> Vec<TaskDto> {
    // ASYNC (GAP-22): recursive tasks-folder walk — off the main thread.
    tauri::async_runtime::spawn_blocking(move || services::list_tasks(&ServicePaths::real(), &id))
        .await
        .unwrap_or_else(|e| {
            log::warn!("list_tasks: task failed: {e}");
            Vec::new()
        })
}

#[tauri::command]
pub async fn count_open_tasks(id: String) -> usize {
    // ASYNC (GAP-22): same walk as list_tasks, fanned out per vault by the
    // panel's badge refresh.
    tauri::async_runtime::spawn_blocking(move || services::count_open_tasks(&ServicePaths::real(), &id))
        .await
        .unwrap_or_else(|e| {
            log::warn!("count_open_tasks: task failed: {e}");
            0
        })
}
```

(Keep each command's existing doc comment above the new ASYNC paragraph.)

- [ ] **Step 4: Run the gates**

Run: `cd src-tauri && cargo fmt && cargo test -p vault-buddy --lib && cargo clippy --workspace --all-targets -- -D warnings`
Then: `npx tauri build --no-bundle` and `npm test` (the frontend touches nothing, but the full Vitest run pins that promise-based `invoke` sites are agnostic — cheap insurance).
Expected: green.

- [ ] **Step 5: Tombstone GAP-22, LOC baseline, commit**

Gaps.md:

```markdown
### GAP-22 · ~~Medium~~ FIXED 2026-07-10 · Read-only list commands do unbounded filesystem/device work on the main thread
`list_recordings`, `list_tasks`, `count_open_tasks`, and
`list_audio_devices` are async now, each wrapping its filesystem/COM work
in `spawn_blocking` (the `search_vaults` precedent); a panicked task
degrades to the empty value each already used, with a warn.
```

```bash
git add src-tauri/src/capture_commands.rs src-tauri/src/task_commands.rs docs/Gaps.md scripts/loc-baseline.json
git commit -m "fix(shell): move the read-only list commands onto the blocking pool" -m "GAP-22: list_recordings (dated-folder scan + per-note frontmatter reads), list_tasks/count_open_tasks (recursive walk), and list_audio_devices (COM/WASAPI enumeration, commonly hundreds of ms) all ran synchronously on the main thread, stalling the UI on every panel open — the reason search_vaults was made async. None touches a window API or window-state lock." -m "LOC baseline: <fill if --update ran>." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 4: Docs — the IPC table gains sync/async annotations; the sync-command bullet names the new set (B4)

**Files:**
- Modify: `AGENTS.md` (IPC surface table + the architecture-overview sync-command bullet)
- Test: none (docs only; `npm run lint` etc. unaffected). This task is deliberately doc-only — the spec's B4 frontend work (result typing, rejection tests) shipped inside Tasks 1–2 with their backend changes.

- [ ] **Step 1: Annotate the IPC table**

In AGENTS.md's "The IPC surface" section:
- `capture_commands.rs` row: mark `start_capture`, `stop_capture`, `list_recordings`, `list_audio_devices` as `(async)` — e.g. `start_capture` → `start_capture` *(async)* — matching the existing style used for `search_vaults` ("async — deliberate, see search") and `mcp_commands.rs`' annotations.
- `task_commands.rs` row: mark `list_tasks` and `count_open_tasks` `(async)`.
- Keep the table's command count line accurate (the count is unchanged — no commands added or removed).

- [ ] **Step 2: Update the sync-command bullet**

In the architecture overview, the bullet reading "**Sync commands run on the main thread** (that is why window-touching commands are sync and `search_vaults` is async — see the window system section)." becomes:

```markdown
- **Sync commands run on the main thread** — the window-thread invariant
  pins only *window-touching* commands to sync. Everything that does
  blocking filesystem/device work and touches no window API is async on
  the blocking pool: `search_vaults`, `start_capture`, `stop_capture`
  (typed `stillSaving` on its bounded-wait expiry), `list_recordings`,
  `list_tasks`, `count_open_tasks`, `list_audio_devices`, and the MCP
  settings commands. `start_capture`'s buddy-show indicator tail is
  marshalled back via `run_on_main_thread`.
```

Also update the window-system invariant's flip side sentence — "**a sync command must never block** — long work belongs on a worker thread or in an async command (see docs/Gaps.md for the current violations)" — drop the parenthetical (GAP-20/21/22 are fixed; no catalogued violations remain).

- [ ] **Step 3: Cross-check against the code**

`grep -n "pub async fn" src-tauri/src/*.rs` and confirm the annotated set matches exactly. Report the grep output.

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md
git commit -m "docs(agents): annotate the async IPC commands and the window-thread rule" -m "Sub-pass B moved start/stop_capture and the four read-only list commands onto the blocking pool (GAP-20/21/22); the IPC table now marks the async set and the sync-command bullet states the actual rule — only window-touching commands are pinned to sync — instead of pointing at now-fixed violations." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 5: Sub-pass close-out — full gate run

- [ ] **Step 1: Full gate run, CI order**

```bash
rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri/core && cargo test
cd src-tauri/capture && cargo test
cd src-tauri/transcribe && cargo test
cd src-tauri/mcp && cargo test
cd src-tauri && cargo test -p vault-buddy --lib && cargo deny check
npx tauri build --no-bundle
```

(cargo-machete and cargo-llvm-cov run in CI's rust-core job — confirm that job is green on the pushed head rather than claiming a local run.)

- [ ] **Step 2: Verify the ledger**

GAP-20/21/22 tombstones present; one commit per task; ledger updated.

- [ ] **Step 3: Push**

```bash
git push -u origin claude/task-management-vertical-slice-ikeuly
```

PR #46 exists — do not open a new one. Then the final whole-sub-pass review (controller dispatches it on the most capable model) gates the close.

---

## Self-review record

- **Spec coverage:** B1→Task 1 (async + typed still-saving, frontend maps to existing saving UI), B2→Task 2 (async, 10 s wait off-main, reservation semantics unchanged), B3→Task 3 (four commands, spawn_blocking, per-command window-API verification stated), B4→Tasks 1/2 (frontend typing + rejection/still-saving tests, invoke sites otherwise untouched) + Task 4 (AGENTS.md annotations + one-line reason). Spec's testing strategy: compile-level async assertions (Tasks 1–3), typed timeout variant test (Task 1), new-rejection-timing cases (Tasks 1–2), and an honest statement that perceived responsiveness itself is Windows-manual (carried by the umbrella spec's Windows checklist note — restate in each task report).
- **Placeholder scan:** the two `<fill ...>` slots in commit bodies are deliberate run-time values (actual LOC numbers), not design placeholders; everything else is complete code.
- **Type consistency:** `StopWait::{Cleared,TimedOut}` and `StopOutcomeDto::from_wait` used identically in Task 1's test and impl; `start_capture_blocking(&AppHandle, String, Option<String>)` matches the async shell's call; the four B3 signatures match their compile-level assertions.
- **Known risk called out:** dropping `tauri::State` params changes nothing on the wire (Tauri injects state server-side; the frontend never passed it) — stated here so reviewers don't chase it.
