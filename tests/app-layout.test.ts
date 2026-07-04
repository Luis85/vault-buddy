import {
  beforeAll,
  afterAll,
  beforeEach,
  afterEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { config, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { nextTick } from "vue";

// App.vue now imports the logging bridge; under mockIPC (which sets
// __TAURI_INTERNALS__) that would otherwise fire real plugin-log IPC.
vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));

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
  geometryWidths: [] as number[],
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
import { COLLAPSED, BUBBLE } from "../src/composables/useCompanionWindow";

const flush = () => new Promise((r) => setTimeout(r));

describe("App layout geometry", () => {
  const originalStubs = config.global.stubs;

  beforeAll(() => {
    // The bubble is wrapped in a real <Transition> in App.vue (fade in/out).
    // Stubbing it makes v-if add/remove synchronous so the "hides"/"stays
    // dismissed" absence assertions below don't have to wait out a real
    // ~150ms leave animation under fake/real timers.
    config.global.stubs = { ...originalStubs, transition: true };
  });

  afterAll(() => {
    config.global.stubs = originalStubs;
  });

  beforeEach(() => {
    localStorage.clear(); // settings persistence must not leak across tests
    setActivePinia(createPinia());
    mockIPC((cmd, args) => {
      if (cmd === "list_vaults") return [];
      if (cmd === "set_window_geometry") {
        state.geometryWidths.push((args as { width: number }).width);
      }
    });
    state.pos = { x: 100, y: 100 };
    state.eventHandlers = {};
    state.geometryWidths = [];
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

  it("toggles buddy dragging from the native menu event", async () => {
    const wrapper = mount(App);
    await flush(); // let onMounted register the event listener
    expect(wrapper.find("button.buddy").classes()).toContain("cursor-grab");

    state.eventHandlers["buddy-toggle-dragging"]?.();
    await nextTick();
    expect(wrapper.find("button.buddy").classes()).toContain("cursor-pointer");

    state.eventHandlers["buddy-toggle-dragging"]?.();
    await nextTick();
    expect(wrapper.find("button.buddy").classes()).toContain("cursor-grab");
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

  it("honors a real desktop click right after a drag (second blur)", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(100_000);
      const wrapper = mount(App);
      await flush();
      const store = useVaultsStore();
      await store.togglePanel();

      const buddy = wrapper.find("button.buddy");
      await buddy.trigger("pointerdown", {
        button: 0,
        screenX: 50,
        screenY: 50,
      });
      await buddy.trigger("pointermove", { screenX: 90, screenY: 90 });

      // blur #1 is the drag entering the OS move loop — suppressed
      state.focusHandler?.({ payload: false });
      await flush();
      expect(store.panelOpen).toBe(true);

      // blur #2 within the same window is the user clicking the desktop;
      // the window is unfocused now, so no later focus event will fire —
      // this one must close the panel or it stays stuck expanded
      state.focusHandler?.({ payload: false });
      await flush();
      expect(store.panelOpen).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows a greeting bubble on launch", async () => {
    const wrapper = mount(App);
    await flush();
    await nextTick();
    const bubble = wrapper.find('[data-testid="speech-bubble"]');
    expect(bubble.exists()).toBe(true);
    expect(bubble.text().length).toBeGreaterThan(0);
  });

  it("hides the greeting bubble once the panel opens", async () => {
    const wrapper = mount(App);
    await flush();
    await nextTick();
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);

    const store = useVaultsStore();
    await store.togglePanel();
    await flush();
    await nextTick();
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(false);
  });

  it("feeds the greeting into the window geometry (grows to bubble size on launch)", async () => {
    const wrapper = mount(App);
    await flush();
    await nextTick();
    await flush();
    // Without bubbleVisible wired into useCompanionWindow the window would
    // stay collapsed (88) on launch; a BUBBLE-width geometry call is proof
    // the greeting reached the geometry composable. 260 comes only from the
    // bubble state (panel is 440, collapsed 88).
    expect(state.geometryWidths).toContain(BUBBLE.width);
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);
  });

  it("stays dismissed after the panel opens and closes within the greeting window", async () => {
    const wrapper = mount(App);
    await flush();
    await nextTick();
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(true);

    const store = useVaultsStore();
    await store.togglePanel(); // opening must cancel the greeting timer
    await flush();
    await nextTick();
    await store.togglePanel(); // close again, still within the 5s greeting window
    await flush();
    await nextTick();
    // The bubble must NOT reappear. It is gone here only because dismiss()
    // set bubbleVisible=false; without that watch the bubble would render
    // again now that panelOpen is false.
    expect(wrapper.find('[data-testid="speech-bubble"]').exists()).toBe(false);
  });
});
