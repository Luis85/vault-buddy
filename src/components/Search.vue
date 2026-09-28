<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";

import { announce } from "../announce";
import { noteOpenedMessage } from "../buddyMessages";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import type { SearchHit, SearchResponse } from "../types";
import {
  clearRecentSearches,
  loadRecentSearches,
  pushRecentSearch,
} from "../utils/recentSearches";
import {
  groupSearchHits,
  searchHitId,
  searchSummary,
} from "../utils/searchResults";
import AppIcon from "./AppIcon.vue";
import SearchResultList from "./SearchResultList.vue";
import Banner from "./ui/Banner.vue";
import EmptyState from "./ui/EmptyState.vue";

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

// Kind filter + per-vault collapse feed ONE computed that yields the group
// list AND the flat list of VISIBLE rows the keyboard navigates — built in
// the same pass so arrows can never select a hidden row. Both survive
// refinements while the view lives (the component unmounts on view exit).
const kindFilter = ref<"all" | "notes" | "files">("all");
const collapsed = ref(new Set<string>());

const kindFiltered = computed(() => {
  const all = hits.value;
  if (kindFilter.value === "notes") return all.filter((h) => h.isNote);
  if (kindFilter.value === "files") return all.filter((h) => !h.isNote);
  return all;
});

const resultView = computed(() =>
  groupSearchHits(kindFiltered.value, collapsed.value),
);
const visibleHits = computed(() => resultView.value.flat);

// `N matches in M vaults` over the FULL response (pre-filter); `100+` when
// truncated. Rendered aria-live so screen readers hear result updates.
const summary = computed(() => searchSummary(hits.value, truncated.value));

function toggleGroup(vaultId: string) {
  if (collapsed.value.has(vaultId)) collapsed.value.delete(vaultId);
  else collapsed.value.add(vaultId);
}

// Keyboard selection: a flat index into `hits`, moved by the arrow keys on
// the input, opened by Enter. Reset to the top hit on every new result set.
const selected = ref(0);

watch(results, () => {
  selected.value = 0;
});

// Collapsing/filtering can shrink the visible list under the selection —
// clamp (new results still reset to 0 via the results watcher above).
watch(visibleHits, (list) => {
  if (selected.value >= list.length) {
    selected.value = Math.max(0, list.length - 1);
  }
});

function onArrow(event: KeyboardEvent, delta: 1 | -1) {
  if (event.isComposing) return; // IME candidate navigation owns the arrows
  if (visibleHits.value.length === 0) return;
  event.preventDefault(); // the list owns arrows; keep the caret still
  selected.value = Math.min(
    Math.max(selected.value + delta, 0),
    visibleHits.value.length - 1,
  );
  void nextTick(() => {
    document
      .getElementById(searchHitId(selected.value))
      ?.scrollIntoView({ block: "nearest" });
  });
}

