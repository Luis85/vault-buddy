import { computed, ref } from "vue";

import type { Vault } from "../types";

/**
 * The name+path vault filter shared by ActionPanel's vault list and
 * ImportVaultPicker's vault-first picker (fallow clone group — both grew the
 * identical filter/threshold/Escape logic independently). `source` is a
 * getter (not a plain array) so the returned computeds stay reactive to
 * whatever list the caller passes — a Pinia store array in both current
 * callers — without this composable depending on Pinia itself.
 *
 * Callers keep their own `showFilter` gate (each gates on a different view
 * flag AND `aboveThreshold`) and their own `shownNonce` reset watch — only
 * the query state, the threshold, the match predicate, and the IME-guarded
 * Escape handler are common.
 */
export function useVaultFilter(source: () => Vault[]) {
  const filter = ref("");
  // A short list is scannable at a glance; only offer filtering when the
  // list is long enough that scanning stops working.
  const FILTER_THRESHOLD = 5;
  const aboveThreshold = computed(() => source().length > FILTER_THRESHOLD);
  const filtered = computed(() => {
    const query = filter.value.trim().toLowerCase();
    if (!query) return source();
    return source().filter(
      (v) => v.name.toLowerCase().includes(query) || v.path.toLowerCase().includes(query),
    );
  });
  function onFilterEscape(event: KeyboardEvent) {
    // GAP-31: IME composition can emit Escape; that must dismiss the IME, not clear the filter
    if (event.isComposing) return;
    if (filter.value) {
      // first Escape clears the filter; a second one bubbles up and closes
      filter.value = "";
      event.stopPropagation();
    }
  }
  return { filter, FILTER_THRESHOLD, aboveThreshold, filtered, onFilterEscape };
}
