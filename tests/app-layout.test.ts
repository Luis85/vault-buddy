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
  focusHandler: null as
    | ((event: { payload: boolean }) => void)
    | null,
  eventHandlers: {} as Record<string, () => void>,
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: () => void) => {
    state.eventHandlers[name] = handler;
    return Promise.resolve(() => {
      delete state.eventHandlers[name];
    });
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
    onFocusChanged: (handler: (event: { payload: boolean }) => void) => {
      state.focusHandler = handler;
      return Promise.resolve(() => {
        state.focusHandler = null;
      });
    },
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
    localStorage.clear(); // settings persistence must not leak across tests
    setActivePinia(createPinia());
    mockIPC((cmd) => {
      if (cmd === "list_vaults") return [];
    });
    state.pos = { x: 100, y: 100 };
    state.eventHandlers = {};
  });

  afterEach(() => {
    clearMocks();
  });

  it("keeps the buddy in a fixed cell with the collapsed-window size", () => {
    // the placement offset math assumes this exact cell geometry; if the
    // cell size drifts from COLLAPSED the buddy will jump when flipping
    expect(COLLAPSED).toEqual({ width: 88, height: 88 });
    const wrapper = mount(App);
    const cell = wrapper.find('[data-testid="buddy-cell"]');
    expect(cell.classes()).toContain("w-[88px]");
    expect(cell.classes()).toContain("h-[88px]");
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

  it("closes the panel on Escape", async () => {
    mount(App);
    const store = useVaultsStore();
    await store.togglePanel();
    expect(store.panelOpen).toBe(true);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    await flush();
    expect(store.panelOpen).toBe(false);
  });

  it("toggles the buddy animation from the native menu event", async () => {
    const wrapper = mount(App);
    await flush(); // let onMounted register the event listener
    expect(wrapper.find("button.buddy").classes()).not.toContain("still");

    state.eventHandlers["buddy-toggle-animation"]?.();
    await nextTick();
    expect(wrapper.find("button.buddy").classes()).toContain("still");

    state.eventHandlers["buddy-toggle-animation"]?.();
    await nextTick();
    expect(wrapper.find("button.buddy").classes()).not.toContain("still");
  });

  it("closes the panel when the transparent gutter is clicked", async () => {
    const wrapper = mount(App);
    const store = useVaultsStore();
    await store.togglePanel();
    await flush();
    expect(store.panelOpen).toBe(true);
    // a click that lands on <main> itself is in the invisible gutter —
    // the user thinks they clicked the desktop
    await wrapper.find("main").trigger("click");
    await flush();
    expect(store.panelOpen).toBe(false);
  });

  it("closes the panel when the window loses focus", async () => {
    mount(App);
    await flush(); // let onMounted register the focus listener
    const store = useVaultsStore();
    await store.togglePanel();
    expect(store.panelOpen).toBe(true);
    state.focusHandler?.({ payload: false });
    await flush();
    expect(store.panelOpen).toBe(false);
  });

  it("keeps the panel open when the focus loss comes from a buddy drag", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(100_000);
      const wrapper = mount(App);
      await flush();
      const store = useVaultsStore();
      await store.togglePanel();
      expect(store.panelOpen).toBe(true);

      // dragging the buddy enters the OS move loop, which steals focus;
      // collapsing the window mid-drag would cancel the drag and dump the
      // buddy at the panel's old top-left corner
      const buddy = wrapper.find("button.buddy");
      await buddy.trigger("pointerdown", {
        button: 0,
        screenX: 50,
        screenY: 50,
      });
      await buddy.trigger("pointermove", { screenX: 90, screenY: 90 });
      state.focusHandler?.({ payload: false });
      await flush();
      expect(store.panelOpen).toBe(true); // still open — drag in progress

      // a focus loss well after the drag started is a real one
      vi.setSystemTime(100_000 + 5_000);
      state.focusHandler?.({ payload: false });
      await flush();
      expect(store.panelOpen).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});
