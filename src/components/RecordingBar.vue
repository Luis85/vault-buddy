<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

const props = defineProps<{
  startedAtMs: number | null;
  saving: boolean;
  warning: string | null;
}>();
defineEmits<{ (e: "stop"): void }>();

const now = ref(Date.now());
let timer: ReturnType<typeof setInterval> | null = null;
onMounted(() => {
  timer = setInterval(() => (now.value = Date.now()), 1000);
});
onBeforeUnmount(() => {
  if (timer) clearInterval(timer);
});

const elapsed = computed(() => {
  if (props.startedAtMs === null) return "0:00";
  const total = Math.max(0, Math.floor((now.value - props.startedAtMs) / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  return h > 0
    ? `${h}:${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}`
    : `${m}:${String(s).padStart(2, "0")}`;
});
</script>

<template>
  <div class="rounded-lg bg-red-500/15 px-2 py-1.5">
    <div class="flex items-center gap-2">
      <span
        class="h-2.5 w-2.5 shrink-0 animate-pulse rounded-full bg-red-500"
        aria-hidden="true"
      ></span>
      <span class="flex-1 text-sm font-medium text-red-100" role="status">
        {{ saving ? "Saving…" : `Recording ${elapsed}` }}
      </span>
      <button
        type="button"
        class="cursor-pointer rounded-lg bg-red-500/80 px-2 py-1 text-xs font-semibold text-white hover:bg-red-500 focus:outline-none focus-visible:ring-2 focus-visible:ring-red-300 disabled:cursor-default disabled:opacity-50"
        aria-label="Stop recording"
        :disabled="saving"
        @click="$emit('stop')"
      >
        ⏹ Stop
      </button>
    </div>
    <p v-if="warning" class="mt-1 text-xs text-amber-200">{{ warning }}</p>
  </div>
</template>
