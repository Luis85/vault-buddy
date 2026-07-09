<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { logWarning } from "../logging";
import { announce } from "../announce";
import { noteOpenedMessage } from "../buddyMessages";
import { useNotificationsStore } from "../stores/notifications";
import HighlightText from "./HighlightText.vue";
import type { SearchHit, SearchResponse } from "../types";

const notifications = useNotificationsStore();

// Mirrors core::search::MIN_QUERY_CHARS. Counted in Unicode code points
// (matching Rust's chars().count()) — String.length counts UTF-16 units and
// let a single emoji through to a backend refusal, which then rendered a
// false "No matches".
const MIN_QUERY_CHARS = 2;
const DEBOUNCE_MS = 300;
const charCount = (s: string) => [...s].length;

const query = ref("");
// The last response and the query it answers — one value, so highlights,
// the empty state and the truncation footer can never disagree with the
// hits they describe.
const results = ref<{
  query: string;
  hits: SearchHit[];
  truncated: boolean;
} | null>(null);
const searching = ref(false);
const error = ref<string | null>(null);
const inputEl = ref<HTMLInputElement | null>(null);

let timer: ReturnType<typeof setTimeout> | undefined;
// Monotonic ticket: a resolving search that is no longer the latest is
// dropped, so a slow older response can never overwrite newer results.
let ticket = 0;

const tooShort = computed(() => charCount(query.value.trim()) < MIN_QUERY_CHARS);
const hits = computed(() => results.value?.hits ?? []);
const resultsQuery = computed(() => results.value?.query ?? "");
const truncated = computed(() => results.value?.truncated ?? false);

// Flat hits → per-vault groups, preserving the backend's vault order. Rows
// carry their flat index so keyboard selection can address them.
const groups = computed(() => {
  const map = new Map<
    string,
    { vaultName: string; rows: { hit: SearchHit; i: number }[] }
  >();
  hits.value.forEach((hit, i) => {
    const group = map.get(hit.vaultId);
    if (group) group.rows.push({ hit, i });
    else map.set(hit.vaultId, { vaultName: hit.vaultName, rows: [{ hit, i }] });
  });
  return [...map.entries()].map(([vaultId, g]) => ({ vaultId, ...g }));
});

watch(query, () => {
  if (timer) clearTimeout(timer);
  const trimmed = query.value.trim();
  if (charCount(trimmed) < MIN_QUERY_CHARS) {
    // Invalidate any in-flight response too — its results answer a query
    // that no longer exists.
    ticket++;
    searching.value = false;
    results.value = null;
    error.value = null;
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
    results.value = {
      query: trimmed,
      hits: response.hits,
      truncated: response.truncated,
    };
    error.value = null;
  } catch (e) {
    if (mine !== ticket) return;
    // Keep the previous results up — a live refinement that errors must not
    // blank a working list (the backend rejects on infrastructure failures
    // precisely so this branch handles them).
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
        v-for="row in group.rows"
        :key="row.hit.file + (row.hit.isNote ? ':n' : ':a')"
        type="button"
        data-testid="search-hit"
        class="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-left transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        @click="openHit(row.hit)"
      >
        <span
          class="w-full truncate text-sm text-slate-100"
          :title="row.hit.name"
        >
          <HighlightText :text="row.hit.name" :query="resultsQuery" />
        </span>
        <span v-if="row.hit.folder" class="w-full truncate text-xs text-slate-500">
          {{ row.hit.folder }}
        </span>
        <span v-if="row.hit.snippet" class="w-full truncate text-xs text-slate-400">
          <HighlightText :text="row.hit.snippet" :query="resultsQuery" />
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
