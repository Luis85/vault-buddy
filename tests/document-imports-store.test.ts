import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useDocumentImportsStore } from "../src/stores/documentImports";

const VAULT = { id: "v1", name: "Personal" };

describe("documentImports store", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => {
    clearMocks();
    vi.useRealTimers();
  });

  it("exposes the in-flight conversion while convert_document is pending", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(50_000));
    let resolveConvert: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "convert_document")
        return new Promise((r) => {
          resolveConvert = r;
        });
    });
    const store = useDocumentImportsStore();
    // Windows backslash path — the display name must be the basename.
    const done = store.convert(VAULT, "C:\\docs\\Quarterly Report.docx");
    expect(store.active).toEqual({
      fileName: "Quarterly Report.docx",
      sourcePath: "C:\\docs\\Quarterly Report.docx",
      vaultId: "v1",
      vaultName: "Personal",
      startedAtMs: 50_000,
    });
    resolveConvert("Documents/2026/07/note.md");
    await expect(done).resolves.toBe("Documents/2026/07/note.md");
    expect(store.active).toBeNull();
  });

  it("clears the active slot when the conversion fails and rethrows the raw error", async () => {
    mockIPC((cmd) => {
      if (cmd === "convert_document") throw "pandoc crashed";
    });
    const store = useDocumentImportsStore();
    await expect(store.convert(VAULT, "/a.docx")).rejects.toBe("pandoc crashed");
    // A failed conversion must never strand a stale "converting" card.
    expect(store.active).toBeNull();
  });

  it("rejects a concurrent second convert without touching the first run's state", async () => {
    let resolveConvert: (v: unknown) => void = () => {};
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "convert_document") {
        calls += 1;
        return new Promise((r) => {
          resolveConvert = r;
        });
      }
    });
    const store = useDocumentImportsStore();
    const first = store.convert(VAULT, "/a.docx");
    // Same message the Rust ImportLock would return — and crucially the
    // second call must neither invoke nor let its cleanup clear the FIRST
    // conversion's active slot.
    await expect(
      store.convert({ id: "v2", name: "Work" }, "/b.docx"),
    ).rejects.toBe("An import is already in progress.");
    expect(calls).toBe(1);
    expect(store.active?.fileName).toBe("a.docx");
    resolveConvert("Documents/2026/07/a.md");
    await expect(first).resolves.toBe("Documents/2026/07/a.md");
    expect(store.active).toBeNull();
  });
});
