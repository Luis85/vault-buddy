<script setup lang="ts">
import { computed, onMounted, onUnmounted, watch } from "vue";
import { storeToRefs } from "pinia";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { listen } from "@tauri-apps/api/event";
import CompanionCharacter from "./components/CompanionCharacter.vue";
import ActionPanel from "./components/ActionPanel.vue";
import SpeechBubble from "./components/SpeechBubble.vue";
import { useCompanionWindow } from "./composables/useCompanionWindow";
import { useVaultsStore } from "./stores/vaults";
import { useSettingsStore } from "./stores/settings";
import { useGreeting } from "./composables/useGreeting";

const store = useVaultsStore();
const settings = useSettingsStore();
const { panelOpen, busyVaultId } = storeToRefs(store);
const working = computed(() => busyVaultId.value !== null);

const { bubbleVisible, bubbleText, dismiss } = useGreeting();
const { side, valign } = useCompanionWindow(panelOpen, bubbleVisible);

// Opening the panel supersedes the greeting: cancel its timer and hide it
// so it can't reappear when the panel closes again within the greeting
// window. (The bubble is also hidden by v-if while the panel is open.)
watch(panelOpen, (open) => {
  if (open) dismiss();
});

// Dragging the buddy enters the OS window-move loop, which steals focus
// from the webview. Closing the panel on that focus loss would resize the
// window mid-drag — Windows then cancels the drag and the buddy lands at
// the collapsed window's origin (the panel's old top-left corner). The
// drag causes exactly ONE focus loss, so suppress only the first blur in
// the window after a drag begins: a second blur is the user clicking the
// desktop, and with the window already unfocused no later focus event
// would ever close the panel.
const DRAG_CLOSE_SUPPRESS_MS = 500;
let dragStartedAt = 0;
let dragBlurPending = false;

function onDragStart() {
  dragStartedAt = Date.now();
  dragBlurPending = true;
}

function dragJustStarted() {
  return Date.now() - dragStartedAt < DRAG_CLOSE_SUPPRESS_MS;
}

function closePanel() {
  if (store.panelOpen) void store.togglePanel();
}

function closePanelUnlessDragging() {
  if (!dragJustStarted()) closePanel();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closePanel();
}

// The stock WebView context menu (Refresh, etc.) breaks the desktop-widget
// illusion. Suppress it everywhere except text fields, where the native
// copy/paste menu stays useful. The buddy shows its own native menu.
function onContextMenu(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (!target?.closest("input, textarea")) event.preventDefault();
}

let unlistenFocus: (() => void) | undefined;
let unlistenAnimation: (() => void) | undefined;
let unlistenDragging: (() => void) | undefined;

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("contextmenu", onContextMenu);
  try {
    // The buddy's native right-click menu toggles these from the Rust side.
    unlistenAnimation = await listen("buddy-toggle-animation", () => {
      settings.toggleAnimations();
    });
    unlistenDragging = await listen("buddy-toggle-dragging", () => {
      settings.toggleDragging();
    });
  } catch {
    // not running under Tauri (unit tests)
  }
  try {
    // Clicking the desktop takes focus off the companion — close the panel
    // so the transparent window shrinks out of the way.
    unlistenFocus = await getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        if (focused) {
          // the window is back in front — the drag (if any) is over
          dragBlurPending = false;
          return;
        }
        if (dragBlurPending && dragJustStarted()) {
          dragBlurPending = false; // consumed the drag's own blur
          return;
        }
        closePanel();
      },
    );
  } catch {
    // not running under Tauri (unit tests)
  }
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("contextmenu", onContextMenu);
  unlistenFocus?.();
  unlistenAnimation?.();
  unlistenDragging?.();
});
</script>

<template>
  <!--
    The buddy lives in a fixed cell with the exact collapsed-window size
    (88x88). When the window grows by the size delta and the layout
    mirrors, the cell lands precisely where the collapsed window was, so
    the character never visibly moves — the placement offset in
    useCompanionWindow assumes exactly this geometry.
  -->
  <!--
    Clicks that land on <main> or the panel wrapper itself hit the invisible
    gutter of the expanded window — the user believes they clicked the
    desktop, so close the panel and get out of the way.
  -->
  <main
    class="flex h-screen w-screen"
    :class="[
      side === 'left' ? 'flex-row-reverse' : 'flex-row',
      valign === 'up' ? 'items-end' : 'items-start',
    ]"
    @click.self="closePanelUnlessDragging"
  >
    <div
      data-testid="buddy-cell"
      class="flex h-[88px] w-[88px] shrink-0 items-start justify-start p-2"
    >
      <CompanionCharacter
        :working="working"
        :animated="settings.animationsEnabled"
        :character="settings.character"
        :draggable="settings.draggingEnabled"
        :facing="settings.facing"
        @toggle="store.togglePanel()"
        @drag-start="onDragStart"
      />
    </div>
    <Transition name="bubble-fade">
      <div
        v-if="bubbleVisible && !panelOpen"
        class="flex min-w-0 flex-1 items-center self-stretch p-2"
      >
        <SpeechBubble :text="bubbleText" :side="side" :valign="valign" />
      </div>
    </Transition>
    <div
      v-if="panelOpen"
      class="min-w-0 flex-1 self-stretch p-2"
      @click.self="closePanelUnlessDragging"
    >
      <ActionPanel />
    </div>
  </main>
</template>

<style scoped>
/* The greeting fades in on launch and out on dismiss (design spec). Short
   enough that the buddy still feels responsive. */
.bubble-fade-enter-active,
.bubble-fade-leave-active {
  transition: opacity 150ms ease;
}
.bubble-fade-enter-from,
.bubble-fade-leave-to {
  opacity: 0;
}
</style>
