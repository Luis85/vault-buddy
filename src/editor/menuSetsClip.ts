/**
 * The clip context menus (visual-parity Task 5; concept spec §8 "Clip menu"
 * and "Multi-clip", design D13/D14). Every item maps to a registry action,
 * one `editor_execute` command, an inspector/library tab or a view change,
 * through the `MenuContext`. The concept's "Copy / paste look" is omitted:
 * the clipboard carries clips, not a look, so there is nothing for it to do.
 *
 * **Which time.** A right-click acts where the pointer was (Split at, Trim
 * to pointer, Add caption here); Shift+F10 and Edit actions act at the
 * playhead. The registry's split already prefers the pointer's time, and
 * the label shows the same time the command will use.
 *
 * **Locked tracks.** Every item that would change the clip stays listed but
 * disabled with the track's reason, so the menu reads the same and says why;
 * Copy stays enabled, since copying changes nothing.
 *
 * **Shortcuts.** An item shows only a shortcut that really is bound (the
 * registry's own table). The concept's F2 for Rename and V for Clear
 * selection are not bound in this app, so they are not shown.
 */
import type { MenuAction, MenuItem } from "../components/editor/menus/menuModel";
import { formatMenuTime, SEPARATOR } from "../components/editor/menus/menuModel";
import type { Adjustments, Clip, Project, Rotation } from "../editorTypes";
import { clipSpanOf, primaryTargetClip, targetClips } from "./actionTargets";
import { addCaptionAt } from "./captionRules";
import { COLOR_TREATMENTS, isTreatment } from "./colorPresets";
import type { EditorCommand } from "./editorCommandTypes";
import type { InspectorTab, MenuContext } from "./menuContext";
import { actionItem, clearSelectionItem, executeInOrder, lockReason, sendCommand } from "./menuContext";
import { clipOutputEnd, sourceAt } from "./timeMap";

/** The concept's speed presets; the Speed tab takes any other value. */
const SPEEDS = [0.25, 0.5, 1, 1.5, 2, 4] as const;
/** The concept's both-edges fade presets; 0 removes the fades. */
const EDGE_FADES_MS = [0, 250, 500, 1_000] as const;
/** "Go to this clip" lands just inside it, the concept's 400 ms. */
const GO_TO_OFFSET_MS = 400;
const NOT_INSIDE = "Choose a point inside the clip.";

/** One right-clicked clip and what every item needs to know about it. */
interface Target {
  ctx: MenuContext;
  project: Project;
  clip: Clip;
  /** The time the item acts at: the pointer's, else the playhead's. */
  t: number;
  /** Where the clip ends on the timeline (speed included). */
  end: number;
  /** The locked track's reason, or `null` when the clip may change. */
  locked: string | null;
  /** Picture items (Transform, Color, Cover, Detach audio) need a video. */
  isVideo: boolean;
}

/** A clip's timeline end: its start plus its source length at its speed. */
function endOf(clip: Clip): number {
  return clipOutputEnd(clipSpanOf(clip));
}

function targetOf(ctx: MenuContext): Target | null {
  const project = ctx.action.project;
  const clip = primaryTargetClip(ctx.action);
  if (!project || !clip) return null;
  return {
    ctx,
    project,
    clip,
    t: ctx.action.pointerTarget?.timeMs ?? ctx.action.playheadMs,
    end: endOf(clip),
    locked: lockReason(project, [clip.id]),
    isVideo: project.assets.find((a) => a.id === clip.asset_id)?.kind !== "audio",
  };
}

/** `t` pulled inside the clip, for an item that needs a playing frame. */
function timeInside(x: Target): number {
  return Math.min(Math.max(x.t, x.clip.start_ms), x.end - 1);
}

/** Selects the clip and opens one inspector tab on it (the "…" items). */
function tabItem(x: Target, id: string, label: string, tab: InspectorTab): MenuAction {
  return {
    id,
    label,
    run: () => {
      x.ctx.selectClips([x.clip.id]);
      x.ctx.openProperty(tab);
    },
  };
}

// ---- shared with the multi-clip menu -----------------------------------------

function fadeLabel(ms: number, edges: string): string {
  return ms === 0 ? "Remove edge fades" : `${ms / 1_000}s · ${edges}`;
}

/** Both edges at `ms`, never past half a clip (Rust's `fade_limit`). */
function fadeCommands(clips: Clip[], ms: number): EditorCommand[] {
  return clips.map((c) => {
    const fade = Math.min(ms, Math.floor((endOf(c) - c.start_ms) / 2));
    return { kind: "setFades", clipId: c.id, fadeInMs: fade, fadeOutMs: fade };
  });
}

