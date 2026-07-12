import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";

import { logWarning } from "../logging";

// Shared load scaffold for a settings tab: a `loading` gate + a `loadError`
// ref, and a `load` that fetches one per-vault config command and applies it.
// A failed read populates `loadError` (the tab renders an inline error and NO
// editable fields, so a seeded default can't be auto-saved over an unread
// value) and logs — never swallowed. Extracted from the Documents/Tasks tabs,
// which shared this try/catch/finally verbatim.
export function useSettingsLoad() {
  const loading = ref(true);
  const loadError = ref<string | null>(null);

  async function load<T>(cmd: string, id: string, apply: (cfg: T) => void) {
    try {
      apply(await invoke<T>(cmd, { id }));
    } catch (e) {
      loadError.value = String(e);
      logWarning(`${cmd} failed (vault ${id}): ${String(e)}`);
    } finally {
      loading.value = false;
    }
  }

  return { loading, loadError, load };
}
