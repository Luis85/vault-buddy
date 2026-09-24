<script setup lang="ts">
/**
 * The tutorial editor's preview toolbar (Task 17; F-15, F-49, F-48;
 * SCREENS-AND-INTERACTIONS.md §02: "One row immediately above the video
 * owns the preview tools, aspect ratio, Review and panel controls"). Fills
 * the `data-testid="preview-toolbar"` slot `EditorShell.vue` (Task 16) left
 * as a placeholder — this component's own root carries that testid now, so
 * the Playwright "exactly one preview-toolbar row" assertion still holds
 * unchanged (`tests/e2e/editorShell.spec.ts`).
 *
 * Every button reads `resolveActions`/`commandFor` from the ONE registry
 * (`actions.ts`) — the same source `ContextMenu.vue` reads — so a disabled
 * reason here and in a right-click menu can never disagree.
 *
 * `ActionContext`'s base (project/snapshot/playhead/selection) now comes
 * from `../../../editor/actionContext`'s `baseActionContext` (Task 20),
 * reading the REAL playhead/selection off `editorWorkspace` — this
 * component's own doc used to read "the `editorWorkspace` store that will
 * own selection/playhead lands in Task 18 … Task 18+ wires the real context
 * in without this component's own logic changing", which is exactly this
 * change, one task later than that doc guessed. No pointer target and no
 * clipboard still (this toolbar has neither a right-clicked thing nor a
 * wired clipboard) — every clip-scoped action reads "Select a clip first"
 * exactly when nothing IS selected, real now rather than permanent.
 *
 * Overflow: `TOOLBAR_ITEMS` is a fixed, ordered list; a `ResizeObserver` on
 * the row measures its own width and moves however many TRAILING items
 * don't fit into a labelled "More" menu, never a second toolbar row (one
 * `role="toolbar"` element, always — SCREENS-AND-INTERACTIONS.md §02:
 * "Primary creation actions remain reachable at compact widths through
 * labeled overflow, not a second toolbar"). happy-dom implements no real
 * layout engine, so `overflowCount` is also a prop: a test drives it
 * directly, production leaves it unset and the observer computes it.
 *
 * **Teaching tools (Task 35; F-27–F-33)** go through the same registry
 * (`cueActions.ts` resolves which clip and which source span); the one
 * extra step here is selecting the cue a successful add created, so the
 * user lands on its handles and inspector instead of hunting for it.
 *
 * **The ratio control (Task 32; F-38)** is the one item in `TOOLBAR_ITEMS`
 * that is not a plain button: it is a native `<select>` over the four
 * canvas presets (`CANVAS_RATIOS`, mirroring `core::editor::limits::
 * CANVASES` — read from the Rust source, the `useInspectorDraft.ts` rule —
 * through this one TS constant). A native select needs no popup-positioning
 * code of its own (no clipping-by-an-`overflow-hidden`-ancestor risk the
 * way a custom dropdown would have), so it fits directly into the same
 * `v-for` as every other item, just rendered differently for `id ===
 * "ratio"`. Choosing an option sends `setCanvas` DIRECTLY (`onRatioChange`,
 * never through `commandFor`/`BUILDERS` — see `actions.ts`'s own module
 * doc for why); a re-pick of the CURRENT ratio fires no `change` event at
 * all (native select semantics), which is also Rust's own no-op refusal
 * for a `setCanvas` naming the project's current canvas, so there is
 * nothing to guard against twice.
 *
 * A successful `setCanvas` shows a one-time toast (F-38: "crop/caption
 * warnings prompt a review") naming Checks — through the SAME shared
 * mechanism `ActionPanel.vue` already uses, `useNotificationsStore().info()`
 * + `NotificationHost.vue` (mounted once in `EditorShell.vue`, fix round 1),
 * rather than a second hand-rolled timer/dismiss here. Since Task 54 the
 * toast carries an **Open Checks** action (`checkReveal.openChecks`), where
 * the `canvasReview` finding names each source, text cue and caption the
 * new canvas no longer fits. An actionable toast is never deduped by the
 * store, so "one-time" is kept here instead: a second pick dismisses the
 * first toast before raising its replacement.
 *
 * **Review (Task 47; F-42, F18)** is the registry's `render` action: it
 * opens `ReviewDialog` over the selection's output span, or 5 s either side
 * of the playhead (`renderRanges.reviewRange`), which renders that range
 * for real and plays the encoded file. The dialog is this component's
 * SECOND root, a sibling of the toolbar row — inside the row, a key pressed
 * in the dialog would bubble into the row's roving-tabindex handler.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { useGuideOverflow, useGuideTarget } from "../../../composables/useGuideTarget";
import { baseActionContext } from "../../../editor/actionContext";
import type { ActionContext, ActionId } from "../../../editor/actions";
import { commandFor, resolveActions } from "../../../editor/actions";
import { addedEffectId } from "../../../editor/cueActions";
import type { AddEffectCommand } from "../../../editor/editorCommandTypes";
import { reviewRange } from "../../../editor/renderRanges";
import { onReveal, openChecks } from "../../../editor/revealBus";
import type { RenderRange } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { useNotificationsStore } from "../../../stores/notifications";
import ReviewDialog from "../dialogs/ReviewDialog.vue";
import RatioSelect from "./RatioSelect.vue";
import ToolbarOverflowMenu from "./ToolbarOverflowMenu.vue";

/** Fixed order: teaching tools, then ratio/review, then the panel/focus
 * toggles (SCREENS-AND-INTERACTIONS.md §02's own ordering). */
