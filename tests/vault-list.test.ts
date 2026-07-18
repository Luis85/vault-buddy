import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";

import VaultList from "../src/components/VaultList.vue";
import { useVaultsStore } from "../src/stores/vaults";

type Busy = "open_vault" | "open_daily_note" | null;

const mountList = (
  vaults: Array<{ id: string; name: string; path: string; open: boolean }>,
  busyVaultId: string | null = null,
  busyCommand: Busy = null,
  captureDisabled = false,
  recordingVaultId: string | null = null,
  transcribingVaultId: string | null = null,
  taskCounts: Record<string, number> = {},
) =>
  mount(VaultList, {
    props: {
      vaults,
      busyVaultId,
      busyCommand,
      captureDisabled,
      recordingVaultId,
      transcribingVaultId,
      taskCounts,
    },
  });

const sample = [
  { id: "aaa111", name: "Personal", path: "C:\\vaults\\Personal", open: false },
  { id: "bbb222", name: "Work", path: "C:\\vaults\\Work", open: false },
];

describe("VaultList", () => {
  beforeEach(() => {
    // VaultList now reads favorites straight from the vaults store (Task 5),
    // so every mount needs an active Pinia — and a clean localStorage, since
    // the store's `favorites` seeds itself from it at creation time and the
    // row-star test below toggles for real (favoriteVaults.ts) rather than
    // mocking the store.
    setActivePinia(createPinia());
    localStorage.clear();
  });

  it("opens the vault when the row is clicked", async () => {
    const wrapper = mountList(sample);
    await wrapper.find('[aria-label="Open vault Personal"]').trigger("click");
    expect(wrapper.emitted("open-vault")).toEqual([["aaa111"]]);
  });

  it("shows a transcribing indicator on the transcribing vault", () => {
    const wrapper = mountList(sample, null, null, false, null, "aaa111");
    const dot = wrapper.get('[title="Transcribing…"]');
    expect(dot.classes()).toContain("bg-violet-400");
  });

  it("opens the daily note from the calendar button", async () => {
    const wrapper = mountList(sample);
    await wrapper
      .find('[aria-label="Open today\'s daily note in Work"]')
      .trigger("click");
    expect(wrapper.emitted("open-daily-note")).toEqual([["bbb222"]]);
  });

  it("shows an avatar initial per vault", () => {
    const wrapper = mountList(sample);
    expect(wrapper.text()).toContain("P");
    expect(wrapper.text()).toContain("W");
  });

  it("lists open vaults first under an 'Open now' header", () => {
    const wrapper = mountList([
      { id: "a", name: "Alpha", path: "C:\\v\\Alpha", open: false },
      { id: "z", name: "Zulu", path: "C:\\v\\Zulu", open: true },
    ]);
    expect(wrapper.text()).toContain("Open now");
    expect(wrapper.text()).toContain("Other vaults");
    // Zulu is alphabetically last but open — it must render first
    const names = wrapper
      .findAll("li .text-sm")
      .map((node) => node.text().trim());
    expect(names[0]).toBe("Zulu");
    expect(names[1]).toBe("Alpha");
  });

  it("marks open vaults with an indicator dot", () => {
    const wrapper = mountList([
      { id: "z", name: "Zulu", path: "C:\\v\\Zulu", open: true },
      { id: "a", name: "Alpha", path: "C:\\v\\Alpha", open: false },
    ]);
    expect(wrapper.findAll('[title="Open in Obsidian"]')).toHaveLength(1);
  });

  it("shows a flat list without headers when nothing is open", () => {
    const wrapper = mountList(sample);
    expect(wrapper.text()).not.toContain("Open now");
    expect(wrapper.text()).not.toContain("Other vaults");
  });

  it("shows a spinner on the busy action and disables all buttons", () => {
    const wrapper = mountList(sample, "aaa111", "open_vault");
    expect(wrapper.find('[role="status"]').exists()).toBe(true);
    const buttons = wrapper.findAll("button");
    // 6 per row (open, daily-note, tasks, capture, capture-settings, favorite
    // star) x 2 rows — bumped from 10 with the Task 5 star button.
    expect(buttons.length).toBe(12);
    expect(buttons.every((b) => b.attributes("disabled") !== undefined)).toBe(
      true,
    );
  });

  it("emits capture with the vault id", async () => {
    const wrapper = mountList(sample);
    await wrapper
      .find('[aria-label^="Capture knowledge in"]')
      .trigger("click");
    expect(wrapper.emitted("capture")).toEqual([[sample[0].id]]);
  });

  it("titles the capture button 'Capture knowledge' (not audio-only)", () => {
    // The chooser now also imports documents, so the old
    // "Capture knowledge (record audio)" tooltip misdescribed it.
    const wrapper = mountList(sample);
    const button = wrapper.find('[aria-label^="Capture knowledge in"]');
    expect(button.attributes("title")).toBe("Capture knowledge");
  });

  it("disables capture buttons when captureDisabled", () => {
    const wrapper = mountList(sample, null, null, true);
    expect(
      wrapper
        .find('[aria-label^="Capture knowledge in"]')
        .attributes("disabled"),
    ).toBeDefined();
  });

  it("shows the path for vaults with duplicate names so they can be told apart", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Notes", path: "C:\\personal\\Notes", open: false },
      { id: "bbb222", name: "Notes", path: "D:\\work\\Notes", open: false },
    ]);
    expect(wrapper.text()).toContain("C:\\personal\\Notes");
    expect(wrapper.text()).toContain("D:\\work\\Notes");
  });

  it("disambiguates duplicate names in the accessible action labels too", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Notes", path: "C:\\personal\\Notes", open: false },
      { id: "bbb222", name: "Notes", path: "D:\\work\\Notes", open: false },
    ]);
    // screen-reader users must not hear two identical controls that target
    // different vaults
    const labels = wrapper
      .findAll("button")
      .map((b) => b.attributes("aria-label"));
    expect(labels).toContain("Open vault Notes (C:\\personal\\Notes)");
    expect(labels).toContain(
      "Open today's daily note in Notes (D:\\work\\Notes)",
    );
  });

  it("hides the path when vault names are unique", () => {
    const wrapper = mountList(sample);
    expect(wrapper.text()).not.toContain("C:\\vaults\\Personal");
    expect(wrapper.text()).not.toContain("C:\\vaults\\Work");
  });

  it("always exposes the full path as a tooltip on the row", () => {
    const wrapper = mountList([sample[0]]);
    expect(wrapper.find("li").attributes("title")).toBe("C:\\vaults\\Personal");
  });

  it("emits capture-settings with the vault id from the gear", async () => {
    const wrapper = mountList(sample);
    await wrapper
      .find('[aria-label="Capture settings for Work"]')
      .trigger("click");
    expect(wrapper.emitted("capture-settings")).toEqual([["bbb222"]]);
  });

  it("marks the recording vault's row with a red dot", () => {
    const wrapper = mountList(sample, null, null, true, "bbb222");
    const dots = wrapper.findAll('[title="Recording…"]');
    expect(dots).toHaveLength(1);
    // the dot sits on the Work row
    const workRow = wrapper.findAll("li").find((li) => li.text().includes("Work"))!;
    expect(workRow.find('[title="Recording…"]').exists()).toBe(true);
  });

  it("shows no recording dot when nothing records", () => {
    const wrapper = mountList(sample);
    expect(wrapper.find('[title="Recording…"]').exists()).toBe(false);
  });

  it("emits open-tasks with the vault id", async () => {
    const wrapper = mountList([
      { id: "v1", name: "Test", path: "C:\\vaults\\Test", open: false },
    ]);
    await wrapper.get('[data-testid="open-tasks"]').trigger("click");
    expect(wrapper.emitted("open-tasks")?.[0]).toEqual(["v1"]);
  });

  it("shows the open-task badge when the count is > 0", () => {
    const wrapper = mountList(
      [{ id: "v1", name: "Test", path: "C:\\vaults\\Test", open: false }],
      null,
      null,
      false,
      null,
      null,
      { v1: 4 },
    );
    expect(wrapper.get('[data-testid="task-count"]').text()).toBe("4");
  });

  it("hides the badge when the open-task count is 0 or missing", () => {
    const wrapper = mountList(
      [{ id: "v1", name: "Test", path: "C:\\vaults\\Test", open: false }],
      null,
      null,
      false,
      null,
      null,
      {},
    );
    expect(wrapper.find('[data-testid="task-count"]').exists()).toBe(false);
  });

  // Task 5: favorites pin above Open now / Other vaults, a favorite renders
  // once regardless of open state, and a per-row star toggles it. VaultList
  // reads `store.favorites`/`store.toggleFavorite` straight from the vaults
  // store rather than via props, so these tests seed/spy the store directly
  // instead of the brief's illustrative `mountList({ ..., favorites })`
  // shape (this file's mountList takes positional args — see above). The
  // brief's snippet also queried "h3" for the group header; the component
  // uses "h2" (matching every other panel section header — BuddySettings,
  // Transcriptions, VaultFolderSetting, etc. — Tasks.vue's "h3" is a header
  // nested one level deeper, under its own "h2" view sections, which does
  // not apply here), so the query below matches the real markup.
  it("pins favorites into a Favorites group above the others", async () => {
    const store = useVaultsStore();
    store.favorites = new Set(["c"]);
    const wrapper = mountList([
      { id: "a", name: "Apple", path: "Apple", open: true },
      { id: "b", name: "Box", path: "Box", open: false },
      { id: "c", name: "Cat", path: "Cat", open: false },
    ]);
    const headers = wrapper.findAll("h2").map((h) => h.text());
    expect(headers[0]).toContain("Favorites");
    // The favorite appears once, in the Favorites group.
    const favSection = wrapper.get('[data-section="favorites"]');
    expect(favSection.text()).toContain("Cat");
    expect(wrapper.findAll('[data-section="favorites"] li')).toHaveLength(1);
  });

  it("toggles a favorite via the row star", async () => {
    const store = useVaultsStore();
    const spy = vi.spyOn(store, "toggleFavorite");
    const wrapper = mountList([
      { id: "a", name: "Apple", path: "Apple", open: false },
    ]);
    await wrapper.get('[data-testid="vault-favorite-a"]').trigger("click");
    expect(spy).toHaveBeenCalledWith("a");
  });

  it("keeps the open dot on a favorited-and-open vault's row (in Favorites, not duplicated)", () => {
    const store = useVaultsStore();
    store.favorites = new Set(["a"]);
    const wrapper = mountList([
      { id: "a", name: "Apple", path: "Apple", open: true },
      { id: "b", name: "Box", path: "Box", open: false },
    ]);
    // Apple is open AND favorited: it must not appear twice (once under
    // Favorites, once under Open now) and must keep exactly one open dot.
    expect(wrapper.text()).not.toContain("Open now");
    const favSection = wrapper.get('[data-section="favorites"]');
    expect(favSection.findAll('[title="Open in Obsidian"]')).toHaveLength(1);
    expect(wrapper.findAll('[title="Open in Obsidian"]')).toHaveLength(1);
  });
});
