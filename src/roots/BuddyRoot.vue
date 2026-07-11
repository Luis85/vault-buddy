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
const isSupportedDoc = (p: string) => {
  const ext = p.split(".").pop()?.toLowerCase();
  return ext ? SUPPORTED_DOC_EXTENSIONS.includes(ext) : false;
};

// True while a droppable document hovers over the buddy — drives the
// character's drop-target highlight so the drop visibly "registers" before
// the user lets go. Only a SUPPORTED doc lights it up, matching what the drop
// handler will actually act on (an unsupported drop is silently ignored).
const dragActive = ref(false);

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
      const payload = event.payload;
      if (payload.type === "drop") {
        dragActive.value = false; // the hover is over either way
        const path = payload.paths.find(isSupportedDoc);
        if (!path) return; // unsupported drop — ignore
        invokeQuiet("begin_document_import", { path });
        return;
      }
      // `enter` carries the paths; `over` repeats with position only, so the
      // enter-time verdict is held until the drag leaves or drops. Highlight
      // only when a SUPPORTED doc is present — the same files the drop acts on.
      if (payload.type === "enter" || payload.type === "over") {
        if ("paths" in payload) dragActive.value = payload.paths.some(isSupportedDoc);
        return;
      }
      dragActive.value = false; // leave / cancel — the hover ended
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
      :drop-target="dragActive"
      @toggle="onToggle"
      @drag-start="onDragStart"
    />
  </div>
</template>
