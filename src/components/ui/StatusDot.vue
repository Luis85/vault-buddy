<!-- src/components/ui/StatusDot.vue -->
<script setup lang="ts">
// The 1.5px status dot (VaultList open/recording/transcribing, TaskRow
// priority). Tone colors are literal palette values on purpose: they are
// one-off status hues, and keeping them literal holds existing dot
// assertions (vault-list transcribing dot = bg-violet-400) green.
withDefaults(
  defineProps<{
    tone: "success" | "recording" | "transcribing" | "priority-high" | "priority-low";
    pulse?: boolean;
    title?: string;
  }>(),
  { pulse: false },
);
const TONE: Record<string, string> = {
  success: "bg-emerald-400",
  recording: "bg-red-500",
  transcribing: "bg-violet-400",
  "priority-high": "bg-red-400",
  "priority-low": "bg-slate-500",
};
</script>

<template>
  <span
    class="h-1.5 w-1.5 shrink-0 rounded-full"
    :class="[TONE[tone], pulse ? 'animate-pulse' : '']"
    :title="title"
    aria-hidden="true"
  />
</template>
