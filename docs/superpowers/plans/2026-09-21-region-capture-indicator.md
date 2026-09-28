# Region Capture Indicator Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** While a REGION screen capture runs, outline the recorded rectangle on the user's screen with a border that never appears in the recording and never blocks a click.

**Architecture:** A sixth window, `region-indicator`, declared in `tauri.conf.json`, transparent and click-through, shown by the capture start path and hidden by the one teardown chokepoint. It is in `capture_exclusion::EXCLUDED_LABELS`, which is what keeps the border out of the footage. Its geometry is a pure Linux-tested function; its pause colour is four app-wide events in a store-less Vue root.

**Tech Stack:** Rust (Tauri 2.11.5 shell), Vue 3 + Tailwind 4, Vitest, `cargo test`.

**Spec:** `docs/superpowers/specs/2026-09-20-region-capture-indicator-design.md`, **as amended 2026-09-21 (A1–A7)**. Where the body of that spec and an amendment disagree, the amendment wins — the body says so inline at each point.

## Global Constraints

- **`EXCLUDED_LABELS` is hardware-gated, not config-derived.** GAP-166: display affinity on a WebView2 window broke the editor's painting and other applications' pointer input. A window is excluded only with a hardware run behind it. The allowlist lives in the TEST, never in production (A2).
- **The window is DECLARED in `tauri.conf.json`, never built at runtime.** Every config-derived list test and `capture_exclusion`'s one-shot apply go blind to a runtime-built window simultaneously.
- **`available_monitors()` must never be called on the main thread** — it marshals to the event loop and blocks with no timeout. Deadlock (A7).
- **`region_commands::matches_display` is the ONE place the two monitor numbering schemes are joined.** Reuse it; never copy it. Phase 2 shipped a wrong-screen bug by crossing them.
- **A failed indicator never fails a capture.** Every step best-effort, `log::warn!`-logged. The one exception is ordering, not severity: if the window cannot be made click-through it is not shown at all.
- **Nonblank-LOC caps: Rust 800, frontend 500.** Never run `check:loc --update` or `check:quality --update`; if a cap would breach, extract. Current headroom on files touched: `screen_commands.rs` 720/800 is the tight one — it gains one line.
- **`ALL_WINDOW_LABELS` must keep `main` LAST** (`tray.rs`'s quit-walk test asserts it: the buddy's position is saved at quit and destroying it first races that save), and `POSITION_DENYLIST` must equal `ALL_WINDOW_LABELS` order minus `main`.
- **Commit messages:** Conventional Commits, body explains the *why* and the failure mode. No backticks in `git commit -m` — use a heredoc. Footer:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
  ```

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/core/src/screen_geometry.rs` (modify) | Gains `indicator_bounds` — the monitor-origin translation, pure and Linux-tested |
| `src-tauri/tauri.conf.json` (modify) | Declares the `region-indicator` window |
| `src-tauri/capabilities/default.json` (modify) | Grants that window permissions |
| `src-tauri/src/tray.rs` (modify) | The three config-derived label lists |
| `src-tauri/src/capture_exclusion.rs` (modify) | `EXCLUDED_LABELS` + its inverted pin becomes a hardware allowlist |
| `src-tauri/src/region_indicator.rs` (**create**) | The window's lifecycle: `indicator_rect` (pure gate), `show`, `hide`, and the structural pins |
| `src-tauri/src/region_commands.rs` (modify) | `matches_display` becomes `pub(crate)` |
| `src-tauri/src/screen_capture_worker.rs` (modify) | The ONE show site |
| `src-tauri/src/screen_commands.rs` (modify) | The ONE hide site |
| `src-tauri/src/lib.rs` (modify) | `mod region_indicator;` |
| `src/roots/RegionIndicatorRoot.vue` (**create**) | The border, and its four events |
| `src/roots/index.ts` (modify) | `rootFor("region-indicator")` |
| `tests/regionIndicatorRoot.test.ts` (**create**) | The root's colour transitions |
| `tests/main-root.test.ts` (modify) | The new label mapping |

---

### Task 1: `indicator_bounds` — the pure geometry

**Files:**
- Modify: `src-tauri/core/src/screen_geometry.rs`
- Test: same file (this repo keeps Rust unit tests inline)

**Interfaces:**
- Consumes: the existing `PhysicalRect { x: u32, y: u32, width: u32, height: u32 }` in this module.
- Produces: `pub fn indicator_bounds(monitor_origin: (i32, i32), rect: PhysicalRect) -> (i32, i32, u32, u32)` — returns `(x, y, width, height)`, position signed, size unsigned. Task 4 calls it.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `#[cfg(test)] mod tests` block in `src-tauri/core/src/screen_geometry.rs`:

```rust
    // The three tests below are deliberately ASYMMETRIC -- a non-zero
    // origin whose components differ, and a non-square rect. A square
    // rect at the origin distinguishes none of the three ways this can
    // be wrong (dropping the offset, adding it to the wrong axis,
    // transposing width and height), and the spec calls this addition
    // "the single most likely thing to get wrong".

    #[test]
    fn a_region_on_a_monitor_at_the_origin_is_the_rect_itself() {
        // The numbers the 2026-09-21 verification pass actually reported
        // for row 11, so a reader can line this up with the checklist.
        let rect = PhysicalRect { x: 770, y: 290, width: 1840, height: 1684 };
        assert_eq!(indicator_bounds((0, 0), rect), (770, 290, 1840, 1684));
    }

    #[test]
    fn the_monitor_origin_is_added_to_both_axes_independently() {
        let rect = PhysicalRect { x: 10, y: 20, width: 300, height: 400 };
        // Omitting the offset gives (10, 20); crossing the axes gives
        // (130, -1900); transposing the size gives (.., 400, 300).
        assert_eq!(indicator_bounds((-1920, 120), rect), (-1910, 140, 300, 400));
    }

    #[test]
    fn a_monitor_left_of_and_above_the_primary_keeps_negative_coordinates() {
        // A window's position is virtual-desktop coordinates, which are
        // signed. Returning unsigned here would put a secondary monitor's
        // border on the primary -- the exact confusion the indicator
        // exists to remove.
        let rect = PhysicalRect { x: 5, y: 7, width: 64, height: 32 };
        assert_eq!(indicator_bounds((-2560, -300), rect), (-2555, -293, 64, 32));
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri/core && cargo test screen_geometry::tests::` 
Expected: FAIL to COMPILE with `cannot find function 'indicator_bounds' in this scope`.

