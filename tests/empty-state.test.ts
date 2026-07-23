// tests/empty-state.test.ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import EmptyState from "../src/components/ui/EmptyState.vue";

describe("EmptyState", () => {
  it("renders the title verbatim and nothing else by default", () => {
    const w = mount(EmptyState, { props: { title: "No tasks yet." } });
    // .text() must equal the title exactly, so a `<p>`→EmptyState swap keeps
    // existing `.text()` assertions green.
    expect(w.text()).toBe("No tasks yet.");
    expect(w.get("[data-testid='empty-state']").exists()).toBe(true);
  });

  it("renders an aria-hidden icon slot without adding text", () => {
    const w = mount(EmptyState, {
      props: { title: "No recordings yet." },
      slots: { icon: "<svg data-testid='ic'/>" },
    });
    expect(w.find("[data-testid='ic']").exists()).toBe(true);
    expect(w.get("[data-testid='empty-state-icon']").attributes("aria-hidden")).toBe("true");
    expect(w.text()).toBe("No recordings yet."); // icon contributes no text
  });

  it("renders the optional hint and action slot", () => {
    const w = mount(EmptyState, {
      props: { title: "Obsidian not found", hint: "Install it first." },
      slots: { action: "<button>Retry</button>" },
    });
    expect(w.text()).toContain("Obsidian not found");
    expect(w.text()).toContain("Install it first.");
    expect(w.find("button").exists()).toBe(true);
  });
});
