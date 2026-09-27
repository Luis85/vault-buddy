/**
 * A vault's NAME for display (design D6, visual-parity Task 21): the Render
 * dialog shows where a publish goes by the name Obsidian lists, never by the
 * registry id. Read through the port's `list_vaults` whenever the id
 * changes. A list that cannot be read, or that no longer names the vault,
 * reads as the role wording "the capture's vault" — never the id, never a
 * path. No id at all is `null`: the caller says a vault is not chosen yet.
 */
import { computed, ref, watch } from "vue";

import { logWarning } from "../logging";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";

const UNKNOWN_VAULT = "the capture's vault";

export function useVaultName(vaultId: () => string) {
  const editorProject = useEditorProjectStore();
  const found = ref<string | null>(null);
  let ticket = 0;

  async function load(id: string): Promise<void> {
    const mine = ++ticket;
    found.value = null;
    if (!id) return;
    try {
      const vaults = await editorProject.port.listVaults();
      if (mine === ticket) found.value = vaults.find((v) => v.id === id)?.name ?? null;
    } catch (e) {
      // The role wording below stands in. Logged by code and operationId,
      // never by message (S-15).
      const failure = toEditorError(e);
      logWarning(`editor: the vault list could not be read: ${failure.code} (${failure.operationId})`);
    }
  }

  watch(vaultId, (id) => void load(id), { immediate: true });

  return computed<string | null>(() => (vaultId() ? (found.value ?? UNKNOWN_VAULT) : null));
}
