import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const openDialog = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: openDialog }));

import FfmpegSettings from "../src/components/FfmpegSettings.vue";
import { useFfmpegStore } from "../src/stores/ffmpeg";

const NOT_INSTALLED = {
  installed: false,
  version: null,
  path: null,
  ffprobePath: null,
  h264Encoder: null,
  configuredPath: null,
};

const INSTALLED = {
  installed: true,
  version: "ffmpeg version 6.1.1-3ubuntu5 Copyright (c)",
  path: "/usr/bin/ffmpeg",
  ffprobePath: "/usr/bin/ffprobe",
  h264Encoder: "libx264",
  configuredPath: null,
};

/** Mount the card with `detect_ffmpeg` answering `status`, recording every
 * `set_ffmpeg_path` argument so a write-through can be asserted by value. */
async function mountCard(status: unknown = INSTALLED) {
  const setCalls: unknown[] = [];
  let detectCalls = 0;
  mockIPC((cmd, args) => {
    if (cmd === "detect_ffmpeg") {
      detectCalls += 1;
      return status;
    }
    if (cmd === "set_ffmpeg_path") {
      setCalls.push(args);
      return null;
    }
    if (cmd === "open_external_url") return null;
    return undefined;
  });
  const w = mount(FfmpegSettings);
  await flushPromises();
  return { w, setCalls, detects: () => detectCalls };
}

describe("FfmpegSettings", () => {
  beforeEach(() => {
    setActivePinia(createPinia());
    openDialog.mockReset();
  });
  afterEach(() => clearMocks());

  it("renders a found ffmpeg with its version banner", async () => {
    const { w } = await mountCard();
    const label = w.get('[data-testid="ffmpeg-status"]').text();
    expect(label).toContain("Installed");
    expect(label).toContain("6.1.1");
  });

  it("renders a not-installed ffmpeg, so the export's dependency is visible before a recording", async () => {
    const { w } = await mountCard(NOT_INSTALLED);
    expect(w.get('[data-testid="ffmpeg-status"]').text()).toContain("Not installed");
  });

  it("reports a build that cannot encode H.264 rather than calling it simply installed", async () => {
    // A minimal LGPL build remuxes an untouched capture but cannot save an
    // EDITED one. Reporting it as plain "Installed" would send the user back
    // to the same late failure GAP-144 exists to prevent.
    const { w } = await mountCard({ ...INSTALLED, h264Encoder: null });
    const label = w.get('[data-testid="ffmpeg-status"]').text();
    expect(label).toContain("Installed");
    expect(label).toContain("H.264");
  });

  it("Browse writes the picked path through set_ffmpeg_path and re-detects", async () => {
    openDialog.mockResolvedValue("C:/tools/ffmpeg.exe");
    const { w, setCalls, detects } = await mountCard(NOT_INSTALLED);
    const before = detects();
    await w.get('[data-testid="ffmpeg-browse"]').trigger("click");
    await flushPromises();
    expect(setCalls).toEqual([{ ffmpegPath: "C:/tools/ffmpeg.exe" }]);
    // The card must not keep showing the pre-save status.
    expect(detects()).toBe(before + 1);
    expect((w.get('[data-testid="ffmpeg-path-input"]').element as HTMLInputElement).value)
      .toBe("C:/tools/ffmpeg.exe");
  });

  it("a cancelled Browse writes nothing", async () => {
    openDialog.mockResolvedValue(null);
    const { w, setCalls } = await mountCard(NOT_INSTALLED);
    await w.get('[data-testid="ffmpeg-browse"]').trigger("click");
    await flushPromises();
    expect(setCalls).toEqual([]);
  });

  it("clearing the override field persists null, not an empty string", async () => {
    const { w, setCalls } = await mountCard({ ...INSTALLED, configuredPath: "C:/old/ffmpeg.exe" });
    const input = w.get('[data-testid="ffmpeg-path-input"]');
    await input.setValue("   ");
    await input.trigger("change");
    await flushPromises();
    expect(setCalls).toEqual([{ ffmpegPath: null }]);
  });

  it("Recheck re-probes", async () => {
    const { w, detects } = await mountCard(NOT_INSTALLED);
    const before = detects();
    await w.get('[data-testid="ffmpeg-recheck"]').trigger("click");
    await flushPromises();
    expect(detects()).toBe(before + 1);
  });

  it("writes its probe through to the shared store, so the intake surface sees the fix", async () => {
    await mountCard();
    expect(useFfmpegStore().status?.installed).toBe(true);
  });

  it("stays rendered when detection itself fails, so Recheck and the override survive", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") throw new Error("spawn exploded");
    });
    const w = mount(FfmpegSettings);
    await flushPromises();
    expect(w.get('[data-testid="ffmpeg-error"]').text()).toContain("spawn exploded");
    expect(w.find('[data-testid="ffmpeg-recheck"]').exists()).toBe(true);
    expect(w.find('[data-testid="ffmpeg-path-input"]').exists()).toBe(true);
  });
});
