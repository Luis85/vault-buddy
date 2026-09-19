# Screen Capture — Phase 2 (Capture Engine) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record a chosen monitor or window, with any number of selected audio devices mixed to one stereo track, into a crash-survivable fragmented-MP4 file in the app's staging directory — driven from a new panel picker and capture bar, and mutually exclusive with audio recording.

**Architecture:** A new `screen::session` worker owns the capture. Frame acquisition (`windows-capture`) and audio acquisition (the existing cpal path, generalized to N selected devices) each run on their own named thread and hand **plain byte buffers plus a timestamp** to a third thread that exclusively owns the Media Foundation `IMFSinkWriter`. Both stamp from one `screen::clock::CaptureClock`, so A/V sync across a pause is structural. The sink is fragmented MP4 (`MFCreateFMPEG4MediaSink`), which spec §6.4's resolved spike proved stays playable after a process crash. Nothing is written into a vault in this phase — the output lands in `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\`.

**Tech Stack:** Rust 2021 (`vault_buddy_screen`, `vault_buddy_capture`, the Tauri shell crate), `windows-capture` 2.0.1 (MIT, crates.io), the `windows` crate 0.62 (already in the lockfile), `serde`/`serde_json` for the staging sidecar, Vue 3 + Pinia + Tailwind 4 for the picker and capture bar.

**Spec:** `docs/superpowers/specs/2026-09-18-screen-capture-intake-design.md` — §4.1 (crate layout), §6 (capture pipeline), §6.2 (the one clock), §6.3 (pause), §6.4 (fragmented MP4 — **resolved, do not re-spike**), §6.5 (N-source mixing), §7.1–7.3 (entry point, picker, capture bar), §10 (staging), §11 (IPC surface + events), §12 (config, bitrate, keyframe interval), §13 (phase 2 row), §14 (error handling), §15 (testing).

---

## Global Constraints

Every task's requirements implicitly include this section.

### The spike is done. Do not re-run it.

Spec §6.4 is **RESOLVED** as of commit `b2fd94e`. Fragmented MP4 survives a
process crash (280/300 frames decoded); the standard-MP4 control decoded 0.
The chunked-rolling fallback is **not** adopted. `src-tauri/screen/src/fmp4_spike.rs`
is the working reference for `MFCreateFMPEG4MediaSink` →
`MFCreateSinkWriterFromMediaSink` → `WriteSample` → `Finalize`. Read it
before writing `sink.rs`.

Two caveats from §6.4 bind this phase:

- The spike proves survival of a **process crash**, not power loss. Do not
  claim power-loss safety anywhere — in code comments, docs, or UI copy.
- **Do not add a per-fragment byte-stream flush.** The file-size probe that
  once seemed to require one does not measure anything (Windows updates a
  directory entry's size lazily while a handle is open). The retracted probe
  is still in `fmp4_spike.rs`, commented as broken, precisely so nobody
  re-adds the requirement.

### Hard rules

- **`IMFSinkWriter::Flush` DISCARDS pending samples.** Its documentation says
  it "drops all pending samples" — it is a seek-style flush, not a drain.
  Calling it anywhere in the teardown path destroys the capture. **Only
  `Finalize` drains.** Two spike runs died on exactly this. There must be no
  `Flush` call anywhere in `sink.rs` or `session.rs`.
- **Do not trust `std::fs::metadata().len()` on a file Windows has open.** It
  reported 0 bytes across a capture that had written 59 KB. Never gate logic
  or a test on it.
- **No vault write in this phase.** The staging directory lives outside every
  vault (spec §10). The ninth sanctioned vault write is Phase 5's. If a task
  finds itself calling `capture_paths::safe_recording_root` against a vault
  path, it has gone out of scope.
- **Sync commands run on the main thread and must never block.** Only
  `screen_capture_status`, `pause_screen_capture` and `resume_screen_capture`
  are sync (they touch a mutex and a channel send, nothing else).
  `list_capture_sources`, `start_screen_capture` and `stop_screen_capture`
  are `async` on the blocking pool, exactly as their audio counterparts are.
- **Every spawned thread is named** (`std::thread::Builder::new().name(..)`),
  per the diagnostics invariant. This phase's names: `screen-frames`,
  `screen-audio`, `screen-mux`, `screen-warn`, `screen-stats`.
- **No swallowed error.** Anything caught-and-hidden goes through
  `log::warn!`/`log::error!`; every user-facing failure funnels through the
  one `emit_screen_failed` chokepoint, mirroring `capture_commands::emit_failed`.
- **The buddy is the recording indicator for screen capture too.** A live
  screen capture must make `tray::hide_buddy` no-op and must block shutdown,
  exactly as an audio recording does.
- **Sanitize before naming a file after a window title.** A window title can
  contain `:`, `\`, `/`, `?`, `*`, `"`, `<`, `>`, `|`, control characters,
  and can be empty or made entirely of dots. A staged filename derived from
  one without sanitization is a path-traversal bug, not a cosmetic one.

### Exact values, copied from the spec

- Keyframe interval: **1 second**, regardless of quality preset (§12). In MF
  terms `MF_MT_MAX_KEYFRAME_SPACING = fps`.
- Bitrate: `core::screen_capture_config::bitrate_bps(quality, width, height, fps)`
  — already built in Phase 1. Do not re-derive it.
- Frame rate: `core::screen_capture_config::normalize_fps(fps)` — 30 or 60,
  anything else → 30. Already built.
- Audio encode target: **AAC, 48 000 Hz, 2 channels, 16-bit PCM input**,
  128 000 bit/s. (The existing MP3 path targets 44 100 Hz; the AAC encoder
  MFT's supported input rates are 44 100 and 48 000, and 48 000 is the rate
  every Windows AAC encoder is required to accept. See Task 6 for how the
  existing 44.1 kHz mixer output is handled.)
- Video pixel format into the sink: **NV12** (`MFVideoFormat_NV12`), the
  format every hardware H.264 encoder MFT accepts. `windows-capture` delivers
  BGRA; Task 6 converts.
- Staging dir: `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\`
  (spec §10) — resolved from Tauri's `app.path().app_local_data_dir()`, the
  same root `diagnostics` uses for logs.
- Staged file names (spec §10):
  - in progress / crashed: `.<base>.mp4.part` (hidden, dot-prefixed)
  - staged: `<base>.mp4`
  - sidecar: `<base>.json`
  - where `<base>` is `capture_paths::base_name(date, hour, minute, label)`,
    i.e. `YYYY-MM-DD HHmm <label>` — **reuse that function, do not re-grow it.**

### Gates each task must pass before its commit

From `src-tauri/`:

```bash
cargo fmt --check
cargo clippy -p <crate> --all-targets -- -D warnings
cargo test -p <crate>
```

For any task touching `#[cfg(windows)]` code, **additionally**:

```bash
cargo clippy -p <crate> --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

That target is installed and type-checks real `windows`-crate bindings on
Linux. It caught every signature problem in the spike before CI did. Linking
and *running* still need the Windows runner.

For any task touching the frontend or a baseline, from the repo root, **in
this order**, with no `coverage/` directory present when `check:quality` runs:

```bash
rm -rf coverage
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
```

Whole-phase, before declaring the phase done:

```bash
cd src-tauri && cargo machete .
cd src-tauri && cargo deny check
cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture \
  -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94
```

### Traps that have already cost this work real time

- **The LOC guard covers Rust files.** The cap is 800 non-blank lines per
  `.rs` file under a crate's `src/`. `core/src/vault_config.rs` is
  allowlisted at **1282 with zero headroom** — do not add a line to it.
  (This phase does not need to: the seven `screen_*` config fields already
  exist from Phase 1.)
- **Never run `npm run check:loc -- --update`.** It rewrites every `—`
  escape across all 11 baseline entries as a literal em dash, producing a
  20-line diff over unrelated records. Hand-edit the single entry you need.
- **Never put backticks in `git commit -m`.** Bash expands them as command
  substitution and silently deletes the words. Use `git commit -F <file>`.
- **Commit style:** Conventional Commits, imperative subject, body explains
  the *why* and the failure mode being prevented. Scopes in use: `feat(screen)`,
  `fix(screen)`, `feat(capture)`, `feat(shell)`, `feat(ui)`, `ci(...)`,
  `docs(...)`, `chore(loc)`.
- **Branch:** `claude/screen-capture-intake-g0j49q`. **PR #79 is already open
  for it.** Do not open a second PR.

### What this phase does NOT build

Out of scope, each landing in a named later phase. A task that starts on one
of these has misread the plan.

- **Region selection** and the `overlay` window — Phase 3. The picker ships
  `Screen | Window` tabs only. Do not render a disabled Region tab.
- **`WDA_EXCLUDEFROMCAPTURE`** (excluding our own windows from the
  recording) — Phase 3. In Phase 2 the buddy appears in the recording; that
  is expected and goes in the verification checklist as a known Phase-2 state.
- **The `editor` window, preview, timeline UI** — Phase 4.
- **`reader.rs`, `export.rs`, the vault write, the companion note,
  `run_screen_recovery`, resume-or-discard** — Phase 5. Phase 2 writes the
  sidecar (Phase 4 resumes from it) but never scans staging for orphans.
- **`ScreenCaptureConfigTab.vue` and `set_screen_capture_config`** — Phase 6.
  Phase 2 **reads** the per-vault screen config that Phase 1 already parses;
  it adds no settings UI and no config-write command.

---

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `src-tauri/src/capture_guard.rs` | The one shared `CaptureKind` mutual-exclusion guard | Create |
| `src-tauri/src/capture_commands.rs` | Audio capture lifecycle | Modify: claim/release the guard |
| `src-tauri/src/screen_commands.rs` | Screen capture IPC: sources, lifecycle, status, events | Create |
| `src-tauri/src/tray.rs` | Hide chokepoint | Modify: no-op during a screen capture too |
| `src-tauri/src/lib.rs` | Builder, handler registration, shutdown | Modify: register state + 6 commands; block shutdown on a screen capture |
| `src-tauri/screen/Cargo.toml` | Crate manifest | Modify: add `windows-capture`, `windows`, `serde`, `serde_json`, `log` |
| `src-tauri/screen/src/lib.rs` | Crate root, `ScreenError` | Modify: new variants, new `pub mod`s, retire `engine` |
| `src-tauri/screen/src/engine.rs` | Phase-1 stub | **Delete** — `session.rs` supersedes it |
| `src-tauri/screen/src/staging.rs` | Staging paths, base naming, title sanitization, sidecar | Create |
| `src-tauri/screen/src/source.rs` | Monitor/window enumeration; pure id encode/parse | Create |
| `src-tauri/screen/src/sink.rs` | Media Foundation fragmented-MP4 `IMFSinkWriter` wrapper | Create |
| `src-tauri/screen/src/session.rs` | The capture worker: frames + audio → sink, via one mux thread | Create |
| `src-tauri/screen/src/convert.rs` | BGRA → NV12, pure and unit-tested | Create |
| `src-tauri/capture/src/devices.rs` | cpal device enumeration/opening | Modify: `open_selected_sources` for N devices |
| `src/stores/screenCapture.ts` | Screen capture state mirrored from Rust | Create |
| `src/components/ScreenSourcePicker.vue` | Screen/Window tabs + Start | Create |
| `src/components/ScreenAudioPicker.vue` | Multi-select audio checklist | Create |
| `src/components/ScreenCaptureBar.vue` | Live capture bar on the panel list view | Create |
| `src/components/RecordMode.vue` | Intake chooser | Modify: third option, Record Screen |
| `src/components/ActionPanel.vue` | Panel shell + view routing | Modify: route `screenCapture`, render the bar |
| `src/stores/vaults.ts` | Panel view state | Modify: `screenCapture` view + `screenCaptureVaultId` |
| `src/roots/BuddyRoot.vue`, `src/roots/PanelRoot.vue` | Per-window store wiring | Modify: `screenCapture.init()` in both |
| `src/types.ts` | Shared DTO types | Modify: screen capture DTOs |
| `.github/workflows/ci.yml` | CI gates | Modify: `-p vault_buddy_screen` in the `windows-app` test line (GAP-102); retire the spike step |
| `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md` | Manual Windows checklist | Create |
| `AGENTS.md`, `docs/Gaps.md`, `scripts/loc-baseline.json` | Docs + baselines | Modify |

**Task order rationale:** Tasks 1–3 are independent of each other and of the
Windows work. Tasks 4–5 are independent Windows surfaces. Task 6 consumes
2, 3, 4, 5. Task 7 consumes 1 and 6. Task 8 consumes 7's DTOs; Task 9
consumes 8's store. Task 10 is CI/docs and comes last.

---

### Task 1: The shared mutual-exclusion capture guard

Spec §7.3: *"A single shared `CaptureKind` guard in the shell owns this, so
neither domain can be started behind the other's back."*

Today `capture_commands::CaptureState` rejects a second **audio** start. It
knows nothing about screen capture, and a screen-capture state would
symmetrically know nothing about audio. Two independent mutexes checked in
two orders is the textbook two-lock race: both starts read "idle", both
proceed, both fight over the same microphone. This task builds the one guard
both domains claim, and wires **audio** to it — screen claims it in Task 7.

Doing this first, against the domain that already works, means any regression
shows up immediately in the existing audio tests rather than being blamed on
new video code later.

**Files:**
- Create: `src-tauri/src/capture_guard.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod capture_guard;`, `.manage(...)`)
- Modify: `src-tauri/src/capture_commands.rs` (claim in `start_capture_blocking`, release in `clear_active`)

**Interfaces:**
- Consumes: `vault_buddy_core::sync_util::lock_ignoring_poison`.
- Produces: `capture_guard::{CaptureKind, CaptureGuard}` with
  `CaptureKind::{Audio, Screen}`, `CaptureKind::label(self) -> &'static str`,
  `CaptureKind::busy_message(self) -> String`, and on `CaptureGuard`:
  `try_claim(&self, kind: CaptureKind) -> Result<(), CaptureKind>`,
  `release(&self, kind: CaptureKind)`, `active(&self) -> Option<CaptureKind>`.
  Task 7 claims `CaptureKind::Screen` through the same API.

**The lock-ordering invariant this task establishes** (documented in the
module header, because AGENTS.md's concurrency section is where the next
agent will look): `CaptureGuard`'s mutex is **never held while acquiring any
other lock**. `try_claim` takes it, decides, and drops it before returning.
That is what makes "guard, then domain state" safe without a documented
ordering to get wrong.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/capture_guard.rs` containing ONLY the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_idle_guard_grants_either_kind() {
        let g = CaptureGuard::default();
        assert_eq!(g.active(), None);
        assert_eq!(g.try_claim(CaptureKind::Audio), Ok(()));
        assert_eq!(g.active(), Some(CaptureKind::Audio));
    }

    #[test]
    fn a_held_guard_refuses_the_other_kind_and_names_the_holder() {
        // The whole point of the guard (spec 7.3): the refusal must say
        // WHICH kind is running, because the two produce different UI copy
        // ("A recording is already in progress." vs the screen wording).
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Screen).unwrap();
        assert_eq!(g.try_claim(CaptureKind::Audio), Err(CaptureKind::Screen));
        assert_eq!(g.active(), Some(CaptureKind::Screen));
    }

    #[test]
    fn a_held_guard_refuses_a_second_claim_of_its_own_kind() {
        // Regression: the guard must not be re-entrant. An audio double-start
        // was previously rejected only by CaptureState; if the guard granted
        // it, a future refactor that leaned on the guard alone would let two
        // recordings start.
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Audio).unwrap();
        assert_eq!(g.try_claim(CaptureKind::Audio), Err(CaptureKind::Audio));
    }

    #[test]
    fn release_frees_the_guard_for_the_other_kind() {
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Audio).unwrap();
        g.release(CaptureKind::Audio);
        assert_eq!(g.active(), None);
        assert_eq!(g.try_claim(CaptureKind::Screen), Ok(()));
    }

    #[test]
    fn release_of_a_kind_that_is_not_held_never_frees_the_other_kind() {
        // Regression, and the reason `release` takes a kind at all: audio's
        // `clear_active` runs on paths where audio never claimed (a start
        // that failed before the claim, the recovery sweep). An unkeyed
        // release there would free a LIVE screen capture's claim and let a
        // second capture start on top of it.
        let g = CaptureGuard::default();
        g.try_claim(CaptureKind::Screen).unwrap();
        g.release(CaptureKind::Audio);
        assert_eq!(g.active(), Some(CaptureKind::Screen));
    }

    #[test]
    fn release_on_an_idle_guard_is_a_no_op() {
        // `clear_active` is called from several audio paths, some of which
        // never claimed. It must be safe to call unconditionally.
        let g = CaptureGuard::default();
        g.release(CaptureKind::Audio);
        assert_eq!(g.active(), None);
    }

    #[test]
    fn the_busy_message_names_the_running_kind() {
        // Spec 14: the typed `alreadyCapturing` error is rendered by the UI;
        // it must tell the user which capture to stop, not just that one is
        // running.
        assert!(CaptureKind::Audio.busy_message().contains("recording"));
        assert!(CaptureKind::Screen.busy_message().contains("screen capture"));
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

```bash
cd src-tauri && cargo test -p vault-buddy --lib capture_guard
```

Expected: compile error — `CaptureGuard` / `CaptureKind` not found. (The
shell crate needs its GUI deps and a built `../dist` to compile; if `npm run
setup:linux` has not been run in this container, run it once and
`npm run build` to produce `dist/`.)

- [ ] **Step 3: Write the implementation**

Prepend to `src-tauri/src/capture_guard.rs`, above the test module:

```rust
//! The one process-wide "what is capturing right now" guard.
//!
//! A screen capture and an audio recording cannot run concurrently (spec
//! 7.3): both contend for the same audio endpoints, and WASAPI loopback
//! capture of one endpoint from two sessions is a reliability hazard. Two
//! independent per-domain mutexes could not enforce that — each start would
//! see its own domain idle and proceed — so both domains claim THIS guard
//! before touching their own state.
//!
//! LOCK ORDERING: this mutex is never held while any other lock is
//! acquired. `try_claim` takes it, decides, and drops it before returning,
//! so "claim the guard, then take the domain's own state lock" needs no
//! ordering rule to remember and cannot deadlock against the reverse.
//!
//! The guard does not replace `CaptureState`'s own double-start check. It
//! sits in front of it: defence in depth, and the only place that knows
//! about both domains at once.

use std::sync::Mutex;
use vault_buddy_core::sync_util::lock_ignoring_poison;

/// Which capture domain currently owns the devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Audio,
    Screen,
}

impl CaptureKind {
    pub fn label(self) -> &'static str {
        match self {
            CaptureKind::Audio => "audio recording",
            CaptureKind::Screen => "screen capture",
        }
    }

    /// The refusal a start gets when the OTHER kind (or its own) holds the
    /// guard. Names the running kind so the user knows what to stop.
    pub fn busy_message(self) -> String {
        match self {
            CaptureKind::Audio => "A recording is already in progress.".to_string(),
            CaptureKind::Screen => "A screen capture is already in progress.".to_string(),
        }
    }
}

#[derive(Default)]
pub struct CaptureGuard(Mutex<Option<CaptureKind>>);

impl CaptureGuard {
    /// Claim the devices for `kind`. `Err(held)` names the kind that already
    /// holds them — including `kind` itself, because the guard is
    /// deliberately NOT re-entrant.
    pub fn try_claim(&self, kind: CaptureKind) -> Result<(), CaptureKind> {
        let mut guard = lock_ignoring_poison(&self.0);
        match *guard {
            Some(held) => Err(held),
            None => {
                *guard = Some(kind);
                Ok(())
            }
        }
    }

    /// Release the claim, but ONLY if `kind` is what holds it. Audio's
    /// `clear_active` runs on paths that never claimed; an unkeyed release
    /// there would free a live screen capture's claim.
    pub fn release(&self, kind: CaptureKind) {
        let mut guard = lock_ignoring_poison(&self.0);
        if *guard == Some(kind) {
            *guard = None;
        }
    }

    pub fn active(&self) -> Option<CaptureKind> {
        *lock_ignoring_poison(&self.0)
    }
}
```

- [ ] **Step 4: Run the tests and watch them pass**

```bash
cd src-tauri && cargo test -p vault-buddy --lib capture_guard
```

Expected: 7 passed.

- [ ] **Step 5: Register the guard and wire audio to it**

In `src-tauri/src/lib.rs`, add the module beside the others (alphabetical,
after `mod capture_config_commands;`):

```rust
mod capture_guard;
```

and register the state beside the existing `.manage(...)` calls (the block
starting at `.manage(capture_commands::CaptureState::default())`):

```rust
        .manage(capture_guard::CaptureGuard::default())
```

In `src-tauri/src/capture_commands.rs`, add the import at the top:

```rust
use crate::capture_guard::{CaptureGuard, CaptureKind};
```

In `start_capture_blocking`, claim the guard **immediately before** the
existing `CaptureState` reservation block (i.e. just before
`let state = app.state::<CaptureState>();`), and make every early return
between the claim and the successful start release it. Insert:

```rust
    // Mutual exclusion across BOTH capture domains (spec 7.3). Claimed
    // before the per-domain reservation below, and released by
    // `clear_active` — the single chokepoint every failure path already
    // funnels through, so no start path can leak the claim.
    if let Err(held) = app.state::<CaptureGuard>().try_claim(CaptureKind::Audio) {
        return Err(held.busy_message());
    }
```

Then in `clear_active`, release it — this is the chokepoint that makes the
wiring leak-proof:

```rust
fn clear_active(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    *lock_ignoring_poison(&state.0) = None;
    // Release the cross-domain claim from the SAME chokepoint that clears
    // the reservation, so a path that forgets one cannot forget the other.
    // Keyed on Audio: this function also runs on paths where audio never
    // claimed, and an unkeyed release there would free a live screen
    // capture's claim (see capture_guard's tests).
    app.state::<CaptureGuard>().release(CaptureKind::Audio);
    state.1.notify_all();
}
```

**Audit every early return between the claim and the point where the
reservation is installed.** Read `start_capture_blocking` top to bottom: any
`return Err(..)` after the claim that does not go through `clear_active`
leaks the guard and wedges both domains until restart. At the time of
writing, the returns between the claim and the reservation are the
`guard.is_some()` rejection (which cannot be reached after a successful
claim, but must still release), and the device-thread spawn failure (which
already calls `clear_active`). Add the release to the `guard.is_some()` arm:

```rust
        if guard.is_some() {
            drop(guard);
            app.state::<CaptureGuard>().release(CaptureKind::Audio);
            return Err("A recording is already running.".to_string());
        }
