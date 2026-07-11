import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import NotificationHost from "../src/components/NotificationHost.vue";
import { useNotificationsStore } from "../src/stores/notifications";

describe("NotificationHost", () => {
  beforeEach(() => setActivePinia(createPinia()));
  it("renders items with kind styling and role, and dismisses", async () => {
    const n = useNotificationsStore(); n.error("boom");
    const w = mount(NotificationHost);
    const item = w.get('[data-testid="notification"]');
    expect(item.text()).toContain("boom");
    expect(item.attributes("role")).toBe("alert");
    await w.get('[data-testid="notification-dismiss"]').trigger("click");
    expect(n.items).toHaveLength(0);
    expect(w.find('[data-testid="notification"]').exists()).toBe(false);
  });

  it("sets aria-live assertive for an error and polite for a non-error kind", () => {
    const n = useNotificationsStore();
    n.error("boom");
    n.warning("careful");
    const w = mount(NotificationHost);
    const items = w.findAll('[data-testid="notification"]');
    expect(items).toHaveLength(2);
    expect(items[0]!.attributes("aria-live")).toBe("assertive");
    expect(items[1]!.attributes("aria-live")).toBe("polite");
  });

  it("renders the expected color class for a non-error kind", () => {
    const n = useNotificationsStore();
    n.warning("careful");
    n.success("done");
    const w = mount(NotificationHost);
    const items = w.findAll('[data-testid="notification"]');
    expect(items[0]!.classes()).toContain("text-amber-50");
    expect(items[1]!.classes()).toContain("text-emerald-50");
  });

  it("gives each toast an opaque background so it stays readable over the translucent panel", () => {
    // Regression: toasts used bg-red-500/20 (and /15 for others) — a ~15-20%
    // tint over the semi-transparent panel window left the text barely
    // legible ("not readable due to its transparency"). Each kind must now use
    // a solid, high-contrast background, never the low-alpha variants.
    const n = useNotificationsStore();
    n.error("boom");
    n.warning("careful");
    n.success("done");
    n.info("fyi");
    const w = mount(NotificationHost);
    const items = w.findAll('[data-testid="notification"]');
    const bgOf = (i: number) =>
      items[i]!.classes().filter((c) => c.startsWith("bg-"));
    // No toast may keep a translucent (alpha-suffixed, e.g. /20, /15, /10)
    // background — that is exactly the transparency that made them unreadable.
    for (const item of items) {
      for (const cls of item.classes().filter((c) => c.startsWith("bg-"))) {
        expect(cls).not.toContain("/");
      }
    }
    expect(bgOf(0)).toContain("bg-red-900"); // error
    expect(bgOf(1)).toContain("bg-amber-900"); // warning
    expect(bgOf(2)).toContain("bg-emerald-900"); // success
    expect(bgOf(3)).toContain("bg-slate-800"); // info
  });

  it("renders an action button that runs the action and dismisses the toast", async () => {
    const n = useNotificationsStore();
    const run = vi.fn();
    n.notify("success", "Imported X", { action: { label: "Open", run } });
    const w = mount(NotificationHost);
    const action = w.get('[data-testid="notification-action"]');
    expect(action.text()).toBe("Open");
    await action.trigger("click");
    expect(run).toHaveBeenCalledTimes(1);
    expect(n.items).toHaveLength(0); // clicking the action dismisses the toast
  });

  it("surfaces an error when a toast action fails instead of dismissing it silently", async () => {
    // A failed Open (vault removed, OS can't launch obsidian://) must not just
    // vanish the toast — the user needs to know the action didn't work.
    const n = useNotificationsStore();
    const run = vi.fn().mockRejectedValue(new Error("no obsidian handler"));
    n.notify("success", "Imported X", { action: { label: "Open in Obsidian", run } });
    const w = mount(NotificationHost);
    await w.get('[data-testid="notification-action"]').trigger("click");
    await flushPromises();
    expect(
      n.items.some((i) => i.kind === "error" && i.message.includes("no obsidian handler")),
    ).toBe(true);
    // the original actionable toast is gone — the error toast reports it now
    expect(n.items.some((i) => i.message === "Imported X")).toBe(false);
  });

  it("renders no action button for a plain toast", () => {
    const n = useNotificationsStore();
    n.success("done");
    const w = mount(NotificationHost);
    expect(w.find('[data-testid="notification-action"]').exists()).toBe(false);
  });

  it("keeps the container pointer-events-none while each toast is pointer-events-auto", () => {
    // The host overlays every panel view (Task 3/5) — it must never itself
    // intercept clicks outside a toast; only an actual toast (and its
    // dismiss button) should be clickable.
    const n = useNotificationsStore();
    n.error("boom");
    const w = mount(NotificationHost);
    expect(w.get('[data-testid="notification-host"]').classes()).toContain(
      "pointer-events-none",
    );
    expect(w.get('[data-testid="notification"]').classes()).toContain(
      "pointer-events-auto",
    );
  });
});
