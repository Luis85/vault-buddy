import { vi } from "vitest";

import type { EditorPort } from "../../src/editor/port";
import type {
  Clip,
  EditorOpenResult,
  JobProgressDto,
  ProductDto,
  Project,
  RenderRequest,
} from "../../src/editorTypes";
import { useEditorProjectStore } from "../../src/stores/editorProject";
import { fakeEditorPort } from "./fakeEditorPort";

/**
 * The render/product suites' shared fixture (Task 47): one open session
 * over a 12.5 s project, and a fake `startRender` that hands every started
 * job's Channel callback back to the test, so progress can be delivered in
 * any order. Asymmetric on purpose: the two clips sit at different times
 * with different lengths, so a range that mixes up starts and ends fails.
 */
export const SESSION = "ses-r";

function clip(id: string, startMs: number, inMs: number, outMs: number): Clip {
  return {
    id,
    asset_id: "cap",
    track_id: "v1",
    name: id,
    start_ms: startMs,
    in_ms: inMs,
    out_ms: outMs,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0,
    y: 0,
    w: 1,
    h: 1,
  };
}

export function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-r",
    title: "Walkthrough",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks: [
      { id: "v1", kind: "video", name: "V1", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    ],
    // c1 plays output [1500, 4000); c2 plays [6000, 12500) at speed 2.
    clips: [clip("c1", 1_500, 0, 2_500), { ...clip("c2", 6_000, 1_000, 14_000), speed: 2 }],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "", folder: "", dated: false },
  };
}

export function openResult(revision = 7): EditorOpenResult {
  return {
    snapshot: {
      sessionId: SESSION,
      projectId: "project-r",
      revision,
      persistedRevision: revision,
      title: "Walkthrough",
      durationMs: 12_500,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: project(),
    workspace: {},
    missing: [],
    sourceBase: "base",
    recovered: false,
  };
}

export function product(overrides: Partial<ProductDto> = {}): ProductDto {
  return {
    id: "prod-a",
    projectId: "project-r",
    name: "Walkthrough v1",
    filename: "prod-a.mp4",
    mime: "video/mp4",
    revision: 5,
    durationMs: 12_500,
    createdAt: "2026-09-24T10:00:00+02:00",
    editFingerprint: "sha256:ab",
    renderRange: null,
    available: true,
    ...overrides,
  };
}

export function progress(jobId: string, overrides: Partial<JobProgressDto> = {}): JobProgressDto {
  return {
    sessionId: SESSION,
    jobId,
    kind: "render",
    sequence: 1,
    phase: "rendering",
    fraction: 0,
    terminal: null,
    ...overrides,
  };
}

/** Opens `SESSION` over `port` extras plus a recording `startRender`:
 * `requests` lists every request sent, `deliver(i, m)` sends `m` on the
 * i-th started job's Channel. Job ids are `job-0`, `job-1`, … */
export async function openWithRenders(extra: Partial<EditorPort> = {}) {
  const requests: RenderRequest[] = [];
  const channels: ((m: JobProgressDto) => void)[] = [];
  const startRender = vi.fn((request: RenderRequest, onProgress: (m: JobProgressDto) => void) => {
    requests.push(request);
    channels.push(onProgress);
    return Promise.resolve({ jobId: `job-${channels.length - 1}`, revision: request.expectedRevision });
  });
  const port = fakeEditorPort({
    openStaged: () => Promise.resolve(openResult()),
    // Task 54: the Render dialog reads the checks; none unless a suite says.
    getChecks: () => Promise.resolve([]),
    startRender,
    ...extra,
  });
  const editorProject = useEditorProjectStore();
  editorProject.setPort(port);
  await editorProject.openStaged("base");
  return {
    port,
    startRender,
    requests,
    deliver: (i: number, m: JobProgressDto) => channels[i](m),
  };
}
