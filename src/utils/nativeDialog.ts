import { invoke } from "@tauri-apps/api/core";

import { logWarning } from "../logging";

/**
 * Run a native OS dialog (`tauri-plugin-dialog` `open()`) with the panel's
 * focus-out auto-hide suppressed for its lifetime. A native picker steals OS
 * focus, which the panel's focus-out check would read as "clicked away" and
 * hide the panel — taking any in-progress import state (the `Converting…` line
 * and the success/error toast, both rendered in the panel window) out of sight
 * once the user picks a file. So we flag a dialog as in-flight in Rust for the
 * duration of the call. Best-effort: a failed suppress invoke (e.g. no Tauri
 * runtime under test) is logged, never fatal — opening the dialog matters more
 * than the flag, and the `finally` always clears it.
 */
export async function withDialogSuppressed<T>(run: () => Promise<T>): Promise<T> {
  try {
    await invoke("set_dialog_active", { active: true });
  } catch (e) {
    logWarning(`set_dialog_active(true) failed: ${String(e)}`);
  }
  try {
    return await run();
  } finally {
    try {
      await invoke("set_dialog_active", { active: false });
    } catch (e) {
      logWarning(`set_dialog_active(false) failed: ${String(e)}`);
    }
  }
}
