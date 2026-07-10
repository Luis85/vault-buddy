import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { dailyNoteOpenedMessage } from "../src/buddyMessages";
import ActionPanel from "../src/components/ActionPanel.vue";
import { useCaptureStore } from "../src/stores/capture";
import { useNotificationsStore } from "../src/stores/notifications";
import { useVaultsStore } from "../src/stores/vaults";

const sampleVaults = [
  { id: "d4e5f6", name: "Personal", path: "C:\\vaults\\Personal", open: false },
  { id: "a1b2c3", name: "Work", path: "C:\\vaults\\Work", open: false },
];

const manyVaults = Array.from({ length: 8 }, (_, i) => ({
  id: `id${i}`,
  name: `Vault ${i}`,
  path: `C:\\vaults\\Vault ${i}`,
  open: false,
}));

describe("ActionPanel", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
  });

  afterEach(() => {
    clearMocks();
  });

  it("lists each vault with both actions and a count badge", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("Personal");
    expect(wrapper.text()).toContain("Work");
    expect(wrapper.text()).toContain("2"); // count badge
    const buttons = wrapper.findAll(".panel-scroll button");
    expect(buttons).toHaveLength(10); // 2 vaults × (row + daily note + tasks + capture + gear)
    // the list scrolls inside the fixed-height panel with the themed scrollbar
    expect(wrapper.find(".panel-scroll.overflow-y-auto").exists()).toBe(true);
  });

  it("dispatches open_daily_note with the vault id", async () => {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper
      .find('[aria-label="Open today\'s daily note in Personal"]')
      .trigger("click");
    expect(calls).toEqual([
      { cmd: "open_daily_note", args: { id: "d4e5f6" } },
      { cmd: "announce", args: { text: dailyNoteOpenedMessage() } },
      { cmd: "close_panel", args: {} },
    ]);
  });

  it("shows the search icon beside the cog on the list view and opens the search view", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.find('[data-testid="search-toggle"]').exists()).toBe(true);
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(true);
    await wrapper.get('[data-testid="search-toggle"]').trigger("click");
    expect(store.view).toBe("search");
    expect(wrapper.text()).toContain("Search");
    expect(wrapper.find('[data-testid="search-input"]').exists()).toBe(true);
    // Off the list view the header swaps to the back button — no search icon.
    expect(wrapper.find('[data-testid="search-toggle"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="back-button"]').exists()).toBe(true);
  });

  it("slash on the list view opens search, but not while typing in the filter", async () => {
    const store = useVaultsStore();
    store.vaults = manyVaults; // > threshold so the filter input renders
    store.loaded = true;
    const wrapper = mount(ActionPanel, { attachTo: document.body });
    // typing "/" into the filter must keep typing, not switch views
    await wrapper.get('input[type="search"]').trigger("keydown", { key: "/" });
    expect(store.view).toBe("list");
    // "/" anywhere else on the list view jumps to search
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", bubbles: true }));
    await wrapper.vm.$nextTick();
    expect(store.view).toBe("search");
    wrapper.unmount();
  });

  it("Ctrl+F opens search from the list view and is inert elsewhere", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel, { attachTo: document.body });
    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "f", ctrlKey: true, bubbles: true }),
    );
    await wrapper.vm.$nextTick();
    expect(store.view).toBe("search");
    // on a non-list view the shortcut must not fire (search's own input owns keys)
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "/", bubbles: true }));
    await wrapper.vm.$nextTick();
    expect(store.view).toBe("search"); // unchanged, no re-trigger side effects
    wrapper.unmount();
  });

  it("back from the search view returns to the vault list", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper.get('[data-testid="search-toggle"]').trigger("click");
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(store.view).toBe("list");
    expect(wrapper.text()).toContain("Personal");
  });

  it("hides the filter for short lists", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.find('input[type="search"]').exists()).toBe(false);
  });

  it("filters long lists by name and path", async () => {
    const store = useVaultsStore();
    store.vaults = manyVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const input = wrapper.find('input[type="search"]');
    expect(input.exists()).toBe(true);
    await input.setValue("Vault 3");
    expect(wrapper.text()).toContain("Vault 3");
    expect(wrapper.text()).not.toContain("Vault 5");
  });

  it("shows a friendly message when nothing matches the filter", async () => {
    const store = useVaultsStore();
    store.vaults = manyVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper.find('input[type="search"]').setValue("zzz");
    expect(wrapper.text()).toContain('No vaults match "zzz"');
  });

  it("clears the filter on Escape instead of closing", async () => {
    const store = useVaultsStore();
    store.vaults = manyVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const input = wrapper.find('input[type="search"]');
    await input.setValue("Vault 3");
    await input.trigger("keydown", { key: "Escape" });
    expect((input.element as HTMLInputElement).value).toBe("");
    expect(wrapper.text()).toContain("Vault 5"); // list unfiltered again
  });

  it("shows the friendly empty state when no vaults were found", () => {
    const store = useVaultsStore();
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const text = wrapper.text().replace(/\s+/g, " ");
    expect(text).toContain(
      "Obsidian not found — no vaults discovered. Is Obsidian installed and has it been opened at least once?"
    );
  });

  it("shows the error banner when an action failed", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.error = "failed to launch obsidian://open";
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("failed to launch");
  });

  it("disables all buttons while busy", () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.busyVaultId = "a1b2c3";
    store.busyCommand = "open_vault";
    const wrapper = mount(ActionPanel);
    // vault action buttons only — the header's settings gear stays usable
    const buttons = wrapper.findAll(".panel-scroll button");
    expect(buttons).toHaveLength(10);
    expect(buttons.every((b) => b.attributes("disabled") !== undefined)).toBe(
      true
    );
  });

  it("switches between the vault list and the buddy settings via the gear", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(true);

    await wrapper.get('[data-testid="settings-toggle"]').trigger("click");
    expect(wrapper.text()).toContain("Buddy settings");
    expect(wrapper.text()).toContain("Classic");
    expect(wrapper.text()).not.toContain("Personal");

    // the header cog is list-only; getting back to the list is the back button
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(false);
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(wrapper.text()).toContain("Vaults");
    expect(wrapper.text()).toContain("Personal");
  });

  it("mounts on the settings view when the store says so", () => {
    // an install failure reopens the destroyed panel directly on settings,
    // where the update error and retry button live
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.view = "settings";
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("Buddy settings");
    expect(wrapper.text()).not.toContain("Personal");
  });

  it("hides the filter and count badge while settings are open", async () => {
    const store = useVaultsStore();
    store.vaults = manyVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    expect(wrapper.find('input[type="search"]').exists()).toBe(true);
    await wrapper.find('[data-testid="settings-toggle"]').trigger("click");
    expect(wrapper.find('input[type="search"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("8"); // count badge hidden
  });

  it("renders error banner and empty state together", () => {
    const store = useVaultsStore();
    store.loaded = true;
    store.error = "failed to launch obsidian://open";
    const wrapper = mount(ActionPanel);
    expect(wrapper.text()).toContain("failed to launch");
    expect(wrapper.text()).toContain("Obsidian not found");
  });

  it("opens capture settings when a vault gear is clicked", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper
      .find('[aria-label="Capture settings for Personal"]')
      .trigger("click");
    expect(store.view).toBe("captureSettings");
    expect(store.captureSettingsVaultId).toBe("d4e5f6");
  });

  it("shows the idle rename warning as a notification in the list view", async () => {
    // The old list-view-only capture.warning banner was replaced by the
    // NotificationHost overlay (Task 3) — the warning now reaches the panel
    // via the notifications store rather than the capture store directly.
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const capture = useCaptureStore();
    const notifications = useNotificationsStore();
    capture.status = "idle";
    notifications.warning("Recording renamed, but its note needs attention");
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="notification"]').exists()).toBe(true);
    expect(wrapper.text()).toContain(
      "Recording renamed, but its note needs attention"
    );
    // RecordingBar is unmounted while idle, so this must be the only copy
    expect(wrapper.findAll("[data-testid='level-meter']")).toHaveLength(0);
  });

  it("shows a notification regardless of which panel view is open", async () => {
    // NotificationHost overlays every view (that's the point of Task 3) — a
    // warning raised while a non-list view (e.g. settings) is open must
    // still surface, where the old list-view-only banner used to hide it.
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.view = "settings";
    const wrapper = mount(ActionPanel);
    const notifications = useNotificationsStore();
    notifications.warning("Recording renamed, but its note needs attention");
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="notification"]').exists()).toBe(true);
    expect(wrapper.text()).toContain(
      "Recording renamed, but its note needs attention"
    );
  });

  it("does not duplicate the warning banner while RecordingBar is showing it", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    const capture = useCaptureStore();
    capture.status = "recording";
    capture.startedAtMs = Date.now();
    capture.warning = "Recording renamed, but its note needs attention";
    await wrapper.vm.$nextTick();
    const matches = wrapper.text().match(/note needs attention/g) ?? [];
    expect(matches).toHaveLength(1);
  });

  it("shows the rename prompt after a save and hides it on dismiss", async () => {
    const wrapper = mount(ActionPanel);
    const capture = useCaptureStore();
    capture.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("name this recording");
    capture.lastSaved = null;
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).not.toContain("name this recording");
  });

  it("opens the record view from a vault's capture button", async () => {
    mockIPC((cmd) => {
      if (cmd === "get_capture_config") return { mode: "meeting" };
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper.get('[aria-label="Capture knowledge in Personal"]').trigger("click");
    expect(store.view).toBe("recordMode");
    expect(store.recordModeVaultId).toBe("d4e5f6");
  });

  it("shows a back button in non-list views that returns to the parent", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_recordings") return [];
      if (cmd === "get_capture_config") return { mode: "meeting" };
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.openRecordings("d4e5f6");
    const wrapper = mount(ActionPanel);
    await flushPromises();
    // no cog in a non-list view; a back button instead
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(false);
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(store.view).toBe("recordMode"); // recordings → record view
  });

  it("clears the filter text when the panel is shown again", async () => {
    // The panel window is only hidden/shown, not unmounted, so onUnmounted no
    // longer clears local state on close. Filter text used to survive a close;
    // reopening (shownNonce bump) must reset it, or a reopen shows the vault
    // list still filtered. (The record chooser is now a store-owned view —
    // reset by refresh/showList — not a local dialog that could go stale.)
    mockIPC(() => undefined);
    const store = useVaultsStore();
    store.vaults = manyVaults;
    store.loaded = true;
    const wrapper = mount(ActionPanel);
    await wrapper.find('input[type="search"]').setValue("Vault"); // keeps all
    expect(
      (wrapper.find('input[type="search"]').element as HTMLInputElement).value,
    ).toBe("Vault");

    store.shownNonce++; // the panel was reopened
    await wrapper.vm.$nextTick();
    expect(
      (wrapper.find('input[type="search"]').element as HTMLInputElement).value,
    ).toBe("");
  });

  it("dismisses a stale rename prompt when the panel is shown again", async () => {
    const wrapper = mount(ActionPanel);
    const capture = useCaptureStore();
    capture.lastSaved = { mp3: "/v/2026-07-04 1405 Meeting.mp3", note: null };
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).toContain("name this recording");

    const store = useVaultsStore();
    store.shownNonce++; // the panel was reopened
    await wrapper.vm.$nextTick();
    expect(wrapper.text()).not.toContain("name this recording");
  });

  it("renders the Recordings view with its title", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_recordings") return [];
    });
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.openRecordings("d4e5f6");
    const wrapper = mount(ActionPanel);
    await flushPromises();
    expect(wrapper.get("h1").text()).toBe("Recordings");
    expect(wrapper.text()).toContain("No recordings yet.");
  });

  it("renders the Transcriptions view with its title", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.openTranscriptions();
    const wrapper = mount(ActionPanel);
    await flushPromises();
    expect(wrapper.get("h1").text()).toBe("Transcriptions");
    expect(wrapper.text()).toContain("No transcriptions yet.");
  });

  it("shows a back button (not the settings gear) on the Transcriptions view, returning to the list", async () => {
    const store = useVaultsStore();
    store.vaults = sampleVaults;
    store.loaded = true;
    store.openTranscriptions();
    const wrapper = mount(ActionPanel);
    await flushPromises();
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(false);
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(store.view).toBe("list");
    expect(wrapper.text()).toContain("Personal");
  });

  it("renders the Tasks view with a back button when view is tasks", async () => {
    const store = useVaultsStore();
    store.openTasks("v1");
    const wrapper = mount(ActionPanel, {
      global: { stubs: { Tasks: true } },
    });
    await flushPromises();
    expect(wrapper.find('[data-testid="back-button"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("Tasks");
  });

  it("titles the per-vault settings view 'Vault settings'", async () => {
    const store = useVaultsStore();
    store.openCaptureSettings("v1");
    const wrapper = mount(ActionPanel, {
      global: { stubs: { CaptureSettings: true } },
    });
    await flushPromises();
    expect(wrapper.text()).toContain("Vault settings");
  });

  it("routes the importPicker view to ImportVaultPicker with a back button", async () => {
    const store = useVaultsStore();
    store.openImportPicker("C:/x/Report.docx");
    const wrapper = mount(ActionPanel, {
      global: { stubs: { ImportVaultPicker: true } },
    });
    await flushPromises();
    expect(wrapper.get("h1").text()).toBe("Import document");
    expect(wrapper.find('[data-testid="settings-toggle"]').exists()).toBe(false);
    expect(wrapper.find('[data-testid="back-button"]').exists()).toBe(true);
    await wrapper.get('[data-testid="back-button"]').trigger("click");
    expect(store.view).toBe("list");
    expect(store.pendingImportPath).toBeNull();
  });
});
