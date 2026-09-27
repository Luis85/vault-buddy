/**
 * The preview header's rules (visual-parity Task 11; concept spec §4.1,
 * §11; design D14): how wide the header is decides its density, which
 * teaching tools sit in the strip at that density, and what the More tools
 * and View menus hold. Pure, so the density table and the menus' item sets
 * are testable without a layout engine; the components supply the effects.
 *
 * The concept's `workspace-ui.js` is the behaviour mirrored here
 * (`density()`, `commandItems('tools' | 'view')`). Two of its items have no
 * native backend and are omitted, never shown disabled (D14): View's
 * "Download annotated frame…" (a browser download) and More tools' "Browse
 * all teaching tools" (the concept's library "teach" section, which this
 * editor has no counterpart for — every teaching tool is already in the
 * strip or this menu).
 */
import type { EditorIconName } from "../components/editor/icons/conceptIcons";
import type { MenuItem } from "../components/editor/menus/menuModel";
import { SEPARATOR } from "../components/editor/menus/menuModel";
import type { ActionId, ResolvedAction } from "./actions";

export type PreviewDensity = "wide" | "medium" | "compact";

/** `wide` from 800px, `medium` from 520, `compact` below (§11). A header
 * not measured yet (0) reads wide: nothing is hidden until a width says so. */
export function previewDensity(width: number): PreviewDensity {
  if (width <= 0 || width >= 800) return "wide";
  return width >= 520 ? "medium" : "compact";
}

export interface TeachingTool {
  id: ActionId;
  /** The strip button's `data-testid` suffix. */
  key: string;
  label: string;
  icon: EditorIconName;
}

const TEXT: TeachingTool = { id: "addText", key: "text", label: "Text", icon: "text" };
const ARROW: TeachingTool = { id: "addArrow", key: "arrow", label: "Arrow", icon: "arrowUpRight" };
const HIGHLIGHT: TeachingTool = { id: "addHighlight", key: "highlight", label: "Highlight", icon: "box" };
const ZOOM: TeachingTool = { id: "addZoom", key: "zoom", label: "Zoom", icon: "zoomIn" };

/** The tools that only ever live in More tools (§4.1). */
const MORE_ONLY: TeachingTool[] = [
  { id: "addSpotlight", key: "spotlight", label: "Spotlight", icon: "spotlight" },
  { id: "addStep", key: "step", label: "Numbered step", icon: "step" },
  { id: "addMask", key: "cover", label: "Privacy cover", icon: "shield" },
];

/** The strip's tools at `density`: compact moves Highlight and Zoom into
 * More (§11). */
export function stripTools(density: PreviewDensity): TeachingTool[] {
  return density === "compact" ? [TEXT, ARROW] : [TEXT, ARROW, HIGHLIGHT, ZOOM];
}

/** The strip tools a density moved into More. */
function overflowTools(density: PreviewDensity): TeachingTool[] {
  return density === "compact" ? [HIGHLIGHT, ZOOM] : [];
}

export const MORE_TOOLS_HEADING = "More teaching tools";
export const MORE_TOOLS_SUBTITLE = "Choose a cue for the current moment";

/** More tools: any tools the strip had no room for, a separator, then
 * Spotlight, Numbered step and Privacy cover — each enabled, or disabled
 * with the registry's reason. */
export function moreToolsItems(
  density: PreviewDensity,
  resolved: Record<ActionId, ResolvedAction>,
  run: (id: ActionId) => void,
): MenuItem[] {
  const item = (t: TeachingTool): MenuItem => ({
    id: t.id,
    label: t.label,
    icon: t.icon,
    disabledReason: resolved[t.id].enabled ? null : resolved[t.id].reason,
    run: () => run(t.id),
  });
  const overflow = overflowTools(density).map(item);
  return [...overflow, ...(overflow.length > 0 ? [SEPARATOR] : []), ...MORE_ONLY.map(item)];
}

export const VIEW_MENU_HEADING = "View & workspace";
export const VIEW_MENU_SUBTITLE = "Change your workspace, not your edit";

export interface ViewMenuContext {
  libraryVisible: boolean;
  inspectorVisible: boolean;
  focusPreview: boolean;
  lightTheme: boolean;
  toggleLibrary: () => void;
  toggleInspector: () => void;
  toggleFocusPreview: () => void;
  resetLayout: () => void;
  toggleTheme: () => void;
  openMixer: () => void;
  openShortcuts: () => void;
}

/** The View menu (§4.1): workspace toggles, the theme, the mixer and help.
 * None of these is an edit. */
export function viewMenuItems(ctx: ViewMenuContext): MenuItem[] {
  return [
    { id: "library", label: "Show media library", icon: "panelLeft", checked: ctx.libraryVisible, run: ctx.toggleLibrary },
    { id: "properties", label: "Show properties", icon: "sliders", checked: ctx.inspectorVisible, run: ctx.toggleInspector },
    { id: "focusPreview", label: "Focus preview", icon: "video", checked: ctx.focusPreview, run: ctx.toggleFocusPreview },
    { id: "resetLayout", label: "Reset panel layout", icon: "undo", run: ctx.resetLayout },
    SEPARATOR,
    { id: "lightTheme", label: "Light theme", icon: "sun", checked: ctx.lightTheme, run: ctx.toggleTheme },
    SEPARATOR,
    { id: "mixer", label: "Audio mixer…", icon: "sliders", run: ctx.openMixer },
    { id: "help", label: "Keyboard shortcuts & help…", icon: "info", run: ctx.openShortcuts },
  ];
}

/** The four canvases (concept §9.11 `FORMATS`), mirroring
 * `core::editor::limits::CANVASES` — read from the Rust source, the
 * `useInspectorDraft.ts` rule — so a format offered here is one Rust
 * accepts. */
export interface FrameFormat {
  name: string;
  ratio: string;
  width: number;
  height: number;
}

export const FRAME_FORMATS: readonly FrameFormat[] = [
  { name: "Landscape", ratio: "16:9", width: 1280, height: 720 },
  { name: "Portrait", ratio: "9:16", width: 720, height: 1280 },
  { name: "Square", ratio: "1:1", width: 720, height: 720 },
  { name: "Classic", ratio: "4:3", width: 960, height: 720 },
];

/** The ratio button's label for `canvas` ("16:9"); a canvas that is none
 * of the four (a hand-edited project) reads as its size. */
export function ratioLabel(canvas: { width: number; height: number } | null | undefined): string {
  if (!canvas) return "16:9";
  const known = FRAME_FORMATS.find((f) => f.width === canvas.width && f.height === canvas.height);
  return known?.ratio ?? `${canvas.width}×${canvas.height}`;
}

/** A panel toggle's title names what a press does (§11). */
export function panelToggleTitle(shown: boolean, panel: "media library" | "properties"): string {
  return `${shown ? "Hide" : "Show"} ${panel}`;
}
