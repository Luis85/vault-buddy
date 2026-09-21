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

/** A second monitor. Every Q1 case needs TWO, because the defect it pins is
 * the region's own monitor diverging from the highlighted target row. */
const SCREEN_2 = {
  id: "screen:2",
  kind: "screen",
  title: "LG 27",
  detail: "1920x1080",
  width: 1920,
  height: 1080,
  isPrimary: false,
};

/** Two monitors and the window, in enumeration order. */
const TWO_SCREENS = [SOURCES[0], SCREEN_2, SOURCES[1]];

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

/** The reply `select_capture_region` gives for a 1280x720 region at
 * (320, 180) on display 1. */
const REGION = {
  sourceId: "region:1,320,180,1280,720",
  x: 320,
  y: 180,
  width: 1280,
  height: 720,
};

function mockSources(
  sources: unknown[] = SOURCES,
  devices: unknown = NO_DEVICES,
  region: unknown = REGION,
) {
  mockIPC((cmd) => {
    if (cmd === "list_capture_sources") return sources;
    if (cmd === "list_audio_devices") return devices;
    if (cmd === "select_capture_region") return region;
    return undefined;
  });
}

async function mountPicker(attach = false) {
  // Mount the picker on the view it actually lives on, so a navigation
  // assertion is a real change rather than the store's "list" default.
  useVaultsStore().openScreenCapture("v1");
  // `attachTo` only for the focus test: document.activeElement is meaningless
  // for a detached tree.
  const wrapper = mount(ScreenSourcePicker, {
    props: { vaultId: "v1" },
    ...(attach ? { attachTo: document.body } : {}),
  });
  await flushPromises();
  return wrapper;
}

/** The region row's own element, so an assertion reads the ROW rather than
 * the whole panel — which also carries the target rows' monitor names. */
const regionRow = (w: ReturnType<typeof mount>) =>
  w.get(`[data-testid="source-${REGION.sourceId}"]`);

