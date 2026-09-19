import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import ScreenSourcePicker from "../src/components/ScreenSourcePicker.vue";
import { useScreenCaptureStore } from "../src/stores/screenCapture";
import { useVaultsStore } from "../src/stores/vaults";

const SOURCES = [
  {
    id: "screen:1",
    kind: "screen",
    title: "Screen 1",
    detail: "2560x1440 · Primary",
    width: 2560,
    height: 1440,
    isPrimary: true,
  },
  {
    id: "window:1234",
    kind: "window",
    title: "Figma",
    detail: "Figma.exe",
    width: 1280,
    height: 800,
    isPrimary: false,
  },
];

const NO_DEVICES = { inputs: [], outputs: [] };

/** `ScreenStatusPayload` for a capture that really started. */
const STARTED = {
  capturing: true,
  vaultId: "v1",
  startedAtMs: 10,
  paused: false,
  pausedTotalMs: 0,
  pausedSinceMs: null,
  sourceTitle: "Screen 1",
};

const DEVICES = {
  inputs: [{ name: "Microphone (Yeti)", isDefault: true }],
  outputs: [{ name: "Speakers (Realtek)", isDefault: false }],
};

function mockSources(sources: unknown[] = SOURCES, devices: unknown = NO_DEVICES) {
  mockIPC((cmd) => {
    if (cmd === "list_capture_sources") return sources;
    if (cmd === "list_audio_devices") return devices;
    return undefined;
  });
}

async function mountPicker() {
  // Mount the picker on the view it actually lives on, so a navigation
  // assertion is a real change rather than the store's "list" default.
  useVaultsStore().openScreenCapture("v1");
  const wrapper = mount(ScreenSourcePicker, { props: { vaultId: "v1" } });
  await flushPromises();
  return wrapper;
}

/** TabGroup mounts EVERY panel and only `v-show`s the active one, so
 * `wrapper.text()` contains the inactive tab's rows too — asserting a title is
 * "absent" from the whole component would pass on a picker that lists
 * everything under one tab. Read the panel the row must live in instead. */
const panel = (w: ReturnType<typeof mount>, id: string) =>
  w.get(`[data-testid="panel-${id}"]`).text();

