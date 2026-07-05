import { onMounted, onUnmounted } from "vue";

// The stock WebView2 right-click menu (Reload, Back, Save as…) shatters the
// desktop-widget illusion, so every window suppresses it — except over text
// fields, where the copy/paste menu stays useful. This lived in the deleted
// App.vue; each window root now installs it. Window-level (not element-level)
// so it also covers each window's transparent gutter, not just the card.
export function useSuppressContextMenu() {
  function onContextMenu(event: MouseEvent) {
    const target = event.target as HTMLElement | null;
    if (!target?.closest("input, textarea")) event.preventDefault();
  }
  onMounted(() => window.addEventListener("contextmenu", onContextMenu));
  onUnmounted(() => window.removeEventListener("contextmenu", onContextMenu));
}
