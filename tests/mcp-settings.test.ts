import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises,mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

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

  it("serializes saves: a second toggle mid-save fires no second set_mcp_config", async () => {
    // Two quick toggles would otherwise race: the second request is built
    // from a pre-response snapshot and can undo the first (Codex review
    // catch). Asserting the disabled attributes is NOT discriminating — the
    // guard is defense-in-depth for an event that lands before the disabled
    // re-render paints, and test-utils' setValue/trigger politely SKIP
    // disabled elements (BaseWrapper.isDisabled), so a setValue here would
    // exercise the attribute, never the guard. The mid-save toggle therefore
    // dispatches the change event on the raw element, and the discriminating
    // assertion is the set_mcp_config call count: deleting save()'s
    // in-flight early-return makes it 2.
    let setCalls = 0;
    let resolveSet: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "get_mcp_config") return { ...baseConfig };
      if (cmd === "set_mcp_config") {
        setCalls += 1;
        return new Promise((res) => {
          resolveSet = res;
        });
      }
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    // Sets checked directly and dispatches change like the browser would —
    // deliberately not setValue, so the disabled attribute can't mask a
    // missing guard (and the checked-prop write can't be silently skipped).
    const fireWritesChange = (checked: boolean) => {
      const el = wrapper.find('[data-testid="mcp-writes"]')
        .element as HTMLInputElement;
      el.checked = checked;
      el.dispatchEvent(new Event("change"));
      return flushPromises();
    };
    await wrapper.find('[data-testid="mcp-enabled"]').setValue(true);
    expect(setCalls).toBe(1);
    expect(
      wrapper.find('[data-testid="mcp-writes"]').attributes("disabled"),
    ).toBeDefined();
    // Second toggle while the first save is pending: only the guard stands
    // between this and a second, stale-snapshot request.
    await fireWritesChange(true);
    expect(setCalls).toBe(1);
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
    // The guard must reset once the save resolves — a toggle now goes through.
    await fireWritesChange(true);
    expect(setCalls).toBe(2);
    resolveSet({
      ...baseConfig,
      enabled: true,
      allowWrites: true,
      token: "tok123",
      status: { state: "running", port: 22082, message: null },
    });
    await flushPromises();
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

  it("surfaces and logs a failed save, then re-enables the controls", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_mcp_config") return { ...baseConfig };
      if (cmd === "set_mcp_config") throw new Error("port taken");
      if (cmd === "regenerate_mcp_token") throw new Error("disk full");
      return undefined;
    });
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-enabled"]').setValue(true);
    await flushPromises();
    // The error reaches the card AND the guard resets — a failed save must
    // not leave the whole card disabled forever.
    expect(wrapper.text()).toContain("port taken");
    expect(
      wrapper.find('[data-testid="mcp-enabled"]').attributes("disabled"),
    ).toBeUndefined();
    // Same contract for the regenerate path. Token must be present for the
    // button to render, so seed one via a status-driven refetch instead:
    const withToken = mount(McpSettings);
    mockIPC((cmd) => {
      if (cmd === "get_mcp_config") return { ...baseConfig, token: "tok" };
      if (cmd === "regenerate_mcp_token") throw new Error("disk full");
      return undefined;
    });
    withToken.unmount();
    const wrapper2 = mount(McpSettings);
    await flushPromises();
    await wrapper2.find('[data-testid="mcp-regenerate"]').trigger("click");
    await flushPromises();
    expect(wrapper2.text()).toContain("disk full");
  });

  it("copies the token and the client snippets to the clipboard", async () => {
    const copied: string[] = [];
    vi.stubGlobal("navigator", {
      clipboard: {
        writeText: (t: string) => {
          copied.push(t);
          return Promise.resolve();
        },
      },
    });
    mockIPC((cmd) =>
      cmd === "get_mcp_config"
        ? {
            ...baseConfig,
            enabled: true,
            token: "tok123",
            status: { state: "running", port: 22082, message: null },
          }
        : undefined,
    );
    const wrapper = mount(McpSettings);
    await flushPromises();
    await wrapper.find('[data-testid="mcp-copy-token"]').trigger("click");
    expect(copied).toContain("tok123");
    // Every snippet copy button routes its live text through the same path.
    const buttons = wrapper.findAll("button").filter((b) => b.text() === "Copy");
    for (const b of buttons) await b.trigger("click");
    expect(copied.some((t) => t.includes("claude mcp add"))).toBe(true);
    expect(copied.some((t) => t.includes("mcp-remote"))).toBe(true);
    vi.unstubAllGlobals();
  });

  it("unlistens from mcp:status on unmount", async () => {
    mockIPC((cmd) => (cmd === "get_mcp_config" ? { ...baseConfig } : undefined));
    const wrapper = mount(McpSettings);
    await flushPromises();
    expect(listeners["mcp:status"]).toBeDefined();
    wrapper.unmount();
    await flushPromises();
    // The mock's unlisten deletes the registration — a hidden-panel remount
    // must not stack a second live listener.
    expect(listeners["mcp:status"]).toBeUndefined();
  });
});