- [ ] **Step 3: Write the implementation**

Add to `src-tauri/core/src/screen_geometry.rs`, after `clamp_to_frame`:

```rust
/// Where an indicator window tracing `rect` sits on the virtual desktop.
///
/// `rect` is PHYSICAL pixels relative to its own monitor's origin (see this
/// module's doc for why that distinction is load-bearing); a window's
/// position is virtual-desktop coordinates. This translation is the whole of
/// the function.
///
/// It is a function rather than an inline addition because omitting the
/// origin fails SILENTLY in the worst way: the border lands on the primary
/// monitor while the capture records the secondary one -- precisely the
/// confusion the indicator exists to remove. Inline, nothing could pin it;
/// here, Linux can.
///
/// The position is SIGNED: a monitor left of or above the primary has
/// negative virtual-desktop coordinates. The size is the rect's own and is
/// never the monitor's.
pub fn indicator_bounds(monitor_origin: (i32, i32), rect: PhysicalRect) -> (i32, i32, u32, u32) {
    // `try_from` rather than `as`: a u32 above i32::MAX would wrap to a
    // negative offset and place the border on another monitor entirely.
    // Unreachable in practice -- `clamp_to_frame` has already bounded the
    // rect by the monitor's own size -- so this saturates rather than
    // returning an Option a caller would have to invent a behaviour for.
    let dx = i32::try_from(rect.x).unwrap_or(i32::MAX);
    let dy = i32::try_from(rect.y).unwrap_or(i32::MAX);
    (
        monitor_origin.0.saturating_add(dx),
        monitor_origin.1.saturating_add(dy),
        rect.width,
        rect.height,
    )
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri/core && cargo test screen_geometry::tests::`
Expected: PASS, three new tests among them.

- [ ] **Step 5: Mutation-check the asymmetry**

Temporarily change the implementation's first tuple field to `monitor_origin.1.saturating_add(dx)` (the crossed-axis defect). Run the tests again.
Expected: `the_monitor_origin_is_added_to_both_axes_independently` FAILS naming `(130, ...)` against `(-1910, ...)`.
Then restore the file byte-for-byte and re-run to confirm green. Verify with `git diff --stat` showing only the intended addition.

- [ ] **Step 6: Lint and commit**

```bash
cd src-tauri && cargo fmt --check && cd core && cargo clippy --all-targets -- -D warnings
cd /home/user/vault-buddy && git add src-tauri/core/src/screen_geometry.rs
```
Commit with a heredoc message titled `feat(core): add indicator_bounds, the region indicator's monitor-origin translation`, explaining in the body that the spec named this addition the single most likely thing to get wrong and the most silent when wrong, and that the fixtures are asymmetric so the three distinct defects each fail with a distinct number.

---

### Task 2: Declare the sixth window and place it in every list

This task must land as one commit: a window declared in the config but missing from `rootFor` renders `BuddyRoot`, i.e. a second buddy inside the indicator window; and missing from any list reddens a `tray.rs` test.

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/tray.rs:17-33`
- Modify: `src-tauri/src/capture_exclusion.rs`
- Create: `src/roots/RegionIndicatorRoot.vue`
- Modify: `src/roots/index.ts`
- Modify: `tests/main-root.test.ts`

**Interfaces:**
- Produces: the window label `"region-indicator"`, used by Task 4's `INDICATOR_LABEL`; and `RegionIndicatorRoot.vue`, extended by Task 3.

- [ ] **Step 1: Write the failing tests**

In `tests/main-root.test.ts`, add the import and extend the mapping test:

```ts
import RegionIndicatorRoot from "../src/roots/RegionIndicatorRoot.vue";
```
```ts
  it("mounts the indicator root for the region-indicator window", () => {
    expect(rootFor("region-indicator")).toBe(RegionIndicatorRoot);
  });
