import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Banner from "../src/components/ui/Banner.vue";

describe("Banner", () => {
  it("defaults to the danger tone", () => {
    const w = mount(Banner, { slots: { default: "Boom" } });
    const p = w.get("p");
    expect(p.text()).toBe("Boom");
    expect(p.classes()).toEqual(expect.arrayContaining(["bg-red-500/20", "text-red-200"]));
  });

  it("switches tone classes", () => {
    const w = mount(Banner, { props: { tone: "success" }, slots: { default: "ok" } });
    expect(w.get("p").classes()).toContain("text-emerald-200");
  });
});
