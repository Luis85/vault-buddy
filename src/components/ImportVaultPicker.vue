<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { PandocStatus } from "../types";

// Reached only via a buddy drag-drop: Rust stashes the dropped path
// (begin_document_import) and shows the panel; the panel's refresh()
// consumes it (take_pending_import) into store.pendingImportPath and lands
// here. The extension (.docx/.odt/.rtf) is already validated by the drop
// handler in BuddyRoot, so this view only needs to pick the destination
// vault and gate on Pandoc — same convert_document contract RecordMode's
// "Import Document" action uses.
const store = useVaultsStore();
const notifications = useNotificationsStore();

const pandoc = ref<PandocStatus | null>(null);
const busyVaultId = ref<string | null>(null);

async function detectPandoc() {
  // Same degrade-to-disabled pattern as RecordMode: a null status (failed
  // read, or no Tauri runtime under test) is treated as "not installed"
  // rather than optimistically letting a convert_document call fail later.
  try {
    pandoc.value = await invoke<PandocStatus>("detect_pandoc");
  } catch (e) {
    logWarning(`import picker: detect_pandoc failed: ${String(e)}`);
  }
}

onMounted(() => {
  void detectPandoc();
});

const gate = computed(() => {
  if (!pandoc.value?.installed) {
    return {
      blocked: true,
      hint: "Pandoc isn't installed — install it to import documents.",
    };
  }
  if (!pandoc.value.sandboxSupported) {
    return {
      blocked: true,
      hint: "Your Pandoc is too old for safe import (need 2.15+).",
    };
  }
  return { blocked: false, hint: "" };
});

const sourceName = computed(() => {
  const path = store.pendingImportPath;
  if (!path) return "";
  return path.split(/[\\/]/).pop() ?? path;
});

async function pick(vaultId: string) {
  if (busyVaultId.value || !store.pendingImportPath) return;
  busyVaultId.value = vaultId;
  try {
    const notePath = await invoke<string>("convert_document", {
      id: vaultId,
      sourcePath: store.pendingImportPath,
    });
    const name = notePath.split(/[\\/]/).pop() ?? notePath;
    notifications.success(`Imported ${name}`);
    store.showList();
  } catch (e) {
    logWarning(`import picker: convert_document failed: ${String(e)}`);
    notifications.error(`Couldn't import document: ${String(e)}`);
  } finally {
    busyVaultId.value = null;
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <p class="text-xs text-slate-400">
      Import
      <span
        v-if="sourceName"
        class="font-medium text-slate-200"
      >{{ sourceName }}</span>
      into which vault?
    </p>
    <div
      v-if="gate.blocked"
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
        @click="store.openSettings()"
      >
        Install Pandoc in Settings
      </button>
    </div>
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
    <p
      v-if="!gate.blocked && store.vaults.length === 0"
      class="text-xs text-slate-400"
    >
      No vaults found.
    </p>
  </div>
</template>
