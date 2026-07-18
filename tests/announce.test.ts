import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { announce } from "../src/announce";
import { useSettingsStore } from "../src/stores/settings";

describe("announce", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
  });
  afterEach(() => clearMocks());

  it("forwards an action so the bubble becomes clickable", () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    useSettingsStore();
    announce("Update ready", "openUpdate");
    expect(calls).toEqual([
      { cmd: "announce", args: { text: "Update ready", action: "openUpdate" } },
    ]);
  });

  it("omits the action key when there is none (unchanged for every other caller)", () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    useSettingsStore();
    announce("Opening Personal ✨");
    expect(calls).toEqual([
      { cmd: "announce", args: { text: "Opening Personal ✨" } },
    ]);
  });

  it("stays silent when Buddy messages are off", () => {
    localStorage.setItem("vault-buddy.messages", "off");
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
    });
    useSettingsStore();
    announce("Update ready", "openUpdate");
    expect(calls).toEqual([]);
  });
});
