<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import CompanionCharacter from "../components/CompanionCharacter.vue";
import { useSettingsStore } from "../stores/settings";
import { useCaptureStore } from "../stores/capture";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";

const settings = useSettingsStore();
const capture = useCaptureStore();
useSuppressContextMenu();

function invokeQuiet(cmd: string, args?: Record<string, unknown>) {
  void invoke(cmd, args).catch(() => {
    // not under Tauri (tests) / best-effort window command
  });
}

function onToggle() {
  invokeQuiet("toggle_panel");
}
function onDragStart() {
  // a drag repositions the buddy — get the panel out of the way
  invokeQuiet("close_panel");
}

let unlistenAnimation: (() => void) | undefined;
let unlistenDragging: (() => void) | undefined;

// the buddy and panel are separate webviews sharing localStorage; a
// character/animation change made in the panel's settings view only
// reaches this window via the storage event, not Vue reactivity.
const onStorage = () => settings.syncFromStorage();

onMounted(async () => {
  void capture.init();
  window.addEventListener("storage", onStorage);
  try {
    unlistenAnimation = await listen("buddy-toggle-animation", () =>
      settings.toggleAnimations(),
    );
    unlistenDragging = await listen("buddy-toggle-dragging", () =>
      settings.toggleDragging(),
    );
  } catch {
    // not under Tauri (tests)
  }
});
onUnmounted(() => {
  window.removeEventListener("storage", onStorage);
  unlistenAnimation?.();
  unlistenDragging?.();
});
</script>

<template>
  <div class="flex h-screen w-screen items-start justify-start p-2">
    <CompanionCharacter
      :working="false"
      :animated="settings.animationsEnabled"
      :character="settings.character"
      :draggable="settings.draggingEnabled"
      :facing="settings.facing"
      :recording="capture.status === 'recording' || capture.status === 'saving'"
      :paused="capture.paused"
      @toggle="onToggle"
      @drag-start="onDragStart"
    />
  </div>
</template>
