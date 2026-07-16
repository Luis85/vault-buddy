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

  it("re-probes while Pandoc is installed but too old for the sandbox", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        calls += 1;
        return {
          installed: true,
          version: "pandoc 2.14",
          path: "pandoc",
          sandboxSupported: false,
          configuredPath: null,
        };
      }
    });
    const store = usePandocStore();
    await store.ensureDetected();
    await store.ensureDetected();
    expect(calls).toBe(2); // too old is not a usable cache hit → re-probe
  });

  it("a late probe result does not overwrite a newer markDetected (Codex P2)", async () => {
    // ImportVaultPicker starts a probe, the user opens settings which writes an
    // authoritative status, then the original slow probe resolves stale. The
    // write-through must win — the stale probe cannot clobber it.
    let resolveProbe: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        return new Promise((r) => {
          resolveProbe = r;
        });
      }
    });
    const store = usePandocStore();
    const pending = store.ensureDetected(); // probe starts (status null)
    store.markDetected(installed()); // settings writes through while it's in flight
    expect(store.status?.installed).toBe(true);
    resolveProbe(NOT_INSTALLED); // the original probe resolves late + stale
    await pending;
    expect(store.status?.installed).toBe(true);
    expect(store.checking).toBe(false);
  });

  it("only the latest probe clears the checking gate (Codex P2)", async () => {
    // Two overlapping probes: the older one resolving first must not drop the
    // gate while the newer probe is still pending, or the intake UI flashes a
    // result before the authoritative one lands.
    const resolvers: Array<(v: unknown) => void> = [];
    mockIPC((cmd) => {
      if (cmd === "detect_pandoc") {
        return new Promise((r) => resolvers.push(r));
      }
    });
    const store = usePandocStore();
    const p1 = store.ensureDetected();
    const p2 = store.ensureDetected(); // status still null → probes again
    expect(store.checking).toBe(true);
    resolvers[0](NOT_INSTALLED); // older probe resolves first
    await p1;
    expect(store.checking).toBe(true); // newer probe still pending → gate stays
    resolvers[1](installed());
    await p2;
    expect(store.checking).toBe(false);
    expect(store.status?.installed).toBe(true);
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
