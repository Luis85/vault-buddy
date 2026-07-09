<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
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

// Keyboard selection: a flat index into `hits`, moved by the arrow keys on
// the input, opened by Enter. Reset to the top hit on every new result set.
const selected = ref(0);
const hitId = (i: number) => `search-hit-${i}`;

watch(results, () => {
  selected.value = 0;
});

function onArrow(event: KeyboardEvent, delta: 1 | -1) {
  if (hits.value.length === 0) return;
  event.preventDefault(); // the list owns arrows; keep the caret still
  selected.value = Math.min(
    Math.max(selected.value + delta, 0),
    hits.value.length - 1,
  );
  void nextTick(() => {
    document
      .getElementById(hitId(selected.value))
      ?.scrollIntoView({ block: "nearest" });
  });
}

function onEnter(event: KeyboardEvent) {
  const hit = hits.value[selected.value];
  if (hit) void openHit(hit, event.ctrlKey || event.metaKey);
}

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

async function openHit(hit: SearchHit, keepOpen = false) {
  try {
    await invoke("open_search_result", { id: hit.vaultId, file: hit.file });
    // Same acknowledgement pattern as vault/daily-note opens (the panel
    // window is the announcer for opens); a failed open stays silent — the
    // toast is the feedback there. Ctrl-open keeps the panel up for a
    // multi-open workflow; plain open gets out of the way like a vault
    // launch does.
    announce(noteOpenedMessage(hit.name));
    if (!keepOpen) void invoke("close_panel").catch(() => {});
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
    <div class="relative">
      <input
        ref="inputEl"
        v-model="query"
        data-testid="search-input"
        type="search"
        placeholder="Search all vaults…"
        aria-label="Search all vaults"
        role="combobox"
        aria-expanded="true"
        aria-controls="search-results"
        :aria-activedescendant="hits.length ? hitId(selected) : undefined"
        class="w-full rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.escape="onEscape"
        @keydown.down="onArrow($event, 1)"
        @keydown.up="onArrow($event, -1)"
        @keydown.enter="onEnter"
      />
      <span
        v-if="searching && hits.length > 0"
        data-testid="search-refreshing"
        class="absolute right-2 top-1/2 h-2 w-2 -translate-y-1/2 animate-pulse rounded-full bg-violet-400"
        aria-hidden="true"
      ></span>
    </div>
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
      id="search-results"
      role="listbox"
      aria-label="Search results"
      class="flex flex-col gap-2"
    >
      <div
        v-for="group in groups"
        :key="group.vaultId"
        class="flex flex-col gap-1"
      >
        <h2
          class="flex items-center gap-2 text-xs font-semibold uppercase tracking-wide text-slate-400"
        >
          {{ group.vaultName }}
          <span
            data-testid="group-count"
            class="rounded-full bg-white/10 px-1.5 py-0.5 text-[10px] font-normal normal-case text-slate-400"
            >{{ group.rows.length }}</span
          >
        </h2>
        <button
          v-for="row in group.rows"
          :id="hitId(row.i)"
          :key="row.hit.file + (row.hit.isNote ? ':n' : ':a')"
          type="button"
          data-testid="search-hit"
          role="option"
          :aria-selected="row.i === selected"
          class="flex w-full cursor-pointer flex-col items-start gap-0.5 rounded-lg border px-2 py-1 text-left transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            row.i === selected
              ? 'border-violet-400/60 bg-white/10'
              : 'border-white/10 bg-white/5'
          "
          @click="openHit(row.hit, $event.ctrlKey || $event.metaKey)"
          @mousemove="selected = row.i"
        >
          <span class="flex w-full min-w-0 items-center gap-1.5">
            <svg
              v-if="row.hit.isNote"
              data-testid="hit-icon-note"
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
              class="shrink-0 text-slate-400"
            >
              <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
              <path d="M14 2v6h6M16 13H8M16 17H8M10 9H8" />
            </svg>
            <svg
              v-else
              data-testid="hit-icon-file"
              width="12"
              height="12"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              aria-hidden="true"
              class="shrink-0 text-slate-400"
            >
              <path
                d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"
              />
            </svg>
            <span
              class="min-w-0 flex-1 truncate text-sm text-slate-100"
              :title="row.hit.name"
            >
              <HighlightText :text="row.hit.name" :query="resultsQuery" />
            </span>
          </span>
          <span v-if="row.hit.folder" class="w-full truncate text-xs text-slate-500">
            {{ row.hit.folder }}
          </span>
          <span v-if="row.hit.snippet" class="w-full truncate text-xs text-slate-400">
            <HighlightText :text="row.hit.snippet" :query="resultsQuery" />
          </span>
        </button>
      </div>
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
