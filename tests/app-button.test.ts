import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import AppButton from "../src/components/ui/AppButton.vue";

describe("AppButton", () => {
  it("renders label slot and defaults to a primary button", () => {
    const w = mount(AppButton, { slots: { default: "Save" } });
    expect(w.text()).toBe("Save");
    expect(w.get("button").attributes("type")).toBe("button");
    expect(w.get("button").classes()).toContain("bg-accent");
  });

  it("applies the secondary variant classes", () => {
    const w = mount(AppButton, { props: { variant: "secondary" }, slots: { default: "x" } });
    expect(w.get("button").classes()).toContain("bg-white/5");
  });

  it("emits click; disabled suppresses it", async () => {
    const w = mount(AppButton, { slots: { default: "x" } });
    await w.get("button").trigger("click");
    expect(w.emitted("click")).toHaveLength(1);
    const d = mount(AppButton, { props: { disabled: true }, slots: { default: "x" } });
    expect(d.get("button").attributes("disabled")).toBeDefined();
  });

  it("supports type=submit for form composers", () => {
    const w = mount(AppButton, { props: { type: "submit" }, slots: { default: "x" } });
    expect(w.get("button").attributes("type")).toBe("submit");
  });
});
