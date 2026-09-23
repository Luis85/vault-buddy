/**
 * The read side of captions and chapters (Task 36; F-34–F-36): what
 * `CaptionsLibrary`/`ChaptersLibrary` list, the notices they raise, and the
 * one command each "at the playhead" button would send -- shared with the
 * `addCaption`/`addMarker` actions (`actions.ts`), so a library button and
 * the action registry can never pick a different clip or time.
 *
 * **Two time bases, never mixed.** Every cue and marker is stored in its
 * clip's SOURCE time (`core::editor::commands::captions`/`cues`); everything
 * a person reads or types is OUTPUT time. The mapping is `timeMap.ts`'s, the
 * TypeScript twin Rust's own `time.rs` is held to by the shared fixture:
 * `cueOutputSpan` for a cue, `outputAt` for a marker, `sourceAt` for the
 * playhead going the other way. A cue or marker whose source instant is no
 * longer inside its clip (trimmed away) is not listed -- Rust's own
 * `commands::chapters` omits exactly the same markers (docs/Gaps.md
 * GAP-181).
 *
 * **Notices are editorial heuristics, not certification** (the reference
 * editor's own wording): a cue reading faster than `DENSITY_LIMIT_CPS`
 * characters per second of OUTPUT, and a cue that starts before an earlier
 * one ends. Each names the cue it concerns so "Select cue" can reveal it.
 */
import type { CaptionCue, Clip, EditorCommand, Marker, Project } from "../editorTypes";
import { formatDuration } from "../utils/formatDuration";
import { lockedReason, NO_PROJECT } from "./actionMeta";
import type { ActionContext, Verdict } from "./actionTargets";
import { clipSpanOf, lockedTrackName, OK } from "./actionTargets";
import { clipIsActive, cueOutputSpan, outputAt, sourceAt } from "./timeMap";

/** The brief's reading-speed line, in characters per second of output. */
export const DENSITY_LIMIT_CPS = 20;
/** A new caption's length, in OUTPUT ms (the teaching cues' default). */
const CAPTION_DEFAULT_MS = 3_000;
/** `core::editor::limits::MAX_CAPTIONS` / `MAX_MARKERS`. */
const MAX_CAPTIONS = 2_000;
const MAX_MARKERS = 300;
const NEW_CAPTION_TEXT = "New caption";

const NO_CAPTION_CLIP = "No clip at the playhead to add a caption to";
const NO_MARKER_CLIP = "No clip at the playhead to add a chapter to";
const NO_CUE_AT_PLAYHEAD = "Move the playhead inside a caption to split it";

/** One listed caption: its cue, its clip, and where it plays. */
export interface CaptionRow {
  cue: CaptionCue;
  clip: Clip;
  startMs: number;
  endMs: number;
  /** Characters per second of OUTPUT. */
  cps: number;
  /** 1-based position in the output-ordered list, for "Caption 3". */
  index: number;
}

export interface CaptionNotice {
  kind: "density" | "overlap";
  row: CaptionRow;
  message: string;
}

export interface ChapterRow {
  marker: Marker;
  clip: Clip;
  outputMs: number;
}

/** A command a button would send, or the reason it cannot. */
export type Draft = { command: EditorCommand } | { reason: string };

/** `m:ss.t` -- a list's output timestamp, tenths included. */
export function formatOutputTime(ms: number): string {
  return `${formatDuration(ms)}.${Math.floor((Math.max(0, ms) % 1_000) / 100)}`;
}

function clipsById(project: Project): Map<string, Clip> {
  return new Map(project.clips.map((c) => [c.id, c]));
}

/** Every caption still visible in the edit, in OUTPUT order. */
export function captionRows(project: Project | null): CaptionRow[] {
  if (!project?.captions) return [];
  const clips = clipsById(project);
  const rows: Omit<CaptionRow, "index">[] = [];
  for (const cue of project.captions.cues) {
    const clip = clips.get(cue.clip_id);
    const span = clip ? cueOutputSpan(clipSpanOf(clip), cue.start_ms, cue.end_ms) : null;
    if (!clip || !span) continue;
    const seconds = (span[1] - span[0]) / 1_000;
    const cps = seconds > 0 ? Array.from(cue.text).length / seconds : 0;
    rows.push({ cue, clip, startMs: span[0], endMs: span[1], cps });
  }
  rows.sort((a, b) => a.startMs - b.startMs || a.endMs - b.endMs);
  return rows.map((row, i) => ({ ...row, index: i + 1 }));
}

/** The density and overlap notices, in list order. An overlap is reported
 * on the LATER cue, against the earlier cue that runs latest into it. */
export function captionNotices(rows: CaptionRow[]): CaptionNotice[] {
  const notices: CaptionNotice[] = [];
  let latest: CaptionRow | null = null;
  for (const row of rows) {
    if (row.cps > DENSITY_LIMIT_CPS) {
      notices.push({
        kind: "density",
        row,
        message: `Caption ${row.index} reads at ${row.cps.toFixed(1)} characters per second`,
      });
    }
    if (latest && row.startMs < latest.endMs) {
      notices.push({
        kind: "overlap",
        row,
        message: `Captions ${latest.index} and ${row.index} overlap`,
      });
    }
    if (!latest || row.endMs > latest.endMs) latest = row;
  }
  return notices;
}

function onVisibleTrack(project: Project, clip: Clip): boolean {
  return project.tracks.find((t) => t.id === clip.track_id)?.visible ?? false;
}

/** The topmost clip playing at `t` on a visible track -- `tracks[0]` is on
 * top (`previewLayers.ts`), so the lowest track index wins. */
