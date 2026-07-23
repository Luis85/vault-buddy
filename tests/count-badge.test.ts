import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import CountBadge from "../src/components/ui/CountBadge.vue";

describe("CountBadge", () => {
  it("renders nothing for zero", () => {
    const w = mount(CountBadge, { props: { count: 0 } });
    expect(w.find("span").exists()).toBe(false);
  });

  it("renders the count", () => {
    const w = mount(CountBadge, { props: { count: 7 } });
    expect(w.get("span").text()).toBe("7");
  });

  it("caps at max with a plus (default 99)", () => {
    const w = mount(CountBadge, { props: { count: 250 } });
    expect(w.get("span").text()).toBe("99+");
  });
});
