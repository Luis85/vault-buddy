import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import PanelSizeSetting from "../src/components/PanelSizeSetting.vue";

describe("PanelSizeSetting", () => {
  it("renders the three presets and marks the selected one", () => {
    const w = mount(PanelSizeSetting, { props: { modelValue: "comfortable" } });
    const btns = w.findAll("button");
    expect(btns.map((b) => b.text())).toEqual(["Compact", "Comfortable", "Large"]);
    // role="radio" takes aria-checked (aria-pressed is a toggle-button state);
    // matches every other segmented radio in the app.
    expect(w.get('[data-testid="panel-size-comfortable"]').attributes("aria-checked")).toBe("true");
    expect(w.get('[data-testid="panel-size-large"]').attributes("aria-checked")).toBe("false");
  });

  it("emits the chosen size", async () => {
    const w = mount(PanelSizeSetting, { props: { modelValue: "comfortable" } });
    await w.get('[data-testid="panel-size-large"]').trigger("click");
    expect(w.emitted("update:modelValue")).toEqual([["large"]]);
  });

  it("disables every preset while the disabled prop is set", () => {
    const w = mount(PanelSizeSetting, {
      props: { modelValue: "comfortable", disabled: true },
    });
    expect(
      w.findAll("button").every((b) => b.attributes("disabled") !== undefined),
    ).toBe(true);
  });
});
