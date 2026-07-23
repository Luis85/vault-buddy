import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import IconButton from "../src/components/ui/IconButton.vue";

describe("IconButton", () => {
  it("renders the slot and uses label as aria-label + title fallback", () => {
    const w = mount(IconButton, { props: { label: "Search" }, slots: { default: "<svg/>" } });
    const btn = w.get("button");
    expect(btn.attributes("aria-label")).toBe("Search");
    expect(btn.attributes("title")).toBe("Search");
    expect(w.find("svg").exists()).toBe(true);
  });

  it("carries the shared focus ring", () => {
    const w = mount(IconButton, { props: { label: "X" } });
    expect(w.get("button").classes()).toContain("focus-visible:ring-focus");
  });

  it("emits click when enabled and is inert when disabled", async () => {
    const w = mount(IconButton, { props: { label: "X", disabled: true } });
    expect(w.get("button").attributes("disabled")).toBeDefined();
    const enabled = mount(IconButton, { props: { label: "X" } });
    await enabled.get("button").trigger("click");
    expect(enabled.emitted("click")).toHaveLength(1);
  });

  it("prefers an explicit title over the label", () => {
    const w = mount(IconButton, { props: { label: "Aria", title: "Tip" } });
    expect(w.get("button").attributes("title")).toBe("Tip");
  });
});
