import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import BubbleRoot from "../src/roots/BubbleRoot.vue";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

describe("BubbleRoot", () => {
  beforeEach(() => {
    mockIPC(() => {});
  });
  afterEach(() => clearMocks());

  it("renders the greeting text", async () => {
    const wrapper = mount(BubbleRoot);
    await Promise.resolve();
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);
  });
});
