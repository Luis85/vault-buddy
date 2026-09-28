import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { useFfmpegStore } from "../src/stores/ffmpeg";

const NOT_INSTALLED = {
  installed: false,
  version: null,
  path: null,
  ffprobePath: null,
  h264Encoder: null,
  configuredPath: null,
};

const installed = () => ({
  installed: true,
  version: "ffmpeg version 6.1.1-3ubuntu5 Copyright (c)",
  path: "/usr/bin/ffmpeg",
  ffprobePath: "/usr/bin/ffprobe",
  h264Encoder: "libx264",
  configuredPath: null,
});

describe("useFfmpegStore", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  it("probes once and caches when ffmpeg is installed", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") {
        calls += 1;
        return installed();
      }
    });
    const store = useFfmpegStore();
    await store.ensureDetected();
    expect(store.status?.installed).toBe(true);
    // Found → a second ensureDetected must NOT re-spawn `ffmpeg -version`.
    // Failure mode this pins: the Record Screen pre-flight consults the store
    // on every open, so without the short-circuit each visit pays a subprocess
    // spawn on the blocking pool (GAP-144's whole reason for a cached store).
    await store.ensureDetected();
    expect(calls).toBe(1);
  });

  it("re-probes while ffmpeg is not installed", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") {
        calls += 1;
        return NOT_INSTALLED;
      }
    });
    const store = useFfmpegStore();
    await store.ensureDetected();
    await store.ensureDetected();
    // Not cached — a user who installs ffmpeg after seeing the notice must be
    // picked up on the next open, with no app restart.
    expect(calls).toBe(2);
  });

  it("caches a build with no H.264 encoder, because it can still remux", async () => {
    // Deliberately DIFFERENT from the Pandoc store's "too old → keep probing"
    // rule: an ffmpeg with no H.264 encoder is not a degraded install, it is a
    // working export path for an untouched capture. Treating it as a miss would
    // re-spawn a subprocess on every open forever.
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") {
        calls += 1;
        return { ...installed(), h264Encoder: null };
      }
    });
    const store = useFfmpegStore();
    await store.ensureDetected();
    await store.ensureDetected();
    expect(calls).toBe(1);
    expect(store.status?.h264Encoder).toBeNull();
  });

  it("degrades to null and does not throw when the probe fails", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") throw new Error("io error");
    });
    const store = useFfmpegStore();
    await store.ensureDetected();
    expect(store.status).toBeNull();
    expect(store.checking).toBe(false);
  });

  it("markDetected caches a settings-side status without probing", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") {
        calls += 1;
        return installed();
      }
    });
    const store = useFfmpegStore();
    const token = store.beginProbe();
    store.markDetected(installed(), token);
    expect(store.status?.installed).toBe(true);
    // The written-through status counts as "found", so ensureDetected skips —
    // a fix made in the settings card is reflected in the intake surface.
    await store.ensureDetected();
    expect(calls).toBe(0);
  });

  it("a stale settings probe never clobbers a newer intake result", async () => {
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") return installed();
    });
    const store = useFfmpegStore();
    const settingsToken = store.beginProbe(); // settings probe starts (older)
    await store.ensureDetected(); // intake probe resolves good (newer)
    expect(store.status?.installed).toBe(true);
    store.markDetected(NOT_INSTALLED, settingsToken); // resolves last, stale
    expect(store.status?.installed).toBe(true);
  });

  it("only the latest probe clears the checking gate", async () => {
    const resolvers: Array<(v: unknown) => void> = [];
    mockIPC((cmd) => {
      if (cmd === "detect_ffmpeg") {
        return new Promise((r) => resolvers.push(r));
      }
    });
    const store = useFfmpegStore();
    const p1 = store.ensureDetected();
    const p2 = store.ensureDetected(); // status still null → probes again
    expect(store.checking).toBe(true);
    resolvers[0](NOT_INSTALLED); // older probe resolves first
    await p1;
    expect(store.checking).toBe(true); // newer still pending → gate stays
    resolvers[1](installed());
    await p2;
    expect(store.checking).toBe(false);
    expect(store.status?.installed).toBe(true);
  });
});
