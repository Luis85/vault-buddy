/**
 * The concept-parity harness's sample project (Task 1 of the tutorial
 * editor visual-parity effort): a populated `EditorOpenResult` mirroring
 * concept screen `docs/concepts/vault-buddy-editor/screens/02-workspace.png`
 * — a mid-edit "Create your first project" tutorial with three screen-
 * recording clips, a webcam presenter overlay, a detail overlay, a music
 * bed, teaching cues, chapter markers, a transition and captions.
 *
 * Every shape below is checked against the decoders in
 * `src/editor/decodeProject.ts` / `src/editor/decode.ts` field for field —
 * this is a decode-valid `EditorOpenResult`, not a loose approximation, so
 * every later task's e2e spec can build real UI state on top of it rather
 * than re-deriving its own fixture.
 *
 * **Teaching cues and markers are stored in each clip's own SOURCE time**
 * (`decodeProject.ts`'s module doc: "`start_ms`/`end_ms` are SOURCE time,
 * like every clip-linked cue"). The brief gives cue times as they read on
 * the OUTPUT timeline (the numbers a viewer scrubbing the whole project
 * would see), so each cue below is "attached" to whichever clip's output
 * span `[start_ms, start_ms + duration)` contains it, then converted to
 * that clip's source time. Every clip here has `in_ms: 0` and no `speed`
 * (so `speed` defaults to 1 — `core::editor::time`'s own default), which
 * makes the conversion a plain subtraction: `source_ms = output_ms -
 * clip.start_ms` (the general case is `outputAt`/`sourceAt` in
 * `src/editor/timeMap.ts`; this fixture is simple enough not to need the
 * import). A cue landing exactly on a clip boundary (e.g. `9500`, where c1
 * ends and c2 begins) resolves to the clip whose span is the half-open
 * INCLUDING end — `[start, end)` — so `9500` belongs to c2, not c1, exactly
 * as `clipIsActive` in `timeMap.ts` would decide it.
 */
import type { EditorOpenResult, ProductDto, ProjectSummaryDto } from "../../../src/editorTypes";

// ---- clip layout (all in_ms: 0, speed: 1 -- see the module doc above) -----

const C1_START = 0;
const C1_LEN = 9_500; // "Open your workspace"
const C2_START = C1_START + C1_LEN; // 9,500
const C2_LEN = 14_000; // "Create a project"
const C3_START = C2_START + C2_LEN; // 23,500
const C3_LEN = 10_000; // "Save to your vault"
const C3_END = C3_START + C3_LEN; // 33,500 -- the project's total duration

/** Converts an output-time instant to `clipId`'s own source time -- the
 * caller has already picked the clip; this only does the "output ms -> that
 * clip's source ms" half. The fixture's own tiny version of `sourceAt`,
 * valid because every clip here has `in_ms: 0` and `speed: 1` (see the
 * module doc). */
function attach(outputMs: number, clipId: string, clipStartMs: number): { clip_id: string; ms: number } {
  return { clip_id: clipId, ms: outputMs - clipStartMs };
}

// ---- assets -----------------------------------------------------------

const assets: EditorOpenResult["project"]["assets"] = [
  { id: "capture", kind: "video", name: "Getting started.capture", duration_ms: 36_000 },
  { id: "presenter", kind: "video", name: "Presenter · demo", duration_ms: 33_000 },
  { id: "detail", kind: "video", name: "Project detail.capture", duration_ms: 18_000 },
  { id: "music", kind: "audio", name: "Guide cues · synth", duration_ms: 36_000 },
  { id: "bed", kind: "audio", name: "Ambient bed · synth", duration_ms: 36_000 },
];

// ---- tracks, top to bottom ---------------------------------------------

function track(id: string, kind: "audio" | "video", name: string): EditorOpenResult["project"]["tracks"][number] {
  return { id, kind, name, visible: true, locked: false, muted: false, solo: false, volume: 1 };
}

const tracks: EditorOpenResult["project"]["tracks"] = [
  track("v3", "video", "Webcam · presenter"),
  track("v2", "video", "Detail overlay"),
  track("v1", "video", "Screen recording"),
  track("a1", "audio", "Guide cues"),
];

// ---- clips --------------------------------------------------------------

