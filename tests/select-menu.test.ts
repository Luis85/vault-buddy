import { flushPromises, mount } from "@vue/test-utils";
import { afterEach, describe, expect, it } from "vitest";

import SelectMenu from "../src/components/SelectMenu.vue";

const OPTIONS = [
  { value: "", label: "Auto-detect" },
  { value: "de", label: "German" },
  { value: "es", label: "Spanish" },
];

// The popup is Teleported to <body>; unmount removes it between tests.
let active: ReturnType<typeof mount> | null = null;
afterEach(() => {
  active?.unmount();
  active = null;
  document.body.innerHTML = "";
});

function mountMenu(modelValue: string, dataTestid = "lang") {
  active = mount(SelectMenu, {
    props: { modelValue, options: OPTIONS, dataTestid },
    attachTo: document.body,
  });
  return active;
}

describe("SelectMenu", () => {
  it("shows the selected option's label on the trigger", () => {
    const w = mountMenu("de");
    expect(w.get('[data-testid="lang"]').text()).toContain("German");
  });

  it("opens on click and lists the options", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    const options = document.body.querySelectorAll('[role="option"]');
    expect(options.length).toBe(3);
  });

  it("emits the chosen value and closes when an option is clicked", async () => {
    const w = mountMenu("");
    await w.get('[data-testid="lang"]').trigger("click");
    (document.body.querySelector('[data-testid="lang-option-de"]') as HTMLElement).click();
    await flushPromises();
    expect(w.emitted("update:modelValue")).toEqual([["de"]]);
    expect(document.body.querySelector('[role="option"]')).toBeNull();
  });

  it("marks expanded state on the trigger", async () => {
    const w = mountMenu("");
    expect(w.get('[data-testid="lang"]').attributes("aria-expanded")).toBe("false");
    await w.get('[data-testid="lang"]').trigger("click");
    expect(w.get('[data-testid="lang"]').attributes("aria-expanded")).toBe("true");
  });
});
