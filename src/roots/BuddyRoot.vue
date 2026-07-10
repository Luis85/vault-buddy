<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { computed, onMounted, onUnmounted, ref } from "vue";

import CompanionCharacter from "../components/CompanionCharacter.vue";
import { useBuddyAnnouncements } from "../composables/useBuddyAnnouncements";
import { useSettingsStorageSync } from "../composables/useSettingsStorageSync";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";
import { useCaptureStore } from "../stores/capture";
import type { Facing } from "../stores/settings";
import { useSettingsStore } from "../stores/settings";

const settings = useSettingsStore();
const capture = useCaptureStore();
useSuppressContextMenu();
useSettingsStorageSync();
// The buddy window is the single announcer for capture-driven progress
// (recording/transcription); the panel window announces its own vault/note
// opens. Keeping capture announcements here avoids double bubbles.
useBuddyAnnouncements();

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
// The buddy's "working" pulse while a job occupies the transcription queue —
// derived from the per-job map (there's no more singular `transcribing` flag).
const transcribing = computed(() => capture.activeTranscription !== null);

// Document extensions convert_document accepts (Pandoc readers): a drop of
// anything else is ignored rather than routed to the picker, where it would
// only fail at convert_document with a confusing error.
const SUPPORTED_DOC_EXTENSIONS = ["docx", "odt", "rtf"];

let unlistenAnimation: (() => void) | undefined;
let unlistenDragging: (() => void) | undefined;
let unlistenFacing: (() => void) | undefined;
let unlistenDrop: (() => void) | undefined;

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
  try {
    // A document dropped on the buddy: stash the path Rust-side and show
    // the panel on the import picker (see begin_document_import). No
    // toggle_panel and no event emit here — the buddy and panel windows
    // have separate Pinia stores, so the buddy can't set the panel's view
    // directly, and toggle_panel would HIDE an already-open panel instead
    // of routing it to the picker.
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const path = event.payload.paths.find((p) => {
        const ext = p.split(".").pop()?.toLowerCase();
        return ext ? SUPPORTED_DOC_EXTENSIONS.includes(ext) : false;
      });
      if (!path) return; // unsupported drop — ignore
      invokeQuiet("begin_document_import", { path });
    });
  } catch {
    // not under Tauri (tests)
  }
});
onUnmounted(() => {
  unlistenAnimation?.();
  unlistenDragging?.();
  unlistenFacing?.();
  unlistenDrop?.();
});
</script>

<template>
  <!-- Center the character in the window. Windows clamps this tiny borderless
       window up to its minimum size (wider/taller than the ~64px character), so
       top-left anchoring left all the slack on one side — and the placement math
       (bubble tuck, VMode::Center, the facing midline) assumes the character is
       centered in the window. Centering makes those assumptions hold. -->
  <div class="flex h-screen w-screen items-center justify-center">
    <CompanionCharacter
      :working="transcribing"
      :animated="settings.animationsEnabled"
      :character="settings.character"
      :draggable="settings.draggingEnabled"
      :facing="facing"
      :recording="capture.status === 'recording' || capture.status === 'saving'"
      :paused="capture.paused"
      :transcribing="transcribing"
      @toggle="onToggle"
      @drag-start="onDragStart"
    />
  </div>
</template>
