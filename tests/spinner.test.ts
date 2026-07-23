// tests/spinner.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import Spinner from "../src/components/ui/Spinner.vue";

describe("Spinner", () => {
  it("renders a spinning status ring with a default label", () => {
    const w = mount(Spinner);
    const el = w.get("[data-testid='spinner']");
    expect(el.classes()).toEqual(expect.arrayContaining(["animate-spin", "rounded-full"]));
    expect(el.attributes("role")).toBe("status");
    expect(el.attributes("aria-label")).toBe("Loading…");
  });

  it("honors a custom label and size", () => {
    const w = mount(Spinner, { props: { label: "Opening vault…", size: "md" } });
    const el = w.get("[data-testid='spinner']");
    expect(el.attributes("aria-label")).toBe("Opening vault…");
    expect(el.classes()).toContain("h-5");
  });
});
