<script setup lang="ts">
import { useNotificationsStore } from "../stores/notifications";
const notifications = useNotificationsStore();
// Solid, high-contrast backgrounds — NOT the old low-alpha tints
// (bg-red-500/20 etc.). The panel window is transparent, so a ~15-20% tint
// left the toast text barely legible ("not readable due to its transparency").
// An opaque background makes each toast readable regardless of what shows
// through the panel behind it.
const cls: Record<string, string> = {
  error: "bg-red-900 text-red-50 ring-1 ring-red-500/50",
  warning: "bg-amber-900 text-amber-50 ring-1 ring-amber-500/50",
  success: "bg-emerald-900 text-emerald-50 ring-1 ring-emerald-500/50",
  info: "bg-slate-800 text-slate-100 ring-1 ring-white/15",
};
</script>
<template>
  <div
    v-if="notifications.items.length"
    data-testid="notification-host"
    class="pointer-events-none absolute inset-x-3 bottom-3 z-10 flex flex-col gap-1"
  >
    <div
      v-for="item in notifications.items"
      :key="item.id"
      data-testid="notification"
      :role="item.kind === 'error' ? 'alert' : 'status'"
      :aria-live="item.kind === 'error' ? 'assertive' : 'polite'"
      :class="['pointer-events-auto flex items-start justify-between gap-2 rounded-lg px-2 py-1 text-xs shadow-lg', cls[item.kind]]"
    >
      <span class="min-w-0 break-words">{{ item.message }}</span>
      <button
        type="button"
        data-testid="notification-dismiss"
        aria-label="Dismiss"
        class="shrink-0 cursor-pointer rounded p-0.5 hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40"
        @click="notifications.dismiss(item.id)"
      >
        <svg
          width="10"
          height="10"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="3"
          stroke-linecap="round"
          aria-hidden="true"
        >
          <path d="M18 6 6 18M6 6l12 12" />
        </svg>
      </button>
    </div>
  </div>
</template>
