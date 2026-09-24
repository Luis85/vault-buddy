/**
 * What a before-you-share finding's button does (Task 54; F-45; SCREENS
 * 07: "Selecting a finding reveals the relevant object/control"): select
 * the object, seek to where it PLAYS, and open the panel, inspector tab or
 * surface that fixes it. Nothing here edits the project — revealing is
 * view state (`editorWorkspace`) plus a request to a surface that owns its
 * own open state (`revealBus.ts`).
 */
import type { CheckAction, CheckFinding, CheckTarget, Clip, Project } from "../editorTypes";
import { useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";
import { clipSpanOf } from "./actionTargets";
import { captionRows } from "./captionRules";
import { requestReveal, requestTimelineReveal } from "./revealBus";
import { cueOutputSpan } from "./timeMap";

/** Each action's button label, in the dialog. */
export const CHECK_ACTION_LABELS: Record<CheckAction, string> = {
  reconnect: "Reconnect media",
  select: "Show it",
  openCaptions: "Open Captions",
  openLayout: "Open Layout",
  openAudio: "Open the mixer",
  openWebcam: "Open Webcam",
  reviewCanvas: "Review the canvas",
  setDestination: "Choose a vault",
};

type Workspace = ReturnType<typeof useEditorWorkspaceStore>;

function seek(ws: Workspace, ms: number): void {
  ws.setPlayhead(ms);
  requestTimelineReveal(ms);
}

function showClip(ws: Workspace, clip: Clip): void {
  ws.select([clip.id]);
  ws.setSelected(null);
  seek(ws, clip.start_ms);
}

function firstClip(project: Project, test: (c: Clip) => boolean): Clip | undefined {
  return project.clips.filter(test).sort((a, b) => a.start_ms - b.start_ms)[0];
}

function showEffect(ws: Workspace, project: Project, id: string): void {
  const effect = project.effects.find((e) => e.id === id);
  const clip = effect && project.clips.find((c) => c.id === effect.clip_id);
  if (!effect || !clip) return;
  ws.select([clip.id]);
  ws.setSelected({ type: "effect", id });
  const span = cueOutputSpan(clipSpanOf(clip), effect.start_ms, effect.end_ms);
  seek(ws, span ? span[0] : clip.start_ms);
  requestReveal("inspector");
}

function showCaption(ws: Workspace, project: Project, id: string): void {
  const row = captionRows(project).find((r) => r.cue.id === id);
  ws.setSelected({ type: "caption", id });
  if (row) seek(ws, row.startMs);
}

/** The clip a clip, track or asset target is shown by: the clip itself,
 * or the earliest one on that track or of that source. */
const CLIP_OF: Partial<Record<CheckTarget["kind"], (project: Project, id: string) => Clip | undefined>> = {
  clip: (project, id) => project.clips.find((c) => c.id === id),
  track: (project, id) => firstClip(project, (c) => c.track_id === id),
  asset: (project, id) => firstClip(project, (c) => c.asset_id === id),
};

/** Selects and seeks to whatever `target` names; the project itself, or
 * an object that is gone, reveals nothing. */
function showTarget(ws: Workspace, project: Project, target: CheckTarget): void {
  const id = target.id;
  if (id === null) return;
  if (target.kind === "effect") return showEffect(ws, project, id);
  if (target.kind === "caption") return showCaption(ws, project, id);
  const clip = CLIP_OF[target.kind]?.(project, id);
  if (clip) showClip(ws, clip);
}

function openLibrary(ws: Workspace, tab: string): void {
  ws.setLibraryTab(tab);
  requestReveal("library");
}

function openProperty(ws: Workspace, tab: string): void {
  ws.setPropertyTab(tab);
  requestReveal("inspector");
}

type Revealer = (ws: Workspace, project: Project, finding: CheckFinding) => void;

const REVEALERS: Record<Exclude<CheckAction, "setDestination">, Revealer> = {
  reconnect: (ws) => {
    openLibrary(ws, "media");
    requestReveal("reconnect");
  },
  select: (ws, project, f) => showTarget(ws, project, f.target),
  openCaptions: (ws, project, f) => {
    openLibrary(ws, "captions");
    showTarget(ws, project, f.target);
  },
  openLayout: (ws, project, f) => {
    showTarget(ws, project, f.target);
    openProperty(ws, "layout");
  },
  openAudio: (ws, project, f) => {
    showTarget(ws, project, f.target);
    if (f.target.kind === "clip") openProperty(ws, "audio");
    requestReveal("mixer");
  },
  openWebcam: (ws) => {
    openLibrary(ws, "media");
    requestReveal("webcam");
  },
  reviewCanvas: (ws, project, f) => {
    showTarget(ws, project, f.target);
    requestReveal("ratio");
  },
};

/**
 * Reveals `finding`'s object. Answers whether the Checks dialog should
 * close to show it: `setDestination` is answered INSIDE the dialog (a vault
 * picker there), and a finding with no action has nothing to show.
 */
export function revealFinding(finding: CheckFinding): boolean {
  const project = useEditorProjectStore().project;
  if (!project || finding.action === null || finding.action === "setDestination") return false;
  REVEALERS[finding.action](useEditorWorkspaceStore(), project, finding);
  return true;
}
