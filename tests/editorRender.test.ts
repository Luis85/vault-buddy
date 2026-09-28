/**
 * The render and product IPC (Task 46): `editor_start_render`,
 * `editor_get_products`, `editor_restore_product`. The decoders are held to
 * the SAME literals `render_jobs_tests.rs`' `render_wire_shapes_are_pinned`
 * pins on the Rust side, and each port method is driven through `mockIPC`
 * so the argument names are the Rust parameters, camelCased.
 */
import { Channel } from "@tauri-apps/api/core";
import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it } from "vitest";

import { ProtocolError } from "../src/editor/decode";
import { decodeProducts, decodeRenderStarted } from "../src/editor/decodeRender";
import { createTauriEditorPort } from "../src/editor/port";
import type { JobProgressDto, RenderRequest } from "../src/editorTypes";

afterEach(() => clearMocks());

// `render_jobs_tests.rs`' ProductDto literal, verbatim.
const PRODUCT = {
  id: "prod-a",
  projectId: "proj1",
  name: "Walkthrough",
  filename: "prod-a.mp4",
  mime: "video/mp4",
  revision: 3,
  durationMs: 1_250,
  createdAt: "2026-09-24T10:00:00+02:00",
  editFingerprint: "sha256:ab",
  renderRange: { startMs: 250, endMs: 1_500 },
  available: true,
};

const PROJECTION = {
  snapshot: {
    sessionId: "ses-1",
    projectId: "project",
    revision: 5,
    persistedRevision: 3,
    title: "Frozen",
    durationMs: 0,
    canUndo: true,
    canRedo: false,
    undoLabel: "Restore render",
    redoLabel: null,
  },
  project: {
    schema: "vault-buddy-video-project/3",
    id: "project",
    title: "Frozen",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1.0,
    assets: [],
    tracks: [],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    destination: { vault: "", folder: "", dated: false },
  },
};

describe("render decoders", () => {
  it("decodes the render's immediate { jobId, revision }", () => {
    expect(decodeRenderStarted({ jobId: "job-a", revision: 7 })).toEqual({ jobId: "job-a", revision: 7 });
    expect(() => decodeRenderStarted({ jobId: "job-a" })).toThrow(ProtocolError);
  });

  it("decodes products, with a null range and an unavailable file", () => {
    const whole = { ...PRODUCT, renderRange: null, available: false };
    expect(decodeProducts([PRODUCT, whole])).toEqual([PRODUCT, whole]);
  });

  it("refuses a product without `available`, or with a range ending before it starts", () => {
    const { available: _, ...noAvailable } = PRODUCT;
    expect(() => decodeProducts([noAvailable])).toThrow(ProtocolError);
    expect(() => decodeProducts([{ ...PRODUCT, renderRange: { startMs: 9, endMs: 4 } }])).toThrow(ProtocolError);
    expect(() => decodeProducts({ products: [] })).toThrow(ProtocolError);
  });
});

describe("render port methods", () => {
  it("startRender sends { request, onProgress: Channel } and delivers decoded progress", async () => {
    let channel: Channel<unknown> | null = null;
    const request: RenderRequest = {
      sessionId: "ses-1",
      expectedRevision: 4,
      name: "Walkthrough",
      range: null,
      quality: "balanced",
    };
    mockIPC((cmd, args) => {
      if (cmd !== "editor_start_render") throw new Error(`unexpected command ${cmd}`);
      const a = args as { request: RenderRequest; onProgress: Channel<unknown> };
      expect(a.request).toEqual(request);
      expect(a.onProgress).toBeInstanceOf(Channel);
      channel = a.onProgress;
      return { jobId: "job-r", revision: 4 };
    });
    const received: JobProgressDto[] = [];
    const port = createTauriEditorPort();
    await expect(port.startRender(request, (m) => received.push(m))).resolves.toEqual({
      jobId: "job-r",
      revision: 4,
    });
    const rendering = {
      sessionId: "ses-1", jobId: "job-r", kind: "render", sequence: 2,
      phase: "rendering", fraction: 0.5, terminal: null,
    };
    channel!.onmessage(rendering);
    expect(received).toEqual([rendering]);
  });

  // Task 47 (F18): a Review is the same command with `review: true`
  // (Rust's `#[serde(default)] review`); a product render omits the key.
  it("startRender passes a review request's flag through unchanged", async () => {
    const seen: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd !== "editor_start_render") throw new Error(`unexpected command ${cmd}`);
      seen.push((args as { request: unknown }).request);
      return { jobId: "job-v", revision: 2 };
    });
    const review: RenderRequest = {
      sessionId: "ses-1",
      expectedRevision: 2,
      name: "Review",
      range: { startMs: 0, endMs: 5_000 },
      quality: "balanced",
      review: true,
    };
    await createTauriEditorPort().startRender(review, () => {});
    expect(seen).toEqual([review]);
  });

  it("getProducts and restoreProduct send camelCased arguments and decode the replies", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "editor_get_products") return [PRODUCT];
      if (cmd === "editor_restore_product") return PROJECTION;
      throw new Error(`unexpected command ${cmd}`);
    });
    const port = createTauriEditorPort();
    await expect(port.getProducts("ses-1")).resolves.toEqual([PRODUCT]);
    const restored = await port.restoreProduct("ses-1", 4, "prod-a", "cmd-1");
    expect(restored.snapshot.revision).toBe(5);
    expect(calls).toEqual([
      { cmd: "editor_get_products", args: { sessionId: "ses-1" } },
      {
        cmd: "editor_restore_product",
        args: { sessionId: "ses-1", expectedRevision: 4, productId: "prod-a", commandId: "cmd-1" },
      },
    ]);
  });
});
