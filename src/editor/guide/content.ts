/**
 * The guide's shipped content (Task 55; F-46; ADR R18): 22 lessons in
 * seven chapters, read from `steps.json`/`chapters.json` — VERBATIM copies
 * of the concept bundle's `onboarding.steps.json`/`onboarding.chapters.json`
 * (`tests/editorGuideContent.test.ts` compares the bytes). Rust compiles
 * the same `steps.json` in (`core::editor::guide`), so the lessons a saved
 * progress file may name are exactly these, with no second list.
 *
 * Stable step ids, never array indices, identify progress. A lesson that is
 * later removed goes into `retired-steps.json` (`{ "<old id>": "<chapter>" }`,
 * also read by Rust), so saved progress resumes at that chapter's first
 * lesson instead of silently jumping elsewhere; bump `CONTENT_REVISION` in
 * the same change.
 *
 * Each lesson's reference `target` selector is replaced by a typed key
 * (`STEP_TARGETS`) that the owning component registers (`targets.ts`).
 */
import chapters from "./chapters.json";
import retired from "./retired-steps.json";
import steps from "./steps.json";
import type { GuideTargetKey } from "./targets";

/** The revision of this content a saved progress file was made with. */
export const CONTENT_REVISION = 1;

export type GuideStepId =
  | "welcome" | "media" | "preview" | "timeline"
  | "select" | "split" | "undo" | "arrange" | "context"
  | "tracks" | "webcam" | "layout"
  | "fades" | "audio"
  | "callouts" | "captions" | "chapters"
  | "checks" | "save" | "render"
  | "products" | "help";

type GuideChapterId = "orient" | "edit" | "layers" | "polish" | "teach" | "finish" | "return";

/** One lesson, as the concept bundle writes it. `target`/`focus`/`action`
 * are the reference implementation's selectors: a behaviour map, never
 * queried — the guide resolves `STEP_TARGETS[id]` instead. */
interface GuideStep {
  id: GuideStepId;
  chapter: GuideChapterId;
  title: string;
  target: string;
  focus: string;
  label: string;
  body: string;
  tip: string;
  task?: string;
  action?: string;
  prepare?: string;
  keys?: string[];
  warning?: boolean;
  context?: boolean;
  visual?: string;
}

interface GuideChapter {
  id: GuideChapterId;
  title: string;
  subtitle: string;
  icon: string;
}

export const GUIDE_STEPS = steps as readonly GuideStep[];
export const GUIDE_CHAPTERS = chapters as readonly GuideChapter[];

/** ADR R18: each lesson's typed target, in lesson order. */
export const STEP_TARGETS: Readonly<Record<GuideStepId, GuideTargetKey>> = {
  welcome: "projectbar",
  media: "library.import",
  preview: "transport",
  timeline: "timeline.toolbar",
  select: "clip.selected",
  split: "timeline.split",
  undo: "timeline.undo",
  arrange: "inspector",
  context: "timeline.more",
  tracks: "track.menu",
  webcam: "library.webcam",
  layout: "inspector.layout",
  fades: "inspector.fades",
  audio: "mixer",
  callouts: "preview.toolstrip",
  captions: "library.captions",
  chapters: "library.chapters",
  checks: "header.checks",
  save: "header.save",
  render: "header.render",
  products: "library.products",
  help: "header.help",
};

/** Retired lesson id → the chapter it belonged to (empty today). */
export const RETIRED_STEP_MAP: Readonly<Record<string, string>> = retired;

export function isGuideStepId(id: string): id is GuideStepId {
  return GUIDE_STEPS.some((s) => s.id === id);
}

/** Where a saved step id resumes: itself when it is a lesson; a retired
 * id's chapter's first lesson; anything else the guide's first lesson.
 * `core::editor::guide::resolve_step_id` is the same rule on the Rust side,
 * over the same two files. */
export function resolveStepId(id: string, retiredMap: Readonly<Record<string, string>> = RETIRED_STEP_MAP): GuideStepId {
  if (isGuideStepId(id)) return id;
  const chapter = Object.prototype.hasOwnProperty.call(retiredMap, id) ? retiredMap[id] : undefined;
  const first = GUIDE_STEPS.find((s) => s.chapter === chapter) ?? GUIDE_STEPS[0];
  return first.id;
}

/** The four sentences of a lesson the coach shows. */
export interface LessonCopy {
  label: string;
  body: string;
  tip: string;
  task: string | null;
}

