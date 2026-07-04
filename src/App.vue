<script setup lang="ts">
import { computed, onMounted, onUnmounted } from "vue";
import { storeToRefs } from "pinia";
import { getCurrentWindow } from "@tauri-apps/api/window";
import CompanionCharacter from "./components/CompanionCharacter.vue";
import ActionPanel from "./components/ActionPanel.vue";
import { useCompanionWindow } from "./composables/useCompanionWindow";
import { useVaultsStore } from "./stores/vaults";

const store = useVaultsStore();
const { panelOpen, busyVaultId } = storeToRefs(store);
const working = computed(() => busyVaultId.value !== null);

const { side, valign } = useCompanionWindow(panelOpen);

function closePanel() {
  if (store.panelOpen) void store.togglePanel();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closePanel();
}

let unlistenFocus: (() => void) | undefined;

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  try {
    // Clicking the desktop takes focus off the companion — close the panel
    // so the transparent window shrinks out of the way.
    unlistenFocus = await getCurrentWindow().onFocusChanged(
      ({ payload: focused }) => {
        if (!focused) closePanel();
      },
    );
  } catch {
    // not running under Tauri (unit tests)
  }
});

onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  unlistenFocus?.();
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
    @click.self="closePanel"
  >
    <div
      data-testid="buddy-cell"
      class="flex h-[88px] w-[88px] shrink-0 items-start justify-start p-2"
    >
      <CompanionCharacter :working="working" @toggle="store.togglePanel()" />
    </div>
    <div
      v-if="panelOpen"
      class="min-w-0 flex-1 self-stretch p-2"
      @click.self="closePanel"
    >
      <ActionPanel />
    </div>
  </main>
</template>