const TOOLBAR_ITEMS: ActionId[] = [
  "addText", "addArrow", "addHighlight", "addSpotlight", "addZoom", "addStep", "addMask",
  "ratio", "render", "focusPreview", "toggleLibrary", "toggleInspector",
];

const props = withDefaults(
  defineProps<{
    libraryOpen?: boolean;
    inspectorOpen?: boolean;
    /** Test-only override — see the module doc. `undefined` means "let the
     * `ResizeObserver` decide". */
    overflowCount?: number;
  }>(),
  { libraryOpen: false, inspectorOpen: false, overflowCount: undefined },
);
const emit = defineEmits<{
  (e: "toggle-library"): void;
  (e: "toggle-inspector"): void;
  (e: "focus-preview"): void;
}>();

const editorProject = useEditorProjectStore();
const editorWorkspace = useEditorWorkspaceStore();
const notifications = useNotificationsStore();

const context = computed<ActionContext>(() =>
  baseActionContext(
    editorProject.project,
    editorProject.snapshot,
    editorWorkspace.playheadMs,
    editorWorkspace.selectionClipIds,
  ),
);
const resolved = computed(() => resolveActions(context.value));

// ---- overflow measurement --------------------------------------------------

/** A rough per-button width (px, incl. gap) used only to decide HOW MANY
 * trailing items to move into More — not a pixel-perfect fit (Playwright's
 * `editorLayout.spec.ts` is where exact layout is measured, AGENTS.md
 * Testing conventions: "the ONE thing the suite above structurally cannot
 * do: measure"). */
const ITEM_WIDTH = 84;
const rowRef = ref<HTMLElement | null>(null);
const measuredOverflow = ref(0);
let observer: ResizeObserver | null = null;
/**
 * Fix round 1, finding 4b: this used to subtract one for "More" UNCONDITIONALLY,
 * so a row wide enough to fit every item still reserved a slot and showed a
 * "More" menu holding nothing anyone needed to see. The reservation is only
 * correct once something is actually going to overflow.
 */
function recomputeOverflow(width: number) {
  const totalFit = Math.floor(width / ITEM_WIDTH);
  if (totalFit >= TOOLBAR_ITEMS.length) {
    measuredOverflow.value = 0;
    return;
  }
  const maxFit = Math.max(0, totalFit - 1); // reserve one slot for "More" itself
  measuredOverflow.value = Math.max(0, TOOLBAR_ITEMS.length - maxFit);
}
onMounted(() => {
  if (typeof ResizeObserver === "function" && rowRef.value) {
    observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) recomputeOverflow(entry.contentRect.width);
    });
    observer.observe(rowRef.value);
  }
});
onBeforeUnmount(() => observer?.disconnect());

const overflowCount = computed(() =>
  Math.min(TOOLBAR_ITEMS.length, props.overflowCount ?? measuredOverflow.value),
);
const visibleItems = computed(() => TOOLBAR_ITEMS.slice(0, TOOLBAR_ITEMS.length - overflowCount.value));
const overflowItems = computed(() => TOOLBAR_ITEMS.slice(TOOLBAR_ITEMS.length - overflowCount.value));

// ---- guide targets (Task 55) -------------------------------------------------
// This row is the guide's `preview.toolstrip`; the lesson's own control is
// the Arrow tool, so once Arrow has moved into More the guide points at More
// (`revealed: "overflow"`), never at a tool the user cannot see.
const toolstripTarget = useGuideTarget("preview.toolstrip");
const moreTarget = useGuideOverflow("preview.toolstrip", () => overflowItems.value.includes("addArrow"));
function bindRow(el: Element | ComponentPublicInstance | null): void {
  rowRef.value = el as HTMLElement | null;
  toolstripTarget(el);
}
function bindMore(el: Element | ComponentPublicInstance | null): void {
  setItemRef(visibleItems.value.length, el);
  moreTarget(el);
}

