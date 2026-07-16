import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({
  logWarning: vi.fn(),
}));

import {
  dailyNoteOpenedMessage,
  vaultOpenedMessage,
} from "../src/buddyMessages";
import { logWarning } from "../src/logging";
import { useSettingsStore } from "../src/stores/settings";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal", open: false },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work", open: false },
];

describe("vaults store", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("loads vaults via the list_vaults command", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);
    expect(store.loaded).toBe(true);
  });

  it("refresh triggers a load", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    expect(store.loaded).toBe(false);
    await store.refresh();
    expect(store.loaded).toBe(true);
    expect(store.vaults).toEqual(sampleVaults);
  });

  it("refresh always lands on the vault list", async () => {
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    store.openSettings();
    expect(store.view).toBe("settings");
    await store.refresh();
    expect(store.view).toBe("list");
    store.openCaptureSettings("v1");
    expect(store.view).toBe("captureSettings");
    expect(store.captureSettingsVaultId).toBe("v1");
    store.showList();
    expect(store.view).toBe("list");
    expect(store.captureSettingsVaultId).toBeNull();
  });

  it("runAction passes the vault id and tracks busy state", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    await store.runAction("open_daily_note", "a1b2c3");
    // the buddy speaks between the launch and the panel close
    expect(calls).toEqual([
      { cmd: "open_daily_note", args: { id: "a1b2c3" } },
      { cmd: "announce", args: { text: dailyNoteOpenedMessage() } },
      { cmd: "close_panel", args: {} },
    ]);
    expect(store.busyVaultId).toBe(null);
    expect(store.busyCommand).toBe(null);
    expect(store.error).toBe(null);
  });

  it("the buddy names the vault when an open succeeds", async () => {
    const spoken: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "announce") spoken.push((args as { text: string }).text);
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    await store.runAction("open_vault", "d4e5f6"); // Personal
    expect(spoken).toEqual([vaultOpenedMessage("Personal")]);
  });

  it("the buddy stays silent when Buddy messages is off", async () => {
    localStorage.setItem("vault-buddy.messages", "off");
    const spoken: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "announce") spoken.push(args);
    });
    useSettingsStore(); // reads the persisted "off"
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    await store.runAction("open_vault", "d4e5f6");
    expect(spoken).toEqual([]);
    localStorage.clear();
  });

  it("the buddy stays silent when an open fails (banner is the feedback)", async () => {
    const spoken: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "announce") spoken.push(args);
      if (cmd === "open_vault") throw "vault not found: nope";
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    await store.runAction("open_vault", "d4e5f6");
    expect(spoken).toEqual([]);
    expect(store.error).toContain("vault not found");
  });

  it("does not close the panel when an action fails", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    await store.runAction("open_vault", "nope");
    expect(calls).not.toContain("close_panel");
    expect(store.error).toContain("vault not found");
  });

  it("runAction surfaces command errors", async () => {
    mockIPC(() => {
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    await store.runAction("open_vault", "nope");
    expect(store.error).toContain("vault not found");
    expect(store.busyVaultId).toBe(null);
  });

  it("loadVaults surfaces failures instead of leaving the panel blank", async () => {
    mockIPC(() => {
      throw "ipc unavailable";
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.loaded).toBe(true);
    expect(store.vaults).toEqual([]);
    expect(store.error).toContain("ipc unavailable");
  });

  it("re-runs discovery on every refresh so an empty first load can recover", async () => {
    let discovered: typeof sampleVaults = [];
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return discovered;
    });
    const store = useVaultsStore();
    await store.refresh(); // Obsidian not set up yet
    expect(store.vaults).toEqual([]);

    discovered = sampleVaults; // user has now opened Obsidian
    await store.refresh(); // reopen
    expect(store.vaults).toEqual(sampleVaults);
  });

  it("keeps the previous vault list when a refresh fails transiently", async () => {
    let fail = false;
    mockIPC((cmd) => {
      if (fail) throw "ipc unavailable";
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);

    fail = true;
    await store.loadVaults();
    expect(store.vaults).toEqual(sampleVaults);
    expect(store.error).toContain("ipc unavailable");
  });

  it("a failing list_vaults logs a warning through the log bridge", async () => {
    mockIPC(() => {
      throw "ipc unavailable";
    });
    const store = useVaultsStore();
    await store.loadVaults();
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("vault discovery failed"),
    );
  });

  it("a failing open_vault logs a warning through the log bridge", async () => {
    mockIPC(() => {
      throw "vault not found: nope";
    });
    const store = useVaultsStore();
    await store.runAction("open_vault", "nope");
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("open_vault failed"),
    );
  });

  it("openRecordings switches to the recordings view for a vault", () => {
    const store = useVaultsStore();
    store.openRecordings("a1b2c3");
    expect(store.view).toBe("recordings");
    expect(store.recordingsVaultId).toBe("a1b2c3");
  });

  it("showList clears the recordings vault id", () => {
    const store = useVaultsStore();
    store.openRecordings("a1b2c3");
    store.showList();
    expect(store.view).toBe("list");
    expect(store.recordingsVaultId).toBe(null);
  });

  it("openRecordMode switches to the record view for a vault", () => {
    const store = useVaultsStore();
    store.openRecordMode("a1b2c3");
    expect(store.view).toBe("recordMode");
    expect(store.recordModeVaultId).toBe("a1b2c3");
  });

  it("back() returns each view to its parent", () => {
    const store = useVaultsStore();
    // recordings' parent is the record view (same vault)
    store.openRecordings("a1b2c3");
    store.back();
    expect(store.view).toBe("recordMode");
    expect(store.recordModeVaultId).toBe("a1b2c3");
    // record view's parent is the list
    store.back();
    expect(store.view).toBe("list");
    // capture settings' parent is the list
    store.openCaptureSettings("a1b2c3");
    store.back();
    expect(store.view).toBe("list");
  });

  it("refresh() lands on the vault list and re-runs discovery", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "list_vaults") return sampleVaults;
    });
    const store = useVaultsStore();
    store.openSettings();
    expect(store.view).toBe("settings");
    await store.refresh();
    expect(store.view).toBe("list");
    expect(calls).toContain("list_vaults");
    expect(store.vaults).toEqual(sampleVaults);
  });

  it("requestView survives the next open refresh, then reverts to list", async () => {
    // a failed update install requests the settings view before reopening the
    // panel; the panel-shown refresh must honor it once (not clobber to list),
    // then a later open falls back to the vault list.
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    store.requestView("settings");
    expect(store.view).toBe("settings"); // reflected immediately
    await store.refresh(); // simulates the panel-shown refresh
    expect(store.view).toBe("settings"); // honored, not reset to list
    await store.refresh(); // a subsequent open
    expect(store.view).toBe("list"); // request was one-shot
  });

  it("requestViewOnNextOpen arms the next open without flipping the live view", async () => {
    // the startup update check asks via the NEXT panel open — an already-open
    // panel must not be yanked to settings mid-task (unlike requestView, which
    // flips the live view for the failed-install reopen).
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    store.requestViewOnNextOpen("settings");
    expect(store.view).toBe("list"); // live view untouched
    await store.refresh(); // the next panel-shown refresh
    expect(store.view).toBe("settings"); // consumed once
    await store.refresh();
    expect(store.view).toBe("list"); // one-shot
  });

  it("requestView can target the capture settings of a specific vault", async () => {
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    store.requestView("captureSettings", "v1");
    await store.refresh();
    expect(store.view).toBe("captureSettings");
    expect(store.captureSettingsVaultId).toBe("v1");
  });

  it("refresh populates taskCounts from count_open_tasks", async () => {
    mockIPC((cmd, args) => {
      if (cmd === "list_vaults")
        return [{ id: "v1", name: "A", path: "/a", open: false }];
      if (cmd === "count_open_tasks")
        return (args as { id: string }).id === "v1" ? 3 : 0;
    });
    const store = useVaultsStore();
    await store.refresh();
    expect(store.taskCounts).toEqual({ v1: 3 });
  });

  it("logs a warning and falls back to 0 when count_open_tasks fails", async () => {
    // A broken counter must be distinguishable from a vault with no open tasks
    // — the failure is logged, not silently swallowed (Diagnostics invariant).
    mockIPC((cmd) => {
      if (cmd === "list_vaults")
        return [{ id: "v1", name: "A", path: "/a", open: false }];
      if (cmd === "count_open_tasks") throw "ipc unavailable";
    });
    const store = useVaultsStore();
    await store.refresh();
    expect(store.taskCounts).toEqual({ v1: 0 });
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("count_open_tasks failed for vault v1"),
    );
  });

  it("refresh bumps shownNonce so the panel can reset transient UI on open", async () => {
    mockIPC((cmd) => (cmd === "list_vaults" ? [] : undefined));
    const store = useVaultsStore();
    const before = store.shownNonce;
    await store.refresh();
    expect(store.shownNonce).toBe(before + 1);
  });

  it("opens and backs out of the transcriptions view", () => {
    const s = useVaultsStore();
    s.openTranscriptions();
    expect(s.view).toBe("transcriptions");
    s.back();
    expect(s.view).toBe("list");
  });

  it("openTasks sets the tasks view and vault id", () => {
    const store = useVaultsStore();
    store.openTasks("v1");
    expect(store.view).toBe("tasks");
    expect(store.tasksVaultId).toBe("v1");
  });

  it("back() from tasks returns to the list and clears the vault id", () => {
    const store = useVaultsStore();
    store.openTasks("v1");
    store.back();
    expect(store.view).toBe("list");
    expect(store.tasksVaultId).toBeNull();
  });

  it("openAllTasks opens the tasks view in aggregate mode", () => {
    const store = useVaultsStore();
    store.openAllTasks();
    expect(store.view).toBe("tasks");
    expect(store.tasksVaultId).toBeNull();
    store.back();
    expect(store.view).toBe("list");
  });

  it("opens the search view and back returns to the list", () => {
    const store = useVaultsStore();
    store.openSearch();
    expect(store.view).toBe("search");
    store.back();
    expect(store.view).toBe("list");
  });

  it("openDocumentImport switches to the document-import view, back returns to list", () => {
    // The Pandoc-not-installed gate routes here (a focused setup screen)
    // instead of the buried settings page; its fixed parent is the vault list.
    const store = useVaultsStore();
    store.openDocumentImport();
    expect(store.view).toBe("documentImport");
    store.back();
    expect(store.view).toBe("list");
  });

  it("refreshTaskCount updates one vault and keeps the previous count on failure (GAP-32)", async () => {
    const store = useVaultsStore();
    store.taskCounts = { a: 2, b: 5 };
    mockIPC((cmd, args) => {
      if (cmd === "count_open_tasks") {
        const id = (args as { id: string }).id;
        if (id === "a") return 3;
        throw "ipc unavailable";
      }
    });
    await store.refreshTaskCount("a");
    expect(store.taskCounts.a).toBe(3);
    await store.refreshTaskCount("b");
    expect(store.taskCounts.b).toBe(5); // kept, not zeroed; failure logged
    expect(logWarning).toHaveBeenCalledWith(
      expect.stringContaining("count_open_tasks refresh failed for vault b"),
    );
  });

  it("enqueueImports appends to the queue and shows the picker", () => {
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    expect(store.view).toBe("importPicker");
    expect(store.pendingImports).toEqual(["C:/x/Report.docx"]);
  });

  it("showList clears the import queue", () => {
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    store.showList();
    expect(store.view).toBe("list");
    expect(store.pendingImports).toEqual([]);
  });

  it("dequeueImport drops the head and advances to the list when the queue drains", () => {
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    store.dequeueImport("C:/x/Report.docx");
    expect(store.pendingImports).toEqual([]);
    expect(store.view).toBe("list");
  });

  it("dequeueImport keeps the picker up while more imports are queued", () => {
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/A.docx", "C:/x/B.docx"]);
    store.dequeueImport("C:/x/A.docx");
    expect(store.pendingImports).toEqual(["C:/x/B.docx"]);
    expect(store.view).toBe("importPicker");
  });

  it("a stale dequeueImport after navigating away leaves the view alone (Codex P2)", () => {
    // The user presses Back / opens another view while convert_document is
    // still running; showList() already drained the queue. The late-resolving
    // pick() must not yank navigation back to the list.
    const store = useVaultsStore();
    store.enqueueImports(["C:/x/Report.docx"]);
    store.openTasks("v1"); // navigate away mid-conversion (clears no queue on its own)
    store.showList(); // …simulate a Back that drained the queue
    store.openTasks("v1"); // then the user lands on some other view
    store.dequeueImport("C:/x/Report.docx"); // stale completion fires
    expect(store.view).toBe("tasks");
    expect(store.pendingImports).toEqual([]);
  });

  it("refresh routes to the import picker when Rust has a pending import", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_pending_import") return ["C:/x/Report.docx"];
      if (cmd === "list_vaults") return [];
      if (cmd === "count_open_tasks") return 0;
      return undefined;
    });
    const store = useVaultsStore();
    await store.refresh();
    expect(store.view).toBe("importPicker");
    expect(store.pendingImports).toEqual(["C:/x/Report.docx"]);
  });

  it("a winning drop clears an armed pendingView so a later refresh doesn't consume it", async () => {
    let pending: string[] = ["C:/x/Report.docx"];
    mockIPC((cmd) => {
      if (cmd === "take_pending_import") return pending;
      if (cmd === "list_vaults") return [];
      if (cmd === "count_open_tasks") return 0;
      return undefined;
    });
    const store = useVaultsStore();
    store.requestViewOnNextOpen("settings"); // e.g. the startup update check armed it
    await store.refresh(); // drop wins this open
    expect(store.view).toBe("importPicker");

    // The drop cleared the armed "settings" request; a later empty refresh
    // keeps the un-picked import on the picker (never consumes stale settings).
    pending = [];
    await store.refresh();
    expect(store.view).toBe("importPicker");
  });

  it("refresh falls back to the vault list when there is no pending import", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_pending_import") return [];
      if (cmd === "list_vaults") return [];
      return undefined;
    });
    const store = useVaultsStore();
    await store.refresh();
    expect(store.view).toBe("list");
    expect(store.pendingImports).toEqual([]);
  });

  it("refresh treats a failed take_pending_import as no pending import", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_pending_import") throw "ipc unavailable";
      if (cmd === "list_vaults") return [];
      return undefined;
    });
    const store = useVaultsStore();
    await store.refresh();
    expect(store.view).toBe("list");
  });
});