/** Arm a region on `target`, from a two-monitor list. */
async function armRegionOn(w: ReturnType<typeof mount>, target: string) {
  await w.get('[data-testid="tab-region"]').trigger("click");
  await w.get(`[data-testid="region-target-${target}"]`).trigger("click");
  await w.get('[data-testid="region-select"]').trigger("click");
  await flushPromises();
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

  it("offers Screen, Window and Region tabs", async () => {
    // GAP-111 item 2: phase 2 shipped without a Region tab on purpose and
    // pinned its absence; phase 3 adds the tab and flips the pin, in the
    // same commit, as that entry requires.
    mockSources();
    const w = await mountPicker();
    expect(w.find('[data-testid="tab-screen"]').exists()).toBe(true);
    expect(w.find('[data-testid="tab-window"]').exists()).toBe(true);
    expect(w.find('[data-testid="tab-region"]').exists()).toBe(true);
    // Labels, not just testids: a tab whose id is right and whose label reads
    // "Regions!" ships a typo no other assertion in this suite would catch.
    expect(w.get('[data-testid="tab-screen"]').text()).toBe("Screen");
    expect(w.get('[data-testid="tab-window"]').text()).toBe("Window");
    expect(w.get('[data-testid="tab-region"]').text()).toBe("Region");
    // And it is reachable, not merely rendered.
    await w.get('[data-testid="tab-region"]').trigger("click");
    expect(w.get('[data-testid="tab-region"]').attributes("aria-selected")).toBe("true");
  });

  it("selects a region on the picked monitor and arms Start with it", async () => {
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, ...(args as object) });
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") return STARTED;
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();

    // The overlay is opened for the monitor the user picked, not "the
    // primary" and not whatever the Screen tab happened to have selected.
    expect(calls.find((c) => c.cmd === "select_capture_region")).toMatchObject({
      sourceId: "screen:1",
    });
    // The row reads as spec 7.2 asks: size, origin, and which screen.
    expect(panel(w, "region")).toContain("1280x720 at (320, 180)");
    expect(panel(w, "region")).toContain("Region on Screen 1");
    // And Start now sends the region id, not the monitor id.
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "start_screen_capture")).toMatchObject({
      sourceId: "region:1,320,180,1280,720",
    });
  });

  it("keeps a cancelled region selection from arming Start", async () => {
    // `select_capture_region` resolves null when the user pressed Escape or
    // clicked without dragging. Arming Start off a null would send the
    // string "null" as a source id.
    mockSources(SOURCES, NO_DEVICES, null);
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
    expect(w.find('[data-testid="source-region:1,320,180,1280,720"]').exists()).toBe(false);
  });

  it("keeps the region already chosen when a reselect is cancelled", async () => {
    // The other half of the cancel contract, and the half a naive
    // `region.value = picked` gets wrong: a cancelled RESELECT must leave the
    // region that was already armed exactly as it was, not wipe it and
    // silently disarm Start.
    let picks = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") {
        picks += 1;
        return picks === 1 ? REGION : null;
      }
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    // Second run: the user opened the overlay again and pressed Escape.
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(picks).toBe(2);
    expect(panel(w, "region")).toContain("1280x720 at (320, 180)");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  it("surfaces a failed region selection inline", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") throw new Error("That screen is no longer connected.");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-error"]').text()).toContain("no longer connected");
  });

  // THE REGRESSION THIS TASK IS MOST LIKELY TO SHIP. `loadSources()` drops
  // a selection the refreshed list no longer offers -- and a region id is
  // NEVER in that list, so the naive check clears it on every refresh and
  // Start silently disarms itself.
  it("keeps a selected region across a source refresh", async () => {
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    // A failed start triggers a refresh; the region must survive it.
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(panel(w, "region")).toContain("1280x720 at (320, 180)");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  // The other direction: if the region's own MONITOR goes away, the region
  // is as gone as a closed window and must not stay armed.
  it("drops a selected region when its monitor disappears", async () => {
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        // Second read: the monitor is unplugged, only the window remains.
        return listed === 1 ? SOURCES : [SOURCES[1]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(panel(w, "region")).not.toContain("1280x720 at (320, 180)");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
  });

  it("keeps the region Select button disabled until a monitor is picked", async () => {
    mockSources();
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeDefined();
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeUndefined();
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
        // Both fixtures carry surrounding whitespace ON PURPOSE. `cpal`
        // reports an endpoint's name as the driver spells it, padding and
        // all, and `start_screen_capture` opens each endpoint BY NAME — so a
        // `.trim()` anywhere on this path matches no device and the capture
        // records silence with nothing to show for it. Names without padding
        // cannot tell a verbatim pass from a normalising one.
        return {
          inputs: [{ name: "Microphone (Yeti) ", isDefault: true }],
          outputs: [{ name: " Speakers (Realtek)", isDefault: false }],
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
    // reports, so any normalisation would silently record nothing. The
    // padding in the fixtures is what makes this assertion able to say so:
    // it fails for a `.trim()` as well as for a `.toLowerCase()`.
    expect(args).toEqual([
      {
        id: "v1",
        sourceId: "window:1234",
        inputs: ["Microphone (Yeti) "],
        outputs: [" Speakers (Realtek)"],
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

  // ---------------------------------------------------------------------
  // The region's monitor is the one it was CUT FROM, not the highlighted
  // target row. `regionTargetId` moves on any click; once the two diverge
  // every consumer that keys off the target is wrong. Each of the next three
  // tests needs TWO monitors and a target switch — the phase-3 test
  // `drops a selected region when its monitor disappears` misses all three
  // because there the target and the region's monitor coincide.
  // ---------------------------------------------------------------------

  it("labels a region after the monitor it was cut from, not the highlighted target", async () => {
    // The Rust/TS parity failure `utils/regionLabel.ts` exists to prevent:
    // the capture bar and the staging sidecar render Rust's own
    // `Region on {monitor}` for the id that was started, so a row naming a
    // different monitor is two spellings of one capture.
    mockSources(TWO_SCREENS);
    const w = await mountPicker();
    await armRegionOn(w, "screen:1");
    expect(regionRow(w).text()).toContain("Region on Screen 1");
    // Merely highlighting the other monitor must not re-label the region
    // that is already armed.
    await w.get('[data-testid="region-target-screen:2"]').trigger("click");
    expect(regionRow(w).text()).toContain("Region on Screen 1");
    expect(regionRow(w).text()).not.toContain("LG 27");
  });

  it("keeps a region when a DIFFERENT monitor disappears", async () => {
    // Region on Screen 1, target moved to the LG, the LG unplugged: the
    // region is untouched and Start stays armed. Dropping it here is the
    // same "silently disarms itself" failure the region-survives-refresh
    // rule was written to stop, reached through another door.
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        return listed === 1 ? TWO_SCREENS : [SOURCES[0], SOURCES[1]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await armRegionOn(w, "screen:1");
    await w.get('[data-testid="region-target-screen:2"]').trigger("click");
    // A refused start refreshes the list.
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(regionRow(w).text()).toContain("1280x720 at (320, 180)");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  it("drops a region whose OWN monitor disappears even after the target moved", async () => {
    // Spec 7.2's "refuses to start when the picked source has vanished":
    // leaving this armed sends `region:1,...` to a monitor that is gone and
    // `source::resolve` answers SourceGone — the exact failure with no
    // visible cause the refusal exists to prevent.
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        return listed === 1 ? TWO_SCREENS : [SCREEN_2, SOURCES[1]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await armRegionOn(w, "screen:1");
    await w.get('[data-testid="region-target-screen:2"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(w.find(`[data-testid="source-${REGION.sourceId}"]`).exists()).toBe(false);
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
  });

  it("drops a dangling region target when its monitor disappears", async () => {
    // Same drop discipline, with no region held: a target left pointing at an
    // unplugged monitor keeps "Select region…" enabled on an id the command
    // can only refuse.
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") {
        listed += 1;
        return listed === 1 ? TWO_SCREENS : [SOURCES[0], SOURCES[1]];
      }
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "start_screen_capture") throw new Error("nope");
      return undefined;
    });
    const w = await mountPicker();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:2"]').trigger("click");
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeUndefined();
    // Force a refresh through a refused start on an unrelated source.
    await w.get('[data-testid="tab-screen"]').trigger("click");
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeDefined();
  });

  it("refuses Start while a region selection is in flight", async () => {
    // The overlay covers the target monitor while this panel sits on another
    // and stays clickable (select_capture_region sets DIALOG_ACTIVE, so the
    // panel does not even auto-hide). An ungated Start records under the
    // overlay AND navigates away, unmounting the picker the pending drag has
    // to resolve into.
    let settle: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") {
        return new Promise((resolve) => {
          settle = resolve;
        });
      }
      return undefined;
    });
    const w = await mountPicker();
    // Arm Start off the Screen tab first, so the only thing standing between
    // this click and a capture is the in-flight selection.
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeDefined();
    // And the gate lifts again once the user escapes the overlay.
    settle(null);
    await flushPromises();
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  it("refuses a region selection while a start is in flight", async () => {
    // The symmetric hole: opening the overlay behind a start that is already
    // committing leaves the overlay orphaned over a live capture.
    let settle: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") {
        return new Promise((resolve) => {
          settle = resolve;
        });
      }
      return undefined;
    });
    const w = await mountPicker();
    await armRegionOn(w, "screen:1");
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeUndefined();
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="region-select"]').attributes("disabled")).toBeDefined();
    settle(STARTED);
    await flushPromises();
  });

  it("shows an empty state on the Region tab when no monitor is enumerable", async () => {
    // The two list tabs say so when they have nothing; without this the tab
    // reads "Pick a screen…" above an empty list, and a disabled button reads
    // as "not yet" rather than "there is nothing here".
    mockSources([]);
    const w = await mountPicker();
    expect(panel(w, "region")).toContain("No capture sources available.");
    expect(panel(w, "region")).not.toContain("Pick a screen");
  });

  it("returns focus to the region button after a cancelled selection", async () => {
    // GAP-27 family: clicking the button disables it, which drops focus to
    // <body>, and a keyboard user who then escapes the overlay lands at the
    // top of the document with nothing selected.
    let settle: (v: unknown) => void = () => {};
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") {
        return new Promise((resolve) => {
          settle = resolve;
        });
      }
      return undefined;
    });
    const w = await mountPicker(true);
    await w.get('[data-testid="tab-region"]').trigger("click");
    await w.get('[data-testid="region-target-screen:1"]').trigger("click");
    const button = w.get('[data-testid="region-select"]').element as HTMLButtonElement;
    button.focus();
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    // happy-dom does not reproduce the browser's "a disabled element loses
    // focus" behaviour, so drop focus to <body> the way the browser does.
    document.body.focus();
    expect(document.activeElement).toBe(document.body);
    settle(null);
    await flushPromises();
    expect(document.activeElement).toBe(button);

    // The other direction: focus we did NOT drop is not stolen back. A user
    // who tabbed on while the overlay was up must keep where they are.
    const tab = w.get('[data-testid="tab-region"]').element as HTMLElement;
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    tab.focus();
    settle(null);
    await flushPromises();
    expect(document.activeElement).toBe(tab);
    w.unmount();
  });

  it("re-arms Start from the region row after another source was picked", async () => {
    // The region row is not decoration: select a region, pick a monitor on the
    // Screen tab instead, then come back and click the region row to re-arm
    // Start with it.
    const calls: Record<string, unknown>[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, ...(args as object) });
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") return REGION;
      if (cmd === "start_screen_capture") return STARTED;
      return undefined;
    });
    const w = await mountPicker();
    await armRegionOn(w, "screen:1");
    expect(regionRow(w).attributes("aria-pressed")).toBe("true");
    await w.get('[data-testid="tab-screen"]').trigger("click");
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(regionRow(w).attributes("aria-pressed")).toBe("false");
    await w.get('[data-testid="tab-region"]').trigger("click");
    await regionRow(w).trigger("click");
    expect(regionRow(w).attributes("aria-pressed")).toBe("true");
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(calls.find((c) => c.cmd === "start_screen_capture")).toMatchObject({
      sourceId: REGION.sourceId,
    });
  });

  it("clears a stale selection error once a reselect succeeds", async () => {
    // A banner left over from a failed attempt, still on screen above a
    // region that was just selected successfully, reads as a failure.
    let picks = 0;
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "select_capture_region") {
        picks += 1;
        if (picks === 1) throw new Error("That screen is no longer connected.");
        return REGION;
      }
      return undefined;
    });
    const w = await mountPicker();
    await armRegionOn(w, "screen:1");
    expect(w.get('[data-testid="screen-error"]').text()).toContain("no longer connected");
    await w.get('[data-testid="region-select"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="screen-error"]').exists()).toBe(false);
    expect(regionRow(w).text()).toContain("1280x720 at (320, 180)");
  });

  // --- Staged captures: resume or discard, shown first (spec 10) ---

  /** One row of `list_staged_captures`, as Rust's `StagedCaptureSummaryDto`
   * serialises it. */
  const STAGED_ROW = {
    base: "2026-09-20 1000 Figma",
    vaultId: "v1",
    sourceTitle: "Figma",
    durationMs: 60_000,
    outputDurationMs: 60_000,
    recordedAt: "2026-09-20T10:00:00Z",
    width: 1920,
    height: 1080,
    edited: false,
  };

  /** `mockSources` answers `list_staged_captures` with `undefined`, which is
   * the "no staged captures" path; these cases need the populated one. */
  function mockStaged(
    rows: unknown[] = [STAGED_ROW],
    extra: (cmd: string, args: unknown) => unknown = () => undefined,
  ) {
    const calls: Array<{ cmd: string; args: unknown }> = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "list_staged_captures") return rows;
      return extra(cmd, args);
    });
    return calls;
  }

  it("shows staged captures above the source tabs, not below or behind one", async () => {
    mockStaged();
    const w = await mountPicker();
    // Positional, not merely present: the spec says shown FIRST. A fourth
    // tab, or a block appended after the picker, would satisfy "exists".
    const html = w.html();
    expect(html).toContain('data-testid="staged-list"');
    expect(html.indexOf('data-testid="staged-list"')).toBeLessThan(
      html.indexOf('role="tablist"'),
    );
  });

  it("shows no staged block at all when nothing is staged", async () => {
    mockStaged([]);
    const w = await mountPicker();
    expect(w.find('[data-testid="staged-list"]').exists()).toBe(false);
    expect(w.find('[data-testid="tab-screen"]').exists()).toBe(true);
  });

  it("still lets a new capture be started while staged captures are listed", async () => {
    // The list is an offer, not a modal: a staged capture must not block the
    // thing the user opened this view to do.
    const calls = mockStaged([STAGED_ROW], (cmd) =>
      cmd === "start_screen_capture" ? STARTED : undefined,
    );
    const w = await mountPicker();
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
    await w.get('[data-testid="screen-start"]').trigger("click");
    await flushPromises();
    expect(calls.some((c) => c.cmd === "start_screen_capture")).toBe(true);
    expect(useVaultsStore().view).toBe("list");
  });

  it("resumes a staged capture through the one command that opens the editor", async () => {
    // `open_capture_editor` is what the capture bar's Edit button already
    // invokes. Two ways into the editor would be two things to keep in step.
    const calls = mockStaged();
    const w = await mountPicker();
    await w.get(`[data-testid="staged-resume-${STAGED_ROW.base}"]`).trigger("click");
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "open_capture_editor")).toEqual([
      { cmd: "open_capture_editor", args: { base: STAGED_ROW.base } },
    ]);
  });

  it("discards a staged capture and re-reads the list afterwards", async () => {
    const calls = mockStaged([STAGED_ROW], (cmd) =>
      cmd === "discard_staged_capture" ? null : undefined,
    );
    const w = await mountPicker();
    const base = STAGED_ROW.base;
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "discard_staged_capture")).toEqual([
      { cmd: "discard_staged_capture", args: { base } },
    ]);
    // Re-read, not a local splice: the discard also clears any abandoned
    // export temp, and only the backend knows what actually survived.
    expect(calls.filter((c) => c.cmd === "list_staged_captures")).toHaveLength(2);
  });

  it("surfaces a refused discard and keeps the capture listed", async () => {
    // `discard_staged_capture` really does refuse — a base being exported
    // right now, or one that is not ours. The message is user-facing, and a
    // row silently vanishing from the list would claim a delete that did not
    // happen.
    const calls = mockStaged([STAGED_ROW], (cmd) => {
      if (cmd === "discard_staged_capture") {
        throw new Error("That capture is being saved right now.");
      }
      return undefined;
    });
    const w = await mountPicker();
    const base = STAGED_ROW.base;
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="screen-error"]').text()).toContain("being saved right now");
    expect(w.find(`[data-testid="staged-row-${base}"]`).exists()).toBe(true);
    // Nothing to re-read: the list on screen is still true.
    expect(calls.filter((c) => c.cmd === "list_staged_captures")).toHaveLength(1);
  });

  // FIX: and BECAUSE nothing is re-read, `captures` keeps its identity, so
  // the list's disarm-on-relist watch never fires — the refused row stayed
  // armed and the user's next SINGLE click destroyed the recording with no
  // second confirm. An armed destructive control surviving a refusal is the
  // trap the two-click gate exists to prevent.
  it("disarms the refused row, so the next single click cannot delete it", async () => {
    const calls = mockStaged([STAGED_ROW], (cmd) => {
      if (cmd === "discard_staged_capture") {
        throw new Error("That capture is being saved right now.");
      }
      return undefined;
    });
    const w = await mountPicker();
    const base = STAGED_ROW.base;
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await flushPromises();
    // Back to a one-click ARM, not a one-click delete.
    expect(w.find(`[data-testid="staged-keep-${base}"]`).exists()).toBe(false);
    await w.get(`[data-testid="staged-discard-${base}"]`).trigger("click");
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "discard_staged_capture")).toHaveLength(1);
    expect(w.find(`[data-testid="staged-keep-${base}"]`).exists()).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// GAP-144: the export's hard dependency, surfaced BEFORE the recording.
