<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import ActionPanel from "../components/ActionPanel.vue";
import { useVaultsStore } from "../stores/vaults";

const store = useVaultsStore();

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

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  // Re-run discovery on mount so a user who just launched Obsidian sees a
  // fresh list when the panel window appears.
  void store.refresh();
});
onUnmounted(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="h-screen w-screen p-2" @click="onGutterClick">
    <ActionPanel />
  </div>
</template>