describe("ScreenSourcePicker", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("lists monitors under Screen and windows under Window", async () => {
    mockSources();
    const w = await mountPicker();
    expect(panel(w, "screen")).toContain("Screen 1");
    expect(panel(w, "screen")).toContain("2560x1440");
    // Both directions: a picker that ignored `kind` and listed everything
    // twice would satisfy either assertion on its own.
    expect(panel(w, "screen")).not.toContain("Figma");
    expect(panel(w, "window")).toContain("Figma");
    expect(panel(w, "window")).toContain("Figma.exe");
    expect(panel(w, "window")).not.toContain("Screen 1");
    // And the Window tab really is reachable, not just rendered.
    await w.get('[data-testid="tab-window"]').trigger("click");
    expect(w.get('[data-testid="tab-window"]').attributes("aria-selected")).toBe("true");
    expect(w.get('[data-testid="tab-screen"]').attributes("aria-selected")).toBe("false");
  });

  it("offers Screen and Window tabs, and no Region tab in this phase", async () => {
    // Region capture arrives in phase 3. A disabled tab is dead UI that
    // invites a click with nothing behind it. Asserting only the absence
    // would also pass on a picker that rendered no tabs at all, so the two
    // tabs that DO exist are pinned in the same test.
    mockSources();
    const w = await mountPicker();
    expect(w.find('[data-testid="tab-screen"]').exists()).toBe(true);
    expect(w.find('[data-testid="tab-window"]').exists()).toBe(true);
    expect(w.find('[data-testid="tab-region"]').exists()).toBe(false);
    expect(w.text()).not.toContain("Region");
  });

  it("keeps Start disabled until a source is picked", async () => {
    mockSources();
    const w = await mountPicker();
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
    expect(w.get('[data-testid="source-screen:1"]').attributes("aria-pressed")).toBe("true");
  });

  it("permits Start with zero audio devices and says so", async () => {
    // Spec 6.5 and 7.2: a silent capture is a real use case (a UI demo), so
    // the zero-device state is a NOTE, never a block.
    mockSources();
    const w = await mountPicker();
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w.text()).toContain("No audio will be recorded");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  it("drops the no-audio note once a device is ticked, and sends it to Rust", async () => {
    const args: unknown[] = [];
    mockIPC((cmd, a) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") {
        return {
          inputs: [{ name: "Microphone (Yeti)", isDefault: true }],
          outputs: [{ name: "Speakers (Realtek)", isDefault: false }],
        };
      }
      if (cmd === "start_screen_capture") {
        args.push(a);
        return {
          capturing: true,
          vaultId: "v1",
          startedAtMs: 10,
          paused: false,
          pausedTotalMs: 0,
          pausedSinceMs: null,
          sourceTitle: "Screen 1",
        };
      }
      return undefined;
    });
    const w = await mountPicker();
    expect(w.text()).toContain("No audio will be recorded");
    await w.get('[data-testid="audio-input-0"]').setValue(true);
    await w.get('[data-testid="audio-output-0"]').setValue(true);
    expect(w.text()).not.toContain("No audio will be recorded");
    await w.get('[data-testid="source-window:1234"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    // The ticked names travel VERBATIM — Rust matches them against what cpal
    // reports, so any normalisation would silently record nothing.
    expect(args).toEqual([
      {
        id: "v1",
        sourceId: "window:1234",
        inputs: ["Microphone (Yeti)"],
        outputs: ["Speakers (Realtek)"],
      },
    ]);
    // The capture bar lives on the list view, like the audio domain's.
    expect(useVaultsStore().view).toBe("list");
  });

  it("does not send a device the user ticked and then unticked", async () => {
    // Recompute-from-enumeration exists so an untick really removes the
    // device: a selection that only ever grows would record a microphone the
    // user explicitly turned off, with nothing on screen saying so.
    const args: unknown[] = [];
    mockIPC((cmd, a) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return DEVICES;
      if (cmd === "start_screen_capture") {
        args.push(a);
        return STARTED;
      }
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="audio-input-0"]').setValue(true);
    await w.get('[data-testid="audio-output-0"]').setValue(true);
    await w.get('[data-testid="audio-input-0"]').setValue(false);
    // The tick box is the user's only readback of what is armed, so it has to
    // follow the untick as well as the list that reaches Rust.
    const mic = w.get('[data-testid="audio-input-0"]').element as HTMLInputElement;
    expect(mic.checked).toBe(false);
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(args).toEqual([
      { id: "v1", sourceId: "screen:1", inputs: [], outputs: ["Speakers (Realtek)"] },
    ]);
  });

  it("drops the no-audio note for a system-audio-only selection", async () => {
    // "No audio will be recorded" must read the whole selection: a loopback
    // capture with no microphone records audio, so claiming silence there
    // would be a lie about what is on tape.
    mockSources(SOURCES, DEVICES);
    const w = await mountPicker();
    expect(w.text()).toContain("No audio will be recorded");
    await w.get('[data-testid="audio-output-0"]').setValue(true);
    expect(w.text()).not.toContain("No audio will be recorded");
  });

  it("refuses a start whose source vanished, and refreshes the list", async () => {
    // Spec 7.2 and 14: never a started-then-dead capture. The refusal is
    // inline and the list is re-read, so the user's next click is against
    // reality rather than against the stale list they just failed on.
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        return listed === 1 ? SOURCES : [SOURCES[0]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "start_screen_capture") {
        throw new Error("the capture source is no longer available");
      }
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="source-window:1234"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-error"]').text()).toContain("no longer available");
    // The re-read is the point: two enumerations, and the vanished row is
    // gone from the refreshed list.
    expect(listed).toBe(2);
    expect(w.find('[data-testid="source-window:1234"]').exists()).toBe(false);
    // A refused start must not navigate away, and must not leave the store
    // believing a capture is running.
    expect(useVaultsStore().view).toBe("screenCapture");
    expect(useScreenCaptureStore().status).toBe("idle");
  });

  it("clears the selection when the picked source vanishes from the refreshed list", async () => {
    // Leaving it selected would re-arm Start against a row that is no longer
    // on screen: the next click reproduces the same failure with no visible
    // cause.
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        return listed === 1 ? SOURCES : [SOURCES[0]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "start_screen_capture") {
        throw new Error("the capture source is no longer available");
      }
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="source-window:1234"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
  });

  it("shows an empty state rather than an error when nothing is capturable", async () => {
    mockSources([]);
    const w = await mountPicker();
    expect(w.text()).toContain("No capture sources");
    expect(w.find('[data-testid="screen-error"]').exists()).toBe(false);
  });

  it("surfaces an enumeration failure inline instead of a silent empty list", async () => {
    // An empty list and a failed read mean different things: "nothing to
    // capture" invites the user to open a window, a failure invites a retry.
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") throw new Error("enumeration exploded");
      if (cmd === "list_audio_devices") return NO_DEVICES;
      return undefined;
    });
    const w = await mountPicker();
    expect(w.get('[data-testid="screen-error"]').text()).toContain("enumeration exploded");
  });
});