```

In `src-tauri/src/capture_exclusion.rs`, REPLACE the whole `only_the_buddy_is_excluded_from_capture` test with:

```rust
    /// The labels a HARDWARE run has cleared for display affinity, and the
    /// evidence for each. Production is checked against THIS, never the
    /// other way round: a window added to `tauri.conf.json` must not become
    /// excluded by being declared, which is the property GAP-166 bought and
    /// the one worth keeping.
    ///
    /// - `main` -- the buddy. Three one-variable rebuilds on 2026-09-21:
    ///   {all five windows} stopped the editor painting and stopped
    ///   Explorer's and Notepad's toolbars taking pointer input until the
    ///   process exited; a no-op `set_affinity` was clean; `main` alone was
    ///   clean WITH the buddy absent from a Win+Shift+S snip during the
    ///   capture and from the played-back file. Checklist rows 16, 17.
    /// - `region-indicator` -- the region border, whose whole value depends
    ///   on this. The premise probe on the same machine excluded a SECOND
    ///   window of this one's exact shape class (`panel`: transparent,
    ///   undecorated, always-on-top, skipTaskbar, focus:false) across
    ///   several captures -- Explorer's and Notepad's toolbars survived and
    ///   the window painted normally (docs/Gaps.md GAP-165). Verified for
    ///   this window itself by checklist rows 44, 45 and 46.
    const HARDWARE_CLEARED_FOR_AFFINITY: [&str; 2] = ["main", "region-indicator"];

    #[test]
    fn only_hardware_cleared_windows_are_excluded_from_capture() {
        let declared = declared_window_labels();
        for label in HARDWARE_CLEARED_FOR_AFFINITY {
            assert!(
                declared.iter().any(|d| d == label),
                "tauri.conf.json no longer declares {label:?}"
            );
            assert!(
                EXCLUDED_LABELS.contains(&label),
                "{label:?} has a hardware run behind it and must be excluded from capture"
            );
        }
        assert_eq!(
            EXCLUDED_LABELS.len(),
            HARDWARE_CLEARED_FOR_AFFINITY.len(),
            "EXCLUDED_LABELS is {EXCLUDED_LABELS:?}, which is not the hardware-cleared set \
             {HARDWARE_CLEARED_FOR_AFFINITY:?}"
        );
        for label in declared
            .iter()
            .filter(|d| !HARDWARE_CLEARED_FOR_AFFINITY.contains(&d.as_str()))
        {
            assert!(
                !EXCLUDED_LABELS.contains(&label.as_str()),
                "window {label:?} is in EXCLUDED_LABELS with no hardware run behind it. \
                 Setting display affinity on a WebView2 window broke its painting and other \
                 applications' input until the process exited (GAP-166); add it to \
                 HARDWARE_CLEARED_FOR_AFFINITY only with checklist evidence"
            );
        }
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test -p vault-buddy --lib capture_exclusion`
Expected: FAIL — `tauri.conf.json no longer declares "region-indicator"`.

Run: `cd /home/user/vault-buddy && npx vitest run tests/main-root.test.ts`
Expected: FAIL to resolve `../src/roots/RegionIndicatorRoot.vue`.

- [ ] **Step 3: Declare the window**

In `src-tauri/tauri.conf.json`, add to `app.windows` immediately AFTER the `editor` entry:

```json
    {
      "label": "region-indicator",
      "title": "Vault Buddy — Recording Region",
      "visible": false,
      "transparent": true,
      "decorations": false,
      "alwaysOnTop": true,
      "resizable": false,
      "skipTaskbar": true,
      "shadow": false,
      "focus": false
    }
```

In `src-tauri/capabilities/default.json`, add `"region-indicator"` to the `windows` array.

- [ ] **Step 4: Put it in every list**

`src-tauri/src/tray.rs` — note `main` STAYS LAST in `ALL_WINDOW_LABELS`, and `POSITION_DENYLIST` must equal that order minus `main`:

```rust
pub const ALL_WINDOW_LABELS: [&str; 6] =
    ["panel", "bubble", "overlay", "editor", "region-indicator", "main"];
```
```rust
pub const COMPANION_LABELS: [&str; 5] =
    ["panel", "bubble", "overlay", "region-indicator", "main"];
```
```rust
pub const POSITION_DENYLIST: [&str; 5] =
    ["panel", "bubble", "overlay", "editor", "region-indicator"];
```

`src-tauri/src/capture_exclusion.rs` — the constant, and its doc comment updated to stop saying "the buddy, and ONLY the buddy":

```rust
/// The windows a HARDWARE run has cleared for `WDA_EXCLUDEFROMCAPTURE`.
///
/// Through phases 3-5 this was every window the app declares, pinned to
/// `tauri.conf.json` by a test that failed until a new window was added
/// here. GAP-166 inverted that on hardware: `SetWindowDisplayAffinity` on a
/// WebView2-hosting window stopped the editor painting and stopped OTHER
/// applications' toolbars taking pointer input until the process exited,
/// and `main` alone was clean.
///
/// `region-indicator` joined on its own evidence, not by being declared: a
/// probe excluding a second window of its exact shape class was clean
/// across several captures (docs/Gaps.md GAP-165). Which property of the
/// other four made them hazardous is still not established, so the test
/// below checks this constant against an allowlist that names each
/// window's evidence -- a new window is NOT excluded by declaring it.
pub(crate) const EXCLUDED_LABELS: &[&str] = &["main", "region-indicator"];
```

- [ ] **Step 5: Create the root and map it**

Create `src/roots/RegionIndicatorRoot.vue`:

```vue
<script setup lang="ts">
/**
 * The region-capture indicator (GAP-165). This root runs in the
 * `region-indicator` window, which Rust has already sized and positioned
 * over the recorded rectangle while it was still hidden — so this viewport
 * IS the region, and the border below simply traces its own edges. There is
 * no arithmetic here on purpose: the translation from a monitor-local rect
 * to a virtual-desktop position is `core::screen_geometry::indicator_bounds`,
 * where Linux can test it.
 *
 * Like `RegionRoot`, this root wires NO Pinia store, and that is not an
 * omission: AGENTS.md's per-window `init()` rule exists because a store
 * mirroring Rust state is dead in any webview that never subscribed. This
 * root mirrors one boolean with no derived state.
 *
 * The window is excluded from capture (`capture_exclusion::EXCLUDED_LABELS`),
 * which is the whole reason a border drawn OVER the recorded area is free:
 * the user sees it and the recording does not contain it.
 */
</script>

<template>
  <!-- `pointer-events-none` is belt-and-braces only. The real click-through
       is Rust's `set_ignore_cursor_events(true)`, applied before every show:
       a CSS rule cannot stop a transparent always-on-top window from taking
       the click at the OS level. -->
  <div
    data-testid="region-indicator-border"
    class="pointer-events-none fixed inset-0 border-2 border-recording"
  ></div>
</template>
```

In `src/roots/index.ts`, add the import and the branch BEFORE the `main` fallback:

```ts
import RegionIndicatorRoot from "./RegionIndicatorRoot.vue";
```
```ts
  if (label === "region-indicator") return RegionIndicatorRoot;
```

- [ ] **Step 6: Run every affected suite**

```bash
cd src-tauri && cargo test -p vault-buddy --lib
cd /home/user/vault-buddy && npx vitest run tests/main-root.test.ts
```
Expected: both PASS. In particular `the_quit_walk_destroys_every_configured_window`, `the_hide_walk_covers_every_companion_window_and_not_the_editor`, `only_the_buddy_persists_a_position` and `only_hardware_cleared_windows_are_excluded_from_capture` all green.

- [ ] **Step 7: Mutation-check the allowlist guard**

Temporarily add `"editor"` to `EXCLUDED_LABELS` (NOT to `HARDWARE_CLEARED_FOR_AFFINITY`). Run `cargo test -p vault-buddy --lib capture_exclusion`.
Expected: FAIL naming `"editor"` and GAP-166. Restore byte-for-byte, re-run green, confirm with `git diff`.

- [ ] **Step 8: Commit**

```bash
cd /home/user/vault-buddy && npm run check:loc
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json src-tauri/src/tray.rs src-tauri/src/capture_exclusion.rs src/roots/RegionIndicatorRoot.vue src/roots/index.ts tests/main-root.test.ts
```
Commit titled `feat(screen): declare the region-indicator window and clear it for capture exclusion`. The body must record that the exclusion pin moved from "only the buddy" to a test-side allowlist naming each window's hardware evidence, and why that keeps "declaring a window does not exclude it".

---

### Task 3: The border's pause colour, and its reset

**Files:**
- Modify: `src/roots/RegionIndicatorRoot.vue`
- Create: `tests/regionIndicatorRoot.test.ts`

**Interfaces:**
- Consumes: Task 2's `RegionIndicatorRoot.vue`.
- Produces: nothing other tasks read.

- [ ] **Step 1: Write the failing test**

Create `tests/regionIndicatorRoot.test.ts`:

```ts
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Captured so a test can drive the screen lifecycle the way Rust does.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

import RegionIndicatorRoot from "../src/roots/RegionIndicatorRoot.vue";

enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const k of Object.keys(listeners)) delete listeners[k];
});

