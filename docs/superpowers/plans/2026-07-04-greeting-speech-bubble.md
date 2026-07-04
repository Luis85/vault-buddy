# Greeting Speech Bubble Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** On app launch the buddy shows a speech bubble with a short greeting chosen from the local time-of-day and weekday/weekend, which auto-dismisses after a few seconds.

**Architecture:** Three new frontend units — `greeting.ts` (pure content + selection), `useGreeting.ts` (lifecycle/timer), `SpeechBubble.vue` (presentational) — plus a third "bubble" window-geometry state added to the existing `useCompanionWindow` composable. The bubble reuses the panel's edge-aware placement and serialized transition queue; the panel always takes geometry precedence over the bubble. No Rust changes.

**Tech Stack:** Vue 3 `<script setup>` + Pinia, Tailwind 4, Vitest + happy-dom + @vue/test-utils, `@tauri-apps/api` mocked via `mockIPC` / `vi.mock`.

## Global Constraints

- Node 22; `npm test` runs the full Vitest suite; `npx vitest run tests/<file>.test.ts` runs one file.
- Frontend-only increment. Do **not** touch `src-tauri/`. No new Rust IPC command — geometry uses the existing `set_window_geometry` invoke.
- Tests live in `tests/*.test.ts`, use happy-dom, and must never require a real Tauri runtime (IPC is mocked; geometry `invoke` calls already `.catch()` to no-ops off-Tauri).
- The window-geometry invariants are load-bearing: position + size change in ONE `set_window_geometry` call; the panel-open offset is mirrored to Rust via `set_panel_offset`; transitions are serialized in a queue where a newer desired state supersedes an in-flight one. Preserve all of these.
- Commits: Conventional Commits. Use scope `feat(ui)` for the greeting feature commits and `test(ui)` only if a commit is tests-only. Imperative subject; body explains the *why*.
- Tunables (single named constants): `GREETING_MS = 5000`; `BUBBLE = { width: 260, height: 150 }`; phrase pools in `greeting.ts` (exactly 3 phrases per cell).
- Character-neutral phrasing (one shared phrase set for all characters). Time buckets (local time): morning 05:00–11:59, afternoon 12:00–16:59, evening 17:00–21:59, night 22:00–04:59. Weekend = Saturday/Sunday.

---

### Task 1: Greeting content + selection (`src/greeting.ts`)

Pure module — no Vue, no Tauri. The single source of greeting text and the daypart/weekend bucketing. Fully deterministic under test via an injected `pick`.

**Files:**
- Create: `src/greeting.ts`
- Test: `tests/greeting.test.ts`

**Interfaces:**
- Consumes: nothing (leaf module).
- Produces:
  - `type Daypart = "morning" | "afternoon" | "evening" | "night"`
  - `daypartFor(date: Date): Daypart`
  - `isWeekend(date: Date): boolean`
  - `greetingFor(date: Date, pick?: (n: number) => number): string` — `pick(n)` returns an index in `[0, n)`; defaults to `Math.random`-based selection. Task 4 (`useGreeting`) calls `greetingFor(new Date())`.

- [ ] **Step 1: Write the failing test**

Create `tests/greeting.test.ts`:

```ts
import { describe, expect, it, vi } from "vitest";
import {
  daypartFor,
  isWeekend,
  greetingFor,
  type Daypart,
} from "../src/greeting";

// Local-time constructor: new Date(y, monthIndex, day, hour, min) uses the
// runtime's local zone, and daypartFor/isWeekend read getHours()/getDay(),
// so these assertions hold regardless of the CI timezone.
// Jan 2026: 1st = Thu, 3rd = Sat, 4th = Sun, 5th = Mon.
const weekdayAt = (h: number) => new Date(2026, 0, 5, h, 0); // Monday
const weekendAt = (h: number) => new Date(2026, 0, 3, h, 0); // Saturday

describe("daypartFor", () => {
  it("buckets each hour into the right daypart at the boundaries", () => {
    expect(daypartFor(weekdayAt(4))).toBe("night");
    expect(daypartFor(weekdayAt(5))).toBe("morning");
    expect(daypartFor(weekdayAt(11))).toBe("morning");
    expect(daypartFor(weekdayAt(12))).toBe("afternoon");
    expect(daypartFor(weekdayAt(16))).toBe("afternoon");
    expect(daypartFor(weekdayAt(17))).toBe("evening");
    expect(daypartFor(weekdayAt(21))).toBe("evening");
    expect(daypartFor(weekdayAt(22))).toBe("night");
    expect(daypartFor(weekdayAt(0))).toBe("night");
  });
});

describe("isWeekend", () => {
  it("is true only for Saturday and Sunday", () => {
    expect(isWeekend(new Date(2026, 0, 5))).toBe(false); // Mon
    expect(isWeekend(new Date(2026, 0, 1))).toBe(false); // Thu
    expect(isWeekend(new Date(2026, 0, 3))).toBe(true); // Sat
    expect(isWeekend(new Date(2026, 0, 4))).toBe(true); // Sun
  });
});

describe("greetingFor", () => {
  it("selects from the daypart+weekday cell via the injected pick", () => {
    const pick = vi.fn(() => 0);
    const msg = greetingFor(weekdayAt(9), pick); // morning, weekday
    expect(pick).toHaveBeenCalledWith(3); // exactly 3 phrases per cell
    expect(typeof msg).toBe("string");
    expect(msg.length).toBeGreaterThan(0);
  });

  it("selects from the weekend cell on weekends", () => {
    const weekday = greetingFor(weekdayAt(9), () => 0);
    const weekend = greetingFor(weekendAt(9), () => 0);
    expect(weekend).not.toBe(weekday); // different cell, distinct copy
  });

  it("returns a non-empty phrase for every daypart and day type", () => {
    const dayparts: Daypart[] = ["morning", "afternoon", "evening", "night"];
    const hours: Record<Daypart, number> = {
      morning: 9,
      afternoon: 14,
      evening: 19,
      night: 23,
    };
    for (const dp of dayparts) {
      for (const at of [weekdayAt(hours[dp]), weekendAt(hours[dp])]) {
        for (let i = 0; i < 3; i++) {
          const msg = greetingFor(at, () => i);
          expect(msg.length).toBeGreaterThan(0);
        }
      }
    }
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/greeting.test.ts`
Expected: FAIL — cannot resolve `../src/greeting` / exports undefined.

- [ ] **Step 3: Write minimal implementation**

Create `src/greeting.ts`:

```ts
export type Daypart = "morning" | "afternoon" | "evening" | "night";

// Local-time hour buckets, contiguous and half-open so every hour maps to
// exactly one daypart:
//   morning   05:00–11:59
//   afternoon 12:00–16:59
//   evening   17:00–21:59
//   night     22:00–04:59
export function daypartFor(date: Date): Daypart {
  const h = date.getHours();
  if (h >= 5 && h < 12) return "morning";
  if (h >= 12 && h < 17) return "afternoon";
  if (h >= 17 && h < 22) return "evening";
  return "night";
}

// getDay(): 0 = Sunday … 6 = Saturday.
export function isWeekend(date: Date): boolean {
  const d = date.getDay();
  return d === 0 || d === 6;
}

// Character-neutral greetings. Exactly 3 phrasings per cell so repeated
// launches don't feel canned; the count is asserted in the tests.
const GREETINGS: Record<Daypart, { weekday: string[]; weekend: string[] }> = {
  morning: {
    weekday: [
      "Good morning! Ready to dive into your notes?",
      "Morning! A fresh day, a fresh page.",
      "Rise and shine — your vault awaits.",
    ],
    weekend: [
      "Good morning! Enjoy a relaxed start to your weekend.",
      "Weekend morning — no rush, just your notes and a coffee.",
      "Morning! A perfect time for some unhurried thinking.",
    ],
  },
  afternoon: {
    weekday: [
      "Good afternoon! Let's keep the momentum going.",
      "Afternoon! Time to capture a thought or two?",
      "Hope your day's going well — your vault's right here.",
    ],
    weekend: [
      "Good afternoon! A calm weekend for tending your notes.",
      "Afternoon! Weekend projects, meet your vault.",
      "Hope you're having a lovely weekend afternoon.",
    ],
  },
  evening: {
    weekday: [
      "Good evening! Winding down or wrapping up?",
      "Evening! A good time to review the day's notes.",
      "Evening! Let's tie up any loose ends.",
    ],
    weekend: [
      "Good evening! Enjoy a cozy weekend night.",
      "Evening! The weekend's still going — savor it.",
      "Good evening! Relax; your notes will keep.",
    ],
  },
  night: {
    weekday: [
      "Working late? Your vault's here whenever you need it.",
      "It's getting late — one more note, then rest.",
      "Late-night thoughts? Let's jot them down.",
    ],
    weekend: [
      "Late weekend night — the quiet's good for ideas.",
      "Still up? Your vault doesn't mind the hour.",
      "Night-owl mode: your notes are ready when you are.",
    ],
  },
};

/**
 * Pick one greeting for the given moment. `pick(n)` returns an index in
 * [0, n); it is injected in tests for determinism and defaults to a
 * Math.random-based choice.
 */
export function greetingFor(
  date: Date,
  pick: (n: number) => number = (n) => Math.floor(Math.random() * n),
): string {
  const cell = GREETINGS[daypartFor(date)];
  const pool = isWeekend(date) ? cell.weekend : cell.weekday;
  return pool[pick(pool.length)];
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/greeting.test.ts`
Expected: PASS (all cases green).

