<script setup lang="ts">
import { onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import SpeechBubble from "../components/SpeechBubble.vue";
import { useGreeting } from "../composables/useGreeting";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";

// The bubble window is shown by Rust on launch; useGreeting drives the text
// and the auto-dismiss timer. When it dismisses, hide the window.
const { bubbleVisible, bubbleText } = useGreeting();
useSuppressContextMenu();

// Which side of the buddy the bubble sits on and how its tail aligns — Rust
// decides this when it places the window (side derived from the buddy's screen
// position, edge-flip, and vertical clamp) and pushes it via `bubble-anchor`.
// Default to `right`/`middle`; the anchor event lands before the bubble shows.
const side = ref<"left" | "right">("right");
// `middle` is the resting case (bubble centered level with the buddy); the
// anchor event switches it to `top`/`bottom` only near a screen edge.
const valign = ref<"top" | "middle" | "bottom">("middle");

let unlistenAnchor: (() => void) | undefined;

watch(bubbleVisible, (visible) => {
  if (!visible) void invoke("close_bubble").catch(() => {});
});

onMounted(async () => {
  try {
    unlistenAnchor = await listen<{
      side: "left" | "right";
      valign: "top" | "middle" | "bottom";
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
  <!-- Hug the bubble toward the buddy so it sits against the character, not
       adrift in the window's dead space: justify toward the buddy horizontally,
       align toward it vertically (middle = centered on the buddy). The tail
       then points straight at the buddy. -->
  <div
    class="flex h-screen w-screen p-1"
    :class="[
      side === 'right' ? 'justify-start' : 'justify-end',
      valign === 'top'
        ? 'items-start'
        : valign === 'bottom'
          ? 'items-end'
          : 'items-center',
    ]"
  >
    <SpeechBubble :text="bubbleText" :side="side" :valign="valign" />
  </div>
</template>
