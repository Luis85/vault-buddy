<script setup lang="ts">
defineProps<{ working: boolean }>();
defineEmits<{ (e: "toggle"): void }>();
</script>

<template>
  <div class="flex flex-col items-center">
    <div
      data-tauri-drag-region
      class="cursor-move px-2 py-1 text-xs leading-none text-slate-400"
      title="Drag to move Vault Buddy"
      aria-hidden="true"
    >
      ⠿
    </div>
    <button
      type="button"
      class="buddy block focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      :class="{ working }"
      aria-label="Vault Buddy — click to open the panel"
      @click="$emit('toggle')"
    >
      <svg width="96" height="96" viewBox="0 0 96 96" aria-hidden="true">
        <ellipse cx="48" cy="52" rx="34" ry="32" fill="#7c5cff" />
        <circle class="eye" cx="38" cy="46" r="5" fill="#fff" />
        <circle class="eye" cx="58" cy="46" r="5" fill="#fff" />
        <path
          d="M40 62 Q48 70 56 62"
          stroke="#fff"
          stroke-width="3"
          fill="none"
          stroke-linecap="round"
        />
      </svg>
    </button>
  </div>
</template>

<style scoped>
/* idle */
.buddy {
  animation: bob 3s ease-in-out infinite;
}
/* greeting */
.buddy:hover:not(.working) {
  animation: wiggle 0.6s ease-in-out infinite;
}
/* working */
.buddy.working {
  animation: pulse 0.9s ease-in-out infinite;
}
.buddy .eye {
  animation: blink 4s infinite;
  transform-origin: center;
  transform-box: fill-box;
}
@keyframes bob {
  0%,
  100% {
    transform: translateY(0);
  }
  50% {
    transform: translateY(-4px);
  }
}
@keyframes wiggle {
  0%,
  100% {
    transform: rotate(-4deg);
  }
  50% {
    transform: rotate(4deg);
  }
}
@keyframes pulse {
  0%,
  100% {
    transform: scale(1);
  }
  50% {
    transform: scale(0.94);
  }
}
@keyframes blink {
  0%,
  92%,
  100% {
    transform: scaleY(1);
  }
  96% {
    transform: scaleY(0.1);
  }
}
</style>
