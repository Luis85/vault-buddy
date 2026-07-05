<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import CompanionCharacter from "../components/CompanionCharacter.vue";
import { useSettingsStore } from "../stores/settings";
import { useCaptureStore } from "../stores/capture";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";
import { useSettingsStorageSync } from "../composables/useSettingsStorageSync";
import type { Facing } from "../stores/settings";

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

// The buddy looks toward the screen center; its facing is DERIVED from its
// position by Rust, not a stored setting. Read the initial value on mount, then
// let the `buddy-facing` event flip it when a drag carries the buddy across the
// screen midline.
const facing = ref<Facing>("right");

let unlistenAnimation: (() => void) | undefined;
let unlistenDragging: (() => void) | undefined;
let unlistenFacing: (() => void) | undefined;

onMounted(async () => {
  void capture.init();
  try {
    const initial = await invoke<string>("get_buddy_facing");
    facing.value = initial === "left" ? "left" : "right";
  } catch {
    // not under Tauri (tests) — keep the default
  }
  try {
    unlistenAnimation = await listen("buddy-toggle-animation", () =>
      settings.toggleAnimations(),
    );
    unlistenDragging = await listen("buddy-toggle-dragging", () =>
      settings.toggleDragging(),
    );
    unlistenFacing = await listen<string>("buddy-facing", (event) => {
      facing.value = event.payload === "left" ? "left" : "right";
    });
  } catch {
    // not under Tauri (tests)
  }
});
onUnmounted(() => {
  unlistenAnimation?.();
  unlistenDragging?.();
  unlistenFacing?.();
});
</script>

<template>
  <div class="flex h-screen w-screen items-start justify-start p-2">
    <CompanionCharacter
      :working="false"
      :animated="settings.animationsEnabled"
      :character="settings.character"
      :draggable="settings.draggingEnabled"
      :facing="facing"
      :recording="capture.status === 'recording' || capture.status === 'saving'"
      :paused="capture.paused"
      @toggle="onToggle"
      @drag-start="onDragStart"
    />
  </div>
</template>
