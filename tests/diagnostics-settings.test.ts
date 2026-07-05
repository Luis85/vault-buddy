import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

import DiagnosticsSettings from "../src/components/DiagnosticsSettings.vue";

describe("DiagnosticsSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => clearMocks());

  it("invokes open_logs_folder when the button is clicked", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return undefined;
    });
    const wrapper = mount(DiagnosticsSettings);
    expect(wrapper.text()).toContain("Diagnostics");
    await wrapper.find('[data-testid="open-logs"]').trigger("click");
    expect(calls).toContain("open_logs_folder");
  });
});
