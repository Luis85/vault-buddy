import { mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import DocumentImportSettings from "../src/components/DocumentImportSettings.vue";

describe("DocumentImportSettings", () => {
  it("shows Not Installed and re-detects on Recheck", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return calls === 1
          ? { installed: false, version: null, path: null, sandboxSupported: false, configuredPath: null }
          : { installed: true, version: "pandoc 3.1.9", path: "pandoc", sandboxSupported: true, configuredPath: null };
      }
      return undefined;
    });
    const wrapper = mount(DocumentImportSettings);
    await new Promise((r) => setTimeout(r));
    expect(wrapper.text()).toContain("Not installed");
    await wrapper.get('[data-testid="pandoc-recheck"]').trigger("click");
    await new Promise((r) => setTimeout(r));
    expect(wrapper.text()).toContain("3.1.9");
  });
});