- [ ] **Step 5: Commit**

```bash
git add src/greeting.ts tests/greeting.test.ts
git commit -m "feat(ui): add time-of-day greeting text module

Pure daypart/weekend bucketing and phrase selection behind an injected
pick() so the whole module is deterministic under test. Content for the
buddy's startup greeting bubble (increment 3)."
```

---

### Task 2: Add the "bubble" window-geometry state (`useCompanionWindow`)

Extend the composable to accept a second reactive input (`bubbleOpen`) and drive a third geometry state — a modest `BUBBLE` window that fits the buddy plus a speech bubble — through the existing serialized queue, with the panel always winning.

**Files:**
- Modify: `src/composables/useCompanionWindow.ts` (rewrite the composable body; add `BUBBLE` export)
- Test: `tests/companion-window.test.ts` (add cases)

**Interfaces:**
- Consumes: `planPanelPlacement`, `type Rect` from `./companionPlacement` (unchanged); existing `set_window_geometry` / `set_panel_offset` IPC.
- Produces:
  - `export const BUBBLE = { width: 260, height: 150 }`
  - `useCompanionWindow(panelOpen: Ref<boolean>, bubbleOpen?: Ref<boolean>): { side: Ref<"right" | "left">; valign: Ref<"down" | "up"> }` — App (Task 5) passes the greeting's `bubbleVisible` as `bubbleOpen`. Precedence: panel > bubble > collapsed.

- [ ] **Step 1: Write the failing test**

Add these cases inside the existing `describe("useCompanionWindow", …)` block in `tests/companion-window.test.ts` (import `BUBBLE` alongside `COLLAPSED`/`EXPANDED`):

Update the import at the top of the file:

```ts
import {
  useCompanionWindow,
  COLLAPSED,
  EXPANDED,
  BUBBLE,
} from "../src/composables/useCompanionWindow";
```

Append these tests before the closing `});` of the describe block:

```ts
it("grows to the bubble size when the greeting bubble opens", async () => {
  const panel = ref(false);
  const bubble = ref(false);
  useCompanionWindow(panel, bubble);

  bubble.value = true;
  await nextTick();
  await flush();
  expect(lastResize()).toBe(String(BUBBLE.width));

  bubble.value = false;
  await nextTick();
  await flush();
  expect(lastResize()).toBe(String(COLLAPSED.width));
});

it("lets the panel win when both the panel and the bubble are open", async () => {
  const panel = ref(false);
  const bubble = ref(false);
  useCompanionWindow(panel, bubble);

  bubble.value = true;
  panel.value = true;
  await nextTick();
  await flush();
  await flush();
  // panel precedence: the window opens to the full panel size, not BUBBLE
  expect(lastResize()).toBe(String(EXPANDED.width));
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/companion-window.test.ts`
Expected: FAIL — `BUBBLE` is not exported / `useCompanionWindow` ignores the second arg (grows to EXPANDED width or never to 260).

