<script setup lang="ts">
/**
 * `CaptionsLibrary`'s three actions -- Import, Add at playhead, Split at
 * playhead -- and the import's "replace" choice (Task 36). Presentational:
 * each button is disabled by, and titled with, the reason its parent
 * computed (`captionRules` drafts, R20: a disabled control says why), and
 * a click is only an emit. Split out so the library's own template stays a
 * list of sections.
 */
import { computed } from "vue";

const props = defineProps<{
  importReason: string | null;
  importing: boolean;
  addReason: string | null;
  splitReason: string | null;
}>();

const replace = defineModel<boolean>("replace", { required: true });

const emit = defineEmits<{
  (e: "import"): void;
  (e: "add"): void;
  (e: "split"): void;
}>();

const actions = computed(() => [
  {
    id: "import" as const,
    label: props.importing ? "Importing…" : "Import SRT / WebVTT",
    reason: props.importing ? "An import is already running" : props.importReason,
    hint: "Import SRT / WebVTT onto the selected clip",
  },
  { id: "add" as const, label: "Add at playhead", reason: props.addReason, hint: "Add a caption at the playhead" },
  { id: "split" as const, label: "Split at playhead", reason: props.splitReason, hint: "Split the caption at the playhead" },
]);

function run(id: "import" | "add" | "split"): void {
  if (id === "import") emit("import");
  else if (id === "add") emit("add");
  else emit("split");
}
</script>

<template>
  <div class="flex flex-wrap gap-1">
    <button
      v-for="action in actions"
      :key="action.id"
      type="button"
      :data-testid="`caption-${action.id}`"
      :disabled="action.reason !== null"
      :title="action.reason ?? action.hint"
      class="rounded border border-line px-2 py-0.5 text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-50"
      @click="run(action.id)"
    >
      {{ action.label }}
    </button>
  </div>
  <label class="flex items-center gap-1">
    <input
      v-model="replace"
      data-testid="caption-import-replace"
      type="checkbox"
      class="accent-violet-500"
    >
    Replace this clip's captions on import
  </label>
</template>
