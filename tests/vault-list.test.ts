import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import VaultList from "../src/components/VaultList.vue";

type Busy = "open_vault" | "open_daily_note" | null;

const mountList = (
  vaults: Array<{ id: string; name: string; path: string; open: boolean }>,
  busyVaultId: string | null = null,
  busyCommand: Busy = null,
  captureDisabled = false,
  recordingVaultId: string | null = null,
) =>
  mount(VaultList, {
    props: { vaults, busyVaultId, busyCommand, captureDisabled, recordingVaultId },
  });

const sample = [
  { id: "aaa111", name: "Personal", path: "C:\\vaults\\Personal", open: false },
  { id: "bbb222", name: "Work", path: "C:\\vaults\\Work", open: false },
];

describe("VaultList", () => {
  it("opens the vault when the row is clicked", async () => {
    const wrapper = mountList(sample);
    await wrapper.find('[aria-label="Open vault Personal"]').trigger("click");
    expect(wrapper.emitted("open-vault")).toEqual([["aaa111"]]);
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
    expect(buttons.length).toBe(8);
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
});
