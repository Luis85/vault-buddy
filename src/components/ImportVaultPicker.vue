<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onMounted, ref, watch } from "vue";

import { logWarning } from "../logging";
import { useDocumentImportsStore } from "../stores/documentImports";
import { useNotificationsStore } from "../stores/notifications";
import { usePandocStore } from "../stores/pandoc";
import { useVaultsStore } from "../stores/vaults";
import { basename } from "../utils/basename";
import { withDialogSuppressed } from "../utils/nativeDialog";
import ImportProgress from "./ImportProgress.vue";

// Two ways in, one mode split on the queue. (1) A buddy drag-drop: Rust
// stashes the dropped path (begin_document_import) and shows the panel; the
// panel's refresh() consumes it (take_pending_import) into
// store.pendingImports and lands here — the queue HEAD is the file. (2) The
// buddy-menu "Import document…" request (begin_add_document →
// take_add_document_request): the queue is EMPTY, so picking a vault opens
// the OS file picker for the file AFTER the vault choice (vault-first mode).
// Drop extensions (.docx/.odt/.rtf) are validated by BuddyRoot's drop
// handler; the vault-first dialog filters to the same set — either way this
// view only picks the destination vault and gates on Pandoc, same convert
// contract RecordMode's "Import Document" action uses.
const store = useVaultsStore();
const notifications = useNotificationsStore();
const pandocStore = usePandocStore();
// The conversion lifecycle lives in the shared documentImports store (not a
// local busy ref) so the working state renders identically here, in
// RecordMode, and on the list view — and survives leaving this view.
const documentImports = useDocumentImportsStore();
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

// While converting: how many queue entries are NOT covered by the running
// conversion. The head is "covered" only when the picker itself started it
// (sourcePath matches); a conversion started from RecordMode leaves the whole
// queue — head included — waiting for a vault pick.
const waitingCount = computed(() => {
  const head = store.pendingImports[0];
  const headIsConverting =
    head !== undefined && documentImports.active?.sourcePath === head;
  return Math.max(0, store.pendingImports.length - (headIsConverting ? 1 : 0));
});
// ONE queue indicator for both body states (folded into a computed — the
// template's branch count is what the complexity gate measures): while
// converting, the entries the running conversion doesn't cover; otherwise the
// entries behind the head the header line is asking about. "" = hidden.
const queueLabel = computed(() => {
  if (documentImports.active) {
    return waitingCount.value > 0 ? `+${waitingCount.value} more queued` : "";
  }
  return queuedMore.value > 0 ? `(+${queuedMore.value} more queued)` : "";
});

// One discriminated state drives the body, so the template branches on a
// single value instead of scattered `checking` / `gate.blocked` / length
// booleans (which pushed the render function past the complexity gate).
// `converting` outranks everything: once a conversion runs, the pick decision
// is made and the working card replaces the (inert) list — a grayed-out list
// under a one-line hint was the "working state not clear enough" complaint.
const viewState = computed<
  "converting" | "checking" | "blocked" | "empty" | "list"
>(() => {
  if (documentImports.active) return "converting";
  if (pandocStore.checking) return "checking";
  if (gate.value.blocked) return "blocked";
  if (store.vaults.length === 0) return "empty";
  return "list";
});

// Same filter idiom as the vault list (ActionPanel.vue): only offer it once
// scanning stops working, match name+path, and let Escape clear-then-close.
// Gated on viewState === "list" (ActionPanel's showFilter bakes in the same
// check against its own `view`) — unlike the vault list, this component has
// other live states (checking/blocked/empty/converting) with no `<ul>` under
// them, and an ungated filter would float above those with nothing to filter.
const filter = ref("");
const FILTER_THRESHOLD = 5;
const showFilter = computed(
  () => viewState.value === "list" && store.vaults.length > FILTER_THRESHOLD,
);
const filteredVaults = computed(() => {
  const query = filter.value.trim().toLowerCase();
  if (!query) return store.vaults;
  return store.vaults.filter(
    (v) => v.name.toLowerCase().includes(query) || v.path.toLowerCase().includes(query),
  );
});
function onFilterEscape(event: KeyboardEvent) {
  if (event.isComposing) return; // GAP-31: IME Escape must not clear
  if (filter.value) {
    filter.value = "";
    event.stopPropagation();
  }
}
// Reset the query whenever the panel is re-shown, so a stale filter can't
// strand the picker (the ActionPanel precedent).
watch(() => store.shownNonce, () => (filter.value = ""));

