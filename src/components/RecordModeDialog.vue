<script setup lang="ts">
import { onMounted, ref, type ComponentPublicInstance } from "vue";

const props = defineProps<{
  vaultName: string;
  defaultMode: "meeting" | "voice-note";
}>();
const emit = defineEmits<{
  (e: "start", mode: "meeting" | "voice-note"): void;
  (e: "browse"): void;
  (e: "cancel"): void;
}>();

const OPTIONS = [
  {
    key: "meeting",
    title: "Meeting",
    hint: "Microphone + desktop audio",
    testId: "mode-meeting",
    ariaLabel: "Start a meeting recording",
  },
  {
    key: "voice-note",
    title: "Voice Note",
    hint: "Microphone only",
    testId: "mode-voice-note",
    ariaLabel: "Start a voice note recording",
  },
] as const;

// The default option gets keyboard focus immediately — the whole point of
// the modal is a fast confirm-or-switch, so the common case (confirm the
// default) should be reachable with a single Enter press. A dynamic :ref
// inside v-for can't use the static-name auto-binding script setup gets
// for a literal ref="…", so this callback captures the matching element.
const defaultButton = ref<HTMLButtonElement | null>(null);
function captureDefaultRef(key: "meeting" | "voice-note") {
  return (el: Element | ComponentPublicInstance | null) => {
    if (key === props.defaultMode) defaultButton.value = el as HTMLButtonElement | null;
  };
}
onMounted(() => defaultButton.value?.focus());
</script>

<template>
  <div
    role="dialog"
    aria-modal="true"
    aria-label="Choose recording mode"
    class="absolute inset-0 z-10 flex items-center justify-center rounded-2xl bg-slate-950/60"
    @click.self="emit('cancel')"
    @keydown.escape.stop.prevent="emit('cancel')"
  >
    <div
      class="w-64 rounded-xl border border-white/10 bg-slate-900 p-3 shadow-lg"
    >
      <div class="mb-2 flex items-center justify-between">
        <h2 class="text-sm font-semibold text-slate-100">
          Record in {{ props.vaultName }}
        </h2>
        <button
          type="button"
          aria-label="Cancel recording"
          class="cursor-pointer rounded-lg p-1 text-slate-400 transition-colors hover:bg-white/10 hover:text-slate-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="emit('cancel')"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            aria-hidden="true"
          >
            <path d="M18 6 6 18M6 6l12 12" />
          </svg>
        </button>
      </div>
      <div class="flex flex-col gap-2">
        <button
          v-for="option in OPTIONS"
          :key="option.key"
          :ref="captureDefaultRef(option.key)"
          type="button"
          :data-testid="option.testId"
          :aria-label="option.ariaLabel"
          class="w-full cursor-pointer rounded-lg border px-3 py-2 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            option.key === props.defaultMode
              ? 'border-violet-400 bg-violet-500/20'
              : 'border-white/10 bg-white/5 hover:bg-white/10'
          "
          @click="emit('start', option.key)"
        >
          <span class="block text-sm font-medium text-slate-100">
            {{ option.title }}
          </span>
          <span class="block text-xs text-slate-400">{{ option.hint }}</span>
        </button>
      </div>
      <button
        type="button"
        data-testid="mode-browse"
        aria-label="Browse past recordings"
        class="mt-2 w-full cursor-pointer border-t border-white/10 pt-2 text-left text-xs text-slate-400 transition-colors hover:text-slate-200 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="emit('browse')"
      >
        Browse recordings…
        <span class="block text-slate-500">See past recordings in this vault</span>
      </button>
    </div>
  </div>
</template>
