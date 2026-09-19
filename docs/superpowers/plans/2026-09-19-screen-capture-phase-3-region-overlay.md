# Screen Capture — Phase 3 (Region Overlay) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let the user draw a rectangle on a monitor and record exactly that rectangle — correctly at every Windows DPI scale — and make Vault Buddy's own windows invisible to the recording while they stay visible to the user.

**Architecture:** A fifth window (`overlay`, transparent, always-on-top, one monitor) is positioned while hidden over the chosen monitor and then shown; `RegionRoot.vue` paints a scrim and a rubber band and reports the drag as a **logical** (CSS pixel) rectangle, monitor-local because the overlay's own origin is the monitor's origin. Rust multiplies by that monitor's `scale_factor` through the Phase-1 `core::screen_geometry::to_physical`, clamps with `clamp_to_frame`, and encodes the result into the existing source-id string as `region:<display>,<x>,<y>,<w>,<h>` — so `start_screen_capture` needs no new parameter. At capture time the region is re-clamped against the monitor's *current* size and the session crops each delivered frame at an offset, through a new pure `convert::bgra_crop_to_nv12`. Separately, `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` is applied to every app window for the duration of a capture, from one chokepoint, and cleared from one.

**Tech Stack:** Rust 2021 (`vault_buddy_screen`, the Tauri shell crate), `windows-sys` 0.61 (already a shell dependency — one added feature, no new crate), Tauri v2 window/monitor APIs, Vue 3 + Pinia + Tailwind 4.

**Spec:** `docs/superpowers/specs/2026-09-18-screen-capture-intake-design.md` — §5.2 (the `overlay` window and coordinate spaces), §5.3 (`WDA_EXCLUDEFROMCAPTURE`), §7.1 (entry-point copy), §7.2 (the Region tab), §11 (`select_capture_region`), §13 (the Phase 3 row and its gate: *"Region capture works across DPI scales"*), §14 (`sourceGone`), §15 (testing).

**Phase 3's gate:** region capture works across DPI scales. Per the user's standing decision, **manual Windows re-testing is deferred until after the final phase** — Task 8 adds Phase 3's rows to the verification checklist and carries the open Phase-2 rows forward; it does not ask anyone to run them now.

---

## Global Constraints

Every task's requirements implicitly include this section.

### What Phase 1 and Phase 2 already built — do not rebuild it

- **`core::screen_geometry` exists, is fully unit-tested, and has no production caller.** `to_physical(LogicalRect, scale) -> PhysicalRect` and `clamp_to_frame(PhysicalRect, frame_w, frame_h) -> Option<PhysicalRect>` are Phase 1's, and **Phase 3 is their consumer.** Do not write a second DPI conversion, a second clamp, or a second even-rounding rule. `clamp_to_frame` already rounds both dimensions **down to even** (H.264/NV12 4:2:0 needs even width and height — GAP-101) and already returns `None` for an empty intersection.
- Read that module's **module doc** before touching anything geometric. It states the invariant this phase must honour: `to_physical` is correct **only for MONITOR-LOCAL coordinates at that monitor's own scale**. Virtual-desktop-global coordinates (which can be negative, and whose origin belongs to a different monitor at a different DPI) cannot be expressed by its single `scale` parameter. This phase satisfies that by making the overlay cover exactly one monitor, so the webview's own origin **is** the monitor's origin.
- `screen::source::SourceId` already encodes `screen:<n>` and `window:<hwnd>` and **deliberately refuses `region:`** (`source.rs`, the `// "region" deliberately absent` comment). Task 1 is what removes that refusal.
- `screen::convert::bgra_to_nv12` already crops implicitly at the **top-left only** and `session::pacing::usable_frame` already judges a frame purely by size. Task 2 generalises both to an offset.
- Mutual exclusion (`CaptureGuard`), staging, the sink, the session, the six IPC commands and the picker/bar all shipped in Phase 2. Phase 3 adds two commands and one window; it changes no lifecycle.

### Hard rules

- **No vault write in this phase.** The ninth sanctioned vault write is Phase 5's. Staging still lives outside every vault. A task calling `capture_paths::safe_recording_root` against a vault path has gone out of scope.
- **Window show/hide/position/size and every window getter run on the MAIN thread only.** The overlay is positioned **while hidden** and then shown — the same discipline `position_panel` and `show_bubble` follow, for the same stale-frame reason (AGENTS.md, "The window system"). A sync command runs on the main thread and **must never block**; anything that waits belongs in an `async` command on the blocking pool with `run_on_main_thread` used for the window calls.
- **`select_capture_region` must set the existing `DIALOG_ACTIVE` flag for its whole duration and clear it in a `finally`-equivalent**, exactly as `withDialogSuppressed` does for native file pickers. The overlay steals OS focus; without the flag the panel's `schedule_focus_out_check` hides the panel — and the picker's in-progress state — out from under the user.
- **Every spawned thread is named** (`std::thread::Builder::new().name(..)`), per the diagnostics invariant. This phase should need no new thread; if a task finds itself spawning one, name it `region-*`.
- **No swallowed error.** Anything caught-and-hidden goes through `log::warn!` / `log::error!` (Rust) or `src/logging.ts` (frontend).
- **`SetWindowDisplayAffinity` failing is logged and ignored** (spec §5.3): on a pre-2004 Windows 10 build the capture proceeds with the buddy visible in frame. It is never an error the user sees and never blocks a start.
- **One apply site, one clear site, for the capture exclusion** — the `CaptureGuard` discipline applied to a second piece of paired state, pinned by a structural test. A leaked `WDA_EXCLUDEFROMCAPTURE` leaves the user's windows invisible in *other* apps' recordings (Teams, OBS) with no error and no log line, which is exactly the class of silent wedge the guard's own structural test exists to catch.
- **A structural test that scans source must scan PRODUCTION source only.** `include_str!` pulls in the file the test itself lives in, so the test's own literal counts as a match and an `== 1` assertion becomes unreachable. Use the `production_src()` precedent already in `screen/src/sink.rs` and `src-tauri/src/screen_commands.rs` (split at the trailing `#[cfg(test)]`). This has already shipped broken twice on this branch.

### Exact values, copied from the spec and from the verified source

- Overlay window config (spec §5.2), verbatim:
  ```jsonc
  { "label": "overlay", "title": "Vault Buddy — Select Region",
    "visible": false, "transparent": true, "decorations": false,
    "alwaysOnTop": true, "resizable": false, "skipTaskbar": true,
    "focus": true }
  ```
- `WDA_EXCLUDEFROMCAPTURE = 17u32`, `WDA_NONE = 0u32`, and
  `SetWindowDisplayAffinity(hwnd: HWND, dwaffinity: WINDOW_DISPLAY_AFFINITY) -> BOOL`
  all live in `windows_sys::Win32::UI::WindowsAndMessaging`, behind the
  `Win32_UI_WindowsAndMessaging` feature. **Verified in the vendored crate**
  (`windows-sys-0.61.2/src/Windows/Win32/UI/WindowsAndMessaging/mod.rs:411`,
  `:3522`, `:3524`) — no new dependency, one added feature on the
  `[target."cfg(windows)".dependencies] windows-sys` entry the shell already
  has for `GetKeyState`.
- `windows_sys::Win32::Foundation::HWND` is `*mut core::ffi::c_void`
  (verified, same crate, `Foundation/mod.rs:5274`). Tauri's
  `WebviewWindow::hwnd()` returns `windows::Win32::Foundation::HWND`, a tuple
  struct — so the bridge is `handle.0 as windows_sys::Win32::Foundation::HWND`.
- **The two monitor numbering schemes, and the one identity that joins them.**
  This is the trap that shipped a wrong-screen bug in Phase 2 (Task 5), so it
  is written out with its verification:
  - `windows_capture::monitor::Monitor::index()` is
    `device_name.replace("\\\\.\\DISPLAY", "").parse()` — **verified** at
    `windows-capture-2.0.1/src/monitor.rs:163-166`. It is the GDI display
    number, *not* an enumeration position.
  - Tauri's `Monitor::name()` on Windows is `MONITORINFOEXW.szDevice` —
    **verified** at `tao-0.35.3/src/platform_impl/windows/monitor.rs:181-186` —
    i.e. the string `\\.\DISPLAYn`.
  - Therefore the display number parsed out of Tauri's monitor name is
    **the same value** as `Monitor::index()`. That parse is Task 1's
    `region::display_number_from_device_name`, it is pure, and it is the ONLY
    place the two schemes are joined. Do not use `Monitor::from_index`
    (positional — the Phase-2 bug) and do not match monitors by friendly name.
  - `windows_capture::monitor::Monitor::name()` is the **friendly** name (via
    `QueryDisplayConfig`), which is what the picker shows. It is not an
    identifier and must never be matched on.
- Region source id format: `region:<display>,<x>,<y>,<w>,<h>`, all decimal, no
  spaces, `<display>` a `usize`, the rest `u32`. Commas because the existing
  `SourceId::parse` contract rejects a payload containing a second `:`.
- Region selection timeout: **120 s** (`REGION_TIMEOUT`). A webview that
  never resolves must not strand a full-screen invisible always-on-top window.
- Minimum drag before a selection counts as a selection rather than a stray
  click: **8 logical pixels** on either axis (`MIN_DRAG_PX`, frontend).

### Gates each task must pass before its commit

From `src-tauri/`:

```bash
cargo fmt --check
cargo clippy -p <crate> --all-targets -- -D warnings
cargo test -p <crate>
```

For any task touching `#[cfg(windows)]` code, **additionally and mandatorily**:

```bash
cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings
cargo clippy -p vault-buddy --lib --target x86_64-pc-windows-msvc -- -D warnings
```

That target is installed and type-checks real `windows`/`windows-sys`
bindings on Linux. **It is the only local check of the `cfg(windows)` arms,
which execute in no automated test anywhere (docs/Gaps.md GAP-117)**, so it
is not optional. Note the crate-scoped flags: `vault_buddy_capture` needs
`--lib` (its `minimp3-sys` dev-dependency needs MSVC's `lib.exe`, which a
Linux box has not got — see the SDD ledger), and the shell crate needs
`--lib` for the same class of reason. `vault_buddy_screen` takes
`--all-targets`.

For any task touching the frontend or a baseline, from the repo root, **in
this exact order**, with no `coverage/` directory present when
`check:quality` runs:

```bash
rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage && npm run build
```

Expect **exactly one pre-existing lint warning, in `src/main.ts`**. Any
other warning is yours.

**Never pipe a gate through `grep`.** It swallows both the ERROR lines and
the exit code; a `test:coverage` run reported as passing on this branch had
actually failed. Read the output and check `echo "exit=$?"`.

Whole-phase, before declaring the phase done:

```bash
cd src-tauri && cargo machete .
cd src-tauri && cargo deny check
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings
cd src-tauri && cargo test -p vault_buddy_core -p vault_buddy_capture \
  -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen
cd src-tauri && cargo test -p vault-buddy --lib
cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture \
  -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94
```

### Testing rules this phase enforces, because Phase 2 shipped eight bad tests

- **Every test must be mutation-verified.** Before committing, break the
  line the test claims to pin — invert a comparison, delete the guard,
  point-sample instead of averaging — re-run, and confirm the test **FAILS**.
  Then restore byte-identically and confirm it passes again. State the
  mutation and its result in the task report. A test nobody has seen fail is
  not evidence.
- **Never derive an expected value by running the implementation.** Write
  the number by hand from the spec or from arithmetic you did yourself, and
  show the arithmetic in a comment. Phase 2 shipped a sample whose expected
  value contradicted its own test because it was copied out of a run.
- **Vitest matchers take no message argument.** `expect(x).not.toContain("y", "because…")`
  silently ignores the string. Put the reason in the test name or a comment.
- **A test that mounts a component without awaiting its async load asserts
  against an empty render** and passes on anything. Await the load, and
  assert the positive case in the same test as the negative one.
- **`TabGroup` mounts every panel and only `v-show`s the active one**
  (`TabGroup.vue`, its own line-5 comment). `expect(wrapper.text()).not.toContain(…)`
  can therefore never prove a tab's content is absent. Assert on the tab
  list, or on `[data-testid="panel-<id>"]` existence.
- **When a flake is root-caused, grep for sibling tests sharing the
  mechanism** and fix them in the same commit. Phase 2 fixed one of two
  tests with an identical truncation trap; the other flaked a day later.

### Traps that have already cost this work real time

- **The LOC guard covers Rust files**, cap 800 non-blank lines per `.rs`
  under a crate's `src/`, and 500 for `src/**.{ts,vue}`. Current headroom
  that matters to this phase: `src-tauri/src/screen_commands.rs` **717/800**
  and `src-tauri/screen/src/source.rs` **554/800**. `screen_commands.rs`
  cannot absorb the region commands — that is why Task 5 creates a new file.
  `core/src/vault_config.rs` is allowlisted at **1282 with zero headroom**;
  this phase does not need to touch it.
- **Never run `npm run check:loc -- --update`.** It rewrites every `—`
  escape as a literal em dash across all 11 baseline entries, producing a
  20-line diff over unrelated records. Hand-edit the single entry you need
  and write the justification into its `reason` string.
- **Baselines are shrink-only.** Preserve: `quality-baseline.json`
  `averageMaintainability` **91.4** and `complexFunctions` **13**;
  `capture/src/session.rs` at **1058**; coverage floors **95 / 91 / 93 / 96**
  (statements/branches/functions/lines) and the Rust `llvm-cov` floor **94**.
  Prefer extraction over raising — Phase 2 resolved three of its four
  baseline pressures by doing the work instead.
- **Never put backticks in `git commit -m`.** Bash expands them as command
  substitution and has silently deleted words on this branch. Write the
  message to a file and use `git commit -F <file>`.
- **Stage with explicit file lists.** Never `git add -A`.
- **Commit style:** Conventional Commits, imperative subject, body explains
  the *why* and the failure mode being prevented. Scopes in use:
  `feat(screen)`, `fix(screen)`, `feat(shell)`, `feat(ui)`, `docs(...)`,
  `chore(loc)`. Every commit ends with:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
  ```
- **Branch:** `claude/screen-capture-intake-g0j49q`. **PR #79 is already open
  for it. Do not open a second PR, and do not open one against `main`.**
  `main` is red for an unrelated reason; this branch carries the fix.
- **GAP ids:** GAP-123 is the highest in use at plan time. **GAP-104 is
  retired and must never be reused.** Verify with
  `grep -n "^### GAP-" docs/Gaps.md | tail -5` before allocating, and
  allocate upward from whatever that shows.

### What this phase does NOT build

Out of scope, each landing in a named later phase. A task that starts on one
of these has misread the plan.

- **Per-device audio level bars in `ScreenAudioPicker`** — GAP-111 item 1.
  It stays open after Phase 3 and is **not** a frontend change: nothing emits
  a per-device level outside a running audio recording, so the bars would be
  a permanently dead indicator. Restoring them needs a Rust-side emit first.
  Only GAP-111 items **2** (the Region tab) and **3** (the chooser hint) are
  closed here.
- **The `editor` window, asset protocol, CSP, preview, timeline UI** — Phase 4.
- **`reader.rs`, `export.rs`, the vault write, the companion note,
  `run_screen_recovery`, resume-or-discard** — Phase 5.
- **`ScreenCaptureConfigTab.vue`, `set_screen_capture_config`, the staging
  size + clear action, CONTEXT.md's §2 terms, the README entry** — Phase 6.
- **Region selection spanning more than one monitor.** The overlay covers
  exactly one monitor by construction, which is what makes
  `to_physical`'s single scale factor correct. Spec §1 already rules out
  "all screens combined" for the same reason.
- **Cropping on the GPU before readback** (spec §17.2). The crop stays on the
  CPU, in the pure converter, where Linux can test it. Recorded as a residual
  in Task 8.

---

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `src-tauri/screen/src/region.rs` | PURE: the region source's id encoding, its strict parse, and the display-number parse that joins Tauri's monitor names to `windows-capture`'s indices | **Create** |
| `src-tauri/screen/src/lib.rs` | Crate root | Modify: `pub mod region;` |
| `src-tauri/screen/src/source.rs` | Source enumeration, id encode/parse, start-time resolve | Modify: `SourceKind::Region`, `SourceId::Region`, `ResolvedSource.crop_{x,y}`, the `resolve` Region arm |
| `src-tauri/screen/src/convert.rs` | BGRA → NV12 | Modify: add `bgra_crop_to_nv12`; `bgra_to_nv12` becomes its `(0,0)` delegate |
| `src-tauri/screen/src/session/pacing.rs` | Pure frame-plan decisions | Modify: `usable_frame` becomes crop-aware |
| `src-tauri/screen/src/frames.rs` | WGC frame callback | Modify: carry and apply the crop origin |
| `src-tauri/screen/src/session/windows_session.rs` | Session start | Modify: thread the crop from `ResolvedSource` into `FrameFlags` |
| `src-tauri/src/region_commands.rs` | `select_capture_region` / `resolve_region_selection`, the overlay's show-position-await-hide lifecycle, the selection rendezvous | **Create** |
| `src-tauri/src/capture_exclusion.rs` | `WDA_EXCLUDEFROMCAPTURE` apply/clear, one site each | **Create** |
| `src-tauri/src/screen_commands.rs` | Screen IPC | Modify: clear the capture exclusion from `clear_active_screen` |
| `src-tauri/src/screen_capture_worker.rs` | Start path | Modify: apply the capture exclusion once the start is committed |
| `src-tauri/src/lib.rs` | Builder, state, handler registration | Modify: `mod region_commands; mod capture_exclusion;`, manage `RegionSelectionState`, register 2 commands |
| `src-tauri/Cargo.toml` | Shell manifest | Modify: one added `windows-sys` feature |
| `src-tauri/tauri.conf.json` | Window definitions | Modify: the `overlay` window |
| `src-tauri/capabilities/default.json` | Capability scope | Modify: `"overlay"` in `windows` |
| `src/roots/RegionRoot.vue` | The rubber-band selection surface | **Create** |
| `src/roots/index.ts` | Label → root map | Modify: `overlay → RegionRoot` |
| `src/components/ScreenSourcePicker.vue` | Source picker | Modify: the Region tab |
| `src/utils/regionLabel.ts` | PURE: the region row's title/detail strings | **Create** |
| `src/components/RecordMode.vue` | Intake chooser | Modify: the §7.1 hint and aria label |
| `src/types.ts` | Shared DTOs | Modify: `kind` gains `"region"`; `RegionSelection` |
| `tests/regionRoot.test.ts` | RegionRoot behaviour | **Create** |
| `tests/regionLabel.test.ts` | The pure label composer | **Create** |
| `tests/screenSourcePicker.test.ts` | Picker | Modify: Region tab cases |
| `tests/record-mode.test.ts` | Intake chooser | Modify: the region hint assertion |
| `tests/main-root.test.ts` | `rootFor` (this is where its test lives — there is no `roots.test.ts`) | Modify: the `overlay` case |
| `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md` | Manual checklist | Modify: Phase 3 rows + carried-forward Phase 2 rows |
| `AGENTS.md`, `docs/Gaps.md`, `scripts/loc-baseline.json` | Docs + baselines | Modify |

**Task order rationale.** Tasks 1 and 2 are pure, independent of each other
and of Windows. Task 3 consumes both. Task 4 (the window + root) and Task 6
(the exclusion) are independent of everything else. Task 5 consumes Tasks 1
and 4. Task 7 consumes Tasks 1 and 5. Task 8 is docs and baselines and comes
last. **There are exactly eight tasks; run them 1, 2, 3, 4, 5, 6, 7, 8.**
Phase 2's dispatcher skipped its Task 10 because the plan's own order
rationale named a different count than the task list; check off each number
as you dispatch it.

---

### Task 1: The region source id (`screen::region`) — PURE

Spec §5.2 and §11: a region is *a rectangle on one monitor*. It has to cross
the IPC boundary as a string the webview holds and hands back, exactly like
`screen:` and `window:` do, and it has to come back **untrusted**.

**Files:**
- Create: `src-tauri/screen/src/region.rs`
- Modify: `src-tauri/screen/src/lib.rs` (add `pub mod region;` beside `pub mod select;`)
- Modify: `src-tauri/screen/src/source.rs` (`SourceKind`, `SourceId`, `parse`, `Display`, `kind`)
- Test: inline `#[cfg(test)]` in both files

