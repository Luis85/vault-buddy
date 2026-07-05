import { invoke } from "@tauri-apps/api/core";
import { useSettingsStore } from "./stores/settings";

/**
 * Ask the buddy to speak `text` in its bubble — but only if the user hasn't
 * turned Buddy messages off. Routes through Rust's `announce` command, which
 * shows/positions the bubble window beside the buddy and pushes the text to it.
 * Best-effort: the bubble is a nicety, so a failed IPC (or no Tauri under
 * tests) is swallowed. Call sites live in the single announcer per event — the
 * buddy window for capture progress, the panel window's vaults store for opens.
 */
export function announce(text: string): void {
  if (!useSettingsStore().buddyMessagesEnabled) return;
  void invoke("announce", { text }).catch(() => {});
}
