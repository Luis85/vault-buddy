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
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorHeader from "./EditorHeader.vue";
import PreviewToolbar from "./PreviewToolbar.vue";

/** SCREENS-AND-INTERACTIONS.md §12's own breakpoint. */
const COMPACT_BREAKPOINT = 1180;

const viewportWidth = ref(window.innerWidth);
function onResize() {
  viewportWidth.value = window.innerWidth;
}
onMounted(() => window.addEventListener("resize", onResize));
onBeforeUnmount(() => window.removeEventListener("resize", onResize));

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
</script>

<template>
  <div
    data-testid="editor-shell"
    class="flex flex-col gap-2 text-fg"
  >
    <EditorHeader
      :is-compact="isCompact"
      :library-open="libraryOpen"
      :inspector-open="inspectorOpen"
      :theme="workspace.theme"
      @toggle-library="libraryOpen = !libraryOpen"
      @toggle-inspector="inspectorOpen = !inspectorOpen"
      @toggle-theme="toggleTheme"
    />

    <div
      class="grid gap-2"
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
        <div class="text-micro text-fg-subtle">
          <slot name="preview">
            Preview canvas — arrives in Task 17.
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
  </div>
</template>

<style scoped>
.editor-shell-grid {
  grid-template-columns: var(--editor-sidebar) 1fr var(--editor-inspector);
}
</style>