**Interfaces:**
- Consumes: `vault_buddy_core::screen_geometry::PhysicalRect` (Phase 1).
- Produces, for Tasks 3, 5 and 7:
  - `screen::region::RegionSource { pub monitor: usize, pub rect: PhysicalRect }`
    — `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`
  - `screen::region::encode_payload(r: RegionSource) -> String` — the part
    after `region:`, e.g. `"2,320,180,1280,720"`
  - `screen::region::parse_payload(rest: &str) -> Option<RegionSource>`
  - `screen::region::display_number_from_device_name(name: &str) -> Option<usize>`
  - `screen::source::SourceKind::Region` (serialises as `"region"`)
  - `screen::source::SourceId::Region(RegionSource)`; `SourceId::parse` accepts
    `region:…`; `Display` emits it; `kind()` returns `SourceKind::Region`

- [ ] **Step 1: Write the failing tests for `region.rs`**

Create `src-tauri/screen/src/region.rs` with the module doc, the type, and
the tests only (no function bodies yet — write `todo!()` bodies so the file
compiles and the tests fail on the assertion, not on a missing symbol):

```rust
//! The REGION capture source: a rectangle on one monitor, encoded as the
//! string that crosses the IPC boundary.
//!
//! PURE and on both platforms deliberately. Everything here is either
//! parsing untrusted input or joining two monitor-numbering schemes, and
//! both are things no CI runner can check on Windows (docs/Gaps.md
//! GAP-117) — so they live where Linux can pin them.
//!
//! **Why commas.** `source::SourceId::parse` splits on the FIRST `:` and
//! rejects a payload containing a second one, so the five fields of a
//! region cannot be colon-separated without changing that contract for
//! every source kind.
//!
//! **Why a canonical round-trip is the strictness rule.** `"+1"`, `"01"`
//! and `" 1"` are all things `u32::from_str` will or will not accept in
//! ways nobody remembers; requiring `encode_payload(parsed) == input`
//! makes exactly one spelling legal without enumerating the illegal ones.
//! The id is untrusted by the time it comes back — a lenient parse that
//! resolved to *some* rectangle would record something the user never drew.

use vault_buddy_core::screen_geometry::PhysicalRect;

/// A capture region: which monitor, and where on it in PHYSICAL pixels
/// relative to that monitor's own origin (never virtual-desktop
/// coordinates — see `core::screen_geometry`'s module doc for why the
/// distinction is load-bearing).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionSource {
    /// The GDI display number, i.e. the `n` in `\\.\DISPLAYn`. The SAME
    /// value `windows_capture::monitor::Monitor::index()` reports.
    pub monitor: usize,
    pub rect: PhysicalRect,
}

/// The part of the source id after `region:`.
pub fn encode_payload(r: RegionSource) -> String {
    todo!()
}

/// Strict parse of that payload. `None` for anything that is not exactly
/// what `encode_payload` produces.
pub fn parse_payload(rest: &str) -> Option<RegionSource> {
    todo!()
}

/// `\\.\DISPLAY2` -> `2`.
pub fn display_number_from_device_name(name: &str) -> Option<usize> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(monitor: usize, x: u32, y: u32, width: u32, height: u32) -> RegionSource {
        RegionSource {
            monitor,
            rect: PhysicalRect {
                x,
                y,
                width,
                height,
            },
        }
    }

    #[test]
    fn a_region_payload_round_trips() {
        // The expected string is written out by hand, field by field, in
        // the documented order: monitor, x, y, width, height.
        let r = region(2, 320, 180, 1280, 720);
        assert_eq!(encode_payload(r), "2,320,180,1280,720");
        assert_eq!(parse_payload("2,320,180,1280,720"), Some(r));
    }

    #[test]
    fn an_origin_region_round_trips() {
        let r = region(1, 0, 0, 1920, 1080);
        assert_eq!(encode_payload(r), "1,0,0,1920,1080");
        assert_eq!(parse_payload("1,0,0,1920,1080"), Some(r));
    }

    // Untrusted input. A lenient parse would start a capture of a
    // rectangle the user never drew, so every one of these must be None
    // rather than "close enough".
    #[test]
    fn a_malformed_region_payload_is_refused() {
        for bad in [
            "",                    // empty
            "1",                   // one field
            "1,2,3,4",             // four fields
            "1,2,3,4,5,6",         // six fields
            "a,2,3,4,5",           // non-numeric monitor
            "1,-2,3,4,5",          // negative origin
            "1,2,3,4.5,6",         // fractional
            "1,2,3,0,5",           // zero width: unencodable
            "1,2,3,4,0",           // zero height: unencodable
            " 1,2,3,4,5",          // leading space
            "1,2,3,4,5 ",          // trailing space
            "+1,2,3,4,5",          // non-canonical monitor
            "01,2,3,4,5",          // non-canonical monitor
            "1,02,3,4,5",          // non-canonical origin
            "1,2,3,4,5,",          // trailing separator
        ] {
            assert_eq!(parse_payload(bad), None, "must refuse {bad:?}");
        }
    }

    // The join between the two monitor numbering schemes. Getting this
    // wrong is not a cosmetic bug: `Monitor::from_index` is POSITIONAL
    // while `Monitor::index()` is the GDI display number, and crossing
    // them silently records the WRONG SCREEN with no error — which is
    // exactly what phase 2's task 5 shipped before review caught it.
    #[test]
    fn a_gdi_device_name_yields_its_display_number() {
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY1"), Some(1));
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY2"), Some(2));
        // Two digits, so a prefix-only implementation that grabbed one
        // character fails here.
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY12"), Some(12));
    }

    #[test]
    fn a_non_display_device_name_is_refused() {
        // `\\.\DISPLAY1\Monitor0` is a real Windows device string — the
        // MONITOR under a display, not the display. Accepting it by
        // "parsing the leading digits" would map two different devices
        // onto one id.
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY1\Monitor0"), None);
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAY"), None);
        assert_eq!(display_number_from_device_name(r"\\.\DISPLAYx"), None);
        assert_eq!(display_number_from_device_name("DISPLAY1"), None);
        assert_eq!(display_number_from_device_name(""), None);
    }
}
```

- [ ] **Step 2: Run the tests and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen region:: 2>&1 | tail -20
```
Expected: the four `region::tests::*` tests **fail** (panicking in `todo!()`).

- [ ] **Step 3: Implement the three functions**

Replace the three `todo!()` bodies:

```rust
pub fn encode_payload(r: RegionSource) -> String {
    format!(
        "{},{},{},{},{}",
        r.monitor, r.rect.x, r.rect.y, r.rect.width, r.rect.height
    )
}

pub fn parse_payload(rest: &str) -> Option<RegionSource> {
    let mut fields = rest.split(',');
    let monitor = fields.next()?.parse::<usize>().ok()?;
    let x = fields.next()?.parse::<u32>().ok()?;
    let y = fields.next()?.parse::<u32>().ok()?;
    let width = fields.next()?.parse::<u32>().ok()?;
    let height = fields.next()?.parse::<u32>().ok()?;
    if fields.next().is_some() {
        return None;
    }
    // A zero-sized region is not encodable (NV12 has no zero dimension)
    // and `clamp_to_frame` would reject it later anyway — refusing it
    // here means the failure names the id rather than the monitor.
    if width == 0 || height == 0 {
        return None;
    }
    let parsed = RegionSource {
        monitor,
        rect: PhysicalRect {
            x,
            y,
            width,
            height,
        },
    };
    // Canonical spelling only; see the module doc.
    (encode_payload(parsed) == rest).then_some(parsed)
}

pub fn display_number_from_device_name(name: &str) -> Option<usize> {
    let digits = name.strip_prefix(r"\\.\DISPLAY")?;
    // ALL of the remainder must be digits: `\\.\DISPLAY1\Monitor0` is a
    // different device and must not collapse onto display 1.
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<usize>().ok()
}
```

- [ ] **Step 4: Register the module and run the tests**

Add to `src-tauri/screen/src/lib.rs`, in the existing alphabetical-ish
`pub mod` block (after `pub mod mp4_boxes;`):

```rust
// The region source's id encoding and the display-number parse that joins
// Tauri's monitor names to windows-capture's indices. PURE: phase 3's
// correctness lives here, because its cfg(windows) consumer is testable
// nowhere (docs/Gaps.md GAP-117).
pub mod region;
```

```bash
cd src-tauri && cargo test -p vault_buddy_screen region:: 2>&1 | tail -20
```
Expected: `test result: ok. 4 passed`.

- [ ] **Step 5: Mutation-verify each of the four tests**

Run these one at a time; after each, restore the file byte-identically and
re-run to confirm green. Record the outcome in your report.

| Mutation | Must fail |
| --- | --- |
| Swap `r.rect.x` and `r.rect.y` in `encode_payload` | `a_region_payload_round_trips` |
| Delete the `if width == 0 \|\| height == 0` guard | `a_malformed_region_payload_is_refused` |
| Replace the canonical round-trip check with `Some(parsed)` | `a_malformed_region_payload_is_refused` |
| Replace `!digits.bytes().all(..)` with `digits.bytes().next().is_none_or(\|b\| !b.is_ascii_digit())` (i.e. check only the FIRST byte) | `a_non_display_device_name_is_refused` |

If any mutation leaves the suite green, the test is not pinning what it
claims and must be strengthened before you continue.

- [ ] **Step 6: Write the failing `source.rs` tests for the new variant**

Add to `src-tauri/screen/src/source.rs`'s existing `#[cfg(test)] mod tests`:

```rust
    // Phase 3: a region is a first-class source id, so the same untrusted
    // string the webview hands back carries the whole rectangle and
    // `start_screen_capture` needs no new parameter.
    #[test]
    fn a_region_source_id_round_trips() {
        let id = SourceId::Region(crate::region::RegionSource {
            monitor: 2,
            rect: vault_buddy_core::screen_geometry::PhysicalRect {
                x: 320,
                y: 180,
                width: 1280,
                height: 720,
            },
        });
        assert_eq!(id.to_string(), "region:2,320,180,1280,720");
        assert_eq!(SourceId::parse("region:2,320,180,1280,720"), Some(id));
        assert_eq!(id.kind(), SourceKind::Region);
    }

    #[test]
    fn a_malformed_region_source_id_is_refused() {
        // Delegated strictness: whatever region::parse_payload refuses,
        // SourceId::parse must refuse too, rather than falling back to a
        // screen or a default.
        assert_eq!(SourceId::parse("region:"), None);
        assert_eq!(SourceId::parse("region:2,320,180,1280"), None);
        assert_eq!(SourceId::parse("region:2,320,180,1280,0"), None);
    }

    // The existing kinds must be untouched by the new arm.
    #[test]
    fn screen_and_window_ids_still_round_trip_unchanged() {
        assert_eq!(SourceId::parse("screen:3"), Some(SourceId::Screen(3)));
        assert_eq!(SourceId::Screen(3).to_string(), "screen:3");
        assert_eq!(SourceId::parse("window:-42"), Some(SourceId::Window(-42)));
        assert_eq!(SourceId::Window(-42).to_string(), "window:-42");
        assert_eq!(SourceId::parse("region"), None, "no separator");
        assert_eq!(SourceId::parse("nonsense:1"), None);
    }
```

```bash
cd src-tauri && cargo test -p vault_buddy_screen source:: 2>&1 | tail -20
```
Expected: FAIL — `no variant named Region found for enum SourceKind`.

- [ ] **Step 7: Add the variant and its three arms**

In `src-tauri/screen/src/source.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Screen,
    Window,
    Region,
}
```

```rust
pub enum SourceId {
    /// Monitor index as `windows_capture::monitor::Monitor::index` reports it.
    Screen(usize),
    /// `HWND` widened to `isize`. Signed on purpose: an HWND is a pointer,
    /// and on a 64-bit process its high bit can be set.
    Window(isize),
    /// A rectangle on one monitor (phase 3). Carrying the rectangle IN the
    /// id is what lets `start_screen_capture` stay a four-parameter
    /// command: the picker holds one opaque string for every source kind.
    Region(crate::region::RegionSource),
}
```

In `parse`, replace the `// "region" deliberately absent` comment and its
`_ => None` neighbourhood with:

```rust
            "screen" => rest.parse::<usize>().ok().map(SourceId::Screen),
            "window" => rest.parse::<isize>().ok().map(SourceId::Window),
            // Strictness is delegated whole to region::parse_payload, so
            // there is exactly one definition of a legal region id.
            "region" => crate::region::parse_payload(rest).map(SourceId::Region),
            _ => None,
```

In `kind`:

```rust
            SourceId::Region(_) => SourceKind::Region,
```

In `Display`:

```rust
            SourceId::Region(r) => write!(f, "region:{}", crate::region::encode_payload(*r)),
```

**Watch the existing `parse` guard.** It currently rejects a `rest`
containing `:` — leave that exactly as it is. A region payload has commas
only, so the guard still holds for every kind.

- [ ] **Step 8: Run the tests**

```bash
cd src-tauri && cargo test -p vault_buddy_screen 2>&1 | tail -20
```
Expected: all pass. The compiler will also flag any non-exhaustive `match`
on `SourceId` or `SourceKind` elsewhere in the crate — Task 3 owns
`resolve`'s arm, so if `resolve` fails to compile here, add a temporary arm
that returns `Err(ScreenError::Unsupported)` with a `// Task 3 replaces
this` comment and note it in your report.

- [ ] **Step 9: Mutation-verify the `source.rs` tests**

| Mutation | Must fail |
| --- | --- |
| `"region" => Some(SourceId::Screen(0))` | `a_region_source_id_round_trips` and `a_malformed_region_source_id_is_refused` |
| `Display` for `Region` writes `"screen:{}"` | `a_region_source_id_round_trips` |
| `kind()` returns `SourceKind::Screen` for `Region` | `a_region_source_id_round_trips` |

- [ ] **Step 10: Run the gates**

```bash
cd src-tauri && cargo fmt --check; echo "fmt exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings; echo "clippy exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "win clippy exit=$?"
cd src-tauri && cargo test -p vault_buddy_screen; echo "test exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "loc exit=$?"
```
Expected: every `exit=0`. `check:loc` covers Rust files — `source.rs` grows
by roughly 40 lines from 554, well under the 800 cap.

- [ ] **Step 11: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t1.txt <<'MSG'
feat(screen): encode a capture region as a first-class source id

A region is a rectangle on one monitor, and it has to survive a round trip
through the webview like every other capture source. Encoding it into the
existing source-id string (region:<display>,<x>,<y>,<w>,<h>) keeps
start_screen_capture a four-parameter command: the picker holds one opaque
string whatever kind of source it is.

The id comes back UNTRUSTED, so the parse is strict by canonical round trip
rather than by enumerating illegal spellings -- "+1", "01" and " 1" all do
different things in u32::from_str and a lenient parse would record a
rectangle the user never drew.

display_number_from_device_name is the one place Tauri's monitor naming
(MONITORINFOEXW.szDevice, i.e. \\.\DISPLAYn) is joined to
windows-capture's Monitor::index (the same number, parsed from the same
device name). Phase 2 shipped a wrong-screen bug by crossing a positional
index with a display number; this keeps the join pure, single-sourced and
tested on Linux, where the cfg(windows) consumer can never be.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/screen/src/region.rs src-tauri/screen/src/lib.rs src-tauri/screen/src/source.rs
git commit -F /tmp/msg-t1.txt
```

---

### Task 2: Offset cropping in the converter and the pacer — PURE

Today a screen capture crops implicitly at the **top-left**:
`convert::bgra_to_nv12` reads `width * 4` bytes out of each of the first
`height` rows, and `pacing::usable_frame` judges a frame by size alone. A
region is a rectangle **at an offset**, and it is captured by recording the
whole monitor and cropping — so both of those need an origin. Both are pure
and therefore the only part of region capture that any CI runner can prove.

**Files:**
- Modify: `src-tauri/screen/src/convert.rs`
- Modify: `src-tauri/screen/src/session/pacing.rs:197`
- Modify: `src-tauri/screen/src/session/mod.rs:531-547` (the existing `usable_frame` tests)
- Test: inline `#[cfg(test)]` in both

**Interfaces:**
- Consumes: nothing from Task 1.
- Produces, for Task 3:
  - `convert::bgra_crop_to_nv12(bgra: &[u8], stride: usize, src_x: u32, src_y: u32, width: u32, height: u32, out: &mut Vec<u8>) -> Result<(), ConvertError>`
  - `convert::bgra_to_nv12(bgra, stride, width, height, out)` — unchanged
    signature, now a delegate at `(0, 0)`
  - `pacing::usable_frame(got_w: u32, got_h: u32, crop_x: u32, crop_y: u32, want_w: u32, want_h: u32) -> bool`

- [ ] **Step 1: Write the failing crop tests**

Add to `src-tauri/screen/src/convert.rs`'s `#[cfg(test)] mod tests`:

```rust
    /// A 4x4 BGRA frame whose four 2x2 quadrants are four distinct
    /// luminances, packed (stride = 16). Every expected Y below is derived
    /// BY HAND from `luma709` = `16 + Y*(219/255)` rounded, never by
    /// running the converter:
    ///   top-left     black  (0,0,0)       -> Y=0    -> 16
    ///   top-right    white  (255,255,255) -> Y=255  -> 16 + 219      = 235
    ///   bottom-left  grey   (128,128,128) -> Y=128  -> 16 + 109.929  = 126
    ///   bottom-right blue   (b=255,g=0,r=0) -> Y=18.411 -> 16 + 15.812 = 32
    /// The four differ enough that reading the wrong quadrant can never
    /// coincidentally produce the right answer.
    #[rustfmt::skip]
    fn four_quadrants() -> Vec<u8> {
        vec![
            // row 0: black, black, white, white          (b, g, r, a)
            0,0,0,255,      0,0,0,255,      255,255,255,255, 255,255,255,255,
            // row 1: black, black, white, white
            0,0,0,255,      0,0,0,255,      255,255,255,255, 255,255,255,255,
            // row 2: grey, grey, blue, blue
            128,128,128,255, 128,128,128,255, 255,0,0,255,    255,0,0,255,
            // row 3: grey, grey, blue, blue
            128,128,128,255, 128,128,128,255, 255,0,0,255,    255,0,0,255,
        ]
    }

    // THE region-capture test. A crop that ignores its origin reads the
    // top-left quadrant (16); one that swaps x and y reads the opposite
    // off-diagonal quadrant; one that negates the offset reads the
    // top-left again. All three mutations produce a different constant
    // from the correct one, in every direction.
    #[test]
    fn a_crop_reads_the_quadrant_its_origin_names() {
        let src = four_quadrants();
        let mut out = Vec::new();

        bgra_crop_to_nv12(&src, 16, 2, 0, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[235, 235, 235, 235], "top-right is white");

        bgra_crop_to_nv12(&src, 16, 0, 2, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[126, 126, 126, 126], "bottom-left is grey");

        bgra_crop_to_nv12(&src, 16, 2, 2, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[32, 32, 32, 32], "bottom-right is blue");

        bgra_crop_to_nv12(&src, 16, 0, 0, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[16, 16, 16, 16], "top-left is black");
    }

    // A crop is not required to start on an even pixel, and that is
    // deliberate rather than an oversight: NV12's 2x2 chroma blocks are
    // computed from the FULL-COLOUR BGRA source inside the crop, so an odd
    // origin produces a self-consistent frame. (Slicing an existing NV12
    // buffer at an odd offset would not — do not "fix" this by rounding
    // the origin, which would silently move the region the user drew.)
    #[test]
    fn an_odd_crop_origin_is_allowed_and_reads_the_right_pixels() {
        let src = four_quadrants();
        let mut out = Vec::new();
        // x=1,y=1: one pixel of each quadrant. Row 0 of the crop is
        // (1,1)=black and (2,1)=white; row 1 is (1,2)=grey and (2,2)=blue.
        bgra_crop_to_nv12(&src, 16, 1, 1, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[16, 235, 126, 32]);
    }

    // The whole point of keeping one implementation: the no-crop entry
    // point must still be byte-for-byte what it was, or every frame of
    // every non-region capture changes.
    #[test]
    fn the_uncropped_entry_point_equals_a_zero_origin_crop() {
        let src = four_quadrants();
        let mut via_plain = Vec::new();
        let mut via_crop = Vec::new();
        bgra_to_nv12(&src, 16, 4, 4, &mut via_plain).unwrap();
        bgra_crop_to_nv12(&src, 16, 0, 0, 4, 4, &mut via_crop).unwrap();
        assert_eq!(via_plain, via_crop);
        // And it is not vacuously equal because both are empty.
        assert_eq!(via_plain.len(), nv12_len(4, 4));
    }

    // A crop whose RIGHT edge runs past the row is a read of the next
    // row's pixels, which shears the image rather than overrunning the
    // buffer -- so the length check alone cannot catch it. Same class as
    // the existing narrow-stride test, one origin to the right.
    #[test]
    fn a_crop_running_past_the_row_is_refused() {
        let src = four_quadrants();
        let mut out = Vec::new();
        // stride 16 holds 4 pixels; a 4-wide crop at x=2 needs 6.
        assert!(matches!(
            bgra_crop_to_nv12(&src, 16, 2, 0, 4, 2, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    // A crop whose BOTTOM edge runs past the frame overruns the buffer.
    #[test]
    fn a_crop_running_past_the_last_row_is_refused() {
        let src = four_quadrants(); // 4 rows
        let mut out = Vec::new();
        assert!(matches!(
            bgra_crop_to_nv12(&src, 16, 0, 2, 2, 4, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    // Odd DIMENSIONS stay refused whatever the origin: NV12 has no way to
    // express a half chroma sample.
    #[test]
    fn a_crop_with_odd_dimensions_is_refused() {
        let src = four_quadrants();
        let mut out = Vec::new();
        assert!(matches!(
            bgra_crop_to_nv12(&src, 16, 1, 1, 3, 2, &mut out),
            Err(ConvertError::OddDimensions)
        ));
    }
```