// Vault-first mode's file prompt, shown only after the vault choice. Wrapped
// in withDialogSuppressed so the OS dialog stealing focus can't trip the
// panel's focus-out auto-hide. null = cancelled.
async function pickSourceFile(): Promise<string | null> {
  const picked = await withDialogSuppressed(() =>
    open({
      multiple: false,
      filters: [{ name: "Documents", extensions: ["docx", "odt", "rtf"] }],
    }),
  );
  return typeof picked === "string" ? picked : null;
}

async function pick(vaultId: string) {
  // Drop mode converts the head of the queue (a mid-conversion drop appends
  // to the tail — GAP-55 — so afterward we drop the head and either advance
  // to the next queued document or return to the list); vault-first mode
  // (empty queue) asks for the file now that the vault is chosen.
  if (documentImports.active) return;
  // Capture the queue epoch before the (slow) conversion: if the user backs out
  // — and maybe re-drops the same path — before it resolves, the epoch moves on
  // and the completion must not consume a queue entry or navigate (Codex P2).
  // The vault-first flow shares the guard via settleAddImport.
  const epoch = store.importEpoch;
  const vault = store.vaults.find((v) => v.id === vaultId);
  try {
    // Remember WHERE the source came from: only a queue-sourced conversion
    // may consume the queue head on success. A dialog-sourced (vault-first)
    // one must not — a document dropped mid-conversion lands in the queue
    // and dequeueImport's shift would silently eat it (Codex PR #63).
    const queuedSource = store.pendingImports[0];
    const source = queuedSource ?? (await pickSourceFile());
    if (source === null) return; // cancelled file picker — stay on the picker
    const notePath = await documentImports.convert(
      { id: vaultId, name: vault?.name ?? "" },
      source,
    );
    // Offer to open the freshly-imported note in the vault it landed in, rather
    // than leaving the user to hunt for it after the picker returns to the list.
    notifications.notify("success", `Imported ${basename(notePath)}`, {
      action: {
        label: "Open in Obsidian",
        run: () => invoke("open_imported_document", { id: vaultId, path: notePath }),
      },
    });
    if (queuedSource !== undefined) store.dequeueImport(epoch);
    else store.settleAddImport(epoch);
  } catch (e) {
    // Stay on the picker (queue head unchanged) so the user can retry a
    // different vault for this same document.
    logWarning(`import picker: convert_document failed: ${String(e)}`);
    notifications.error(`Couldn't import document: ${String(e)}`);
  }
}
</script>

<template>
  <div class="flex flex-col gap-2">
    <!-- Converting: the working card replaces the header (the pick is made)
         — only the queue tail still needs communicating. -->
    <ImportProgress v-if="viewState === 'converting'" />
    <p
      v-else
      class="text-xs text-slate-400"
    >
      Import
      <span
        v-if="sourceName"
        class="font-medium text-slate-200"
      >{{ sourceName }}</span>
      <!-- v-else costs no template-complexity branch, unlike a second v-if -->
      <template v-else>
        a document
      </template>
      into which vault?
    </p>
    <p
      v-if="queueLabel"
      data-testid="import-picker-queued"
      class="text-xs text-slate-500"
    >
      {{ queueLabel }}
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
    <input
      v-if="showFilter"
      v-model="filter"
      type="search"
      placeholder="Filter vaults…"
      aria-label="Filter vaults"
      data-testid="import-picker-filter"
      class="mb-2 w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-white/20 focus:outline-none"
      @keydown.escape="onFilterEscape"
    >
    <ul
      v-if="viewState === 'list'"
      class="space-y-1"
    >
      <li
        v-for="vault in filteredVaults"
        :key="vault.id"
      >
        <button
          type="button"
          data-testid="import-picker-vault"
          class="flex w-full cursor-pointer items-center gap-2 rounded-lg border border-white/10 bg-white/5 px-3 py-2 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          @click="pick(vault.id)"
        >
          <span class="min-w-0 flex-1 truncate text-sm font-medium text-slate-100">
            {{ vault.name }}
          </span>
        </button>
      </li>
    </ul>
  </div>
</template>
