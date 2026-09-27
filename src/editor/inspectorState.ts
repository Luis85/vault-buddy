/**
 * What the inspector shows (visual-parity Task 13; concept spec §5): which
 * of its five states the selection puts it in, the heading each state
 * reads, the categories a clip offers, and the selection card's line for a
 * clip. Pure, so `InspectorPanel` stays orchestration.
 *
 * The states follow the concept's own order (`editor.js` `renderInspector`,
 * `contextual-ui.js`): a selected track, then a selected teaching cue, then
 * several clips, then one clip, else nothing. Selecting a track clears the
 * clips and the cue (`workspaceSelection.ts`), so only one ever applies in
 * practice; the order is the tie-break a stale mix would fall to.
 */
import type { AssetKind, Clip, Project } from "../editorTypes";
import type { InspectorTab } from "./menuContext";
import { clipOutputDuration } from "./timeMap";

export type InspectorMode = "none" | "clip" | "multi" | "track" | "effect";

export const INSPECTOR_TITLES: Record<InspectorMode, string> = {
  none: "Properties",
  clip: "Clip properties",
  multi: "Selection properties",
  track: "Track properties",
  effect: "Teaching properties",
};

export function inspectorMode(trackId: string | null, effectId: string | null, clipCount: number): InspectorMode {
  if (trackId) return "track";
  if (effectId) return "effect";
  if (clipCount > 1) return "multi";
  return clipCount === 1 ? "clip" : "none";
}

/** A picture clip's six categories; a sound-only clip has no Layout or
 * Color (`contextual-ui.js`). */
const VIDEO_TABS: readonly InspectorTab[] = ["clip", "layout", "fades", "audio", "speed", "color"];
const AUDIO_TABS: readonly InspectorTab[] = ["clip", "fades", "audio", "speed"];

export function tabsFor(kind: AssetKind | undefined): readonly InspectorTab[] {
  return kind === "audio" ? AUDIO_TABS : VIDEO_TABS;
}

export const TAB_LABELS: Record<InspectorTab, string> = {
  clip: "Clip",
  layout: "Layout",
  fades: "Fades",
  audio: "Audio",
  speed: "Speed",
  color: "Color",
};

/** The persisted tab when these categories include it, else the first. */
export function activeTabOf(saved: string | null, tabs: readonly InspectorTab[]): InspectorTab {
  return tabs.find((t) => t === saved) ?? tabs[0];
}

/** The categories that act on a whole multi-selection at once: Layout and
 * Color send one atomic `setLayout` / `setAdjustments` over every clip. */
export const BATCH_TABS: readonly InspectorTab[] = ["layout", "color"];

/** Whether `clip` offers the category `tab`. */
export function offersTab(project: Project | null, clip: Clip, tab: InspectorTab): boolean {
  return tabsFor(assetKindOf(project, clip)).includes(tab);
}

/** "{track name} · {d.d}s", the clip's length on the timeline. */
export function clipCardDetail(project: Project | null, clip: Clip): string {
  const track = project?.tracks.find((t) => t.id === clip.track_id);
  const seconds = clipOutputDuration(clip.in_ms, clip.out_ms, clip.speed ?? 1) / 1_000;
  return `${track?.name ?? "Track"} · ${seconds.toFixed(1)}s`;
}

export function assetKindOf(project: Project | null, clip: Clip | null): AssetKind | undefined {
  return clip ? project?.assets.find((a) => a.id === clip.asset_id)?.kind : undefined;
}
