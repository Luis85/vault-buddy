import { onMounted, onUnmounted } from "vue";

import { useSettingsStore } from "../stores/settings";

// The buddy and panel are separate webviews that share localStorage but not
// Vue reactivity. A settings change made in one window (the panel's settings
// view, or a tray toggle handled in the buddy window) only reaches the other
// via the `storage` event — so every window that reads settings must re-sync
// on it, or it silently keeps (and can write back) stale values.
export function useSettingsStorageSync() {
  const settings = useSettingsStore();
  const onStorage = () => settings.syncFromStorage();
  onMounted(() => window.addEventListener("storage", onStorage));
  onUnmounted(() => window.removeEventListener("storage", onStorage));
}