async function mountRoot() {
  const w = mount(RegionIndicatorRoot);
  await flushPromises();
  return w;
}

const border = (w: ReturnType<typeof mount>) =>
  w.get('[data-testid="region-indicator-border"]').classes();

describe("RegionIndicatorRoot", () => {
  it("starts in the recording colour", async () => {
    const w = await mountRoot();
    expect(border(w)).toContain("border-recording");
    expect(border(w)).not.toContain("border-amber-400");
  });

  it("turns amber while paused and back on resume", async () => {
    const w = await mountRoot();
    listeners["screen:paused"]({ payload: { atMs: 1000 } });
    await flushPromises();
    expect(border(w)).toContain("border-amber-400");
    expect(border(w)).not.toContain("border-recording");

    listeners["screen:resumed"]({ payload: { pausedTotalMs: 500 } });
    await flushPromises();
    expect(border(w)).toContain("border-recording");
  });

  // The defect this test exists for: this window is declared in the config
  // and hidden-not-destroyed, so its webview mounts ONCE per process. With
  // only the pause edges wired, a capture paused and then stopped leaves the
  // border amber, and the NEXT capture comes up amber while genuinely
  // recording -- a silent lie in the one surface whose whole job is telling
  // the truth about what is being recorded. See spec amendment A5.
  it("resets to the recording colour when a paused capture stops", async () => {
    const w = await mountRoot();
    listeners["screen:paused"]({ payload: { atMs: 1000 } });
    await flushPromises();
    expect(border(w)).toContain("border-amber-400");

    listeners["screen:stopped"]({ payload: { base: "cap one" } });
    await flushPromises();
    expect(border(w)).toContain("border-recording");
    expect(border(w)).not.toContain("border-amber-400");
  });

  it("resets to the recording colour when a paused capture fails", async () => {
    const w = await mountRoot();
    listeners["screen:paused"]({ payload: { atMs: 1000 } });
    await flushPromises();
    listeners["screen:failed"]({ payload: { message: "gone" } });
    await flushPromises();
    expect(border(w)).toContain("border-recording");
  });

  it("stops listening when unmounted", async () => {
    const w = await mountRoot();
    expect(Object.keys(listeners).sort()).toEqual([
      "screen:failed",
      "screen:paused",
      "screen:resumed",
      "screen:stopped",
    ]);
    w.unmount();
    await flushPromises();
    expect(Object.keys(listeners)).toHaveLength(0);
  });
});
```

- [ ] **Step 2: Run it to verify it fails**

Run: `npx vitest run tests/regionIndicatorRoot.test.ts`
Expected: FAIL — `listeners["screen:paused"] is not a function` (the root subscribes to nothing yet).

- [ ] **Step 3: Implement the four listeners**

Replace the `<script setup>` block in `src/roots/RegionIndicatorRoot.vue` (keep the whole doc comment, add below it):

```ts
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const paused = ref(false);

/**
 * Four events, not two. `screen:paused` / `screen:resumed` are the
 * transitions; `screen:stopped` and `screen:failed` are what returns the
 * border to the recording colour between captures.
 *
 * Without the latter two, this window's once-per-process mount makes the
 * colour outlive its capture: pause a region capture, stop it while paused,
 * start another, and the border comes up amber on a capture that is
 * genuinely recording. Every path that showed this window is past the
 * capture's commit point, so one of `stopped` / `failed` always arrives.
 * Spec amendment A5.
 */
const unlisteners: UnlistenFn[] = [];

