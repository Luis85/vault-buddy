<script setup lang="ts">
/**
 * One toast (visual-parity Task 6). Split out of `NotificationHost.vue` so
 * its panel and editor variants share ONE copy of the item markup — role,
 * aria-live, the action button and the dismiss `svg` — instead of two
 * near-identical template blocks (a clone the quality ratchet catches, and
 * needless complexity in the host's own template). Only the chrome
 * (container/action/dismiss classes) differs by `variant`; the behaviour
 * (`run`/`dismiss`) is identical either way.
 */
import { computed } from "vue";

import type { Notification } from "../stores/notifications";

const props = defineProps<{ item: Notification; variant: "panel" | "editor" }>();
defineEmits<{ (e: "run"): void; (e: "dismiss"): void }>();

// Solid, high-contrast panel backgrounds — NOT the old low-alpha tints
// (bg-red-500/20 etc.). The panel window is transparent, so a ~15-20% tint
// left the toast text barely legible ("not readable due to its
// transparency"). An opaque background makes each toast readable regardless
// of what shows through the panel behind it. The editor toast (concept-spec
// §9.12) has no per-kind colour at all — one neutral `raised` card.
const PANEL_CARD: Record<string, string> = {
  error: "bg-red-900 text-red-50 ring-1 ring-red-500/50",
  warning: "bg-amber-900 text-amber-50 ring-1 ring-amber-500/50",
  success: "bg-emerald-900 text-emerald-50 ring-1 ring-emerald-500/50",
  info: "bg-slate-800 text-slate-100 ring-1 ring-white/15",
};
const EDITOR_CARD =
  "pointer-events-auto flex max-w-[min(650px,90vw)] items-start gap-2 rounded-lg border border-line bg-raised px-[17px] py-[11px] text-xs text-fg";
const EDITOR_ACTION =
  "shrink-0 cursor-pointer rounded bg-hover px-1.5 py-0.5 font-medium text-fg hover:bg-hover-subtle focus:outline-none focus-visible:ring-2 focus-visible:ring-focus";
const EDITOR_DISMISS =
  "shrink-0 cursor-pointer rounded p-0.5 text-fg-muted hover:bg-hover hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus";
const PANEL_ACTION =
  "shrink-0 cursor-pointer rounded bg-white/15 px-1.5 py-0.5 font-medium hover:bg-white/25 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40";
const PANEL_DISMISS =
  "shrink-0 cursor-pointer rounded p-0.5 hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-white/40";

const cardClass = computed(() =>
  props.variant === "editor"
    ? EDITOR_CARD
    : ["pointer-events-auto flex items-start justify-between gap-2 rounded-control px-2 py-1 text-xs shadow-lg", PANEL_CARD[props.item.kind]],
);
const actionClass = computed(() => (props.variant === "editor" ? EDITOR_ACTION : PANEL_ACTION));
const dismissClass = computed(() => (props.variant === "editor" ? EDITOR_DISMISS : PANEL_DISMISS));
</script>

<template>
  <div
    data-testid="notification"
    :role="item.kind === 'error' ? 'alert' : 'status'"
    :aria-live="item.kind === 'error' ? 'assertive' : 'polite'"
    :class="cardClass"
  >
    <span class="min-w-0 break-words">{{ item.message }}</span>
    <button
      v-if="item.action"
      type="button"
      data-testid="notification-action"
      :class="actionClass"
      @click="$emit('run')"
    >
      {{ item.action.label }}
    </button>
    <button
      type="button"
      data-testid="notification-dismiss"
      aria-label="Dismiss"
      :class="dismissClass"
      @click="$emit('dismiss')"
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
</template>