- [ ] **Step 3: Write minimal implementation**

Replace the entire contents of `src/composables/useCompanionWindow.ts` with:

```ts
import { ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  currentMonitor,
  LogicalSize,
} from "@tauri-apps/api/window";
import { planPanelPlacement, type Rect } from "./companionPlacement";

export const COLLAPSED = { width: 88, height: 88 };
export const EXPANDED = { width: 440, height: 340 };
// A transient window just big enough for the buddy plus a greeting speech
// bubble beside it. Kept small so the invisible click area it creates at
// startup is minimal and short-lived (the bubble auto-dismisses).
export const BUBBLE = { width: 260, height: 150 };

type WindowState = "collapsed" | "bubble" | "expanded";

interface MonitorLike {
  position: { x: number; y: number };
  size: { width: number; height: number };
  workArea?: {
    position: { x: number; y: number };
    size: { width: number; height: number };
  };
}

/** Prefer the work area (excludes the taskbar) when the runtime provides it. */
function monitorRect(monitor: MonitorLike | null): Rect | null {
  if (!monitor) return null;
  const src = monitor.workArea ?? monitor;
  return {
    x: src.position.x,
    y: src.position.y,
    width: src.size.width,
    height: src.size.height,
  };
}

/**
 * Grows the transparent window when the panel opens or a greeting bubble
 * shows, and shrinks it back when neither wants space, so the invisible
 * window never blocks clicks on the desktop beneath it. The growth
 * direction respects the monitor edges: near the right or bottom edge the
 * window is shifted so it unfolds toward free space, and the returned
 * `side`/`valign` let the layout mirror itself to keep the buddy visually
 * pinned. The panel always takes precedence over the bubble.
 */
export function useCompanionWindow(
  panelOpen: Ref<boolean>,
  bubbleOpen?: Ref<boolean>,
): {
  side: Ref<"right" | "left">;
  valign: Ref<"down" | "up">;
} {
  const side = ref<"right" | "left">("right");
  const valign = ref<"down" | "up">("down");
  // Physical px subtracted from the window position while it is grown. The
  // collapse path adds it back relative to the *current* position, so the
  // buddy stays put even if the window was dragged while open.
  let offset = { x: 0, y: 0 };

  // Mirror the offset to the Rust side: quitting from the tray saves the
  // window position, and it must save the unshifted home position even if
  // the window is grown at that moment.
  function reportOffset() {
    void invoke("set_panel_offset", { x: offset.x, y: offset.y }).catch(
      () => {
        // not running under Tauri (unit tests) — nothing to report to
      },
    );
  }

  // Panel beats bubble beats collapsed. When the panel is open its larger
  // window already contains the buddy, so the greeting bubble never drives
  // geometry.
  function desiredState(): WindowState {
    if (panelOpen.value) return "expanded";
    if (bubbleOpen?.value) return "bubble";
    return "collapsed";
  }

  const sizeFor = (target: WindowState) =>
    target === "expanded" ? EXPANDED : BUBBLE;

  // Position and size must change in ONE native call: applying them as two
  // IPC round-trips painted an intermediate geometry — the buddy flashed to
  // a corner whenever the window grew shifted (left/up placements).
  function setGeometry(
    pos: { x: number; y: number },
    size: { width: number; height: number },
  ): Promise<void> {
    return invoke("set_window_geometry", {
      x: pos.x,
      y: pos.y,
      width: size.width,
      height: size.height,
    });
  }

  // `target` is the grown state this transition was queued for ("bubble" or
  // "expanded"). If the desired state has since changed (a newer toggle),
  // the transition is stale and must not paint an outdated geometry.
  async function applyGrow(target: WindowState): Promise<void> {
    const size = sizeFor(target);
    const win = getCurrentWindow();
    try {
      const [pos, scale, monitor] = await Promise.all([
        win.outerPosition(),
        win.scaleFactor(),
        currentMonitor(),
      ]);
      if (desiredState() !== target) return;
      // Plan from the unshifted "home" position. If a previous grow already
      // shifted the window (rapid grow→collapse→grow where the collapse was
      // superseded), planning from the raw position would conclude there is
      // room and reset the pending offset — the following collapse would
      // then never move the buddy back to where the user left it.
      const home = { x: pos.x + offset.x, y: pos.y + offset.y };
      const placement = planPanelPlacement(
        home,
        monitorRect(monitor),
        scale,
        COLLAPSED,
        size,
      );
      // Record before moving: if we're superseded right after the move, the
      // collapse transition still knows what to undo.
      offset = placement.offset;
      reportOffset();
      side.value = placement.side;
      valign.value = placement.valign;
      await setGeometry(
        {
          x: home.x - placement.offset.x,
          y: home.y - placement.offset.y,
        },
        size,
      );
    } catch {
      // No window/monitor info — grow right/down in place. Leave any
      // recorded offset untouched so a pending shift is still undone on
      // collapse.
      side.value = "right";
      valign.value = "down";
      await win
        .setSize(new LogicalSize(size.width, size.height))
        .catch(() => {});
    }
  }

  async function applyCollapse(): Promise<void> {
    const win = getCurrentWindow();
    try {
      const pos = await win.outerPosition();
      await setGeometry(
        { x: pos.x + offset.x, y: pos.y + offset.y },
        COLLAPSED,
      );
    } catch {
      // window may be gone during shutdown — best-effort collapse
      await win
        .setSize(new LogicalSize(COLLAPSED.width, COLLAPSED.height))
        .catch(() => {});
    }
    if (offset.x !== 0 || offset.y !== 0) {
      offset = { x: 0, y: 0 };
      reportOffset();
    }
    side.value = "right";
    valign.value = "down";
  }

  function applyState(target: WindowState): Promise<void> {
    return target === "collapsed" ? applyCollapse() : applyGrow(target);
  }

  // Serialize transitions: a collapse never interleaves with an in-flight
  // grow (which could re-expand the window after it was collapsed, leaving
  // an invisible click-blocking area). Superseded transitions are skipped.
  let queue: Promise<void> = Promise.resolve();
  function schedule() {
    const target = desiredState();
    queue = queue
      .then(() => {
        if (desiredState() !== target) return; // a newer toggle already won
        return applyState(target);
      })
      .catch(() => {
        // a failed transition must not wedge the queue
      });
  }

  watch(panelOpen, schedule);
  if (bubbleOpen) watch(bubbleOpen, schedule);

  return { side, valign };
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run tests/companion-window.test.ts`
Expected: PASS — the three original cases (open/close, stale-close, edge restore) plus the two new bubble cases are all green.

