/**
 * A lesson's safe preparation (Task 56; F-46; ONBOARDING.md § Safe
 * preparation: "Preparation may reveal a tab/panel, select a suitable
 * existing item, or position the playhead to illustrate a lesson. It must
 * not add/remove/trim/split media, change gains/effects, clear Undo,
 * render, save/download, request device permission or start recording.").
 *
 * So everything here is VIEW state: the library tab or inspector category
 * that holds the lesson's control, a closed compact drawer opened
 * (`revealBus`, the same request a Checks finding makes), and — for the
 * lessons about a selected clip — the earliest clip selected when nothing
 * is, with the timeline scrolled to it. Nothing calls `editor_execute`, and
 * an empty project simply has nothing to select: the coach then explains
 * rather than highlights.
 */
import type { Project } from "../../editorTypes";
import type { useEditorWorkspaceStore } from "../../stores/editorWorkspace";
import { requestReveal, requestTimelineReveal } from "../revealBus";
import type { GuideTargetKey } from "./targets";

type Workspace = ReturnType<typeof useEditorWorkspaceStore>;

/** The library tab each library lesson's control lives in. */
const LIBRARY_TAB: Partial<Record<GuideTargetKey, string>> = {
  "library.import": "media",
  "library.webcam": "media",
  "library.captions": "captions",
  "library.chapters": "chapters",
  "library.products": "products",
};

/** The inspector category each inspector lesson points into. `inspector`
 * itself is the arrange lesson: its timing fields are the Clip category. */
const PROPERTY_TAB: Partial<Record<GuideTargetKey, string>> = {
  inspector: "clip",
  "inspector.layout": "layout",
  "inspector.fades": "fades",
};

/** Lessons whose control exists only while a clip is selected. */
const NEEDS_SELECTION: ReadonlySet<GuideTargetKey> = new Set<GuideTargetKey>([
  "clip.selected",
  "inspector",
  "inspector.layout",
  "inspector.fades",
]);

function selectFirstClip(ws: Workspace, project: Project | null): void {
  if (!project || ws.selectionClipIds.length > 0) return;
  const first = [...project.clips].sort((a, b) => a.start_ms - b.start_ms)[0];
  if (!first) return;
  ws.select([first.id]);
  requestTimelineReveal(first.start_ms);
}

/** Shows the part of the editor lesson `key`'s control lives in. */
export function prepareLesson(key: GuideTargetKey, ws: Workspace, project: Project | null): void {
  const libraryTab = LIBRARY_TAB[key];
  if (libraryTab) {
    ws.setLibraryTab(libraryTab);
    requestReveal("library");
  }
  if (NEEDS_SELECTION.has(key)) selectFirstClip(ws, project);
  const propertyTab = PROPERTY_TAB[key];
  if (propertyTab) {
    ws.setPropertyTab(propertyTab);
    requestReveal("inspector");
  }
}