```

- [ ] **Step 6: Prove audio still behaves, and the claim does not leak**

Add to `src-tauri/src/capture_guard.rs`'s test module — a structural test, in
the spirit of `config_lock_guard.rs`, that the wiring cannot silently rot:

```rust
    // Structural regression: the guard is released from exactly one place in
    // the audio domain (`clear_active`). If a future edit adds a second
    // release site, or moves the one release out of the chokepoint, the
    // claim can leak on some path and BOTH capture domains wedge until the
    // app restarts — a failure with no error message and no log line. This
    // scan fails loudly instead.
    #[test]
    fn audio_releases_the_guard_only_from_the_clear_active_chokepoint() {
        let src = include_str!("capture_commands.rs");
        let releases = src.matches("release(CaptureKind::Audio)").count();
        assert_eq!(
            releases, 2,
            "expected exactly two Audio releases in capture_commands.rs \
             (clear_active, and the defensive already-reserved arm); found {releases}. \
             If you added a release site, funnel it through clear_active instead."
        );
    }
```

```bash
cd src-tauri && cargo test -p vault-buddy --lib
```

Expected: the whole shell suite passes, including the existing capture tests.

- [ ] **Step 7: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings
cd src-tauri && cargo test -p vault-buddy --lib
```

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/src/capture_guard.rs src-tauri/src/lib.rs src-tauri/src/capture_commands.rs
cat > /tmp/msg.txt <<'EOF'
feat(shell): add the cross-domain capture guard and claim it from audio

Screen capture and audio recording cannot run concurrently (spec 7.3): they
contend for the same audio endpoints, and WASAPI loopback capture of one
endpoint from two sessions is a reliability hazard. CaptureState rejects a
second AUDIO start but knows nothing about screen capture; a symmetric
screen-side mutex would know nothing about audio, so both starts would read
their own domain idle and proceed.

CaptureGuard is the one place that knows about both. It is claimed before
each domain's own reservation and released from that domain's single clear
chokepoint, so no failure path can leak the claim and wedge both domains.
Its mutex is never held while another lock is taken, so "guard then domain
state" needs no ordering rule to remember.

Release is keyed on the claiming kind: clear_active also runs on paths where
audio never claimed, and an unkeyed release there would free a live screen
capture's claim and let a second capture start on top of it.

Screen capture claims the same guard when its commands land.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 2: Staging paths, title sanitization, and the sidecar

Spec §10. Everything here is **pure path and string logic**, so it compiles
and is fully tested on Linux — which is the whole reason the spec puts it in
its own module rather than inline in the session worker.

**Files:**
- Create: `src-tauri/screen/src/staging.rs`
- Modify: `src-tauri/screen/src/lib.rs` (add `pub mod staging;`)
- Modify: `src-tauri/screen/Cargo.toml` (add `serde`, `serde_json`, `chrono`)

**Interfaces:**
- Consumes: `vault_buddy_core::capture_paths::base_name`.
- Produces:
  - `staging::STAGING_DIR_NAME: &str` (`"screen-captures"`)
  - `staging::staging_dir(local_app_data: &Path) -> PathBuf`
  - `staging::sanitize_title(raw: &str) -> String`
  - `staging::part_file_name(base: &str) -> String`
  - `staging::base_from_part(file_name: &str) -> Option<String>`
  - `staging::mp4_file_name(base: &str) -> String`
  - `staging::sidecar_file_name(base: &str) -> String`
  - `staging::reserve_base(dir: &Path, base: &str) -> String`
  - `staging::StagedSidecar { base, vault_id, source_title, source_kind, inputs, duration_ms, paused_ms, width, height, recorded_at, timeline }`
    (`Serialize + Deserialize`, camelCase)
  - `staging::write_sidecar(dir: &Path, sidecar: &StagedSidecar) -> std::io::Result<PathBuf>`
  - `staging::read_sidecar(path: &Path) -> Option<StagedSidecar>`

  Task 6 calls `part_file_name` / `mp4_file_name` / `write_sidecar`; Task 7
  calls `staging_dir` / `sanitize_title` / `reserve_base`; Phase 4's editor
  calls `read_sidecar`.

- [ ] **Step 1: Add the dependencies**

In `src-tauri/screen/Cargo.toml`, under `[dependencies]` (these are
platform-independent — the staging module is pure and tested on Linux):

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", default-features = false, features = ["clock", "serde"] }
```

`cargo machete` fails on a declared-but-unused dependency, so add each one
only in the task that first uses it. All three are used by the end of this
task.

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/screen/src/staging.rs` containing ONLY the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn staging_lives_beside_the_logs_not_inside_any_vault() {
        // Spec 10: an unedited, unapproved capture is not knowledge. If this
        // ever resolved under a vault path, discard would leave litter in the
        // user's notes and the app would be writing to a vault nobody asked
        // it to touch.
        let dir = staging_dir(&PathBuf::from("/lad/com.vaultbuddy.desktop"));
        assert_eq!(
            dir,
            PathBuf::from("/lad/com.vaultbuddy.desktop/screen-captures")
        );
    }

    #[test]
    fn sanitize_strips_every_windows_reserved_filename_character() {
        // A window title is attacker-adjacent input: it is whatever the app
        // being recorded chose to put in its title bar. Unsanitized, a title
        // containing a separator escapes the staging directory outright.
        assert_eq!(sanitize_title(r#"a:b\c/d?e*f"g<h>i|j"#), "a-b-c-d-e-f-g-h-i-j");
    }

    #[test]
    fn sanitize_drops_control_characters_rather_than_replacing_them() {
        assert_eq!(sanitize_title("Ti\u{0}tle\u{1f}"), "Title");
    }

    #[test]
    fn sanitize_collapses_runs_and_trims_the_edges() {
        // Without collapsing, "A :: B" becomes "A -- B"; without trimming,
        // Windows silently strips a trailing dot or space from a filename,
        // so the name on disk stops matching the name we reserved.
        assert_eq!(sanitize_title("  A ::  B . "), "A - B");
    }

    #[test]
    fn sanitize_never_returns_an_empty_or_dot_only_name() {
        // A title of "..." or "" must not produce "." or "" — both are
        // directory references, not file names, and `dir.join(".")` is the
        // staging directory itself.
        assert_eq!(sanitize_title(""), "Screen Capture");
        assert_eq!(sanitize_title("..."), "Screen Capture");
        assert_eq!(sanitize_title("   "), "Screen Capture");
    }

    #[test]
    fn sanitize_bounds_the_length_so_the_full_path_stays_writable() {
        // MAX_PATH is 260 by default; a 300-character window title plus the
        // staging path plus " (12).mp4.part" overruns it and the create
        // fails with a bewildering OS error at capture start.
        let long = "x".repeat(300);
        assert_eq!(sanitize_title(&long).chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn the_part_file_is_hidden_and_round_trips_back_to_its_base() {
        // Mirrors the audio domain's .mp3.part convention: dot-prefixed so
        // Obsidian and Explorer ignore an in-progress capture.
        let base = "2026-09-18 1432 Figma walkthrough";
        let part = part_file_name(base);
        assert_eq!(part, ".2026-09-18 1432 Figma walkthrough.mp4.part");
        assert_eq!(base_from_part(&part).as_deref(), Some(base));
    }

    #[test]
    fn base_from_part_rejects_anything_that_is_not_our_own_part_file() {
        // Recovery (phase 5) deletes what this recognizes. A loose match
        // would let it delete a user file that happens to live in the
        // staging directory.
        assert_eq!(base_from_part("notes.mp4"), None);
        assert_eq!(base_from_part(".notes.mp3.part"), None);
        assert_eq!(base_from_part("notes.mp4.part"), None, "must be dot-prefixed");
        assert_eq!(base_from_part(".mp4.part"), None, "empty base");
    }

    #[test]
    fn the_staged_and_sidecar_names_derive_from_the_same_base() {
        let base = "2026-09-18 1432 Figma walkthrough";
        assert_eq!(mp4_file_name(base), "2026-09-18 1432 Figma walkthrough.mp4");
        assert_eq!(sidecar_file_name(base), "2026-09-18 1432 Figma walkthrough.json");
    }

    #[test]
    fn reserve_base_returns_the_plain_base_when_nothing_collides() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap");
    }

    #[test]
    fn reserve_base_suffixes_past_any_of_the_three_names_it_owns() {
        // The pairwise reservation the audio domain uses: a base is free only
        // when the .mp4, the .json AND the .mp4.part are all free. Checking
        // only the .mp4 would let a second capture reuse a base whose
        // in-progress .part still exists, and the two would fight over one
        // file.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cap.mp4"), b"x").unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap (2)");

        std::fs::write(dir.path().join("cap (2).json"), b"x").unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap (3)");

        std::fs::write(dir.path().join(part_file_name("cap (3)")), b"x").unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap (4)");
    }

    #[test]
    fn a_sidecar_round_trips_through_json() {
        let s = StagedSidecar {
            base: "2026-09-18 1432 Demo".into(),
            vault_id: "abc123".into(),
            source_title: "Figma \u{2014} Design System".into(),
            source_kind: "window".into(),
            inputs: vec!["Microphone (Yeti)".into()],
            duration_ms: 197_000,
            paused_ms: 12_000,
            width: 1920,
            height: 1080,
            recorded_at: "2026-09-18T14:32:00+02:00".into(),
            timeline: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = write_sidecar(dir.path(), &s).unwrap();
        let back = read_sidecar(&path).expect("sidecar reads back");
        assert_eq!(back.base, s.base);
        assert_eq!(back.duration_ms, 197_000);
        assert_eq!(back.source_title, "Figma \u{2014} Design System");
    }

    #[test]
    fn read_sidecar_degrades_to_none_rather_than_erroring() {
        // Same defensive-read posture as the rest of the vault domain: a
        // hand-edited or truncated sidecar must make ONE capture
        // un-resumable, never fail the whole staging scan.
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, b"{ not json").unwrap();
        assert!(read_sidecar(&bad).is_none());
        assert!(read_sidecar(&dir.path().join("missing.json")).is_none());
    }

    #[test]
    fn the_sidecar_serializes_camel_case_for_the_webview() {
        // Phase 4's editor reads this through the IPC boundary, where every
        // other DTO in this app is camelCase. A snake_case key here would
        // deserialize to undefined in the editor with no error.
        let s = StagedSidecar {
            base: "b".into(), vault_id: "v".into(), source_title: "t".into(),
            source_kind: "screen".into(), inputs: vec![], duration_ms: 1,
            paused_ms: 0, width: 2, height: 2, recorded_at: "r".into(),
            timeline: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"vaultId\""), "got {json}");
        assert!(json.contains("\"durationMs\""), "got {json}");
        assert!(!json.contains("vault_id"), "got {json}");
    }
}
```

`tempfile` is already a `[dev-dependencies]` entry in the workspace's other
crates; add it to `src-tauri/screen/Cargo.toml` under `[dev-dependencies]`
if it is not there:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run the tests and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen staging
```

Expected: compile error — nothing in `staging` is defined yet.

- [ ] **Step 4: Write the implementation**

Prepend to `src-tauri/screen/src/staging.rs`:

```rust
//! Staging: where a capture lives between "stopped" and "saved into a
//! vault".
//!
//! Staging is deliberately OUTSIDE every vault (spec 10). An unedited,
//! unapproved capture is not knowledge; putting it in a vault would make
//! discard leave litter in the user's notes and would mean the app writes
//! to a vault the user never asked it to touch.
//!
//! Everything here is pure path and string logic, so it compiles and is
//! tested on Linux — which is the point: no CI runner can record a screen,
//! so every rule that CAN be checked without a screen is checked here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The staging directory's name under the app's local data dir.
pub const STAGING_DIR_NAME: &str = "screen-captures";

/// Longest title we will put in a file name. MAX_PATH is 260 by default on
/// Windows; the staging path, the `YYYY-MM-DD HHmm ` prefix, a possible
/// ` (12)` collision suffix and `.mp4.part` all have to fit alongside it.
pub const MAX_TITLE_CHARS: usize = 64;

/// Fallback when a title sanitizes to nothing. Never an empty string and
/// never ".": both are directory references, and `dir.join(".")` is the
/// staging directory itself, not a file in it.
const FALLBACK_TITLE: &str = "Screen Capture";

const PART_SUFFIX: &str = ".mp4.part";

pub fn staging_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join(STAGING_DIR_NAME)
}

/// Turn a window or monitor title into something safe to put in a file
/// name.
///
/// This is a security boundary, not cosmetics: a window title is whatever
/// the recorded application chose to put in its title bar, and an
/// unsanitized separator in it escapes the staging directory.
pub fn sanitize_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            // Windows-reserved, plus the separators.
            ':' | '\\' | '/' | '?' | '*' | '"' | '<' | '>' | '|' => out.push('-'),
            // Control characters are dropped outright rather than replaced:
            // replacing them would turn an invisible character into a
            // visible dash the user never typed.
            c if c.is_control() => {}
            c => out.push(c),
        }
    }

    // Collapse runs of separators and whitespace so "A ::  B" reads as
    // "A - B" rather than "A --  B".
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_sep = false;
    for ch in out.chars() {
        let sep = ch == '-' || ch.is_whitespace();
        if sep {
            if !prev_sep {
                collapsed.push(if ch == '-' { '-' } else { ' ' });
            } else if ch == '-' {
                // A dash beats a space in a run: "A - B" not "A   B".
                if collapsed.ends_with(' ') {
                    collapsed.pop();
                    collapsed.push('-');
                }
            }
            prev_sep = true;
        } else {
            collapsed.push(ch);
            prev_sep = false;
        }
    }

    // Windows silently strips a trailing dot or space from a file name, so a
    // name ending in either stops matching the name we reserved.
    let trimmed: String = collapsed
        .trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '-')
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    let trimmed = trimmed
        .trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '-')
        .to_string();

    if trimmed.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        trimmed
    }
}

/// The hidden in-progress file, mirroring the audio domain's `.mp3.part`.
pub fn part_file_name(base: &str) -> String {
    format!(".{base}{PART_SUFFIX}")
}

/// Recover the base from a part file name, or `None` when the name is not
/// one of ours. Phase 5's recovery deletes what this recognizes, so it is
/// deliberately strict — a loose match would let recovery delete a user
/// file that happens to sit in the staging directory.
pub fn base_from_part(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix('.')?;
    let base = rest.strip_suffix(PART_SUFFIX)?;
    if base.is_empty() {
        return None;
    }
    Some(base.to_string())
}

pub fn mp4_file_name(base: &str) -> String {
    format!("{base}.mp4")
}

pub fn sidecar_file_name(base: &str) -> String {
    format!("{base}.json")
}

/// Find a free base in `dir`, suffixing ` (N)` on collision.
///
/// A base is free only when ALL THREE names it owns are free — the staged
/// `.mp4`, the `.json` sidecar and the hidden `.mp4.part`. This is the
/// pairwise reservation the audio domain uses, widened to three: checking
/// only the `.mp4` would let a second capture adopt a base whose
/// in-progress `.part` still exists, and the two captures would then write
/// the same file.
pub fn reserve_base(dir: &Path, base: &str) -> String {
    let free = |candidate: &str| {
        !dir.join(mp4_file_name(candidate)).exists()
            && !dir.join(sidecar_file_name(candidate)).exists()
            && !dir.join(part_file_name(candidate)).exists()
    };
    if free(base) {
        return base.to_string();
    }
    for n in 2..10_000 {
        let candidate = format!("{base} ({n})");
        if free(&candidate) {
            return candidate;
        }
    }
    // Astronomically unreachable; a timestamped fallback beats a panic in a
    // capture-start path.
    format!("{base} ({})", std::process::id())
}

/// What the editor needs to resume a staged capture (spec 10).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedSidecar {
    pub base: String,
    pub vault_id: String,
    pub source_title: String,
    /// `"screen"` or `"window"` — a string, not an enum, so a sidecar
    /// written by a future version that adds a source kind still
    /// deserializes here instead of failing the whole file.
    pub source_kind: String,
    pub inputs: Vec<String>,
    pub duration_ms: u64,
    pub paused_ms: u64,
    pub width: u32,
    pub height: u32,
    pub recorded_at: String,
    /// The in-progress edit, saved on each editor operation (phase 4).
    /// `None` until the editor touches it.
    #[serde(default)]
    pub timeline: Option<serde_json::Value>,
}

pub fn write_sidecar(dir: &Path, sidecar: &StagedSidecar) -> std::io::Result<PathBuf> {
    let path = dir.join(sidecar_file_name(&sidecar.base));
    let json = serde_json::to_vec_pretty(sidecar)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Read a sidecar, degrading to `None` on anything unreadable.
///
/// Same defensive-read posture as the rest of the vault domain: a
/// hand-edited or truncated sidecar must make ONE capture un-resumable,
/// never fail the whole staging scan.
pub fn read_sidecar(path: &Path) -> Option<StagedSidecar> {
    let bytes = std::fs::read(path)
        .map_err(|e| log::warn!("screen staging: cannot read sidecar {}: {e}", path.display()))
        .ok()?;
    serde_json::from_slice(&bytes)
        .map_err(|e| log::warn!("screen staging: malformed sidecar {}: {e}", path.display()))
        .ok()
}
```

`read_sidecar` logs, so the crate needs `log` back. Add to
`src-tauri/screen/Cargo.toml` under `[dependencies]`:

```toml
log = "0.4"
```

and delete the Phase-1 comment in that file that says `log` is deliberately
absent — it is now used, and a stale comment claiming otherwise is worse
than none.

- [ ] **Step 5: Register the module**

In `src-tauri/screen/src/lib.rs`, beside the existing `pub mod` lines:

```rust
pub mod staging;
```

- [ ] **Step 6: Run the tests and watch them pass**

```bash
cd src-tauri && cargo test -p vault_buddy_screen staging
```

Expected: 14 passed.

- [ ] **Step 7: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings
cd src-tauri && cargo test -p vault_buddy_screen
cd src-tauri && cargo machete .
```

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/screen/src/staging.rs src-tauri/screen/src/lib.rs src-tauri/screen/Cargo.toml
cat > /tmp/msg.txt <<'EOF'
feat(screen): add staging paths, title sanitization and the resume sidecar

Spec 10. A staged capture lives outside every vault, because an unedited,
unapproved capture is not knowledge: staging it in a vault would make
discard leave litter in the user's notes and would mean the app writes to a
vault nobody asked it to touch.

Title sanitization is a security boundary, not cosmetics. A window title is
whatever the recorded application put in its title bar; an unsanitized
separator in one escapes the staging directory. Control characters are
dropped rather than replaced, trailing dots and spaces are trimmed (Windows
silently strips them, so the name on disk would stop matching the name we
reserved), and the result is length-bounded so a long title plus the
staging path plus a collision suffix cannot overrun MAX_PATH at capture
start. A title that sanitizes to nothing falls back to a real name, never
to "." — joining "." would target the staging directory itself.

reserve_base treats a base as free only when its .mp4, .json and .mp4.part
are ALL free. Checking the .mp4 alone would let a second capture adopt a
base whose in-progress part file still exists, and both would write it.

read_sidecar degrades to None rather than erroring, so one hand-edited
sidecar makes a single capture un-resumable instead of failing the scan.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 3: Open N explicitly-selected audio devices

Spec §1.3 and §7.2: the screen-capture picker offers a **multi-select**
checklist of inputs and loopback outputs. `devices::open_sources` today
opens exactly one microphone plus, in meeting mode, one loopback output, and
it **falls back to the default device** when a configured name is missing —
correct for a stale *configuration*, wrong for an *explicit pick the user
just made in front of the device list*. Silently recording a different
microphone than the one ticked is worse than recording nothing.

So this is a sibling function, not a rewrite: `open_sources` keeps its
behaviour byte-for-byte (the audio domain's regression surface), and the new
`open_selected_sources` has the posture the picker needs — **skip a missing
pick with a warning, never substitute**.

**Files:**
- Modify: `src-tauri/capture/src/devices.rs`

**Interfaces:**
- Consumes: the existing private `find_by_name`, `build_stream`,
  `device_name`, and `OpenSources` / `SourceInput`.
- Produces: `devices::DeviceSelection { inputs: Vec<String>, outputs: Vec<String> }`
  and `devices::open_selected_sources(sel: &DeviceSelection) -> Result<OpenSources, String>`.
  Task 6 feeds the resulting `OpenSources.inputs` into the screen session;
  Task 7 builds the `DeviceSelection` from the IPC arguments.

**Behaviour contract** (each clause exists because the alternative is a
silent wrong recording):

| Case | Behaviour |
| --- | --- |
| A named input resolves | opened, added to `inputs` |
| A named input is missing | **skipped**, a warning naming it, others continue |
| A named output resolves (Windows) | opened as WASAPI loopback, suffixed `" (loopback)"` |
| A named output is missing | **skipped**, a warning naming it |
| An output is named on non-Windows | skipped with a warning (loopback is Windows-only) |
| The selection is empty | `Ok` with zero inputs — **a legal, silent capture** (spec §6.5: "a silent UI demo is a real use case and blocking Start on it would be wrong") |
| Every named device is missing | `Ok` with zero inputs and one warning per device — the capture records silently rather than failing, matching the audio domain's "a device vanishing mid-capture drops that source, the rest keeps recording" posture |

- [ ] **Step 1: Write the failing tests**

Append to the existing `mod tests` in `src-tauri/capture/src/devices.rs`:

```rust
    #[test]
    fn an_empty_selection_opens_nothing_and_is_not_an_error() {
        // Spec 6.5: zero audio sources is a legal, documented outcome — a
        // silent UI demo is a real use case, and refusing Start on it would
        // be wrong. This must hold on a CI runner with no devices at all.
        let open = open_selected_sources(&DeviceSelection::default())
            .expect("an empty selection is not a failure");
        assert!(open.inputs.is_empty());
        assert!(open.streams.is_empty());
        assert!(open.warnings.is_empty(), "nothing was asked for, so nothing is missing");
    }

    #[test]
    fn a_missing_pick_is_skipped_with_a_warning_and_never_substituted() {
        // THE behavioural difference from open_sources. That function falls
        // back to the default device, which is right for a stale config and
        // wrong for a pick the user just made in front of the device list:
        // recording a different microphone than the one they ticked is worse
        // than recording nothing. The capture proceeds silently and says so.
        let sel = DeviceSelection {
            inputs: vec!["No Such Device 9000".to_string()],
            outputs: vec![],
        };
        let open = open_selected_sources(&sel).expect("a missing pick does not fail the start");
        assert!(
            open.inputs.is_empty(),
            "a missing pick must NOT be substituted with the default device"
        );
        assert!(
            open.warnings.iter().any(|w| w.contains("No Such Device 9000")),
            "the warning names the missing device: {:?}",
            open.warnings
        );
    }

    #[test]
    fn a_missing_output_pick_is_reported_on_every_platform() {
        // On Windows it is a missing loopback endpoint; elsewhere loopback
        // is unavailable entirely. Either way the user ticked something that
        // will not be recorded, so either way they are told.
        let sel = DeviceSelection {
            inputs: vec![],
            outputs: vec!["No Such Output 9000".to_string()],
        };
        let open = open_selected_sources(&sel).expect("a missing output does not fail the start");
        assert!(open.inputs.is_empty());
        assert!(
            !open.warnings.is_empty(),
            "an unrecorded pick is always reported"
        );
    }

    #[test]
    fn open_selected_sources_never_panics_on_a_real_selection() {
        // Mirrors open_sources_never_panics: on a device-less CI runner this
        // exercises the skip-and-warn path; on a dev machine it opens real
        // streams.
        let list = list_devices();
        let sel = DeviceSelection {
            inputs: list.inputs.iter().map(|d| d.name.clone()).collect(),
            outputs: vec![],
        };
        match open_selected_sources(&sel) {
            Ok(open) => assert!(open.inputs.iter().all(|i| !i.name.is_empty())),
            Err(message) => assert!(!message.is_empty()),
        }
    }

    #[test]
    fn open_sources_keeps_its_fallback_posture_unchanged() {
        // Regression guard for the split: the audio domain's stale-config
        // fallback must not be "unified" into the new skip-and-warn posture
        // by a later tidy-up. A meeting recording whose configured mic was
        // unplugged still records on the default device.
        match open_sources(false, Some("No Such Device 9000"), None) {
            Ok(open) => {
                assert!(
                    !open.inputs.is_empty(),
                    "open_sources substitutes the default; it does not skip"
                );
                assert!(open
                    .warnings
                    .iter()
                    .any(|w| w.contains("No Such Device 9000")));
            }
            Err(message) => assert!(!message.is_empty()), // device-less CI
        }
    }
```

- [ ] **Step 2: Run the tests and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_capture devices
```

Expected: compile error — `DeviceSelection` / `open_selected_sources` not
found. (Linux needs `libasound2-dev`; it is already installed.)

- [ ] **Step 3: Write the implementation**

Append to `src-tauri/capture/src/devices.rs`, after `open_sources`:

```rust
/// An explicit, user-made multi-select of audio endpoints (spec 7.2).
#[derive(Debug, Clone, Default)]
pub struct DeviceSelection {
    /// Input device names, exactly as `list_devices` reported them.
    pub inputs: Vec<String>,
    /// Output device names to capture as WASAPI loopback (Windows only).
    pub outputs: Vec<String>,
}

/// Open every explicitly-selected endpoint.
///
/// Deliberately NOT `open_sources` with a list parameter. `open_sources`
/// substitutes the DEFAULT device when a configured name is missing, which
/// is right for a stale config and wrong here: these names were ticked by
/// the user seconds ago in front of the live device list, so recording a
/// different microphone than the one they picked is worse than recording
/// nothing. A missing pick is SKIPPED with a warning that names it.
///
/// An empty selection — asked for, or left over after every pick went
/// missing — is a legal outcome, not an error: spec 6.5 keeps a silent
/// capture available because a silent UI demo is a real use case.
pub fn open_selected_sources(sel: &DeviceSelection) -> Result<OpenSources, String> {
    let host = cpal::default_host();
    let mut inputs = Vec::new();
    let mut streams = Vec::new();
    let mut warnings = Vec::new();

    for name in &sel.inputs {
        let Some(device) = host
            .input_devices()
            .ok()
            .and_then(|it| find_by_name(it, name))
        else {
            let w = format!("Selected microphone \"{name}\" is no longer available — it will not be recorded");
            log::warn!("screen capture: {w}");
            warnings.push(w);
            continue;
        };
        let config = match device.default_input_config() {
            Ok(c) => c,
            Err(e) => {
                let w = format!("Selected microphone \"{name}\" could not be opened ({e}) — it will not be recorded");
                log::warn!("screen capture: {w}");
                warnings.push(w);
                continue;
            }
        };
        let (tx, rx) = std::sync::mpsc::channel();
        match build_stream(&device, &config, tx) {
            Ok(stream) => {
                streams.push(stream);
                inputs.push(SourceInput {
                    name: name.clone(),
                    rate: config.sample_rate(),
                    channels: config.channels(),
                    rx,
                });
            }
            Err(e) => {
                let w = format!("Selected microphone \"{name}\" could not be started ({e}) — it will not be recorded");
                log::warn!("screen capture: {w}");
                warnings.push(w);
            }
        }
    }

    for name in &sel.outputs {
        #[cfg(windows)]
        {
            // WASAPI loopback: cpal exposes it by building an *input* stream
            // on an *output* device — you get exactly what the speakers play.
            let Some(device) = host
                .output_devices()
                .ok()
                .and_then(|it| find_by_name(it, name))
            else {
                let w = format!("Selected desktop audio \"{name}\" is no longer available — it will not be recorded");
                log::warn!("screen capture: {w}");
                warnings.push(w);
                continue;
            };
            let config = match device.default_output_config() {
                Ok(c) => c,
                Err(e) => {
                    let w = format!("Selected desktop audio \"{name}\" could not be opened ({e}) — it will not be recorded");
                    log::warn!("screen capture: {w}");
                    warnings.push(w);
                    continue;
                }
            };
            let (tx, rx) = std::sync::mpsc::channel();
            match build_stream(&device, &config, tx) {
                Ok(stream) => {
                    streams.push(stream);
                    inputs.push(SourceInput {
                        name: format!("{name} (loopback)"),
                        rate: config.sample_rate(),
                        channels: config.channels(),
                        rx,
                    });
                }
                Err(e) => {
                    let w = format!("Selected desktop audio \"{name}\" could not be started ({e}) — it will not be recorded");
                    log::warn!("screen capture: {w}");
                    warnings.push(w);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let w = format!(
                "Desktop audio (loopback) is Windows-only — \"{name}\" will not be recorded"
            );
            log::warn!("screen capture: {w}");
            warnings.push(w);
        }
    }

    Ok(OpenSources {
        inputs,
        streams,
        warnings,
    })
}
```

- [ ] **Step 4: Run the tests and watch them pass**

```bash
cd src-tauri && cargo test -p vault_buddy_capture devices
```

Expected: the five new tests plus the three existing ones pass.

- [ ] **Step 5: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault_buddy_capture --all-targets -- -D warnings
cd src-tauri && cargo clippy -p vault_buddy_capture --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cd src-tauri && cargo test -p vault_buddy_capture
```

The Windows-target clippy matters here: the loopback arm is `cfg(windows)`
and would otherwise never be type-checked until CI.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/capture/src/devices.rs
cat > /tmp/msg.txt <<'EOF'
feat(capture): open N explicitly-selected audio endpoints

The screen-capture picker is a multi-select checklist of inputs and loopback
outputs (spec 7.2), and open_sources opens exactly one of each.

This is a sibling function rather than a parameter on open_sources, because
the two need OPPOSITE postures on a missing device. open_sources falls back
to the default endpoint, which is right for a stale saved config. These
names were ticked by the user seconds ago in front of the live device list,
so substituting a different microphone for the one they picked would record
the wrong audio silently -- worse than recording none. A missing pick is
skipped with a warning naming it, and the rest of the selection still
records.

An empty selection is Ok with zero sources, not an error: spec 6.5 keeps a
silent capture available because a silent UI demo is a real use case, and
that stays true when every pick turns out to be gone.

A regression test pins open_sources' fallback posture so a later tidy-up
cannot unify the two behaviours and quietly change what a meeting recording
does when its configured mic is unplugged.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 4: The fragmented-MP4 sink (`screen::sink`)

Spec §6.4 — **resolved**; this task turns the spike's proven wiring into the
real component. Read `src-tauri/screen/src/fmp4_spike.rs` first: it is a
working `MFCreateFMPEG4MediaSink` → `MFCreateSinkWriterFromMediaSink` →
`WriteSample` → `Finalize` reference, and its comments record two failures
you must not repeat.

The one structural addition over the spike: **an audio stream**.
`MFCreateFMPEG4MediaSink`'s third parameter is the audio media type. Passing
it at creation gives stream index **0 = video, 1 = audio**; those indices are
fixed by the order the sink declares them, not chosen by us.

**Files:**
- Create: `src-tauri/screen/src/sink.rs`
- Modify: `src-tauri/screen/src/lib.rs` (add `pub mod sink;`, new `ScreenError` variants)
- Modify: `src-tauri/screen/Cargo.toml` (make `windows` a real, non-optional Windows dependency)

**Interfaces:**
- Consumes: `crate::ScreenError`.
- Produces:
  - `sink::VideoFormat { width: u32, height: u32, fps: u32, bitrate_bps: u32 }`
  - `sink::AudioFormat { sample_rate: u32, channels: u16, bitrate_bps: u32 }`
  - `sink::FragmentedSink` with
    `create(path: &Path, video: VideoFormat, audio: Option<AudioFormat>) -> Result<FragmentedSink, ScreenError>`,
    `write_video(&mut self, nv12: &[u8], ts: Duration, duration: Duration) -> Result<(), ScreenError>`,
    `write_audio(&mut self, pcm: &[i16], ts: Duration, duration: Duration) -> Result<(), ScreenError>`,
    `finalize(self) -> Result<(), ScreenError>`.

  Task 6's mux thread owns exactly one `FragmentedSink` and is the only
  caller.

**Non-negotiables for this module:**

1. **No `Flush` call, anywhere.** `IMFSinkWriter::Flush` drops pending
   samples. Only `Finalize` drains. Two spike runs died on this.
2. **No file-size probing.** `std::fs::metadata().len()` is meaningless while
   Windows holds the handle open.
3. **`MF_MT_MAX_KEYFRAME_SPACING = fps`** on the H.264 output type —
   spec §12's fixed 1-second keyframe interval, which bounds the editor
   preview's seek latency and leaves a future no-re-encode fast-cut path open.
4. **`finalize` consumes `self`** so the type system forbids writing after
   finalizing.
5. **The non-Windows arm is a real compiling stub** returning
   `ScreenError::Unsupported`, so `rust-core` keeps building the crate on
   Linux.

- [ ] **Step 1: Widen `ScreenError` and add the tests it needs**

In `src-tauri/screen/src/lib.rs`, add two variants to `ScreenError` (the
existing variants and their tests stay exactly as they are):

```rust
    /// The capture could not be written — sink creation, a sample write, or
    /// finalize failed. Carries the OS message.
    Sink(String),
    /// A capture was requested while one is already running.
    AlreadyCapturing,
```

and extend the existing `fixed_variants_render_a_constant_message_with_no_caller_data`
test with the new fixed variant, plus a new test for the interpolating one:

```rust
        assert_eq!(
            ScreenError::AlreadyCapturing.to_string(),
            "a capture is already running"
        );
```

```rust
    // `Sink`, like `Io`, deliberately carries and renders OS-supplied text:
    // a sink failure the user can act on ("no H.264 encoder") is useless
    // without the message. Pinned separately so the fixed/interpolating
    // split above stays a deliberate distinction rather than an accident.
    #[test]
    fn sink_variant_interpolates_the_underlying_message() {
        assert!(ScreenError::Sink("MF_E_TOPO_CODEC_NOT_FOUND".into())
            .to_string()
            .contains("MF_E_TOPO_CODEC_NOT_FOUND"));
    }
```

and the matching `Display` arms:

```rust
            ScreenError::Sink(e) => write!(f, "screen capture could not be written: {e}"),
            ScreenError::AlreadyCapturing => write!(f, "a capture is already running"),
```

- [ ] **Step 2: Write the Linux-side stub test**

Create `src-tauri/screen/src/sink.rs` with ONLY the non-Windows arm and its
test, so there is something failing to drive:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // The Linux compile gate builds this crate. The stub must DEGRADE, not
    // panic and not silently succeed: a stub that returned Ok would make
    // every caller's happy path untestable proof of nothing.
    #[cfg(not(windows))]
    #[test]
    fn the_non_windows_sink_reports_unsupported_rather_than_panicking() {
        let r = FragmentedSink::create(
            std::path::Path::new("/tmp/vb-unused.mp4"),
            VideoFormat { width: 1920, height: 1080, fps: 30, bitrate_bps: 8_000_000 },
            None,
        );
        assert!(matches!(r, Err(crate::ScreenError::Unsupported)));
    }

    #[test]
    fn a_video_format_is_plain_data_both_platforms_agree_on() {
        // The format structs are shared by both arms, so a field added to
        // the Windows arm only would not compile here — which is the point.
        let v = VideoFormat { width: 1280, height: 720, fps: 60, bitrate_bps: 5_000_000 };
        assert_eq!(v.width * v.height, 921_600);
        let a = AudioFormat { sample_rate: 48_000, channels: 2, bitrate_bps: 128_000 };
        assert_eq!(a.channels, 2);
    }
}
```

- [ ] **Step 3: Run it and watch it fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen sink
```

Expected: compile error — `FragmentedSink`, `VideoFormat`, `AudioFormat` not
found.

- [ ] **Step 4: Write the shared types and the non-Windows arm**

Prepend to `src-tauri/screen/src/sink.rs`:

```rust
//! The staged capture's container: a FRAGMENTED MP4 written through Media
//! Foundation's `IMFSinkWriter`.
//!
//! Why fragmented rather than a standard MP4 (spec 6.4, RESOLVED): a
//! standard MP4 writes its `moov` index at finalize, so a capture that
//! crashes mid-recording is unopenable — every byte of video present, no
//! index. A fragmented MP4 is self-describing per fragment and degrades to
//! a playable prefix. Measured on a windows-latest runner, killing the
//! process with abort() after 300 frames: the fragmented file decoded back
//! 280 of 300 frames; the standard-MP4 control, holding a comparable
//! amount of data, decoded 0. That control is what makes the result mean
//! anything.
//!
//! The measurement covers a PROCESS CRASH, not power loss. Power loss only
//! shortens the recoverable prefix; it cannot make a fragmented file behave
//! like an unindexed one.
//!
//! TWO THINGS THIS MODULE MUST NEVER DO, both learned the expensive way:
//!
//! 1. **Never call `IMFSinkWriter::Flush`.** It is documented to "drop all
//!    pending samples" — a seek-style discard, not a drain. Two spike runs
//!    called it before teardown and destroyed the footage they were
//!    measuring. Only `Finalize` drains.
//! 2. **Never infer anything from the file's size during capture.** Windows
//!    updates a directory entry's size lazily while a handle is open;
//!    `std::fs::metadata().len()` reported 0 bytes across a capture that
//!    had written 59 KB. A retracted reading of that probe once invented a
//!    "flush every fragment" requirement that does not exist.

use std::path::Path;
use std::time::Duration;

/// H.264 output parameters. `bitrate_bps` comes from
/// `vault_buddy_core::screen_capture_config::bitrate_bps` — do not
/// re-derive it here.
#[derive(Debug, Clone, Copy)]
pub struct VideoFormat {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
}

/// AAC output parameters. Input is always 16-bit interleaved PCM.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u16,
    pub bitrate_bps: u32,
}

