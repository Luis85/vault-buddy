<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { usePandocStore } from "../stores/pandoc";
import { useVaultsStore } from "../stores/vaults";
import { basename } from "../utils/basename";

// Reached only via a buddy drag-drop: Rust stashes the dropped path
// (begin_document_import) and shows the panel; the panel's refresh()
// consumes it (take_pending_import) into store.pendingImports and lands
// here. The extension (.docx/.odt/.rtf) is already validated by the drop
// handler in BuddyRoot, so this view only needs to pick the destination
// vault and gate on Pandoc — same convert_document contract RecordMode's
// "Import Document" action uses.
const store = useVaultsStore();
const notifications = useNotificationsStore();
const pandocStore = usePandocStore();

const busyVaultId = ref<string | null>(null);
// The shared pandoc store owns detection: the picker reuses a cached
// "installed" result instead of re-probing on every drop, and
// `pandocStore.checking` gates the pre-probe window so a quick drop-then-click
// can't flash the install gate / route to Settings before the probe settles.

onMounted(() => {
  void pandocStore.ensureDetected();
});

const gate = computed(() => {
  if (!pandocStore.status?.installed) {
    return {
      blocked: true,
      hint: "Pandoc isn't installed — install it to import documents.",
    };
  }
  if (!pandocStore.status.sandboxSupported) {
    return {
      blocked: true,
      hint: "Your Pandoc is too old for safe import (need 2.15+).",
    };
  }
  return { blocked: false, hint: "" };
});

const sourceName = computed(() => {
  const path = store.pendingImports[0];
  return path ? basename(path) : "";
});
const queuedMore = computed(() => Math.max(0, store.pendingImports.length - 1));

// One discriminated state drives the body, so the template branches on a
// single value instead of scattered `checking` / `gate.blocked` / length
// booleans (which pushed the render function past the complexity gate).
const viewState = computed<"checking" | "blocked" | "empty" | "list">(() => {
  if (pandocStore.checking) return "checking";
  if (gate.value.blocked) return "blocked";
  if (store.vaults.length === 0) return "empty";
  return "list";
});

async function pick(vaultId: string) {
  // Convert the head of the queue; a mid-conversion drop appends to the tail
  // (GAP-55), so afterward we drop the head and either advance to the next
  // queued document or return to the list.
  const source = store.pendingImports[0];
  if (busyVaultId.value || !source) return;
  // Capture the queue epoch before the (slow) conversion: if the user backs out
  // — and maybe re-drops the same path — before it resolves, the epoch moves on
  // and dequeueImport must not consume the wrong entry or navigate (Codex P2).
  const epoch = store.importEpoch;
  busyVaultId.value = vaultId;
  try {
    const notePath = await invoke<string>("convert_document", {
      id: vaultId,
      sourcePath: source,
    });
    // Offer to open the freshly-imported note in the vault it landed in, rather
    // than leaving the user to hunt for it after the picker returns to the list.
    notifications.notify("success", `Imported ${basename(notePath)}`, {
      action: {
        label: "Open in Obsidian",
        run: () => invoke("open_imported_document", { id: vaultId, path: notePath }),
      },
    });
    store.dequeueImport(epoch);
  } catch (e) {
    // Stay on the picker (queue head unchanged) so the user can retry a
    // different vault for this same document.
    logWarning(`import picker: convert_document failed: ${String(e)}`);
    notifications.error(`Couldn't import document: ${String(e)}`);
  } finally {
    busyVaultId.value = null;
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <p
      v-if="busyVaultId"
      data-testid="import-picker-converting"
      class="text-xs text-slate-400"
    >
      Converting <span class="font-medium text-slate-200">{{ sourceName }}</span>… this can
      take a few seconds.
    </p>
    <p
      v-else
      class="text-xs text-slate-400"
    >
      Import
      <span
        v-if="sourceName"
        class="font-medium text-slate-200"
      >{{ sourceName }}</span>
      into which vault?
      <span
        v-if="queuedMore > 0"
        data-testid="import-picker-queued"
        class="text-slate-500"
      >(+{{ queuedMore }} more queued)</span>
    </p>
    <p
      v-if="viewState === 'checking'"
      data-testid="import-picker-checking"
      class="text-xs text-slate-400"
    >
      Checking Pandoc…
    </p>
    <div
      v-else-if="viewState === 'blocked'"
      class="flex flex-col gap-2 rounded-lg border border-white/10 bg-white/5 p-2"
    >
      <p
        data-testid="import-picker-gate-hint"
        class="text-xs text-slate-300"
      >
        {{ gate.hint }}
      </p>
      <button
        type="button"
        data-testid="import-picker-settings"
        class="w-fit cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-300 hover:bg-white/10"
        @click="store.openDocumentImport()"
      >
        Set up Pandoc
      </button>
    </div>
    <p
      v-else-if="viewState === 'empty'"
      class="text-xs text-slate-400"
    >
      No vaults found.
    </p>
    <ul
      v-else
      class="space-y-1"
    >
      <li
        v-for="vault in store.vaults"
        :key="vault.id"
      >
        <button
          type="button"
          data-testid="import-picker-vault"
          class="flex w-full cursor-pointer items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-50"
          :disabled="busyVaultId !== null"
          @click="pick(vault.id)"
        >
          <span class="min-w-0 flex-1 truncate text-sm font-medium text-slate-100">
            {{ vault.name }}
          </span>
          <span
            v-if="busyVaultId === vault.id"
            class="h-4 w-4 shrink-0 animate-spin rounded-full border-2 border-white/30 border-t-white"
            role="status"
            aria-label="Importing…"
          />
        </button>
      </li>
    </ul>
  </div>
</template>