onMounted(async () => {
  unlisteners.push(
    await listen("screen:paused", () => {
      paused.value = true;
    }),
    await listen("screen:resumed", () => {
      paused.value = false;
    }),
    await listen("screen:stopped", () => {
      paused.value = false;
    }),
    await listen("screen:failed", () => {
      paused.value = false;
    }),
  );
});

onBeforeUnmount(() => {
  for (const off of unlisteners) off();
  unlisteners.length = 0;
});

/**
 * The capture bar's own status-dot colours, deliberately -- one visual
 * language across the buddy, the bar and this border. `ScreenCaptureBar`
 * uses a bespoke dot rather than the `StatusDot` primitive because that
 * primitive has no amber tone; this follows the bar, not the primitive, for
 * the same reason.
 */
const borderClass = computed(() =>
  paused.value ? "border-amber-400" : "border-recording",
);
```

And bind it in the template, replacing the static class:

```html
  <div
    data-testid="region-indicator-border"
    class="pointer-events-none fixed inset-0 border-2"
    :class="borderClass"
  ></div>
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `npx vitest run tests/regionIndicatorRoot.test.ts`
Expected: PASS, 5 tests.

- [ ] **Step 5: Mutation-check the reset**

Temporarily delete the `screen:stopped` listener. Run the suite.
Expected: `resets to the recording colour when a paused capture stops` FAILS. Restore byte-for-byte and re-run green.

- [ ] **Step 6: Lint and commit**

```bash
npm run lint && npm run check:loc
git add src/roots/RegionIndicatorRoot.vue tests/regionIndicatorRoot.test.ts
```
Commit titled `feat(ui): the region indicator's border tracks pause, and resets between captures`, with the once-per-process mount defect named in the body.

---

### Task 4: The Rust lifecycle — one show site, one hide site

**Files:**
- Create: `src-tauri/src/region_indicator.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod region_indicator;` beside the other `mod` lines)
- Modify: `src-tauri/src/region_commands.rs` (`matches_display` → `pub(crate)`)
- Modify: `src-tauri/src/screen_capture_worker.rs` (the show site, beside `capture_exclusion::apply`)
- Modify: `src-tauri/src/screen_commands.rs` (the hide site, inside `clear_active_screen`)

**Interfaces:**
- Consumes: `vault_buddy_core::screen_geometry::indicator_bounds` (Task 1); `region-indicator` as a declared window (Task 2); `region_commands::matches_display(name: Option<&str>, display: usize) -> bool`.
- Produces: `pub(crate) fn indicator_rect(source: &SourceId) -> Option<RegionSource>`, `pub(crate) fn show(app: &AppHandle, source: &SourceId)`, `pub(crate) fn hide(app: &AppHandle)`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/region_indicator.rs` containing ONLY the test module for now, so the tests fail on missing items rather than on a missing file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::structural_scan::{call_sites, offset_of, production_half};
    // `use super::*` already brings `SourceId` and `RegionSource` in from the
    // parent's imports; only `PhysicalRect` is new here.
    use vault_buddy_core::screen_geometry::PhysicalRect;

    fn region(monitor: usize) -> SourceId {
        SourceId::Region(RegionSource {
            monitor,
            rect: PhysicalRect { x: 10, y: 20, width: 300, height: 400 },
        })
    }

    // The gate, extracted precisely so it can be tested off Windows: `show`
    // itself needs an AppHandle and executes in no automated test anywhere.
    // A border drawn around a FULL-SCREEN capture is cosmetic-but-confusing
    // and would otherwise surface only on hardware.
    #[test]
    fn only_a_region_source_raises_the_indicator() {
        assert!(indicator_rect(&SourceId::Screen(1)).is_none());
        assert!(indicator_rect(&SourceId::Window(0x1234)).is_none());
        let got = indicator_rect(&region(2)).expect("a region raises the indicator");
        assert_eq!(got.monitor, 2);
        assert_eq!(got.rect.width, 300);
    }

    // The `capture_exclusion` pattern, and for the identical reason: a
    // leaked indicator is a border the user cannot dismiss, with no capture
    // behind it, and a file-granular assertion would let both natural
    // relocations pass.
    #[test]
    fn the_indicator_is_shown_and_hidden_from_exactly_one_place_each() {
        let shows = call_sites("region_indicator::show(");
        let hides = call_sites("region_indicator::hide(");
        assert_eq!(shows.len(), 1, "expected exactly one show site, found {shows:?}");
        assert_eq!(hides.len(), 1, "expected exactly one hide site, found {hides:?}");
        assert!(
            shows[0].ends_with("screen_capture_worker.rs"),
            "the show must sit in start_screen_capture_blocking, beside the exclusion apply \
             and after the reservation -- but it is in {}",
            shows[0]
        );
        assert!(
            hides[0].ends_with("screen_commands.rs"),
            "the hide must sit in clear_active_screen, the one chokepoint every teardown \
             funnels through -- but it is in {}",
            hides[0]
        );

        let worker = production_half(include_str!("screen_capture_worker.rs"));
        let blocking_start = offset_of(worker, "fn start_screen_capture_blocking(");
        let show_call = offset_of(worker, "crate::region_indicator::show(");
        let after_blocking_start = offset_of(worker, "\nfn describe_screen_error(");
        assert!(
            blocking_start < show_call && show_call < after_blocking_start,
            "the show must sit INSIDE start_screen_capture_blocking. In the async tail it can \
             land after a self-finalized capture has already hidden the indicator, leaving a \
             border on the desktop with nothing recording"
        );

        let commands = production_half(include_str!("screen_commands.rs"));
        let chokepoint = offset_of(commands, "fn clear_active_screen(");
        let hide_call = offset_of(commands, "crate::region_indicator::hide(");
        let after_chokepoint = offset_of(commands, "\npub fn is_capturing(");
        assert!(
            chokepoint < hide_call && hide_call < after_chokepoint,
            "the hide must sit INSIDE clear_active_screen. Moved into stop_screen_capture it \
             is skipped entirely by a self-finalized capture, stranding the border"
        );
    }

    // A7: enumerating monitors inside a main-thread closure deadlocks --
    // `available_monitors` marshals to the event loop and blocks on the
    // reply with no timeout, so the main thread would wait on a reply only
    // it can produce. The symptom is a HANG on every region capture start:
    // no crash, no log line, no crash record. Structural because no test on
    // any platform executes this function.
    #[test]
    fn the_monitor_enumeration_happens_before_the_main_thread_closure() {
        let src = production_half(include_str!("region_indicator.rs"));
        let enumerate = offset_of(src, "available_monitors()");
        let marshal = offset_of(src, "run_on_main_thread(");
        assert!(
            enumerate < marshal,
            "available_monitors() must be called on the CALLING thread (a worker: \
             start_screen_capture_blocking runs under spawn_blocking) and never from inside \
             run_on_main_thread -- it marshals to the event loop and blocks with no timeout, \
             so the main thread would deadlock waiting on itself. Spec amendment A7"
        );
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Add `mod region_indicator;` to `src-tauri/src/lib.rs` beside the other module declarations, then run:
`cd src-tauri && cargo test -p vault-buddy --lib region_indicator`
Expected: FAIL to COMPILE — `cannot find function 'indicator_rect'`, `cannot find type 'SourceId'`.

- [ ] **Step 3: Write the implementation**

Prepend to `src-tauri/src/region_indicator.rs`, above the test module:

```rust
//! The region-capture indicator (GAP-165, spec
//! `2026-09-20-region-capture-indicator-design.md` as amended 2026-09-21).
//!
//! While a REGION capture runs, the recorded rectangle is outlined on the
//! user's screen. Windows' own yellow border surrounds the whole monitor,
//! because WGC really is capturing the whole monitor and
//! `convert::bgra_crop_to_nv12` crops on the CPU -- so the border Windows
//! draws is truthful about what it captures and useless about what we
//! record. This one is ours.
//!
//! **Why drawing over the recorded area is free.** The window is in
//! `capture_exclusion::EXCLUDED_LABELS`, so it is invisible to screen
//! capture while fully visible to the user. That membership is hardware
//! evidence, not a declaration -- see that module and docs/Gaps.md GAP-165.
//!
//! **Paired state, one site each way**, the `capture_exclusion` shape and
//! for the same reason: a leaked indicator is a border the user cannot
//! dismiss with no capture behind it, and the natural relocations of both
//! calls leak permanently while keeping the counts right. Pinned by the
//! structural tests below.
//!
//! **Best-effort throughout.** A failed indicator must never fail a
//! capture: the recording proceeds without a border and a `log::warn!`
//! records why. This differs from `region:begin`'s hard-error treatment
//! deliberately -- an un-armed overlay IS an unusable full-screen scrim,
//! whereas a missing border merely returns the user to today's behaviour.

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize};
use vault_buddy_core::screen_geometry::indicator_bounds;
use vault_buddy_screen::region::RegionSource;
use vault_buddy_screen::source::SourceId;