//
// ffmpeg is needed only by the final Save into a vault. Recording and editing
// work perfectly without it, so this pre-flight WARNS and must never block:
// disabling Start (the Pandoc-import precedent) would take away working
// functionality and be a worse bug than the late discovery it replaces.
// ---------------------------------------------------------------------------
describe("ScreenSourcePicker — the ffmpeg pre-flight", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  const FFMPEG_FOUND = {
    installed: true,
    version: "ffmpeg version 6.1.1-3ubuntu5 Copyright (c)",
    path: "/usr/bin/ffmpeg",
    ffprobePath: "/usr/bin/ffprobe",
    h264Encoder: "libx264",
    configuredPath: null,
  };
  const FFMPEG_MISSING = {
    installed: false,
    version: null,
    path: null,
    ffprobePath: null,
    h264Encoder: null,
    configuredPath: null,
  };

  function mockWithFfmpeg(ffmpeg: unknown) {
    mockIPC((cmd) => {
      if (cmd === "list_capture_sources") return SOURCES;
      if (cmd === "list_audio_devices") return NO_DEVICES;
      if (cmd === "list_staged_captures") return [];
      if (cmd === "detect_ffmpeg") return ffmpeg;
      return undefined;
    });
  }

  it("says nothing when ffmpeg is installed", async () => {
    mockWithFfmpeg(FFMPEG_FOUND);
    const w = await mountPicker();
    expect(w.find('[data-testid="ffmpeg-preflight"]').exists()).toBe(false);
  });

  it("warns when ffmpeg is absent, naming both what works and what will not", async () => {
    mockWithFfmpeg(FFMPEG_MISSING);
    const w = await mountPicker();
    const notice = w.get('[data-testid="ffmpeg-preflight"]').text();
    // Both halves have to be true, or the notice is either a false alarm
    // ("you cannot record") or a useless one ("something is missing").
    expect(notice).toContain("record and edit");
    expect(notice).toContain("saving it into a vault");
    expect(notice).toContain("ffmpeg");
  });

  // REGRESSION (the design decision this whole pre-flight turns on): ffmpeg
  // is needed only by the export. A picker that disabled Start — or hid it
  // behind a setup route, the way a blocked document Import does — would take
  // away a recording the user can perfectly well make and edit, which is a
  // worse bug than discovering the dependency at Save.
  it("leaves Start enabled whether or not ffmpeg is installed", async () => {
    mockWithFfmpeg(FFMPEG_MISSING);
    const w = await mountPicker();
    await w.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w.find('[data-testid="ffmpeg-preflight"]').exists()).toBe(true);
    expect(w.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();

    // And the found case is not accidentally the only enabled one.
    clearMocks();
    setActivePinia(createPinia());
    mockWithFfmpeg(FFMPEG_FOUND);
    const w2 = await mountPicker();
    await w2.get('[data-testid="source-screen:1"]').trigger("click");
    expect(w2.get('[data-testid="screen-start"]').attributes("disabled")).toBeUndefined();
  });

  it("routes to the ffmpeg settings card, so the export's own refusal names a real screen", async () => {
    mockWithFfmpeg(FFMPEG_MISSING);
    const w = await mountPicker();
    await w.get('[data-testid="ffmpeg-preflight-settings"]').trigger("click");
    const store = useVaultsStore();
    expect(store.view).toBe("settings");
    // …and on the tab the card actually lives on. Landing on the Buddy tab is
    // exactly GAP-144's failure scenario: the user follows a message into
    // Buddy settings, finds nothing, and concludes the app is broken.
    expect(store.settingsTab).toBe("integrations");
  });
});
