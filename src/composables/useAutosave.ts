import { getCurrentInstance, onBeforeUnmount, ref } from "vue";

import { logWarning } from "../logging";
import { useSettingsStatusStore } from "../stores/settingsStatus";

// Debounce window for typed fields; a blur/flush or a toggle bypasses it.
const DEBOUNCE_MS = 600;

/**
 * Wraps an async save fn with the mechanics every auto-saving settings field
 * needs, so no card re-implements them:
 * - `schedule()` debounces typed input (rapid keystrokes collapse to one save);
 * - `flush()` runs a *pending* debounced save now (bind to @focusout — a blur
 *   with nothing scheduled is a no-op, so an unchanged field doesn't re-save);
 * - `saveNow()` runs immediately (toggles/selects);
 * - in-flight serialization: a trigger while a save is running does NOT start a
 *   second concurrent write — it coalesces into ONE trailing run that re-reads
 *   the latest values (the McpSettings saving-guard lesson, generalized);
 * - status reporting to the shared settingsStatus store + an inline `error` ref;
 * - a flush on beforeUnmount so leaving the settings view never drops a queued
 *   write.
 *
 * `save` builds the payload from live refs and invokes; it must reject on
 * failure. `label` names the component in the warning log line.
 */
export function useAutosave(save: () => Promise<void>, opts: { label?: string } = {}) {
  const status = useSettingsStatusStore();
  const error = ref<string | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let running = false; // an invoke is awaiting
  let pending = false; // a save was requested mid-flight → run once more

  function clearTimer() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  async function run() {
    clearTimer();
    if (running) {
      // Coalesce: don't start a second concurrent write; mark a trailing run.
      pending = true;
      return;
    }
    running = true;
    status.saving();
    error.value = null;
    try {
      await save();
      status.saved();
    } catch (e) {
      const message = String(e);
      error.value = message;
      status.failed(message);
      logWarning(`${opts.label ?? "settings"} autosave failed: ${message}`);
    } finally {
      running = false;
      if (pending) {
        // A trailing run for the edit(s) made mid-flight, with latest values.
        pending = false;
        void run();
      }
    }
  }

  function schedule() {
    clearTimer();
    timer = setTimeout(() => void run(), DEBOUNCE_MS);
  }
  function flush() {
    if (timer !== null) void run(); // run() clears the timer
  }
  function saveNow() {
    void run();
  }

  // A pending debounced save must not die with the component when the settings
  // view navigates away (ActionPanel v-if-unmounts the settings component).
  if (getCurrentInstance()) {
    onBeforeUnmount(() => {
      if (timer !== null) void run();
    });
  }

  return { schedule, flush, saveNow, error };
}
