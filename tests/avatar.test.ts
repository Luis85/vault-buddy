import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Avatar from "../src/components/ui/Avatar.vue";

describe("Avatar", () => {
  it("shows the uppercased initial", () => {
    const w = mount(Avatar, { props: { name: "personal" } });
    expect(w.get("span").text()).toBe("P");
    expect(w.get("span").attributes("aria-hidden")).toBe("true");
  });

  it("sizes sm vs md", () => {
    const sm = mount(Avatar, { props: { name: "Work", size: "sm" } });
    expect(sm.get("span").classes()).toContain("h-4");
    const md = mount(Avatar, { props: { name: "Work" } });
    expect(md.get("span").classes()).toContain("h-7");
  });
});