- [ ] **Step 2: Run them and watch them fail**

```bash
cd src-tauri && cargo test -p vault_buddy_screen convert:: 2>&1 | tail -20
```
Expected: FAIL — `cannot find function bgra_crop_to_nv12`.

- [ ] **Step 3: Implement the crop**

In `src-tauri/screen/src/convert.rs`, replace `bgra_to_nv12`'s body with a
delegation and add the real function. Keep the existing doc comment on
`bgra_to_nv12` and extend it.

```rust
/// Convert one BGRA frame into `out`, reusing its allocation.
///
/// `stride` is the source's BYTES PER ROW, which for a D3D11 staging texture
/// is usually wider than `width * 4`. Reading the buffer as tightly packed
/// shears the image progressively down the frame -- a bug that presents as a
/// capture glitch rather than as a stride mistake.
///
/// This is the whole-frame (top-left) case of `bgra_crop_to_nv12`, kept as
/// its own entry point because every non-region capture uses it and its
/// behaviour must not drift: it delegates rather than duplicating, so there
/// is one conversion, not two that can diverge.
pub fn bgra_to_nv12(
    bgra: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), ConvertError> {
    bgra_crop_to_nv12(bgra, stride, 0, 0, width, height, out)
}

/// Convert the `width` x `height` rectangle at `(src_x, src_y)` of a BGRA
/// frame into NV12 in `out`.
///
/// This is how REGION capture works (spec 5.2): WGC hands us the whole
/// monitor and the region is a window onto it, so the crop happens here,
/// on the CPU, in a pure function -- which is the only reason any of
/// region capture's correctness is provable on Linux (docs/Gaps.md
/// GAP-117). Cropping on the GPU before readback would be cheaper at 4K60
/// (spec 17.2) and is deliberately not done: it would move this logic
/// somewhere nothing can test it.
///
/// `src_x` / `src_y` are NOT required to be even. The 2x2 chroma blocks are
/// averaged from the full-colour BGRA source *inside* the crop, so an odd
/// origin still yields a self-consistent NV12 frame; rounding the origin to
/// even would silently move the rectangle the user drew.
pub fn bgra_crop_to_nv12(
    bgra: &[u8],
    stride: usize,
    src_x: u32,
    src_y: u32,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), ConvertError> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(ConvertError::OddDimensions);
    }
    // The crop's RIGHT edge must lie inside one row. A row that ends short
    // of it does not overrun the buffer until the last row -- every
    // earlier row's furthest read lands inside the following row's bytes,
    // so a length check alone stays satisfied right up to the end and then
    // panics, or (worse, and invisibly) reads the next row's pixels and
    // shears the image. u64 throughout: `src_x + width` is attacker-
    // adjacent arithmetic and must not wrap.
    let right_edge_bytes = (u64::from(src_x) + u64::from(width)) * 4;
    if right_edge_bytes > stride as u64 {
        return Err(ConvertError::ShortInput {
            needed: right_edge_bytes as usize,
            got: stride,
        });
    }
    // Conservative on purpose: a mapped D3D11 staging texture is always
    // `stride * rows` bytes, so requiring the whole final row costs
    // nothing real and keeps the zero-origin case byte-for-byte the check
    // it has always been.
    let needed = (u64::from(src_y) + u64::from(height)) * stride as u64;
    if (bgra.len() as u64) < needed {
        return Err(ConvertError::ShortInput {
            needed: needed as usize,
            got: bgra.len(),
        });
    }

    let (w, h) = (width as usize, height as usize);
    let (ox, oy) = (src_x as usize, src_y as usize);
    let y_len = w * h;
    // resize() keeps the existing allocation when the length is unchanged,
    // which is the whole point: at 60 fps and 4K a per-frame allocation is
    // megabytes per second of churn in the hot path.
    out.resize(nv12_len(width, height), 0);
    let (y_plane, uv_plane) = out.split_at_mut(y_len);

    for row in 0..h {
        let src_row = (oy + row) * stride + ox * 4;
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
                    let i = (oy + by * 2 + dy) * stride + (ox + bx * 2 + dx) * 4;
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

The chroma block above is the EXISTING loop with `oy +` and `ox +` added to
its one index expression, and nothing else. **Do not rewrite the averaging
arithmetic:** a four-distinct-colour test already pins it
(`chroma_is_the_2x2_average…`, which a point-sampling regression fails),
and changing it here would be an unrelated regression riding a crop change.
Note that `o`, the DESTINATION index, takes no origin — the output plane
starts at its own zero.

- [ ] **Step 4: Run the whole convert suite**

```bash
cd src-tauri && cargo test -p vault_buddy_screen convert:: 2>&1 | tail -25
```
Expected: every new test passes **and every pre-existing convert test still
passes** — those are now the regression proof that the no-crop path is
unchanged. If `a_short_input_is_refused_rather_than_read_past_the_end` or
`a_narrow_stride_is_refused_rather_than_panicking_on_the_last_row` fails,
the delegation changed behaviour and the check is wrong, not the test.

- [ ] **Step 5: Make `usable_frame` crop-aware**

First update the four existing call sites in
`src-tauri/screen/src/session/mod.rs` and add the region cases:

```rust
    #[test]
    fn a_frame_at_or_above_the_declared_size_is_kept_and_cropped() {
        // A window can be resized mid-capture and WGC then delivers a
        // different size; the output format is fixed at start, so a
        // bigger frame crops to the declared size for free.
        assert!(usable_frame(1920, 1080, 0, 0, 1920, 1080));
        assert!(usable_frame(2560, 1440, 0, 0, 1920, 1080));
    }

    #[test]
    fn an_undersized_frame_is_rejected() {
        // Reading the declared height out of a shorter frame runs past the
        // end of the mapped staging texture.
        assert!(!usable_frame(1920, 1079, 0, 0, 1920, 1080));
        assert!(!usable_frame(1919, 1080, 0, 0, 1920, 1080));
    }

    // REGION capture: the frame is the whole monitor and the output is a
    // rectangle inside it, so "big enough" is measured from the crop's FAR
    // edge, not from the output size. A monitor that drops to 1280x720
    // while a 640x480 region at (1600, 900) is being recorded delivers a
    // frame that is larger than the OUTPUT and still unusable.
    #[test]
    fn a_region_is_judged_by_its_far_edge_not_its_size() {
        assert!(usable_frame(1920, 1080, 1280, 600, 640, 480));
        assert!(!usable_frame(1919, 1080, 1280, 600, 640, 480));
        assert!(!usable_frame(1920, 1079, 1280, 600, 640, 480));
        assert!(
            !usable_frame(1280, 720, 1600, 900, 640, 480),
            "the frame is bigger than the output but the region is off it"
        );
    }

    // The offsets arrive from a parsed id. Adding them in u32 would wrap
    // and turn an impossible region into a usable one.
    #[test]
    fn an_overflowing_crop_offset_is_rejected_rather_than_wrapping() {
        assert!(!usable_frame(1920, 1080, u32::MAX, 0, 2, 1080));
        assert!(!usable_frame(1920, 1080, 0, u32::MAX, 1920, 2));
    }
```

Then in `src-tauri/screen/src/session/pacing.rs`:

```rust
/// Is this delivered frame big enough to produce the output we declared?
///
/// The output format is fixed at start, but a WINDOW can be resized
/// mid-capture and WGC then delivers a different size, and a REGION is a
/// rectangle inside a frame that is bigger than it. A frame whose far edge
/// covers the crop can be cropped for free -- `bgra_crop_to_nv12` reads
/// `width * 4` bytes out of `height` rows starting at `(crop_x, crop_y)` of
/// a stride-pitched buffer. Anything smaller has to be dropped and counted:
/// reading past the crop runs off the end of the mapped staging texture,
/// and row padding can make a buffer-length check pass while the dimensions
/// do not.
///
/// u64 arithmetic because `crop_x` and `crop_y` come from a parsed source
/// id: in u32, `crop_x + want_w` can wrap and call an impossible region
/// usable.
pub fn usable_frame(
    got_w: u32,
    got_h: u32,
    crop_x: u32,
    crop_y: u32,
    want_w: u32,
    want_h: u32,
) -> bool {
    u64::from(got_w) >= u64::from(crop_x) + u64::from(want_w)
        && u64::from(got_h) >= u64::from(crop_y) + u64::from(want_h)
}
```

`frames.rs:90`'s call site does not compile until Task 3 threads the origin
through. For **this** task, pass literal zeros there with a
`// Task 3 replaces these with the session's crop origin` comment, so the
crate builds and this task's tests run in isolation.

- [ ] **Step 6: Run the tests**

```bash
cd src-tauri && cargo test -p vault_buddy_screen 2>&1 | tail -20
```
Expected: all pass.

- [ ] **Step 7: Mutation-verify every new test**

One at a time; restore byte-identically and re-confirm green after each.

| Mutation | Must fail |
| --- | --- |
| `let src_row = row * stride + ox * 4;` (drop `oy`) | `a_crop_reads_the_quadrant_its_origin_names` |
| `let src_row = (oy + row) * stride;` (drop `ox`) | `a_crop_reads_the_quadrant_its_origin_names` |
| Swap `ox` and `oy` in `src_row` | `a_crop_reads_the_quadrant_its_origin_names` |
| `let src_row = (ox + row) * stride + oy * 4;` | `an_odd_crop_origin_is_allowed_and_reads_the_right_pixels` |
| Drop the `right_edge_bytes` guard | `a_crop_running_past_the_row_is_refused` |
| `let needed = u64::from(height) * stride as u64;` (drop `src_y`) | `a_crop_running_past_the_last_row_is_refused` |
| `usable_frame` ignores `crop_x`/`crop_y` (revert to the size-only form) | `a_region_is_judged_by_its_far_edge_not_its_size` |
| `usable_frame` adds in `u32` (`got_w >= crop_x + want_w`) | `an_overflowing_crop_offset_is_rejected_rather_than_wrapping` (it will also overflow-panic in debug, which counts as a failure) |
| Make `bgra_to_nv12` its own copy of the loop with `ox`/`oy` hard-zeroed | `the_uncropped_entry_point_equals_a_zero_origin_crop` must still pass, so this one is a NEGATIVE check: note in your report that this test does **not** pin the delegation and rely on the others |

The last row is deliberate: if a mutation you expect to be caught is not,
say so in the report rather than quietly strengthening nothing.

- [ ] **Step 8: Run the gates**

```bash
cd src-tauri && cargo fmt --check; echo "fmt exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings; echo "clippy exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "win clippy exit=$?"
cd src-tauri && cargo test -p vault_buddy_screen; echo "test exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "loc exit=$?"
```
Expected: every `exit=0`. Watch for clippy's `too_many_arguments` (its
threshold is 7 and `bgra_crop_to_nv12` has exactly 7) — if it fires,
**do not** add an `#[allow]`; group the origin into a small
`Crop { x: u32, y: u32 }` struct in `convert.rs` and update the call sites
and this plan's Task 3 signature accordingly, noting the change in your
report.

- [ ] **Step 9: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t2.txt <<'MSG'
feat(screen): crop a converted frame at an offset, not only at the top left

Region capture records the whole monitor and keeps a rectangle of it, so
the converter needs an origin and the frame-usability rule needs to measure
from the crop's far edge rather than from the output size. A 640x480 region
at (1600, 900) is unusable on a 1280x720 frame even though the frame is
bigger than the output -- the size-only check called that frame fine and
would have read off the end of the mapped staging texture.

bgra_to_nv12 keeps its signature and delegates at (0, 0), so every
non-region capture goes through the identical code path it always did and
there is one conversion rather than two that can drift apart.

The crop origin is deliberately NOT rounded to even. NV12's 2x2 chroma
blocks are averaged from the full-colour BGRA source inside the crop, so an
odd origin is self-consistent; rounding it would silently move the
rectangle the user drew.

Both changes are pure and unit-tested on Linux, which is the only place any
of region capture's correctness can be proven -- the cfg(windows) arms that
consume them execute in no automated test anywhere (docs/Gaps.md GAP-117).

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/screen/src/convert.rs src-tauri/screen/src/session/pacing.rs src-tauri/screen/src/session/mod.rs src-tauri/screen/src/frames.rs
git commit -F /tmp/msg-t2.txt
```

---

### Task 3: Resolve a region and crop the session's frames — `cfg(windows)`

Tasks 1 and 2 built the id and the crop. This wires them to the engine:
`source::resolve` learns the Region arm, `ResolvedSource` learns a crop
origin, and the session carries it to the frame callback.

**This task is almost entirely `cfg(windows)` code that executes in no
automated test anywhere (docs/Gaps.md GAP-117).** The Windows-target clippy
run is therefore mandatory, not optional, and the arm you write must contain
**no logic** beyond: look the monitor up, read its current size, call
`clamp_to_frame`, build the struct. Every decision it makes is already
tested — the id parse in `screen::region`, the clamp and the even-rounding
in `core::screen_geometry`, the crop in `screen::convert`, the usability
rule in `session::pacing`. If you find yourself writing an `if` that is not
one of those, it belongs in a pure module instead.

**Files:**
- Modify: `src-tauri/screen/src/source.rs` (`ResolvedSource`, the `resolve` Region arm, the non-Windows stub)
- Modify: `src-tauri/screen/src/frames.rs` (`FrameFlags`, `on_frame_arrived`)
- Modify: `src-tauri/screen/src/session/windows_session.rs:50-51, 60-75, 212-220`
- Test: inline `#[cfg(test)]` in `source.rs`

**Interfaces:**
- Consumes: `region::RegionSource`, `SourceId::Region` (Task 1);
  `convert::bgra_crop_to_nv12`, the six-argument `pacing::usable_frame`
  (Task 2); `vault_buddy_core::screen_geometry::clamp_to_frame` (Phase 1).
- Produces, for Task 5 (which needs none of it at runtime but must not
  contradict it): `ResolvedSource` gains `pub crop_x: u32, pub crop_y: u32`,
  zero for a whole screen or window.

- [ ] **Step 1: Write the failing test for the struct's contract**

There is nothing Linux can call in `resolve`, so this test pins the one
thing it can: that the whole-source arms leave the crop at the origin, as a
compile-time-and-value contract other code depends on. Add to
`src-tauri/screen/src/source.rs`'s test module:

```rust
    // A whole screen or window is the (0, 0) case of a region, and the
    // rest of the pipeline relies on that: `pacing::usable_frame` measures
    // from `crop + want`, so a non-zero default here would reject every
    // frame of every non-region capture. Constructible off Windows on
    // purpose, so the contract is pinned somewhere CI runs.
    #[test]
    fn a_whole_source_crops_at_the_origin() {
        let whole = uncropped_dims(1920, 1080);
        assert_eq!(whole, (0, 0, 1920, 1080));
    }
```

and, beside `capture_size_from_bounds` in the PURE part of the file:

```rust
/// The `(crop_x, crop_y, width, height)` a WHOLE source contributes: the
/// full frame, cropped at the origin.
///
/// Trivial, and pinned by a test anyway, because the rest of the pipeline
/// depends on the zero: `pacing::usable_frame` measures from
/// `crop + want`, so a non-zero origin here would reject every frame of
/// every non-region capture with nothing but a rising drop counter to say
/// why. PURE and on both platforms for the same reason
/// `capture_size_from_bounds` is — the `cfg(windows)` arm that calls it
/// executes in no automated test anywhere (docs/Gaps.md GAP-117).
pub fn uncropped_dims(width: u32, height: u32) -> (u32, u32, u32, u32) {
    (0, 0, width, height)
}
```

```bash
cd src-tauri && cargo test -p vault_buddy_screen source:: 2>&1 | tail -10
```
Expected: FAIL — `cannot find function uncropped_dims`. Then add the
function and re-run; expected PASS.

- [ ] **Step 2: Widen `ResolvedSource`**

In `src-tauri/screen/src/source.rs`:

```rust
/// A source re-checked at START time and ready to capture.
pub struct ResolvedSource {
    pub handle: SourceHandle,
    /// The OUTPUT size. For a region this is the region's size, NOT the
    /// monitor's — the monitor is what WGC delivers, the region is what
    /// gets encoded.
    pub width: u32,
    pub height: u32,
    /// Where in each delivered frame the output starts. `(0, 0)` for a
    /// whole screen or window (`uncropped_dims`).
    pub crop_x: u32,
    pub crop_y: u32,
    pub title: String,
}
```

Update the two existing `ResolvedSource { .. }` literals in the
`cfg(windows)` `resolve` (the Screen and Window arms) to build their
dimensions through the helper, so the zero has one origin:

```rust
                let (crop_x, crop_y, width, height) = uncropped_dims(width, height);
                Ok(ResolvedSource {
                    handle: SourceHandle::Screen(monitor),
                    width,
                    height,
                    crop_x,
                    crop_y,
                    title,
                })
```

(and the identical shape in the `Window` arm, with `SourceHandle::Window`.)

- [ ] **Step 3: Write the Region arm of `resolve`**

Add to the `cfg(windows)` `resolve`'s `match`, after the `Window` arm:

```rust
            // REGION (spec 5.2): the same monitor lookup as `Screen`
            // above — scanning for the monitor whose `.index()` matches,
            // never `Monitor::from_index`, which is POSITIONAL and
            // silently resolves to a different live screen (the phase-2
            // wrong-screen bug) — and then one clamp.
            //
            // `clamp_to_frame` runs HERE, at start, and not only when the
            // user drew the rectangle, because the monitor's resolution
            // can change in between (spec 5.2). An unclamped stale
            // rectangle indexes outside the frame buffer. `None` means the
            // region no longer intersects the monitor at all, which is
            // exactly `SourceGone`: the thing the user picked is not there
            // any more.
            //
            // Nothing else happens in this arm on purpose — the id parse,
            // the clamp, the even-rounding and the crop are each tested in
            // a pure module, and logic added here would be logic nothing
            // can reach (docs/Gaps.md GAP-117).
            SourceId::Region(region) => {
                let monitors = Monitor::enumerate().map_err(|e| {
                    log::warn!("screen source: monitor enumeration failed: {e}");
                    ScreenError::SourceGone
                })?;
                let monitor = monitors
                    .into_iter()
                    .find(|m| match m.index() {
                        Ok(i) => i == region.monitor,
                        Err(_) => false,
                    })
                    .ok_or_else(|| {
                        log::warn!("screen source: monitor {} is gone", region.monitor);
                        ScreenError::SourceGone
                    })?;
                let (Ok(mon_w), Ok(mon_h)) = (monitor.width(), monitor.height()) else {
                    return Err(ScreenError::SourceGone);
                };
                let Some(rect) =
                    vault_buddy_core::screen_geometry::clamp_to_frame(region.rect, mon_w, mon_h)
                else {
                    log::warn!(
                        "screen source: the selected region no longer fits monitor {} ({mon_w}x{mon_h})",
                        region.monitor
                    );
                    return Err(ScreenError::SourceGone);
                };
                let label = monitor
                    .name()
                    .unwrap_or_else(|_| format!("Screen {}", region.monitor));
                Ok(ResolvedSource {
                    handle: SourceHandle::Screen(monitor),
                    width: rect.width,
                    height: rect.height,
                    crop_x: rect.x,
                    crop_y: rect.y,
                    // Must read the same as the picker's own row, which
                    // `src/utils/regionLabel.ts` composes (Task 7). Two
                    // spellings of the same source is a support problem,
                    // not a cosmetic one.
                    title: format!("Region on {label}"),
                })
            }
```

