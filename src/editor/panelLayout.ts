/**
 * The editor frame's panel rules (visual-parity Task 4; design D4, D5;
 * concept spec §1.3, §1.4, §6.1): which panel is a column and which a
 * drawer at a given window width, what each toggle does there, and the
 * timeline's height range. Pure functions over plain values, plus the one
 * factory `editorWorkspace` spreads into its own API, so the store stays
 * orchestration and these rules stay testable on their own.
 *
 * The widths come from `window.innerWidth`, never a CSS media query:
 * happy-dom has no layout engine to evaluate one against, and the shell's
 * collapse has to be assertable in Vitest (built inventory §2).
 *
 * D5, the audit's finding 2: every toggle is a real grid state at EVERY
 * width. Above a drawer breakpoint "Show media library" / "Show
 * properties" collapse their column to 0 (the persisted `library_hidden` /
 * `properties_hidden`); at or below it they open and close the drawer
 * (`properties_open` for the inspector, as the concept persists it; the
 * library drawer is session-only). Focus preview hides both and closes any
 * drawer. The concept's own `workspace-ui.js` (`toggleLibrary`,
 * `toggleProperties`, `focusPreview`, `showLibrary`, `showProperty`) is the
 * behaviour mirrored here, one rule per function.
 */
import type { Ref } from "vue";
import { computed } from "vue";

/** At or below this width the inspector is an overlay drawer (§1.4). */
const INSPECTOR_DRAWER_MAX_WIDTH = 1080;
/** At or below this width the library is an overlay drawer too (§1.4). */
const LIBRARY_DRAWER_MAX_WIDTH = 860;
/** At or below this width the header drops its save text (§1.4). */
export const SAVE_TEXT_MAX_WIDTH = 1350;
/** At or below this height the frame's bars shrink (§1.4, "≤760h"). */
const SHORT_WINDOW_MAX_HEIGHT = 760;

/** The timeline's default height and its keyboard step (§1.3, §6.1). */
export const TIMELINE_DEFAULT_HEIGHT = 400;
export const TIMELINE_KEY_STEP = 25;
const TIMELINE_MIN = 170;
const TIMELINE_MAX = 540;
/** What the rest of the frame needs below `innerHeight` (§1.3). */
const TIMELINE_HEADROOM = 370;

/** `[min, max]` for the timeline at a window `innerHeight` tall. */
export function timelineHeightRange(innerHeight: number): [number, number] {
  return [TIMELINE_MIN, Math.max(TIMELINE_MIN, Math.min(TIMELINE_MAX, innerHeight - TIMELINE_HEADROOM))];
}

export function clampTimelineHeight(height: number, innerHeight: number): number {
  const [lo, hi] = timelineHeightRange(innerHeight);
  return Math.round(Math.min(hi, Math.max(lo, height)));
}

/** The panel flags the toggles read and write. */
interface PanelState {
  libraryHidden: boolean;
  propertiesHidden: boolean;
  propertiesOpen: boolean;
  libraryDrawerOpen: boolean;
  focusPreview: boolean;
}

const libraryIsDrawerAt = (width: number) => width <= LIBRARY_DRAWER_MAX_WIDTH;
const inspectorIsDrawerAt = (width: number) => width <= INSPECTOR_DRAWER_MAX_WIDTH;

function libraryShown(s: PanelState, width: number): boolean {
  if (s.focusPreview) return false;
  return libraryIsDrawerAt(width) ? s.libraryDrawerOpen : !s.libraryHidden;
}

function inspectorShown(s: PanelState, width: number): boolean {
  if (s.focusPreview) return false;
  return inspectorIsDrawerAt(width) ? s.propertiesOpen : !s.propertiesHidden;
}

/** Show or hide the library where it lives at `width`; showing its drawer
 * closes the inspector's, so the two never stack. Leaves Focus preview. */
function setLibrary(s: PanelState, width: number, show: boolean): PanelState {
  if (!libraryIsDrawerAt(width)) return { ...s, focusPreview: false, libraryHidden: !show };
  return { ...s, focusPreview: false, libraryDrawerOpen: show, propertiesOpen: show ? false : s.propertiesOpen };
}