const INDICATOR_LABEL: &str = "region-indicator";

/// The gate: only a REGION capture raises an indicator.
///
/// Windows' own border is already truthful for a screen or a window
/// capture -- it surrounds exactly what is being captured -- so a second
/// border there would be noise. Extracted as a pure function because
/// `show` needs an `AppHandle` and therefore executes in no automated test
/// on any platform; this way the gate itself is pinned on Linux.
pub(crate) fn indicator_rect(source: &SourceId) -> Option<RegionSource> {
    match source {
        SourceId::Region(r) => Some(*r),
        SourceId::Screen(_) | SourceId::Window(_) => None,
    }
}

/// Raise the border over the recorded rectangle. THE ONE SHOW SITE is
/// `screen_capture_worker`'s start path, beside the exclusion apply.
///
/// **Two threads, deliberately (spec amendment A7).** The caller is a
/// worker -- `start_screen_capture_blocking` runs under `spawn_blocking` --
/// which is the only reason `available_monitors` is safe here: it marshals
/// to the event loop and blocks on the reply with no timeout, so calling it
/// from inside the closure below would be the main thread waiting on a
/// reply only it can produce. Everything that needs the event loop is
/// computed FIRST and carried in as four plain numbers.
pub(crate) fn show(app: &AppHandle, source: &SourceId) {
    let Some(region) = indicator_rect(source) else {
        return;
    };
    let monitors = match app.available_monitors() {
        Ok(m) => m,
        Err(e) => {
            log::warn!("region indicator: could not read the display layout: {e}");
            return;
        }
    };
    // The ONE join between Tauri's monitor naming and windows-capture's
    // index lives in `region_commands`; never a second copy, because its
    // own doc records that phase 2 shipped a wrong-screen bug by crossing
    // exactly these two schemes.
    let Some(monitor) = monitors
        .iter()
        .find(|m| crate::region_commands::matches_display(m.name().map(String::as_str), region.monitor))
    else {
        log::warn!(
            "region indicator: no monitor for GDI display {}; the border is skipped",
            region.monitor
        );
        return;
    };
    let origin = *monitor.position();
    let (x, y, w, h) = indicator_bounds((origin.x, origin.y), region.rect);

    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || place_and_show(&handle, x, y, w, h)) {
        log::warn!("region indicator: could not reach the main thread: {e}");
    }
}

