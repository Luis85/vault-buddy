import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, it } from "vitest";
import { defineComponent } from "vue";

import { useSuppressContextMenu } from "../src/composables/useSuppressContextMenu";

const Host = defineComponent({
  setup() {
    useSuppressContextMenu();
  },
  template: `<div class="host"><input class="field" /></div>`,
});

describe("useSuppressContextMenu", () => {
  let wrapper: ReturnType<typeof mount> | null = null;
  afterEach(() => {
    wrapper?.unmount();
    wrapper = null;
  });

  it("suppresses the stock menu outside text fields", () => {
    wrapper = mount(Host, { attachTo: document.body });
    const ev = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
    });
    wrapper.get(".host").element.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });

  it("leaves the native menu on inputs so copy/paste works", () => {
    wrapper = mount(Host, { attachTo: document.body });
    const ev = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
    });
    wrapper.get(".field").element.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
  });

  it("removes the listener on unmount", () => {
    wrapper = mount(Host, { attachTo: document.body });
    wrapper.unmount();
    wrapper = null;
    const ev = new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
    });
    document.body.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(false);
  });
});
