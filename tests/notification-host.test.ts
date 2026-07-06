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
});