- [ ] **Step 5: Commit**

```bash
git add src/composables/useCompanionWindow.ts tests/companion-window.test.ts
git commit -m "feat(ui): add transient bubble window geometry state

useCompanionWindow gains an optional bubbleOpen input and a third target
size (BUBBLE) between collapsed and the full panel. Grow/collapse now
route through a single desired-state resolver (panel > bubble >
collapsed) so the existing one-native-call, offset-mirroring and
supersede-in-flight invariants hold unchanged. Gives the greeting bubble
room without the large invisible area the full panel window would create."
```

---

### Task 3: The speech bubble component (`SpeechBubble.vue`)

Presentational only: renders the greeting text in a bubble with a tail that points back at the buddy, mirrored by the same `side`/`valign` the layout uses. No store, no logic.

**Files:**
- Create: `src/components/SpeechBubble.vue`
- Test: `tests/speech-bubble.test.ts`

**Interfaces:**
- Consumes: nothing (pure props component).
- Produces: `<SpeechBubble text side valign />` where `text: string`, `side: "left" | "right"`, `valign: "up" | "down"`. Renders a root element `[data-testid="speech-bubble"]` carrying classes `side-${side}` and `valign-${valign}`. Used by App (Task 5).

- [ ] **Step 1: Write the failing test**

