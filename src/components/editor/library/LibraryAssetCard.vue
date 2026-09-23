<script setup lang="ts">
/**
 * One asset card in the media library (Task 25): name, kind, duration,
 * availability, and "+". Presentational — `MediaLibrary.vue` decides the
 * target track and the refusal reason and owns the `insertClip`; this only
 * renders them and emits `insert` when "+" can act. A refused "+" keeps
 * `aria-disabled` (never the native attribute) so its reason stays
 * reachable in the `title`.
 */
import { computed } from "vue";

const props = defineProps<{
  id: string;
  name: string;
  kindLabel: string;
  duration: string;
  missing: boolean;
  /** Why "+" cannot act, or null when it can. */
  refusal: string | null;
  /** The track "+" would insert onto (for the title). */
  targetName: string;
}>();

const emit = defineEmits<{ (e: "insert"): void }>();

const detail = computed(() =>
  [props.kindLabel, props.duration, props.missing ? "Missing" : null].filter(Boolean).join(" · "),
);
const title = computed(() => props.refusal ?? `Insert at the playhead on ${props.targetName}`);
const refused = computed(() => props.refusal !== null);

function onInsert(): void {
  if (!refused.value) emit("insert");
}
</script>

<template>
  <li
    :data-testid="`library-asset-${id}`"
    class="flex items-center gap-1 rounded border border-line bg-raised px-1 py-0.5"
  >
    <div class="flex min-w-0 flex-1 flex-col">
      <span class="truncate text-fg">{{ name }}</span>
      <span
        class="text-fg-subtle"
        :class="{ 'text-danger-fg': missing }"
      >{{ detail }}</span>
    </div>
    <button
      type="button"
      :data-testid="`library-asset-${id}-insert`"
      :aria-label="`Insert ${name} at the playhead`"
      :aria-disabled="refused"
      :title="title"
      class="shrink-0 rounded px-1 text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
      @click="onInsert"
    >
      +
    </button>
  </li>
</template>
