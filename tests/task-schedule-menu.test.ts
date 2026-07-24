import { mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

import TaskScheduleMenu from "../src/components/TaskScheduleMenu.vue";
import { comingSaturday, localDatePlus, localToday } from "../src/utils/taskFields";

// TaskScheduleMenu is modeled on TaskSectionMenu's popover, but — unlike
// TaskSectionMenu, which needs the container's busy/rename/archive/delete
// wiring to exercise meaningfully — it's fully self-contained (its only
// output is the `schedule` emit), so it's tested directly here rather than
// only through Tasks.vue integration (the TaskListPicker/Chip/IconButton
// precedent for standalone presentational components).

let active: VueWrapper | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

function mountMenu(props: Partial<InstanceType<typeof TaskScheduleMenu>["$props"]> = {}) {
  active = mount(TaskScheduleMenu, {
    props: { title: "Task A", scheduled: null, busy: false, ...props },
    attachTo: document.body,
  });
  return active;
}

const trigger = (w: VueWrapper) => w.get('[data-testid="task-schedule-Task A"]');
const popover = (w: VueWrapper) => w.find('[data-testid="task-schedule-popover-Task A"]');

describe("TaskScheduleMenu", () => {
  it("opens the popover on trigger click and closes after choosing Today", async () => {
    const w = mountMenu();
    expect(popover(w).exists()).toBe(false);
    await trigger(w).trigger("click");
    expect(popover(w).exists()).toBe(true);
    await w.get('[data-testid="task-schedule-today"]').trigger("click");
    expect(w.emitted("schedule")).toEqual([[localToday()]]);
    expect(popover(w).exists()).toBe(false); // choosing closes it
  });

  it("declares dialog semantics: haspopup=dialog, aria-controls links the role=dialog popover, aria-expanded tracks open", async () => {
    // The trigger is a disclosure that opens a dialog-like popup — screen
    // readers need matching semantics: aria-haspopup="dialog" (not "true"/menu,
    // which would promise a menu structure this unroled popover doesn't have),
    // a live aria-expanded, and an aria-controls link to the role="dialog"
    // popover (Codex, PR #75).
    const w = mountMenu();
    expect(trigger(w).attributes("aria-haspopup")).toBe("dialog");
    expect(trigger(w).attributes("aria-expanded")).toBe("false");
    expect(trigger(w).attributes("aria-controls")).toBeUndefined(); // no dangling ref while closed
    await trigger(w).trigger("click");
    expect(trigger(w).attributes("aria-expanded")).toBe("true");
    const pop = popover(w);
    expect(pop.attributes("role")).toBe("dialog");
    expect(pop.attributes("aria-label")).toBe("Schedule");
    expect(pop.attributes("id")).toBeTruthy();
    expect(trigger(w).attributes("aria-controls")).toBe(pop.attributes("id")); // linked
  });

  it("restores focus to the trigger after a keyboard-driven close (choose / Escape)", async () => {
    // Removing the focused popup child (choose) or container (Escape) via v-if
    // otherwise drops focus to <body>, forcing a keyboard user to re-traverse
    // the panel (Codex, PR #75). mountMenu attaches to body, so activeElement is
    // meaningful.
    const w = mountMenu();
    await trigger(w).trigger("click");
    await w.get('[data-testid="task-schedule-today"]').trigger("click");
    await nextTick();
    expect(document.activeElement).toBe(trigger(w).element);
    // Escape path (the handler is on the component root; keydown bubbles to it)
    await trigger(w).trigger("click");
    await trigger(w).trigger("keydown", { key: "Escape" });
    await nextTick();
    expect(document.activeElement).toBe(trigger(w).element);
  });

  it("emits Tomorrow and This weekend from their own buttons", async () => {
    const w = mountMenu();
    await trigger(w).trigger("click");
    await w.get('[data-testid="task-schedule-tomorrow"]').trigger("click");
    expect(w.emitted("schedule")?.[0]).toEqual([localDatePlus(1)]);

    await trigger(w).trigger("click");
    await w.get('[data-testid="task-schedule-weekend"]').trigger("click");
    expect(w.emitted("schedule")?.[1]).toEqual([comingSaturday()]);
  });

  it("picking a date emits it and closes", async () => {
    const w = mountMenu();
    await trigger(w).trigger("click");
    await w.get('[data-testid="task-schedule-pick"]').setValue("2026-08-01");
    expect(w.emitted("schedule")).toEqual([["2026-08-01"]]);
    expect(popover(w).exists()).toBe(false);
  });

  it("an empty date-picker change is a no-op (no schedule emitted)", async () => {
    const w = mountMenu();
    await trigger(w).trigger("click");
    // A change event with no value (e.g. the browser's native clear control).
    await w.get('[data-testid="task-schedule-pick"]').setValue("");
    expect(w.emitted("schedule")).toBeUndefined();
  });

  it("shows Clear only when a date is already scheduled, and emits null", async () => {
    const noDate = mountMenu();
    await trigger(noDate).trigger("click");
    expect(noDate.find('[data-testid="task-schedule-clear"]').exists()).toBe(false);
    noDate.unmount();

    const w = mountMenu({ scheduled: "2026-07-20" });
    await trigger(w).trigger("click");
    expect(w.find('[data-testid="task-schedule-clear"]').exists()).toBe(true);
    await w.get('[data-testid="task-schedule-clear"]').trigger("click");
    expect(w.emitted("schedule")).toEqual([[null]]);
  });

  it("busy disables the trigger", () => {
    const w = mountMenu({ busy: true });
    expect((trigger(w).element as HTMLButtonElement).disabled).toBe(true);
  });

  it("a choice made busy mid-open (another row action started) is swallowed", async () => {
    // The trigger disabling prevents opening while busy in the normal UI
    // flow, but `busy` can flip true while the popover is ALREADY open (a
    // concurrent row action) — choose()'s own guard is the defense for that.
    const w = mountMenu();
    await trigger(w).trigger("click");
    await w.setProps({ busy: true });
    await w.get('[data-testid="task-schedule-today"]').trigger("click");
    expect(w.emitted("schedule")).toBeUndefined();
  });

  it("Escape closes the popover without reaching the window (GAP-27 class)", async () => {
    const w = mountMenu();
    await trigger(w).trigger("click");
    let reachedWindow = false;
    const spy = () => {
      reachedWindow = true;
    };
    window.addEventListener("keydown", spy);
    try {
      await popover(w).trigger("keydown", { key: "Escape", isComposing: false });
      expect(popover(w).exists()).toBe(false);
      expect(reachedWindow).toBe(false);
    } finally {
      window.removeEventListener("keydown", spy);
    }
  });

  it("closes when a pointerdown lands outside the popover", async () => {
    const w = mountMenu();
    await trigger(w).trigger("click");
    expect(popover(w).exists()).toBe(true);
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await w.vm.$nextTick();
    expect(popover(w).exists()).toBe(false);
  });

  // Fold-in #2 (Codex P2): the popover is `absolute` under Tasks.vue's own
  // `overflow-y-auto` scroll ancestor, so a downward-only popover on a row
  // near the bottom of that viewport is clipped and its lower items (the
  // date picker, Clear) become unreachable. Verified at the DOM level
  // (mocking getBoundingClientRect on the trigger, the task-reorder.test.ts
  // precedent for happy-dom's all-zero rects) rather than via an extracted
  // pure helper — happy-dom + an instance-level rect override make the real
  // measure-and-flip code path directly assertable, so no fallback needed.
  describe("viewport flip", () => {
    it("opens downward when there is room below the trigger", async () => {
      vi.spyOn(window, "innerHeight", "get").mockReturnValue(768);
      const w = mountMenu();
      (trigger(w).element as HTMLElement).getBoundingClientRect = () =>
        ({ top: 40, bottom: 60, left: 0, right: 30, width: 30, height: 20, x: 0, y: 40, toJSON: () => ({}) }) as DOMRect;
      await trigger(w).trigger("click");
      const classes = popover(w).classes();
      expect(classes).toContain("top-full");
      expect(classes).not.toContain("bottom-full");
    });

    it("flips upward when the trigger sits near the bottom of the viewport", async () => {
      vi.spyOn(window, "innerHeight", "get").mockReturnValue(768);
      const w = mountMenu();
      (trigger(w).element as HTMLElement).getBoundingClientRect = () =>
        ({ top: 700, bottom: 720, left: 0, right: 30, width: 30, height: 20, x: 0, y: 700, toJSON: () => ({}) }) as DOMRect;
      await trigger(w).trigger("click");
      const classes = popover(w).classes();
      expect(classes).toContain("bottom-full");
      expect(classes).not.toContain("top-full");
    });

    it("clips against the .panel-scroll ancestor's bottom, not the window", async () => {
      // Trigger fits far below the WINDOW bottom but near the inner scroll
      // container's bottom → must flip up, proving the clip edge is the
      // container, not window.innerHeight (Codex, PR #75).
      vi.spyOn(window, "innerHeight", "get").mockReturnValue(2000);
      const scroll = document.createElement("div");
      scroll.className = "panel-scroll";
      document.body.appendChild(scroll);
      const w = mount(TaskScheduleMenu, {
        props: { title: "Task A", scheduled: null, busy: false },
        attachTo: scroll,
      });
      scroll.getBoundingClientRect = () =>
        ({ top: 0, bottom: 560, left: 0, right: 300, width: 300, height: 560, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
      (trigger(w).element as HTMLElement).getBoundingClientRect = () =>
        ({ top: 480, bottom: 500, left: 0, right: 30, width: 30, height: 20, x: 0, y: 480, toJSON: () => ({}) }) as DOMRect;
      await trigger(w).trigger("click");
      // 500 + 200 (est) = 700 > 560 (container) → flip up, though 700 < 2000 (window).
      expect(popover(w).classes()).toContain("bottom-full");
      w.unmount();
      scroll.remove();
    });

    // GAP-73a: the one-sided predecessor clipped only against the bottom, so
    // it could flip up whenever room below was tight — even where room above
    // was tighter still, clipping the popover's OWN top controls instead. The
    // two-sided fix requires BOTH "not enough room below" AND "more room
    // above than below" before it ever flips.
    it("never flips when there is ample room below, even with even more room above", async () => {
      // Room above (500) > room below (480), which a broken "just compare the
      // two sides" implementation would flip on — but room below (480) alone
      // already comfortably fits the ~200px popover, so it must stay down.
      vi.spyOn(window, "innerHeight", "get").mockReturnValue(1000);
      const w = mountMenu();
      (trigger(w).element as HTMLElement).getBoundingClientRect = () =>
        ({ top: 500, bottom: 520, left: 0, right: 30, width: 30, height: 20, x: 0, y: 500, toJSON: () => ({}) }) as DOMRect;
      await trigger(w).trigger("click");
      const classes = popover(w).classes();
      expect(classes).toContain("top-full");
      expect(classes).not.toContain("bottom-full");
    });

    it("does NOT flip up when room below is short but room above is even shorter", async () => {
      // A compact .panel-scroll (150px tall) with the trigger near its TOP:
      // room below = 150-30 = 120 (< the ~200px popover, so downward alone
      // would clip) but room above = 10-0 = 10 — flipping up would clip WORSE.
      // The old one-sided heuristic flipped here (120 < 200 was its whole
      // check); the two-sided fix must decline.
      const scroll = document.createElement("div");
      scroll.className = "panel-scroll";
      document.body.appendChild(scroll);
      const w = mount(TaskScheduleMenu, {
        props: { title: "Task A", scheduled: null, busy: false },
        attachTo: scroll,
      });
      scroll.getBoundingClientRect = () =>
        ({ top: 0, bottom: 150, left: 0, right: 300, width: 300, height: 150, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect;
      (trigger(w).element as HTMLElement).getBoundingClientRect = () =>
        ({ top: 10, bottom: 30, left: 0, right: 30, width: 30, height: 20, x: 0, y: 10, toJSON: () => ({}) }) as DOMRect;
      await trigger(w).trigger("click");
      const classes = popover(w).classes();
      expect(classes).toContain("top-full");
      expect(classes).not.toContain("bottom-full");
      w.unmount();
      scroll.remove();
    });
  });
});
