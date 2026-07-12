import { invoke } from "@tauri-apps/api/core";
import type { Ref } from "vue";

import { logWarning } from "../logging";

// The optional per-vault folder fields' load/save pair (tasks + documents
// folders), extracted from CaptureSettings.vue: each folder keeps its own
// command pair and saves with the form's single Save button as an independent
// invoke, so one folder's failure can't block the capture save or the other
// folder's, and its errors stay field-level.
export function useOptionalFolderField(vaultId: () => string) {
  // Load a folder field off the capture form's critical path: a failure warns
  // and continues (the folder is optional), and the resolved value is dropped
  // if the user already started typing (their edit owns the field — the same
  // rule as RecordMode's pre-load toggle guard). `onPersisted` reports the
  // PERSISTED value even when an edit owns the input — the tasks folder uses
  // it to seed the lists-card reload baseline, which must track disk, not the
  // draft. `onLoaded` hands back the whole parsed config for a command that
  // carries MORE than the one folder field (documents also carries the
  // date-folders toggle) — the caller gates its own extra ref the same way,
  // so a late-resolving load can't clobber an edit made before it returned.
  // One options object (not positional params): the field count already sits
  // at ESLint's max-params ceiling.
  async function loadOptionalField<T>(opts: {
    cmd: string;
    editedRef: Ref<boolean>;
    loadedRef: Ref<boolean>;
    targetRef: Ref<string>;
    extract: (cfg: T) => string | null;
    onPersisted?: (value: string) => void;
    onLoaded?: (cfg: T) => void;
  }) {
    const { cmd, editedRef, loadedRef, targetRef, extract, onPersisted, onLoaded } = opts;
    try {
      const cfg = await invoke<T>(cmd, { id: vaultId() });
      const persisted = extract(cfg) ?? "";
      if (!editedRef.value) targetRef.value = persisted;
      loadedRef.value = true;
      onPersisted?.(persisted);
      onLoaded?.(cfg);
    } catch (e) {
      logWarning(`${cmd} failed (vault ${vaultId()}): ${String(e)}`);
    }
  }

  // Save a folder field through its own command. Gated on loaded-or-edited: a
  // value that is neither is the default seed, and writing it would clear the
  // vault's real folder. A failure is a field-level error (returned true so
  // the caller can withhold the "Saved ✓") — deliberately NOT short-circuited
  // by the capture-config save, so neither write can block the other's.
  // `extra` merges additional fields into the same invoke (documents also
  // saves its date-folders toggle through this command) — gated by the SAME
  // loaded/edited check as the folder, since both come from the one command's
  // response, so one flag correctly covers either field being unresolved.
  async function saveOptionalField(opts: {
    cmd: string;
    key: string;
    value: string;
    loaded: boolean;
    edited: boolean;
    errorRef: Ref<string | null>;
    extra?: Record<string, unknown>;
  }): Promise<boolean> {
    const { cmd, key, value, loaded, edited, errorRef, extra = {} } = opts;
    if (!loaded && !edited) return false;
    const trimmed = value.trim();
    try {
      await invoke(cmd, { id: vaultId(), [key]: trimmed === "" ? null : trimmed, ...extra });
      return false;
    } catch (e) {
      errorRef.value = String(e);
      logWarning(`${cmd} failed (vault ${vaultId()}): ${String(e)}`);
      return true;
    }
  }

  return { loadOptionalField, saveOptionalField };
}
