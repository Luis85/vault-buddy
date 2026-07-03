import { watch, type Ref } from "vue";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

export const COLLAPSED = { width: 140, height: 170 };
export const EXPANDED = { width: 440, height: 340 };

/**
 * Grows the transparent window when the panel opens and shrinks it back when
 * it closes, so the invisible window never blocks clicks on the desktop
 * beneath it.
 */
export function useCompanionWindow(panelOpen: Ref<boolean>): void {
  watch(panelOpen, async (open) => {
    const size = open ? EXPANDED : COLLAPSED;
    await getCurrentWindow().setSize(new LogicalSize(size.width, size.height));
  });
}
