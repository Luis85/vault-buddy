import { mount, type VueWrapper } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

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
  });
});
