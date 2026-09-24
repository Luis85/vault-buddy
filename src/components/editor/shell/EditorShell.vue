<script setup lang="ts">
/**
 * The tutorial editor's responsive workspace shell (Task 16, F-48;
 * SCREENS-AND-INTERACTIONS.md §02/§12; DESIGN-SYSTEM.md "Desktop shell:
 * application header, workspace with optional library/inspector, resizable
 * timeline, unobtrusive status footer").
 *
 * A CSS grid: header row; library | preview | inspector row; timeline row.
 * This task shipped the frame and lightweight placeholders for all five
 * named regions (`library`, `preview-toolbar`, `preview`, `inspector`,
 * `timeline`) so the grid, the responsive collapse and the header were all
 * real and testable before there was anything to put in them. Task 17
 * filled `preview-toolbar` with a real `PreviewToolbar` (below); later
 * tasks fill the four REMAINING region slots (`library`, `preview`,
 * `inspector`, `timeline`) with real content.
 *
 * Below 1180px (§12: "At 960×640 retain one preview toolbar and accessible
 * primary actions ... Side panels can use drawers; opening one must not
 * hide every route back") the library/inspector columns become toggled
 * disclosures instead of static columns — `EditorHeader` owns the toggle
 * buttons, this component owns the open/closed state and which rule
 * ("always shown" vs "shown only when open") applies. The header is a
 * SIBLING of the collapsible row, never inside it, so no drawer state can
 * ever cover or unmount it — that is what keeps it "the route back".
 *
 * `isCompact` is tracked from `window.innerWidth` in a plain `ref`, not a
 * CSS media query alone: `tests/editorShell.test.ts` has to be able to
 * assert the collapse in Vitest, and happy-dom has no layout engine to
 * evaluate a media query against (AGENTS.md's Testing conventions) — the
 * REAL, pixel-measured version of this rule is `tests/e2e/editorShell.
 * spec.ts`, driving the production bundle in real Chromium.
 *
 * **The keyboard shortcut dispatcher (Task 21)** lives here — the carried
 * Task 17 finding ("there are no clip elements to focus until the timeline
 * … CARRY the wiring to Task 20/21") lands on THIS root element rather than
 * `window`, and deliberately so: `LegacyCaptureEditor.vue` (still mounted
 * alongside this shell — `SHOW_LEGACY_EDITOR`, `EditorRoot.vue`) binds its
 * OWN Ctrl+Z/Shift+Z/Y listener on `window` unconditionally, so a second
 * `window`-level listener here would double-fire on every undo/redo
 * keystroke while both surfaces are up. A `@keydown` on this shell's own
 * root instead only ever sees a keystroke whose focus target is somewhere
 * INSIDE this subtree (a toolbar button, a timeline clip, an inspector
 * field) — bubbling, no capture — and calls `event.stopPropagation()` for
 * every combo it actually handles, so a shortcut this dispatcher claims
 * never reaches the legacy surface's `window` listener at all; an
 * unmatched, currently-disabled, or nothing-to-send combo (Ctrl+S, or F6
 * with the guide closed — `activateEditorAction` returns `false` for
 * those; F1/? and F6 are the guide's, below) is left alone to
 * bubble normally, never swallowed with nothing done. A keystroke whose
 * target sits inside an open `role="menu"`/`role="dialog"` is the menu's
 * (`shouldHandle`'s `menuOwnsKeys`): Delete pressed in the context menu
 * must not delete the selection behind it.
 *
 * **Height (Task 22).** The shell root, its grid and the preview slot's
 * wrapper all `grow`: the preview stage (`PreviewSurface`) takes whatever
 * height the window has left rather than a fixed or aspect-derived one.
 * While the legacy phase-4 surface still shares the window
 * (`SHOW_LEGACY_EDITOR`), its own `flex-1` preview and this shell split the
 * leftover height between them, and at the 960x640 floor both give theirs
 * up entirely — so the stage never pushes Save off-screen (the
 * `tests/e2e/editorLayout.spec.ts` contract). `min-height` stays `auto`
 * everywhere: the shell never shrinks below its own content.
 *
 * **`NotificationHost` (Task 32 fix round 1).** The editor window had no
 * toast surface at all until `PreviewToolbar`'s ratio control needed one —
 * mounted here, once, the same `useNotificationsStore`/`NotificationHost`
 * pair `ActionPanel.vue` already uses in the panel window (each webview
 * gets its own Pinia instance, AGENTS.md's window model, so this is a
 * distinct store from the panel's). The root below gains `relative` so the
 * host's own `absolute inset-x-3 bottom-3` anchors to this shell rather
 * than whatever positioned ancestor happens to sit further up the tree.
 *
 * **Guide progress (Task 55)** is read once per window when the shell first
 * mounts (`editorOnboarding.load()` — idempotent, and it never throws: an
 * unreadable store only flips the header's "Session only").
 *
 * **The guide (Task 56)** — `GuideInvitation` and `GuideCoach` — mounts
 * here, at the shell root: fixed-position layers OUTSIDE the preview
 * section, so nothing the guide draws can sit inside what a render mirrors
 * (A26). The dispatcher below answers the guide's keys itself: F1/?
 * (`help`) start or resume the walkthrough, F6 (`guideFocus`) moves focus
 * between the card and its control — even out of a text field, since the
 * control can be one and F6 types nothing — and an Escape nothing else answered
 * pauses it once no menu is open. A pending progress save is flushed when
 * the window hides and when the shell unmounts.
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import type { ActionId } from "../../../editor/actionMeta";
import { activateEditorAction } from "../../../editor/clipboard";
import { onReveal } from "../../../editor/revealBus";
import { isGuideDismissKey, matchShortcut, shouldHandle } from "../../../editor/shortcuts";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import NotificationHost from "../../NotificationHost.vue";
import GuideCoach from "../guide/GuideCoach.vue";
import GuideInvitation from "../guide/GuideInvitation.vue";
import EditorHeader from "./EditorHeader.vue";
import PreviewToolbar from "./PreviewToolbar.vue";

/** SCREENS-AND-INTERACTIONS.md §12's own breakpoint. */
/** Task 39: the header's "Open a project file", forwarded to `EditorRoot`,
 * which owns which project the shell is showing. */