/// Media Foundation's time base: 100-nanosecond units.
#[allow(dead_code)] // used only by the Windows arm
const HNS_PER_SEC: i64 = 10_000_000;

#[allow(dead_code)]
fn to_hns(d: Duration) -> i64 {
    // saturating: a Duration beyond ~29 000 years cannot be represented, and
    // a panic in the capture write path is never the right answer.
    i64::try_from(d.as_nanos() / 100).unwrap_or(i64::MAX)
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use crate::ScreenError;

    /// Non-Windows stub. It must DEGRADE rather than panic: the Linux
    /// compile gate builds this crate, and an abort here would kill CI
    /// instead of reporting.
    pub struct FragmentedSink;

    impl FragmentedSink {
        pub fn create(
            _path: &Path,
            _video: VideoFormat,
            _audio: Option<AudioFormat>,
        ) -> Result<FragmentedSink, ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn write_video(
            &mut self,
            _nv12: &[u8],
            _ts: Duration,
            _duration: Duration,
        ) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn write_audio(
            &mut self,
            _pcm: &[i16],
            _ts: Duration,
            _duration: Duration,
        ) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn finalize(self) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }
    }
}

pub use imp::FragmentedSink;
```

- [ ] **Step 5: Verify the Linux arm passes, then write the Windows arm**

```bash
cd src-tauri && cargo test -p vault_buddy_screen sink
```

Expected: 2 passed.

Now make `windows` a real dependency. In `src-tauri/screen/Cargo.toml`,
replace the optional spike-only entry with:

```toml
[target.'cfg(windows)'.dependencies]
# 0.62 is already in the workspace lockfile transitively via tauri, so this
# pins no new version. windows-capture 2.0.1 requires ^0.62.2, the same one.
windows = { version = "0.62", features = [
    "Win32_Foundation",
    "Win32_Media_MediaFoundation",
    "Win32_System_Com",
] }
```

and keep the `fmp4-spike` feature working by changing it to a plain marker:

```toml
[features]
default = []
# Spec 6.4's gating spike, RESOLVED 2026-09-19 and kept re-runnable. It no
# longer pulls a dependency in (the `windows` crate is now a real Windows
# dependency of this crate), so this is just a switch on the module.
fmp4-spike = []
```

Add the Windows arm to `src-tauri/screen/src/sink.rs`, after the
`cfg(not(windows))` module:

```rust
#[cfg(windows)]
mod imp {
    use super::*;
    use crate::ScreenError;
    use windows::core::{Result as WinResult, HSTRING};
    use windows::Win32::Media::MediaFoundation::*;

    /// Stream indices are fixed by the ORDER the sink declares its streams:
    /// `MFCreateFMPEG4MediaSink(stream, video_type, audio_type)` gives
    /// video 0 and audio 1. They are not ours to choose.
    const VIDEO_STREAM: u32 = 0;
    const AUDIO_STREAM: u32 = 1;

    fn sink_err(context: &str, e: windows::core::Error) -> ScreenError {
        log::error!("screen sink: {context}: {e}");
        ScreenError::Sink(format!("{context}: {e}"))
    }

    /// Media Foundation must be started per process and shut down once. The
    /// sink owns one of these for its lifetime.
    struct MfRuntime;

