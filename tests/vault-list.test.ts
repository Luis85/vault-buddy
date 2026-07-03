import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import VaultList from "../src/components/VaultList.vue";

const mountList = (vaults: Array<{ id: string; name: string; path: string }>) =>
  mount(VaultList, { props: { vaults, busyVaultId: null } });

describe("VaultList", () => {
  it("shows the path for vaults with duplicate names so they can be told apart", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Notes", path: "C:\\personal\\Notes" },
      { id: "bbb222", name: "Notes", path: "D:\\work\\Notes" },
    ]);
    expect(wrapper.text()).toContain("C:\\personal\\Notes");
    expect(wrapper.text()).toContain("D:\\work\\Notes");
  });

  it("hides the path when vault names are unique", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Personal", path: "C:\\vaults\\Personal" },
      { id: "bbb222", name: "Work", path: "C:\\vaults\\Work" },
    ]);
    expect(wrapper.text()).not.toContain("C:\\vaults\\Personal");
    expect(wrapper.text()).not.toContain("C:\\vaults\\Work");
  });

  it("always exposes the full path as a tooltip on the row", () => {
    const wrapper = mountList([
      { id: "aaa111", name: "Personal", path: "C:\\vaults\\Personal" },
    ]);
    expect(wrapper.find("li").attributes("title")).toBe("C:\\vaults\\Personal");
  });
});