- [ ] **Step 4: Carry the crop to the frame callback**

`src-tauri/screen/src/frames.rs` — widen `FrameFlags` and use it:

```rust
pub(crate) struct FrameFlags {
    pub clock: SharedClock,
    pub tx: SyncSender<MuxMsg>,
    /// Where in each delivered frame the output starts. `(0, 0)` for a
    /// whole screen or window; a region's origin inside its monitor.
    pub crop_x: u32,
    pub crop_y: u32,
    /// The size the sink was opened with — already rounded to even by
    /// `convert::even_dims`. A resized window's frames are judged against
    /// this, never the other way round: the output format is fixed at start.
    pub width: u32,
    pub height: u32,
    pub counters: Arc<Counters>,
    pub stopping: Arc<AtomicBool>,
    pub warnings: Arc<Warnings>,
}
```

In `on_frame_arrived`, the usability check becomes:

```rust
        if !pacing::usable_frame(
            frame.width(),
            frame.height(),
            self.flags.crop_x,
            self.flags.crop_y,
            self.flags.width,
            self.flags.height,
        ) {
```

and the conversion becomes:

```rust
        let stride = buffer.row_pitch() as usize;
        let (crop_x, crop_y) = (self.flags.crop_x, self.flags.crop_y);
        let (width, height) = (self.flags.width, self.flags.height);
        let bytes = buffer.as_raw_buffer();

        if let Err(e) =
            convert::bgra_crop_to_nv12(bytes, stride, crop_x, crop_y, width, height, &mut self.nv12)
        {
            self.drop_frame(&e.to_string());
            return Ok(());
        }
```

**Leave `diagnose::undersized_drop_reason` reporting the frame size against
the OUTPUT size** as it does today. A region drop's real cause is the
monitor shrinking, and the two numbers in that line are still the ones that
tell a wrongly-declared size from a resized source apart. Do not widen its
signature in this task.

- [ ] **Step 5: Carry the crop through the session**

`src-tauri/screen/src/session/windows_session.rs`:

1. Add to the `ScreenSession` struct, beside `width` / `height`:
   ```rust
       crop_x: u32,
       crop_y: u32,
   ```
2. In `start`, after the existing `even_dims` call:
   ```rust
           let (width, height) = convert::even_dims(source.width, source.height);
           // The crop origin is NOT rounded — `even_dims` shrinks a size to
           // something NV12 can express, which is about the output, while the
           // origin is about where in the source that output starts.
           // `clamp_to_frame` has already made a region's size even, so this
           // is a no-op for regions and only ever trims a whole screen or
           // window (whose origin is 0 either way).
           let (crop_x, crop_y) = (source.crop_x, source.crop_y);
   ```
   and set both on the constructed `ScreenSession`.
3. At the `FrameFlags` literal (`~line 212`), add:
   ```rust
               crop_x: self.crop_x,
               crop_y: self.crop_y,
   ```

**Do not change `SinkPlan` / `VideoFormat`.** The sink is opened at the
OUTPUT size, which is already `width`/`height` — a region's sink is the
region's size, which is exactly right and needs no new field.

- [ ] **Step 6: Build for both targets**

```bash
cd src-tauri && cargo test -p vault_buddy_screen; echo "test exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings; echo "linux clippy exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "win clippy exit=$?"
```
Expected: every `exit=0`. The Windows run is the ONLY thing that
type-checks the Region arm, the widened `FrameFlags` and the session
plumbing — if it is not green, nothing else in this task is verified.

- [ ] **Step 7: Check the shell still builds against the widened struct**

`ResolvedSource` is public and the shell constructs nothing of it, but the
shell does consume `source::resolve`. Confirm:

```bash
cd src-tauri && cargo clippy -p vault-buddy --lib -- -D warnings; echo "shell exit=$?"
cd src-tauri && cargo clippy -p vault-buddy --lib --target x86_64-pc-windows-msvc -- -D warnings; echo "shell win exit=$?"
cd src-tauri && cargo test -p vault-buddy --lib; echo "shell test exit=$?"
```
Expected: every `exit=0`.

- [ ] **Step 8: Mutation-verify what can be verified, and say what cannot**

| Mutation | Must fail |
| --- | --- |
| `uncropped_dims` returns `(1, 1, width, height)` | `a_whole_source_crops_at_the_origin` |
| `uncropped_dims` returns `(0, 0, height, width)` | `a_whole_source_crops_at_the_origin` |

Then state plainly in your report: **the Region arm of `resolve`, the
widened `FrameFlags` and the session plumbing are pinned by no automated
test on any platform** — the Windows-target clippy run proves only that
they type-check. Name that in the report rather than implying coverage the
task does not have; Task 8 records it against GAP-117.

- [ ] **Step 9: Run the remaining gates and commit**

```bash
cd src-tauri && cargo fmt --check; echo "fmt exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "loc exit=$?"
```

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t3.txt <<'MSG'
feat(screen): resolve a region source and crop the session's frames

A region is recorded by capturing its whole monitor and keeping a rectangle
of it, so resolve now answers with an output size plus a crop origin and
the frame callback converts from that origin.

clamp_to_frame runs at START, not only when the rectangle was drawn: the
monitor's resolution can change in between, and a stale rectangle indexes
outside the frame buffer. An empty intersection is SourceGone -- the thing
the user picked is genuinely not there any more -- rather than a capture
that starts and produces nothing.

The monitor lookup scans for the monitor whose index() matches, never
Monitor::from_index, which is positional; crossing those two numbering
schemes is how phase 2 shipped a wrong-screen bug.

The crop origin is deliberately not passed through even_dims. Rounding a
size down to something NV12 can express is about the output; the origin is
about where in the source that output starts, and clamp_to_frame has
already made a region's size even.

This arm is cfg(windows) and executes in no automated test anywhere
(docs/Gaps.md GAP-117), which is why it contains no logic beyond lookup,
clamp and struct build -- every decision it relies on is tested in a pure
module that Linux runs.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/screen/src/source.rs src-tauri/screen/src/frames.rs src-tauri/screen/src/session/windows_session.rs
git commit -F /tmp/msg-t3.txt
```

---

### Task 4: The `overlay` window and `RegionRoot.vue`

Spec §5.2's fifth window and the rubber band that runs in it. This task
builds the surface and its behaviour; Task 5 builds the Rust lifecycle that
shows it and consumes its answer. Until Task 5 lands, the command
`RegionRoot` invokes does not exist — that is fine and expected, because
every test mocks IPC.

**Files:**
- Create: `src/roots/RegionRoot.vue`
- Modify: `src/roots/index.ts`
- Modify: `src-tauri/tauri.conf.json` (the `overlay` window)
- Modify: `src-tauri/capabilities/default.json` (`"overlay"` in `windows`)
- Modify: `src/types.ts`
- Create: `tests/regionRoot.test.ts`
- Modify: `tests/main-root.test.ts` (this is where `rootFor`'s test lives — **not** a `roots.test.ts`)

**Interfaces:**
- Consumes: nothing.
- Produces, for Task 5: the shape `RegionRoot` sends —
  `invoke("resolve_region_selection", { rect: RegionPick | null })` where
  ```ts
  export interface RegionPick {
    x: number; y: number; width: number; height: number; dpr: number;
  }
  ```
  All four geometry values are **logical (CSS) pixels relative to the
  overlay's own viewport**, which is the monitor's origin. `dpr` is
  `window.devicePixelRatio`, sent for cross-checking only — Rust scales by
  the monitor's own `scale_factor` and logs a mismatch.

- [ ] **Step 1: Add the window, the capability entry, and the type**

`src-tauri/tauri.conf.json` — append to `app.windows`, after `bubble`,
verbatim from spec §5.2:

```jsonc
      {
        "label": "overlay",
        "title": "Vault Buddy — Select Region",
        "visible": false,
        "transparent": true,
        "decorations": false,
        "alwaysOnTop": true,
        "resizable": false,
        "skipTaskbar": true,
        "focus": true
      }
```

No `width`/`height`: `select_capture_region` sizes it to the target monitor
while it is still hidden (Task 5), and a size here would only be a value
that is always immediately overwritten.

`src-tauri/capabilities/default.json` — `"windows": ["main", "panel", "bubble", "overlay"]`,
and extend the `description` string with:
`Region selection runs in the overlay window, which likewise drives everything through app commands.`

`src/types.ts` — add beside the other screen DTOs:

```ts
/** What `RegionRoot` hands back for one drag. All four geometry values are
 * LOGICAL (CSS) pixels relative to the overlay's own viewport, which is the
 * target monitor's origin — Rust converts to physical with that monitor's
 * own scale factor (`core::screen_geometry::to_physical`). `dpr` is
 * `window.devicePixelRatio`, sent so Rust can LOG a disagreement with the
 * monitor's scale factor; it is never used to scale. */
export interface RegionPick {
  x: number;
  y: number;
  width: number;
  height: number;
  dpr: number;
}

/** `select_capture_region`'s reply. `null` means the user cancelled. */
export interface RegionSelection {
  sourceId: string;
  x: number;
  y: number;
  width: number;
  height: number;
}
```

and widen the existing source kind union:

```ts
  kind: "screen" | "window" | "region";
