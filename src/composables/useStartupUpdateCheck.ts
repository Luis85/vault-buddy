import { onMounted, onUnmounted } from "vue";

import { announce } from "../announce";
import { updateAvailableMessage } from "../buddyMessages";
import { useSettingsStore } from "../stores/settings";
import { useUpdatesStore } from "../stores/updates";
import { useVaultsStore } from "../stores/vaults";

/**
 * With autostart the check can race login networking, and a quiet check
 * fails silently — so wait for the network to settle rather than wasting
 * the session's one shot in the first second after boot.
 */
export const STARTUP_CHECK_DELAY_MS = 15_000;

/**
 * The "Check for updates on start" feature: a quiet update check shortly
 * after launch. Installed by PanelRoot ONLY — the panel webview mounts
 * hidden exactly once at app start, owns the updates store the settings
 * view reads, and is already an announcer (vault opens), so the check runs
 * once and its result is visible where the Install button lives.
 *
 * When an update exists the buddy asks: a bubble announcement (announce()
 * respects the Buddy-messages toggle) and the NEXT panel open lands on the
 * settings view — `requestViewOnNextOpen`, never a live-view yank. When the
 * app is current, or the check fails, nothing is shown at all.
 */
export function useStartupUpdateCheck(): void {
  const settings = useSettingsStore();
  const updates = useUpdatesStore();
  const vaults = useVaultsStore();
  let timer: ReturnType<typeof setTimeout> | undefined;

  onMounted(() => {
    if (!settings.checkUpdatesOnStart) return;
    timer = setTimeout(() => {
      void updates.checkForUpdatesQuietly().then(() => {
        if (updates.phase !== "available") return;
        announce(updateAvailableMessage(updates.available?.version ?? ""));
        vaults.requestViewOnNextOpen("settings");
      });
    }, STARTUP_CHECK_DELAY_MS);
  });
  onUnmounted(() => clearTimeout(timer));
}