function fadesItem(ctx: MenuContext, clips: Clip[], locked: string | null, edges: string, more: MenuItem[]): MenuAction {
  const presets = EDGE_FADES_MS.map((ms) => ({
    id: `fades-${ms}`,
    label: fadeLabel(ms, edges),
    run: () => void executeInOrder(ctx, fadeCommands(clips, ms)),
  }));
  return {
    id: "fades",
    label: "Fades",
    icon: "fade",
    disabledReason: locked,
    submenu: [...presets, ...more],
  };
}

function colorItem(
  ctx: MenuContext,
  clipIds: string[],
  locked: string | null,
  current: Adjustments | null | undefined,
  more: MenuItem[],
): MenuAction {
  const treatments = COLOR_TREATMENTS.map((t) => ({
    id: `color-${t.id}`,
    label: t.label,
    checked: clipIds.length === 1 ? isTreatment(current, t.adjustments) : undefined,
    run: sendCommand(ctx, { kind: "setAdjustments", clipIds, adjustments: t.adjustments }),
  }));
  return {
    id: "color",
    label: "Color treatment",
    icon: "palette",
    disabledReason: locked,
    submenu: [...treatments, ...more],
  };
}

// ---- the single-clip menu --------------------------------------------------------

/** "Go to this clip" selects it and puts the playhead just inside it, so
 * the preview shows its picture; "Fit this clip" only changes the view. */
function navigationItems(x: Target): MenuItem[] {
  const { ctx, clip } = x;
  const inset = Math.min(GO_TO_OFFSET_MS, Math.floor((x.end - clip.start_ms) / 2));
  return [
    {
      id: "goTo",
      label: "Go to this clip",
      icon: "locate",
      run: () => {
        ctx.selectClips([clip.id]);
        ctx.seek(clip.start_ms + inset);
      },
    },
    {
      id: "fitClip",
      label: "Fit this clip",
      icon: "zoomIn",
      run: () => ctx.fitRange(clip.start_ms, x.end),
    },
  ];
}

/** The clipboard trio through the registry (Copy also fills the window's
 * clipboard, which the registry's `activateEditorAction` does), and Rename,
 * which is the inspector's name field rather than a second rename UI. */
function editItems(x: Target): MenuItem[] {
  const { ctx } = x;
  return [
    actionItem(ctx, "copy", { label: "Copy clip", icon: "copy" }),
    actionItem(ctx, "cut", { label: "Cut clip", icon: "scissors" }),
    actionItem(ctx, "duplicate", { label: "Duplicate", icon: "copy" }),
    {
      id: "rename",
      label: "Rename…",
      icon: "edit",
      disabledReason: x.locked,
      run: () => {
        ctx.selectClips([x.clip.id]);
        ctx.openProperty("clip");
        ctx.focusClipName(x.clip.id);
      },
    },
  ];
}

/** Trim to the pointer: the start moves to the pointer (its source point
 * becomes the new in point), or the end is cut there. Refused at or past an
 * edge, where either trim would leave nothing or change nothing. */
function trimItem(x: Target): MenuAction {
  const { clip } = x;
  const source = sourceAt(clipSpanOf(clip), x.t);
  const inside = source !== null && x.t > clip.start_ms && source > clip.in_ms;
  const at = source ?? clip.in_ms;
  return {
    id: "trim",
    label: "Trim to pointer",
    icon: "scissors",
    disabledReason: x.locked ?? (inside ? null : NOT_INSIDE),
    submenu: [
      {
        id: "trim-start",
        label: "Trim start to here",
        run: sendCommand(x.ctx, {
          kind: "trimClip",
          clipId: clip.id,
          startMs: x.t,
          inMs: at,
          outMs: clip.out_ms,
        }),
      },
      {
        id: "trim-end",
        label: "Trim end to here",
        run: sendCommand(x.ctx, {
          kind: "trimClip",
          clipId: clip.id,
          startMs: clip.start_ms,
          inMs: clip.in_ms,
          outMs: at,
        }),
      },
    ],
  };
}

/** A preset keeps the clip's own pitch choice (on by default, as in the
 * Speed tab), so changing speed from here never changes how voices sound. */
