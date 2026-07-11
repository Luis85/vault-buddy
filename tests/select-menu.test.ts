import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it, vi } from "vitest";

import SelectMenu from "../src/components/SelectMenu.vue";

const OPTIONS = [
  { value: "", label: "Auto-detect" },
  { value: "de", label: "German" },
  { value: "es", label: "Spanish" },
];

// The popup is Teleported to <body>; unmount removes it between tests.
let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  document.body.innerHTML = "";
});

function mountMenu(modelValue: string, dataTestid = "lang") {
  active = mount(SelectMenu, {
    props: { modelValue, options: OPTIONS, dataTestid },
    attachTo: document.body,
  });
  return active;
}

describe("SelectMenu", () => {
  it("shows the selected option's label on the trigger", () => {
    const w = mountMenu("de");
    expect(w.get('[data-testid="lang"]').text()).toContain("German");
  });

  it("opens on click and lists the options", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    const options = document.body.querySelectorAll('[role="option"]');
    expect(options.length).toBe(3);
  });

  it("emits the chosen value and closes when an option is clicked", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    (document.body.querySelector('[data-testid="lang-option-de"]') as HTMLElement).click();
    await flushPromises();
    expect(w.emitted("update:modelValue")).toEqual([["de"]]);
    expect(document.body.querySelector('[role="option"]')).toBeNull();
  });

  it("marks expanded state on the trigger", async () => {
    const w = mountMenu("");
    expect(w.get('[data-testid="lang"]').attributes("aria-expanded")).toBe("false");
    await w.get('[data-testid="lang"]').trigger("click");
    expect(w.get('[data-testid="lang"]').attributes("aria-expanded")).toBe("true");
  });

  it("Escape closes the menu without reaching window listeners (GAP-27)", async () => {
    // Dismissing a dropdown used to bubble Escape to window, where PanelRoot
    // closes the whole panel.
    const windowSpy = vi.fn();
    window.addEventListener("keydown", windowSpy);
    try {
      const w = mountMenu("");
      await w.get('[data-testid="lang"]').trigger("click");
      const popup = document.body.querySelector('[role="listbox"]') as HTMLElement;
      await popup.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
      );
      await flushPromises();
      expect(document.body.querySelector('[role="option"]')).toBeNull();
      expect(windowSpy).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener("keydown", windowSpy);
    }
  });

  it("stays open when the popup's own option list scrolls", async () => {
    // User-reported: with enough options the popup itself scrolls (max-height
    // ~220px), and the capture-phase window scroll listener closed the menu on
    // the popup's OWN scroll — the lower options were unreachable by wheel or
    // scrollbar. Scroll doesn't bubble, but a capture listener sees it anyway.
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    const popup = document.body.querySelector('[role="listbox"]') as HTMLElement;
    popup.dispatchEvent(new Event("scroll"));
    await flushPromises();
    expect(document.body.querySelector('[role="listbox"]')).not.toBeNull();
  });

  it("stays open (repositioning) when the page behind scrolls", async () => {
    // Scrolling the panel content must track the trigger, not dismiss the
    // menu — the popup is position:fixed, so it repositions instead.
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    document.body.dispatchEvent(new Event("scroll"));
    await flushPromises();
    expect(document.body.querySelector('[role="listbox"]')).not.toBeNull();
  });

  it("still closes on a pointerdown outside the popup", async () => {
    // Pin the dismissal path that remains after the scroll fix.
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    document.body.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
    await flushPromises();
    expect(document.body.querySelector('[role="listbox"]')).toBeNull();
  });

  it("moves aria-activedescendant with the keyboard highlight (GAP-33)", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    const ul = document.body.querySelector('[role="listbox"]') as HTMLElement;
    ul.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    await flushPromises();
    const active = ul.getAttribute("aria-activedescendant");
    expect(active).toBeTruthy();
    expect(document.getElementById(active!)?.textContent).toContain("German");
  });

  it("Home and End jump to the first and last option (GAP-33)", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    const ul = document.body.querySelector('[role="listbox"]') as HTMLElement;
    ul.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    await flushPromises();
    expect(ul.getAttribute("aria-activedescendant")).toMatch(/-opt-2$/);
    ul.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    await flushPromises();
    expect(ul.getAttribute("aria-activedescendant")).toMatch(/-opt-0$/);
  });
});
