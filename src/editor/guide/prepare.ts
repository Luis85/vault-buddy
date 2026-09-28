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
import type { Clip, Project } from "../../editorTypes";
import type { useEditorWorkspaceStore } from "../../stores/editorWorkspace";
import { offersTab } from "../inspectorState";
import type { InspectorTab } from "../menuContext";
import { requestReveal, requestTimelineReveal } from "../revealBus";
import type { GuideTargetKey } from "./targets";

type Workspace = ReturnType<typeof useEditorWorkspaceStore>;

/** The library tab each library lesson's control lives in. */
const LIBRARY_TAB: Partial<Record<GuideTargetKey, string>> = {
  "library.import": "media",
  "library.webcam": "media",
  "library.captions": "captions",
  "library.chapters": "chapters",
  // Not a tab: the library's Project section (visual-parity Task 10, D9).
  "library.products": "project",
};

/** The inspector category each inspector lesson points into. `inspector`
 * itself is the arrange lesson: its timing fields are the Clip category. */
const PROPERTY_TAB: Partial<Record<GuideTargetKey, string>> = {
  inspector: "clip",
  "inspector.layout": "layout",
  "inspector.fades": "fades",
};

/** Lessons whose control exists only while a clip is selected. */
const NEEDS_SELECTION: ReadonlySet<GuideTargetKey> = new Set<GuideTargetKey>(["clip.selected", "inspector"]);

/** Lessons whose control is one clip's category tab. */
const NEEDS_TAB: Partial<Record<GuideTargetKey, InspectorTab>> = {
  "inspector.layout": "layout",
  "inspector.fades": "fades",
};

function earliest(clips: readonly Clip[]): Clip | undefined {
  return [...clips].sort((a, b) => a.start_ms - b.start_ms)[0];
}

function selectFirstClip(ws: Workspace, project: Project | null): void {
  if (!project || ws.selectionClipIds.length > 0) return;
  const first = earliest(project.clips);
  if (!first) return;
  ws.select([first.id]);
  requestTimelineReveal(first.start_ms);
}

/** The Layout and Fades lessons point at a category tab, which exists only
 * while ONE clip that offers it is selected (visual-parity Task 13: several
 * clips show Selection properties, a sound clip has no Layout). Keeps such
 * a selection; otherwise selects the earliest clip that offers the tab. */
function selectClipOffering(ws: Workspace, project: Project | null, tab: InspectorTab): void {
  if (!project) return;
  const offering = (c: Clip) => offersTab(project, c, tab);
  const selected = ws.selectionClipIds.length === 1 ? project.clips.find((c) => c.id === ws.selectionClipIds[0]) : undefined;
  const target = selected && offering(selected) ? selected : earliest(project.clips.filter(offering));
  // Already the one clip on show (no cue selected over it): nothing to do.
  if (!target || (target === selected && ws.selected === null)) return;
  ws.selectClipsOnly([target.id]);
  requestTimelineReveal(target.start_ms);
}

/** Shows the part of the editor lesson `key`'s control lives in. */
export function prepareLesson(key: GuideTargetKey, ws: Workspace, project: Project | null): void {
  const libraryTab = LIBRARY_TAB[key];
  if (libraryTab) {
    ws.setLibraryTab(libraryTab);
    requestReveal("library");
  }
  if (NEEDS_SELECTION.has(key)) selectFirstClip(ws, project);
  const tab = NEEDS_TAB[key];
  if (tab) selectClipOffering(ws, project, tab);
  const propertyTab = PROPERTY_TAB[key];
  if (propertyTab) {
    ws.setPropertyTab(propertyTab);
    requestReveal("inspector");
  }
}