function onEnter(event: KeyboardEvent) {
  // An IME commit's Enter arrives as a keydown with isComposing — the user
  // is finishing their query text, not opening the selection.
  if (event.isComposing) return;
  const hit = visibleHits.value[selected.value];
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
    // Only queries that actually produced a response are worth re-offering.
    recents.value = pushRecentSearch(trimmed);
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
    // keepOpen travels to Rust: skipping close_panel below is not enough on
    // its own — Obsidian grabs focus when it handles the URI, and the
    // panel's focus-out check would hide the panel moments later. The
    // command pins the panel open across that grab.
    await invoke("open_search_result", {
      id: hit.vaultId,
      file: hit.file,
      keepOpen,
    });
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

// Recent successful queries as chips under the hint state — a panel reopen
// shouldn't mean retyping. localStorage-backed (settings precedent).
const recents = ref<string[]>(loadRecentSearches());

function useRecent(q: string) {
  query.value = q; // the watcher debounces + runs the search normally
}

function onClearRecents() {
  clearRecentSearches();
  recents.value = [];
}

function onEscape(event: KeyboardEvent) {
  if (event.isComposing) return; // IME cancel, not a query clear
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
        :aria-expanded="visibleHits.length > 0 ? 'true' : 'false'"
        aria-autocomplete="list"
        aria-controls="search-results"
        :aria-activedescendant="visibleHits.length ? searchHitId(selected) : undefined"
        class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
        @keydown.escape="onEscape"
        @keydown.down="onArrow($event, 1)"
        @keydown.up="onArrow($event, -1)"
        @keydown.enter="onEnter"
      >
      <span
        v-if="searching && hits.length > 0"
        data-testid="search-refreshing"
        class="absolute right-2 top-1/2 h-2 w-2 -translate-y-1/2 animate-pulse rounded-full bg-violet-400"
        aria-hidden="true"
      />
    </div>
    <Banner
      v-if="error"
      tone="danger"
    >
      {{ error }}
    </Banner>
    <p
      v-if="tooShort"
      class="text-xs text-fg-muted"
    >
      Type at least {{ MIN_QUERY_CHARS }} characters to search.
    </p>
    <div
      v-if="tooShort && recents.length > 0"
      data-testid="recent-section"
      class="flex flex-col gap-1"
    >
      <div class="flex items-center justify-between">
        <span
          class="text-xs font-semibold uppercase tracking-wide text-fg-muted"
        >Recent</span>
        <button
          type="button"
          data-testid="recent-clear"
          class="cursor-pointer rounded px-1 text-micro text-fg-subtle transition-colors hover:bg-white/10 hover:text-fg-secondary focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="onClearRecents"
        >
          Clear
        </button>
      </div>
      <div class="flex flex-wrap gap-1">
        <button
          v-for="q in recents"
          :key="q"
          type="button"
          data-testid="recent-chip"
          class="max-w-full cursor-pointer truncate rounded-full bg-white/5 px-2 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="useRecent(q)"
        >
          {{ q }}
        </button>
      </div>
    </div>
    <p
      v-else-if="searching && hits.length === 0"
      class="text-xs text-fg-muted"
    >
      Searching…
    </p>
    <EmptyState
      v-else-if="hits.length === 0 && resultsQuery"
      :title="`No matches for &quot;${resultsQuery}&quot;.`"
    >
      <template #icon>
        <AppIcon :size="28">
          <circle
            cx="11"
            cy="11"
            r="8"
          />
          <path d="m21 21-4.35-4.35" />
        </AppIcon>
      </template>
    </EmptyState>
    <p
      v-if="summary"
      data-testid="search-summary"
      role="status"
      aria-live="polite"
      class="text-xs text-fg-muted"
    >
      {{ summary }}
    </p>
    <div
      v-if="hits.length > 0"
      role="group"
      aria-label="Filter results by kind"
      class="flex items-center gap-1"
    >
      <button
        v-for="k in ['all', 'notes', 'files'] as const"
        :key="k"
        type="button"
        :data-testid="`search-filter-${k}`"
        :aria-pressed="kindFilter === k"
        class="cursor-pointer rounded-full px-2 py-0.5 text-micro transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="
          kindFilter === k
            ? 'bg-violet-500/30 text-fg'
            : 'bg-white/5 text-fg-muted hover:bg-white/10'
        "
        @click="kindFilter = k"
      >
        {{ k === "all" ? "All" : k === "notes" ? "Notes" : "Files" }}
      </button>
    </div>
    <EmptyState
      v-if="hits.length > 0 && kindFiltered.length === 0"
      title="Nothing matches this filter."
    />
    <SearchResultList
      :groups="resultView.groups"
      :selected="selected"
      :query="resultsQuery"
      @open="openHit"
      @hover="selected = $event"
      @toggle="toggleGroup"
    />
    <p
      v-if="truncated"
      data-testid="search-truncated"
      class="text-xs text-fg-subtle"
    >
      Showing the first {{ hits.length }} matches — refine your query.
    </p>
  </div>
</template>
