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
 * - `media`/`tracks`/`audio`: there is no sample project;
 * - `select`: there is no Clear selection item and no V shortcut;
 * - `split`: the delete modes are "Leave gap" and "Close gap on this
 *   track", not "Ripple this track";
 * - `context`: only a clip has a right-click menu;
 * - `tracks`: there is no Add track menu — the lesson points at the top
 *   track's own menu, and a track is added by dropping media below the
 *   last one;
 * - `fades`: there are no fade presets;
 * - `chapters`: there is no companion-note preview;
 * - `save`: Save project stores the project here, there is no download,
 *   and recovery is the native journal;
 * - `render`: an ffmpeg render has no three-minute limit, lands in the
 *   project's Products, and Publish DOES write a copy into a vault;
 * - `products`: there is no Project menu — products are the library's
 *   Products tab;
 * `help` carried an override until Task 57 shipped the learning center its
 * verbatim text describes (Help's menu, chapter jumps, quick answers,
 * shortcuts); it is the concept text again.
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
  context: {
    body: "Right-click a clip for the actions that apply to it. Edit actions in the timeline opens the same menu for the selected clips without a right click.",
  },
  tracks: {
    label: "Track menu",
    tip: "Adding a track is optional: dropping media below the last track makes a new one.",
    task: "Open the top track's menu to see what it offers. You do not need to change anything.",
  },
  fades: {
    body: "Set Fade in and Fade out, or drag the gold handles on a timeline clip. Video fades reveal the layer beneath; audio fades change volume.",
  },
  audio: {
    tip: "Listen to the actual rendered file before sharing.",
  },
  chapters: {
    task: "Look through the chapter list. Adding a chapter is optional.",
  },
  save: {
    tip: "Save project stores the editable project in Vault Buddy on this computer, and unsaved edits are kept for recovery if the editor closes. A portable project file may include uncensored originals.",
    task: "Open the menu beside Save project to see the file formats. Saving is optional.",
  },
  render: {
    tip: "Rendering runs on this computer with your installed ffmpeg and adds the video to this project's Products. Publishing a product copies it, with an optional companion note, into a vault. Keep the editable project for later changes.",
  },
  products: {
    label: "Rendered products",
    body: "Rendered videos are listed in the library's Products tab, separate from the editable project. Watch one, or restore the edit snapshot behind an earlier output.",
    task: "Look through Products, then continue.",
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
