import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { nextTick } from "vue";

const state = vi.hoisted(() => ({
  pos: { x: 100, y: 100 },
  monitor: {
    position: { x: 0, y: 0 },
    size: { width: 1920, height: 1080 },
  },
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    outerPosition: () => Promise.resolve({ ...state.pos }),
    scaleFactor: () => Promise.resolve(1),
    setPosition: (p: { x: number; y: number }) => {
      state.pos = { x: p.x, y: p.y };
      return Promise.resolve();
    },
    setSize: () => Promise.resolve(),
    startDragging: () => Promise.resolve(),
  }),
  currentMonitor: () => Promise.resolve(state.monitor),
  LogicalSize: class {
    constructor(
      public width: number,
      public height: number,
    ) {}
  },
  PhysicalPosition: class {
    constructor(
      public x: number,
      public y: number,
    ) {}
  },
}));

import App from "../src/App.vue";
import { useVaultsStore } from "../src/stores/vaults";
import { COLLAPSED } from "../src/composables/useCompanionWindow";

const flush = () => new Promise((r) => setTimeout(r));

describe("App layout geometry", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return [];
    });
    state.pos = { x: 100, y: 100 };
  });

  afterEach(() => {
    clearMocks();
  });

  it("keeps the buddy in a fixed cell with the collapsed-window size", () => {
    // the placement offset math assumes this exact cell geometry; if the
    // cell size drifts from COLLAPSED the buddy will jump when flipping
    expect(COLLAPSED).toEqual({ width: 140, height: 170 });
    const wrapper = mount(App);
    const cell = wrapper.find('[data-testid="buddy-cell"]');
    expect(cell.classes()).toContain("w-[140px]");
    expect(cell.classes()).toContain("h-[170px]");
    expect(cell.classes()).toContain("shrink-0");
  });

  it("lays out right/down when there is room", async () => {
    const wrapper = mount(App);
    const store = useVaultsStore();
    await store.togglePanel();
    await flush();
    await nextTick();
    expect(wrapper.find("main").classes()).toContain("flex-row");
    expect(wrapper.find("main").classes()).toContain("items-start");
  });

  it("mirrors the layout when the buddy sits in the bottom-right corner", async () => {
    state.pos = { x: 1780, y: 910 };
    const wrapper = mount(App);
    const store = useVaultsStore();
    await store.togglePanel();
    await flush();
    await nextTick();
    expect(wrapper.find("main").classes()).toContain("flex-row-reverse");
    expect(wrapper.find("main").classes()).toContain("items-end");
  });
});
