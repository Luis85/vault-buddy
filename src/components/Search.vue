<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { announce } from "../announce";
import { noteOpenedMessage } from "../buddyMessages";
import { useNotificationsStore } from "../stores/notifications";
import { highlightParts } from "../utils/highlight";
import type { SearchHit, SearchResponse } from "../types";

const notifications = useNotificationsStore();

// Mirrors core::search::MIN_QUERY_CHARS — the backend refuses shorter
// queries anyway; gating here saves the IPC round-trip and drives the hint.
const MIN_QUERY_CHARS = 2;
const DEBOUNCE_MS = 300;

const query = ref("");
const hits = ref<SearchHit[]>([]);
const truncated = ref(false);
const searching = ref(false);
const error = ref<string | null>(null);
// The query the current results answer — drives highlights and the empty
// state; the live input may already be ahead of it while a search is in
// flight.
const resultsQuery = ref("");
const inputEl = ref<HTMLInputElement | null>(null);

let timer: ReturnType<typeof setTimeout> | undefined;
// Monotonic ticket: a resolving search that is no longer the latest is
// dropped, so a slow older response can never overwrite newer results.
let ticket = 0;

const tooShort = computed(() => query.value.trim().length < MIN_QUERY_CHARS);

// Flat hits → per-vault groups, preserving the backend's vault order.
const groups = computed(() => {
  const map = new Map<string, { vaultName: string; hits: SearchHit[] }>();
  for (const h of hits.value) {
    const group = map.get(h.vaultId);
    if (group) group.hits.push(h);
    else map.set(h.vaultId, { vaultName: h.vaultName, hits: [h] });
  }
  return [...map.entries()].map(([vaultId, g]) => ({ vaultId, ...g }));
});

watch(query, () => {
  if (timer) clearTimeout(timer);
  const trimmed = query.value.trim();
  if (trimmed.length < MIN_QUERY_CHARS) {
    // Invalidate any in-flight response too — its results answer a query
    // that no longer exists.
    ticket++;
    searching.value = false;
    hits.value = [];
    truncated.value = false;
    error.value = null;
    resultsQuery.value = "";
    return;
  }
  timer = setTimeout(() => void runSearch(trimmed), DEBOUNCE_MS);
});

async function runSearch(trimmed: string) {
  const mine = ++ticket;
  searching.value = true;
  try {
    const response = await invoke<SearchResponse>("search_vaults", {
      query: trimmed,
    });
    if (mine !== ticket) return; // stale — a newer search superseded this one
    hits.value = response.hits;
    truncated.value = response.truncated;
    resultsQuery.value = trimmed;
    error.value = null;
  } catch (e) {
    if (mine !== ticket) return;
    // Keep the previous results up — a live refinement that errors must not
    // blank a working list (mirrors the vaults store's refresh behavior).
    error.value = String(e);
    logWarning(`search_vaults failed: ${String(e)}`);
  } finally {
    if (mine === ticket) searching.value = false;
  }
}

async function openHit(hit: SearchHit) {
  try {
    await invoke("open_search_result", { id: hit.vaultId, file: hit.file });
    // Same acknowledgement pattern as vault/daily-note opens (the panel
    // window is the announcer for opens); a failed open stays silent — the
    // toast is the feedback there.
    announce(noteOpenedMessage(hit.name));
    void invoke("close_panel").catch(() => {});
  } catch (e) {
    notifications.error(String(e));
    logWarning(`open_search_result failed for ${hit.file}: ${String(e)}`);
  }
}

function onEscape(event: KeyboardEvent) {
  if (query.value) {
    // First Escape clears the query; a second one bubbles up to PanelRoot
    // and closes the panel (same pattern as the vault filter).
    query.value = "";
    event.stopPropagation();
  }
}

onMounted(() => inputEl.value?.focus());
onUnmounted(() => {
  if (timer) clearTimeout(timer);
});
</script>

<template>
  <div class="flex flex-col gap-2">
    <input
      ref="inputEl"
      v-model="query"
      data-testid="search-input"
      type="search"
      placeholder="Search all vaults…"
      aria-label="Search all vaults"
      class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      @keydown.escape="onEscape"
    />
    <p
      v-if="error"
      class="rounded-lg bg-red-500/20 px-2 py-1 text-xs text-red-200"
    >
      {{ error }}
    </p>
    <p v-if="tooShort" class="text-xs text-slate-400">
      Type at least {{ MIN_QUERY_CHARS }} characters to search.
    </p>
    <p v-else-if="searching && hits.length === 0" class="text-xs text-slate-400">
      Searching…
    </p>
    <p
      v-else-if="hits.length === 0 && resultsQuery"
      class="text-xs text-slate-400"
    >
      No matches for "{{ resultsQuery }}".
    </p>
    <div
      v-for="group in groups"
      :key="group.vaultId"
      class="flex flex-col gap-1"
    >
      <h2 class="text-xs font-semibold uppercase tracking-wide text-slate-400">
        {{ group.vaultName }}
      </h2>
      <button
        v-for="hitItem in group.hits"
        :key="hitItem.file"
        type="button"
        data-testid="search-hit"
        class="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="openHit(hitItem)"
      >
        <span
          class="w-full truncate text-sm text-slate-100"
          :title="hitItem.name"
        >
          <template
            v-for="(part, i) in highlightParts(hitItem.name, resultsQuery)"
            :key="i"
          >
            <mark v-if="part.match" class="rounded bg-violet-500/40 text-inherit">{{ part.text }}</mark>
            <template v-else>{{ part.text }}</template>
          </template>
        </span>
        <span v-if="hitItem.folder" class="w-full truncate text-xs text-slate-500">
          {{ hitItem.folder }}
        </span>
        <span v-if="hitItem.snippet" class="w-full truncate text-xs text-slate-400">
          <template
            v-for="(part, i) in highlightParts(hitItem.snippet, resultsQuery)"
            :key="i"
          >
            <mark v-if="part.match" class="rounded bg-violet-500/40 text-inherit">{{ part.text }}</mark>
            <template v-else>{{ part.text }}</template>
          </template>
        </span>
      </button>
    </div>
    <p
      v-if="truncated"
      data-testid="search-truncated"
      class="text-xs text-slate-500"
    >
      Showing the first {{ hits.length }} matches — refine your query.
    </p>
  </div>
</template>