type CopyOverride = Partial<Record<"label" | "body" | "tip" | "task", string>>;

/**
 * Native corrections to the verbatim lesson text (Task 56; docs/Gaps.md
 * GAP-203). `steps.json` stays byte-identical to the concept bundle, which
 * was written for the BROWSER reference, so a sentence it gets wrong about
 * this app is replaced here, by lesson id, and nowhere else — the coach
 * renders `lessonCopy(id)`, never a raw step. Each entry replaces exactly
 * the sentence that was untrue:
 * - `media`/`tracks`/`audio`: there is no sample project (for `tracks`
 *   that is the whole correction: since visual-parity Task 16 the ruler's
 *   Add track is the menu the lesson describes, and its `track.menu`
 *   target — Task 17 removed the header's ⋮ it pointed at before);
 * - `select`: there is no Clear selection item and no V shortcut;
 * - `split`: the delete modes are "Leave gap" and "Close gap on this
 *   track", not "Ripple this track";
 * - `chapters`: there is no companion-note preview;
 * - `save`: the lesson highlights the header's Save project, which stores
 *   the project in Vault Buddy (no download, no portable file — that is
 *   the Project menu's "Save a copy as project file…", visual-parity Task
 *   8), and recovery is the native journal; since visual-parity Task 23
 *   the body and task describe that button, the control the ring is on;
 * - `render`: an ffmpeg render has no three-minute limit, lands in the
 *   project's Products, and Publish DOES write a copy into a vault;
 * - `products`: the Project menu has no "workspace files" list — its
 *   "Workspace & rendered products" opens the library's Project section
 *   (visual-parity Task 10, design D9), where the rendered products live;
 * `help` carried an override until Task 57 shipped the learning center its
 * verbatim text describes (Help's menu, chapter jumps, quick answers,
 * shortcuts); it is the concept text again. So is `context` since
 * visual-parity Task 23: tracks (Task 17), teaching cues (Task 19) and
 * media rows (Task 9) have right-click menus now, as its text says.
 * `tests/editorGuideContent.test.ts` pins every entry whole.
 */
export const LESSON_COPY_OVERRIDES: Readonly<Partial<Record<GuideStepId, CopyOverride>>> = {
  media: {
    tip: "Imported files are copied into this project; the file you pick is never changed. Importing is optional: the whole guide works with the recording already open.",
    task: "Open the import picker if you want to add a file, or continue.",
  },
  select: {
    tip: "Selection is not an edit: choosing which clip is selected never changes the video.",
  },
  split: {
    tip: "Leave gap keeps other clips in place. Close gap on this track closes the gap on this track only, which can change its alignment with other tracks.",
  },
  tracks: {
    tip: "Adding a track is optional: dropping media below the last track makes a new one too.",
  },
  audio: {
    tip: "Listen to the actual rendered file before sharing.",
  },
  chapters: {
    task: "Look through the chapter list. Adding a chapter is optional.",
  },
  save: {
    body: "Save project keeps the editable workspace without rendering: the timeline, teaching layers and render history are stored in Vault Buddy on this computer, so you can continue later.",
    tip: "Unsaved edits are kept for recovery if the editor closes. To move or back up a project, use Project → Save a copy as project file…; a portable copy may include uncensored originals.",
    task: "Choose Save project, or press Ctrl+S, when you want to store your changes. Saving is optional.",
  },
  render: {
    tip: "Rendering runs on this computer with your installed ffmpeg and adds the video to this project's Products. Publishing a product copies it, with an optional companion note, into a vault. Keep the editable project for later changes.",
  },
  products: {
    label: "Rendered products",
    body: "Rendered videos are listed under Project → Workspace & rendered products, beside the editable project. Watch one, or restore the edit snapshot behind an earlier output.",
    task: "Look through the rendered products, then continue.",
  },
};

/** What the coach shows for lesson `id`: the verbatim text with this
 * app's corrections applied. */
export function lessonCopy(id: GuideStepId): LessonCopy {
  const step = GUIDE_STEPS.find((s) => s.id === id) ?? GUIDE_STEPS[0];
  const over = LESSON_COPY_OVERRIDES[id] ?? {};
  return {
    label: over.label ?? step.label,
    body: over.body ?? step.body,
    tip: over.tip ?? step.tip,
    task: over.task ?? step.task ?? null,
  };
}