/** Show or hide the inspector where it lives at `width`. Leaves Focus
 * preview. */
function setInspector(s: PanelState, width: number, show: boolean): PanelState {
  const next = inspectorIsDrawerAt(width)
    ? { ...s, propertiesOpen: show }
    : { ...s, propertiesHidden: !show };
  return { ...next, focusPreview: false };
}

function toggleLibrary(s: PanelState, width: number): PanelState {
  return setLibrary(s, width, !libraryShown(s, width));
}

function toggleInspector(s: PanelState, width: number): PanelState {
  const show = !inspectorShown(s, width);
  const next = setInspector(s, width, show);
  return show && libraryIsDrawerAt(width) ? { ...next, libraryDrawerOpen: false } : next;
}

function toggleFocus(s: PanelState): PanelState {
  return { ...s, focusPreview: !s.focusPreview, libraryDrawerOpen: false, propertiesOpen: false };
}

/** A finding or a guide lesson asks for a panel: show it, never hide it. */
function revealLibrary(s: PanelState, width: number): PanelState {
  return setLibrary(s, width, true);
}

function revealInspector(s: PanelState, width: number): PanelState {
  return setInspector(s, width, true);
}

/** The store refs the panel rules read and write. */
interface PanelRefs {
  libraryHidden: Ref<boolean>;
  propertiesHidden: Ref<boolean>;
  propertiesOpen: Ref<boolean>;
  libraryDrawerOpen: Ref<boolean>;
  focusPreview: Ref<boolean>;
  viewportWidth: Ref<number>;
  viewportHeight: Ref<number>;
  timelineHeight: Ref<number>;
}

function read(r: PanelRefs): PanelState {
  return {
    libraryHidden: r.libraryHidden.value,
    propertiesHidden: r.propertiesHidden.value,
    propertiesOpen: r.propertiesOpen.value,
    libraryDrawerOpen: r.libraryDrawerOpen.value,
    focusPreview: r.focusPreview.value,
  };
}

function write(r: PanelRefs, s: PanelState): void {
  r.libraryHidden.value = s.libraryHidden;
  r.propertiesHidden.value = s.propertiesHidden;
  r.propertiesOpen.value = s.propertiesOpen;
  r.libraryDrawerOpen.value = s.libraryDrawerOpen;
  r.focusPreview.value = s.focusPreview;
}

/** `editorWorkspace`'s panel getters and actions. Every action persists. */
export function createPanelControls(r: PanelRefs, persist: () => void) {
  function apply(step: (s: PanelState, width: number) => PanelState): void {
    write(r, step(read(r), r.viewportWidth.value));
    persist();
  }
  return {
    libraryIsDrawer: computed(() => libraryIsDrawerAt(r.viewportWidth.value)),
    inspectorIsDrawer: computed(() => inspectorIsDrawerAt(r.viewportWidth.value)),
    libraryVisible: computed(() => libraryShown(read(r), r.viewportWidth.value)),
    inspectorVisible: computed(() => inspectorShown(read(r), r.viewportWidth.value)),
    shortWindow: computed(() => r.viewportHeight.value <= SHORT_WINDOW_MAX_HEIGHT),
    /** The timeline's height as shown: the stored one, clamped to the window. */
    timelineHeightPx: computed(() => clampTimelineHeight(r.timelineHeight.value, r.viewportHeight.value)),
    setViewport(width: number, height: number): void {
      r.viewportWidth.value = width;
      r.viewportHeight.value = height;
    },
    toggleLibrary: () => apply(toggleLibrary),
    toggleInspector: () => apply(toggleInspector),
    toggleFocusPreview: () => apply((s) => toggleFocus(s)),
    revealLibrary: () => apply(revealLibrary),
    revealInspector: () => apply(revealInspector),
    setTimelineHeight(height: number): void {
      r.timelineHeight.value = clampTimelineHeight(height, r.viewportHeight.value);
      persist();
    },
  };
}
