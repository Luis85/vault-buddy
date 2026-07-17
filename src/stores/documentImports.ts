import { invoke } from "@tauri-apps/api/core";
import { defineStore } from "pinia";

import { basename } from "../utils/basename";

/** The single in-flight document conversion. The Rust-side ImportLock caps
 * the process at one conversion, so one slot — not a list — is the honest
 * model. */
interface ActiveImport {
  /** Basename of the source document, for display. */
  fileName: string;
  /** The full source path, so the import picker can tell whether the running
   * conversion is its own queue head or one started from RecordMode. */
  sourcePath: string;
  vaultId: string;
  vaultName: string;
  startedAtMs: number;
}

// Owns the convert_document lifecycle so every surface shows the SAME
// working state (ImportProgress renders it on the intake views and the list
// view) and no surface can strand a stale "converting" flag — the picker and
// record-mode each hand-rolled a local busy ref before this store existed.
// Panel-webview only, which is where all import UI lives; the state survives
// the panel being hidden/shown because the webview is never unmounted.
export const useDocumentImportsStore = defineStore("documentImports", {
  state: () => ({
    active: null as ActiveImport | null,
  }),
  actions: {
    /** Run one conversion: set `active`, invoke, ALWAYS clear in `finally`,
     * return the new note's path / rethrow the raw IPC error — callers keep
     * their own toast + navigation behavior. A concurrent second call is
     * rejected up front with the same message the Rust ImportLock would
     * return, WITHOUT touching `active`: only the first conversion's
     * `finally` may clear the slot, or a same-tick double-trigger would blank
     * the working card while Pandoc still runs. Thrown as a plain string to
     * match the IPC rejection shape callers already `String(e)`. */
    async convert(
      vault: { id: string; name: string },
      sourcePath: string,
    ): Promise<string> {
      if (this.active) throw "An import is already in progress.";
      this.active = {
        fileName: basename(sourcePath),
        sourcePath,
        vaultId: vault.id,
        vaultName: vault.name,
        startedAtMs: Date.now(),
      };
      try {
        return await invoke<string>("convert_document", {
          id: vault.id,
          sourcePath,
        });
      } finally {
        this.active = null;
      }
    },
  },
});