function clip(
  id: string,
  assetId: string,
  trackId: string,
  startMs: number,
  lengthMs: number,
  name: string,
  extra: Partial<EditorOpenResult["project"]["clips"][number]> = {},
): EditorOpenResult["project"]["clips"][number] {
  return {
    id,
    asset_id: assetId,
    track_id: trackId,
    name,
    start_ms: startMs,
    in_ms: 0,
    out_ms: lengthMs,
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
    ...extra,
  };
}

const clips: EditorOpenResult["project"]["clips"] = [
  clip("c1", "capture", "v1", C1_START, C1_LEN, "Open your workspace"),
  clip("c2", "capture", "v1", C2_START, C2_LEN, "Create a project"),
  clip("c3", "capture", "v1", C3_START, C3_LEN, "Save to your vault"),
  clip("c4", "detail", "v2", 11_000, 7_200, "A closer look", { x: 0.1, y: 0.1, w: 0.5, h: 0.5 }),
  clip("c5", "presenter", "v3", 1_500, 32_000, "Presenter · demo", {
    x: 0.75,
    y: 0.05,
    w: 0.2,
    h: 0.2,
    fade_in_ms: 600,
    fade_out_ms: 600,
  }),
  clip("c6", "music", "a1", 300, 33_000, "Chapter cues · demo"),
];

// ---- teaching cues (effects) --------------------------------------------
// Output-time ranges per the task brief, attached to whichever clip's
// output span contains them (see the module doc above).

const t1 = attach(500, "c1", C1_START);
const t1End = attach(6_500, "c1", C1_START);
const hl = attach(2_700, "c1", C1_START);
const hlEnd = attach(8_000, "c1", C1_START);
const arrow = attach(10_000, "c2", C2_START);
const arrowEnd = attach(17_000, "c2", C2_START);
const t2 = attach(10_000, "c2", C2_START);
const t2End = attach(17_000, "c2", C2_START);
const zoom = attach(18_000, "c2", C2_START);
const zoomEnd = attach(23_000, "c2", C2_START);
const t3 = attach(24_000, "c3", C3_START);
const t3End = attach(30_000, "c3", C3_START);

const effects: EditorOpenResult["project"]["effects"] = [
  {
    id: "fx1",
    clip_id: t1.clip_id,
    kind: "text",
    start_ms: t1.ms,
    end_ms: t1End.ms,
    x: 0.5,
    y: 0.88,
    color: "#ffffff",
    text: "A little structure. A lot more clarity.",
    fontSize: 32,
  },
  {
    id: "fx2",
    clip_id: hl.clip_id,
    kind: "highlight",
    start_ms: hl.ms,
    end_ms: hlEnd.ms,
    x: 0.3,
    y: 0.35,
    w: 0.4,
    h: 0.3,
    color: "#fbbf24",
  },
  {
    id: "fx3",
    clip_id: arrow.clip_id,
    kind: "arrow",
    start_ms: arrow.ms,
    end_ms: arrowEnd.ms,
    x: 0.3,
    y: 0.6,
    x2: 0.55,
    y2: 0.4,
    stroke: 4,
    color: "#38bdf8",
  },
  {
    id: "fx4",
    clip_id: t2.clip_id,
    kind: "text",
    start_ms: t2.ms,
    end_ms: t2End.ms,
    x: 0.5,
    y: 0.88,
    color: "#ffffff",
    text: "Give your project a name. Make it yours.",
    fontSize: 32,
  },
  {
    id: "fx5",
    clip_id: zoom.clip_id,
    kind: "zoom",
    start_ms: zoom.ms,
    end_ms: zoomEnd.ms,
    x: 0.5,
    y: 0.5,
    color: "#ffffff",
    factor: 1.65,
  },
  {
    id: "fx6",
    clip_id: t3.clip_id,
    kind: "text",
    start_ms: t3.ms,
    end_ms: t3End.ms,
    x: 0.5,
    y: 0.88,
    color: "#ffffff",
    text: "Ready to find.",
    fontSize: 32,
  },
];

// ---- markers --------------------------------------------------------------
// One at each cut point on v1, attached the same half-open way as the
// effects above -- 9,500 lands on c2 (its span's inclusive start), 23,500
// on c3.