    impl MfRuntime {
        fn start() -> WinResult<Self> {
            unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL)? };
            Ok(MfRuntime)
        }
    }

    impl Drop for MfRuntime {
        fn drop(&mut self) {
            unsafe {
                let _ = MFShutdown();
            }
        }
    }

    fn video_output_type(v: VideoFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            t.SetUINT32(&MF_MT_AVG_BITRATE, v.bitrate_bps)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            // Spec 12: a fixed 1-second keyframe interval regardless of
            // preset. It costs little, bounds the editor preview's seek
            // latency, and leaves a future no-re-encode fast-cut path
            // possible without changing the capture format.
            t.SetUINT32(&MF_MT_MAX_KEYFRAME_SPACING, v.fps)?;
            // MF packs these as one UINT64: high 32 bits then low 32.
            t.SetUINT64(&MF_MT_FRAME_SIZE, ((v.width as u64) << 32) | v.height as u64)?;
            t.SetUINT64(&MF_MT_FRAME_RATE, ((v.fps as u64) << 32) | 1u64)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1u64)?;
            Ok(t)
        }
    }

    fn video_input_type(v: VideoFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            // NV12 is the format every hardware H.264 encoder MFT accepts;
            // handing the encoder BGRA would work only where a software
            // colour converter happens to be inserted.
            t.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
            t.SetUINT64(&MF_MT_FRAME_SIZE, ((v.width as u64) << 32) | v.height as u64)?;
            t.SetUINT64(&MF_MT_FRAME_RATE, ((v.fps as u64) << 32) | 1u64)?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1u64)?;
            Ok(t)
        }
    }

    fn audio_output_type(a: AudioFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?;
            t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, a.sample_rate)?;
            t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, a.channels as u32)?;
            // The AAC encoder MFT takes BYTES per second, not bits.
            t.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, a.bitrate_bps / 8)?;
            Ok(t)
        }
    }

    fn audio_input_type(a: AudioFormat) -> WinResult<IMFMediaType> {
        unsafe {
            let t = MFCreateMediaType()?;
            t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
            t.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
            t.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
            t.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, a.sample_rate)?;
            t.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, a.channels as u32)?;
            let block_align = a.channels as u32 * 2; // 16-bit samples
            t.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block_align)?;
            t.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, a.sample_rate * block_align)?;
            t.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
            Ok(t)
        }
    }

    pub struct FragmentedSink {
        writer: IMFSinkWriter,
        has_audio: bool,
        /// Dropped last, after the writer: MFShutdown must not run while a
        /// Media Foundation object is still alive.
        _mf: MfRuntime,
    }

    impl FragmentedSink {
        pub fn create(
            path: &Path,
            video: VideoFormat,
            audio: Option<AudioFormat>,
        ) -> Result<FragmentedSink, ScreenError> {
            unsafe {
                let mf = MfRuntime::start().map_err(|e| sink_err("MFStartup", e))?;

                let byte_stream = MFCreateFile(
                    MF_ACCESSMODE_WRITE,
                    MF_OPENMODE_DELETE_IF_EXIST,
                    MF_FILEFLAGS_NONE,
                    &HSTRING::from(path.to_string_lossy().as_ref()),
                )
                .map_err(|e| sink_err("create the capture file", e))?;

                let video_out =
                    video_output_type(video).map_err(|e| sink_err("build the H.264 type", e))?;
                let audio_out = match audio {
                    Some(a) => {
                        Some(audio_output_type(a).map_err(|e| sink_err("build the AAC type", e))?)
                    }
                    None => None,
                };

                // FRAGMENTED, not MFCreateMPEG4MediaSink. This one line is
                // the difference between a crashed capture that plays as a
                // prefix and one that cannot be opened at all (spec 6.4).
                let sink: IMFMediaSink =
                    MFCreateFMPEG4MediaSink(&byte_stream, &video_out, audio_out.as_ref())
                        .map_err(|e| sink_err("create the fragmented MP4 sink", e))?;

                let writer = MFCreateSinkWriterFromMediaSink(&sink, None)
                    .map_err(|e| sink_err("create the sink writer", e))?;

                writer
                    .SetInputMediaType(
                        VIDEO_STREAM,
                        &video_input_type(video).map_err(|e| sink_err("build the NV12 type", e))?,
                        None,
                    )
                    // The one failure a user can act on: no usable H.264
                    // encoder on this machine.
                    .map_err(|_| ScreenError::EncoderUnavailable)?;

                if let Some(a) = audio {
                    writer
                        .SetInputMediaType(
                            AUDIO_STREAM,
                            &audio_input_type(a)
                                .map_err(|e| sink_err("build the PCM type", e))?,
                            None,
                        )
                        .map_err(|e| sink_err("configure the audio stream", e))?;
                }

                writer
                    .BeginWriting()
                    .map_err(|e| sink_err("begin writing", e))?;

                Ok(FragmentedSink {
                    writer,
                    has_audio: audio.is_some(),
                    _mf: mf,
                })
            }
        }

        pub fn write_video(
            &mut self,
            nv12: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            self.write(VIDEO_STREAM, nv12, ts, duration)
        }

        pub fn write_audio(
            &mut self,
            pcm: &[i16],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            if !self.has_audio {
                // Not an error: a capture with no audio devices selected is
                // legal (spec 6.5), and the mux thread should not have to
                // branch on it.
                return Ok(());
            }
            let bytes: &[u8] = unsafe {
                std::slice::from_raw_parts(pcm.as_ptr() as *const u8, std::mem::size_of_val(pcm))
            };
            self.write(AUDIO_STREAM, bytes, ts, duration)
        }

        fn write(
            &mut self,
            stream: u32,
            bytes: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            unsafe {
                let len = u32::try_from(bytes.len())
                    .map_err(|_| ScreenError::Sink("sample larger than 4 GiB".into()))?;
                let buffer =
                    MFCreateMemoryBuffer(len).map_err(|e| sink_err("allocate a sample", e))?;
                let mut dst: *mut u8 = std::ptr::null_mut();
                buffer
                    .Lock(&mut dst, None, None)
                    .map_err(|e| sink_err("lock a sample", e))?;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                buffer.Unlock().map_err(|e| sink_err("unlock a sample", e))?;
                buffer
                    .SetCurrentLength(len)
                    .map_err(|e| sink_err("size a sample", e))?;

                let sample = MFCreateSample().map_err(|e| sink_err("create a sample", e))?;
                sample
                    .AddBuffer(&buffer)
                    .map_err(|e| sink_err("attach a sample buffer", e))?;
                sample
                    .SetSampleTime(to_hns(ts))
                    .map_err(|e| sink_err("stamp a sample", e))?;
                sample
                    .SetSampleDuration(to_hns(duration))
                    .map_err(|e| sink_err("set a sample duration", e))?;

                self.writer
                    .WriteSample(stream, &sample)
                    .map_err(|e| sink_err("write a sample", e))
            }
        }

        /// Drain and close. Consumes `self`, so writing after finalizing is
        /// a compile error rather than a runtime surprise.
        ///
        /// There is NO `Flush` here and there must never be one: MF's
        /// `Flush` drops pending samples instead of draining them. Finalize
        /// is the only drain.
        pub fn finalize(self) -> Result<(), ScreenError> {
            unsafe {
                self.writer
                    .Finalize()
                    .map_err(|e| sink_err("finalize the capture", e))
            }
        }
    }
}
```

Register the module in `src-tauri/screen/src/lib.rs`:

```rust
pub mod sink;
```

- [ ] **Step 6: Type-check the Windows arm from Linux**

```bash
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

Expected: clean. This is the single highest-value check in the task — it
type-checks every `windows`-crate signature without a 10-minute CI round
trip. Fix everything it reports before moving on.

- [ ] **Step 7: Add the structural regression test**

Append to `src-tauri/screen/src/sink.rs`'s test module:

```rust
    // Regression, spec 6.4's second caveat: IMFSinkWriter::Flush DROPS
    // pending samples rather than draining them. Two spike runs called it
    // before teardown and destroyed the footage they were measuring, then
    // reported the absence as a result. A future "tidy up the teardown"
    // edit that reintroduces it would silently empty every capture, with no
    // test failing and no error logged. This scan is the alarm.
    #[test]
    fn the_sink_never_calls_flush() {
        let src = include_str!("sink.rs");
        assert!(
            !src.contains(".Flush("),
            "sink.rs must never call IMFSinkWriter::Flush - it discards \
             pending samples. Only Finalize drains."
        );
    }

    // Regression, spec 6.4's first caveat: a per-frame std::fs::metadata
    // probe reported 0 bytes across a capture that had written 59 KB,
    // because Windows updates a directory entry's size lazily while a
    // handle is open. Reading it literally once produced an invented
    // "flush every fragment" requirement.
    #[test]
    fn the_sink_never_probes_the_file_size_while_writing() {
        let src = include_str!("sink.rs");
        assert!(
            !src.contains("fs::metadata"),
            "sink.rs must not probe file size during capture - it measures \
             a stale directory entry, not the sink."
        );
    }
```

```bash
cd src-tauri && cargo test -p vault_buddy_screen sink
```

Expected: 4 passed.

- [ ] **Step 8: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cd src-tauri && cargo test -p vault_buddy_screen
cd src-tauri && cargo machete .
cd src-tauri && cargo deny check
```

`cargo deny check` runs here because this is the commit that makes `windows`
a real dependency of the crate.

- [ ] **Step 9: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/screen/src/sink.rs src-tauri/screen/src/lib.rs src-tauri/screen/Cargo.toml
cat > /tmp/msg.txt <<'EOF'
feat(screen): add the fragmented-MP4 Media Foundation sink

Turns spec 6.4's resolved spike into the real component. Fragmented rather
than standard MP4 because a standard MP4 writes its moov index at finalize:
a crashed capture holds every byte of video and cannot be opened. Measured
on windows-latest, killing the process with abort() after 300 frames, the
fragmented file decoded 280 frames back while the standard-MP4 control --
holding a comparable amount of data -- decoded none.

Adds an audio stream the spike did not have. MFCreateFMPEG4MediaSink takes
the audio type as its third argument, which fixes the stream indices at
video 0 and audio 1; they are declared by the sink, not chosen by us. Input
is NV12 video and 16-bit PCM audio, output H.264 and AAC, with
MF_MT_MAX_KEYFRAME_SPACING pinned to the frame rate for spec 12's fixed
one-second keyframe interval.

finalize consumes self, so writing after finalizing is a compile error.

Two structural tests guard the failures that already cost this work real
time: sink.rs must never call IMFSinkWriter::Flush, which discards pending
samples instead of draining them and emptied two spike runs, and must never
probe the file size mid-capture, which reports a stale directory entry and
once produced an invented per-fragment flush requirement.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 5: Source enumeration (`screen::source`)

Spec §7.2's Screen and Window tabs, plus §14's *"source vanished before
start → typed `sourceGone`; inline error + refreshed list. Never a
started-then-dead capture."*

The `windows-capture` 2.0.1 API this builds on, **verified against the
crate's source**, not assumed:

- `windows_capture::monitor::Monitor` — `enumerate() -> Result<Vec<Monitor>, Error>`,
  `index()`, `name()`, `device_name()`, `width()`, `height()`,
  `as_raw_hmonitor() -> *mut c_void`, `from_raw_hmonitor(*mut c_void)`.
- `windows_capture::window::Window` — `enumerate() -> Result<Vec<Window>, Error>`,
  `title()`, `process_name()`, `process_id()`, `width() -> Result<i32,_>`,
  `height() -> Result<i32,_>`, `is_valid() -> bool`,
  `as_raw_hwnd() -> *mut c_void`, `from_raw_hwnd(*mut c_void)`.

**The id is a string, and its encoding is pure.** A `*mut c_void` cannot
cross the IPC boundary, and a bare index is not stable across a
re-enumeration. So a source is addressed by a string — `screen:<index>` or
`window:<hwnd as isize>` — whose encode/parse pair is pure, lives on both
platforms, and is therefore **unit-tested on Linux**. That is the same
reason `core::screen_geometry` exists: push everything checkable off the
Windows-only island.

**Files:**
- Create: `src-tauri/screen/src/source.rs`
- Modify: `src-tauri/screen/src/lib.rs` (add `pub mod source;`)
- Modify: `src-tauri/screen/Cargo.toml` (add `windows-capture` for Windows)

**Interfaces:**
- Consumes: `crate::ScreenError`.
- Produces:
  - `source::SourceKind { Screen, Window }` (`Serialize`, lowercase)
  - `source::SourceId { Screen(usize), Window(isize) }` with
    `to_string(&self) -> String` and `parse(s: &str) -> Option<SourceId>` — **pure**
  - `source::CaptureSourceInfo { id: String, kind: SourceKind, title: String, detail: String, width: u32, height: u32, is_primary: bool }` (`Serialize`, camelCase)
  - `source::list_sources(exclude_titles: &[String]) -> Vec<CaptureSourceInfo>`
  - `source::resolve(id: &SourceId) -> Result<ResolvedSource, ScreenError>` returning
    `ResolvedSource { handle: SourceHandle, width: u32, height: u32, title: String }`,
    where `SourceHandle` is the `cfg(windows)` `Monitor`/`Window` union and a
    unit struct elsewhere.

  Task 6 takes a `ResolvedSource`; Task 7 calls `list_sources` and `resolve`.

**Two behaviours that are easy to get wrong:**

- `list_sources` **degrades to an empty list**, never an error — the
  discovery precedent. A failed enumeration shows "No sources found" with a
  Refresh, not a red banner the user cannot act on.
- `resolve` is where `sourceGone` comes from. It re-checks `is_valid()` for a
  window and re-enumerates for a monitor, **at start time**, because the list
  the user clicked may be seconds stale.

- [ ] **Step 1: Write the failing tests (all pure, all run on Linux)**

Create `src-tauri/screen/src/source.rs` with ONLY the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_screen_id_round_trips() {
        let id = SourceId::Screen(2);
        assert_eq!(id.to_string(), "screen:2");
        assert_eq!(SourceId::parse("screen:2"), Some(SourceId::Screen(2)));
    }

    #[test]
    fn a_window_id_round_trips_including_a_negative_handle() {
        // An HWND is a pointer widened to isize. On a 64-bit process the
        // high bit can be set, so the decimal form can be negative; parsing
        // it as unsigned would silently address a different window.
        let id = SourceId::Window(-1_234_567);
        assert_eq!(id.to_string(), "window:-1234567");
        assert_eq!(SourceId::parse("window:-1234567"), Some(SourceId::Window(-1_234_567)));
    }

    #[test]
    fn parse_rejects_anything_it_does_not_fully_understand() {
        // The id crosses the IPC boundary, so it is untrusted input. A
        // lenient parse that fell back to a default would start a capture of
        // something the user did not pick.
        assert_eq!(SourceId::parse("screen"), None);
        assert_eq!(SourceId::parse("screen:"), None);
        assert_eq!(SourceId::parse("screen:abc"), None);
        assert_eq!(SourceId::parse("screen:-1"), None, "a monitor index is never negative");
        assert_eq!(SourceId::parse("region:0"), None, "region arrives in phase 3");
        assert_eq!(SourceId::parse(""), None);
        assert_eq!(SourceId::parse("window:1:2"), None);
    }

    #[test]
    fn the_kind_serializes_lowercase_for_the_webview_and_the_sidecar() {
        assert_eq!(serde_json::to_string(&SourceKind::Screen).unwrap(), "\"screen\"");
        assert_eq!(serde_json::to_string(&SourceKind::Window).unwrap(), "\"window\"");
    }

    #[test]
    fn source_info_serializes_camel_case() {
        let info = CaptureSourceInfo {
            id: "screen:0".into(),
            kind: SourceKind::Screen,
            title: "Screen 1".into(),
            detail: "2560x1440 - Primary".into(),
            width: 2560,
            height: 1440,
            is_primary: true,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"isPrimary\":true"), "got {json}");
        assert!(!json.contains("is_primary"), "got {json}");
    }

    #[cfg(not(windows))]
    #[test]
    fn listing_sources_off_windows_degrades_to_empty_rather_than_erroring() {
        // The discovery precedent: an unavailable source list shows an empty
        // state with a Refresh, never a banner the user cannot act on. This
        // also keeps the Linux compile gate honest about what it proves.
        assert!(list_sources(&[]).is_empty());
    }

    #[cfg(not(windows))]
    #[test]
    fn resolving_off_windows_reports_unsupported() {
        assert!(matches!(
            resolve(&SourceId::Screen(0)),
            Err(crate::ScreenError::Unsupported)
        ));
    }

    #[cfg(windows)]
    #[test]
    fn listing_sources_on_windows_never_panics_and_labels_everything() {
        // A CI runner's session may have no capturable windows at all, so
        // this asserts shape, not contents: whatever comes back is fully
        // labelled and carries a parseable id.
        for info in list_sources(&[]) {
            assert!(!info.title.is_empty(), "every source is named");
            assert!(SourceId::parse(&info.id).is_some(), "id parses: {}", info.id);
        }
    }

    #[cfg(windows)]
    #[test]
    fn our_own_windows_are_filtered_out_by_title() {
        // Spec 7.2: Vault Buddy's own windows must not appear in the picker.
        // (Phase 3 additionally hides them from the RECORDING itself via
        // WDA_EXCLUDEFROMCAPTURE; this is only the picker.)
        let all = list_sources(&[]);
        if let Some(first_window) = all.iter().find(|s| s.kind == SourceKind::Window) {
            let filtered = list_sources(&[first_window.title.clone()]);
            assert!(
                !filtered.iter().any(|s| s.id == first_window.id),
                "an excluded title must not come back"
            );
        }
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen source
```

Expected: compile error — nothing in `source` exists yet.

- [ ] **Step 3: Write the pure half plus both platform arms**

Prepend to `src-tauri/screen/src/source.rs`:

```rust
//! What can be captured: monitors and top-level windows.
//!
//! The id encoding is PURE and lives on both platforms deliberately. A
//! `*mut c_void` cannot cross the IPC boundary and a bare list index is not
//! stable across a re-enumeration, so a source is addressed by a string the
//! webview holds and hands back. That string is untrusted input by the time
//! it returns, which is why `SourceId::parse` is strict: a lenient parse
//! that fell back to a default would start a capture of something the user
//! did not pick.
//!
//! Enumeration DEGRADES to an empty list rather than erroring, following
//! `discovery`'s precedent — an empty state with a Refresh is actionable, a
//! red banner about a WinRT error is not.

use serde::Serialize;

use crate::ScreenError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Screen,
    Window,
}

/// A capture target, in the form that crosses the IPC boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceId {
    /// Monitor index as `windows_capture::monitor::Monitor::index` reports it.
    Screen(usize),
    /// `HWND` widened to `isize`. Signed on purpose: an HWND is a pointer,
    /// and on a 64-bit process its high bit can be set.
    Window(isize),
}

impl SourceId {
    pub fn parse(s: &str) -> Option<SourceId> {
        let (kind, rest) = s.split_once(':')?;
        if rest.is_empty() || rest.contains(':') {
            return None;
        }
        match kind {
            "screen" => rest.parse::<usize>().ok().map(SourceId::Screen),
            "window" => rest.parse::<isize>().ok().map(SourceId::Window),
            // "region" deliberately absent: region capture is phase 3, and
            // accepting an id we cannot honour would start the wrong capture.
            _ => None,
        }
    }

    pub fn kind(&self) -> SourceKind {
        match self {
            SourceId::Screen(_) => SourceKind::Screen,
            SourceId::Window(_) => SourceKind::Window,
        }
    }
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceId::Screen(i) => write!(f, "screen:{i}"),
            SourceId::Window(h) => write!(f, "window:{h}"),
        }
    }
}

/// One row in the picker.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSourceInfo {
    pub id: String,
    pub kind: SourceKind,
    /// What the user reads first: a monitor label or a window title.
    pub title: String,
    /// The secondary line: resolution and primary flag, or the owning
    /// process.
    pub detail: String,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

/// A source re-checked at START time and ready to capture.
pub struct ResolvedSource {
    pub handle: SourceHandle,
    pub width: u32,
    pub height: u32,
    pub title: String,
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    /// Off Windows there is nothing to hold.
    pub enum SourceHandle {}

    pub fn list_sources(_exclude_titles: &[String]) -> Vec<CaptureSourceInfo> {
        log::debug!("screen source: enumeration is Windows-only; returning an empty list");
        Vec::new()
    }

    pub fn resolve(_id: &SourceId) -> Result<ResolvedSource, ScreenError> {
        Err(ScreenError::Unsupported)
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use windows_capture::monitor::Monitor;
    use windows_capture::window::Window;

    pub enum SourceHandle {
        Screen(Monitor),
        Window(Window),
    }

    pub fn list_sources(exclude_titles: &[String]) -> Vec<CaptureSourceInfo> {
        let mut out = Vec::new();

        match Monitor::enumerate() {
            Ok(monitors) => {
                for m in monitors {
                    // Every accessor is fallible (a monitor can be unplugged
                    // between enumerate and read). One unreadable monitor
                    // must not empty the whole list.
                    let (Ok(index), Ok(width), Ok(height)) = (m.index(), m.width(), m.height())
                    else {
                        log::warn!("screen source: skipping an unreadable monitor");
                        continue;
                    };
                    let is_primary = index == 1; // Monitor::index is 1-based
                    let title = m.name().unwrap_or_else(|_| format!("Screen {index}"));
                    let detail = if is_primary {
                        format!("{width}x{height} - Primary")
                    } else {
                        format!("{width}x{height}")
                    };
                    out.push(CaptureSourceInfo {
                        id: SourceId::Screen(index).to_string(),
                        kind: SourceKind::Screen,
                        title,
                        detail,
                        width,
                        height,
                        is_primary,
                    });
                }
            }
            Err(e) => log::warn!("screen source: monitor enumeration failed: {e}"),
        }

        match Window::enumerate() {
            Ok(windows) => {
                for w in windows {
                    let Ok(title) = w.title() else { continue };
                    if title.trim().is_empty() {
                        continue;
                    }
                    // Spec 7.2: Vault Buddy's own windows are filtered out.
                    if exclude_titles.iter().any(|t| t == &title) {
                        continue;
                    }
                    let (Ok(width), Ok(height)) = (w.width(), w.height()) else {
                        continue;
                    };
                    // A zero-sized or negative-sized window cannot be
                    // captured; offering it would produce a start that fails
                    // for a reason the user cannot see.
                    let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height))
                    else {
                        continue;
                    };
                    if width == 0 || height == 0 {
                        continue;
                    }
                    let detail = w.process_name().unwrap_or_default();
                    out.push(CaptureSourceInfo {
                        id: SourceId::Window(w.as_raw_hwnd() as isize).to_string(),
                        kind: SourceKind::Window,
                        title,
                        detail,
                        width,
                        height,
                        is_primary: false,
                    });
                }
            }
            Err(e) => log::warn!("screen source: window enumeration failed: {e}"),
        }

        out
    }

    /// Re-check the picked source AT START TIME (spec 14). The list the user
    /// clicked may be seconds stale: a window can have closed, a monitor can
    /// have been unplugged. A started-then-dead capture is the outcome this
    /// exists to prevent.
    pub fn resolve(id: &SourceId) -> Result<ResolvedSource, ScreenError> {
        match *id {
            SourceId::Screen(index) => {
                let monitor = Monitor::from_index(index).map_err(|e| {
                    log::warn!("screen source: monitor {index} is gone: {e}");
                    ScreenError::SourceGone
                })?;
                let (Ok(width), Ok(height)) = (monitor.width(), monitor.height()) else {
                    return Err(ScreenError::SourceGone);
                };
                let title = monitor.name().unwrap_or_else(|_| format!("Screen {index}"));
                Ok(ResolvedSource {
                    handle: SourceHandle::Screen(monitor),
                    width,
                    height,
                    title,
                })
            }
            SourceId::Window(hwnd) => {
                let window = Window::from_raw_hwnd(hwnd as *mut std::ffi::c_void);
                if !window.is_valid() {
                    log::warn!("screen source: window {hwnd} is gone");
                    return Err(ScreenError::SourceGone);
                }
                let (Ok(width), Ok(height)) = (window.width(), window.height()) else {
                    return Err(ScreenError::SourceGone);
                };
                let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
                    return Err(ScreenError::SourceGone);
                };
                if width == 0 || height == 0 {
                    return Err(ScreenError::SourceGone);
                }
                let title = window.title().unwrap_or_else(|_| "Window".to_string());
                Ok(ResolvedSource {
                    handle: SourceHandle::Window(window),
                    width,
                    height,
                    title,
                })
            }
        }
    }
}

pub use imp::{list_sources, resolve, SourceHandle};
```

Add the dependency in `src-tauri/screen/Cargo.toml` under
`[target.'cfg(windows)'.dependencies]`:

```toml
# MIT, crates.io (verified against the registry: 2.0.1, not yanked), and it
# requires `windows` ^0.62.2 -- the exact version already in the workspace
# lockfile, so it pins nothing new. deny.toml's licence allow-list already
# permits MIT and `[sources] unknown-git = "deny"` is satisfied, so no
# deny.toml entry is needed.
windows-capture = "2.0.1"
```

Register the module in `src-tauri/screen/src/lib.rs`:

```rust
pub mod source;
```

- [ ] **Step 4: Run the tests and type-check the Windows arm**

```bash
cd src-tauri && cargo test -p vault_buddy_screen source
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

Expected: 7 Linux tests pass; the Windows-target clippy is clean. **If the
Windows clippy reports a signature mismatch against `windows-capture`, trust
it over this plan and fix the call site** — the API was read from the 2.0.1
source, but a patch release may differ.

- [ ] **Step 5: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings
cd src-tauri && cargo test -p vault_buddy_screen
cd src-tauri && cargo machete .
cd src-tauri && cargo deny check
```