const emit = defineEmits<{ (e: "open-project-file"): void }>();

const COMPACT_BREAKPOINT = 1180;

const viewportWidth = ref(window.innerWidth);
function onResize() {
  viewportWidth.value = window.innerWidth;
}
onMounted(() => window.addEventListener("resize", onResize));
onBeforeUnmount(() => window.removeEventListener("resize", onResize));

const onboarding = useEditorOnboardingStore();
onMounted(() => void onboarding.load());

/** A hidden window may never show again (the X hides it; a quit follows):
 * the last lesson change must not wait out the save debounce. */
function onVisibilityChange() {
  if (document.visibilityState === "hidden") void onboarding.flush();
}
onMounted(() => document.addEventListener("visibilitychange", onVisibilityChange));
onBeforeUnmount(() => {
  document.removeEventListener("visibilitychange", onVisibilityChange);
  void onboarding.flush();
});

const isCompact = computed(() => viewportWidth.value < COMPACT_BREAKPOINT);

/** Drawer open/closed — local view state (ARCHITECTURE-AND-STACK.md's
 * `editorWorkspace` boundary: "panel sizes" is exactly this store's future
 * job; it does not exist yet, so this stays a plain ref, same as `theme`
 * below). Closed by default: a drawer starts collapsed, like any other
 * disclosure. */
const libraryOpen = ref(false);
const inspectorOpen = ref(false);
/** Above the breakpoint both columns are always shown (a normal three-
 * column layout, no toggle semantics); below it, visibility follows the
 * drawer's own open state. */
