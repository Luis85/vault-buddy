/**
 * `DialogHost.vue` and the shared `dialogs.ts` stack it registers on —
 * Task 19. A controlled component (`ContextMenu.vue`'s own contract): every
 * test drives `open` via `setProps`, the way `editorContextMenu.test.ts`
 * drives `ContextMenu`'s own `open` prop.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import DialogHost from "../src/components/editor/shell/DialogHost.vue";
import { dialogStackSize } from "../src/editor/dialogs";
import { useEditorOnboardingStore } from "../src/stores/editorOnboarding";

enableAutoUnmount(afterEach);
beforeEach(() => setActivePinia(createPinia()));

function mountDialog(open: boolean, closable = true) {
  return mount(DialogHost, {
    attachTo: document.body,
    props: { open, label: "Test dialog", closable },
    slots: {
      default: `<button data-testid="first">First</button><button data-testid="second">Second</button>`,
    },
  });
}

describe("DialogHost", () => {
  it("dialog restores focus to the opener", async () => {
    const opener = document.createElement("button");
    document.body.appendChild(opener);
    opener.focus();
    expect(document.activeElement).toBe(opener);

    const w = mountDialog(false);
    await w.setProps({ open: true });
    await flushPromises();
    // Opening moves focus into the dialog's first focusable element.
    expect(document.activeElement).not.toBe(opener);
    expect(document.activeElement?.getAttribute("data-testid")).toBe("first");

    await w.setProps({ open: false });
    await flushPromises();
    expect(document.activeElement).toBe(opener);

    opener.remove();
  });

  it("dialog host announces suspend and resume", async () => {
    const w = mountDialog(false);
    expect(w.emitted("suspend")).toBeUndefined();
    expect(w.emitted("resume")).toBeUndefined();

    await w.setProps({ open: true });
    await flushPromises();
    expect(w.emitted("suspend")).toHaveLength(1);
    expect(w.emitted("resume")).toBeUndefined();

    await w.setProps({ open: false });
    await flushPromises();
    expect(w.emitted("resume")).toHaveLength(1);
    expect(w.emitted("suspend")).toHaveLength(1); // still exactly once
  });

  // Task 56: every modal dialog suspends the guide's coach while it is open
  // (ONBOARDING.md, A25) -- and a dialog stacked on another keeps it
  // suspended until the LAST one closes, not the first.
  it("suspends the guide while open, until the last stacked dialog closes", async () => {
    const guide = useEditorOnboardingStore();
    const outer = mountDialog(false);
    const inner = mountDialog(false);

    await outer.setProps({ open: true });
    await inner.setProps({ open: true });
    await flushPromises();
    expect(guide.suspended).toBe(true);

    await inner.setProps({ open: false });
    await flushPromises();
    expect(guide.suspended).toBe(true);

    await outer.setProps({ open: false });
    await flushPromises();
    expect(guide.suspended).toBe(false);
  });

  it("a dialog removed while open still resumes the guide", async () => {
    const guide = useEditorOnboardingStore();
    const w = mountDialog(true);
    await flushPromises();
    expect(guide.suspended).toBe(true);
    w.unmount();
    expect(guide.suspended).toBe(false);
  });

  it("Tab cycles focus inside the dialog (the focus trap)", async () => {
    const w = mountDialog(true);
    await flushPromises();
    const first = w.get('[data-testid="first"]').element as HTMLElement;
    const second = w.get('[data-testid="second"]').element as HTMLElement;
    expect(document.activeElement).toBe(first);

    second.focus();
    await w.get('[data-testid="dialog-host-content"]').trigger("keydown", { key: "Tab" });
    expect(document.activeElement).toBe(first); // wraps forward past the last

    await w.get('[data-testid="dialog-host-content"]').trigger("keydown", {
      key: "Tab",
      shiftKey: true,
    });
    expect(document.activeElement).toBe(second); // wraps backward past the first
  });

  it("Escape does nothing when the dialog is not closable", async () => {
    const w = mountDialog(true, false);
    await flushPromises();
    await w.get('[data-testid="dialog-host-content"]').trigger("keydown", { key: "Escape" });
    expect(w.emitted("close")).toBeUndefined();
  });

  it("Escape closes the dialog when it allows", async () => {
    const w = mountDialog(true, true);
    await flushPromises();
    await w.get('[data-testid="dialog-host-content"]').trigger("keydown", { key: "Escape" });
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("a backdrop click closes a closable dialog", async () => {
    const w = mountDialog(true);
    await flushPromises();
    await w.get('[data-testid="dialog-host"]').trigger("mousedown");
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("only the top-of-stack dialog answers Escape, and the stack unwinds on close", async () => {
    const outer = mountDialog(true);
    await flushPromises();
    expect(dialogStackSize()).toBe(1);

    const inner = mount(DialogHost, {
      attachTo: document.body,
      props: { open: true, label: "Inner dialog" },
      slots: { default: `<button data-testid="inner-btn">Inner</button>` },
    });
    await flushPromises();
    expect(dialogStackSize()).toBe(2);

    // The outer dialog's own Escape is inert while a later one is on top of it.
    await outer.get('[data-testid="dialog-host-content"]').trigger("keydown", { key: "Escape" });
    expect(outer.emitted("close")).toBeUndefined();

    await inner.get('[data-testid="dialog-host-content"]').trigger("keydown", { key: "Escape" });
    expect(inner.emitted("close")).toHaveLength(1);

    await inner.setProps({ open: false });
    await flushPromises();
    expect(dialogStackSize()).toBe(1);

    await outer.setProps({ open: false });
    await flushPromises();
    expect(dialogStackSize()).toBe(0);
  });
});