```

- [ ] **Step 2: Write the failing `RegionRoot` tests**

Create `tests/regionRoot.test.ts`:

```ts
import { mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// `mockIPC` installs `__TAURI_INTERNALS__`, which makes `logging.ts` stop
// being a no-op and route through the log plugin — harmless, but it would
// put `plugin:log|log` calls in `calls`. Mocked for the same reason every
// other suite mocks it: the assertions read that array.
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import RegionRoot from "../src/roots/RegionRoot.vue";

type Call = { cmd: string; args: Record<string, unknown> };

let calls: Call[] = [];

beforeEach(() => {
  calls = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return Promise.resolve(null);
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

function surface(w: ReturnType<typeof mount>) {
  return w.get('[data-testid="region-surface"]');
}

/** One pointer drag, in logical CSS pixels. */
async function drag(
  w: ReturnType<typeof mount>,
  from: [number, number],
  to: [number, number],
) {
  await surface(w).trigger("pointerdown", { clientX: from[0], clientY: from[1] });
  await surface(w).trigger("pointermove", { clientX: to[0], clientY: to[1] });
  await surface(w).trigger("pointerup", { clientX: to[0], clientY: to[1] });
}

function resolved() {
  return calls.filter((c) => c.cmd === "resolve_region_selection");
}

describe("RegionRoot", () => {
  it("reports a drag as a logical rectangle", async () => {
    const w = mount(RegionRoot);
    await drag(w, [100, 50], [420, 230]);
    expect(resolved()).toHaveLength(1);
    // Hand-derived: x = min(100, 420) = 100, y = min(50, 230) = 50,
    // width = |420 - 100| = 320, height = |230 - 50| = 180.
    expect(resolved()[0].args.rect).toMatchObject({
      x: 100,
      y: 50,
      width: 320,
      height: 180,
    });
  });

  // A drag up-and-left is the same rectangle. Without normalisation the
  // width and height go negative, `to_physical` reads them as zero (its
  // `non_negative` guard) and the region silently becomes 1x1.
  it("normalises a drag made up and to the left", async () => {
    const w = mount(RegionRoot);
    await drag(w, [420, 230], [100, 50]);
    expect(resolved()[0].args.rect).toMatchObject({
      x: 100,
      y: 50,
      width: 320,
      height: 180,
    });
  });

  it("shows a live size readout while dragging", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    await surface(w).trigger("pointermove", { clientX: 210, clientY: 110 });
    // Still dragging: nothing reported yet, but the user can read the size.
    expect(resolved()).toHaveLength(0);
    expect(w.get('[data-testid="region-readout"]').text()).toContain("200");
    expect(w.get('[data-testid="region-readout"]').text()).toContain("100");
  });

  it("cancels on Escape", async () => {
    const w = mount(RegionRoot);
    await surface(w).trigger("pointerdown", { clientX: 10, clientY: 10 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();
  });

  // A stray click is not a selection. Without this, a click produces a
  // 0x0 (or 1x1) region, `clamp_to_frame` refuses it and the user gets an
  // error toast for having clicked.
  it("treats a drag below the minimum as a cancel, not a tiny region", async () => {
    const w = mount(RegionRoot);
    await drag(w, [100, 100], [104, 103]);
    expect(resolved()).toHaveLength(1);
    expect(resolved()[0].args.rect).toBeNull();
  });

  // Exactly at the threshold counts, so the boundary is pinned in both
  // directions rather than only on the reject side.
  it("accepts a drag exactly at the minimum", async () => {
    const w = mount(RegionRoot);
    await drag(w, [100, 100], [108, 108]);
    expect(resolved()[0].args.rect).toMatchObject({ width: 8, height: 8 });
  });

  it("reports the device pixel ratio alongside the rectangle", async () => {
    const w = mount(RegionRoot);
    await drag(w, [0, 0], [100, 100]);
    const rect = resolved()[0].args.rect as { dpr: number };
    expect(rect.dpr).toBe(window.devicePixelRatio);
  });

  // The overlay resolves exactly once. A second resolve would answer a
  // selection nobody asked for -- the Rust side has already taken its
  // one-shot sender out of the state by then, so the extra call is a
  // silent no-op that hides a real double-fire.
  it("resolves only once even if pointerup fires again", async () => {
    const w = mount(RegionRoot);
    await drag(w, [0, 0], [100, 100]);
    await surface(w).trigger("pointerup", { clientX: 200, clientY: 200 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await w.vm.$nextTick();
    expect(resolved()).toHaveLength(1);
  });

  it("removes its window key listener on unmount", async () => {
    const remove = vi.spyOn(window, "removeEventListener");
    const w = mount(RegionRoot);
    w.unmount();
    expect(remove).toHaveBeenCalledWith("keydown", expect.any(Function));
  });
});
```

```bash
cd /home/user/vault-buddy && npx vitest run tests/regionRoot.test.ts 2>&1 | tail -20
```
Expected: FAIL — cannot resolve `../src/roots/RegionRoot.vue`.

- [ ] **Step 3: Write `RegionRoot.vue`**

```vue
<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import type { RegionPick } from "../types";

/**
 * The region-selection surface (spec 5.2). This root runs in the `overlay`
 * window, which Rust has already sized and positioned over ONE monitor
 * while it was still hidden — so this viewport's origin IS that monitor's
 * origin, and every coordinate below is monitor-local by construction.
 * That is what makes `core::screen_geometry::to_physical`'s single scale
 * factor correct; see its module doc for why virtual-desktop coordinates
 * would not be.
 *
 * Everything here is reported in LOGICAL (CSS) pixels. The conversion to
 * physical happens in Rust with the monitor's own scale factor, not here:
 * a webview knows its `devicePixelRatio` but not which monitor Rust picked,
 * and two sources of truth for the scale is exactly the bug this feature is
 * most likely to have.
 */

/** Below this, on either axis, a drag is a stray click rather than a
 * selection. A 0x0 region fails `clamp_to_frame` and would surface as an
 * error toast for the crime of clicking. */
const MIN_DRAG_PX = 8;

const startX = ref(0);
const startY = ref(0);
const curX = ref(0);
const curY = ref(0);
const dragging = ref(false);
/** One-shot: the Rust side takes its sender out of the state on the first
 * answer, so a second call is a silent no-op that would hide a double-fire
 * rather than being harmless. */
const resolvedOnce = ref(false);

const box = computed(() => ({
  x: Math.min(startX.value, curX.value),
  y: Math.min(startY.value, curY.value),
  width: Math.abs(curX.value - startX.value),
  height: Math.abs(curY.value - startY.value),
}));

const style = computed(() => ({
  left: `${box.value.x}px`,
  top: `${box.value.y}px`,
  width: `${box.value.width}px`,
  height: `${box.value.height}px`,
}));

function report(rect: RegionPick | null) {
  if (resolvedOnce.value) return;
  resolvedOnce.value = true;
  dragging.value = false;
  invoke("resolve_region_selection", { rect }).catch((e) => {
    // Nothing to retry against: Rust's own bounded wait hides the overlay
    // on expiry, so a lost answer costs a cancelled selection, not a stuck
    // window. Never silent, though.
    logWarning(`resolve_region_selection failed: ${String(e)}`);
  });
}

function onDown(e: PointerEvent) {
  startX.value = e.clientX;
  startY.value = e.clientY;
  curX.value = e.clientX;
  curY.value = e.clientY;
  dragging.value = true;
}

function onMove(e: PointerEvent) {
  if (!dragging.value) return;
  curX.value = e.clientX;
  curY.value = e.clientY;
}

function onUp(e: PointerEvent) {
  if (!dragging.value) return;
  curX.value = e.clientX;
  curY.value = e.clientY;
  const { x, y, width, height } = box.value;
  if (width < MIN_DRAG_PX || height < MIN_DRAG_PX) {
    report(null);
    return;
  }
  report({ x, y, width, height, dpr: window.devicePixelRatio });
}

function onKeydown(e: KeyboardEvent) {
  if (e.key !== "Escape") return;
  e.preventDefault();
  report(null);
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <!-- The scrim is load-bearing, not decoration: a FULLY transparent
       region of a transparent window does not reliably receive pointer
       events, so the surface that reads the drag must be painted. -->
  <div
    data-testid="region-surface"
    class="fixed inset-0 cursor-crosshair select-none bg-black/35"
    @pointerdown="onDown"
    @pointermove="onMove"
    @pointerup="onUp"
  >
    <div
      v-if="dragging"
      data-testid="region-box"
      class="pointer-events-none absolute border-2 border-violet-400 bg-white/15"
      :style="style"
    >
      <span
        data-testid="region-readout"
        class="absolute left-0 top-full mt-1 rounded bg-black/70 px-1.5 py-0.5 text-xs font-medium text-fg"
      >
        {{ Math.round(box.width) }} x {{ Math.round(box.height) }}
      </span>
    </div>
    <p
      v-if="!dragging"
      data-testid="region-hint"
      class="absolute left-1/2 top-8 -translate-x-1/2 rounded-control bg-black/70 px-3 py-1.5 text-sm text-fg"
    >
      Drag to select a region — Esc to cancel
    </p>
  </div>
</template>
```

- [ ] **Step 4: Map the label and extend the `rootFor` test**

`src/roots/index.ts`:

```ts
import type { Component } from "vue";

import BubbleRoot from "./BubbleRoot.vue";
import BuddyRoot from "./BuddyRoot.vue";
import PanelRoot from "./PanelRoot.vue";
import RegionRoot from "./RegionRoot.vue";

/** Which root component a given window label renders. */
export function rootFor(label: string): Component {
  if (label === "panel") return PanelRoot;
  if (label === "bubble") return BubbleRoot;
  if (label === "overlay") return RegionRoot;
  return BuddyRoot; // "main" and any unexpected label
}
```

`tests/main-root.test.ts` — import `RegionRoot` and add to the existing
mapping assertions:

```ts
    expect(rootFor("overlay")).toBe(RegionRoot);
```

- [ ] **Step 5: Run the tests**

```bash
cd /home/user/vault-buddy && npx vitest run tests/regionRoot.test.ts tests/main-root.test.ts 2>&1 | tail -20
```
Expected: all pass.

- [ ] **Step 6: Mutation-verify every new test**

| Mutation | Must fail |
| --- | --- |
| `box` uses `startX` / `startY` directly instead of `Math.min` | `normalises a drag made up and to the left` |
| `box` uses `curX - startX` without `Math.abs` | `normalises a drag made up and to the left` |
| Drop the `MIN_DRAG_PX` check | `treats a drag below the minimum as a cancel` |
| `width <= MIN_DRAG_PX` instead of `<` | `accepts a drag exactly at the minimum` |
| Drop the `resolvedOnce` guard | `resolves only once even if pointerup fires again` |
| `onKeydown` checks `e.key === "Esc"` | `cancels on Escape` |
| Drop the `onBeforeUnmount` listener removal | `removes its window key listener on unmount` |
| Swap `width` and `height` in the reported rect | `reports a drag as a logical rectangle` |

If `resolves only once…` passes without the guard, look again: the
`dragging` flag alone may already be suppressing the second `pointerup`,
in which case the Escape half of that test is what has to fail. Report
which mechanism actually killed it.

- [ ] **Step 7: Run the full frontend chain**

```bash
cd /home/user/vault-buddy
rm -rf coverage && npm run lint; echo "lint exit=$?"
npm run check:loc; echo "loc exit=$?"
npm run check:quality; echo "quality exit=$?"
npm run test:coverage; echo "coverage exit=$?"
npm run build; echo "build exit=$?"
```
Expected: every `exit=0`, and **exactly one** lint warning, in
`src/main.ts`. Do **not** pipe any of these through `grep`.

If `check:quality` reports `averageMaintainability` below **91.4**: try
extraction first (the box/normalisation maths is a candidate for a pure
`src/utils/` helper with its own test). Only loosen the baseline if
extraction genuinely does not recover it, and then write the reasoning into
the baseline's `reason` string and name it in your report — Phase 2 had to
do that once and it must be visible in the PR description.

- [ ] **Step 8: Verify the Rust side still builds with the new window**

`tauri.conf.json` is compiled into the binary by `generate_context!`.

```bash
cd src-tauri && cargo test -p vault-buddy --lib; echo "shell test exit=$?"
```
Expected: `exit=0`. A malformed window entry fails here, not at runtime.

- [ ] **Step 9: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t4.txt <<'MSG'
feat(ui): add the region-selection overlay window and its root

Spec 5.2's fifth window: transparent, undecorated, always-on-top, created
hidden, and sized over one monitor by Rust before it is shown -- the same
position-while-hidden discipline the panel and the bubble follow, for the
same stale-frame reason.

Covering exactly one monitor is not a simplification, it is what makes the
geometry correct: the overlay's own viewport origin is that monitor's
origin, so every coordinate the drag reports is monitor-local and
core::screen_geometry::to_physical's single scale factor applies. Virtual-
desktop coordinates would need the origin scaled by one monitor's DPI and
the size by another's, which that function cannot express and its module
doc says so.

The root reports LOGICAL pixels and lets Rust scale. A webview knows its
own devicePixelRatio but not which monitor Rust picked, and two sources of
truth for the scale factor is the likeliest bug this feature has; the
ratio is sent alongside so Rust can log a disagreement rather than act on
one.

The scrim is load-bearing rather than decorative: a fully transparent
region of a transparent window does not reliably receive pointer events.
A drag under 8 logical pixels on either axis reports a cancel instead of a
tiny region, so a stray click cannot surface as an error toast.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src/roots/RegionRoot.vue src/roots/index.ts src/types.ts src-tauri/tauri.conf.json src-tauri/capabilities/default.json tests/regionRoot.test.ts tests/main-root.test.ts
git commit -F /tmp/msg-t4.txt
```

---

### Task 5: `select_capture_region` and the overlay's lifecycle

Spec §5.2 and §11's fourteenth command. `screen_commands.rs` is at
**717/800** non-blank lines and cannot absorb this, so it gets its own file
— which is also the right split: this is a window-lifecycle concern, not a
capture-lifecycle one.

**Files:**
- Create: `src-tauri/src/region_commands.rs`
- Modify: `src-tauri/src/lib.rs` (`mod region_commands;`, `.manage(...)`, two `generate_handler!` entries)
- Test: inline `#[cfg(test)]` in `region_commands.rs` (runs in the `linux-app` CI job)

**Interfaces:**
- Consumes: `region::{RegionSource, encode_payload, display_number_from_device_name}` and
  `source::SourceId` (Task 1); `RegionRoot`'s `{ rect: RegionPick | null }`
  payload (Task 4); `core::screen_geometry::{LogicalRect, to_physical, clamp_to_frame}`
  (Phase 1); `crate::set_dialog_active` (`lib.rs:101`).
- Produces, for Task 7:
  - `select_capture_region(sourceId: string) -> RegionSelection | null`
    (async; `null` = cancelled; `Err(string)` = a real failure to show)
  - `resolve_region_selection(rect: RegionPick | null) -> ()` (sync)
  - the wire DTO
    `RegionSelection { sourceId: string; x: number; y: number; width: number; height: number }`

- [ ] **Step 1: Write the failing tests for the pure core**

The whole DPI question — *"region capture works across DPI scales"*, this
phase's gate — reduces to one pure function, and that is deliberate: it is
the only part of this command a CI runner can execute. Create
`src-tauri/src/region_commands.rs` containing the module doc, the DTOs, a
`todo!()` body for `region_from_pick`, `matches_display`, and this test
module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn pick(x: f64, y: f64, width: f64, height: f64) -> RegionPick {
        RegionPick {
            x,
            y,
            width,
            height,
            dpr: 1.0,
        }
    }

    // THE PHASE GATE, in the one place a CI runner can reach it. Each
    // expected value is logical x scale, worked out by hand:
    //   100%  : 320x180 at (100, 50) stays 320x180 at (100, 50)
    //   125%  : 800x600 at (0, 0)    -> 1000x750
    //   150%  : 1280x720 at (0, 0)   -> 1920x1080
    //   200%  : 100x100 at (10, 10)  -> 200x200 at (20, 20)
    #[test]
    fn a_pick_scales_by_the_monitors_own_factor() {
        assert_eq!(
            region_from_pick(pick(100.0, 50.0, 320.0, 180.0), 1.0, 1920, 1080, 1),
            Ok(RegionSelectionDto {
                source_id: "region:1,100,50,320,180".into(),
                x: 100,
                y: 50,
                width: 320,
                height: 180,
            })
        );
        assert_eq!(
            region_from_pick(pick(0.0, 0.0, 800.0, 600.0), 1.25, 2560, 1440, 1)
                .map(|d| (d.width, d.height)),
            Ok((1000, 750))
        );
        assert_eq!(
            region_from_pick(pick(0.0, 0.0, 1280.0, 720.0), 1.5, 2560, 1440, 2)
                .map(|d| (d.source_id, d.width, d.height)),
            Ok(("region:2,0,0,1920,1080".into(), 1920, 1080))
        );
        assert_eq!(
            region_from_pick(pick(10.0, 10.0, 100.0, 100.0), 2.0, 3840, 2160, 1)
                .map(|d| (d.x, d.y, d.width, d.height)),
            Ok((20, 20, 200, 200))
        );
    }

    // The overlay reports what the WEBVIEW thinks; the monitor is the
    // authority. Sending a wrong dpr must change nothing but the log --
    // if this ever starts failing, someone has wired the webview's ratio
    // into the arithmetic and mixed-DPI setups will silently mis-scale.
    #[test]
    fn the_webviews_device_pixel_ratio_never_scales_anything() {
        let lying = RegionPick {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
            dpr: 3.0,
        };
        assert_eq!(
            region_from_pick(lying, 2.0, 3840, 2160, 1).map(|d| (d.width, d.height)),
            Ok((200, 200)),
            "scaled by the monitor's 2.0, not the webview's 3.0"
        );
    }

    // H.264 with NV12 4:2:0 needs even dimensions on both axes (GAP-101),
    // and clamp_to_frame rounds DOWN -- rounding up would grow the region
    // past what the user drew. 401 logical at 1.5 is 601.5 -> 601 -> 600.
    #[test]
    fn an_odd_physical_dimension_is_rounded_down_to_even() {
        assert_eq!(
            region_from_pick(pick(0.0, 0.0, 401.0, 300.0), 1.5, 2560, 1440, 1)
                .map(|d| (d.width, d.height)),
            Ok((600, 450))
        );
    }

    // The monitor is smaller than the scaled rectangle claims -- which is
    // what a resolution change between drawing and picking looks like.
    #[test]
    fn a_pick_overflowing_the_monitor_is_truncated() {
        assert_eq!(
            region_from_pick(pick(1800.0, 1000.0, 400.0, 400.0), 1.0, 1920, 1080, 1)
                .map(|d| (d.x, d.y, d.width, d.height)),
            Ok((1800, 1000, 120, 80))
        );
    }

    #[test]
    fn a_pick_entirely_off_the_monitor_is_an_error() {
        assert!(region_from_pick(pick(5000.0, 5000.0, 100.0, 100.0), 1.0, 1920, 1080, 1).is_err());
        // And a sub-2-physical-pixel region, which rounds to zero.
        assert!(region_from_pick(pick(0.0, 0.0, 1.0, 100.0), 1.0, 1920, 1080, 1).is_err());
    }

    // The join between Tauri's monitor naming and windows-capture's index
    // (see the plan's Global Constraints). A monitor with no readable name
    // must not match SOMETHING -- it must match nothing.
    #[test]
    fn a_monitor_is_matched_by_its_gdi_display_number() {
        assert!(matches_display(Some(r"\\.\DISPLAY2"), 2));
        assert!(!matches_display(Some(r"\\.\DISPLAY2"), 1));
        assert!(!matches_display(Some("Dell U2720Q"), 2));
        assert!(!matches_display(None, 2));
        assert!(!matches_display(None, 0));
    }

    // Only a whole SCREEN can be the target of a region selection: the
    // overlay covers one monitor, which is what makes to_physical's single
    // scale factor correct. A window or an existing region id must be
    // refused rather than quietly selecting on the primary display.
    #[test]
    fn only_a_screen_id_can_be_the_region_target() {
        assert_eq!(target_display("screen:2"), Some(2));
        assert_eq!(target_display("window:1234"), None);
        assert_eq!(target_display("region:1,0,0,100,100"), None);
        assert_eq!(target_display("nonsense"), None);
    }
}
```

```bash
cd src-tauri && cargo test -p vault-buddy --lib region_commands 2>&1 | tail -20
```
Expected: FAIL (the `todo!()` bodies panic).

- [ ] **Step 2: Implement the pure core**

```rust
//! Region selection (spec 5.2, 11): show the overlay over ONE monitor,
//! wait for the user's drag, and hand back a `region:` source id.
//!
//! Split out of `screen_commands.rs` because that file is at 717 of its
//! 800-line cap, and because this is a WINDOW-lifecycle concern rather
//! than a capture-lifecycle one — nothing here touches `CaptureGuard`,
//! `ScreenCaptureState` or the session.
//!
//! **Why the overlay covers exactly one monitor.** `to_physical` applies
//! ONE scale factor to the origin and the size alike, which is only
//! correct for monitor-local coordinates at that monitor's own DPI (see
//! `core::screen_geometry`'s module doc). Sizing the overlay to one
//! monitor makes its viewport origin that monitor's origin, so the
//! webview's coordinates are monitor-local by construction rather than by
//! a subtraction somebody has to remember.
//!
//! **Why the webview's `devicePixelRatio` is carried but never used.** The
//! webview knows its own ratio; it does not know which monitor Rust
//! picked. Two sources of truth for the scale factor is the most likely
//! way this feature goes wrong on a mixed-DPI desktop, so the monitor's
//! `scale_factor` is the only one that scales anything and a disagreement
//! is LOGGED — visible in a bug report, inert in the arithmetic.

use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition, PhysicalSize};
use vault_buddy_core::screen_geometry::{clamp_to_frame, to_physical, LogicalRect};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::region::{self, RegionSource};
use vault_buddy_screen::source::SourceId;

const OVERLAY_LABEL: &str = "overlay";

/// A selection nobody ever answers must not strand a full-screen,
/// invisible, always-on-top window over the user's desktop. Generous
/// because drawing a rectangle is a human action, bounded because a
/// crashed webview is a real outcome.
const REGION_TIMEOUT: Duration = Duration::from_secs(120);

/// One drag, in LOGICAL (CSS) pixels relative to the overlay's viewport,
/// i.e. to the target monitor's origin.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionPick {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The webview's own `devicePixelRatio`. Diagnostic only — see the
    /// module doc.
    pub dpr: f64,
}

/// What the picker renders and hands to `start_screen_capture`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegionSelectionDto {
    pub source_id: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The one-shot answer slot. `Some` exactly while a selection is in
/// flight, so a second `select_capture_region` is refused rather than
/// stealing the first one's answer.
#[derive(Default)]
pub struct RegionSelectionState(pub Mutex<Option<Sender<Option<RegionPick>>>>);

/// Does this Tauri monitor name denote GDI display `display`?
///
/// The ONE place the two monitor numbering schemes are joined: Tauri names
/// a Windows monitor by `MONITORINFOEXW.szDevice` (`\\.\DISPLAYn`) and
/// `windows_capture::monitor::Monitor::index()` is that same `n`. Never
/// match on a friendly name, and never use a positional index — phase 2
/// shipped a wrong-screen bug by crossing exactly these.
fn matches_display(name: Option<&str>, display: usize) -> bool {
    name.and_then(region::display_number_from_device_name) == Some(display)
}

/// The GDI display number a region selection targets, or `None` if the id
/// is not a whole screen.
fn target_display(source_id: &str) -> Option<usize> {
    match SourceId::parse(source_id)? {
        SourceId::Screen(n) => Some(n),
        // A window moves and resizes; a region of one is not a rectangle
        // on a monitor and the overlay has nothing to cover. A region of a
        // region is meaningless. Both are refused rather than silently
        // falling back to the primary display.
        SourceId::Window(_) | SourceId::Region(_) => None,
    }
}

/// Turn one logical drag into a region source id, scaled by the MONITOR's
/// factor and clamped to the monitor's current size.
fn region_from_pick(
    picked: RegionPick,
    scale: f64,
    monitor_w: u32,
    monitor_h: u32,
    display: usize,
) -> Result<RegionSelectionDto, String> {
    if (picked.dpr - scale).abs() > 0.01 {
        // Not an error: the monitor is the authority and the arithmetic
        // below already uses it. This line is how a mixed-DPI mis-scale
        // becomes visible in a bug report instead of being invisible.
        log::warn!(
            "region select: the overlay reported devicePixelRatio {} but monitor {display} \
             scales at {scale}; using the monitor's factor",
            picked.dpr
        );
    }
    let physical = to_physical(
        LogicalRect {
            x: picked.x,
            y: picked.y,
            width: picked.width,
            height: picked.height,
        },
        scale,
    );
    // Clamped at SELECTION time so an impossible region is refused while
    // the user is still looking at the picker. It is clamped AGAIN at
    // capture start (`source::resolve`), because the resolution can change
    // in between — this one is the friendly refusal, that one is the
    // correctness gate.
    let rect = clamp_to_frame(physical, monitor_w, monitor_h)
        .ok_or_else(|| "That region is not on the screen any more.".to_string())?;
    Ok(RegionSelectionDto {
        source_id: SourceId::Region(RegionSource {
            monitor: display,
            rect,
        })
        .to_string(),
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    })
}
```

```bash
cd src-tauri && cargo test -p vault-buddy --lib region_commands 2>&1 | tail -20
```
Expected: all seven tests pass. (`region_commands.rs` is not yet declared in
`lib.rs`, so add `mod region_commands;` first — see Step 4 — or this will
not compile at all.)

- [ ] **Step 3: Write the two commands**

Append to `src-tauri/src/region_commands.rs`:

```rust
/// Hide the overlay, drop any pending answer slot, let the panel auto-hide
/// again, and give the panel back its focus.
///
/// THE ONE CLEANUP SITE, the `clear_active_screen` discipline applied to
/// this feature's paired state: a path that hid the overlay without
/// clearing `DIALOG_ACTIVE` would leave the panel unable to auto-hide for
/// the rest of the session, with no error and no log line. Pinned by a
/// structural test below.
fn finish_region_selection(app: &AppHandle) {
    *lock_ignoring_poison(&app.state::<RegionSelectionState>().0) = None;
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(overlay) = handle.get_webview_window(OVERLAY_LABEL) {
            let _ = overlay.hide();
        }
        // The overlay took focus when it was shown; hand it back rather
        // than leaving the user on whatever Windows picks next.
        if let Some(panel) = handle.get_webview_window("panel") {
            if panel.is_visible().unwrap_or(false) {
                let _ = panel.set_focus();
            }
        }
    });
    crate::set_dialog_active(false);
}

/// ASYNC: this WAITS for a human to draw a rectangle. A sync command runs
/// on the main thread, where waiting freezes the event loop — which is
/// also the thread that has to dispatch the overlay's own pointer events,
/// so a sync version could never complete.
#[tauri::command]
pub async fn select_capture_region(
    app: AppHandle,
    source_id: String,
) -> Result<Option<RegionSelectionDto>, String> {
    let outcome = select_region_inner(&app, source_id).await;
    finish_region_selection(&app);
    outcome
}

async fn select_region_inner(
    app: &AppHandle,
    source_id: String,
) -> Result<Option<RegionSelectionDto>, String> {
    let display = target_display(&source_id)
        .ok_or_else(|| "Pick a screen to select a region on.".to_string())?;

    // `available_monitors` marshals to the event loop and blocks on the
    // reply with no timeout (the same tauri-runtime-wry round trip
    // `screen_commands::our_window_titles` documents). Safe only because
    // this command is async; do not make it sync.
    let monitors = app
        .available_monitors()
        .map_err(|e| format!("Could not read the display layout: {e}"))?;
    let monitor = monitors
        .into_iter()
        .find(|m| matches_display(m.name().map(String::as_str), display))
        .ok_or_else(|| "That screen is no longer connected.".to_string())?;
    let position = *monitor.position();
    let size = *monitor.size();
    let scale = monitor.scale_factor();

    let (tx, rx) = mpsc::channel::<Option<RegionPick>>();
    {
        let state = app.state::<RegionSelectionState>();
        let mut slot = lock_ignoring_poison(&state.0);
        if slot.is_some() {
            return Err("A region selection is already in progress.".to_string());
        }
        *slot = Some(tx);
    }

    // The overlay steals OS focus the moment it is shown; without this the
    // panel's focus-out check hides the panel — and the picker state the
    // user is halfway through — out from under them. Cleared in
    // `finish_region_selection`, on every path.
    crate::set_dialog_active(true);

    // POSITION AND SIZE WHILE HIDDEN, then show: a moved-or-resized window
    // that is already visible repaints its stale last frame at the new
    // bounds for a frame (AGENTS.md, "The window system"). Physical units
    // throughout, so no logical/physical conversion happens on the way to
    // a monitor whose scale we are about to depend on.
    let (setup_tx, setup_rx) = mpsc::channel::<Result<(), String>>();
    let shower = app.clone();
    app.run_on_main_thread(move || {
        let result = (|| {
            let overlay = shower
                .get_webview_window(OVERLAY_LABEL)
                .ok_or_else(|| "The region overlay window is missing.".to_string())?;
            overlay
                .set_position(PhysicalPosition::new(position.x, position.y))
                .map_err(|e| format!("Could not place the region overlay: {e}"))?;
            overlay
                .set_size(PhysicalSize::new(size.width, size.height))
                .map_err(|e| format!("Could not size the region overlay: {e}"))?;
            overlay
                .show()
                .map_err(|e| format!("Could not show the region overlay: {e}"))?;
            let _ = overlay.set_focus();
            Ok(())
        })();
        let _ = setup_tx.send(result);
    })
    .map_err(|e| format!("Could not reach the main thread: {e}"))?;
    setup_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| "The region overlay did not open.".to_string())??;

    let picked = tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(REGION_TIMEOUT))
        .await
        .map_err(|e| format!("Region selection failed: {e}"))?;
    let picked = match picked {
        Ok(p) => p,
        Err(_) => {
            // The webview never answered. Treat it as a cancel rather than
            // an error the user has to dismiss: nothing is broken, and
            // `finish_region_selection` is about to take the overlay down.
            log::warn!("region select: no answer within {REGION_TIMEOUT:?}; cancelling");
            None
        }
    };
    let Some(picked) = picked else {
        return Ok(None);
    };
    region_from_pick(picked, scale, size.width, size.height, display).map(Some)
}

