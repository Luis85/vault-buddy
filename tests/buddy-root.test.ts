import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises,mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import BuddyRoot from "../src/roots/BuddyRoot.vue";
import { useCaptureStore } from "../src/stores/capture";
import { useScreenCaptureStore } from "../src/stores/screenCapture";
import { useSettingsStore } from "../src/stores/settings";

vi.mock("@tauri-apps/plugin-log", () => ({
  info: vi.fn(),
  warn: vi.fn(),
  error: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

// `getCurrentWebview().onDragDropEvent` is stubbed here (not left to the
// "no Tauri runtime" catch every other listener falls into) so the
// buddy-drop tests below can drive the handler directly and assert what it
// does with drop/non-drop payloads and supported/unsupported extensions.
type DragDropPayload =
  | { type: "drop" | "enter"; paths: string[] }
  | { type: "over" | "leave" | "cancel"; paths?: string[] };
type DragDropHandler = (event: { payload: DragDropPayload }) => void;
const dragDropMocks = vi.hoisted(() => ({
  onDragDropEvent: vi.fn(),
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: dragDropMocks.onDragDropEvent }),
}));

const calls: string[] = [];
const argsLog: { cmd: string; args: unknown }[] = [];

describe("BuddyRoot", () => {
  beforeEach(() => {
    localStorage.clear();
    setActivePinia(createPinia());
    calls.length = 0;
    argsLog.length = 0;
    dragDropMocks.onDragDropEvent.mockReset();
    dragDropMocks.onDragDropEvent.mockResolvedValue(() => {});
    mockIPC((cmd, args) => {
      calls.push(cmd);
      argsLog.push({ cmd, args });
      if (cmd === "start_buddy_drag") return true;
    });
  });
  afterEach(() => clearMocks());

  it("initializes the screen-capture store on mount, like the panel window does", async () => {
    // The buddy IS the capture indicator, so it must learn about a screen
    // capture the panel started. Each window is its own webview with its own
    // Pinia instance; initialising in only one leaves the other dead — the
    // same rule capture.init() already follows in both roots.
    mount(BuddyRoot);
    await flushPromises();
    expect(calls).toContain("screen_capture_status");
  });

  it("shows the recording badge while a SCREEN capture runs", async () => {
    // The buddy is documented (spec 7.3, AGENTS.md) as the capture indicator
    // for BOTH capture kinds, and `tray::hide_buddy` refuses to hide it
    // mid-screen-capture on exactly that premise. It bound only the AUDIO
    // store, so a screen capture left the buddy looking idle while Hide
    // silently did nothing — the user is refused with no visible reason.
    const wrapper = mount(BuddyRoot);
    await flushPromises();
    const screen = useScreenCaptureStore();
    screen.$patch({ status: "capturing" });
    await wrapper.vm.$nextTick();
    expect(wrapper.find("button.buddy").classes()).toContain("recording");
    expect(wrapper.find(".rec-dot").exists()).toBe(true);
  });

  it("shows the paused tint while a SCREEN capture is paused", async () => {
    // The screen store's tri-state is not the audio store's two booleans:
    // `paused` is a getter over `status === "paused"`, so the paused arm has
    // to read the screen store too or a paused capture renders as a live one.
    const wrapper = mount(BuddyRoot);
    await flushPromises();
    const screen = useScreenCaptureStore();
    screen.$patch({ status: "paused" });
    await wrapper.vm.$nextTick();
    const buddy = wrapper.find("button.buddy");
    expect(buddy.classes()).toContain("recording");
    expect(buddy.classes()).toContain("paused");
  });

  it("stays idle-looking when no capture of either kind runs", async () => {
    // The other half of the badge: `status !== "idle"` must not be a way of
    // saying "always on".
    const wrapper = mount(BuddyRoot);
    await flushPromises();
    expect(wrapper.find("button.buddy").classes()).not.toContain("recording");
    expect(wrapper.find(".rec-dot").exists()).toBe(false);
  });

  it("toggles the panel when the buddy is clicked", async () => {
    const wrapper = mount(BuddyRoot);
    await wrapper.find("button.buddy").trigger("click");
    expect(calls).toContain("toggle_panel");
  });

  it("closes the panel when a drag starts", async () => {
    const wrapper = mount(BuddyRoot);
    const buddy = wrapper.find("button.buddy");
    await buddy.trigger("pointerdown", { button: 0, screenX: 50, screenY: 50 });
    await buddy.trigger("pointermove", { buttons: 1, screenX: 90, screenY: 90 });
    await Promise.resolve();
    expect(calls).toContain("start_buddy_drag");
    expect(calls).toContain("close_panel");
  });

  it("re-syncs settings from localStorage on a cross-window storage event", async () => {
    mount(BuddyRoot);
    await Promise.resolve();
    localStorage.setItem("vault-buddy.animations", "off");
    window.dispatchEvent(new Event("storage"));
    expect(useSettingsStore().animationsEnabled).toBe(false);
  });

  it("puts the buddy in its working animation while transcribing", async () => {
    const wrapper = mount(BuddyRoot);
    await flushPromises();
    const capture = useCaptureStore();
    // transcription is the buddy's "working" state — it should run/pulse, not
    // just show the dot. Driven from the capture store, like recording/paused.
    capture.transcriptions = {
      "/v/a.mp3": {
        mp3: "/v/a.mp3",
        vaultId: "v1",
        name: "a",
        phase: "preparing",
        progress: null,
        model: null,
        error: null,
        startedAtMs: Date.now(),
      },
    };
    await wrapper.vm.$nextTick();
    expect(wrapper.find("button.buddy").classes()).toContain("working");
  });

  it("reads the position-derived buddy facing from Rust on mount", async () => {
    mount(BuddyRoot);
    await flushPromises();
    // Facing is derived from the buddy's position by Rust; the buddy window
    // pulls the initial value on mount and then listens for `buddy-facing`
    // flips (it no longer pushes a stored facing setting to Rust).
    expect(calls).toContain("get_buddy_facing");
    expect(calls).not.toContain("set_buddy_facing");
  });

  describe("document drag-drop", () => {
    let handler: DragDropHandler | undefined;

    beforeEach(() => {
      handler = undefined;
      dragDropMocks.onDragDropEvent.mockImplementation((cb: DragDropHandler) => {
        handler = cb;
        return Promise.resolve(() => {});
      });
    });

    it("begins an import for a supported dropped document", async () => {
      mount(BuddyRoot);
      await flushPromises();
      handler?.({
        payload: { type: "drop", paths: ["C:/x/Notes.txt", "C:/x/Report.DOCX"] },
      });
      await flushPromises();
      expect(calls).toContain("begin_document_import");
      // Extension match is case-insensitive; the first supported path wins —
      // no toggle_panel and no event emit (begin_document_import shows the
      // panel itself, see AGENTS.md's "why not emit-then-toggle").
      expect(
        argsLog.find((c) => c.cmd === "begin_document_import")?.args,
      ).toEqual({ path: "C:/x/Report.DOCX" });
      expect(calls).not.toContain("toggle_panel");
    });

    it("ignores a drop with no supported document extension", async () => {
      mount(BuddyRoot);
      await flushPromises();
      handler?.({ payload: { type: "drop", paths: ["C:/x/image.png"] } });
      await flushPromises();
      expect(calls).not.toContain("begin_document_import");
    });

    it("ignores non-drop drag events (hover/cancel)", async () => {
      mount(BuddyRoot);
      await flushPromises();
      handler?.({ payload: { type: "over", paths: ["C:/x/Report.docx"] } });
      handler?.({ payload: { type: "cancel" } });
      await flushPromises();
      expect(calls).not.toContain("begin_document_import");
    });

    it("highlights the buddy while a supported document is dragged over it", async () => {
      const wrapper = mount(BuddyRoot);
      await flushPromises();
      handler?.({ payload: { type: "enter", paths: ["C:/x/Report.docx"] } });
      await wrapper.vm.$nextTick();
      expect(wrapper.find("button.buddy").classes()).toContain("drop-target");
    });

    it("does not highlight while dragging an unsupported file", async () => {
      const wrapper = mount(BuddyRoot);
      await flushPromises();
      handler?.({ payload: { type: "enter", paths: ["C:/x/photo.png"] } });
      await wrapper.vm.$nextTick();
      expect(wrapper.find("button.buddy").classes()).not.toContain("drop-target");
    });

    it("clears the highlight when the drag leaves", async () => {
      const wrapper = mount(BuddyRoot);
      await flushPromises();
      handler?.({ payload: { type: "enter", paths: ["C:/x/Report.docx"] } });
      await wrapper.vm.$nextTick();
      expect(wrapper.find("button.buddy").classes()).toContain("drop-target");
      handler?.({ payload: { type: "leave" } });
      await wrapper.vm.$nextTick();
      expect(wrapper.find("button.buddy").classes()).not.toContain("drop-target");
    });

    it("clears the highlight after a drop", async () => {
      const wrapper = mount(BuddyRoot);
      await flushPromises();
      handler?.({ payload: { type: "enter", paths: ["C:/x/Report.docx"] } });
      await wrapper.vm.$nextTick();
      expect(wrapper.find("button.buddy").classes()).toContain("drop-target");
      handler?.({ payload: { type: "drop", paths: ["C:/x/Report.docx"] } });
      await wrapper.vm.$nextTick();
      expect(wrapper.find("button.buddy").classes()).not.toContain("drop-target");
    });

    it("unregisters the drop listener on unmount", async () => {
      const unlisten = vi.fn();
      dragDropMocks.onDragDropEvent.mockResolvedValue(unlisten);
      const wrapper = mount(BuddyRoot);
      await flushPromises();
      wrapper.unmount();
      expect(unlisten).toHaveBeenCalled();
    });

    it("degrades quietly when onDragDropEvent is unavailable", async () => {
      dragDropMocks.onDragDropEvent.mockRejectedValue(new Error("no runtime"));
      expect(() => mount(BuddyRoot)).not.toThrow();
      await flushPromises();
    });
  });
});
