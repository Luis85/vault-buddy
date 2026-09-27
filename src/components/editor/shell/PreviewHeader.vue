<script setup lang="ts">
/**
 * The preview column's header (visual-parity Task 11; concept spec §4.1,
 * §11; design D1, D5, D14, D15), replacing Task 17's `PreviewToolbar`. One
 * 48px row (44 in a window 760px tall or less, `editorWorkspace.
 * shortWindow`) on `--bg` under a `line` rule, left to right:
 * - the **library toggle** (panelLeft, `aria-pressed`, "Hide/Show media
 *   library") and **Preview**; then the **ratio button** (`RatioButton`,
 *   which opens Frame your tutorial);
 * - the teaching-tool strip (`Toolstrip`: Text, Arrow, Highlight, Zoom and
 *   More tools), `margin-left: auto` so it centres between the two groups;
 * - **Review**, **View options** (`ViewMenuButton`) and the **properties
 *   toggle** (sliders, `aria-pressed`, "Hide/Show properties"),
 *   `margin-left: auto`.
 *
 * The two toggles are `editorWorkspace`'s panel rules (`panelLayout.ts`):
 * a column at full width, a drawer below the drawer breakpoints (D5), so
 * at 960×640 they are what opens the drawers the header's toggles once
 * did.
 *
 * **Density** is the header's own width (the concept's `density()`, a
 * `ResizeObserver` on this row, else window resizes): wide from 800px,
 * medium from 520 ("Preview" hidden, "More tools" reads "More"), compact
 * below (Highlight and Zoom move into More, Review shows its icon only).
 * Unmeasured reads wide.
 *
 * **Review (Task 47; F-42, F18)** is the registry's `render` action: it
 * opens `ReviewDialog` over the selection's output span, or 5 s either side
 * of the playhead (`renderRanges.reviewRange`), frozen when pressed. Ctrl+E
 * asks for it through the reveal bus (Task 57).
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import { resolveActions } from "../../../editor/actions";
import { panelToggleTitle, previewDensity } from "../../../editor/previewHeader";
import { reviewRange } from "../../../editor/renderRanges";
import { onReveal } from "../../../editor/revealBus";
import type { RenderRange } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { useNotificationsStore } from "../../../stores/notifications";
import ReviewDialog from "../dialogs/ReviewDialog.vue";
import EditorIcon from "../icons/EditorIcon.vue";
import PanelToggleButton from "./PanelToggleButton.vue";
import RatioButton from "./RatioButton.vue";
import Toolstrip from "./Toolstrip.vue";
import ViewMenuButton from "./ViewMenuButton.vue";

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const notifications = useNotificationsStore();

/** The registry's context, shared with the tool strip, and its verdicts
 * on the header's own two actions: the ratio button and Review. */
const context = computed(() =>
  baseActionContext(editorProject.project, editorProject.snapshot, workspace.playheadMs, workspace.selectionClipIds),
);
const verdicts = computed(() => {
  const all = resolveActions(context.value);
  return { ratio: all.ratio, review: all.render };
});
const ratioReason = computed(() => (verdicts.value.ratio.enabled ? null : verdicts.value.ratio.reason));

// ---- density ------------------------------------------------------------------

const row = ref<HTMLElement | null>(null);
const width = ref(0);
const density = computed(() => previewDensity(width.value));
function measure(): void {
  width.value = row.value?.clientWidth ?? 0;
}
let observer: ResizeObserver | null = null;
onMounted(() => {
  if (typeof ResizeObserver === "function" && row.value) {
    observer = new ResizeObserver(measure);
    observer.observe(row.value);
  } else {
    window.addEventListener("resize", measure);
  }
  measure();
});
onBeforeUnmount(() => {
  observer?.disconnect();
  window.removeEventListener("resize", measure);
});

// ---- Review ---------------------------------------------------------------------

const reviewOpen = ref(false);
/** Frozen when Review is pressed: moving the playhead afterwards does not
 * change what is being rendered. */
const reviewTarget = ref<RenderRange | null>(null);
function openReview(): void {
  reviewTarget.value = reviewRange(
    editorProject.project,
    workspace.selectionClipIds,
    workspace.playheadMs,
    editorProject.durationMs,
  );
  reviewOpen.value = true;
}
function onReview(): void {
  const verdict = verdicts.value.review;
  if (verdict.enabled) openReview();
  else if (verdict.reason) notifications.info(verdict.reason);
}
onReveal("review", () => {
  if (verdicts.value.review.enabled) openReview();
});

/** Review's look at this density: icon-only when compact (§11), dimmed
 * with its reason while there is nothing to review. */
const review = computed(() => {
  const compact = density.value === "compact";
  const verdict = verdicts.value.review;
  return {
    showLabel: !compact,
    ariaLabel: compact ? "Review" : undefined,
    title: verdict.reason ?? "Review: render this part of the video and play it (Ctrl+E)",
    class: [compact ? "w-8 justify-center p-1.5" : "px-2.5", verdict.enabled ? "" : "cursor-not-allowed opacity-40"],
  };
});

const rowHeight = computed(() => (workspace.shortWindow ? "h-11" : "h-12"));
const libraryTitle = computed(() => panelToggleTitle(workspace.libraryVisible, "media library"));
const propertiesTitle = computed(() => panelToggleTitle(workspace.inspectorVisible, "properties"));
</script>

<template>
  <div
    ref="row"
    data-testid="preview-header"
    class="flex w-full shrink-0 items-center gap-2 overflow-hidden border-b border-line bg-app px-3"
    :class="rowHeight"
  >
    <PanelToggleButton
      data-testid="preview-library-toggle"
      icon="panelLeft"
      narrow
      :pressed="workspace.libraryVisible"
      :title="libraryTitle"
      @click="workspace.toggleLibrary()"
    />
    <span
      v-if="density === 'wide'"
      data-testid="preview-heading"
      class="shrink-0 text-[11px] font-semibold text-fg-secondary"
    >Preview</span>
    <RatioButton :disabled-reason="ratioReason" />

    <Toolstrip
      :density="density"
      :context="context"
    />

    <div class="ml-auto flex shrink-0 items-center gap-[3px]">
      <button
        type="button"
        data-testid="preview-review"
        :aria-disabled="!verdicts.review.enabled"
        :aria-label="review.ariaLabel"
        :title="review.title"
        class="inline-flex h-8 min-h-8 shrink-0 items-center gap-[7px] border border-transparent bg-transparent text-[11px] text-fg"
        :class="review.class"
        @click="onReview"
      >
        <EditorIcon name="play" />
        <template v-if="review.showLabel">
          Review
        </template>
      </button>
      <ViewMenuButton />
      <PanelToggleButton
        data-testid="preview-properties-toggle"
        icon="sliders"
        :pressed="workspace.inspectorVisible"
        :title="propertiesTitle"
        @click="workspace.toggleInspector()"
      />
    </div>
  </div>
  <ReviewDialog
    :open="reviewOpen"
    :range="reviewTarget"
    @close="reviewOpen = false"
  />
</template>
