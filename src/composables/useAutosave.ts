import { getCurrentInstance, onBeforeUnmount, ref } from "vue";

import { logWarning } from "../logging";
import { useSettingsStatusStore } from "../stores/settingsStatus";

// Debounce window for typed fields; a blur/flush or a toggle bypasses it.
const DEBOUNCE_MS = 600;

// Stable per-instance id so the shared status store can track each field's
// failure independently — one field's success must not clear another's error.
let nextOwner = 0;

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
  const owner = nextOwner++;
  const error = ref<string | null>(null);
  // Reactive mirror of `running` (which stays true across a coalesced trailing
  // run), so a consumer can fence a conflicting action while a save is in
  // flight — e.g. the Tasks tab disables the folder input while a list save
  // runs, so a folder change can't overlap and land stale prefs on the new root.
  const saving = ref(false);
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
    saving.value = true;
    status.saving(owner);
    error.value = null;
    try {
      await save();
      status.saved(owner);
    } catch (e) {
      const message = String(e);
      error.value = message;
      status.failed(owner, message);
      logWarning(`${opts.label ?? "settings"} autosave failed: ${message}`);
    } finally {
      running = false;
      if (pending) {
        // A trailing run for the edit(s) made mid-flight, with latest values.
        // saving stays true — run() re-enters and sets it again immediately.
        pending = false;
        void run();
      } else {
        saving.value = false;
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

  if (getCurrentInstance()) {
    onBeforeUnmount(() => {
      // A pending debounced save must not die with the component when the
      // settings view navigates away (ActionPanel v-if-unmounts it).
      if (timer !== null) void run();
      // Retire this owner from the shared status so a failure it reported isn't
      // stranded in the header after the component (and its inline error) is
      // gone — the TaskListSettings card unmounts on a folder change, and its
      // remount gets a fresh owner that couldn't otherwise clear the old error.
      status.release(owner);
    });
  }

  return { schedule, flush, saveNow, error, saving };
}
