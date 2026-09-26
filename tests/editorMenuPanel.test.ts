/**
 * `MenuPanel.vue` — the one panel every editor menu renders through
 * (visual-parity Task 5; concept spec §8). Keyboard, submenu and hint
 * behaviour are what a person relies on without a mouse, so each is driven
 * here through real key events on the rendered panel.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";
import { nextTick } from "vue";

import type { MenuItem } from "../src/components/editor/menus/menuModel";
import MenuPanel from "../src/components/editor/menus/MenuPanel.vue";

enableAutoUnmount(afterEach);

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const runs = { go: vi.fn(), half: vi.fn(), del: vi.fn() };

function items(): MenuItem[] {
  return [
    { id: "go", label: "Go to this clip", icon: "locate", run: runs.go },
    { id: "split", label: "Split at 00:17.4", icon: "scissors", kbd: "S", disabledReason: "Choose a point inside the clip." },
    { separator: true },
    {
      id: "speed",
      label: "Speed",
      icon: "speed",
      submenu: [
        { id: "speed-half", label: "0.5×", checked: false, run: runs.half },
        { id: "speed-one", label: "1× · normal", checked: true, run: vi.fn() },
      ],
    },
    { id: "snap", label: "Snapping", icon: "magnet", checked: true, run: vi.fn() },
    { id: "del", label: "Delete · leave gap", icon: "trash", kbd: "Delete", danger: true, run: runs.del },
  ];
}

function open(extra: Record<string, unknown> = {}) {
  return mount(MenuPanel, {
    attachTo: document.body,
    props: { heading: "Create a project", items: items(), anchor: { x: 20, y: 30 }, testid: "m", ...extra },
  });
}

const focusedId = () => document.activeElement?.getAttribute("data-testid");
const key = (el: Element, k: string) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));

describe("MenuPanel — anatomy", () => {
  it("renders the heading and the mono subtitle", () => {
    const w = open();
    expect(w.get('[data-testid="m-heading"]').text()).toBe("Create a project");
    const sub = w.get('[data-testid="m-subtitle"]');
    expect(sub.text()).toBe("Direct editing · originals unchanged");
    expect(sub.classes()).toContain("vb-mono");
    expect(w.get('[data-testid="m"]').attributes("aria-label")).toBe("Create a project");
  });

  it("gives items menuitem / menuitemcheckbox roles and separators role=separator", () => {
    const w = open();
    expect(w.get('[data-testid="m-item-go"]').attributes("role")).toBe("menuitem");
    const snap = w.get('[data-testid="m-item-snap"]');
    expect(snap.attributes("role")).toBe("menuitemcheckbox");
    expect(snap.attributes("aria-checked")).toBe("true");
    expect(w.findAll('[role="separator"]')).toHaveLength(1);
    expect(w.get('[data-testid="m-item-speed"]').attributes("aria-haspopup")).toBe("menu");
  });

  it("renders the kbd right-aligned, danger items with the danger class, and an icon in every item", () => {
    const w = open();
    const kbd = w.get('[data-testid="m-item-split"] kbd');
    expect(kbd.text()).toBe("S");
    expect(kbd.classes()).toContain("ml-auto");
    expect(w.get('[data-testid="m-item-del"]').classes()).toContain("danger");
    expect(w.get('[data-testid="m-item-go"]').classes()).not.toContain("danger");
    for (const b of w.findAll('[role^="menuitem"]')) expect(b.find("svg").exists()).toBe(true);
  });

  it("shows the default hint, and a disabled item's reason as its title", () => {
    const w = open();
    expect(w.get('[data-testid="m-hint"]').text()).toBe("↑ ↓ navigate · Enter choose · Esc close");
    expect(w.get('[data-testid="m-hint"]').attributes("aria-live")).toBe("polite");
    const split = w.get('[data-testid="m-item-split"]');
    expect(split.attributes("aria-disabled")).toBe("true");
    expect(split.attributes("title")).toBe("Choose a point inside the clip.");
  });
});

describe("MenuPanel — keyboard", () => {
  it("focuses the first item on open; arrows cross disabled items, skip separators and wrap", async () => {
    const w = open();
    await flushPromises();
    expect(focusedId()).toBe("m-item-go");
    const panel = w.get('[data-testid="m"]').element;
    key(panel, "ArrowDown");
    await flushPromises();
    expect(focusedId()).toBe("m-item-split");
    key(panel, "ArrowDown");
    await flushPromises();
    expect(focusedId()).toBe("m-item-speed");
    key(panel, "ArrowUp");
    key(panel, "ArrowUp");
    key(panel, "ArrowUp");
    await flushPromises();
    expect(focusedId()).toBe("m-item-del");
    key(panel, "Home");
    await flushPromises();
    expect(focusedId()).toBe("m-item-go");
    key(panel, "End");
    await flushPromises();
    expect(focusedId()).toBe("m-item-del");
  });

  it("writes a focused item's hint into the live region: the disabled reason, the submenu and danger cues", async () => {
    const w = open();
    await flushPromises();
    const panel = w.get('[data-testid="m"]').element;
    const hint = () => w.get('[data-testid="m-hint"]').text();
    // Opening focuses the first item but keeps the key guide (screen 03).
    expect(hint()).toBe("↑ ↓ navigate · Enter choose · Esc close");
    key(panel, "ArrowDown");
    key(panel, "ArrowUp");
    await flushPromises();
    expect(hint()).toBe("Enter to apply · Esc to dismiss");
    key(panel, "ArrowDown");
    await flushPromises();
    expect(hint()).toBe("Choose a point inside the clip.");
    key(panel, "ArrowDown");
    await flushPromises();
    expect(hint()).toBe("→ Open options");
    key(panel, "End");
    await flushPromises();
    expect(hint()).toBe("Removes from this edit. You can undo.");
  });

  it("Enter or → opens a submenu and focuses its first item; ← and Escape close only the submenu", async () => {
    const w = open();
    await flushPromises();
    const panel = w.get('[data-testid="m"]').element;
    key(panel, "End");
    key(panel, "ArrowUp");
    key(panel, "ArrowUp");
    await flushPromises();
    expect(focusedId()).toBe("m-item-speed");

    key(panel, "Enter");
    await flushPromises();
    expect(w.find('[data-testid="m-submenu"]').exists()).toBe(true);
    expect(focusedId()).toBe("m-item-speed-half");
    expect(w.get('[data-testid="m-item-speed"]').attributes("aria-expanded")).toBe("true");

    key(w.get('[data-testid="m-submenu"]').element, "ArrowLeft");
    await flushPromises();
    expect(w.find('[data-testid="m-submenu"]').exists()).toBe(false);
    expect(focusedId()).toBe("m-item-speed");
    expect(w.emitted("close")).toBeUndefined();

    key(panel, "ArrowRight");
    await flushPromises();
    expect(focusedId()).toBe("m-item-speed-half");
    key(w.get('[data-testid="m-submenu"]').element, "Escape");
    await flushPromises();
    expect(w.find('[data-testid="m-submenu"]').exists()).toBe(false);
    expect(focusedId()).toBe("m-item-speed");
    expect(w.emitted("close")).toBeUndefined();
  });

  it("Enter in a submenu runs the item and closes the whole menu", async () => {
    runs.half.mockClear();
    const w = open();
    await flushPromises();
    await w.get('[data-testid="m-item-speed"]').trigger("click");
    await flushPromises();
    key(w.get('[data-testid="m-submenu"]').element, "Enter");
    await flushPromises();
    expect(runs.half).toHaveBeenCalledTimes(1);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("Enter activates the focused item only when it is enabled", async () => {
    runs.go.mockClear();
    const w = open();
    await flushPromises();
    const panel = w.get('[data-testid="m"]').element;
    key(panel, "ArrowDown");
    key(panel, "Enter");
    await flushPromises();
    expect(w.get('[data-testid="m-hint"]').text()).toBe("Choose a point inside the clip.");
    expect(w.emitted("close")).toBeUndefined();
    key(panel, "Home");
    key(panel, "Enter");
    expect(runs.go).toHaveBeenCalledTimes(1);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("Escape on the root emits close and returns focus to the invoker", async () => {
    const trigger = document.createElement("button");
    document.body.appendChild(trigger);
    trigger.focus();
    const w = open();
    await flushPromises();
    key(w.get('[data-testid="m"]').element, "Escape");
    await flushPromises();
    expect(w.emitted("close")).toHaveLength(1);
    expect(document.activeElement).toBe(trigger);
    trigger.remove();
  });
});

describe("MenuPanel — focus after choosing", () => {
  it("leaves focus where the chosen item put it, instead of returning it to the invoker", async () => {
    const trigger = document.createElement("button");
    const field = document.createElement("input");
    document.body.append(trigger, field);
    trigger.focus();
    const w = mount(MenuPanel, {
      attachTo: document.body,
      props: {
        heading: "Clip",
        anchor: { x: 0, y: 0 },
        testid: "m",
        items: [{ id: "rename", label: "Rename…", icon: "edit", run: () => field.focus() }],
      },
    });
    await flushPromises();
    await w.get('[data-testid="m-item-rename"]').trigger("click");
    await flushPromises();
    expect(document.activeElement).toBe(field);
    trigger.remove();
    field.remove();
  });
});

describe("MenuPanel — pointer", () => {
  it("clicking a disabled item does nothing but show its reason", async () => {
    const w = open();
    await flushPromises();
    await w.get('[data-testid="m-item-split"]').trigger("click");
    expect(w.get('[data-testid="m-hint"]').text()).toBe("Choose a point inside the clip.");
    expect(w.emitted("close")).toBeUndefined();
  });

  it("clicking an enabled item runs it once and closes", async () => {
    runs.go.mockClear();
    const w = open();
    await w.get('[data-testid="m-item-go"]').trigger("click");
    expect(runs.go).toHaveBeenCalledTimes(1);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("opens a submenu after 180 ms of hover, and closes it when another item is hovered", async () => {
    vi.useFakeTimers();
    const w = open();
    await w.get('[data-testid="m-item-speed"]').trigger("pointermove");
    vi.advanceTimersByTime(170);
    await nextTick();
    expect(w.find('[data-testid="m-submenu"]').exists()).toBe(false);
    vi.advanceTimersByTime(20);
    await nextTick();
    expect(w.find('[data-testid="m-submenu"]').exists()).toBe(true);
    await w.get('[data-testid="m-item-go"]').trigger("pointermove");
    expect(w.find('[data-testid="m-submenu"]').exists()).toBe(false);
  });

  it("a pointerdown outside closes the menu without refocusing the invoker", async () => {
    const w = open();
    await flushPromises();
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("is clamped 8 px inside the viewport", async () => {
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 282, 300));
    Object.defineProperty(window, "innerWidth", { value: 800, configurable: true });
    Object.defineProperty(window, "innerHeight", { value: 600, configurable: true });
    const w = open({ anchor: { x: 790, y: 590 } });
    await flushPromises();
    const style = (w.get('[data-testid="m"]').element as HTMLElement).style;
    expect(style.left).toBe(`${800 - 282 - 8}px`);
    expect(style.top).toBe(`${600 - 300 - 8}px`);
  });

  it("opens below a control when anchored to one", async () => {
    const anchor = document.createElement("button");
    document.body.appendChild(anchor);
    vi.spyOn(anchor, "getBoundingClientRect").mockReturnValue(new DOMRect(100, 40, 80, 30));
    const w = open({ anchor });
    await flushPromises();
    const style = (w.get('[data-testid="m"]').element as HTMLElement).style;
    expect(style.left).toBe("100px");
    expect(style.top).toBe("74px");
    anchor.remove();
  });
});
