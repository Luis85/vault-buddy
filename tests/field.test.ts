import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Field from "../src/components/ui/Field.vue";

describe("Field", () => {
  it("reflects modelValue and emits update on input", async () => {
    const w = mount(Field, { props: { modelValue: "hi" } });
    const input = w.get("input");
    expect((input.element as HTMLInputElement).value).toBe("hi");
    await input.setValue("bye");
    expect(w.emitted("update:modelValue")).toEqual([["bye"]]);
  });

  it("passes through native attrs to the input root", () => {
    const w = mount(Field, {
      props: { modelValue: "" },
      attrs: { type: "search", placeholder: "Filter…", "aria-label": "Filter vaults" },
    });
    const input = w.get("input");
    expect(input.attributes("type")).toBe("search");
    expect(input.attributes("placeholder")).toBe("Filter…");
    expect(input.attributes("aria-label")).toBe("Filter vaults");
  });
});