Create `tests/speech-bubble.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import SpeechBubble from "../src/components/SpeechBubble.vue";

describe("SpeechBubble", () => {
  it("renders the greeting text", () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Good morning!", side: "right", valign: "down" },
    });
    expect(wrapper.get('[data-testid="speech-bubble"]').text()).toContain(
      "Good morning!",
    );
  });

  it("reflects the buddy side and vertical alignment so the tail points home", () => {
    const wrapper = mount(SpeechBubble, {
      props: { text: "Hi", side: "left", valign: "up" },
    });
    const bubble = wrapper.get('[data-testid="speech-bubble"]');
    expect(bubble.classes()).toContain("side-left");
    expect(bubble.classes()).toContain("valign-up");
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/speech-bubble.test.ts`
Expected: FAIL — cannot resolve `../src/components/SpeechBubble.vue`.

- [ ] **Step 3: Write minimal implementation**

Create `src/components/SpeechBubble.vue`:

```vue
<script setup lang="ts">
defineProps<{
  text: string;
  // mirror the layout so the tail sits on the buddy's side; when the window
  // is edge-shifted the bubble unfolds away from the edge and the tail still
  // points back toward the buddy
  side: "left" | "right";
  valign: "up" | "down";
}>();
</script>

<template>
  <div
    data-testid="speech-bubble"
    class="bubble"
    :class="[`side-${side}`, `valign-${valign}`]"
    role="status"
    aria-live="polite"
  >
    {{ text }}
  </div>
</template>

<style scoped>
.bubble {
  position: relative;
  max-width: 168px;
  border-radius: 12px;
  background: #ffffff;
  color: #1f2333;
  padding: 8px 10px;
  font-size: 12px;
  line-height: 1.35;
  box-shadow: 0 4px 14px rgba(0, 0, 0, 0.22);
  /* the bubble sits beside the buddy; keep a small gap for the tail */
  margin: 0 8px;
}

/* Tail: a small diamond nudged to the edge nearest the buddy. side-right
   means the buddy is to the LEFT of the bubble, so the tail sits on the
   left face, and vice versa. */
.bubble::after {
  content: "";
  position: absolute;
  width: 10px;
  height: 10px;
  background: inherit;
  transform: rotate(45deg);
  top: 24px;
}
.bubble.valign-up::after {
  top: auto;
  bottom: 24px;
}
.bubble.side-right::after {
  left: -4px;
}
.bubble.side-left::after {
  right: -4px;
}
</style>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/speech-bubble.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/SpeechBubble.vue tests/speech-bubble.test.ts
git commit -m "feat(ui): add SpeechBubble presentational component

Renders greeting text in a tailed bubble; the tail edge is mirrored off
side/valign so it always points back at the buddy, including edge-shifted
placements."
```

---

### Task 4: Greeting lifecycle composable (`useGreeting.ts`)

Owns *when* the bubble shows: on mount it computes the greeting, shows the bubble, and starts the auto-dismiss timer; it exposes `dismiss()` so App can cancel early when the panel opens.

**Files:**
- Create: `src/composables/useGreeting.ts`
- Test: `tests/use-greeting.test.ts`

**Interfaces:**
- Consumes: `greetingFor` from `../greeting` (Task 1).
- Produces:
  - `export const GREETING_MS = 5000`
  - `useGreeting(): { bubbleVisible: Ref<boolean>; bubbleText: Ref<string>; dismiss: () => void }` — App (Task 5) passes `bubbleVisible` to `useCompanionWindow` as `bubbleOpen`, renders `SpeechBubble` with `bubbleText`, and calls `dismiss()` when the panel opens.

- [ ] **Step 1: Write the failing test**

