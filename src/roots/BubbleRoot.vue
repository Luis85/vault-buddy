<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SpeechBubble from "../components/SpeechBubble.vue";
import { useGreeting } from "../composables/useGreeting";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";
import { useSettingsStore } from "../stores/settings";

// The bubble window is shown by Rust on launch; useGreeting drives the text
// and the auto-dismiss timer. When it dismisses, hide the window.
const settings = useSettingsStore();
const { bubbleVisible, bubbleText } = useGreeting();
useSuppressContextMenu();

// Which side of the buddy the bubble sits on and how its tail aligns — Rust
// decides this when it places the window (facing preference + screen-edge
// flip) and pushes it via `bubble-anchor`. Default to the buddy's facing side
// so the first paint is right before that event lands.
const side = ref<"left" | "right">(
  settings.facing === "left" ? "left" : "right",
);
const valign = ref<"up" | "down">("down");

let unlistenAnchor: (() => void) | undefined;

watch(bubbleVisible, (visible) => {
  if (!visible) void invoke("close_bubble").catch(() => {});
});

onMounted(async () => {
  try {
    unlistenAnchor = await listen<{
      side: "left" | "right";
      valign: "up" | "down";
    }>("bubble-anchor", (event) => {
      side.value = event.payload.side;
      valign.value = event.payload.valign;
    });
  } catch {
    // not under Tauri (unit tests without the event mock)
  }
});
onUnmounted(() => unlistenAnchor?.());
</script>

<template>
  <!-- Hug the bubble into the corner of the window nearest the buddy so it sits
       against the character, not adrift in the window's dead space: justify
       toward the buddy horizontally, align toward it vertically. The tail then
       points straight at the buddy. -->
  <div
    class="flex h-screen w-screen p-1"
    :class="[
      side === 'right' ? 'justify-start' : 'justify-end',
      valign === 'down' ? 'items-start' : 'items-end',
    ]"
  >
    <SpeechBubble :text="bubbleText" :side="side" :valign="valign" />
  </div>
</template>
