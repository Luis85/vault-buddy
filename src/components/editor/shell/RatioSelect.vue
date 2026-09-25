<script setup lang="ts">
/**
 * The preview toolbar's aspect-ratio control (Task 32; F-38) — the native
 * `<select>` over the four canvas presets, extracted out of
 * `PreviewToolbar.vue` purely to keep THAT file's own `<template>` under
 * the fallow complexity ceiling: it appears twice there (the visible row
 * and the "More" overflow menu), and inlining a `v-if`/`v-else` branch plus
 * a nested `v-for` over four options in BOTH places was what pushed that
 * template's own measured complexity over the line. Nothing here is new
 * behaviour — see `PreviewToolbar.vue`'s own module doc for why the
 * control exists and sends `setCanvas` directly.
 *
 * `CANVAS_RATIOS` mirrors `core::editor::limits::CANVASES` (read from the
 * Rust source, the `useInspectorDraft.ts` rule) — the one TS constant this
 * control's four options come from, so a canvas Rust accepts and a ratio
 * this control offers can never drift apart.
 *
 * `focus()` is exposed so `PreviewToolbar.vue`'s roving-tabindex array
 * (`itemEls`, which calls `.focus()` on whatever a slot's `ref` resolved
 * to) can move focus here exactly as it would to a plain `<button>` — a
 * component ref resolves to the component instance, not its root element,
 * so without this the row's ArrowRight/Left/Home/End navigation would
 * silently skip this control.
 */
import { ref } from "vue";

defineProps<{
  disabled: boolean;
  title: string;
  value: string;
  testid: string;
  /** `undefined` in the overflow menu, where this control is not part of
   * the row's own roving-tabindex array. */
  selectTabindex?: number;
}>();
const emit = defineEmits<{ (e: "change", value: string): void }>();

const CANVAS_RATIOS: { width: number; height: number; label: string }[] = [
  { width: 1280, height: 720, label: "16:9 Landscape" },
  { width: 720, height: 1280, label: "9:16 Portrait" },
  { width: 720, height: 720, label: "1:1 Square" },
  { width: 960, height: 720, label: "4:3 Classic" },
];

const selectEl = ref<HTMLSelectElement | null>(null);
defineExpose({ focus: () => selectEl.value?.focus() });

function onChange(event: Event): void {
  emit("change", (event.target as HTMLSelectElement).value);
}
</script>

<template>
  <select
    ref="selectEl"
    :data-testid="testid"
    :tabindex="selectTabindex"
    aria-label="Canvas ratio"
    :disabled="disabled"
    :title="title"
    class="shrink-0 cursor-pointer rounded border-0 bg-transparent px-1 py-0.5 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed"
    :class="disabled ? 'cursor-default opacity-50' : 'text-fg-secondary'"
    :value="value"
    @change="onChange"
  >
    <option
      v-for="opt in CANVAS_RATIOS"
      :key="`${opt.width}x${opt.height}`"
      :value="`${opt.width}x${opt.height}`"
    >
      {{ opt.label }}
    </option>
  </select>
</template>