// ---- the More menu ----------------------------------------------------------

const moreOpen = ref(false);
function closeMore() {
  moreOpen.value = false;
}
function onWindowPointerDown(event: PointerEvent) {
  if (!moreOpen.value) return;
  if (rowRef.value && !rowRef.value.contains(event.target as Node)) closeMore();
}
onMounted(() => window.addEventListener("pointerdown", onWindowPointerDown));
onBeforeUnmount(() => window.removeEventListener("pointerdown", onWindowPointerDown));

// ---- the ratio control + its one-time toast (Task 32; F-38) -----------------
// The control itself (the four options, the native <select>) lives in the
// sibling `RatioSelect.vue` — see that file's own doc for why it was split
// out (fallow complexity: this template used to inline it twice).

const canvasValue = computed(() => {
  const canvas = editorProject.project?.canvas;
  return canvas ? `${canvas.width}x${canvas.height}` : "";
});

/** The one-time toast's exact text (fix round 1: constant on purpose — see
 * the module doc, `useNotificationsStore`'s own dedupe keys on kind+message
 * equality, so a second pick before the first toast's TTL expires must send
 * this SAME string to restart it rather than push a second toast). */
const CANVAS_TOAST_MESSAGE = "Canvas changed. Review crop, text and caption placement in Checks.";
/** Long enough to reach the action, never sticky: the change is already
 * made and nothing waits on the answer. */
const CANVAS_TOAST_MS = 8_000;

/** The live canvas toast, so a second pick replaces it (see the module doc). */
let canvasToast: number | null = null;

/** `RatioSelect`'s own `change` handler — bypasses `commandFor`/`BUILDERS`
 * entirely (see the module doc): `ratio` needs the CHOSEN pair, which
 * `commandFor` has no way to be handed. */
async function onRatioChange(value: string): Promise<void> {
  const [width, height] = value.split("x").map(Number);
  closeMore();
  const ok = await editorProject.execute({ kind: "setCanvas", width, height });
  if (!ok) return;
  if (canvasToast !== null) notifications.dismiss(canvasToast);
  canvasToast = notifications.notify("info", CANVAS_TOAST_MESSAGE, {
    ttlMs: CANVAS_TOAST_MS,
    action: { label: "Open Checks", run: openChecks },
  });
}

// ---- activation + roving tabindex over the visible row ----------------------

function onActivate(id: ActionId) {
  // Fix round 1, finding 4a: the three panel/focus toggles are the FIRST
  // items to overflow (`TOOLBAR_ITEMS`' own trailing order), so clicking
  // one from inside the open More menu used to leave the menu open —
  // these `return`s skipped the `closeMore()` at the bottom entirely.
  if (id === "toggleLibrary") {
    emit("toggle-library");
    closeMore();
    return;
  }
  if (id === "toggleInspector") {
    emit("toggle-inspector");
    closeMore();
    return;
  }
  if (id === "focusPreview") {
    emit("focus-preview");
    closeMore();
    return;
  }
  if (!resolved.value[id].enabled) return;
  runAction(id);
  closeMore();
}

/** An enabled action's effect: Review opens its dialog; every other one
 * sends the command the registry builds. */
function runAction(id: ActionId) {
  if (id === "render") {
    openReview();
    return;
  }
  const command = commandFor(id, context.value);
  if (command?.kind === "addEffect") void addCue(command);
  else if (command) void editorProject.execute(command);
}

// ---- Review (Task 47; F18) ---------------------------------------------------

const reviewOpen = ref(false);
/** Frozen when Review is pressed: moving the playhead afterwards does not
 * change what is being rendered. */
const reviewTarget = ref<RenderRange | null>(null);
function openReview() {
  reviewTarget.value = reviewRange(
    editorProject.project,
    editorWorkspace.selectionClipIds,
    editorWorkspace.playheadMs,
    editorProject.durationMs,
  );
  reviewOpen.value = true;
}

/** A teaching tool (Task 35): add the cue, then select it so its handles
 * and the effect inspector are up at once — the new id is whichever effect
 * the committed project has that the one before it did not. */
async function addCue(command: AddEffectCommand): Promise<void> {
  const before = editorProject.project;
  if (!(await editorProject.execute(command))) return;
  const id = addedEffectId(before, editorProject.project);
  if (!id) return;
  editorWorkspace.select([command.clipId]);
  editorWorkspace.setSelected({ type: "effect", id });
}

const activeIndex = ref(0);
const itemEls = ref<(HTMLElement | null)[]>([]);
function setItemRef(i: number, el: Element | ComponentPublicInstance | null) {
  // Every item is a native <button>, so `el` is always an `Element` in
  // practice — the `ComponentPublicInstance` half of Vue's template-ref
  // callback type only applies to component refs, never a plain DOM node.
  itemEls.value[i] = el as HTMLElement | null;
}

