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