/// SYNC: takes the one-shot sender and answers. O(1) under a mutex and one
/// channel send on an unbounded channel, so it cannot block the main
/// thread — the same posture as the sibling screen commands.
///
/// Deliberately does NOT hide the overlay. `select_capture_region`'s single
/// cleanup does that, on this path and on the timeout path alike; a second
/// hide site here is the thing the structural test below forbids.
#[tauri::command]
pub fn resolve_region_selection(app: AppHandle, rect: Option<RegionPick>) {
    let state = app.state::<RegionSelectionState>();
    let sender = lock_ignoring_poison(&state.0).take();
    match sender {
        Some(tx) => {
            let _ = tx.send(rect);
        }
        // Not an error: a duplicate pointerup, or an answer arriving after
        // the timeout already cancelled. Logged, never silent.
        None => log::debug!("region select: an answer arrived with no selection in flight"),
    }
}
```

**Remove the unused `LogicalSize` import** if the final code does not use
it — `cargo clippy -D warnings` will tell you.

- [ ] **Step 4: Register the module, the state and the commands**

`src-tauri/src/lib.rs`:
1. Beside the other `mod` declarations: `mod region_commands;`
2. Beside `.manage(screen_commands::ScreenCaptureState::default())` (line ~341):
   `.manage(region_commands::RegionSelectionState::default())`
3. In `generate_handler![…]`, after `screen_commands::screen_capture_status,`:
   ```rust
               region_commands::select_capture_region,
               region_commands::resolve_region_selection,
   ```

- [ ] **Step 5: Add the structural test for the single cleanup site**

Append to `region_commands.rs`'s test module:

```rust
    /// Production source only. `include_str!` pulls in THIS file, tests
    /// included, so a scan over the whole string counts the test's own
    /// literals and an `== 1` assertion becomes unreachable — the
    /// `production_src()` precedent from `screen/src/sink.rs` and
    /// `screen_commands.rs`, which two earlier tasks on this branch got
    /// wrong before review caught it.
    fn production_src() -> &'static str {
        let src = include_str!("region_commands.rs");
        match src.find("\n#[cfg(test)]") {
            Some(i) => &src[..i],
            None => src,
        }
    }

    // A second cleanup path that hid the overlay but forgot
    // `set_dialog_active(false)` would leave the panel unable to auto-hide
    // for the rest of the session — no error, no log line, and a user who
    // has to restart the app. Every exit from a region selection funnels
    // through one function, and this is what keeps it that way.
    #[test]
    fn the_region_selection_is_cleaned_up_from_exactly_one_place() {
        let src = production_src();
        assert_eq!(
            src.matches("finish_region_selection(&app)").count(),
            1,
            "add cleanup to finish_region_selection, do not add a second call site"
        );
        assert_eq!(
            src.matches("set_dialog_active(false)").count(),
            1,
            "DIALOG_ACTIVE must be cleared from the one cleanup function"
        );
        assert_eq!(
            src.matches("overlay.hide()").count(),
            1,
            "the overlay is hidden from the one cleanup function"
        );
    }
```

- [ ] **Step 6: Run the tests and mutation-verify**

```bash
cd src-tauri && cargo test -p vault-buddy --lib; echo "exit=$?"
```

| Mutation | Must fail |
| --- | --- |
| `region_from_pick` uses `picked.dpr` instead of `scale` | `the_webviews_device_pixel_ratio_never_scales_anything` |
| `region_from_pick` skips `clamp_to_frame` and builds from `physical` | `a_pick_overflowing_the_monitor_is_truncated`, `a_pick_entirely_off_the_monitor_is_an_error`, `an_odd_physical_dimension_is_rounded_down_to_even` |
| `region_from_pick` passes `monitor_h, monitor_w` (swapped) to `clamp_to_frame` | `a_pick_overflowing_the_monitor_is_truncated` |
| `target_display` returns `Some(0)` for a `Window` id | `only_a_screen_id_can_be_the_region_target` |
| `matches_display` returns `true` when `name` is `None` | `a_monitor_is_matched_by_its_gdi_display_number` |
| Add a second `overlay.hide()` inside `select_region_inner`'s timeout arm | `the_region_selection_is_cleaned_up_from_exactly_one_place` |

The last one matters most: it is the only thing stopping a future edit from
re-introducing a cleanup path that forgets the dialog flag. Run it, confirm
the test goes red, and restore byte-identically (`md5sum` before and after).

- [ ] **Step 7: Run the gates**

```bash
cd src-tauri && cargo fmt --check; echo "fmt exit=$?"
cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings; echo "clippy exit=$?"
cd src-tauri && cargo clippy -p vault-buddy --lib --target x86_64-pc-windows-msvc -- -D warnings; echo "win clippy exit=$?"
cd src-tauri && cargo test -p vault-buddy --lib; echo "test exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "loc exit=$?"
```
Expected: every `exit=0`. `region_commands.rs` is a NEW Rust file and the
LOC guard **rejects a new file above the 800 cap outright** — it cannot be
allowlisted into existence. If it lands over, split the pure core
(`region_from_pick`, `matches_display`, `target_display` and their tests)
into `src-tauri/src/region_geometry.rs` rather than asking for a baseline
entry.

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t5.txt <<'MSG'
feat(shell): select a capture region on one monitor through the overlay

Adds select_capture_region and resolve_region_selection. The overlay is
positioned and sized over the target monitor WHILE HIDDEN and only then
shown, the discipline every companion window follows: a visible window
that moves repaints its stale last frame at the new bounds for a frame.

Its own file rather than screen_commands.rs, which is at 717 of its
800-line cap -- and the right split anyway, since nothing here touches
CaptureGuard, the reservation or the session.

The monitor's scale factor is the only thing that scales. The overlay also
reports its devicePixelRatio, which is logged on disagreement and used for
nothing: a webview knows its own ratio but not which monitor was picked,
and two sources of truth for the scale is how a mixed-DPI desktop silently
records the wrong rectangle.

Only a whole screen can be a region target. A window moves and resizes, so
a region of one is not a rectangle on a monitor and the overlay would have
nothing to cover; refusing it beats falling back to the primary display.

DIALOG_ACTIVE is set for the selection's whole duration -- the overlay
steals focus and the panel's focus-out check would otherwise hide the
picker mid-selection -- and every exit funnels through one cleanup
function, pinned by a structural test. A path that hid the overlay but
forgot the flag would leave the panel unable to auto-hide for the rest of
the session with nothing in the log to say why.

The wait is bounded at two minutes so a webview that never answers cannot
strand a full-screen, invisible, always-on-top window over the desktop.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/src/region_commands.rs src-tauri/src/lib.rs
git commit -F /tmp/msg-t5.txt
```

---

### Task 6: Exclude Vault Buddy's own windows from the recording

Spec §5.3: `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` on every
app window for the duration of a screen capture. This is what lets the buddy
keep being the recording indicator — visible to the user, **absent from the
recording** — instead of appearing in every screen capture the user makes.

It is also the phase's most dangerous piece of paired state. A leaked
exclusion does not break Vault Buddy at all: it makes the user's windows
invisible in **other** applications' recordings — Teams, OBS, a colleague's
screen share — with no error, no log line and no way to guess the cause. So
it gets the `CaptureGuard` treatment: one apply site, one clear site, both
pinned structurally.

**Files:**
- Create: `src-tauri/src/capture_exclusion.rs`
- Modify: `src-tauri/Cargo.toml` (one added `windows-sys` feature)
- Modify: `src-tauri/src/screen_capture_worker.rs` (the one apply site)
- Modify: `src-tauri/src/screen_commands.rs` (the one clear site, inside `clear_active_screen`)
- Modify: `src-tauri/src/lib.rs` (`mod capture_exclusion;`)
- Test: inline `#[cfg(test)]` in `capture_exclusion.rs`

**Interfaces:**
- Consumes: nothing from other tasks. **Independent of Tasks 1–5 and 7** —
  it can be implemented and reviewed on its own.
- Produces: `capture_exclusion::apply(&AppHandle)` and
  `capture_exclusion::clear(&AppHandle)`.

- [ ] **Step 1: Add the Windows API feature**

`src-tauri/Cargo.toml`, in the existing `[target."cfg(windows)".dependencies]`
block — extend the `windows-sys` entry and its comment:

```toml
# GetKeyState for the start_buddy_drag stale-request guard, and
# SetWindowDisplayAffinity for spec 5.3's WDA_EXCLUDEFROMCAPTURE (keeping
# our own windows out of a screen recording). windows-sys is already in the
# dependency tree via tauri, so neither feature adds a crate to the build.
windows-sys = { version = "0.61", features = [
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
] }
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/capture_exclusion.rs` with the module doc, the label
list, `todo!()`-bodied functions, and:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    // WDA_EXCLUDEFROMCAPTURE is 17 and WDA_NONE is 0 (verified in
    // windows-sys 0.61.2, Win32/UI/WindowsAndMessaging/mod.rs:3522 and
    // :3524). Written out as literals here on purpose: this test is a
    // cross-check on the constant, so importing the constant to compare
    // against itself would assert nothing.
    #[test]
    fn the_affinity_values_are_the_documented_windows_constants() {
        assert_eq!(affinity_for(true), 17);
        assert_eq!(affinity_for(false), 0);
    }

    /// Every window label declared in `tauri.conf.json`.
    fn declared_window_labels() -> Vec<String> {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).expect("tauri.conf.json");
        conf["app"]["windows"]
            .as_array()
            .expect("app.windows is an array")
            .iter()
            .map(|w| {
                w["label"]
                    .as_str()
                    .expect("every window has a label")
                    .to_string()
            })
            .collect()
    }

    // THE point of this test, and why it reads the config rather than
    // repeating a list: phase 4 adds the `editor` window and phase 5
    // may add more. A window that exists but is not excluded appears in
    // every screen recording the user makes, which is a bug nobody will
    // attribute to the window having been added. This fails the moment a
    // window is declared without being listed here.
    #[test]
    fn every_declared_window_is_excluded_from_capture() {
        let declared = declared_window_labels();
        assert!(
            declared.len() >= 4,
            "expected at least main/panel/bubble/overlay, found {declared:?}"
        );
        for label in &declared {
            assert!(
                EXCLUDED_LABELS.contains(&label.as_str()),
                "window {label:?} is declared in tauri.conf.json but is not in \
                 EXCLUDED_LABELS, so it will appear in every screen recording"
            );
        }
    }

    // And the other direction: a label left behind after a window is
    // removed is a silent no-op that makes the list stop describing the
    // app.
    #[test]
    fn no_excluded_label_names_a_window_that_does_not_exist() {
        let declared = declared_window_labels();
        for label in EXCLUDED_LABELS {
            assert!(
                declared.iter().any(|d| d == label),
                "EXCLUDED_LABELS names {label:?}, which no longer exists in tauri.conf.json"
            );
        }
    }

    /// Recursively collect every `.rs` file under `dir`, skipping this
    /// test's OWN file — which necessarily names the functions being
    /// searched for. The `config_lock_guard.rs` precedent, including its
    /// `CARGO_MANIFEST_DIR` root (not the CWD) and its vacuity self-check.
    fn rust_files(dir: &Path, self_name: &std::ffi::OsStr, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, self_name, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
                && path.file_name() != Some(self_name)
            {
                out.push(path);
            }
        }
    }

    fn shell_sources() -> Vec<(PathBuf, String)> {
        let shell_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let self_name = Path::new(file!())
            .file_name()
            .expect("file!() always has a file name")
            .to_owned();
        let mut files = Vec::new();
        rust_files(&shell_src, &self_name, &mut files);
        // A self-check on the walk, not on the invariant: an empty walk
        // would make every assertion below vacuously true.
        assert!(
            files.len() > 5,
            "scan under {shell_src:?} found only {} file(s) — the walk is broken, not the \
             invariant",
            files.len()
        );
        files
            .into_iter()
            .map(|p| {
                let text = std::fs::read_to_string(&p)
                    .unwrap_or_else(|e| panic!("could not read {p:?}: {e}"));
                (p, text)
            })
            .collect()
    }

    /// One entry per occurrence, naming the file it was found in, so a
    /// failure says WHERE the extra call site is rather than only that the
    /// count is wrong.
    fn call_sites(needle: &str) -> Vec<String> {
        let mut hits = Vec::new();
        for (path, text) in shell_sources() {
            for _ in 0..text.matches(needle).count() {
                hits.push(path.display().to_string());
            }
        }
        hits
    }

    // A LEAKED exclusion does not break Vault Buddy — it makes the user's
    // windows invisible in OTHER applications' recordings (Teams, OBS, a
    // colleague's screen share) with no error and no log line. Pairing it
    // to exactly one apply and one clear is the only thing that makes that
    // impossible to get wrong later.
    //
    // Scans the WHOLE shell source tree, not one file: phase 2's
    // equivalent guard scanned only `screen_commands.rs` and went
    // half-blind the moment the lifecycle was split into
    // `screen_capture_worker.rs`.
    #[test]
    fn the_capture_exclusion_is_applied_and_cleared_from_exactly_one_place_each() {
        let applies = call_sites("capture_exclusion::apply(");
        let clears = call_sites("capture_exclusion::clear(");
        assert_eq!(
            applies.len(),
            1,
            "expected exactly one apply site, found {applies:?}"
        );
        assert_eq!(
            clears.len(),
            1,
            "expected exactly one clear site, found {clears:?}"
        );
        assert!(
            clears[0].ends_with("screen_commands.rs"),
            "the clear must live in clear_active_screen — the single chokepoint every \
             screen-capture teardown already funnels through — but it is in {}",
            clears[0]
        );
    }
}
```

```bash
cd src-tauri && cargo test -p vault-buddy --lib capture_exclusion 2>&1 | tail -20
```
Expected: FAIL.

- [ ] **Step 3: Implement**

```rust
//! Spec 5.3: keep Vault Buddy's own windows out of a screen recording.
//!
//! `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` (Windows 10
//! 2004+) makes a window invisible to screen capture while it stays fully
//! visible to the user — which is exactly what the buddy needs, since it
//! is the recording indicator and must not also be in every recording.
//!
//! **Why this is paired state with a single site each way.** A leaked
//! exclusion is invisible from inside this app: the buddy still shows,
//! the panel still works, nothing logs. What breaks is the user's NEXT
//! Teams call or OBS recording, where their Vault Buddy windows are
//! simply gone. There is no plausible bug report for that, so the code
//! shape has to make it impossible — one apply, one clear, both pinned by
//! a structural test over the whole shell source tree.
//!
//! **A failure here is never an error.** On a pre-2004 build the call
//! fails, the capture proceeds with the buddy in frame, and that is the
//! documented degraded behaviour (spec 5.3), not a reason to refuse a
//! start.

use tauri::{AppHandle, Manager};

/// Every window the app owns. Kept in step with `tauri.conf.json` by a
/// test that reads the config — phase 4's `editor` window will fail that
/// test until it is added here, which is the point.
pub(crate) const EXCLUDED_LABELS: &[&str] = &["main", "panel", "bubble", "overlay"];

/// `WDA_EXCLUDEFROMCAPTURE` (17) or `WDA_NONE` (0), the two
/// `WINDOW_DISPLAY_AFFINITY` values this app uses. Pure so the constants
/// are pinned somewhere that runs; `WDA_MONITOR` is deliberately not
/// offered — it hides a window from capture AND from remote sessions,
/// which is not what spec 5.3 asks for.
fn affinity_for(excluded: bool) -> u32 {
    if excluded {
        17
    } else {
        0
    }
}

/// Hide every app window from screen capture, for the duration of a
/// capture. THE ONE APPLY SITE is `screen_capture_worker`'s start path.
pub(crate) fn apply(app: &AppHandle) {
    set_affinity(app, true);
}

/// Put every app window back in view of other applications' captures. THE
/// ONE CLEAR SITE is `screen_commands::clear_active_screen`, the chokepoint
/// every screen-capture teardown — clean stop, self-finalize, and all ten
/// of the start path's early returns — already funnels through.
pub(crate) fn clear(app: &AppHandle) {
    set_affinity(app, false);
}

fn set_affinity(app: &AppHandle, excluded: bool) {
    let affinity = affinity_for(excluded);
    let handle = app.clone();
    // Window handles are read on the main thread like every other window
    // API in this app, and the call is fire-and-forget: the caller is a
    // worker thread that must not wait on the event loop, and an
    // exclusion that lands a few milliseconds late costs at most a frame
    // or two of buddy in the recording.
    if let Err(e) = app.run_on_main_thread(move || {
        for label in EXCLUDED_LABELS {
            let Some(window) = handle.get_webview_window(label) else {
                continue;
            };
            apply_to_window(&window, affinity, label);
        }
    }) {
        log::warn!("capture exclusion: could not reach the main thread: {e}");
    }
}

#[cfg(windows)]
fn apply_to_window(window: &tauri::WebviewWindow, affinity: u32, label: &str) {
    let hwnd = match window.hwnd() {
        Ok(h) => h,
        Err(e) => {
            log::warn!("capture exclusion: no window handle for {label}: {e}");
            return;
        }
    };
    // tauri hands back the `windows` crate's HWND newtype; windows-sys
    // wants the bare pointer it wraps.
    let raw = hwnd.0 as windows_sys::Win32::Foundation::HWND;
    // SAFETY: `raw` is a live top-level window handle owned by this
    // process, obtained from tauri on the thread that owns it, and
    // `affinity` is one of the two documented WINDOW_DISPLAY_AFFINITY
    // values.
    let ok = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity(raw, affinity) };
    if ok == 0 {
        // Windows 10 before 2004 has no WDA_EXCLUDEFROMCAPTURE. Spec 5.3:
        // log and carry on with the buddy visible in frame; this must
        // never be the reason a capture cannot start.
        log::warn!(
            "capture exclusion: SetWindowDisplayAffinity({affinity}) failed for {label}: {}",
            std::io::Error::last_os_error()
        );
    }
}

#[cfg(not(windows))]
fn apply_to_window(_window: &tauri::WebviewWindow, affinity: u32, label: &str) {
    log::debug!("capture exclusion: no-op off Windows ({label}, affinity {affinity})");
}
```

Declare it in `src-tauri/src/lib.rs` beside the other modules:

```rust
mod capture_exclusion;
```

- [ ] **Step 4: Wire the one apply site**

`src-tauri/src/screen_capture_worker.rs`, in
`start_screen_capture_blocking`, immediately **after** the reservation is
installed and **before** the device thread is spawned — i.e. right after the
closing brace of the `{ let mut guard = … *guard = Some(ActiveScreenCapture { … }); }`
block:

```rust
    // Spec 5.3: from here on the capture is committed, and every exit —
    // including every failure below — goes through `clear_active_screen`,
    // which is the one place the exclusion is lifted. Applying it before
    // the session opens means the first frames are already clean; it is
    // fire-and-forget on the main thread, so a busy event loop can still
    // let a frame or two of buddy through (docs/Gaps.md, recorded in
    // task 8) rather than delaying the start.
    crate::capture_exclusion::apply(app);