/** Every currently-tabbable slot: the visible row plus the "More" button
 * when one is rendered. */
const focusableCount = computed(() => visibleItems.value.length + (overflowCount.value > 0 ? 1 : 0));

/**
 * Fix round 1, finding 3: a narrowing resize can shrink `focusableCount`
 * out from under an `activeIndex` an ArrowRight/End press had moved
 * further out — every `:tabindex` binding compares its own position
 * against `activeIndex`, so a stale, now-out-of-range value matched NONE
 * of them and the roving-tabindex row lost every stop on the Tab order at
 * once. Clamping here, not in the template, keeps the template's own
 * bindings simple equality checks (`i === activeIndex`) rather than each
 * one repeating a clamp.
 */
watch(focusableCount, (n) => {
  if (activeIndex.value > n - 1) activeIndex.value = Math.max(0, n - 1);
});

/** Task 54: a `canvasReview` finding points here. After the tick, so the
 * closing Checks dialog's focus restore does not land on top of it. */
onReveal("ratio", () => {
  const i = visibleItems.value.indexOf("ratio");
  if (i === -1) return;
  activeIndex.value = i;
  void nextTick(() => itemEls.value[i]?.focus());
});

async function onRowKeydown(event: KeyboardEvent) {
  const n = focusableCount.value;
  if (n === 0) return;
  let target: number;
  if (event.key === "ArrowRight") target = (activeIndex.value + 1) % n;
  else if (event.key === "ArrowLeft") target = (activeIndex.value - 1 + n) % n;
  else if (event.key === "Home") target = 0;
  else if (event.key === "End") target = n - 1;
  else return;
  event.preventDefault();
  activeIndex.value = target;
  await nextTick();
  itemEls.value[target]?.focus();
}

// ---- small per-item template helpers -----------------------------------
// Pulled out of the template itself (rather than inline ternaries) so the
// template's own measured complexity stays low — these are trivial on
// their own, but three inline ternaries repeated across two `v-for` rows
// is what pushed the template over the fallow health threshold.
function ariaPressedFor(id: ActionId): boolean | undefined {
  if (id === "toggleLibrary") return props.libraryOpen;
  if (id === "toggleInspector") return props.inspectorOpen;
  return undefined;
}
function itemTitle(id: ActionId): string {
  return resolved.value[id].reason ?? resolved.value[id].label;
}
function itemClass(id: ActionId): string {
  return resolved.value[id].enabled ? "text-fg-secondary" : "cursor-default opacity-50";
}
</script>

<template>
  <div
    :ref="bindRow"
    data-testid="preview-toolbar"
    role="toolbar"
    aria-label="Preview tools"
    class="flex h-8 w-full shrink-0 items-center gap-1 overflow-hidden rounded-control border border-line bg-raised px-2 text-micro text-fg-subtle"
    @keydown="onRowKeydown"
  >
    <template
      v-for="(id, i) in visibleItems"
      :key="id"
    >
      <RatioSelect
        v-if="id === 'ratio'"
        :ref="(el) => setItemRef(i, el)"
        testid="preview-toolbar-ratio"
        :select-tabindex="i === activeIndex ? 0 : -1"
        :disabled="!resolved.ratio.enabled"
        :title="itemTitle('ratio')"
        :value="canvasValue"
        @change="onRatioChange"
      />
      <button
        v-else
        :ref="(el) => setItemRef(i, el)"
        type="button"
        :data-testid="`preview-toolbar-${id}`"
        :tabindex="i === activeIndex ? 0 : -1"
        :aria-disabled="!resolved[id].enabled"
        :aria-pressed="ariaPressedFor(id)"
        :title="itemTitle(id)"
        class="shrink-0 cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="itemClass(id)"
        @click="onActivate(id)"
      >
        {{ resolved[id].label }}
      </button>
    </template>

    <div
      v-if="overflowCount > 0"
      class="relative shrink-0"
    >
      <button
        :ref="bindMore"
        type="button"
        data-testid="preview-toolbar-more"
        aria-haspopup="menu"
        :aria-expanded="moreOpen"
        :tabindex="activeIndex === visibleItems.length ? 0 : -1"
        class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="moreOpen = !moreOpen"
      >
        More
      </button>
      <ToolbarOverflowMenu
        v-if="moreOpen"
        :items="overflowItems"
        :resolved="resolved"
        :canvas-value="canvasValue"
        @activate="onActivate"
        @ratio-change="onRatioChange"
      />
    </div>
  </div>
  <ReviewDialog
    :open="reviewOpen"
    :range="reviewTarget"
    @close="reviewOpen = false"
  />
</template>