const showLibrary = computed(() => !isCompact.value || libraryOpen.value);
const showInspector = computed(() => !isCompact.value || inspectorOpen.value);
/** Task 54: a finding revealed in a closed drawer opens that drawer (at
 * full width both columns already show). */
onReveal("library", () => {
  if (isCompact.value) libraryOpen.value = true;
});
onReveal("inspector", () => {
  if (isCompact.value) inspectorOpen.value = true;
});

/**
 * Theme: Task 16 kept this as a local ref seeded from
 * `prefers-color-scheme`, with its own module doc promising "Task 18
 * persists it [in `editorWorkspace`]". This task keeps that seed but moves
 * the STATE itself into `editorWorkspace` (persisted through
 * `editor_save_workspace` — F16 — so the toggle survives a reopen); this
 * component's own job shrinks to the one thing that stays view-local:
 * applying `document.documentElement`'s `data-theme`, which
 * `src/style.css`'s `[data-theme="light"]` block reads. The editor window
 * is its own webview, so this touches no other window.
 */
const workspace = useEditorWorkspaceStore();
watch(
  () => workspace.theme,
  (t) => {
    document.documentElement.dataset.theme = t;
  },
  { immediate: true },
);
function toggleTheme() {
  workspace.toggleTheme();
}

/**
 * `focus-preview` (Task 17's `PreviewToolbar`, F-48): collapses both
 * drawers so the preview gets the room — a real, observable effect at the
 * compact width where drawers exist at all (`showLibrary`/`showInspector`
 * above always show both columns once `!isCompact`, so this is currently a
 * no-op at wide width, same as clicking a closed drawer's own toggle would
 * be). The dedicated distraction-free layout `Workspace.focus_preview`
 * already names (`editorTypes.ts`) is Task 18's `editorWorkspace` store to
 * build — this is the honest, minimal thing available before that store
 * exists, not a placeholder that pretends to do more.
 */
function onFocusPreview() {
  libraryOpen.value = false;
  inspectorOpen.value = false;
}

// ---- keyboard shortcut dispatcher (Task 21) --------------------------------

const editorProject = useEditorProjectStore();

/**
 * A single selection gives the dispatcher a real `pointerTarget` — the
 * `primaryTargetClip`/`targetTrackId` "falls back to a single selected clip"
 * rule `actions.ts` already establishes for a pointer-target-less caller —
 * so, for instance, Ctrl+V pastes onto the selected clip's own track rather
 * than reading "Select a track first" whenever nothing was right-clicked.
 */
function menuOwnsKeys(event: KeyboardEvent): boolean {
  const target = event.target;
  return target instanceof Element && target.closest('[role="menu"], [role="dialog"]') !== null;
}

const coach = ref<InstanceType<typeof GuideCoach> | null>(null);

/** The guide's own keys (Task 56). True when one was acted on. */
function onGuideKey(event: KeyboardEvent, actionId: ActionId | null): boolean {
  if (actionId === "help") {
    onboarding.start();
    return true;
  }
  if (actionId === "guideFocus") return coach.value?.toggleFocus() ?? false;
  return isGuideDismissKey(event) && (coach.value?.dismiss() ?? false);
}

/** F6 types nothing, so it may leave a text field: the guide's highlighted
 * control can BE one (the fades and layout lessons land in an input), and
 * without this F6 could never return to the card from there. Only an open
 * menu or dialog still owns it; every other key keeps `shouldHandle`'s
 * text-field rule. */
function gateAllows(event: KeyboardEvent, actionId: ActionId | null): boolean {
  const ownsKeys = menuOwnsKeys(event);
  if (actionId === "guideFocus") return !ownsKeys;
  return shouldHandle(event, { menuOwnsKeys: ownsKeys });
}