function speedItem(x: Target): MenuAction {
  const { clip } = x;
  const speeds = SPEEDS.map((v) => ({
    id: `speed-${v}`,
    label: v === 1 ? "1× · normal" : `${v}×`,
    checked: (clip.speed ?? 1) === v,
    run: sendCommand(x.ctx, {
      kind: "setSpeed",
      clipId: clip.id,
      speed: v,
      preservePitch: clip.preserve_pitch ?? true,
    }),
  }));
  return {
    id: "speed",
    label: "Speed",
    icon: "speed",
    disabledReason: x.locked,
    submenu: [...speeds, SEPARATOR, tabItem(x, "speed-options", "Speed & timing options…", "speed")],
  };
}

type LayoutFields = Omit<Extract<EditorCommand, { kind: "setLayout" }>, "kind" | "clipIds">;

/** The concept's Transform submenu, each a `setLayout` of the fields it
 * names. Fit and flips are checkboxes showing the clip's current state; a
 * rotation turns a quarter clockwise from where the clip is now. */
function transformRows(clip: Clip): { id: string; label: string; checked?: boolean; fields: LayoutFields }[] {
  return [
    {
      id: "transform-fit",
      label: "Fit entire source",
      checked: (clip.fit ?? "contain") === "contain",
      fields: { fit: "contain" },
    },
    {
      id: "transform-fill",
      label: "Fill frame · crop",
      checked: clip.fit === "cover",
      fields: { fit: "cover" },
    },
    {
      id: "transform-rotate",
      label: "Rotate 90° clockwise",
      fields: { rotation: (((clip.rotation ?? 0) + 90) % 360) as Rotation },
    },
    {
      id: "transform-flip-x",
      label: "Flip horizontally",
      checked: Boolean(clip.mirror),
      fields: { mirror: !clip.mirror },
    },
    {
      id: "transform-flip-y",
      label: "Flip vertically",
      checked: Boolean(clip.flip_y),
      fields: { flipY: !clip.flip_y },
    },
    {
      id: "transform-center",
      label: "Center in canvas",
      fields: { x: (1 - clip.w) / 2, y: (1 - clip.h) / 2 },
    },
    {
      id: "transform-full",
      label: "Full frame",
      fields: { x: 0, y: 0, w: 1, h: 1 },
    },
    {
      id: "transform-reset",
      label: "Reset crop & orientation",
      fields: { rotation: 0, mirror: false, flipY: false, cropZoom: 1, cropX: 0.5, cropY: 0.5 },
    },
  ];
}

function transformItem(x: Target): MenuAction {
  const submenu = transformRows(x.clip).map(({ fields, ...row }) => ({
    ...row,
    run: sendCommand(x.ctx, { kind: "setLayout", clipIds: [x.clip.id], ...fields }),
  }));
  return {
    id: "transform",
    label: "Transform",
    icon: "rotate",
    disabledReason: x.locked,
    submenu,
  };
}

/** The privacy cover tool (the preview's `addMask`), on THIS clip at a time
 * inside it — the registry targets the selected clip under the playhead. */
function coverItem(x: Target): MenuAction {
  const at = timeInside(x);
  const coverCtx = { ...x.ctx.action, selectedClipIds: [x.clip.id], playheadMs: at };
  const look = { id: "cover", label: "Cover private information", icon: "shield" as const };
  const item = actionItem(x.ctx, "addMask", look, coverCtx);
  return {
    ...item,
    run: () => {
      x.ctx.seek(at);
      x.ctx.activate("addMask", coverCtx);
    },
  };
}

/** Transform, Color treatment and Cover private information: an audio clip
 * has no picture, so it has none of the three. */
function pictureItems(x: Target): MenuItem[] {
  if (!x.isVideo) return [];
  const adjust = tabItem(x, "color-options", "Adjust color…", "color");
  return [
    transformItem(x),
    colorItem(x.ctx, [x.clip.id], x.locked, x.clip.adjustments, [adjust]),
    coverItem(x),
  ];
}

/** Mute toggles this clip's own mute (the track's is the track menu's);
 * Detach is the registry's, which already refuses an image or a clip whose
 * audio is detached. */
function audioItem(x: Target): MenuAction {
  const { ctx, clip } = x;
  const mute = {
    id: "audio-mute",
    label: clip.muted ? "Unmute clip" : "Mute clip",
    run: sendCommand(ctx, { kind: "setClipMix", clipIds: [clip.id], muted: !clip.muted }),
  };
  const detachLook = { id: "audio-detach", label: "Detach audio · keep aligned" };
  const detach = x.isVideo ? [actionItem(ctx, "detachAudio", detachLook)] : [];
  return {
    id: "audio",
    label: "Audio",
    icon: "volume",
    disabledReason: x.locked,
    submenu: [mute, ...detach, tabItem(x, "audio-options", "Audio properties…", "audio")],
  };
}

