<script setup lang="ts">
import { onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import CompanionCharacter from "../components/CompanionCharacter.vue";
import { useSettingsStore } from "../stores/settings";
import { useCaptureStore } from "../stores/capture";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";
import { useSettingsStorageSync } from "../composables/useSettingsStorageSync";

const settings = useSettingsStore();
const capture = useCaptureStore();
useSuppressContextMenu();
useSettingsStorageSync();

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

// Mirror the buddy's facing to Rust so the greeting bubble opens on the side
// the buddy faces. The buddy window is the single owner of this push: every
// facing change (buddy menu or panel settings) funnels through the settings
// store, so watching it here covers them all. `immediate` also pushes the
// initial value on mount.
watch(
  () => settings.facing,
  (facing) => invokeQuiet("set_buddy_facing", { facing }),
  { immediate: true },
);

onMounted(async () => {
  void capture.init();
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