function onShellKeydown(event: KeyboardEvent) {
  const actionId = matchShortcut(event);
  if (!gateAllows(event, actionId)) return;
  if (onGuideKey(event, actionId)) {
    event.preventDefault();
    event.stopPropagation();
    return;
  }
  if (!actionId) return;
  const selection = workspace.selectionClipIds;
  const clip = selection.length === 1 ? editorProject.clipById(selection[0]) : undefined;
  const ctx = baseActionContext(
    editorProject.project,
    editorProject.snapshot,
    workspace.playheadMs,
    selection,
    clip ? { kind: "clip", id: clip.id, timeMs: workspace.playheadMs } : null,
  );
  if (!activateEditorAction(actionId, ctx, (command) => editorProject.execute(command))) return;
  // Claimed: stop it here so `LegacyCaptureEditor.vue`'s own `window`
  // listener (still mounted -- see the module doc) never double-handles the
  // same keystroke. A disabled/unmatched/nothing-to-send combo returns
  // above WITHOUT this, so it keeps bubbling exactly as it did before this
  // dispatcher existed.
  event.preventDefault();
  event.stopPropagation();
}
</script>

<template>
  <div
    data-testid="editor-shell"
    class="relative flex grow flex-col gap-2 text-fg"
    @keydown="onShellKeydown"
  >
    <EditorHeader
      :is-compact="isCompact"
      :library-open="libraryOpen"
      :inspector-open="inspectorOpen"
      :theme="workspace.theme"
      @toggle-library="libraryOpen = !libraryOpen"
      @toggle-inspector="inspectorOpen = !inspectorOpen"
      @toggle-theme="toggleTheme"
      @open-project-file="emit('open-project-file')"
    />

    <div
      class="grid grow gap-2"
      :class="isCompact ? 'grid-cols-1' : 'editor-shell-grid'"
    >
      <aside
        v-show="showLibrary"
        data-testid="editor-shell-library"
        :aria-hidden="!showLibrary"
        class="rounded-control border border-line bg-panel p-2 text-micro text-fg-subtle"
      >
        <slot name="library">
          Library — arrives in a later task.
        </slot>
      </aside>

      <section
        data-testid="editor-shell-preview"
        class="flex flex-col gap-2 rounded-control border border-line bg-stage p-2"
      >
        <!-- DESIGN-SYSTEM.md: "The preview has one 48px control/header row."
             `PreviewToolbar` (Task 17) owns the `data-testid="preview-toolbar"`
             row itself now — the Playwright spec's "exactly one row" count
             still holds because its root, not a wrapper here, carries the
             testid. -->
        <PreviewToolbar
          :library-open="libraryOpen"
          :inspector-open="inspectorOpen"
          @toggle-library="libraryOpen = !libraryOpen"
          @toggle-inspector="inspectorOpen = !inspectorOpen"
          @focus-preview="onFocusPreview"
        />
        <div class="flex min-h-0 grow flex-col text-micro text-fg-subtle">
          <slot name="preview">
            Preview — filled by the root (`PreviewSurface`, Task 22).
          </slot>
        </div>
      </section>

      <aside
        v-show="showInspector"
        data-testid="editor-shell-inspector"
        :aria-hidden="!showInspector"
        class="rounded-control border border-line bg-panel p-2 text-micro text-fg-subtle"
      >
        <slot name="inspector">
          Inspector — arrives in a later task.
        </slot>
      </aside>
    </div>

    <!-- `p-1`, not the library/inspector panels' `p-2` -- Task 20's real
         `TimelineView` already draws its own toolbar/lane/resize-handle
         chrome, so the extra 8px this wrapper used to reserve was pure
         padding-on-padding. Trimmed once the slot stopped being a bare
         placeholder line that needed the breathing room. -->
    <section
      data-testid="editor-shell-timeline"
      class="rounded-control border border-line bg-panel p-1 text-micro text-fg-subtle"
    >
      <slot name="timeline">
        Timeline — arrives in a later task.
      </slot>
    </section>

    <NotificationHost />
    <GuideInvitation />
    <GuideCoach ref="coach" />
  </div>
</template>

<style scoped>
.editor-shell-grid {
  grid-template-columns: var(--editor-sidebar) 1fr var(--editor-inspector);
}
</style>
