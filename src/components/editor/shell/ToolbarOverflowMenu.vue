<script setup lang="ts">
/**
 * The preview toolbar's "More" popup contents (Task 17's overflow menu,
 * extracted Task 32) — the `role="menu"` list of whichever `TOOLBAR_ITEMS`
 * did not fit in the row. Split out of `PreviewToolbar.vue` purely to keep
 * THAT file's own `<template>` under the fallow complexity ceiling: this
 * menu's items need none of the roving-tabindex ref-forwarding the VISIBLE
 * row's items do (an overflowed item is reached by opening this popup, not
 * by arrow-keying to it), so unlike `RatioSelect.vue` this component needs
 * no `focus()` to expose. Nothing here is new behaviour.
 */
import type { ActionId, ResolvedAction } from "../../../editor/actions";
import RatioSelect from "./RatioSelect.vue";

const props = defineProps<{
  items: ActionId[];
  resolved: Record<ActionId, ResolvedAction>;
  canvasValue: string;
}>();
const emit = defineEmits<{
  (e: "activate", id: ActionId): void;
  (e: "ratio-change", value: string): void;
}>();

function itemTitle(id: ActionId): string {
  return props.resolved[id].reason ?? props.resolved[id].label;
}
function itemClass(id: ActionId): string {
  return props.resolved[id].enabled ? "text-fg-secondary" : "cursor-default opacity-50";
}
</script>

<template>
  <div
    role="menu"
    aria-label="More tools"
    data-testid="preview-toolbar-more-menu"
    class="absolute right-0 top-full z-10 mt-1 flex min-w-36 flex-col gap-0.5 rounded-control border border-line bg-panel p-1 shadow-lg"
    @click.stop
  >
    <template
      v-for="id in items"
      :key="id"
    >
      <RatioSelect
        v-if="id === 'ratio'"
        testid="preview-toolbar-ratio"
        :disabled="!resolved.ratio.enabled"
        :title="itemTitle('ratio')"
        :value="canvasValue"
        @change="emit('ratio-change', $event)"
      />
      <button
        v-else
        type="button"
        role="menuitem"
        :data-testid="`preview-toolbar-${id}`"
        :aria-disabled="!resolved[id].enabled"
        :title="itemTitle(id)"
        class="cursor-pointer rounded px-1.5 py-0.5 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="itemClass(id)"
        @click="emit('activate', id)"
      >
        {{ resolved[id].label }}
      </button>
    </template>
  </div>
</template>
