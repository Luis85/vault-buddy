<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import ActionPanel from "../components/ActionPanel.vue";
import { useVaultsStore } from "../stores/vaults";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";

const store = useVaultsStore();
useSuppressContextMenu();

function closePanel() {
  void invoke("close_panel").catch(() => {});
}
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closePanel();
}
// Clicks on the transparent gutter around the panel card read as "clicked
// away" — close, like the old expanded-window gutter did.
function onGutterClick(event: MouseEvent) {
  if (event.target === event.currentTarget) closePanel();
}

let unlistenFocus: (() => void) | undefined;

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  // Re-run discovery on mount so a user who just launched Obsidian sees a
  // fresh list when the panel window appears.
  void store.refresh();
  // The panel window is created once and only shown/hidden thereafter, so
  // onMounted fires a single time — mount-only refresh leaves the list stale
  // on every re-open. toggle_panel focuses the panel on each open, so a
  // Focused(true) transition is the reliable "became visible again" signal:
  // re-run discovery there too.
  try {
    unlistenFocus = await getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        if (focused) void store.refresh();
      },
    );
  } catch {
    // not under Tauri (unit tests without the window mock)
  }
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  unlistenFocus?.();
});
</script>

<template>
  <div class="h-screen w-screen p-2" @click="onGutterClick">
    <ActionPanel />
  </div>
</template>
