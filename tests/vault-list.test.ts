import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import VaultList from "../src/components/VaultList.vue";

type Busy = "open_vault" | "open_daily_note" | null;

const mountList = (
  vaults: Array<{ id: string; name: string; path: string }>,
  busyVaultId: string | null = null,
  busyCommand: Busy = null,
) => mount(VaultList, { props: { vaults, busyVaultId, busyCommand } });

const sample = [
  { id: "aaa111", name: "Personal", path: "C:\\vaults\\Personal" },
  { id: "bbb222", name: "Work", path: "C:\\vaults\\Work" },
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

  it("shows a spinner on the busy action and disables all buttons", () => {
    const wrapper = mountList(sample, "aaa111", "open_vault");
    expect(wrapper.find('[role="status"]').exists()).toBe(true);
    const buttons = wrapper.findAll("button");
    expect(buttons.length).toBe(4);
    expect(buttons.every((b) => b.attributes("disabled") !== undefined)).toBe(
      true,
    );
  });

  it("shows the path for vaults with duplicate names so they can be told apart", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Notes", path: "C:\\personal\\Notes" },
      { id: "bbb222", name: "Notes", path: "D:\\work\\Notes" },
    ]);
    expect(wrapper.text()).toContain("C:\\personal\\Notes");
    expect(wrapper.text()).toContain("D:\\work\\Notes");
  });

  it("disambiguates duplicate names in the accessible action labels too", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Notes", path: "C:\\personal\\Notes" },
      { id: "bbb222", name: "Notes", path: "D:\\work\\Notes" },
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
});