```

- [ ] **Step 5: Wire the one clear site**

`src-tauri/src/screen_commands.rs`, inside `clear_active_screen`, which
already frees the reservation and the `CaptureGuard` together:

```rust
pub(crate) fn clear_active_screen(app: &AppHandle) {
    let state = app.state::<ScreenCaptureState>();
    *lock_ignoring_poison(&state.0) = None;
    app.state::<CaptureGuard>().release(CaptureKind::Screen);
    // Spec 5.3's exclusion is lifted HERE and nowhere else, for the same
    // reason the guard is: this is the one function every teardown path
    // funnels through. Clearing an exclusion that was never applied (a
    // start that failed before the commit point) sets WDA_NONE on windows
    // that already had it, which is a no-op — strictly safer than a
    // conditional that could be wrong in the other direction and leave
    // the user's windows hidden from every other app's recordings.
    crate::capture_exclusion::clear(app);
    state.1.notify_all();
}
```

- [ ] **Step 6: Run the tests and both clippy targets**

```bash
cd src-tauri && cargo test -p vault-buddy --lib; echo "test exit=$?"
cd src-tauri && cargo clippy -p vault-buddy --all-targets -- -D warnings; echo "linux clippy exit=$?"
cd src-tauri && cargo clippy -p vault-buddy --lib --target x86_64-pc-windows-msvc -- -D warnings; echo "win clippy exit=$?"
cd src-tauri && cargo fmt --check; echo "fmt exit=$?"
```
Expected: every `exit=0`. The Windows run is the only check that
`hwnd.0 as windows_sys::…::HWND` and the `SetWindowDisplayAffinity`
signature are right — on Linux that whole function is `cfg`-ed away.

Note: `cargo machete` may now flag `windows-sys` if the Linux build no
longer references it — it will not, because `commands.rs`'s `GetKeyState`
already uses it under the same `cfg`. Confirm with
`cd src-tauri && cargo machete .; echo "exit=$?"`.

- [ ] **Step 7: Mutation-verify**

| Mutation | Must fail |
| --- | --- |
| `affinity_for(true)` returns `1` (`WDA_MONITOR`) | `the_affinity_values_are_the_documented_windows_constants` |
| Drop `"overlay"` from `EXCLUDED_LABELS` | `every_declared_window_is_excluded_from_capture` |
| Add `"editor"` to `EXCLUDED_LABELS` | `no_excluded_label_names_a_window_that_does_not_exist` |
| Add a second `capture_exclusion::clear(app)` in `screen_capture_worker.rs` | `the_capture_exclusion_is_applied_and_cleared_from_exactly_one_place_each` |
| Move the clear from `screen_commands.rs` to `screen_capture_worker.rs` | same test, on the `ends_with` assertion |
| Delete the apply call entirely | same test, on `applies.len() == 1` |

The `"editor"` mutation is the one to run carefully: it is the exact shape
of Phase 4's change, and confirming the test catches a label with no window
behind it is what makes it catch the reverse (a window with no label) when
Phase 4 arrives.

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t6.txt <<'MSG'
feat(shell): keep our own windows out of a screen recording

Spec 5.3: SetWindowDisplayAffinity with WDA_EXCLUDEFROMCAPTURE makes a
window invisible to screen capture while it stays fully visible to the
user. That is what lets the buddy go on being the recording indicator
without also being in every recording the user makes.

Treated as paired state with one apply and one clear, pinned by a
structural test, because a leaked exclusion is invisible from inside this
app: the buddy still shows, nothing logs, and what breaks is the user's
NEXT Teams call or OBS recording, where their Vault Buddy windows are
simply gone. There is no plausible bug report for that. The clear lives in
clear_active_screen, the chokepoint every screen-capture teardown already
funnels through, and clearing an exclusion that was never applied is a
harmless no-op -- strictly safer than a conditional that can be wrong in
the direction that hides the user's windows.

The label list is checked against tauri.conf.json rather than repeated by
hand, so phase 4's editor window fails the test until it is excluded too.

A failure to set the affinity is logged and ignored: on Windows 10 before
2004 the API does not exist, and an optional cosmetic exclusion must never
be the reason a capture cannot start.

The structural scan covers the whole shell source tree rather than one
file. Phase 2's equivalent guard scanned only screen_commands.rs and went
half-blind the moment that lifecycle was split across two files.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src-tauri/src/capture_exclusion.rs src-tauri/src/lib.rs src-tauri/src/screen_commands.rs src-tauri/src/screen_capture_worker.rs src-tauri/Cargo.toml
git commit -F /tmp/msg-t6.txt
```

---

### Task 7: The Region tab, and the chooser hint that stops under-promising

Spec §7.2's third tab, and the two deliberate Phase-2 deviations that
docs/Gaps.md **GAP-111** says Phase 3 must undo: item **2** (no Region tab)
and item **3** (the chooser hint reading "Screen or window" where §7.1 says
"Screen, window, or region"). GAP-111 item **1** — the per-device audio
level bars — stays open; it needs a Rust-side emit first and is not this
task.

**How the Region tab works, and why.** §7.2 describes one "Select region…"
button, but a region lives on a monitor and the overlay covers exactly one.
So the Region tab lists the same monitors the Screen tab does, as *targets*,
and the button acts on the one that is picked. The alternative — an overlay
spanning the virtual desktop — was rejected in Task 4's reasoning: one scale
factor cannot describe a mixed-DPI desktop, and `core::screen_geometry`'s
module doc says so explicitly.

**Files:**
- Create: `src/utils/regionLabel.ts`
- Create: `tests/regionLabel.test.ts`
- Modify: `src/components/ScreenSourcePicker.vue`
- Modify: `src/components/RecordMode.vue:26-37`
- Modify: `tests/screenSourcePicker.test.ts`
- Modify: `tests/record-mode.test.ts`

**Interfaces:**
- Consumes: `select_capture_region(sourceId) -> RegionSelection | null` and
  the `RegionSelection` type (Tasks 4 and 5).
- Produces: nothing later tasks depend on.

- [ ] **Step 1: Write the failing tests for the pure label composer**

Create `tests/regionLabel.test.ts`:

```ts
import { describe, expect, it } from "vitest";

import { regionDetail, regionTitle } from "../src/utils/regionLabel";

describe("region labels", () => {
  // Must read the same as the Rust side's own title for the same source
  // (`screen/src/source.rs`'s Region arm, `format!("Region on {label}")`).
  // The capture bar and the sidecar get the Rust one, the picker row gets
  // this one; two spellings of one source is a support problem.
  it("titles a region by the monitor it is on", () => {
    expect(regionTitle("Screen 1")).toBe("Region on Screen 1");
    expect(regionTitle("Dell U2720Q")).toBe("Region on Dell U2720Q");
  });

  // Spec 7.2 renders "1280 x 720 at (320, 180) on Screen 1"; the title
  // carries the "on <monitor>" half and this carries the geometry.
  it("details a region by its size and origin", () => {
    expect(regionDetail({ x: 320, y: 180, width: 1280, height: 720 })).toBe(
      "1280x720 at (320, 180)",
    );
    expect(regionDetail({ x: 0, y: 0, width: 640, height: 480 })).toBe(
      "640x480 at (0, 0)",
    );
  });
});
```

```bash
cd /home/user/vault-buddy && npx vitest run tests/regionLabel.test.ts 2>&1 | tail -10
```
Expected: FAIL — cannot resolve `../src/utils/regionLabel`.

- [ ] **Step 2: Write the composer**

Create `src/utils/regionLabel.ts`:

```ts
/** The picker row for a selected region.
 *
 * Deliberately matches the Rust side's own title for the same source
 * (`src-tauri/screen/src/source.rs`, the `Region` arm of `resolve`, which
 * formats `Region on {monitor}`). The capture bar and the staging sidecar
 * read the Rust string; this row reads this one. Two spellings of the same
 * capture is a support problem, so if you change one, change both.
 *
 * ASCII `x` rather than a multiplication sign, matching the existing
 * `{width}x{height}` detail line that `list_capture_sources` produces for a
 * monitor. */
export function regionTitle(monitorTitle: string): string {
  return `Region on ${monitorTitle}`;
}

export function regionDetail(r: {
  x: number;
  y: number;
  width: number;
  height: number;
}): string {
  return `${r.width}x${r.height} at (${r.x}, ${r.y})`;
}
```

Re-run: expected PASS.

- [ ] **Step 3: Write the failing picker tests**

In `tests/screenSourcePicker.test.ts`, **replace** the test named
`offers Screen and Window tabs, and no Region tab in this phase` (it is the
pin GAP-111 item 2 says must be updated in the same commit that adds the
tab) with the block below, and add the rest after it. Extend the module's
`mockSources` helper to answer the new command:

```ts
/** The reply `select_capture_region` gives for a 1280x720 region at
 * (320, 180) on display 1. */
const REGION = {
  sourceId: "region:1,320,180,1280,720",
  x: 320,
  y: 180,
  width: 1280,
  height: 720,
};

function mockSources(
  sources: unknown[] = SOURCES,
  devices: unknown = NO_DEVICES,
  region: unknown = REGION,
) {
  mockIPC((cmd) => {
    if (cmd === "list_capture_sources") return sources;
    if (cmd === "list_audio_devices") return devices;
    if (cmd === "select_capture_region") return region;
    return undefined;
  });
}
```

```ts
  it("offers Screen, Window and Region tabs", async () => {
    // GAP-111 item 2: phase 2 shipped without a Region tab on purpose and
    // pinned its absence; phase 3 adds the tab and flips the pin, in the
    // same commit, as that entry requires.
    mockSources();
    const w = await mountPicker();
    expect(w.find('[data-testid="tab-screen"]').exists()).toBe(true);
    expect(w.find('[data-testid="tab-window"]').exists()).toBe(true);
    expect(w.find('[data-testid="tab-region"]').exists()).toBe(true);
    // And it is reachable, not merely rendered.
    await w.get('[data-testid="tab-region"]').trigger("click");
    expect(w.get('[data-testid="tab-region"]').attributes("aria-selected")).toBe("true");
  });

  it("selects a region on the picked monitor and arms Start with it", async () => {
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, ...(args as object) });
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") return STARTED;
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();

    // The overlay is opened for the monitor the user picked, not "the
    // primary" and not whatever the Screen tab happened to have selected.
    expect(calls.find((c) => c.cmd === "select_capture_region")).toMatchObject({
      sourceId: "screen:1",
    });
    // The row reads as spec 7.2 asks: size, origin, and which screen.
    expect(panel(w, "region")).toContain("1280x720 at (320, 180)");
    expect(panel(w, "region")).toContain("Region on Screen 1");
    // And Start now sends the region id, not the monitor id.
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "start_screen_capture")).toMatchObject({
      sourceId: "region:1,320,180,1280,720",
    });
  });

  it("keeps a cancelled region selection from arming Start", async () => {
    // `select_capture_region` resolves null when the user pressed Escape or
    // clicked without dragging. Arming Start off a null would send the
    // string "null" as a source id.
    mockSources(SOURCES, NO_DEVICES, null);
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
    expect(w.find('[data-testid="source-region:1,320,180,1280,720"]').exists()).toBe(false);
  });

  it("surfaces a failed region selection inline", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") throw new Error("That screen is no longer connected.");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-error"]').text()).toContain("no longer connected");
  });

  // THE REGRESSION THIS TASK IS MOST LIKELY TO SHIP. `loadSources()` drops
  // a selection the refreshed list no longer offers -- and a region id is
  // NEVER in that list, so the naive check clears it on every refresh and
  // Start silently disarms itself.
  it("keeps a selected region across a source refresh", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    // A failed start triggers a refresh; the region must survive it.
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(panel(w, "region")).toContain("1280x720 at (320, 180)");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  // The other direction: if the region's own MONITOR goes away, the region
  // is as gone as a closed window and must not stay armed.
  it("drops a selected region when its monitor disappears", async () => {
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        // Second read: the monitor is unplugged, only the window remains.
        return listed === 1 ? SOURCES : [SOURCES[1]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(panel(w, "region")).not.toContain("1280x720 at (320, 180)");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
  });

  it("keeps the region Select button disabled until a monitor is picked", async () => {
    mockSources();
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeUndefined();
  });
```

```bash
cd /home/user/vault-buddy && npx vitest run tests/screenSourcePicker.test.ts 2>&1 | tail -25
```
Expected: the new tests FAIL (no `tab-region`), the pre-existing ones pass.

- [ ] **Step 4: Add the Region tab to the picker**

`src/components/ScreenSourcePicker.vue`. In `<script setup>`:

```ts
import type { CaptureSourceInfo, RegionSelection } from "../types";
import { regionDetail, regionTitle } from "../utils/regionLabel";

// Screen, Window and Region (spec 7.2). The Region tab arrived in phase 3
// together with the overlay it opens; phase 2 deliberately shipped without
// it rather than rendering a disabled tab with nothing behind it
// (docs/Gaps.md GAP-111 item 2).
const TABS = [
  { id: "screen", label: "Screen" },
  { id: "window", label: "Window" },
  { id: "region", label: "Region" },
] as const;

/** The two tabs that render a plain row list from `list_capture_sources`.
 * Region builds its own row from a selection, so it gets its own slot. */
const LIST_TABS = TABS.filter((t) => t.id !== "region");

const region = ref<RegionSelection | null>(null);
/** Which monitor "Select region…" opens the overlay on. A region lives on
 * exactly one monitor — see the overlay's own reasoning — so the Region tab
 * lists the monitors as targets rather than guessing the primary. */
const regionTargetId = ref<string | null>(null);
const selectingRegion = ref(false);

const screens = computed(() => rowsFor("screen"));
const regionRowTitle = computed(() => {
  const target = sources.value.find((s) => s.id === regionTargetId.value);
  return regionTitle(target?.title ?? "this screen");
});
```

Extend `canStart` to keep working unchanged (`selectedId` still drives it),
and add:

```ts
async function onSelectRegion() {
  if (regionTargetId.value === null || selectingRegion.value) return;
  selectingRegion.value = true;
  error.value = null;
  try {
    const picked = await invoke<RegionSelection | null>("select_capture_region", {
      sourceId: regionTargetId.value,
    });
    // `null` is a cancel (Escape, or a click that was not a drag), not a
    // failure: leave whatever was selected before exactly as it was.
    if (picked) {
      region.value = picked;
      selectedId.value = picked.sourceId;
    }
  } catch (e) {
    logWarning(`select_capture_region failed: ${String(e)}`);
    error.value = String(e);
  } finally {
    selectingRegion.value = false;
  }
}
```

and change `loadSources`'s selection-dropping rule:

```ts
async function loadSources() {
  try {
    sources.value = await invoke<CaptureSourceInfo[]>("list_capture_sources");
    // A region id is NEVER in `sources` — it is built from a selection, not
    // enumerated — so the "drop what the list no longer offers" rule has to
    // ask about the region's MONITOR instead. Without this the region is
    // cleared on every refresh and Start silently disarms itself.
    if (region.value && !sources.value.some((s) => s.id === regionTargetId.value)) {
      region.value = null;
      regionTargetId.value = null;
    }
    const known =
      sources.value.some((s) => s.id === selectedId.value) ||
      selectedId.value === region.value?.sourceId;
    if (selectedId.value && !known) {
      selectedId.value = null;
    }
  } catch (e) {
    logWarning(`list_capture_sources failed: ${String(e)}`);
    error.value = String(e);
  }
}
```

In the template, change the row-list `v-for` to iterate `LIST_TABS` instead
of `TABS`, and add the Region slot after it:

```vue
      <template #region>
        <div class="flex flex-col gap-2">
          <p class="text-xs text-fg-muted">
            Pick a screen, then drag out the area you want to record.
          </p>
          <ul class="flex flex-col gap-1">
            <li
              v-for="s in screens"
              :key="s.id"
            >
              <button
                type="button"
                :data-testid="`region-target-${s.id}`"
                :aria-pressed="regionTargetId === s.id"
                class="w-full cursor-pointer rounded-control border bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
                :class="regionTargetId === s.id ? 'border-violet-400' : 'border-white/10'"
                @click="regionTargetId = s.id"
              >
                <span class="block truncate text-sm font-medium text-fg">{{ s.title }}</span>
                <span class="block truncate text-xs text-fg-muted">{{ s.detail }}</span>
              </button>
            </li>
          </ul>
          <AppButton
            data-testid="region-select"
            variant="secondary"
            :disabled="regionTargetId === null || selectingRegion"
            @click="onSelectRegion"
          >
            {{ selectingRegion ? "Selecting…" : region ? "Reselect region…" : "Select region…" }}
          </AppButton>
          <button
            v-if="region"
            type="button"
            :data-testid="`source-${region.sourceId}`"
            :aria-pressed="selectedId === region.sourceId"
            class="w-full cursor-pointer rounded-control border bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
            :class="selectedId === region.sourceId ? 'border-violet-400' : 'border-white/10'"
            @click="selectedId = region.sourceId"
          >
            <span class="block truncate text-sm font-medium text-fg">{{ regionRowTitle }}</span>
            <span class="block truncate text-xs text-fg-muted">{{ regionDetail(region) }}</span>
          </button>
        </div>
      </template>
```

`regionDetail` must be exposed to the template — with `<script setup>` an
imported function already is.

- [ ] **Step 5: Fix the chooser hint (GAP-111 item 3)**

`src/components/RecordMode.vue` — the comment at lines 26-32 currently
explains why the hint under-promises. Replace it and the option:

```ts
// Capture actions first (spec 7.1), import next, browse last. The screen
// entry NAVIGATES rather than starting anything — a screen capture needs a
// source picked first — so each option carries its own aria label instead of
// the old derived "Start a <title> recording" (which would have read "Start a
// record screen recording"). The hint reads spec 7.1's own wording now that
// the picker really offers all three: phase 2 shipped "Screen or window"
// deliberately, because advertising region from the chooser while the picker
// could not do it was the same dead promise a disabled Region tab would be
// (docs/Gaps.md GAP-111 item 3).
const OPTIONS = [
  { key: "meeting", title: "Meeting", hint: "Microphone + desktop audio", testId: "mode-meeting", aria: "Start a meeting recording" },
  { key: "voice-note", title: "Voice Note", hint: "Microphone only", testId: "mode-voice-note", aria: "Start a voice note recording" },
  { key: "screen", title: "Record Screen", hint: "Screen, window, or region", testId: "mode-screen", aria: "Choose a screen, window, or region to capture" },
] as const;
```

`RecordMode`'s suite is `tests/record-mode.test.ts` (verified — it is the
only suite that mounts the component and asserts on `mode-screen`). Add
there, matching that file's own mounting helper and naming:

```ts
  // GAP-111 item 3: the chooser must not under-advertise a capability the
  // picker now has. Pinned in both directions so a revert is visible.
  it("advertises region capture in the intake chooser", async () => {
    const w = await mountRecordMode();
    const screen = w.get('[data-testid="mode-screen"]');
    expect(screen.text()).toContain("Screen, window, or region");
    expect(screen.attributes("aria-label")).toContain("region");
  });
```

matching that suite's own mounting helper and naming.

- [ ] **Step 6: Run the tests**

```bash
cd /home/user/vault-buddy && npx vitest run tests/screenSourcePicker.test.ts tests/regionLabel.test.ts 2>&1 | tail -25
```
Expected: all pass, including every pre-existing picker test.

- [ ] **Step 7: Mutation-verify**

| Mutation | Must fail |
| --- | --- |
| `onSelectRegion` passes `selectedId.value` instead of `regionTargetId.value` | `selects a region on the picked monitor and arms Start with it` |
| `onSelectRegion` assigns `region.value = picked` without the `if (picked)` guard | `keeps a cancelled region selection from arming Start` |
| Revert `loadSources` to the original `!sources.some(s => s.id === selectedId)` check | `keeps a selected region across a source refresh` |
| Drop the `region.value = null` branch in `loadSources` | `drops a selected region when its monitor disappears` |
| Remove the `:disabled` on `region-select` | `keeps the region Select button disabled until a monitor is picked` |
| `regionDetail` emits `${r.x}x${r.y} at …` | `details a region by its size and origin` |
| Revert the `RecordMode` hint to "Screen or window" | `advertises region capture in the intake chooser` |

- [ ] **Step 8: Run the full frontend chain**

```bash
cd /home/user/vault-buddy
rm -rf coverage && npm run lint; echo "lint exit=$?"
npm run check:loc; echo "loc exit=$?"
npm run check:quality; echo "quality exit=$?"
npm run test:coverage; echo "coverage exit=$?"
npm run build; echo "build exit=$?"
```
Expected: every `exit=0`, exactly one lint warning in `src/main.ts`.

`ScreenSourcePicker.vue` is at 141 lines against the 500 frontend cap, and
this task adds roughly 70 — comfortable. If `check:quality` drops
`averageMaintainability` below 91.4, extract before loosening: the
region-selection state and its two handlers are a clean
`src/composables/useRegionSelection.ts`, and moving them there is a real
improvement rather than a metric dodge.

- [ ] **Step 9: Commit**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t7.txt <<'MSG'
feat(ui): offer region capture in the source picker

Adds spec 7.2's third tab and closes docs/Gaps.md GAP-111 items 2 and 3 --
the deliberate phase-2 omissions that entry exists so phase 3 undoes on
purpose rather than by accident. Item 1, the per-device audio level bars,
stays open: nothing emits a per-device level outside a running audio
recording, so restoring those needs a Rust change first.

The Region tab lists monitors as TARGETS and acts on the picked one. Spec
7.2 describes a single Select region button, but a region lives on one
monitor and the overlay covers one monitor -- which is not a simplification
but the thing that makes the DPI conversion correct, since one scale factor
cannot describe a mixed-DPI desktop.

loadSources' "drop a selection the refreshed list no longer offers" rule
had to learn about regions. A region id is never in that list, so the naive
check cleared it on every refresh and silently disarmed Start; it now asks
about the region's MONITOR instead, which also means an unplugged monitor
correctly drops the region with it.

