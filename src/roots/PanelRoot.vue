<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { onMounted, onUnmounted } from "vue";

import ActionPanel from "../components/ActionPanel.vue";
import { useSettingsStorageSync } from "../composables/useSettingsStorageSync";
import { useStartupUpdateCheck } from "../composables/useStartupUpdateCheck";
import { useSuppressContextMenu } from "../composables/useSuppressContextMenu";
import { useCaptureStore } from "../stores/capture";
import { useVaultsStore } from "../stores/vaults";

const store = useVaultsStore();
const capture = useCaptureStore();
useSuppressContextMenu();
useSettingsStorageSync();
// Quiet startup update check (panel window only — mounts once, hidden, and
// owns the updates store the settings view reads).
useStartupUpdateCheck();

function closePanel() {
  void invoke("close_panel").catch(() => {});
}
function onKeydown(event: KeyboardEvent) {
  // GAP-31 follow-up: an IME candidate-cancel Escape must dismiss the
  // candidate, never the panel. This is the window-level chokepoint behind
  // every view's own composing guard (the vault filter, Search, …) — without
  // it a composing Escape bubbles here and closes the whole panel, which is
  // worse than the filter-clearing the original bug caused.
  if (event.isComposing) return;
  if (event.key === "Escape") closePanel();
}
// Clicks on the transparent gutter around the panel card read as "clicked
// away" — close, like the old expanded-window gutter did.
function onGutterClick(event: MouseEvent) {
  if (event.target === event.currentTarget) closePanel();
}

let unlistenShown: (() => void) | undefined;

onMounted(async () => {
  window.addEventListener("keydown", onKeydown);
  // The panel is its own webview with its own capture store; without this the
  // panel never sees capture:* events (dead level meter, pause not reflected,
  // stuck on "saving" after stop, no rename prompt). See BuddyRoot for the
  // buddy window's own copy — each window listens independently.
  void capture.init();
  // The panel window is created once and only shown/hidden thereafter, so
  // onMounted fires a single time — discovering on mount would read
  // obsidian.json while the panel is still hidden and never refresh again.
  // Rust's toggle_panel emits `panel-shown` every time it reveals the panel;
  // that is the precise "opened" signal (unlike focus, which also fires on a
  // mere refocus): re-run discovery and pick the view there. `store.refresh`
  // defaults to the vault list unless a one-shot `requestView` asked
  // otherwise, so a reopen can't clobber a failed-update settings view.
  try {
    unlistenShown = await listen("panel-shown", () => void store.refresh());
  } catch {
    // not under Tauri (unit tests without the event mock)
  }
});
onUnmounted(() => {
  window.removeEventListener("keydown", onKeydown);
  unlistenShown?.();
});
</script>

<template>
  <div
    class="h-screen w-screen p-2"
    @click="onGutterClick"
  >
    <ActionPanel />
  </div>
</template>