Create `tests/use-greeting.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent } from "vue";
import { useGreeting, GREETING_MS } from "../src/composables/useGreeting";

// A throwaway host component: returning the composable from setup() exposes
// its refs/functions on wrapper.vm (refs are unwrapped there).
const Host = defineComponent({
  setup: () => useGreeting(),
  render: () => null,
});

describe("useGreeting", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows a greeting on mount", () => {
    const wrapper = mount(Host);
    expect(wrapper.vm.bubbleVisible).toBe(true);
    expect(wrapper.vm.bubbleText.length).toBeGreaterThan(0);
  });

  it("auto-dismisses after GREETING_MS", () => {
    const wrapper = mount(Host);
    vi.advanceTimersByTime(GREETING_MS);
    expect(wrapper.vm.bubbleVisible).toBe(false);
  });

  it("dismiss() hides immediately and cancels the timer", () => {
    const wrapper = mount(Host);
    wrapper.vm.dismiss();
    expect(wrapper.vm.bubbleVisible).toBe(false);
    // advancing past the original timeout must not throw or re-toggle
    vi.advanceTimersByTime(GREETING_MS);
    expect(wrapper.vm.bubbleVisible).toBe(false);
  });

  it("clears the timer on unmount", () => {
    const wrapper = mount(Host);
    wrapper.unmount();
    // no dangling callback flips a ref on a torn-down component
    expect(() => vi.advanceTimersByTime(GREETING_MS)).not.toThrow();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/use-greeting.test.ts`
Expected: FAIL — cannot resolve `../src/composables/useGreeting`.

- [ ] **Step 3: Write minimal implementation**

Create `src/composables/useGreeting.ts`:

```ts
import { onMounted, onUnmounted, ref, type Ref } from "vue";
import { greetingFor } from "../greeting";

// How long the greeting bubble stays before auto-dismissing.
export const GREETING_MS = 5000;

/**
 * Shows a one-shot greeting bubble on mount (app launch) and auto-dismisses
 * it after GREETING_MS. `dismiss()` lets a caller (App, when the panel
 * opens) cancel the timer and hide the bubble immediately. Shows once per
 * mount — a single-instance reveal of an already-running app does not
 * remount the frontend, so it does not re-greet.
 */
export function useGreeting(): {
  bubbleVisible: Ref<boolean>;
  bubbleText: Ref<string>;
  dismiss: () => void;
} {
  const bubbleVisible = ref(false);
  const bubbleText = ref("");
  let timer: ReturnType<typeof setTimeout> | undefined;

  function dismiss() {
    clearTimeout(timer);
    timer = undefined;
    bubbleVisible.value = false;
  }

  onMounted(() => {
    bubbleText.value = greetingFor(new Date());
    bubbleVisible.value = true;
    timer = setTimeout(dismiss, GREETING_MS);
  });

  onUnmounted(() => {
    clearTimeout(timer);
  });

  return { bubbleVisible, bubbleText, dismiss };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run tests/use-greeting.test.ts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/composables/useGreeting.ts tests/use-greeting.test.ts
git commit -m "feat(ui): add useGreeting lifecycle composable

Computes the greeting on mount, shows the bubble, and auto-dismisses
after GREETING_MS. Exposes dismiss() so the panel-open path can cancel
the timer and hide the bubble at once. Timer cleared on unmount."
```

---

### Task 5: Wire the greeting into `App.vue`

Show the bubble on launch, feed its visibility into the window geometry, render `SpeechBubble` beside the buddy, and dismiss the greeting when the panel opens.

**Files:**
- Modify: `src/App.vue`
- Test: `tests/app-layout.test.ts` (add cases)

**Interfaces:**
- Consumes: `useGreeting` (Task 4), `SpeechBubble` (Task 3), extended `useCompanionWindow` (Task 2).
- Produces: no new exports; App renders `[data-testid="speech-bubble"]` when `bubbleVisible && !panelOpen`.

- [ ] **Step 1: Write the failing test**

Add these two cases inside the existing `describe("App layout geometry", …)` block in `tests/app-layout.test.ts`, before its closing `});`:

```ts
it("shows a greeting bubble on launch", async () => {
  const wrapper = mount(App);
  await flush();
  await nextTick();
  const bubble = wrapper.find('[data-testid="speech-bubble"]');
  expect(bubble.exists()).toBe(true);
  expect(bubble.text().length).toBeGreaterThan(0);
});

it("hides the greeting bubble once the panel opens", async () => {
  const wrapper = mount(App);
  await flush();
  await nextTick();
  expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);

  const store = useVaultsStore();
  await store.togglePanel();
  await flush();
  await nextTick();
  expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(false);
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `npx vitest run tests/app-layout.test.ts`
Expected: FAIL — no `[data-testid="speech-bubble"]` in the DOM.

- [ ] **Step 3: Write minimal implementation**

In `src/App.vue`, make exactly these edits.

(a) Add imports after the existing `ActionPanel` import (script block, near line 7):

```ts
import SpeechBubble from "./components/SpeechBubble.vue";
```

and add `watch` to the existing `vue` import (line 2) so it reads:

```ts
import { computed, onMounted, onUnmounted, watch } from "vue";
```

and add the greeting composable import after the settings store import (near line 10):

```ts
import { useGreeting } from "./composables/useGreeting";
```

(b) Replace the geometry wiring block (currently lines 12–17):

```ts
const store = useVaultsStore();
const settings = useSettingsStore();
const { panelOpen, busyVaultId } = storeToRefs(store);
const working = computed(() => busyVaultId.value !== null);

const { side, valign } = useCompanionWindow(panelOpen);
```

with:

```ts
const store = useVaultsStore();
const settings = useSettingsStore();
const { panelOpen, busyVaultId } = storeToRefs(store);
const working = computed(() => busyVaultId.value !== null);

const { bubbleVisible, bubbleText, dismiss } = useGreeting();
const { side, valign } = useCompanionWindow(panelOpen, bubbleVisible);

// Opening the panel supersedes the greeting: cancel its timer and hide it
// so it can't reappear when the panel closes again within the greeting
// window. (The bubble is also hidden by v-if while the panel is open.)
watch(panelOpen, (open) => {
  if (open) dismiss();
});
```

(c) In the template, add the bubble beside the buddy. Immediately after the closing `</div>` of the `data-testid="buddy-cell"` block and before the `v-if="panelOpen"` panel wrapper `<div>`, insert:

```html
    <div
      v-if="bubbleVisible && !panelOpen"
      class="flex min-w-0 flex-1 items-center self-stretch p-2"
    >
      <SpeechBubble :text="bubbleText" :side="side" :valign="valign" />
    </div>
```

The bubble reuses the same flex-row / flex-row-reverse mirroring the panel relies on (driven by `side`/`valign` from `useCompanionWindow`), so it sits on the correct side of the buddy and unfolds toward free space near edges.

- [ ] **Step 4: Run tests to verify they pass**

Run the touched suites plus a full run to confirm no regressions in the existing App layout tests (the greeting now mounts in every App test):

Run: `npx vitest run tests/app-layout.test.ts`
Expected: PASS — the two new cases plus all pre-existing App layout cases stay green.

Run: `npm test`
Expected: PASS — entire suite green.

- [ ] **Step 5: Typecheck the production build**

Run: `npm run build`
Expected: `vue-tsc` typechecks clean and the production build succeeds (no type errors from the new props/exports).

- [ ] **Step 6: Commit**

```bash
git add src/App.vue tests/app-layout.test.ts
git commit -m "feat(ui): greet the user with a speech bubble on launch

App drives the greeting bubble's visibility into the window geometry
(bubble state) and renders SpeechBubble beside the buddy, mirrored by the
same side/valign as the panel. Opening the panel dismisses the greeting.
First increment of the buddy 'talking' to the user."
```

---

## Notes for the implementer

- **Run order matters for TDD:** do the steps top-to-bottom within each task; each task is independently testable and committable.
- **Do not touch `src-tauri/`.** This increment is pure frontend; CI's Windows job is unaffected.
- **`side`/`valign` already exist** as return values of `useCompanionWindow` — App passes them straight to `SpeechBubble`; no new plumbing.
- **Why the panel-precedence check is doubled** (geometry `desiredState()` *and* the template `!panelOpen` plus `watch → dismiss`): geometry precedence keeps the window sized correctly; the template guard stops the bubble from drawing inside the open panel; `dismiss()` cancels the pending timer so a close within the greeting window doesn't resurrect the bubble. Each guards a different failure.
- If `npm run build` flags an unused `Rect` or similar in `useCompanionWindow.ts`, it's imported and used by `monitorRect`'s return type — keep it.
```
