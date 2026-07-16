import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { usePandocStore } from "../src/stores/pandoc";

const NOT_INSTALLED = {
  installed: false,
  version: null,
  path: null,
  sandboxSupported: false,
  configuredPath: null,
};
const installed = () => ({
  installed: true,
  version: "pandoc 3.1.9",
  path: "pandoc",
  sandboxSupported: true,
  configuredPath: null,
});

describe("usePandocStore", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("probes once and caches when Pandoc is installed", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return installed();
      }
    });
    const store = usePandocStore();
    await store.ensureDetected();
    expect(store.status?.installed).toBe(true);
    // Found → a second ensureDetected must NOT re-probe.
    await store.ensureDetected();
    expect(calls).toBe(1);
  });

  it("re-probes while Pandoc is not installed", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return NOT_INSTALLED;
      }
    });
    const store = usePandocStore();
    await store.ensureDetected();
    await store.ensureDetected();
    expect(calls).toBe(2); // not cached — a fresh install can still be picked up
  });

  it("degrades to null and does not throw when the probe fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") throw new Error("io error");
    });
    const store = usePandocStore();
    await store.ensureDetected();
    expect(store.status).toBeNull();
    expect(store.checking).toBe(false);
  });

  it("markDetected caches a status without probing", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return installed();
      }
    });
    const store = usePandocStore();
    store.markDetected(installed());
    expect(store.status?.installed).toBe(true);
    // The written-through status counts as "found", so ensureDetected skips.
    await store.ensureDetected();
    expect(calls).toBe(0);
  });
});