/// The main-thread half: window calls only.
///
/// **The ordering is a rule, not a preference.** Click-through FIRST, and a
/// failure there returns without showing anything: a transparent
/// always-on-top window that takes clicks swallows every one of them over
/// the whole region for the whole capture, which is strictly worse than no
/// indicator at all. Then size and position WHILE HIDDEN, then show -- the
/// discipline every companion window follows, because a visible window
/// repaints its stale last frame at the new bounds for a frame.
fn place_and_show(app: &AppHandle, x: i32, y: i32, w: u32, h: u32) {
    let Some(window) = app.get_webview_window(INDICATOR_LABEL) else {
        log::warn!("region indicator: window `{INDICATOR_LABEL}` is not declared");
        return;
    };
    if let Err(e) = window.set_ignore_cursor_events(true) {
        log::warn!("region indicator: could not make the border click-through, so it is not shown: {e}");
        return;
    }
    if let Err(e) = window.set_size(PhysicalSize::new(w, h)) {
        log::warn!("region indicator: could not size the border: {e}");
        return;
    }
    if let Err(e) = window.set_position(PhysicalPosition::new(x, y)) {
        log::warn!("region indicator: could not place the border: {e}");
        return;
    }
    if let Err(e) = window.show() {
        log::warn!("region indicator: could not show the border: {e}");
    }
}