/** A caption on THIS clip, at the pointer's time inside it — the Captions
 * library's own rule (`captionRules.addCaptionAt`), so its reasons (a
 * locked track, the caption limit) are the ones shown here too. */
function captionItem(x: Target): MenuAction {
  const at = timeInside(x);
  const draft = addCaptionAt(x.project, [x.clip.id], at);
  const command = "command" in draft ? draft.command : null;
  return {
    id: "caption",
    label: "Add caption here…",
    icon: "captions",
    disabledReason: "reason" in draft ? draft.reason : null,
    run: () => {
      if (!command) return;
      x.ctx.seek(at);
      void x.ctx.execute(command);
      x.ctx.openLibrary("captions");
    },
  };
}

/** The single-clip menu, in the concept's order; empty when the target no
 * longer resolves (a delete landed from another surface). */
export function clipMenu(ctx: MenuContext): MenuItem[] {
  const x = targetOf(ctx);
  if (!x) return [];
  const fadesTab = tabItem(x, "fades-options", "Fades & transitions…", "fades");
  return [
    ...navigationItems(x),
    SEPARATOR,
    ...editItems(x),
    SEPARATOR,
    actionItem(ctx, "split", { label: `Split at ${formatMenuTime(x.t)}`, icon: "scissors" }),
    trimItem(x),
    speedItem(x),
    ...pictureItems(x),
    fadesItem(ctx, [x.clip], x.locked, "both edges", [fadesTab]),
    audioItem(x),
    captionItem(x),
    SEPARATOR,
    clearSelectionItem(ctx),
    actionItem(ctx, "delete", { label: "Delete · leave gap", icon: "trash", danger: true }),
    actionItem(ctx, "deleteClose", { label: "Delete · ripple this track", icon: "gap", danger: true }),
  ];
}

// ---- the multi-clip menu -----------------------------------------------------------

/** Mute and Unmute the whole selection, each refused when it would change
 * nothing (every clip already muted, or none muted). */
function muteItems(ctx: MenuContext, clips: Clip[], locked: string | null): MenuAction[] {
  const clipIds = clips.map((c) => c.id);
  return [
    {
      id: "muteSelection",
      label: "Mute selection",
      icon: "muted",
      disabledReason: locked ?? (clips.every((c) => c.muted) ? "Every selected clip is already muted." : null),
      run: sendCommand(ctx, { kind: "setClipMix", clipIds, muted: true }),
    },
    {
      id: "unmuteSelection",
      label: "Unmute selection",
      icon: "volume",
      disabledReason: locked ?? (clips.some((c) => c.muted) ? null : "No selected clip is muted."),
      run: sendCommand(ctx, { kind: "setClipMix", clipIds, muted: false }),
    },
  ];
}

/** The multi-clip menu: what can apply to many clips at once. Fades are one
 * `setFades` per clip (Rust has no batch form), sent in order and stopped
 * at the first refusal; colour and mute are one command for all. */
export function multiClipMenu(ctx: MenuContext): MenuItem[] {
  const project = ctx.action.project;
  if (!project) return [];
  const clips = targetClips(ctx.action);
  const clipIds = clips.map((c) => c.id);
  const locked = lockReason(project, clipIds);
  const start = Math.min(...clips.map((c) => c.start_ms));
  const end = Math.max(...clips.map(endOf));
  return [
    actionItem(ctx, "copy", { label: "Copy selection", icon: "copy" }),
    actionItem(ctx, "cut", { label: "Cut selection", icon: "scissors" }),
    actionItem(ctx, "duplicate", { label: "Duplicate selection", icon: "copy" }),
    SEPARATOR,
    actionItem(ctx, "group", { label: "Group selection", icon: "group" }),
    actionItem(ctx, "ungroup", { label: "Ungroup", icon: "unlock" }),
    { id: "fitSelection", label: "Fit selection", icon: "locate", run: () => ctx.fitRange(start, end) },
    fadesItem(ctx, clips, locked, "fade both edges", []),
    colorItem(ctx, clipIds, locked, null, []),
    ...muteItems(ctx, clips, locked),
    SEPARATOR,
    clearSelectionItem(ctx),
    actionItem(ctx, "delete", { label: "Delete selection · leave gaps", icon: "trash", danger: true }),
  ];
}
