<script setup lang="ts">
import { computed, ref, watch } from "vue";

const props = defineProps<{ savedMp3: string; error: string | null }>();
const emit = defineEmits<{
  (e: "accept", title: string): void;
}>();

// Prefill with the saved base (file name without .mp3) so confirming
// unedited is a no-op and edits can start from the real name. The
// backend strips the duplicated timestamp prefix, so editing the tail
// of the full base is safe too.
const baseName = computed(() => {
  const name = props.savedMp3.split(/[\\/]/).pop() ?? "";
  return name.replace(/\.mp3$/i, "");
});
const title = ref(baseName.value);
watch(baseName, (value) => (title.value = value));
</script>

<template>
  <form
    class="rounded-lg bg-emerald-500/10 px-2 py-1.5"
    @submit.prevent="emit('accept', title)"
  >
    <label
      class="text-xs font-medium text-emerald-200"
      for="rename-input"
    >
      Saved ✓ — name this recording?
    </label>
    <div class="mt-1 flex items-center gap-1">
      <input
        id="rename-input"
        v-model="title"
        type="text"
        aria-label="Recording name"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
      >
      <button
        type="submit"
        class="cursor-pointer rounded-lg bg-emerald-600/80 px-2 py-1 text-xs font-semibold text-white hover:bg-emerald-600 focus:outline-none focus-visible:ring-2 focus-visible:ring-emerald-300"
      >
        Accept
      </button>
    </div>
    <p
      v-if="error"
      class="mt-1 text-xs text-red-300"
    >
      {{ error }}
    </p>
  </form>
</template>
