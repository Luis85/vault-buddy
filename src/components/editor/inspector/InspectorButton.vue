<script setup lang="ts">
/**
 * The inspector's bordered button (visual-parity Task 13; concept spec §5):
 * the `.btn` (11px, 30px tall, an icon first) or, with `preset`, a
 * `.preset-row` button (10px, 29px). A refused button stays focusable with
 * its reason as the tooltip (`aria-disabled`, the track header's rule) and
 * sends nothing — a click is only emitted while `reason` is empty.
 */
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{ icon?: EditorIconName; preset?: boolean; reason?: string | null }>();
const emit = defineEmits<(e: "click") => void>();

function onClick(): void {
  if (!props.reason) emit("click");
}
</script>

<template>
  <button
    type="button"
    :aria-disabled="reason ? 'true' : undefined"
    :title="reason ?? undefined"
    class="inline-flex items-center gap-[7px] rounded-md border border-line bg-panel text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    :class="[
      preset ? 'min-h-[29px] px-2 py-1 text-[10px]' : 'min-h-[30px] px-[9px] py-1.5 text-[11px]',
      reason ? 'cursor-not-allowed opacity-45' : 'cursor-pointer hover:bg-hover',
    ]"
    @click="onClick"
  >
    <EditorIcon
      v-if="icon"
      :name="icon"
      :size="preset ? 13 : 15"
      class="shrink-0"
    />
    <slot />
  </button>
</template>
