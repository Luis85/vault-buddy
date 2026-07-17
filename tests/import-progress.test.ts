import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import ImportProgress from "../src/components/ImportProgress.vue";
import { useDocumentImportsStore } from "../src/stores/documentImports";

const activeImport = (over: Record<string, unknown> = {}) => ({
  fileName: "Report.docx",
  sourcePath: "C:\\docs\\Report.docx",
  vaultId: "v1",
  vaultName: "Personal",
  startedAtMs: 100_000,
  ...over,
});

describe("ImportProgress", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(100_000));
    setActivePinia(createPinia());
  });
  afterEach(() => vi.useRealTimers());

  it("renders nothing while no import is running", () => {
    const wrapper = mount(ImportProgress);
    expect(wrapper.find('[data-testid="import-progress"]').exists()).toBe(false);
  });

  it("shows the file, the target vault, and the elapsed time while active", () => {
    useDocumentImportsStore().active = activeImport({
      startedAtMs: 100_000 - 65_000,
    });
    const wrapper = mount(ImportProgress);
    const card = wrapper.get('[data-testid="import-progress"]');
    expect(card.text()).toContain("Report.docx");
    expect(card.text()).toContain("Personal");
    expect(card.get('[data-testid="import-elapsed"]').text()).toBe("1:05");
  });

  it("ticks the elapsed time while the conversion runs", async () => {
    useDocumentImportsStore().active = activeImport();
    const wrapper = mount(ImportProgress);
    expect(wrapper.get('[data-testid="import-elapsed"]').text()).toBe("0:00");
    vi.advanceTimersByTime(7_000);
    await wrapper.vm.$nextTick();
    expect(wrapper.get('[data-testid="import-elapsed"]').text()).toBe("0:07");
  });

  it("announces the conversion as a status region, with the ticking timer outside it", () => {
    // role="status" is aria-live: a timer INSIDE it would chatter a screen
    // reader every second, so only the stable label is the live region.
    useDocumentImportsStore().active = activeImport();
    const wrapper = mount(ImportProgress);
    const status = wrapper.get('[role="status"]');
    expect(status.text()).toContain("Report.docx");
    expect(status.find('[data-testid="import-elapsed"]').exists()).toBe(false);
  });

  it("shows an indeterminate activity bar (no fake percentage)", () => {
    useDocumentImportsStore().active = activeImport();
    const wrapper = mount(ImportProgress);
    expect(
      wrapper.get('[data-testid="import-progress"]').find('[data-testid="import-activity-bar"]').exists(),
    ).toBe(true);
  });
});
