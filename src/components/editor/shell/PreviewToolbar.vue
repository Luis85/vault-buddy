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
 * rather than a second hand-rolled timer/dismiss here. The store's own
 * dedupe (`notifications.ts`'s `isRepeat`) is what gives "one-time" its
 * exact meaning: a repeat of the SAME message text restarts that toast's
 * TTL instead of stacking a second one, which is why the message below is
 * one constant string regardless of which ratio was picked.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { baseActionContext } from "../../../editor/actionContext";
import type { ActionContext, ActionId } from "../../../editor/actions";
import { commandFor, resolveActions } from "../../../editor/actions";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { useNotificationsStore } from "../../../stores/notifications";
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

/** `RatioSelect`'s own `change` handler — bypasses `commandFor`/`BUILDERS`
 * entirely (see the module doc): `ratio` needs the CHOSEN pair, which
 * `commandFor` has no way to be handed. */
async function onRatioChange(value: string): Promise<void> {
  const [width, height] = value.split("x").map(Number);
  closeMore();
  const ok = await editorProject.execute({ kind: "setCanvas", width, height });
  if (ok) notifications.info(CANVAS_TOAST_MESSAGE);
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
  const command = commandFor(id, context.value);
  if (command) void editorProject.execute(command);
  closeMore();
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
    ref="rowRef"
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
        :ref="(el) => setItemRef(visibleItems.length, el)"
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
</template>
