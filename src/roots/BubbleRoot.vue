<script setup lang="ts">
import { watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import SpeechBubble from "../components/SpeechBubble.vue";
import { useGreeting } from "../composables/useGreeting";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";

// The bubble window is shown by Rust on launch; useGreeting drives the text
// and the auto-dismiss timer. When it dismisses, hide the window.
const { bubbleVisible, bubbleText } = useGreeting();
useSuppressContextMenu();

watch(bubbleVisible, (visible) => {
  if (!visible) void invoke("close_bubble").catch(() => {});
});
</script>

<template>
  <div class="flex h-screen w-screen items-center p-2">
    <SpeechBubble :text="bubbleText" side="right" valign="down" />
  </div>
</template>
