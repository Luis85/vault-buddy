import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";

// The component listens for mcp:status pushes; capture the handler so tests
// can fire one.
const listeners: Record<string, (e: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, cb: (e: { payload: unknown }) => void) => {
    listeners[name] = cb;
    return Promise.resolve(() => delete listeners[name]);
  },
}));

import McpSettings from "../src/components/McpSettings.vue";

const baseConfig = {
  enabled: false,
  port: 22082,
  allowWrites: false,
  token: "",
  status: { state: "stopped", port: null, message: null },
};

describe("McpSettings", () => {
  beforeEach(() => clearMocks());
  afterEach(() => clearMocks());

  it("loads config on mount and renders the stopped status", async () => {
    mockIPC((cmd) => (cmd === "get_mcp_config" ? { ...baseConfig } : undefined));
    const wrapper = mount(McpSettings);
    await flushPromises();
    expect(wrapper.text()).toContain("MCP server");
    expect(wrapper.text()).toContain("Stopped");
    expect(wrapper.text()).toContain("Allow vault writes");
  });

  it("enabling saves via set_mcp_config and shows the running port", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "get_mcp_config") return { ...baseConfig };
      if (cmd === "set_mcp_config")
        return {
          ...baseConfig,
          enabled: true,
          token: "tok123",
          status: { state: "running", port: 22082, message: null },
        };
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-enabled"]').setValue(true);
    await flushPromises();
    const set = calls.find((c) => c.cmd === "set_mcp_config");
    expect(set).toBeTruthy();
    expect((set!.args as { input: { enabled: boolean } }).input.enabled).toBe(true);
    expect(wrapper.text()).toContain("Running on 127.0.0.1:22082");
    // Client snippets render the live port + token.
    expect(wrapper.text()).toContain("claude mcp add");
    expect(wrapper.text()).toContain("tok123");
  });

  it("serializes saves: controls disable while a save is in flight", async () => {
    // Two quick toggles would otherwise race: the second request is built
    // from a pre-response snapshot and can undo the first (Codex review
    // catch). With controls disabled during a save, the stale-snapshot
    // request can never be issued.
    let resolveSet: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "get_mcp_config") return { ...baseConfig };
      if (cmd === "set_mcp_config")
        return new Promise((res) => {
          resolveSet = res;
        });
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-enabled"]').setValue(true);
    expect(
      wrapper.find('[data-testid="mcp-writes"]').attributes("disabled"),
    ).toBeDefined();
    resolveSet({
      ...baseConfig,
      enabled: true,
      token: "tok123",
      status: { state: "running", port: 22082, message: null },
    });
    await flushPromises();
    expect(
      wrapper.find('[data-testid="mcp-writes"]').attributes("disabled"),
    ).toBeUndefined();
  });

  it("regenerate calls the command and mcp:status pushes update the badge", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "get_mcp_config")
        return { ...baseConfig, enabled: true, token: "old" };
      if (cmd === "regenerate_mcp_token")
        return { ...baseConfig, enabled: true, token: "fresh" };
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-regenerate"]').trigger("click");
    await flushPromises();
    expect(calls).toContain("regenerate_mcp_token");
    expect(wrapper.text()).toContain("fresh");
    listeners["mcp:status"]?.({
      payload: { state: "error", port: null, message: "could not bind" },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("could not bind");
  });
});