A null reply is a cancel, not a failure: Escape, or a click that was not a
drag, leaves the previous selection exactly as it was rather than clearing
it or arming Start with nothing.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add src/components/ScreenSourcePicker.vue src/components/RecordMode.vue src/utils/regionLabel.ts tests/regionLabel.test.ts tests/screenSourcePicker.test.ts tests/record-mode.test.ts
git commit -F /tmp/msg-t7.txt
```

---

### Task 8: Docs, gaps, the verification checklist, and baselines

Spec §16. Phase 3 changes the window system, the IPC surface and the
picker, and AGENTS.md is the file the next agent reads before touching any
of them — so it is reconciled here, not "later".

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/Gaps.md`
- Modify: `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`
- Modify: `scripts/loc-baseline.json` / `scripts/quality-baseline.json` — **only if an earlier task actually moved one**
- Modify: `.superpowers/sdd/progress.md` (the ledger's Phase 3 section)

**Interfaces:** none — this task produces no code.

- [ ] **Step 1: Establish the real numbers before writing any of them down**

Do not copy a count out of this plan or out of a task report. Measure:

```bash
cd /home/user/vault-buddy
# Command count for the IPC-surface table's header line.
sed -n '/generate_handler!\[/,/\]);/p' src-tauri/src/lib.rs | grep -c '^\s*[a-z_]*::\?[a-z_]*,$'
# The current highest GAP id, and proof 104 is still absent.
grep -n "^### GAP-" docs/Gaps.md | tail -3
grep -c "GAP-104" docs/Gaps.md
# Whether any baseline actually moved on this branch.
git diff --stat origin/main...HEAD -- scripts/loc-baseline.json scripts/quality-baseline.json
```

AGENTS.md currently claims **79** commands and Phase 3 adds two, so the
table's header should read 81 — **but confirm it against the handler list
rather than trusting the arithmetic.** The Phase-2 review found that
sentence already wrong once (it said 73 when the real count was 79).

- [ ] **Step 2: Update AGENTS.md**

Six places, each named so none is missed:

1. **"Architecture overview"** — the diagram and its prose say *"Three OS
   windows, one frontend bundle, one Rust process"*. It is four now. Add
   the `overlay` box beside `bubble`, labelled `RegionRoot / rubber band`.
2. **"The window system (most invariant-heavy area)"** — it opens *"Three
   separate always-on-top transparent windows"*. Add a bullet for
   `overlay`, in the established shape:
   > - **`overlay`** — the region-selection surface (spec §5.2). Sized and
   >   positioned over ONE monitor **while hidden**, then shown, like
   >   `panel` and `bubble` and for the same stale-frame reason. Covering
   >   exactly one monitor is load-bearing, not a simplification: it makes
   >   the webview's viewport origin the monitor's origin, which is the
   >   only arrangement `core::screen_geometry::to_physical`'s single scale
   >   factor describes correctly (its module doc says why a mixed-DPI
   >   virtual desktop cannot be). `select_capture_region` holds
   >   `DIALOG_ACTIVE` for the selection's whole duration — the overlay
   >   steals focus and the panel's focus-out check would otherwise hide
   >   the picker mid-selection — and every exit funnels through one
   >   cleanup function, `finish_region_selection`, pinned by a structural
   >   test. The wait is bounded at 120 s so a webview that never answers
   >   cannot strand a full-screen invisible always-on-top window.
3. **"The IPC surface"** — correct the command count (Step 1's measured
   number) and add the row:
   > | `region_commands.rs` | `select_capture_region` *(async — it waits for a human to draw a rectangle; on the main thread that would freeze the very event loop that dispatches the overlay's pointer events)*, `resolve_region_selection` *(sync — O(1) under a mutex plus one unbounded-channel send)* |
4. **"Screen capture (phase 2)"** — retitle the heading to **"Screen
   capture (phases 2–3)"** and add three bullets in the section's existing
   voice: region capture (the id carries the rectangle, so
   `start_screen_capture` took no new parameter; `clamp_to_frame` runs
   twice, once at selection for a friendly refusal and once at start for
   correctness, because the resolution can change in between); the
   offset crop (`bgra_crop_to_nv12`, and `usable_frame` measuring from the
   crop's far edge, with the note that the origin is deliberately not
   rounded to even); and `WDA_EXCLUDEFROMCAPTURE` (one apply, one clear,
   why a leak is invisible from inside the app).
5. **"Frontend state"** — `rootFor()` gains `overlay → RegionRoot`, and the
   sentence listing the roots gains it too. Note that `RegionRoot` installs
   no store: it is the one root that mirrors no Rust state.
6. **"Repository map"** — `src/roots/` and the `src-tauri/src/` and
   `screen/src/` lines gain the new files.

Also correct the `screen` crate's row in **"What compiles where"**: its
list of PURE submodules gains `region`, and the sentence naming the
`cfg(windows)` set is unchanged.

- [ ] **Step 3: Update the three existing gap entries**

**GAP-111** — rewrite the entry so it says plainly which items Phase 3
closed and which did not:
- Item 2 (no Region tab) — **closed**; name the commit and the flipped test.
- Item 3 (the "Screen or window" hint) — **closed**; name the new string.
- Item 1 (per-device level bars) — **still open**, and restate why: nothing
  emits a per-device level outside a running audio recording, so this needs
  a Rust-side emit before any frontend change. Retitle the entry to reflect
  that only one deviation remains.

**GAP-117** — its subject is "the `cfg(windows)` arms execute in no
automated test anywhere". Phase 3 adds more of them: `source::resolve`'s
Region arm, the widened `FrameFlags` and session plumbing, and
`capture_exclusion::apply_to_window`. Add them to the entry's file list and
state explicitly that the Windows-target clippy run proves only that they
type-check.

**GAP-116** — "our own windows are filtered out of the picker by TITLE, not
HWND". `WDA_EXCLUDEFROMCAPTURE` does **not** supersede it: the exclusion
keeps our windows out of a recording's *pixels*, while GAP-116 is about our
windows being offered as *sources*. Add one sentence saying so, so a future
reader does not close it on the strength of this phase.

- [ ] **Step 4: Add the new gap entries**

Allocate upward from whatever Step 1's `grep` reported — **GAP-123 was the
highest at plan time, and GAP-104 is retired and must never be reused.**
The ids below assume 124 is free; if it is not, shift them and say so.

**GAP-124 · Low · The capture exclusion is applied fire-and-forget, so the
first frames of a capture can still contain Vault Buddy's own windows.**
`src-tauri/src/capture_exclusion.rs` (`set_affinity` posts to
`run_on_main_thread` and returns immediately) and
`src-tauri/src/screen_capture_worker.rs` (the apply site). The exclusion is
applied before the session opens, but the apply is a queued main-thread
closure while the start path carries on. **Failure scenario:** on a busy
event loop the WGC session can deliver its first frame or two before the
affinity lands, so a recording can open with the buddy visible for ~30 ms.
**Why not fixed:** waiting for the closure means a worker thread blocking on
the event loop, which is the deadlock shape this codebase's window rules
exist to prevent (the original drag crash). **Fix shape:** apply the
exclusion once at startup for a capture that is *about* to run, or
acknowledge the closure with a bounded channel the way
`select_capture_region`'s setup handshake does.

**GAP-125 · Low · Region capture reads back the whole monitor every frame
and crops on the CPU.** `src-tauri/screen/src/frames.rs` /
`src-tauri/screen/src/convert.rs`. Spec §17.2 asks whether cropping can
happen on the GPU texture before readback; it does not, and that is
deliberate — the crop lives in a pure function precisely so Linux can test
it, which is the only automated coverage region capture has (GAP-117).
**Failure scenario:** recording a small region of a 4K monitor at 60 fps
costs the full-monitor readback and BGRA→NV12 pass regardless of how small
the region is, so a 640×480 region is no cheaper than a full-screen
capture. **Fix shape:** a D3D11 `CopySubresourceRegion` before the CPU
readback, which would move the crop into code nothing can test — worth it
only if checklist item 10's measurements show the readback is the
bottleneck.

**GAP-126 · Low · A region selection nobody answers cancels silently after
two minutes.** `src-tauri/src/region_commands.rs` (`REGION_TIMEOUT`). If the
overlay webview dies or never resolves, the wait expires, the overlay is
hidden and `select_capture_region` returns `Ok(None)` — indistinguishable,
to the picker, from the user pressing Escape. **Failure scenario:** a user
whose overlay failed to render sees the picker come back with no region and
no explanation, and repeating it takes another two minutes each time.
**Why it is this way:** the alternative — an error toast — would fire on
every ordinary slow-but-legitimate selection near the deadline. **Fix
shape:** distinguish the two outcomes in the reply (`cancelled` vs
`timedOut`) and surface only the latter, once the overlay has proven
reliable enough on real hardware to know which it is.

**GAP-127 · Low · There is no keyboard-only way to draw a region.**
`src/roots/RegionRoot.vue`. The overlay reads pointer events only; Escape
cancels, but nothing selects. **Failure scenario:** a user who cannot use a
pointing device can record a screen or a window but not a region — the one
capture source with no keyboard path. **Fix shape:** arrow-key cursor
movement with Space to anchor and Enter to commit, plus a live
`aria-live` readout of the rectangle, following the pattern the task
list's own drag-and-drop keyboard fallback already established
(`TaskDragHandle`).

- [ ] **Step 5: Extend the Windows verification checklist**

`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`.

The user's standing decision is that **manual Windows re-testing is
deferred until after the final phase** — so this step writes rows, it does
not run them and does not ask anyone to.

1. Retitle: `# Screen Capture — Windows Verification Checklist` (drop
   "(Phase 2)"), and adjust the intro so it reads as a running document
   across phases rather than one phase's gate.
2. **Carry the open Phase-2 rows forward, do not close them.** Items **2**
   (the mic+loopback mix question is still unanswered), **5** (whether a
   warning surfaced and whether the file holds everything up to the close),
   **7**'s badge half (the buddy red/amber dot and the vault-row dot were
   not separately confirmed) and **9** (the still-screen heartbeat, now
   unblocked by `d5ed392` and the sharpest test of that fix) all stay open
   with their existing text intact. Item **10** stays deferred to Phase 6.
3. Add a **"Covered by Phase 3"** table after the Phase-2 one, same shape
   (`# | Check | Steps | Result`), with these rows and an empty *Result*
   column — and the same measurement discipline the file already states:
   write the observed value, never a tick.

   | # | Check | Steps |
   | --- | --- | --- |
   | 11 | **Region on the primary monitor at 100%** | Capture knowledge → Record screen → **Region** tab → pick the primary screen → *Select region…* → drag a rectangle over something with readable text → Start → record ~20 s → Stop. Play the staged `.mp4`. **Record the file's pixel dimensions** and whether the recorded content is exactly the rectangle drawn — check all four edges, not just that "it looks right". |
   | 12 | **Region at 125 / 150 / 200% display scaling** | Repeat row 11 at each scale (Settings → System → Display → Scale), relaunching the app after each change. **Record, per scale: the rectangle drawn (approximate logical size), the file's actual pixel dimensions, and the ratio between them.** The expected ratio is the scale factor — this row IS the phase gate. An off-by-scale bug shows here as a file that is exactly 1/1.5 or 1/2 of the expected size, or a recording offset from the rectangle by the origin times the scale. |
   | 13 | **Region on a SECONDARY monitor, at a different scale from the primary** | With two monitors at different scales, select a region on the non-primary one. **Record which monitor the overlay appeared on, and whether the recorded content matches the rectangle.** This is the mixed-DPI case `core::screen_geometry`'s module doc warns about; if any row fails, it will be this one. |
   | 14 | **The overlay's own behaviour** | Open *Select region…* and, in separate attempts: (a) press Escape; (b) click once without dragging; (c) drag a tiny box (< 8 px); (d) drag from bottom-right to top-left. **Record for each:** whether the overlay closed, whether the panel came back with focus, and whether the picker shows a region. Expected: (a)(b)(c) no region, (d) the same region as an equivalent top-left-to-bottom-right drag. |
   | 15 | **The panel does not auto-hide during a selection** | With the panel open, start a region selection and leave the overlay up for ~10 s before dragging. **Record whether the panel is still open underneath afterwards.** This is `DIALOG_ACTIVE`; a failure here loses the picker's state mid-selection. |
   | 16 | **`WDA_EXCLUDEFROMCAPTURE`** (spec §5.3) | Start any screen capture with the buddy and the panel plainly in frame. **Record whether they are visible to you during the capture (they must be) and whether they appear in the played-back file (they must not).** Note the Windows build number. |
   | 17 | **The exclusion is lifted afterwards** | After the capture from row 16 stops, record the same screen with a **different** tool (Xbox Game Bar `Win+Alt+R`, or a Teams screen share). **Record whether Vault Buddy's windows are visible in THAT recording.** They must be. A failure here is the leaked-exclusion case the structural test exists to prevent, and is invisible from inside the app. |
   | 18 | **Resolution changed between selecting and starting** | Select a region near the right or bottom edge, then change the display resolution to something smaller **before** pressing Start. **Record the message.** Expected: a refusal naming the source as gone, not a started capture. |

4. Move **"Region capture; region accuracy across mixed-DPI monitors"** and
   **"`WDA_EXCLUDEFROMCAPTURE`"** out of the *"NOT reachable in Phase 2"*
   table — they are rows 11-18 now. Leave every other row of that table
   alone.
5. In *"Known Phase-2 absences"*, replace the "Vault Buddy's own windows DO
   appear in a Phase 2 recording" bullet with a line saying it is now
   excluded (row 16) and that a build older than Windows 10 2004 logs a
   warning and records with the buddy in frame, by design.
6. Add to *"Known absences"*: there is no keyboard-only way to draw a region
   (GAP-127), and the first frames of a capture may still contain our
   windows (GAP-124).

- [ ] **Step 6: Baselines — only if one actually moved**

If Step 1's `git diff --stat` shows no baseline change, **do nothing here
and say so in the report.** If a task did move one:
- `loc-baseline.json`: hand-edit the single entry, extend its `reason`
  string with what grew and why. **Never** `npm run check:loc -- --update`.
- `quality-baseline.json`: only if extraction genuinely failed to recover
  the metric. Record the reasoning in the `reason` string and **name it in
  the PR description** — Phase 2 had to do this once and made it visible.

Re-run the gate that owns each and confirm it passes:

```bash
cd /home/user/vault-buddy
rm -rf coverage && npm run check:loc; echo "loc exit=$?"
npm run check:quality; echo "quality exit=$?"
```

- [ ] **Step 7: Update the SDD ledger**

`.superpowers/sdd/progress.md` already carries Phases 1-2. Append a
`# Screen Capture Phase 3 (Region Overlay)` section in the same shape: the
plan path, the per-task lines with their commits and review verdicts, a
"Weak/vacuous tests the PLAN shipped" running list (be honest — this plan
will have some), and anything a later phase must carry forward.

- [ ] **Step 8: Run the whole gate set, end to end, and check every exit code**

```bash
cd /home/user/vault-buddy/src-tauri
cargo fmt --check; echo "fmt exit=$?"
cargo clippy --workspace --all-targets -- -D warnings; echo "clippy exit=$?"
cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "screen win exit=$?"
cargo clippy -p vault-buddy --lib --target x86_64-pc-windows-msvc -- -D warnings; echo "shell win exit=$?"
cargo clippy -p vault_buddy_capture --lib --target x86_64-pc-windows-msvc -- -D warnings; echo "capture win exit=$?"
cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen; echo "crates exit=$?"
cargo test -p vault-buddy --lib; echo "shell exit=$?"
cargo machete .; echo "machete exit=$?"
cargo deny check; echo "deny exit=$?"
cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94; echo "cov exit=$?"
cd /home/user/vault-buddy
rm -rf coverage && npm run lint; echo "lint exit=$?"
npm run check:loc; echo "loc exit=$?"
npm run check:quality; echo "quality exit=$?"
npm run test:coverage; echo "coverage exit=$?"
npm run build; echo "build exit=$?"
```

Every line must print `exit=0`. **Paste the real output into the report** —
not a summary of it, and never through a `grep`.

- [ ] **Step 9: Commit and push**

```bash
cd /home/user/vault-buddy
cat > /tmp/msg-t8.txt <<'MSG'
docs(screen): reconcile the docs with region capture and capture exclusion

AGENTS.md described three windows, a phase-2-only screen-capture domain and
a command count phase 3 moved; it is the file the next agent reads before
touching any of them, so it is reconciled here rather than later. The
overlay's own invariants -- positioned while hidden, one monitor because
one scale factor, DIALOG_ACTIVE for the duration, one cleanup site, a
bounded wait -- go in the window-system section beside the ones they
mirror.

GAP-111 closes its items 2 and 3 and keeps item 1 open with its reason
restated, so nobody reads the entry as finished. GAP-117 gains phase 3's
cfg(windows) arms. GAP-116 gains a sentence saying the new capture
exclusion does not supersede it: one keeps our windows out of a
recording's pixels, the other is about our windows being offered as
sources.

Four new residuals recorded rather than left to be rediscovered: the
exclusion is applied fire-and-forget so the first frames can still contain
our UI; region capture reads back the whole monitor and crops on the CPU
because that is the only place Linux can test it; an unanswered selection
cancels silently after two minutes; and there is no keyboard-only way to
draw a region.

The verification checklist becomes a running document across phases. The
phase-3 rows ask for measured values -- the drawn size, the file's actual
pixels and the ratio between them at each display scale -- because a
DPI bug shows as an exact factor and a tick would hide it. The open
phase-2 rows are carried forward, not closed.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
MSG
git add AGENTS.md docs/Gaps.md docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md .superpowers/sdd/progress.md
# add the baseline files ONLY if step 6 actually changed one
git commit -F /tmp/msg-t8.txt
git push -u origin claude/screen-capture-intake-g0j49q
```

**PR #79 is already open for this branch. Do not open a second one, and do
not open one against `main`.**

---

## Phase exit criteria

Phase 3 is done when every one of these holds — verified by running the
command, not by reading a report:

- [ ] All eight tasks are committed on `claude/screen-capture-intake-g0j49q`
      and pushed; PR #79 shows them and no second PR exists.
- [ ] `cargo fmt --check` clean.
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
      clean, and the same for `-p vault-buddy --lib` and `-p vault_buddy_capture --lib`.
- [ ] All five member crates' tests pass, plus `cargo test -p vault-buddy --lib`.
- [ ] `cargo machete .`, `cargo deny check` clean; **no new dependency**
      (the one `windows-sys` feature adds no crate).
- [ ] `cargo llvm-cov … --fail-under-lines 94` passes.
- [ ] `rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage && npm run build`
      passes in that order, with exactly one pre-existing lint warning in
      `src/main.ts`.
- [ ] Baselines are unchanged, or a moved one carries a written
      justification and is named in the PR description.
- [ ] `core::screen_geometry` — Phase 1's orphan — now has a production
      caller, through `region_commands::region_from_pick` and
      `source::resolve`'s Region arm.
- [ ] GAP-111 items 2 and 3 are closed and item 1 is explicitly still open.
- [ ] Every `cfg(windows)` addition is named in GAP-117.
- [ ] The verification checklist carries Phase 3's rows AND the four open
      Phase-2 rows. **Nobody is asked to run them yet** — the user's
      standing decision defers manual Windows testing until after the
      final phase.

## What Phase 4 needs from this phase

- **The `editor` window will fail `no_excluded_label_names_a_window_that_does_not_exist`'s
  sibling test** the moment it is declared in `tauri.conf.json` without
  being added to `capture_exclusion::EXCLUDED_LABELS`. That is the design:
  spec §5.3 lists `editor` among the excluded windows, and the test is how
  the list stays true.
- **`overlay` is now in the `default` capability's `windows` array.** The
  `editor` window must be added there too, or its webview gets no
  permissions at all.
- **`rootFor()` has a third explicit branch.** `editor → EditorRoot` follows
  the same shape.
- **The picker's `selectedId` can now hold an id that is not in
  `list_capture_sources`' reply.** Any future code that reasons about "the
  selected source" must ask about the region's monitor, as `loadSources`
  does, rather than assuming the id is enumerable.
- **`ResolvedSource` carries a crop origin.** Phase 5's export reads the
  staged file, whose dimensions are the REGION's, so nothing downstream
  needs the origin — but `select`'s frame plan must not re-derive the
  output size from the source's monitor.