function topmostClipAt(project: Project, t: number): Clip | null {
  const trackIndex = (c: Clip) => project.tracks.findIndex((tr) => tr.id === c.track_id);
  const playing = project.clips.filter((c) => onVisibleTrack(project, c) && clipIsActive(clipSpanOf(c), t));
  playing.sort((a, b) => trackIndex(a) - trackIndex(b));
  return playing[0] ?? null;
}

/** The clip a caption would attach to: the one selected clip, else the
 * topmost clip at the playhead (the reference's "Select footage or audio"). */
export function captionTargetClip(project: Project | null, selectedClipIds: string[], t: number): Clip | null {
  if (!project) return null;
  if (selectedClipIds.length === 1) {
    const selected = project.clips.find((c) => c.id === selectedClipIds[0]);
    if (selected) return selected;
  }
  return topmostClipAt(project, t);
}

function lockReason(project: Project, clip: Clip): string | null {
  const locked = lockedTrackName(project, [clip.id]);
  return locked ? lockedReason(locked) : null;
}

/** `addCaption` at the playhead: from the playhead's source instant (the
 * clip's own start when the playhead is off the selected clip) for
 * `CAPTION_DEFAULT_MS` of output, cut at the clip's source end. */
export function addCaptionAt(project: Project | null, selectedClipIds: string[], t: number): Draft {
  if (!project) return { reason: NO_PROJECT };
  const clip = captionTargetClip(project, selectedClipIds, t);
  if (!clip) return { reason: NO_CAPTION_CLIP };
  const locked = lockReason(project, clip);
  if (locked) return { reason: locked };
  if ((project.captions?.cues.length ?? 0) >= MAX_CAPTIONS) {
    return { reason: `This project already has the maximum of ${MAX_CAPTIONS} captions` };
  }
  const span = clipSpanOf(clip);
  const startMs = sourceAt(span, t) ?? clip.in_ms;
  const endMs = Math.min(clip.out_ms, startMs + Math.round(CAPTION_DEFAULT_MS * span.speed));
  return { command: { kind: "addCaption", clipId: clip.id, startMs, endMs, text: NEW_CAPTION_TEXT } };
}

/** `splitCaption` at the playhead: the selected caption when the playhead
 * is on it, else the first caption playing there. Its source instant must
 * fall strictly inside the cue (Rust refuses a boundary split). */
export function splitCaptionAt(rows: CaptionRow[], selectedId: string | null, t: number): Draft {
  const playing = rows.filter((r) => r.startMs <= t && t < r.endMs);
  const row = playing.find((r) => r.cue.id === selectedId) ?? playing[0];
  if (!row) return { reason: NO_CUE_AT_PLAYHEAD };
  const atMs = sourceAt(clipSpanOf(row.clip), t);
  if (atMs === null || atMs <= row.cue.start_ms || atMs >= row.cue.end_ms) {
    return { reason: NO_CUE_AT_PLAYHEAD };
  }
  return { command: { kind: "splitCaption", captionId: row.cue.id, atMs } };
}

function verdictOf(draft: Draft): Verdict {
  return "reason" in draft ? { enabled: false, reason: draft.reason } : OK;
}

export function resolveAddCaption(ctx: ActionContext): Verdict {
  return verdictOf(addCaptionAt(ctx.project, ctx.selectedClipIds, ctx.playheadMs));
}

/** Called only after `resolveAddCaption` said yes (`commandFor`'s contract). */
export function buildAddCaption(ctx: ActionContext): EditorCommand {
  return (addCaptionAt(ctx.project, ctx.selectedClipIds, ctx.playheadMs) as { command: EditorCommand }).command;
}

/** Every chapter still inside its clip's range, in OUTPUT order (then
 * title) -- `commands::chapters`' own order. */
export function chapterRows(project: Project | null): ChapterRow[] {
  if (!project) return [];
  const clips = clipsById(project);
  const rows: ChapterRow[] = [];
  for (const marker of project.markers) {
    const clip = clips.get(marker.clip_id);
    const outputMs = clip ? outputAt(clipSpanOf(clip), marker.source_ms) : null;
    if (clip && outputMs !== null) rows.push({ marker, clip, outputMs });
  }
  return rows.sort((a, b) => a.outputMs - b.outputMs || a.marker.title.localeCompare(b.marker.title));
}

/** `addMarker` at the playhead on the TOPMOST clip there (the brief). */
export function addMarkerAt(project: Project | null, t: number): Draft {
  if (!project) return { reason: NO_PROJECT };
  const clip = topmostClipAt(project, t);
  if (!clip) return { reason: NO_MARKER_CLIP };
  const locked = lockReason(project, clip);
  if (locked) return { reason: locked };
  if (project.markers.length >= MAX_MARKERS) {
    return { reason: `This project already has the maximum of ${MAX_MARKERS} chapters` };
  }
  const sourceMs = sourceAt(clipSpanOf(clip), t) as number;
  // Counts on from the chapters the user can SEE, not every stored marker
  // (a trimmed-away one is not a chapter of this edit).
  const title = `Chapter ${chapterRows(project).length + 1}`;
  return { command: { kind: "addMarker", clipId: clip.id, sourceMs, title } };
}

export function resolveAddMarker(ctx: ActionContext): Verdict {
  return verdictOf(addMarkerAt(ctx.project, ctx.playheadMs));
}

/** Called only after `resolveAddMarker` said yes. */
export function buildAddMarker(ctx: ActionContext): EditorCommand {
  return (addMarkerAt(ctx.project, ctx.playheadMs) as { command: EditorCommand }).command;
}
