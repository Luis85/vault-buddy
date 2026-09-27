<script setup lang="ts">
import { logWarning } from "../logging";
import type { Notification } from "../stores/notifications";
import { useNotificationsStore } from "../stores/notifications";
import NotificationItem from "./NotificationItem.vue";

// `variant="editor"` (visual-parity Task 6, concept-spec §9.12) restyles the
// toast for the tutorial editor window — fixed, centred, bottom 43px, a
// single `raised` card, no low-alpha panel-window kind colours — WITHOUT
// touching the panel window's own look, which every existing caller (and
// `notification-host.test.ts`) keeps testing with no `variant` at all.
// Two template branches, not one class map switched by variant: the panel
// branch is left byte-for-byte so nothing here can leak into it by accident.
// `NotificationItem` carries the one copy of an item's own markup either
// branch renders, so this file stays a thin container/positioning switch.
withDefaults(defineProps<{ variant?: "panel" | "editor" }>(), { variant: "panel" });

const notifications = useNotificationsStore();

// Run a toast's call-to-action (e.g. "Open" the imported note), then dismiss
// it — the action resolves the toast, so it shouldn't linger afterwards. A
// failed action (the note's vault was removed, the OS can't launch the
// obsidian:// handler) must not vanish the toast silently: report it, or the
// user loses the only feedback with no sign Open failed.
async function runAction(item: Notification) {
  try {
    await item.action?.run();
  } catch (e) {
    logWarning(`notification action "${item.action?.label ?? ""}" failed: ${String(e)}`);
    notifications.error(`Couldn't ${item.action?.label ?? "complete that action"}: ${String(e)}`);
  } finally {
    notifications.dismiss(item.id);
  }
}
</script>
<template>
  <div
    v-if="notifications.items.length && variant === 'panel'"
    data-testid="notification-host"
    class="pointer-events-none absolute inset-x-3 bottom-3 z-10 flex flex-col gap-1"
  >
    <NotificationItem
      v-for="item in notifications.items"
      :key="item.id"
      :item="item"
      variant="panel"
      @run="runAction(item)"
      @dismiss="notifications.dismiss(item.id)"
    />
  </div>

  <!-- concept-spec §9.12: fixed, centred, bottom 43px, `raised`, radius 8,
       11/17 padding, 12px text, sliding up 15px over .16s — the global
       `prefers-reduced-motion` rule in style.css already zeroes that
       transition, so there is nothing extra to gate here. -->
  <TransitionGroup
    v-else-if="notifications.items.length"
    tag="div"
    name="vb-toast"
    data-testid="notification-host"
    class="pointer-events-none fixed inset-x-0 bottom-[43px] z-50 flex flex-col items-center gap-2"
  >
    <NotificationItem
      v-for="item in notifications.items"
      :key="item.id"
      :item="item"
      variant="editor"
      @run="runAction(item)"
      @dismiss="notifications.dismiss(item.id)"
    />
  </TransitionGroup>
</template>

<style scoped>
.vb-toast-enter-active,
.vb-toast-leave-active {
  transition:
    opacity 0.16s ease,
    transform 0.16s ease;
}
.vb-toast-enter-from,
.vb-toast-leave-to {
  opacity: 0;
  transform: translateY(15px);
}
</style>
