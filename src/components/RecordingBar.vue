<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import { formatDuration } from "../utils/formatDuration";

const props = defineProps<{
  startedAtMs: number | null;
  saving: boolean;
  starting: boolean;
  warning: string | null;
  paused: boolean;
  pausedTotalMs: number;
  pausedSinceMs: number | null;
  level: number;
}>();
defineEmits<{ (e: "stop"): void; (e: "pause"): void; (e: "resume"): void }>();

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;
onMounted(() => {
  timer = setInterval(() => (now.value = Date.now()), 1000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

// Wall time minus accumulated pauses (and the still-open span while
// paused) — the display freezes during a pause and never counts the gap.
const elapsed = computed(() => {
  if (props.startedAtMs === null) return "0:00";
  const openPause =
    props.paused && props.pausedSinceMs !== null
      ? now.value - props.pausedSinceMs
      : 0;
  return formatDuration(
    now.value - props.startedAtMs - props.pausedTotalMs - openPause,
  );
});

const label = computed(() => {
  if (props.starting) return "Starting…";
  if (props.saving) return "Saving…";
  if (props.paused) return `Paused ${elapsed.value}`;
  return `Recording ${elapsed.value}`;
});

const meterWidth = computed(
  () => `${Math.round(Math.min(1, Math.max(0, props.level)) * 100)}%`,
);
</script>

<template>
  <div
    class="rounded-lg px-2 py-1.5"
    :class="paused ? 'bg-amber-500/15' : 'bg-red-500/15'"
  >
    <div class="flex items-center gap-2">
      <span
        class="h-2.5 w-2.5 shrink-0 rounded-full"
        :class="paused ? 'bg-amber-400' : 'animate-pulse bg-red-500'"
        aria-hidden="true"
      />
      <span
        class="flex-1 text-sm font-medium"
        :class="paused ? 'text-amber-100' : 'text-red-100'"
        role="status"
      >
        {{ label }}
      </span>
      <button
        v-if="!paused"
        type="button"
        class="cursor-pointer rounded-lg bg-white/10 px-2 py-1 text-xs font-semibold text-white hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Pause recording"
        :disabled="saving || starting"
        @click="$emit('pause')"
      >
        ⏸ Pause
      </button>
      <button
        v-else
        type="button"
        class="cursor-pointer rounded-lg bg-white/10 px-2 py-1 text-xs font-semibold text-white hover:bg-white/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-amber-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Resume recording"
        :disabled="saving || starting"
        @click="$emit('resume')"
      >
        ▶ Resume
      </button>
      <button
        type="button"
        class="cursor-pointer rounded-lg bg-red-500/80 px-2 py-1 text-xs font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Stop recording"
        :disabled="saving || starting"
        @click="$emit('stop')"
      >
        ⏹ Stop
      </button>
    </div>
    <div
      v-if="!starting"
      class="mt-1.5 h-1 overflow-hidden rounded-full bg-white/10"
      aria-hidden="true"
    >
      <div
        data-testid="level-meter"
        class="h-full rounded-full bg-emerald-400 transition-[width] duration-100"
        :style="{ width: meterWidth }"
      />
    </div>
    <p
      v-if="warning"
      class="mt-1 text-xs text-amber-200"
    >
      {{ warning }}
    </p>
  </div>
</template>