`cargo deny check` runs here because this commit adds `windows-capture` and
its transitive tree (`parking_lot`, `rayon`, `thiserror`, `windows-future`).
**If `deny` reports a licence outside the allow-list, stop and report it
rather than widening `deny.toml`** — the spec's dependency decision (§3)
rests on this tree being permissively licensed, and widening the allow-list
is a reviewed decision, not a build fix.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/screen/src/source.rs src-tauri/screen/src/lib.rs src-tauri/screen/Cargo.toml
cat > /tmp/msg.txt <<'EOF'
feat(screen): enumerate monitors and windows as capture sources

Adds windows-capture 2.0.1 (MIT, crates.io, requires the windows crate
0.62.2 already in the lockfile, so it pins no new version and needs no
deny.toml entry).

A source is addressed by a string id rather than a handle: a raw HWND
cannot cross the IPC boundary and a list index is not stable across a
re-enumeration. That encoding is pure and compiles on both platforms, so
its round-trip is unit-tested on Linux -- the same reason the geometry and
timeline modules live in core. The window arm is signed because an HWND is
a pointer whose high bit can be set on a 64-bit process; parsing it
unsigned would address a different window. Parsing is strict, because by
the time an id comes back it is untrusted input: a lenient parse falling
back to a default would start a capture of something the user did not pick.
"region:" is deliberately rejected -- region capture is phase 3, and
accepting an id we cannot honour would capture the wrong thing.

Enumeration degrades to an empty list rather than erroring, following
discovery's precedent, and skips a monitor or window whose accessors fail
rather than emptying the list for one bad entry.

resolve() re-checks the picked source at START time, because the list the
user clicked may be seconds stale. That is where the typed sourceGone comes
from, and it is what prevents a started-then-dead capture.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 6: BGRA → NV12 conversion (`screen::convert`)

`windows-capture` delivers frames as 8-bit BGRA. Every hardware H.264
encoder MFT wants NV12. Handing the encoder BGRA works only where Windows
happens to insert a software colour converter — which is exactly the kind of
"works on my machine" the dropped-frame counter would report and nobody
could explain.

The conversion is **pure arithmetic over two byte slices**, so it lives in
its own module and is fully unit-tested on Linux. This is the largest
per-frame cost in the capture path, so it is also the one place where
getting the loop shape right matters.

**Files:**
- Create: `src-tauri/screen/src/convert.rs`
- Modify: `src-tauri/screen/src/lib.rs` (add `pub mod convert;`)
- Modify: `src-tauri/screen/src/sink.rs` (pin the colour matrix to match)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `convert::nv12_len(width: u32, height: u32) -> usize`
  - `convert::even_dims(width: u32, height: u32) -> (u32, u32)`
  - `convert::bgra_to_nv12(bgra: &[u8], stride: usize, width: u32, height: u32, out: &mut Vec<u8>) -> Result<(), ConvertError>`
  - `convert::ConvertError { ShortInput { needed: usize, got: usize }, OddDimensions }`

  Task 7's frame thread calls `even_dims` once at start and `bgra_to_nv12`
  per frame into a reused buffer.

**Decisions this module locks in, each with its reason:**

- **BT.709, limited range (16–235 luma).** Screen captures are HD or larger,
  and BT.709 is what every HD decoder assumes. Using BT.601 coefficients
  while the decoder assumes BT.709 is the classic "greens are slightly off"
  bug that nobody files and everybody sees. `sink.rs`'s NV12 input type is
  updated in this task to declare the same matrix, so the encoder is told
  what the converter actually produced rather than guessing.
- **Odd dimensions are rejected, not silently rounded, inside the converter**
  — NV12 subsamples chroma 2×2, so an odd width has half a chroma column.
  `even_dims` is the caller's explicit round-down, applied once at capture
  start, so a window of odd width loses one pixel column deliberately rather
  than producing a sheared frame.
- **The output buffer is reused, not allocated per frame.** At 60 fps and
  4K, allocating an NV12 frame per tick is ~12 MB/s of pure allocator churn.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/screen/src/convert.rs` with ONLY the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `width` x `height` BGRA buffer with `stride` bytes per row,
    /// every pixel the same colour, and recognisable padding after each row.
    fn solid(width: u32, height: u32, stride: usize, b: u8, g: u8, r: u8) -> Vec<u8> {
        let mut buf = vec![0xAAu8; stride * height as usize];
        for row in 0..height as usize {
            for col in 0..width as usize {
                let i = row * stride + col * 4;
                buf[i] = b;
                buf[i + 1] = g;
                buf[i + 2] = r;
                buf[i + 3] = 255;
            }
        }
        buf
    }

    #[test]
    fn nv12_is_one_luma_plane_plus_a_half_size_chroma_plane() {
        // 1.5 bytes per pixel. A wrong size here does not fail loudly -- the
        // encoder reads past the luma plane into whatever follows.
        assert_eq!(nv12_len(1920, 1080), 1920 * 1080 * 3 / 2);
        assert_eq!(nv12_len(2, 2), 6);
    }

    #[test]
    fn even_dims_rounds_down_never_up() {
        // Rounding UP would index past the end of the source frame. Down
        // loses at most one row and one column, deliberately.
        assert_eq!(even_dims(1921, 1081), (1920, 1080));
        assert_eq!(even_dims(1920, 1080), (1920, 1080));
        assert_eq!(even_dims(1, 1), (0, 0));
    }

    #[test]
    fn black_converts_to_the_limited_range_floor() {
        // BT.709 LIMITED range: black is luma 16, not 0, and neutral chroma
        // is 128. Emitting 0 here would make every capture look crushed on a
        // player that correctly expands 16-235.
        let src = solid(2, 2, 8, 0, 0, 0);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[16, 16, 16, 16], "luma floor");
        assert_eq!(&out[4..6], &[128, 128], "neutral chroma");
    }

    #[test]
    fn white_converts_to_the_limited_range_ceiling() {
        let src = solid(2, 2, 8, 255, 255, 255);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[235, 235, 235, 235], "luma ceiling");
        assert_eq!(&out[4..6], &[128, 128], "white is achromatic");
    }

    #[test]
    fn pure_red_lands_on_the_bt709_coefficients_not_the_bt601_ones() {
        // Regression naming the failure mode: with BT.601 coefficients pure
        // red gives luma ~81; BT.709 gives ~63. A decoder assuming 709 while
        // we encoded 601 is the "greens are slightly off" bug nobody files
        // and everybody sees. Pinning one known value catches a silent swap.
        let src = solid(2, 2, 8, 0, 0, 255);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(out[0], 63, "BT.709 limited-range luma for pure red");
    }

    #[test]
    fn row_padding_between_rows_is_skipped() {
        // windows-capture hands back a D3D11 staging texture whose row pitch
        // is usually WIDER than width*4. Reading it as tightly packed
        // shears the image progressively down the frame -- a bug that looks
        // like a capture glitch rather than a stride bug.
        let stride = 64; // far wider than 2 px * 4 bytes
        let src = solid(2, 2, stride, 255, 255, 255);
        let mut out = Vec::new();
        bgra_to_nv12(&src, stride, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[235, 235, 235, 235], "padding must not leak into luma");
    }

    #[test]
    fn chroma_is_averaged_over_each_two_by_two_block() {
        // NV12 subsamples chroma 2x2. Sampling only the top-left pixel would
        // be visibly wrong on fine coloured detail -- exactly what a screen
        // recording of text is made of.
        let mut src = vec![0u8; 8 * 2];
        // Top-left red, the other three black.
        src[0] = 0; src[1] = 0; src[2] = 255; src[3] = 255;
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        let (u, v) = (out[4], out[5]);
        // Averaged red-and-black sits between neutral and pure red's chroma;
        // a top-left-only sample would give pure red's values.
        let mut red_only = Vec::new();
        bgra_to_nv12(&solid(2, 2, 8, 0, 0, 255), 8, 2, 2, &mut red_only).unwrap();
        assert!(
            (u as i16 - 128).abs() < (red_only[4] as i16 - 128).abs(),
            "chroma must be averaged, not point-sampled"
        );
        assert!(
            (v as i16 - 128).abs() < (red_only[5] as i16 - 128).abs(),
            "chroma must be averaged, not point-sampled"
        );
    }

    #[test]
    fn a_short_input_is_refused_rather_than_read_past_the_end() {
        // A truncated frame is a real possibility (a texture map that
        // partially failed). Reading past the end is an out-of-bounds read
        // that ships whatever memory follows into the recording.
        let src = vec![0u8; 8]; // one row's worth for a 2x2 frame
        let mut out = Vec::new();
        assert!(matches!(
            bgra_to_nv12(&src, 8, 2, 2, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    #[test]
    fn odd_dimensions_are_refused_rather_than_silently_sheared() {
        let src = solid(3, 3, 12, 0, 0, 0);
        let mut out = Vec::new();
        assert!(matches!(
            bgra_to_nv12(&src, 12, 3, 3, &mut out),
            Err(ConvertError::OddDimensions)
        ));
    }

    #[test]
    fn the_output_buffer_is_reused_across_frames_without_growing() {
        // At 60 fps and 4K, allocating an NV12 frame per tick is megabytes
        // per second of allocator churn in the hot path. The converter must
        // resize once and then reuse.
        let src = solid(4, 4, 16, 10, 20, 30);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 16, 4, 4, &mut out).unwrap();
        let first_cap = out.capacity();
        for _ in 0..50 {
            bgra_to_nv12(&src, 16, 4, 4, &mut out).unwrap();
        }
        assert_eq!(out.len(), nv12_len(4, 4));
        assert_eq!(out.capacity(), first_cap, "no reallocation across frames");
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen convert
```

Expected: compile error — nothing in `convert` exists yet.

- [ ] **Step 3: Write the implementation**

Prepend to `src-tauri/screen/src/convert.rs`:

```rust
//! BGRA -> NV12, the one per-frame cost in the capture path.
//!
//! `windows-capture` delivers 8-bit BGRA; every hardware H.264 encoder MFT
//! wants NV12. Feeding the encoder BGRA works only where Windows happens to
//! insert a software colour converter, which is the kind of machine-specific
//! behaviour the dropped-frame counter would report and nobody could explain.
//!
//! This is pure arithmetic over two byte slices, so it is fully unit-tested
//! on Linux. Given no CI runner can record a screen, everything that CAN be
//! proven without one is proven here.
//!
//! COLOUR SPACE: BT.709, LIMITED range (luma 16-235, chroma centred on 128).
//! Screen captures are HD or larger and BT.709 is what every HD decoder
//! assumes; using BT.601 while the decoder assumes BT.709 is the classic
//! slightly-wrong-colours bug. `sink.rs` declares the same matrix on its
//! NV12 input type, so the encoder is told what this produced rather than
//! guessing from the frame size.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertError {
    /// The source buffer is smaller than `stride * height`. Converting it
    /// anyway would read past the end and ship unrelated memory into the
    /// recording.
    ShortInput { needed: usize, got: usize },
    /// NV12 subsamples chroma 2x2, so an odd dimension has half a chroma
    /// sample. Callers round down once via `even_dims` at capture start.
    OddDimensions,
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::ShortInput { needed, got } => {
                write!(f, "frame buffer too small: needed {needed} bytes, got {got}")
            }
            ConvertError::OddDimensions => write!(f, "NV12 requires even dimensions"),
        }
    }
}

/// NV12 is 1.5 bytes per pixel: a full-size luma plane plus a half-size
/// interleaved chroma plane.
pub fn nv12_len(width: u32, height: u32) -> usize {
    let y = width as usize * height as usize;
    y + y / 2
}

/// Round a frame size down to even. DOWN, never up: rounding up would index
/// past the end of the source frame. At most one row and one column are
/// lost, deliberately, once, at capture start.
pub fn even_dims(width: u32, height: u32) -> (u32, u32) {
    (width & !1, height & !1)
}

/// BT.709 limited-range luma, 16..=235.
#[inline]
fn luma709(r: f32, g: f32, b: f32) -> u8 {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    (16.0 + y * (219.0 / 255.0)).round().clamp(16.0, 235.0) as u8
}

/// BT.709 limited-range chroma pair, 16..=240, centred on 128.
#[inline]
fn chroma709(r: f32, g: f32, b: f32) -> (u8, u8) {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let u = 128.0 + ((b - y) / 1.8556) * (224.0 / 255.0);
    let v = 128.0 + ((r - y) / 1.5748) * (224.0 / 255.0);
    (
        u.round().clamp(16.0, 240.0) as u8,
        v.round().clamp(16.0, 240.0) as u8,
    )
}

/// Convert one BGRA frame into `out`, reusing its allocation.
///
/// `stride` is the source's BYTES PER ROW, which for a D3D11 staging texture
/// is usually wider than `width * 4`. Reading the buffer as tightly packed
/// shears the image progressively down the frame — a bug that presents as a
/// capture glitch rather than as a stride mistake.
pub fn bgra_to_nv12(
    bgra: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), ConvertError> {
    if width % 2 != 0 || height % 2 != 0 {
        return Err(ConvertError::OddDimensions);
    }
    let needed = stride * height as usize;
    if bgra.len() < needed {
        return Err(ConvertError::ShortInput {
            needed,
            got: bgra.len(),
        });
    }

    let (w, h) = (width as usize, height as usize);
    let y_len = w * h;
    // resize() keeps the existing allocation when the length is unchanged,
    // which is the whole point: at 60 fps and 4K a per-frame allocation is
    // megabytes per second of churn in the hot path.
    out.resize(nv12_len(width, height), 0);
    let (y_plane, uv_plane) = out.split_at_mut(y_len);

    for row in 0..h {
        let src_row = row * stride;
        let dst_row = row * w;
        for col in 0..w {
            let i = src_row + col * 4;
            let b = bgra[i] as f32;
            let g = bgra[i + 1] as f32;
            let r = bgra[i + 2] as f32;
            y_plane[dst_row + col] = luma709(r, g, b);
        }
    }

    // Chroma is AVERAGED over each 2x2 block, not point-sampled from the
    // top-left. A screen recording is mostly fine coloured detail (text,
    // syntax highlighting); point-sampling it is visibly wrong.
    for by in 0..h / 2 {
        for bx in 0..w / 2 {
            let mut sr = 0.0f32;
            let mut sg = 0.0f32;
            let mut sb = 0.0f32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = (by * 2 + dy) * stride + (bx * 2 + dx) * 4;
                    sb += bgra[i] as f32;
                    sg += bgra[i + 1] as f32;
                    sr += bgra[i + 2] as f32;
                }
            }
            let (u, v) = chroma709(sr / 4.0, sg / 4.0, sb / 4.0);
            let o = (by * w / 2 + bx) * 2;
            uv_plane[o] = u;
            uv_plane[o + 1] = v;
        }
    }

    Ok(())
}
```

Register it in `src-tauri/screen/src/lib.rs`:

```rust
pub mod convert;
```

- [ ] **Step 4: Tell the encoder what the converter produced**

In `src-tauri/screen/src/sink.rs`, inside `video_input_type`, add the matrix
declaration after the subtype line — and the same in `video_output_type`:

```rust
            // Declared, not inferred. convert.rs produces BT.709 limited
            // range; without this the encoder infers a matrix from the frame
            // size and a sub-HD window capture would be tagged BT.601 while
            // carrying BT.709 samples.
            t.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
            t.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?;
```

Verify it type-checks:

```bash
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

- [ ] **Step 5: Run the tests and watch them pass**

```bash
cd src-tauri && cargo test -p vault_buddy_screen convert
```

Expected: 10 passed. If `pure_red_lands_on_the_bt709_coefficients_not_the_bt601_ones`
fails, do **not** adjust the expected value to match the code — check the
coefficients first; that test exists to catch exactly that edit.

- [ ] **Step 6: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cd src-tauri && cargo test -p vault_buddy_screen
```

- [ ] **Step 7: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/screen/src/convert.rs src-tauri/screen/src/lib.rs src-tauri/screen/src/sink.rs
cat > /tmp/msg.txt <<'EOF'
feat(screen): convert captured BGRA frames to NV12

windows-capture delivers BGRA; every hardware H.264 encoder MFT wants NV12.
Feeding the encoder BGRA works only where Windows happens to insert a
software colour converter -- machine-specific behaviour that would surface
as dropped frames nobody could explain.

The conversion is pure arithmetic over two byte slices, so it lives in its
own module and is fully tested on Linux. No CI runner can record a screen,
so everything provable without one is proven here.

BT.709 limited range, and sink.rs now DECLARES that matrix on both media
types rather than letting the encoder infer one from the frame size: a
sub-HD window capture would otherwise be tagged BT.601 while carrying
BT.709 samples.

Three failures the tests name because each is invisible rather than loud:
row padding must be skipped (a D3D11 staging texture's pitch is wider than
width*4, and reading it as packed shears the image progressively down the
frame); chroma must be averaged over each 2x2 block rather than
point-sampled (a screen recording is mostly fine coloured detail); and a
short buffer must be refused rather than read past, which would ship
unrelated memory into the recording.

The output buffer is reused across frames -- at 60fps and 4K a per-frame
allocation is megabytes per second of churn in the hot path.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 7: The capture session (`screen::session`)

Spec §6.1–6.3. This is where the pieces meet: frames, audio, one clock, one
sink.

**Thread architecture, and why a third thread exists.**

| Thread | Name | Owns |
| --- | --- | --- |
| Frame worker | `screen-frames` | The `windows-capture` session; BGRA → NV12; sends to the mux |
| Audio worker | `screen-audio` | The cpal `SourceInput` receivers (the `Stream`s stay with the caller, they are `!Send`); downmix → resample → mix; sends to the mux |
| Mux | `screen-mux` | **The only owner of the `FragmentedSink`** |

The spec's §6.1 table lists two threads; this adds the mux thread for a
concrete reason. `IMFSample` is a COM interface pointer and is not `Send`,
so a sample built on the frame thread cannot be handed to the audio thread's
writer or vice versa, and writing to one `IMFSinkWriter` from two threads
raises apartment questions nobody wants to answer from Linux with a
ten-minute feedback loop. So **the channels carry plain byte vectors plus a
timestamp**, and one thread builds every `IMFSample` and makes every
`WriteSample` call. The cost is one memcpy per frame — the frame was already
copied out of the GPU staging texture, so this is a second copy of data
already in RAM, not a new GPU readback.

**One clock, shared.** `screen::clock::CaptureClock` lives behind an
`Arc<Mutex<..>>` that both producers read. `output_ts()` returns `None`
while paused, and **a producer whose `output_ts` is `None` drains and
discards** (spec §6.3) — streams stay open, because tearing down and
recreating a WGC session on every pause drops frames on resume and can fail
outright if the target window changed state. Because both streams stamp from
this one clock, A/V sync across a pause is structural, not incidental: there
is no second time base to drift from.

**The static-screen heartbeat.** WGC delivers a frame only when something
changes. A screen left untouched for 30 seconds therefore produces no
samples, no fragment closes, and a crash loses the whole idle stretch —
which defeats the reason §6.4 chose fragmented MP4 in the first place. So
the mux thread **repeats the last frame** when none has arrived for
`HEARTBEAT`. That decision is a pure function and is unit-tested on Linux.

**Files:**
- Create: `src-tauri/screen/src/session.rs`
- Delete: `src-tauri/screen/src/engine.rs` (superseded; see Step 6)
- Modify: `src-tauri/screen/src/lib.rs`

**Interfaces:**
- Consumes: `staging`, `sink::{FragmentedSink, VideoFormat, AudioFormat}`,
  `source::ResolvedSource`, `convert`, `clock::CaptureClock`,
  `vault_buddy_capture::session::{SourceInput, SourceMsg}`,
  `vault_buddy_capture::mixer::{downmix_to_mono, resample_linear, mix_n_to_stereo_i16}`,
  `vault_buddy_core::screen_capture_config::{ScreenQuality, bitrate_bps, normalize_fps}`.
- Produces:
  - `session::Control { Stop, Pause, Resume }`
  - `session::ScreenSessionParams { source: ResolvedSource, part: PathBuf, staged: PathBuf, fps: u32, quality: ScreenQuality, audio: Vec<SourceInput>, warn_tx: Option<Sender<String>>, stats_tx: Option<Sender<FrameStats>> }`
  - `session::FrameStats { fps: f32, dropped: u64 }`
  - `session::ScreenOutcome { mp4: PathBuf, duration_ms: u64, paused_ms: u64, width: u32, height: u32, dropped: u64, warning: Option<String> }`
  - `session::ScreenSession` with `start(params) -> Result<ScreenSession, ScreenError>`,
    `pause(&self)`, `resume(&self)`, `is_running(&self) -> bool`,
    `stop(self) -> Result<ScreenOutcome, ScreenError>`
  - `session::pacing::{frame_duration, should_repeat, audio_chunk_ready}` — pure, tested on Linux

  Task 8's `screen_commands.rs` is the only caller.

**Audio format decisions, stated once:**

- Everything is resampled to **48 000 Hz stereo 16-bit** — the rate every
  Windows AAC encoder MFT is required to accept. The existing MP3 path
  targets 44 100; that path is untouched.
- Mixing is `mixer::mix_n_to_stereo_i16` (Phase 1), which sums N equal-length
  mono buffers through `soft_clip`. **No per-source gain normalisation** —
  spec §6.5 rules it out because dividing by N would change existing
  two-source meeting-recording levels.
- Zero audio sources is legal: `AudioFormat` is `None`, the sink declares no
  audio stream, and `write_audio` is a no-op.

- [ ] **Step 1: Write the failing tests for the pure pacing rules**

Create `src-tauri/screen/src/session.rs` with ONLY the pacing module and its
tests:

```rust
/// The timing rules, kept pure so they are provable on Linux. No CI runner
/// can record a screen, so anything that can be decided without one is
/// decided here.
pub mod pacing {
    use std::time::Duration;

    /// How long a frame occupies at `fps`. Used as each sample's duration.
    pub fn frame_duration(fps: u32) -> Duration {
        Duration::from_nanos(1_000_000_000 / fps.max(1) as u64)
    }

    /// Should the mux repeat the previous frame?
    ///
    /// WGC delivers a frame only when something CHANGES. A screen left
    /// untouched produces no samples, so no fragment closes, so a crash
    /// loses the whole idle stretch — which defeats the reason spec 6.4
    /// chose fragmented MP4. Repeating the last frame keeps fragments
    /// closing and keeps the file playable at a steady rate.
    pub fn should_repeat(since_last_frame: Duration, heartbeat: Duration) -> bool {
        since_last_frame >= heartbeat
    }

    /// Is there enough mixed audio buffered to emit a chunk?
    ///
    /// The AAC encoder works in 1024-sample frames; feeding it a handful of
    /// samples per callback multiplies per-sample overhead by orders of
    /// magnitude. `min_frames` batches several AAC frames per write.
    pub fn audio_chunk_ready(buffered_frames: usize, min_frames: usize) -> bool {
        buffered_frames >= min_frames
    }
}

#[cfg(test)]
mod tests {
    use super::pacing::*;
    use std::time::Duration;

    #[test]
    fn a_frame_lasts_its_share_of_a_second() {
        assert_eq!(frame_duration(30), Duration::from_nanos(33_333_333));
        assert_eq!(frame_duration(60), Duration::from_nanos(16_666_666));
    }

    #[test]
    fn frame_duration_never_divides_by_zero() {
        // fps reaches here from config. normalize_fps guards the config
        // path, but a division by zero in the capture hot path is a panic
        // across the WebView2 FFI boundary — an abort with no crash record.
        assert_eq!(frame_duration(0), frame_duration(1));
    }

    #[test]
    fn a_still_screen_repeats_its_last_frame_once_the_heartbeat_elapses() {
        let hb = Duration::from_millis(500);
        assert!(!should_repeat(Duration::from_millis(499), hb));
        assert!(should_repeat(Duration::from_millis(500), hb));
        assert!(should_repeat(Duration::from_secs(30), hb));
    }

    #[test]
    fn audio_is_batched_rather_than_written_per_callback() {
        assert!(!audio_chunk_ready(1023, 1024));
        assert!(audio_chunk_ready(1024, 1024));
        assert!(audio_chunk_ready(9999, 1024));
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen session
```

Expected: the four pacing tests **pass** — they test only the module Step 1
just wrote. That is the correct state for a pure-logic module written
test-first in one file. The failing test that drives the rest of the task is
Step 3's: it names `ScreenSession`, which does not exist yet, so the file
will not compile until Step 3 defines it.

- [ ] **Step 3: Write the shared session types and the non-Windows arm**

Prepend to `src-tauri/screen/src/session.rs`:

```rust
//! The capture session: frames and audio, one clock, one sink.
//!
//! THREE threads, not the two spec 6.1 lists, for a concrete reason.
//! `IMFSample` is a COM interface pointer and is not `Send`, so a sample
//! built on the frame thread cannot be handed anywhere else, and writing one
//! `IMFSinkWriter` from two threads raises apartment questions with a
//! ten-minute feedback loop attached. So the channels carry PLAIN BYTE
//! VECTORS plus a timestamp, and `screen-mux` is the only thread that ever
//! touches the sink. The cost is one memcpy of data already in RAM — the
//! frame was copied out of the GPU staging texture regardless.
//!
//! ONE CLOCK (spec 6.2). Both producers stamp from the same
//! `CaptureClock`, so A/V sync across a pause is structural rather than
//! incidental: there is no second time base to drift from. While paused,
//! `output_ts` returns None and producers DRAIN AND DISCARD (spec 6.3) —
//! the streams stay open, because tearing down and recreating a WGC session
//! on every pause drops frames on resume and can fail outright if the
//! target window changed state.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::time::Duration;

use vault_buddy_capture::session::SourceInput;
use vault_buddy_core::screen_capture_config::ScreenQuality;

use crate::source::ResolvedSource;
use crate::ScreenError;

pub mod pacing; // (the module written in Step 1 stays exactly as it is)

/// Session control. ONE channel carries all three, following the audio
/// domain's documented reason: a single forwarding point means no second
/// signalling path can race the stop.
pub enum Control {
    Stop,
    Pause,
    Resume,
}

/// Advisory, ~2 Hz, lossy by design (spec 11). A gone receiver must never
/// slow or fail the capture path.
#[derive(Debug, Clone, Copy)]
pub struct FrameStats {
    pub fps: f32,
    pub dropped: u64,
}

pub struct ScreenSessionParams {
    pub source: ResolvedSource,
    /// The hidden in-progress file (`.<base>.mp4.part`).
    pub part: PathBuf,
    /// Where `part` is renamed on a clean finalize (`<base>.mp4`).
    pub staged: PathBuf,
    pub fps: u32,
    pub quality: ScreenQuality,
    /// Already-opened audio sources. The cpal `Stream`s stay with the
    /// caller — they are `!Send` and must outlive the session.
    pub audio: Vec<SourceInput>,
    pub warn_tx: Option<Sender<String>>,
    pub stats_tx: Option<Sender<FrameStats>>,
}

pub struct ScreenOutcome {
    pub mp4: PathBuf,
    pub duration_ms: u64,
    pub paused_ms: u64,
    pub width: u32,
    pub height: u32,
    pub dropped: u64,
    pub warning: Option<String>,
}

/// Repeat the last frame after this long with nothing new, so fragments
/// keep closing on a still screen (see `pacing::should_repeat`).
pub const HEARTBEAT: Duration = Duration::from_millis(500);
/// Everything is resampled to this: the rate every Windows AAC encoder MFT
/// is required to accept. The MP3 path's 44 100 is untouched.
pub const AUDIO_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_BITRATE_BPS: u32 = 128_000;
/// Batch this many stereo frames per AAC write. The encoder works in
/// 1024-sample frames; writing a handful per cpal callback multiplies
/// per-sample overhead by orders of magnitude.
pub const AUDIO_CHUNK_FRAMES: usize = 4096;
```

Then the non-Windows arm, which must compile and degrade:

```rust
#[cfg(not(windows))]
mod imp {
    use super::*;

    pub struct ScreenSession;

    impl ScreenSession {
        pub fn start(_params: ScreenSessionParams) -> Result<ScreenSession, ScreenError> {
            Err(ScreenError::Unsupported)
        }
        pub fn pause(&self) {}
        pub fn resume(&self) {}
        pub fn is_running(&self) -> bool {
            false
        }
        pub fn stop(self) -> Result<ScreenOutcome, ScreenError> {
            Err(ScreenError::Unsupported)
        }
    }
}

pub use imp::ScreenSession;
```

and the test that pins the degradation — note that it is a COMPILE-time
proof, not a runtime one:

```rust
    #[cfg(not(windows))]
    #[test]
    fn a_capture_cannot_even_be_constructed_off_windows() {
        // `source::SourceHandle` is an uninhabited enum on non-Windows, so
        // `ResolvedSource` is unconstructible, so `ScreenSessionParams`
        // cannot be built and `ScreenSession::start` is statically
        // unreachable here. That is a stronger guarantee than a runtime
        // Unsupported, and it is why this file has no runtime stub test.
        //
        // This replaces the phase-1 `engine::start_capture` stub test that
        // `engine.rs`'s deletion removes (Step 6). If a future change gives
        // SourceHandle a non-Windows variant, the `match h {}` below stops
        // compiling — which is the alarm.
        fn _assert_uninhabited(h: crate::source::SourceHandle) -> ! {
            match h {}
        }
        assert!(matches!(
            crate::source::resolve(&crate::source::SourceId::Screen(0)),
            Err(ScreenError::Unsupported)
        ));
    }
```

- [ ] **Step 4: Write the Windows arm**

Append to `src-tauri/screen/src/session.rs`. This is the largest single body
in the phase; build it in the order below and run the Windows-target clippy
after each of the three blocks.

**4a — the mux thread and its message type:**

```rust
#[cfg(windows)]
mod imp {
    use super::*;
    use crate::clock::CaptureClock;
    use crate::convert;
    use crate::sink::{AudioFormat, FragmentedSink, VideoFormat};
    use crate::source::SourceHandle;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::Instant;
    use vault_buddy_capture::mixer;
    use vault_buddy_capture::session::SourceMsg;
    use vault_buddy_core::screen_capture_config::{bitrate_bps, normalize_fps};

    /// What crosses to the mux thread. Plain bytes, never an IMFSample:
    /// COM interface pointers are not Send.
    enum MuxMsg {
        Video { nv12: Vec<u8>, ts: Duration },
        Audio { pcm: Vec<i16>, ts: Duration },
    }

    /// Shared, pause-aware time base for BOTH producers (spec 6.2).
    type SharedClock = Arc<Mutex<CaptureClock>>;

    fn output_ts(clock: &SharedClock) -> Option<Duration> {
        clock
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .output_ts(Instant::now())
    }

    /// The mux thread: the ONLY owner of the sink.
    ///
    /// It also carries the still-screen heartbeat. WGC emits a frame only
    /// when something changes, so an untouched screen closes no fragments
    /// and a crash would lose the whole idle stretch — exactly what spec
    /// 6.4's container choice exists to prevent.
    fn run_mux(
        mut sink: FragmentedSink,
        rx: Receiver<MuxMsg>,
        fps: u32,
        dropped: Arc<AtomicU64>,
    ) -> Result<(), ScreenError> {
        let frame_dur = pacing::frame_duration(fps);
        let mut last_frame: Option<Vec<u8>> = None;
        let mut last_emit = Instant::now();
        let mut last_ts = Duration::ZERO;

        loop {
            match rx.recv_timeout(HEARTBEAT) {
                Ok(MuxMsg::Video { nv12, ts }) => {
                    sink.write_video(&nv12, ts, frame_dur)?;
                    last_ts = ts;
                    last_frame = Some(nv12);
                    last_emit = Instant::now();
                }
                Ok(MuxMsg::Audio { pcm, ts }) => {
                    let dur = Duration::from_nanos(
                        (pcm.len() as u64 / AUDIO_CHANNELS as u64) * 1_000_000_000
                            / AUDIO_RATE as u64,
                    );
                    sink.write_audio(&pcm, ts, dur)?;
                }
                Err(RecvTimeoutError::Timeout) => {
                    // Still screen: repeat the last frame so fragments keep
                    // closing and the file stays playable at a steady rate.
                    if pacing::should_repeat(last_emit.elapsed(), HEARTBEAT) {
                        if let Some(frame) = &last_frame {
                            last_ts += last_emit.elapsed();
                            sink.write_video(frame, last_ts, frame_dur)?;
                            last_emit = Instant::now();
                        }
                    }
                }
                // Both producers dropped their senders: the capture is over.
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        log::info!(
            "screen capture: finalizing after {} dropped frame(s)",
            dropped.load(Ordering::Relaxed)
        );
        // Finalize is the ONLY drain. Never call Flush here — it discards
        // pending samples (see sink.rs's module docs).
        sink.finalize()
    }
}
```

Run the Windows clippy now, before adding more:

```bash
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

**4b — the audio thread.** Inside the same `mod imp`:

```rust
    /// Drain every source, downmix to mono, resample to AUDIO_RATE, mix to
    /// stereo, and hand batched chunks to the mux.
    ///
    /// While paused the drained samples are DISCARDED (spec 6.3): the
    /// streams stay open so resume is instant, but paused wall-clock time
    /// never appears in the output.
    fn run_audio(
        sources: Vec<SourceInput>,
        clock: SharedClock,
        tx: mpsc::SyncSender<MuxMsg>,
        stop: Arc<AtomicBool>,
    ) {
        // One growing buffer per source; they arrive at different rates and
        // must be mixed frame-aligned.
        let mut buffers: Vec<Vec<f32>> = vec![Vec::new(); sources.len()];
        let mut emitted_frames: u64 = 0;

        while !stop.load(Ordering::Relaxed) {
            let mut got_anything = false;
            for (i, src) in sources.iter().enumerate() {
                while let Ok(msg) = src.rx.try_recv() {
                    got_anything = true;
                    match msg {
                        SourceMsg::Samples(raw) => {
                            let mono = mixer::downmix_to_mono(&raw, src.channels);
                            let at_rate = mixer::resample_linear(&mono, src.rate, AUDIO_RATE);
                            buffers[i].extend_from_slice(&at_rate);
                        }
                        SourceMsg::Lost => {
                            // The audio domain's posture: one device dropping
                            // out never stops the capture (spec 14).
                            log::warn!("screen capture: audio source '{}' was lost", src.name);
                        }
                    }
                }
            }

            // While paused, throw away whatever arrived rather than buffering
            // it: buffered paused audio would surface as a burst on resume.
            let paused = output_ts(&clock).is_none();
            if paused {
                for b in &mut buffers {
                    b.clear();
                }
            }

            let ready = buffers.iter().map(|b| b.len()).min().unwrap_or(0);
            if !paused && pacing::audio_chunk_ready(ready, AUDIO_CHUNK_FRAMES) {
                let slices: Vec<&[f32]> = buffers.iter().map(|b| &b[..ready]).collect();
                let stereo = mixer::mix_n_to_stereo_i16(&slices);
                for b in &mut buffers {
                    b.drain(..ready);
                }
                // Audio timestamps come from the SAMPLE COUNT, not the wall
                // clock: a wall-clock stamp would drift against the sample
                // rate and desync from the video within minutes. The shared
                // clock's only job here is deciding what is discarded.
                let ts = Duration::from_nanos(emitted_frames * 1_000_000_000 / AUDIO_RATE as u64);
                emitted_frames += ready as u64;
                if tx.send(MuxMsg::Audio { pcm: stereo, ts }).is_err() {
                    break; // the mux is gone; nothing left to write to
                }
            }

            if !got_anything {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
```

**4c — the frame thread and `ScreenSession`.** The `windows-capture` handler
implements `GraphicsCaptureApiHandler`; its `on_frame_arrived` converts and
sends. Inside the same `mod imp`:

```rust
    use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
    use windows_capture::frame::Frame;
    use windows_capture::graphics_capture_api::InternalCaptureControl;
    use windows_capture::settings::{
        ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
        MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
    };

    struct FrameFlags {
        clock: SharedClock,
        tx: mpsc::SyncSender<MuxMsg>,
        width: u32,
        height: u32,
        dropped: Arc<AtomicU64>,
        stop: Arc<AtomicBool>,
    }

    struct FrameHandler {
        flags: FrameFlags,
        /// Reused across frames: a per-frame NV12 allocation at 4K60 is
        /// megabytes per second of allocator churn in the hot path.
        nv12: Vec<u8>,
    }

    impl GraphicsCaptureApiHandler for FrameHandler {
        type Flags = FrameFlags;
        type Error = ScreenError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(FrameHandler {
                flags: ctx.flags,
                nv12: Vec::new(),
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if self.flags.stop.load(Ordering::Relaxed) {
                capture_control.stop();
                return Ok(());
            }
            // Paused: drain and discard (spec 6.3). The WGC session stays
            // open so resume is instant.
            let Some(ts) = output_ts(&self.flags.clock) else {
                return Ok(());
            };

            let mut buffer = frame.buffer().map_err(|e| ScreenError::Io(e.to_string()))?;
            // The PADDED buffer plus its row pitch, deliberately — NOT
            // `as_nopadding_buffer`, which copies the whole frame to strip
            // padding that `convert::bgra_to_nv12` already skips. That
            // stride parameter exists precisely so this copy is unnecessary.
            let stride = buffer.row_pitch() as usize;
            let bytes = buffer.as_raw_buffer();

            if let Err(e) = convert::bgra_to_nv12(
                bytes,
                stride,
                self.flags.width,
                self.flags.height,
                &mut self.nv12,
            ) {
                // A malformed frame is dropped, never fatal: one bad frame
                // must not end a recording that is otherwise fine.
                self.flags.dropped.fetch_add(1, Ordering::Relaxed);
                log::warn!("screen capture: dropped a frame: {e}");
                return Ok(());
            }

            // try_send semantics via send-on-a-bounded-channel are NOT used:
            // an unbounded channel with a slow mux would grow without limit,
            // but a bounded one that BLOCKS here would stall the WGC
            // callback and drop frames at the source with no counter. So the
            // channel is bounded and a full channel counts a drop and moves
            // on — visible in `screen:frames`, which is what that counter is
            // for (spec 17.3).
            match self.flags.tx.try_send(MuxMsg::Video {
                nv12: self.nv12.clone(),
                ts,
            }) {
                Ok(()) => {}
                Err(mpsc::TrySendError::Full(_)) => {
                    self.flags.dropped.fetch_add(1, Ordering::Relaxed);
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    capture_control.stop();
                }
            }
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            // Spec 14: the source vanishing during capture is a WARNING and
            // the capture finalizes cleanly with what it has. A closed window
            // must not lose the preceding recording.
            log::warn!("screen capture: the capture source closed");
            self.flags.stop.store(true, Ordering::Relaxed);
            Ok(())
        }
    }
```

**Note the bounded channel.** `std::sync::mpsc::channel` is unbounded and
has no `try_send`; use `std::sync::mpsc::sync_channel(CHANNEL_DEPTH)` (a
`SyncSender` does have `try_send`). Add:

```rust
    /// Frames in flight between the capture callback and the mux. Deep
    /// enough to ride out a fragment write, shallow enough that a genuinely
    /// stalled mux shows up as counted drops within a second rather than as
    /// unbounded memory growth.
    const CHANNEL_DEPTH: usize = 16;
```

Then `ScreenSession` itself — `start` resolves the format, creates the sink,
spawns the three named threads; `stop` signals, joins, finalizes, and renames
the `.part` onto the staged name via `capture_paths::rename_noreplace` (never
`std::fs::rename`, which replaces on every platform):

```rust
    pub struct ScreenSession {
        control_tx: mpsc::Sender<Control>,
        handle: JoinHandle<Result<ScreenOutcome, ScreenError>>,
        running: Arc<AtomicBool>,
    }
```

`start` must: `normalize_fps(params.fps)`; `convert::even_dims` the source's
width/height; `bitrate_bps(quality, w, h, fps)`; build
`Some(AudioFormat { sample_rate: AUDIO_RATE, channels: AUDIO_CHANNELS, bitrate_bps: AUDIO_BITRATE_BPS })`
only when `!params.audio.is_empty()`; `FragmentedSink::create(&params.part, ..)`;
spawn `screen-mux`, `screen-audio`, `screen-frames` (the last via
`FrameHandler::start_free_threaded(settings)`); and return.

`stop` must, **in this order**: set `stop`, take the WGC `CaptureControl` and
`stop()` it, drop the audio sender, join the mux (which finalizes), then
rename. Finalize before rename, always — renaming a file the sink still owns
is how you get a zero-byte staged capture.

- [ ] **Step 5: Type-check and test**

```bash
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cd src-tauri && cargo test -p vault_buddy_screen
```

Expected: Windows clippy clean; the Linux pacing tests pass. **The Windows
arm is not executed anywhere in this task** — that is the honest state, and
the Windows verification checklist (Task 11) is where it gets exercised.

- [ ] **Step 6: Retire `engine.rs`**

`engine.rs` was Phase 1's placeholder for exactly this module and the spec's
§4.1 file list does not contain it. Leaving a second "start a capture"
entry point that always returns `Unsupported` would be a live trap for the
next reader.

```bash
cd /home/user/vault-buddy && git rm src-tauri/screen/src/engine.rs
```

In `src-tauri/screen/src/lib.rs`, remove `pub mod engine;` and remove the
`the_engine_stub_reports_unsupported_rather_than_panicking` test — Step 3's
`a_resolved_source_cannot_even_be_constructed_off_windows` replaces it with a
stronger, compile-time guarantee. Keep every other test in `lib.rs` exactly
as it is.

- [ ] **Step 7: Run the gates**

```bash
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cd src-tauri && cargo test -p vault_buddy_screen
cd src-tauri && cargo machete .
```

Also check the LOC guard now, because `session.rs` is the file most likely to
cross 800 non-blank lines:

```bash
cd /home/user/vault-buddy && npm run check:loc
```

If it is over, **split rather than raise the baseline**: move the frame
handler into `screen/src/frames.rs`. A raised baseline needs a written
justification and this one has a clean split available.

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/screen/src/session.rs src-tauri/screen/src/lib.rs
cat > /tmp/msg.txt <<'EOF'
feat(screen): add the capture session -- frames, audio, one clock, one sink

Three threads, not the two the spec's table lists. IMFSample is a COM
interface pointer and is not Send, so a sample built on the frame thread
cannot be handed anywhere else, and writing one IMFSinkWriter from two
threads raises apartment questions with a ten-minute feedback loop
attached. The channels therefore carry plain byte vectors plus a timestamp
and a single screen-mux thread owns the sink. The cost is one memcpy of
data already in RAM; the frame was copied out of the GPU staging texture
regardless.

Both producers stamp from one CaptureClock, so A/V sync across a pause is
structural rather than incidental -- there is no second time base to drift
from. While paused both drain and discard: the streams stay open, because
tearing down and recreating a WGC session on every pause drops frames on
resume and can fail outright if the target window changed state.

Audio timestamps come from the emitted sample count rather than the wall
clock. A wall-clock stamp drifts against the sample rate and desyncs from
the video within minutes.

The mux repeats the last frame after half a second of stillness. WGC emits
a frame only when something changes, so an untouched screen closes no
fragments and a crash would lose the whole idle stretch -- which is exactly
what the fragmented container was chosen to prevent.

The frame channel is bounded and a full channel counts a drop rather than
blocking. Blocking would stall the WGC callback and drop frames at the
source with no counter; growing unbounded would trade a visible drop for
invisible memory growth.

A source closing mid-capture is a warning that finalizes cleanly, not a
failure: a closed window must not lose the recording that preceded it.

Retires engine.rs, phase 1's placeholder for this module. The spec's file
list has no engine, and a second always-Unsupported entry point would be a
trap for the next reader; its stub test is replaced by a compile-time proof
that a capture cannot even be constructed off Windows.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 8: The IPC surface (`screen_commands.rs`)

Spec §11, Phase 2's six commands of the eventual fifteen. The other nine
belong to Phases 3–6 and must not appear here.

| Command | Sync/async | Why |
| --- | --- | --- |
| `list_capture_sources` | **async** | WGC/WinRT enumeration takes hundreds of ms; on the main thread it stalls every window operation (the same reason `list_audio_devices` is async — GAP-22) |
| `start_screen_capture` | **async** | Device setup, sink creation and file I/O |
| `stop_screen_capture` | **async** | Waits for finalize, which is unbounded |
| `pause_screen_capture` | sync | A channel send |
| `resume_screen_capture` | sync | A channel send |
| `screen_capture_status` | sync | A mutex read |

**Events**, all `app.emit` (app-wide), per spec §11:
`screen:started`, `screen:paused`, `screen:resumed`, `screen:stopped`,
`screen:failed`, `screen:warning`, `screen:frames`.

**Files:**
- Create: `src-tauri/src/screen_commands.rs`
- Modify: `src-tauri/src/lib.rs` (`mod`, `.manage`, six `generate_handler` entries, shutdown block)
- Modify: `src-tauri/src/tray.rs` (`hide_buddy` no-ops during a screen capture)

**Interfaces:**
- Consumes: `capture_guard::{CaptureGuard, CaptureKind}` (Task 1),
  `vault_buddy_screen::{source, session, staging}` (Tasks 2, 5, 7),
  `vault_buddy_capture::devices::{DeviceSelection, open_selected_sources}` (Task 3),
  `capture_config::vault_config`, `commands::find_vault`.
- Produces:
  - `screen_commands::ScreenCaptureState(pub Mutex<Option<ActiveScreenCapture>>)`
  - `screen_commands::is_capturing(app: &AppHandle) -> bool`
  - `screen_commands::capture_blocks_shutdown(app: &AppHandle) -> bool`
  - `screen_commands::finalize_if_capturing(app: &AppHandle)`
  - `ScreenStatusPayload { capturing, vaultId, startedAtMs, paused, pausedTotalMs, pausedSinceMs, sourceTitle }`
  - `StagedCaptureDto { base, path, durationMs, sourceTitle, width, height }`
  - the six `#[tauri::command]` functions.

  Task 9's store invokes all six; Task 10's bar reads the status payload.

**The structure to mirror.** `capture_commands.rs` is the reference for every
shape here: an `ActiveScreenCapture` reservation behind a mutex, a named
worker thread owning the `!Send` cpal streams and forwarding `Control`, a
`ready_rx` handshake so a start that fails reports an error rather than a
phantom recording, one `emit_screen_failed` chokepoint, and a
`clear_active_screen` that both drops the reservation and releases the
`CaptureGuard`.

**Three places where this deliberately differs from the audio domain:**

1. **No startup-wedged janitor.** The audio domain's timeout janitor exists
   because a wedged audio driver can hang device setup past the 10 s
   handshake with a `.part` already on disk. Screen capture creates its sink
   before any WGC session exists, so a hang before the handshake has produced
   no file. A start that does not report within 15 s therefore **releases the
   guard and fails cleanly**. Document that in a comment: it is the
   difference between the two domains, not an oversight.
2. **Mutual exclusion is the guard's job, not a second reservation check.**
   `start_screen_capture` claims `CaptureKind::Screen` **first**; the
   `ScreenCaptureState` check is defence in depth behind it.
3. **A source that closes mid-capture is a warning, not a failure** (spec
   §14). `screen:warning` carries it and the capture finalizes with what it
   has.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/screen_commands.rs` with ONLY the test module (shell
crate tests run in `linux-app` via `cargo test -p vault-buddy --lib`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_payload_serializes_camel_case_for_the_webview() {
        let payload = ScreenStatusPayload {
            capturing: true,
            vault_id: Some("v1".into()),
            started_at_ms: Some(1_700_000_000_000),
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            source_title: Some("Screen 1".into()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"startedAtMs\""), "got {json}");
        assert!(json.contains("\"sourceTitle\""), "got {json}");
        assert!(!json.contains("started_at_ms"), "got {json}");
    }

    #[test]
    fn an_idle_status_payload_reports_nothing_rather_than_stale_values() {
        // A reloaded webview re-reads this. Leaking the last capture's vault
        // id or start time into an idle payload would render a phantom
        // capture bar counting up from a recording that ended.
        let p = ScreenStatusPayload::idle();
        assert!(!p.capturing);
        assert_eq!(p.vault_id, None);
        assert_eq!(p.started_at_ms, None);
        assert_eq!(p.source_title, None);
        assert_eq!(p.paused_total_ms, 0);
    }

    #[test]
    fn the_device_selection_is_built_from_the_ipc_arguments_verbatim() {
        // These names were ticked in front of the live device list. Any
        // normalization here (trimming, casing, dedupe) would stop them
        // matching what cpal reports and silently record nothing.
        let sel = selection_from(
            vec!["Microphone (Yeti)".to_string()],
            vec!["Speakers (Realtek)".to_string()],
        );
        assert_eq!(sel.inputs, vec!["Microphone (Yeti)".to_string()]);
        assert_eq!(sel.outputs, vec!["Speakers (Realtek)".to_string()]);
    }

    #[test]
    fn an_unparseable_source_id_is_refused_before_anything_is_claimed() {
        // The id crosses the IPC boundary and is untrusted. Refusing it here,
        // before the guard is claimed, means a malformed request cannot wedge
        // both capture domains.
        assert!(vault_buddy_screen::source::SourceId::parse("nonsense").is_none());
    }

    // Structural regression, mirroring config_lock_guard.rs: the guard must
    // be released from exactly one place in this file. A second release site
    // can free a claim on a path that never made one, letting a capture start
    // on top of a live one; a missing one wedges both domains until restart,
    // with no error and no log line.
    #[test]
    fn the_screen_guard_is_released_only_from_the_clear_chokepoint() {
        let src = include_str!("screen_commands.rs");
        let releases = src.matches("release(CaptureKind::Screen)").count();
        assert_eq!(
            releases, 1,
            "expected exactly one Screen release (clear_active_screen); found {releases}"
        );
    }
}
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd src-tauri && cargo test -p vault-buddy --lib screen_commands
```

Expected: compile error — the module is not registered and its types do not
exist.

- [ ] **Step 3: Write the implementation**

Prepend to `src-tauri/src/screen_commands.rs` the module doc, the state, the
DTOs, the chokepoints, and the six commands:

```rust
//! Screen capture IPC (spec 11). Six of the increment's fifteen commands;
//! the rest belong to phases 3-6.
//!
//! Shaped on `capture_commands.rs` deliberately — the same reservation
//! mutex, the same named worker owning the !Send cpal streams, the same
//! ready-handshake so a failed start reports an error instead of a phantom
//! capture, the same single emit-failed chokepoint.
//!
//! THREE deliberate differences from the audio domain:
//!
//! 1. No startup-wedged janitor. Audio needs one because a wedged driver can
//!    hang device setup past the handshake with a .part already on disk.
//!    Here the sink is created before any WGC session exists, so a hang
//!    before the handshake has produced no file: the start fails cleanly and
//!    releases the guard.
//! 2. Mutual exclusion lives in `CaptureGuard`, claimed FIRST. The
//!    reservation below is defence in depth behind it, not the mechanism.
//! 3. A source closing mid-capture is a WARNING that finalizes cleanly
//!    (spec 14): a closed window must not lose the recording before it.

use std::path::PathBuf;
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use vault_buddy_capture::devices::{open_selected_sources, DeviceSelection};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_core::{capture_config, capture_paths};
use vault_buddy_screen::session::{Control, ScreenOutcome, ScreenSessionParams};
use vault_buddy_screen::{source, staging};

use crate::capture_commands::{now_ms, toast};
use crate::capture_guard::{CaptureGuard, CaptureKind};

pub struct ActiveScreenCapture {
    pub control_tx: Sender<Control>,
    pub vault_id: String,
    pub source_title: String,
    pub started_at_ms: u64,
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    /// The `.part` the live session owns, once the worker has reserved it.
    pub part: Option<PathBuf>,
}

#[derive(Default)]
pub struct ScreenCaptureState(pub Mutex<Option<ActiveScreenCapture>>);

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenStatusPayload {
    pub capturing: bool,
    pub vault_id: Option<String>,
    pub started_at_ms: Option<u64>,
    pub paused: bool,
    pub paused_total_ms: u64,
    pub paused_since_ms: Option<u64>,
    pub source_title: Option<String>,
}

impl ScreenStatusPayload {
    /// Every field cleared. A reloaded webview re-reads the status; leaking
    /// the last capture's vault id or start time would render a phantom
    /// capture bar counting up from a recording that already ended.
    pub fn idle() -> ScreenStatusPayload {
        ScreenStatusPayload {
            capturing: false,
            vault_id: None,
            started_at_ms: None,
            paused: false,
            paused_total_ms: 0,
            paused_since_ms: None,
            source_title: None,
        }
    }
}

/// Build the device selection from the IPC arguments VERBATIM. These names
/// were ticked in front of the live device list; any normalization here
/// (trim, case-fold, dedupe) would stop them matching what cpal reports and
/// silently record nothing.
pub(crate) fn selection_from(inputs: Vec<String>, outputs: Vec<String>) -> DeviceSelection {
    DeviceSelection { inputs, outputs }
}

/// Every screen-capture failure surfaces through here — event AND toast —
/// so no path can log-and-vanish and leave the UI looking healthy.
fn emit_screen_failed(app: &AppHandle, message: &str) {
    log::error!("screen capture: failed: {message}");
    let _ = app.emit("screen:failed", serde_json::json!({ "message": message }));
    toast(app, "Screen capture failed", message);
}

/// THE chokepoint: drop the reservation and release the cross-domain claim
/// together, so no path can do one without the other.
fn clear_active_screen(app: &AppHandle) {
    *lock_ignoring_poison(&app.state::<ScreenCaptureState>().0) = None;
    app.state::<CaptureGuard>().release(CaptureKind::Screen);
}

pub fn is_capturing(app: &AppHandle) -> bool {
    lock_ignoring_poison(&app.state::<ScreenCaptureState>().0).is_some()
}

/// The buddy is the capture indicator for screen capture too, so a live
/// capture blocks hide and shutdown exactly as an audio recording does.
pub fn capture_blocks_shutdown(app: &AppHandle) -> bool {
    is_capturing(app)
}
```

The six commands follow the audio domain's bodies closely. Specifics that
are **not** transferable and must be written as described:

- **`list_capture_sources`** — `async`, `spawn_blocking`, calls
  `source::list_sources(&our_window_titles(app))` where `our_window_titles`
  collects the titles of `main`/`panel`/`bubble` via
  `app.webview_windows()`, so Vault Buddy never offers itself (spec §7.2).
  Returns `Vec<CaptureSourceInfo>`; a failure degrades to an empty vec, never
  an `Err`.
- **`start_screen_capture(id, sourceId, inputs, outputs)`** — `async` →
  `spawn_blocking`:
  1. `source::SourceId::parse(&source_id)` → `Err("Unknown capture source.")`
     **before** claiming anything.
  2. `app.state::<CaptureGuard>().try_claim(CaptureKind::Screen)` →
     `Err(held.busy_message())`.
  3. `find_vault(&id)`, read `capture_config::vault_config` for
     `screen_quality` / `screen_fps`.
  4. Resolve the staging dir from `app.path().app_local_data_dir()`,
     `create_dir_all`, `staging::reserve_base`.
  5. Install the reservation, spawn `screen-capture-device`, handshake on
     `ready_rx` with a **15 s** timeout; on timeout `clear_active_screen`
     and fail — see difference (1) above.
  6. On success emit `screen:started` and set the tray to
     `TrayCaptureState::Recording`.
- **`stop_screen_capture`** — `async`; sends `Control::Stop`, waits on the
  worker's `done_rx` (bounded at 30 s, like the audio stop), writes the
  sidecar via `staging::write_sidecar`, emits `screen:stopped` with the
  `StagedCaptureDto`, then `clear_active_screen` and resets the tray.
- **`pause_screen_capture` / `resume_screen_capture`** — sync; send the
  `Control` message, update the reservation's `paused` / `paused_total_ms` /
  `paused_since_ms` (mirroring the session, which owns the truth for the
  encoded timeline, so a reloaded webview's frozen-elapsed display resyncs),
  emit `screen:paused` / `screen:resumed`, set the tray.
- **`screen_capture_status`** — sync; reads the reservation into a
  `ScreenStatusPayload`, or `ScreenStatusPayload::idle()`.

- [ ] **Step 4: Wire the shell**

In `src-tauri/src/lib.rs`:

```rust
mod screen_commands;
```

```rust
        .manage(screen_commands::ScreenCaptureState::default())
```

and add the six entries to `generate_handler!`:

```rust
            screen_commands::list_capture_sources,
            screen_commands::start_screen_capture,
            screen_commands::stop_screen_capture,
            screen_commands::pause_screen_capture,
            screen_commands::resume_screen_capture,
            screen_commands::screen_capture_status,
```

In the `CloseRequested` handler, widen the shutdown block so Alt+F4 during a
screen capture finalizes instead of stranding a `.part`:

```rust
                if capture_commands::recording_blocks_shutdown(app)
                    || screen_commands::capture_blocks_shutdown(app)
                {
```

and inside that worker, finalize both:

```rust
                            capture_commands::finalize_if_recording(&app);
                            screen_commands::finalize_if_capturing(&app);
```

In `src-tauri/src/tray.rs`, widen the hide chokepoint — this is the invariant
that keeps the buddy visible as the capture indicator:

```rust
pub fn hide_buddy(app: &AppHandle) {
    if crate::capture_commands::recording_blocks_shutdown(app)
        || crate::screen_commands::capture_blocks_shutdown(app)
    {
        log::info!("hide ignored: a capture is in progress");
        return;
    }
```

Apply the same widening in `tray::quit`'s finalize branch.

- [ ] **Step 5: Run the tests and the gates**

```bash
cd src-tauri && cargo test -p vault-buddy --lib
cd src-tauri && cargo fmt --check
cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings
cd /home/user/vault-buddy && npm run check:loc
```

`screen_commands.rs` is the second file at risk of the 800-line cap. If it
is over, split the lifecycle worker into `screen_capture_worker.rs` rather
than raising the baseline.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src-tauri/src/screen_commands.rs src-tauri/src/lib.rs src-tauri/src/tray.rs
cat > /tmp/msg.txt <<'EOF'
feat(shell): add the screen capture IPC surface

Six of the increment's fifteen commands (spec 11); the rest belong to later
phases. Enumeration, start and stop are async on the blocking pool --
WGC/WinRT enumeration alone takes hundreds of milliseconds and would stall
every window operation on the main thread, the same reason
list_audio_devices is async. Pause, resume and status are sync: a channel
send and a mutex read.

Mutual exclusion is the CaptureGuard's, claimed before anything else and
released from one chokepoint that also drops the reservation, so no path can
do one without the other. The reservation's own check stays as defence in
depth rather than as the mechanism.

Unlike the audio domain there is no startup-wedged janitor, and the comment
says why rather than leaving it to look like an omission: audio needs one
because a wedged driver can hang device setup past the handshake with a
.part already on disk, whereas here the sink is created before any WGC
session exists, so a hang before the handshake has produced no file.

The buddy is the capture indicator for screen capture too, so hide_buddy
no-ops and Alt+F4 finalizes rather than stranding a .part.

An idle status payload clears every field: a reloaded webview re-reads it,
and leaking the last capture's start time would render a phantom capture bar
counting up from a recording that already ended.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 9: The source picker (store, view, `ScreenSourcePicker`, `ScreenAudioPicker`)

Spec §7.1–7.2. **Screen and Window tabs only** — Region is Phase 3, and a
disabled Region tab is dead UI that invites a click with nothing behind it.

**Files:**
- Create: `src/stores/screenCapture.ts`
- Create: `src/components/ScreenSourcePicker.vue`
- Create: `src/components/ScreenAudioPicker.vue`
- Create: `tests/screenSourcePicker.test.ts`
- Create: `tests/screenCaptureStore.test.ts`
- Modify: `src/types.ts`, `src/stores/vaults.ts`, `src/components/RecordMode.vue`,
  `src/components/ActionPanel.vue`, `src/roots/BuddyRoot.vue`, `src/roots/PanelRoot.vue`

**Interfaces:**
- Consumes: the six commands and seven events from Task 8.
- Produces:
  - `types.ts`: `CaptureSourceInfo { id, kind: "screen" | "window", title, detail, width, height, isPrimary }`,
    `ScreenCaptureStatus { capturing, vaultId, startedAtMs, paused, pausedTotalMs, pausedSinceMs, sourceTitle }`,
    `StagedCapture { base, path, durationMs, sourceTitle, width, height }`
  - `useScreenCaptureStore` with state `{ status, vaultId, sourceTitle, startedAtMs, paused, pausedTotalMs, pausedSinceMs, dropped, lastStaged, error }`
    and actions `init()`, `start(vaultId, sourceId, inputs, outputs)`, `pause()`,
    `resume()`, `stop()`.
  - `vaults` store: the `screenCapture` view, `screenCaptureVaultId`,
    `openScreenCapture(vaultId)`.

  Task 10's capture bar reads the store.

**The per-window wiring rule, which is not optional.** Both `BuddyRoot` and
`PanelRoot` must call `screenCapture.init()` (spec §11). Each window is its
own webview with its own Pinia instance; a store that mirrors Rust state and
is initialised in only one of them leaves the other with a dead indicator —
the documented failure that made `capture.init()` appear in both roots.

**Follow the existing view conventions:** `screenCapture`'s parent is
`recordMode` (so `back()` returns to the intake chooser, and the header
renders a ← rather than the magnifier/cog), it carries
`screenCaptureVaultId`, and `showList()` clears it like every other view id.

- [ ] **Step 1: Write the failing tests**

Create `tests/screenCaptureStore.test.ts`:

```ts
import { mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useScreenCaptureStore } from "../src/stores/screenCapture";

describe("screenCapture store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  it("starts idle and reports nothing", () => {
    const store = useScreenCaptureStore();
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.sourceTitle).toBeNull();
  });

  it("holds the capture's identity after a successful start", async () => {
    mockIPC((cmd) => {
      if (cmd === "start_screen_capture") {
        return {
          capturing: true,
          vaultId: "v1",
          startedAtMs: 1000,
          paused: false,
          pausedTotalMs: 0,
          pausedSinceMs: null,
          sourceTitle: "Screen 1",
        };
      }
      return undefined;
    });
    const store = useScreenCaptureStore();
    await store.start("v1", "screen:1", [], []);
    expect(store.status).toBe("capturing");
    expect(store.vaultId).toBe("v1");
    expect(store.sourceTitle).toBe("Screen 1");
  });

  it("stays idle and surfaces the message when a start is refused", async () => {
    // Spec 14's alreadyCapturing, and every other typed refusal: a failed
    // start must not leave the store believing a capture is running, or the
    // capture bar renders over nothing and Stop has no session to stop.
    mockIPC((cmd) => {
      if (cmd === "start_screen_capture") {
        throw new Error("A recording is already in progress.");
      }
      return undefined;
    });
    const store = useScreenCaptureStore();
    await expect(store.start("v1", "screen:1", [], [])).rejects.toThrow();
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
  });

  it("clears its state on screen:stopped", async () => {
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", vaultId: "v1", sourceTitle: "Screen 1" });
    store.applyStopped({
      base: "2026-09-19 1100 Screen 1",
      path: "C:/staging/2026-09-19 1100 Screen 1.mp4",
      durationMs: 5000,
      sourceTitle: "Screen 1",
      width: 1920,
      height: 1080,
    });
    expect(store.status).toBe("idle");
    expect(store.vaultId).toBeNull();
    expect(store.lastStaged?.durationMs).toBe(5000);
  });

  it("keeps paused time out of the elapsed reading", () => {
    // The capture bar counts from startedAtMs; without subtracting paused
    // time a two-minute pause shows as two minutes of recording that is not
    // in the file.
    const store = useScreenCaptureStore();
    store.$patch({
      status: "paused",
      startedAtMs: 0,
      pausedTotalMs: 30_000,
      pausedSinceMs: 60_000,
    });
    expect(store.elapsedMs(90_000)).toBe(30_000);
  });
});
```

Create `tests/screenSourcePicker.test.ts`:

```ts
import { mount } from "@vue/test-utils";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ScreenSourcePicker from "../src/components/ScreenSourcePicker.vue";

const SOURCES = [
  { id: "screen:1", kind: "screen", title: "Screen 1", detail: "2560x1440 - Primary", width: 2560, height: 1440, isPrimary: true },
  { id: "window:1234", kind: "window", title: "Figma", detail: "Figma.exe", width: 1280, height: 800, isPrimary: false },
];

function mockSources(sources = SOURCES) {
  mockIPC((cmd) => {
    if (cmd === "list_capture_sources") return sources;
    if (cmd === "list_audio_devices") return { inputs: [], outputs: [] };
    return undefined;
  });
}

describe("ScreenSourcePicker", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("lists monitors under Screen and windows under Window", async () => {
    mockSources();
    const w = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
    await vi.waitFor(() => expect(w.text()).toContain("Screen 1"));
    expect(w.text()).toContain("2560x1440");
    expect(w.text()).not.toContain("Figma", "the Window tab is not open yet");
  });

  it("has no Region tab in this phase", () => {
    // Region capture arrives in phase 3. A disabled tab is dead UI that
    // invites a click with nothing behind it.
    mockSources();
    const w = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
    expect(w.text()).not.toContain("Region");
  });

  it("keeps Start disabled until a source is picked", async () => {
    mockSources();
    const w = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
    await vi.waitFor(() => expect(w.text()).toContain("Screen 1"));
    const start = w.get('[data-testid="screen-start"]');
    expect(start.attributes("disabled")).toBeDefined();
  });

  it("permits Start with zero audio devices and says so", async () => {
    // Spec 6.5 and 7.2: a silent capture is a real use case (a UI demo), so
    // the zero-device state is a NOTE, never a block.
    mockSources();
    const w = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
    await vi.waitFor(() => expect(w.text()).toContain("Screen 1"));
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w.text()).toContain("No audio will be recorded");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  it("refuses a start whose source vanished, and refreshes the list", async () => {
    // Spec 7.2 and 14: never a started-then-dead capture. The refusal is
    // inline and the list is re-read, so the user's next click is against
    // reality rather than against the stale list they just failed on.
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        return listed === 1 ? SOURCES : [SOURCES[0]];
      }
      if (cmd === "list_audio_devices") return { inputs: [], outputs: [] };
      if (cmd === "start_screen_capture") {
        throw new Error("the capture source is no longer available");
      }
      return undefined;
    });
    const w = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
    await vi.waitFor(() => expect(w.text()).toContain("Screen 1"));
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await vi.waitFor(() => expect(w.text()).toContain("no longer available"));
    expect(listed).toBeGreaterThan(1, "the source list is re-read after a vanished source");
  });

  it("shows an empty state rather than an error when nothing is capturable", async () => {
    mockSources([]);
    const w = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
    await vi.waitFor(() => expect(w.text()).toContain("No capture sources"));
  });
});
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd /home/user/vault-buddy && npx vitest run tests/screenCaptureStore.test.ts tests/screenSourcePicker.test.ts
```

Expected: module-not-found for both new modules.

- [ ] **Step 3: Implement the store, the types and the view routing**

`src/types.ts` gains the three interfaces listed under **Interfaces** above,
camelCase, matching the Rust DTOs field for field.

`src/stores/screenCapture.ts` mirrors `src/stores/capture.ts`'s shape:
`init()` registers listeners for `screen:started`, `screen:paused`,
`screen:resumed`, `screen:stopped`, `screen:failed`, `screen:warning`,
`screen:frames`, then calls `screen_capture_status` once to resync a
reloaded webview. `elapsedMs(now)` subtracts `pausedTotalMs` and, while
paused, the time since `pausedSinceMs`.

`src/stores/vaults.ts`: add `"screenCapture"` to the `view` union,
`screenCaptureVaultId: null as string | null`, an
`openScreenCapture(vaultId)` action, and clear the id in `showList()`
alongside `recordModeVaultId`.

`src/components/RecordMode.vue`: add the third `OPTIONS` entry, positioned
**after Voice Note and before Import Document** (spec §7.1 — capture actions
first, import next, browse last):

```ts
  { key: "screen", title: "Record Screen", hint: "Screen, window, or region", testId: "mode-screen" },
```

and route it to `store.openScreenCapture(props.vaultId)` rather than to
`start_capture`.

`src/components/ActionPanel.vue`: route the view beside the others:

```vue
        <ScreenSourcePicker
          v-else-if="view === 'screenCapture' && store.screenCaptureVaultId"
          :vault-id="store.screenCaptureVaultId"
        />
```

`src/roots/BuddyRoot.vue` and `src/roots/PanelRoot.vue`: beside the existing
`void capture.init();`, add

```ts
void screenCapture.init();
```

with a comment naming why it is in both — a store mirroring Rust state that
is initialised in only one webview leaves the other with a dead indicator.

- [ ] **Step 4: Implement the two components**

`ScreenSourcePicker.vue` — a `TabGroup` over `Screen | Window`, one row per
source (`data-testid="source-<id>"`), the selected row marked with
`aria-pressed`; `ScreenAudioPicker` below it; a Start button
(`data-testid="screen-start"`) disabled until a source is selected; a
`Banner` for the inline error. On a start rejection it **re-reads
`list_capture_sources`** so the next click is against reality.

`ScreenAudioPicker.vue` — a checklist over `list_audio_devices`' `inputs`
and `outputs`, multi-select, emitting `update:inputs` / `update:outputs`.
Zero selected renders the inline note "No audio will be recorded" and does
**not** disable Start.

Consume the existing UI primitives (`AppButton`, `Chip`, `Banner`,
`EmptyState`, `IconButton`, `SectionHeader`) at their declared prop names,
and the semantic tokens (`text-fg`, `text-fg-muted`, `bg-accent`,
`ring-focus`, `rounded-control`) rather than raw utility strings — see
AGENTS.md § *UI primitives & design tokens*. The tab strip's active state is
one of the documented bespoke cases (`border-violet-400`), so follow
`TabGroup`'s existing treatment rather than forcing `AppButton`.

- [ ] **Step 5: Run the tests and the frontend gates**

```bash
cd /home/user/vault-buddy && npx vitest run tests/screenCaptureStore.test.ts tests/screenSourcePicker.test.ts
cd /home/user/vault-buddy && rm -rf coverage
cd /home/user/vault-buddy && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
```

Run them **in that order**, with no `coverage/` directory present when
`check:quality` runs.

- [ ] **Step 6: Commit**

```bash
cd /home/user/vault-buddy
git add src/stores/screenCapture.ts src/components/ScreenSourcePicker.vue \
  src/components/ScreenAudioPicker.vue src/types.ts src/stores/vaults.ts \
  src/components/RecordMode.vue src/components/ActionPanel.vue \
  src/roots/BuddyRoot.vue src/roots/PanelRoot.vue \
  tests/screenSourcePicker.test.ts tests/screenCaptureStore.test.ts
cat > /tmp/msg.txt <<'EOF'
feat(ui): add the screen capture source picker and its store

Record Screen joins the intake chooser after Voice Note and before Import
Document, preserving the documented ordering: capture actions first, import
next, browse last.

Screen and Window tabs only. Region capture is phase 3, and a disabled
Region tab would be dead UI inviting a click with nothing behind it.

Zero audio devices is a NOTE, never a block: spec 6.5 keeps a silent
capture available because a silent UI demo is a real use case.

A start whose source vanished between enumeration and click re-reads the
source list before showing its inline error, so the user's next click is
against reality rather than against the stale list they just failed on.
That is what keeps a started-then-dead capture impossible.

A refused start leaves the store idle. Believing a capture is running when
none is would render the capture bar over nothing and leave Stop with no
session to stop.

Both BuddyRoot and PanelRoot initialise the store. Each window is its own
webview with its own Pinia instance, and a store mirroring Rust state that
is initialised in only one of them leaves the other with a dead indicator --
the same rule capture.init() already follows.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 10: The capture bar (`ScreenCaptureBar.vue`)

Spec §7.3: elapsed time with paused time excluded, pause/resume, Stop, and
an advisory dropped-frame indicator, rendered on the panel's list view beside
the existing `RecordingBar`.

**Files:**
- Create: `src/components/ScreenCaptureBar.vue`
- Create: `tests/screenCaptureBar.test.ts`
- Modify: `src/components/ActionPanel.vue`

**Interfaces:**
- Consumes: `useScreenCaptureStore` (Task 9).
- Produces: nothing other components consume.

- [ ] **Step 1: Write the failing tests**

Create `tests/screenCaptureBar.test.ts`:

```ts
import { mount } from "@vue/test-utils";
import { mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import ScreenCaptureBar from "../src/components/ScreenCaptureBar.vue";
import { useScreenCaptureStore } from "../src/stores/screenCapture";

describe("ScreenCaptureBar", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("names the source being captured", () => {
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "Figma", startedAtMs: Date.now() });
    const w = mount(ScreenCaptureBar);
    expect(w.text()).toContain("Figma");
  });

  it("excludes paused time from the elapsed reading", async () => {
    // A two-minute pause must not show as two minutes of recording that is
    // not in the file. The clock excludes paused time by construction on the
    // Rust side; the bar must not reintroduce it.
    vi.useFakeTimers();
    vi.setSystemTime(90_000);
    const store = useScreenCaptureStore();
    store.$patch({
      status: "paused",
      sourceTitle: "Screen 1",
      startedAtMs: 0,
      pausedTotalMs: 0,
      pausedSinceMs: 30_000,
    });
    const w = mount(ScreenCaptureBar);
    expect(w.text()).toContain("0:30");
    vi.useRealTimers();
  });

  it("offers Resume while paused and Pause while capturing", async () => {
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.get('[data-testid="screen-pause"]').text()).toContain("Pause");
    store.$patch({ status: "paused", pausedSinceMs: 1 });
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-pause"]').text()).toContain("Resume");
  });

  it("shows the dropped-frame count only when frames have actually dropped", () => {
    // Advisory and lossy by design (spec 11). A permanent "0 dropped" badge
    // is noise; a badge appearing at the moment frames start dropping is the
    // signal spec 17.3 wants visible.
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0, dropped: 0 });
    const w = mount(ScreenCaptureBar);
    expect(w.text()).not.toContain("dropped");
    store.$patch({ dropped: 12 });
    return w.vm.$nextTick().then(() => {
      expect(w.text()).toContain("12");
      expect(w.text()).toContain("dropped");
    });
  });

  it("disables Stop while a stop is already in flight", async () => {
    // Finalize is unbounded; a second Stop during it would send a control
    // message to a session that is already tearing down.
    let stops = 0;
    mockIPC((cmd) => {
      if (cmd === "stop_screen_capture") {
        stops += 1;
        return new Promise(() => {}); // never resolves: the stop is in flight
      }
      return undefined;
    });
    const store = useScreenCaptureStore();
    store.$patch({ status: "capturing", sourceTitle: "S", startedAtMs: 0 });
    const w = mount(ScreenCaptureBar);
    await w.get('[data-testid="screen-stop"]').trigger("click");
    await w.vm.$nextTick();
    expect(w.get('[data-testid="screen-stop"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="screen-stop"]').trigger("click");
    expect(stops).toBe(1);
  });
});
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd /home/user/vault-buddy && npx vitest run tests/screenCaptureBar.test.ts
```

Expected: module-not-found.

- [ ] **Step 3: Implement the bar and render it**

`ScreenCaptureBar.vue` follows `RecordingBar.vue`'s layout: a
`StatusDot` in the `recording` tone, the source title, the elapsed reading
from `store.elapsedMs(Date.now())` through the existing
`utils/formatDuration`, an `IconButton` pause/resume
(`data-testid="screen-pause"`), a Stop button (`data-testid="screen-stop"`)
with its own in-flight guard, and a `Chip` showing dropped frames **only when
`store.dropped > 0`**.

In `ActionPanel.vue`, render it beside `RecordingBar`, on the list view only:

```vue
    <ScreenCaptureBar v-if="view === 'list' && screenCapture.status !== 'idle'" />
```

- [ ] **Step 4: Run the tests and the frontend gates**

```bash
cd /home/user/vault-buddy && npx vitest run tests/screenCaptureBar.test.ts
cd /home/user/vault-buddy && rm -rf coverage
cd /home/user/vault-buddy && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
```

- [ ] **Step 5: Commit**

```bash
cd /home/user/vault-buddy
git add src/components/ScreenCaptureBar.vue src/components/ActionPanel.vue tests/screenCaptureBar.test.ts
cat > /tmp/msg.txt <<'EOF'
feat(ui): add the live screen capture bar

Renders on the panel's list view beside RecordingBar, with the source name,
the elapsed reading, pause/resume and Stop.

Paused time is excluded from the reading. The Rust clock excludes it by
construction so the file contains no paused wall-clock time; showing a
two-minute pause as two minutes of recording would contradict the file.

The dropped-frame count appears only once frames have actually dropped. A
permanent "0 dropped" badge is noise; a badge that appears at the moment
drops begin is the signal the counter exists to make visible.

Stop guards against a second click while a stop is in flight. Finalize is
unbounded, and a second stop during it would signal a session that is
already tearing down.
EOF
git commit -F /tmp/msg.txt
```

---

### Task 11: CI, the Windows verification checklist, docs and baselines

Phase 2's gate is **manual Windows verification** (spec §13). No CI runner
can record a screen, and neither can an agent working from Linux. This task
makes that limit explicit rather than implied, and closes the CI gap Phase 1
deliberately left open for it.

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`
- Modify: `AGENTS.md`, `docs/Gaps.md`, `scripts/loc-baseline.json` (if needed)

- [ ] **Step 1: Close GAP-102 — run the screen crate's tests on Windows**

GAP-102 says, verbatim: *"add `-p vault_buddy_screen` to the `windows-app`
test line in the same PR as Phase 2's `cfg(windows)` split — not before,
since there would be nothing Windows-specific yet to exercise."* That split
has now landed in `sink.rs`, `source.rs` and `session.rs`, so Windows is the
only place that code can execute at all.

In `.github/workflows/ci.yml`, in the `windows-app` job's post-build
`cargo test` step, add the crate:

```yaml
        run: cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --features whisper
```

(Keep the existing flags exactly as they are; only the `-p` list changes.)

- [ ] **Step 2: Retire the spike's CI step**

The `windows-app` job runs `cargo test -p vault_buddy_screen --features
fmp4-spike -- --nocapture --test-threads=1` on every Windows build. That
step answered spec §6.4's question on 2026-09-19 and the answer is recorded
in the spec; re-running it on every build costs Windows-runner minutes and
deliberately aborts child processes to do it.

Delete that step. **Keep `fmp4_spike.rs` and its feature** — it is the
working `MFCreateFMPEG4MediaSink` reference and stays re-runnable on demand
with `cargo test -p vault_buddy_screen --features fmp4-spike -- --nocapture`.
Add a line to the module doc saying the CI step was retired and why, so the
next reader does not conclude the spike was abandoned.

- [ ] **Step 3: Write the Windows verification checklist**

Create `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`.

Spec §15 names the full checklist for the whole increment; this phase can
only cover what Phase 2 built, and the file must **say which items are not
yet reachable** rather than listing them as untested. Structure it as:

- **Covered by Phase 2** — each with the exact steps, and a result column the
  verifier fills in:
  1. Record a monitor with one microphone; play the staged MP4.
  2. Record a window with a microphone **and** a loopback output; confirm
     both are audible and mixed.
  3. Record with **zero** audio devices selected; confirm the capture
     succeeds and the file is silent (spec §6.5).
  4. Pause mid-capture for ~20 s, resume, stop. Confirm the paused stretch is
     **absent** from the file and A/V stay in sync afterwards (spec §6.2/6.3).
  5. Close the recorded window mid-capture. Confirm a warning, a clean
     finalize, and a playable file containing everything up to the close
     (spec §14).
  6. Start an audio recording, then try to start a screen capture, and the
     reverse. Confirm the typed refusal naming the running kind, and that
     neither capture is disturbed (spec §7.3).
  7. Hide to tray mid-capture. Confirm the buddy stays visible.
  8. **Kill the process mid-capture** (Task Manager → End task). Relaunch,
     and confirm the `.mp4.part` still decodes as a prefix — the §6.4
     property, now exercised through the real pipeline rather than the spike.
  9. Leave the screen completely still for ~60 s mid-capture; confirm the
     result plays through the still stretch at a steady rate (the heartbeat).
  10. Record a 4K monitor at 60 fps for two minutes; note the dropped-frame
      indicator (spec §17.3 wants a real measurement, not a guess).

- **NOT reachable in Phase 2**, and which phase each arrives in: region
  capture and mixed-DPI region accuracy (Phase 3), `WDA_EXCLUDEFROMCAPTURE`
  — *so the buddy DOES appear in Phase 2 recordings, expected* (Phase 3),
  editing (Phase 4), export exactness, the untouched fast path, the vault
  write and playback inside Obsidian (Phase 5).

Follow the measurement discipline §6.4's spike earned: **prefer
instrumentation that reports over assertions that confirm.** For item 10,
record the actual dropped count and fps rather than "no drops observed";
for item 8, record the byte count and how much of the file decoded. The
Windows feedback loop is ~10 minutes, and a run that reports values answers
its question on the first attempt where a run that asserts a hoped-for
outcome fails for unrelated reasons.

- [ ] **Step 4: Update AGENTS.md**

Add, in the sections AGENTS.md's own structure calls for:

- **"What compiles where"** — replace the `src-tauri/screen/` row's
  "`engine` is a platform-independent stub" wording with the real state:
  pure submodules (`clock`, `select`, `staging`, `convert`, and `source`'s id
  encoding) test anywhere; `sink`, `source` and `session` are `cfg(windows)`
  with `Unsupported` non-Windows arms; CI gates the crate on Linux **and**
  now runs its tests on Windows.
- **The IPC surface table** — a new `screen_commands.rs` row with the six
  commands and their sync/async markings. Update the command count in the
  table's heading (73 → 79).
- **The events table** — the seven `screen:*` events and that both
  `BuddyRoot` and `PanelRoot` listen.
- **The capture domain** — a short *Screen capture (phase 2)* subsection: the
  three threads and why there are three, the one clock, the fragmented
  container and its measured crash property, the staging directory living
  outside every vault, and the `CaptureGuard` mutual exclusion.
- **The window system** — `tray::hide_buddy` now no-ops for either capture
  kind.
- **Concurrency** — the `CaptureGuard` lock-ordering note: its mutex is never
  held while another lock is taken.
- **Frontend state** — the `screenCapture` view and `screenCaptureVaultId` in
  the view union, and `screenCapture.init()` in both roots.

State plainly in the capture-domain subsection that **Phase 2 writes nothing
into a vault** — the ninth sanctioned vault write arrives in Phase 5.

- [ ] **Step 5: Update docs/Gaps.md**

Mark **GAP-102 resolved** (Step 1 closed it).

Add new entries **starting at GAP-108** — GAP-104 is retired and must not be
reused. Each needs the file reference and failure scenario the file's format
requires. The residuals this phase genuinely leaves:

- The static-screen heartbeat repeats frames at a fixed 500 ms rather than
  adapting, so a long still stretch costs bitrate it need not.
- Audio timestamps derive from the emitted sample count while video derives
  from the wall clock; a sustained sample-rate discrepancy between a device's
  nominal and actual rate would drift them apart over a long capture. No
  measurement exists yet — item 10 of the verification checklist is where one
  comes from.
- `screen-frames` drops rather than blocks when the mux channel is full, so a
  sustained encoder stall silently degrades the frame rate. It is counted and
  surfaced, which is the mitigation, not a fix.
- Phase 2 has no staging recovery: a crashed capture leaves a `.mp4.part`
  that nothing sweeps until Phase 5's `run_screen_recovery`.
- Vault Buddy's own windows appear in a Phase 2 recording
  (`WDA_EXCLUDEFROMCAPTURE` is Phase 3).
- The Windows arm of `sink`/`source`/`session` has no automated execution
  anywhere; `windows-app` now compiles and runs the crate's tests, but those
  tests cover the pure modules. This is the honest coverage statement for the
  phase.

- [ ] **Step 6: Run every gate, and paste the real output**

```bash
cd /home/user/vault-buddy && rm -rf coverage
cd /home/user/vault-buddy && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
cd /home/user/vault-buddy/src-tauri && cargo fmt --check
cd /home/user/vault-buddy/src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd /home/user/vault-buddy/src-tauri && cargo clippy -p vault_buddy_screen -p vault_buddy_capture --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cd /home/user/vault-buddy/src-tauri && cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen
cd /home/user/vault-buddy/src-tauri && cargo test -p vault-buddy --lib
cd /home/user/vault-buddy/src-tauri && cargo machete .
cd /home/user/vault-buddy/src-tauri && cargo deny check
cd /home/user/vault-buddy/src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94
```

If the coverage floor fails, the cause is almost certainly `sink.rs`,
`source.rs` and `session.rs`'s Windows arms being uncoverable on Linux.
**Do not lower the floor.** The correct responses, in order: confirm every
pure module is thoroughly tested; if the floor still fails, the Windows-only
modules need `#[cfg(windows)]`-scoped exclusion from the coverage set, and
that exclusion goes in the PR with a written justification, exactly as the
baseline policy requires.

- [ ] **Step 7: Commit and push**

```bash
cd /home/user/vault-buddy
git add .github/workflows/ci.yml AGENTS.md docs/Gaps.md \
  docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md \
  scripts/loc-baseline.json
cat > /tmp/msg.txt <<'EOF'
ci(screen): run the screen crate's tests on Windows; document phase 2

Closes GAP-102 on the terms that gap set: phase 2's cfg(windows) split has
landed in sink, source and session, so Windows is now the only place that
code can execute and windows-app must run the crate's tests. Before this
split there was nothing Windows-specific to exercise, which is why the gap
asked for the edit in this PR and not earlier.

Retires the fMP4 spike's CI step. It answered spec 6.4's question on
2026-09-19 and the answer is recorded in the spec; re-running it on every
Windows build spends runner minutes deliberately aborting child processes
to re-derive a settled result. The module and its feature stay, re-runnable
on demand, because it is the working MFCreateFMPEG4MediaSink reference.

Adds the Windows verification checklist. Phase 2's gate is manual: no CI
runner can record a screen. The checklist separates what phase 2 can
actually be verified against from what needs later phases, so an untested
item is never mistaken for a passing one -- including that Vault Buddy's own
windows DO appear in a phase 2 recording, since WDA_EXCLUDEFROMCAPTURE is
phase 3.

Records the phase's residuals from GAP-108: GAP-104 is retired and not
reused.
EOF
git commit -F /tmp/msg.txt
git push -u origin claude/screen-capture-intake-g0j49q
```

PR #79 is already open for this branch. **Do not open a second one.**

---

## Phase exit criteria

Phase 2 is complete when **all** of these hold, each verified rather than
assumed:

1. Every task above is committed on `claude/screen-capture-intake-g0j49q`.
2. Every gate in **Global Constraints → Gates** has been **run**, with its
   real output pasted into the completion report. A gate whose output is not
   shown did not pass.
3. CI is green on PR #79's head — all four jobs.
4. The completion report states **explicitly** which behaviour was verified
   by the agent and which was verified only by the user on Windows. The
   entire Windows arm of `sink`/`source`/`session` falls in the second
   category; saying so is the deliverable, not a caveat on it.
5. The Windows verification checklist exists and is honest about what Phase 2
   cannot cover.

**What Phase 2 does NOT deliver, so no one reads its absence as a
regression:** no region capture, no `WDA_EXCLUDEFROMCAPTURE`, no editor, no
export, no vault write, no companion note, no staging recovery, no settings
tab. Those are Phases 3–6.

## What phase 3 needs from this phase

- `source::SourceId` gains a `Region(usize, PhysicalRect)` variant — its
  `parse` already rejects `region:` deliberately, so the refusal is the
  single line that changes.
- `session::ScreenSessionParams` gains an optional crop rectangle, applied
  in the frame thread between `frame.buffer()` and `convert::bgra_to_nv12`
  (`windows-capture`'s `Frame::buffer_crop` may be able to do it on the GPU
  texture — spec §17.2's open question, to be measured, not guessed).
- `core::screen_geometry::{to_physical, clamp_to_frame}` are already built
  and tested (Phase 1); Phase 3 wires them, and re-runs `clamp_to_frame` at
  capture start, because a monitor whose resolution changed between selection
  and start would otherwise index outside the frame buffer.
- The `overlay` window joins `tauri.conf.json`, `rootFor()` and the default
  capability's `windows` array, positioned-while-hidden like `panel` and
  `bubble`, with `DIALOG_ACTIVE` set for its duration so the panel does not
  auto-hide behind it.
