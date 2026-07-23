import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import SectionHeader from "../src/components/ui/SectionHeader.vue";

describe("SectionHeader", () => {
  it("renders an h2 with the subtle uppercase treatment", () => {
    const w = mount(SectionHeader, { slots: { default: "Favorites" } });
    const h = w.get("h2");
    expect(h.text()).toBe("Favorites");
    expect(h.classes()).toEqual(expect.arrayContaining(["uppercase", "text-fg-subtle"]));
  });
});
