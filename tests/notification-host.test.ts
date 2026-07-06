import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
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
    expect(items[0]!.classes()).toContain("text-amber-200");
    expect(items[1]!.classes()).toContain("text-emerald-200");
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
