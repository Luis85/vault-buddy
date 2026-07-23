import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Chip from "../src/components/ui/Chip.vue";

describe("Chip", () => {
  it("renders a static span for neutral/accent", () => {
    const w = mount(Chip, { slots: { default: "3" } });
    expect(w.get("span").text()).toBe("3");
    expect(w.find("button").exists()).toBe(false);
  });

  it("renders an interactive button that emits click", async () => {
    const w = mount(Chip, { props: { variant: "interactive", label: "Filter by tag work" }, slots: { default: "#work" } });
    const btn = w.get("button");
    expect(btn.attributes("aria-label")).toBe("Filter by tag work");
    await btn.trigger("click");
    expect(w.emitted("click")).toHaveLength(1);
  });

  it("uses the micro type size", () => {
    const w = mount(Chip, { slots: { default: "x" } });
    expect(w.get("span").classes()).toContain("text-micro");
  });
});