const m1 = attach(9_500, "c2", C2_START);
const m2 = attach(23_500, "c3", C3_START);

const markers: EditorOpenResult["project"]["markers"] = [
  { id: "m1", clip_id: m1.clip_id, source_ms: m1.ms, title: "Create a project" },
  { id: "m2", clip_id: m2.clip_id, source_ms: m2.ms, title: "Save to your vault" },
];

// ---- one transition, between c1 and c2 -------------------------------------

const transitions: EditorOpenResult["project"]["transitions"] = [
  { id: "tr1", from: "c1", to: "c2", duration_ms: 500, kind: "dissolve" },
];

// ---- captions on c5 ---------------------------------------------------------

const captions: EditorOpenResult["project"]["captions"] = {
  enabled: true,
  burn_in: false,
  font_size: 28,
  position: "bottom",
  background: true,
  cues: [
    { id: "cap1", clip_id: "c5", start_ms: 3_000, end_ms: 6_000, text: "Hi, I'm here to walk you through it." },
    { id: "cap2", clip_id: "c5", start_ms: 15_000, end_ms: 18_500, text: "Let's create your first project together." },
  ],
};

/** A decode-valid `EditorOpenResult` mirroring concept screen 02 — the
 * sample project every later task's parity spec opens against. Revision 3
 * against a `persistedRevision` of 1, so the project reads unsaved (an
 * enabled Save button, an undo label to show). */
export const PARITY_OPEN_RESULT: EditorOpenResult = {
  snapshot: {
    sessionId: "ses-parity",
    projectId: "project-parity",
    revision: 3,
    persistedRevision: 1,
    title: "Create your first project",
    durationMs: C3_END,
    canUndo: true,
    canRedo: false,
    undoLabel: "Split clip",
    redoLabel: null,
  },
  project: {
    schema: "vault-buddy-video-project/3",
    id: "project-parity",
    title: "Create your first project",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets,
    tracks,
    clips,
    effects,
    markers,
    transitions,
    captions,
    destination: { vault: "vault-e2e", folder: "", dated: false },
  },
  workspace: {},
  missing: [],
  sourceBase: "2026-09-21 0848 Screen Capture",
  recovered: false,
};

const product: ProductDto = {
  id: "prod1",
  projectId: "project-parity",
  name: "Create your first project",
  filename: "prod1.mp4",
  mime: "video/mp4",
  revision: 2,
  durationMs: C3_END,
  createdAt: "2026-09-25T10:00:00.000Z",
  editFingerprint: "fingerprint1",
  renderRange: null,
  available: true,
};

const projectSummaries: ProjectSummaryDto[] = [
  {
    projectFileId: "project-parity",
    title: "Create your first project",
    updatedAt: "2026-09-25T10:05:00.000Z",
    persistedRevision: 1,
    hasRecovery: false,
    sourceBase: PARITY_OPEN_RESULT.sourceBase,
  },
  {
    projectFileId: "project-onboarding",
    title: "Onboarding walkthrough",
    updatedAt: "2026-09-24T09:00:00.000Z",
    persistedRevision: 1,
    hasRecovery: false,
    sourceBase: null,
  },
];

/** Every non-project IPC reply the parity harness's opened editor needs,
 * keyed exactly as `tauriStub.ts`'s reply table expects. */
export const PARITY_REPLIES: Record<string, unknown> = {
  editor_get_guide_progress: {
    contentRevision: 1,
    currentStepId: null,
    reviewed: [],
    explored: [],
    invitationDismissed: false,
    active: false,
    collapsed: false,
    completed: false,
    preferences: { dimming: true, motion: "system" },
  },
  editor_save_guide_progress: null,
  editor_get_workspace: {},
  editor_save_workspace: null,
  editor_get_checks: [],
  editor_get_products: [product],
  editor_get_jobs: [],
  list_vaults: [{ id: "vault-e2e", name: "Knowledge vault", path: "C:/v", open: false }],
  editor_media_url: "C:/fixture/capture.webm",
  editor_media_thumbnail: "C:/fixture/thumb.jpg",
  editor_media_peaks: { peaks: Array.from({ length: 400 }, (_, i) => Math.abs(Math.sin(i / 7)) * 0.6) },
  editor_list_projects: projectSummaries,
};
