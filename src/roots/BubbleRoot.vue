<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted, ref, watch } from "vue";

import SpeechBubble from "../components/SpeechBubble.vue";
import { BUBBLE_MS,useBuddyBubble } from "../composables/useBuddyBubble";
import { useSettingsStorageSync } from "../composables/useSettingsStorageSync";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";
import { useSettingsStore } from "../stores/settings";

// The bubble window is shown by Rust (startup greeting, or `announce` for an
// acknowledgement); useBuddyBubble owns the current text + auto-dismiss timer.
// When it dismisses, hide the window.
const { visible, text, action, show, dismiss } = useBuddyBubble();
useSuppressContextMenu();

// A bubble that carries an action is clickable: route the action to Rust,
// then dismiss. Best-effort like every other bubble command. `open_panel`
// idempotently reveals the panel (safe whether it is open or closed); its
// panel-shown refresh consumes the armed pending view — for the update
// announcement, the dedicated update view.
function onBubbleActivate() {
  if (action.value === "openUpdate") {
    void invoke("open_panel").catch(() => {});
  }
  dismiss();
}

// Duration changes are made in the panel window; this webview only hears
// about them via the storage event, so it must install the sync like every
// other settings-reading root — and resolve the tier at each show, never at
// listener-registration time.
const settings = useSettingsStore();
useSettingsStorageSync();

// Which side of the buddy the bubble sits on and how its tail aligns — Rust
// decides this when it places the window (side derived from the buddy's screen
// position, edge-flip, and vertical clamp) and pushes it via `bubble-anchor`.
// Default to `right`/`middle`; the anchor event lands before the bubble shows.
const side = ref<"left" | "right">("right");
// `middle` is the resting case (bubble centered level with the buddy); the
// anchor event switches it to `top`/`bottom` only near a screen edge.
const valign = ref<"top" | "middle" | "bottom">("middle");

let unlistenAnchor: (() => void) | undefined;
let unlistenMessage: (() => void) | undefined;
let unlistenPanelShown: (() => void) | undefined;

watch(visible, (isVisible) => {
  if (!isVisible) void invoke("close_bubble").catch(() => {});
});

onMounted(async () => {
  // Pull the current anchor first: the bubble webview is hidden until the
  // greeting shows, so it can register the listener below only AFTER Rust's
  // startup anchor emits have fired — leaving the tail on its default until a
  // drag re-emits. Pulling on mount closes that race.
  try {
    const anchor = await invoke<{
      side: "left" | "right";
      valign: "top" | "middle" | "bottom";
    }>("get_bubble_anchor");
    side.value = anchor.side;
    valign.value = anchor.valign;
  } catch {
    // not under Tauri (unit tests) — keep the defaults
  }
  try {
    unlistenAnchor = await listen<{
      side: "left" | "right";
      valign: "top" | "middle" | "bottom";
    }>("bubble-anchor", (event) => {
      side.value = event.payload.side;
      valign.value = event.payload.valign;
    });
    // An acknowledgement the buddy should speak: the announcer (buddy/panel
    // window) called `announce`, Rust showed + positioned this window and
    // emitted the text here. Latest-wins replaces any lingering message.
    unlistenMessage = await listen<{ text: string; action?: string | null }>(
      "bubble-message",
      (event) =>
        show(
          event.payload.text,
          BUBBLE_MS[settings.messageDuration].ack,
          event.payload.action ?? null,
        ),
    );
    // The panel opens beside the buddy, over the bubble's spot — dismiss a
    // lingering bubble so the two never overlap.
    unlistenPanelShown = await listen("panel-shown", () => dismiss());
  } catch {
    // not under Tauri (unit tests without the event mock)
  }
});
onUnmounted(() => {
  unlistenAnchor?.();
  unlistenMessage?.();
  unlistenPanelShown?.();
});
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
    <SpeechBubble
      :text="text"
      :side="side"
      :valign="valign"
      :clickable="!!action"
      @activate="onBubbleActivate"
    />
  </div>
</template>