/// Take the border down. THE ONE HIDE SITE is
/// `screen_commands::clear_active_screen`.
///
/// **Unconditional**, exactly like the exclusion clear beside it: hiding an
/// indicator that was never shown is a no-op, which is strictly safer than
/// a conditional that could be wrong in the other direction and strand a
/// border on the user's desktop with no capture behind it.
pub(crate) fn hide(app: &AppHandle) {
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window(INDICATOR_LABEL) {
            if let Err(e) = window.hide() {
                log::warn!("region indicator: could not hide the border: {e}");
            }
        }
    }) {
        log::warn!("region indicator: could not reach the main thread to hide: {e}");
    }
}
```

- [ ] **Step 4: Widen `matches_display` and wire both sites**

In `src-tauri/src/region_commands.rs`, change the signature (keep the doc comment, it is the reason this is shared):

```rust
pub(crate) fn matches_display(name: Option<&str>, display: usize) -> bool {
```

In `src-tauri/src/screen_capture_worker.rs`, immediately AFTER the existing `crate::capture_exclusion::apply(app);` line:

```rust
    // The region border (GAP-165), beside the exclusion for the same
    // reasons: both are capture-scoped window side effects, and both belong
    // after the reservation so every failure below funnels through the same
    // teardown. Gated inside `show` on the source being a REGION -- Windows'
    // own border is already truthful for a screen or window capture.
    crate::region_indicator::show(app, &parsed);
```

In `src-tauri/src/screen_commands.rs`, inside `clear_active_screen`, immediately AFTER `crate::capture_exclusion::clear(app);`:

```rust
    // Unconditional, like the exclusion clear above: hiding a border that
    // was never raised is a no-op, and the other direction strands one on
    // the user's desktop with nothing recording (GAP-165).
    crate::region_indicator::hide(app);
```

- [ ] **Step 5: Run the tests to verify they pass**

```bash
cd src-tauri && cargo test -p vault-buddy --lib
cargo clippy -p vault_buddy_screen --target x86_64-pc-windows-msvc
```
Expected: all shell tests PASS; the Windows-target screen clippy clean.

If `parsed` fails to compile as `&parsed` (a move), check `SourceId`'s derives — it is `Copy` via `RegionSource`; if the borrow is rejected because `parsed` was moved earlier in the function, move the `show` call above that use rather than cloning.

- [ ] **Step 6: Mutation-check both structural pins**

1. Move the show call out of `start_screen_capture_blocking` into `start_screen_capture`'s async tail in `screen_commands.rs`. Run `cargo test -p vault-buddy --lib region_indicator`. Expected: FAIL naming the self-finalize leak. Restore.
2. In `region_indicator.rs`, move the `available_monitors()` call inside the `run_on_main_thread` closure. Run the same. Expected: `the_monitor_enumeration_happens_before_the_main_thread_closure` FAILS naming A7. Restore.
Verify both restorations with `git diff` before continuing.

- [ ] **Step 7: Full Rust gates and commit**

```bash
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test -p vault-buddy --lib
cd /home/user/vault-buddy && npm run check:loc
```
Commit titled `feat(screen): raise a border over the recorded region while a region capture runs`. The body must name the A7 deadlock, the one-site-each-way pinning, and that a window that cannot be made click-through is never shown.

---

### Task 5: Documentation — AGENTS.md, Gaps.md, and three checklist rows

**Files:**
- Modify: `AGENTS.md`
- Modify: `docs/Gaps.md` (GAP-165)
- Modify: `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`

- [ ] **Step 1: Add the nine verification rows**

The spec's own `Verification checklist rows to add` table has SEVEN checks and
amendment A6 adds TWO more. All nine become rows; none is collapsed, because a
row that bundles several checks is a row whose tick hides which check was
actually run — the failure the checklist's own Phase 3 preamble warns about.

Append to the Phase 3 table. The checklist's highest numbered row today is 43,
so these are **44–52**. **Measure the resulting total, never increment it:**

```bash
python3 -c "import re;print(sum(1 for l in open('docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md') if re.match(r'\| \*{0,2}\d+[a-z]?\*{0,2} \|', l)))"
```

| # | Check | Steps |
| --- | --- | --- |
| 44 | **Other applications survive a region capture** | Record a region, stop it, and with Vault Buddy **still running** click Explorer's toolbar and Notepad's menu bar. **Record whether each responds.** GAP-166's exact symptom against the sixth excluded window. A failure here means the indicator loses its exclusion, and the feature with it. |
| 45 | **The panel is not hidden when the indicator appears** | With the panel open, start a region capture and touch nothing for ~10 s. **Record whether the panel is still open.** `show()` on an always-on-top window may activate it despite `focus: false`; the activation would blur the panel and `schedule_focus_out_check` would hide it mid-capture. Undeterminable off Windows. |
| 46 | **The border appears and traces the rectangle** | Record a region ~in the middle of a monitor. **Record** whether a border appears, and whether it traces the drawn rectangle **on all four edges** — not just that it "looks right". |
| 47 | **The border is click-through** | With the capture running, click something underneath the border's edge, and something inside the region. **Record whether each click reached the application underneath.** |
| 48 | **The border is not in the recording** | Play back the staged file. **Record whether the border appears in any frame.** This is what the exclusion buys; without it the feature is worse than nothing. |
| 49 | **The border is on the right monitor** | Record a region on a NON-primary monitor. **Record which monitor the border appeared on.** This is `indicator_bounds`' monitor offset; a failure puts the border on the primary. **Expected BLOCKED on the current verification machine** (single monitor), like row 13 — record it as blocked, not as passed. |
| 50 | **The border tracks pause** | Pause, then resume. **Record the border's colour in each state**, and whether it stayed in place. |
| 51 | **The border goes when the capture does** | Stop the capture. **Record whether the border disappeared.** Then repeat for a capture that self-finalizes (close the recorded source) and one that fails. All three teardowns funnel through `clear_active_screen`; the self-finalize is the one no other path exercises. |
| 52 | **Display scaling** | Repeat row 46 at 125 / 150 / 200 %. **Record the drawn rectangle and whether the border still traces it** at each. Pairs with row 12 — the same arithmetic, seen from the user's side. |

Then add a sentence to the Phase 3 preamble: rows **16, 17 and 43 must be
RE-RUN** on this build, because a sixth excluded window changes exactly what
they verify and their earlier passes do not carry over.

- [ ] **Step 2: Update GAP-165**

The entry stays open — the code landing is not the verification. Make exactly
these edits:

1. Heading: `### GAP-165 · Medium · An APPROVED design for the region-capture indicator was never implemented, and nothing tracked that` becomes `### GAP-165 · Medium · The region-capture indicator: approved 2026-09-20, unimplemented and untracked for a day, implemented 2026-09-21 — rows 44-52 unrun`.
2. Keep the whole premise-probe section verbatim. It is the evidence
   `capture_exclusion`'s test comment cites.
3. Replace the "Measured on this tree" bullet list (which asserts no plan, no
   `region-indicator` in the tree, and no entry here) with what is now true:
   the plan is `docs/superpowers/plans/2026-09-21-region-capture-indicator.md`,
   the window is declared, and the five list memberships exist. State plainly
   that **nothing about the border has been observed on hardware yet** —
   rows 44-52 — and that row 49 is expected BLOCKED on a single-monitor
   machine, so the monitor-offset arithmetic stays verified only by
   `indicator_bounds`' own Linux tests.
4. Keep the residual about no mechanism linking a spec to the branch that
   executed it. That is unfixed and is what let this happen.

- [ ] **Step 3: Update AGENTS.md**

Four places. **Measure each count from the tree; never increment the number
already written** — AGENTS.md's own IPC-command sentence has been wrong four
times that way.

1. **Repository map** — add `region_indicator.rs` to the `src-tauri/src/`
   listing, beside `region_commands.rs`, described as the indicator's
   show/hide lifecycle (a WINDOW lifecycle, like `region_commands`, not a
   capture one).
2. **The window system** — this is the biggest edit. Its opening says "Five
   windows. FOUR of them are the always-on-top transparent companions"; that
   becomes six and five. Add a `region-indicator` bullet: transparent,
   click-through, never focused, shown only while a REGION capture runs,
   positioned and sized while hidden like every companion. Update all four
   label-list constants and their sizes. The `capture_exclusion::EXCLUDED_LABELS`
   paragraph currently reads "**`["main"]`, the buddy and ONLY the buddy**" —
   it becomes the hardware-cleared set of two, keeping the sentence that a
   window declared in the config is NOT excluded until a hardware run says it
   can be, and citing GAP-165's probe for the indicator's own evidence.
3. **The screen-capture section** — a bullet for the indicator: what it is,
   why Windows' own border cannot be retargeted, that the border is absent
   from the footage only because of the exclusion, and A7's two-thread split
   with the deadlock it avoids named.
4. **Frontend state** — add `region-indicator` → `RegionIndicatorRoot` to the
   `rootFor` list, and change "**`RegionRoot` and `EditorRoot` install no
   store** — they are the TWO roots that mirror no Rust state" to three,
   noting the indicator mirrors one boolean from four app-wide events.

- [ ] **Step 4: Commit**

Commit titled `docs: record the region-capture indicator and its three verification rows`.

---

### Task 6: Full gate run and push

- [ ] **Step 1: Rust gates**

```bash
cd src-tauri && cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p vault_buddy_screen --target x86_64-pc-windows-msvc
cargo test -p vault-buddy --lib
cd core && cargo test
cd ../screen && cargo test
cd .. && cargo machete .
cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94
```
Capture the exit code of each gate itself, never of a `tail` piped into.

- [ ] **Step 2: Frontend gates, in CI's order**

```bash
cd /home/user/vault-buddy
npm run lint
npm run check:loc
rm -rf coverage && npm run check:quality
npm run build
npm run test:e2e
npm run test:coverage
```
`check:quality` must run with NO `coverage/` directory present; `test:coverage` goes last.

- [ ] **Step 3: Push and watch CI**

```bash
git push -u origin claude/screen-capture-intake-g0j49q
```
Then re-check the PR's checks. A red job is to be re-diagnosed and fixed, never worked around. **Do NOT "fix" a red `capture_exclusion` test by removing `region-indicator` from `EXCLUDED_LABELS`** — that is the feature.

- [ ] **Step 4: Hand the build to the reporter**

Rows 44, 45, 46, and re-runs of 16, 17 and 43, one step at a time, recording every result verbatim including negatives.
