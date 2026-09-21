/**
 * The SHAPE of the search view's result list: which rows are visible, how
 * they group by vault, the DOM id each row answers to, and the summary line.
 *
 * Split out of Search.vue at 497/500 nonblank lines, along the seam that
 * view already drew between deriving the list and rendering it. Everything
 * here is a pure function of the response plus the view's collapse state —
 * no refs, no IPC, no DOM — so it moved without taking any of the view's
 * lifecycle (debounce, ticket, selection clamp) with it. The rendering half
 * is SearchResultList.vue; the ids it stamps and the input's
 * aria-activedescendant both come from `searchHitId`, so the two can never
 * name different elements.
 */
import type { SearchHit } from "../types";

/** One vault's section: its header state and its VISIBLE rows. */
export interface SearchResultGroup {
  vaultId: string;
  vaultName: string;
  collapsed: boolean;
  /** Every kind-filtered hit in the vault, collapsed or not (the chip). */
  count: number;
  /** `i` indexes `flat` — the keyboard selection's coordinate space. */
  rows: { hit: SearchHit; i: number }[];
}

/** The DOM id of the `i`-th visible row (aria-activedescendant target). */
export const searchHitId = (i: number) => `search-hit-${i}`;

/**
 * Group hits by vault (first-seen order) AND build the flat list of VISIBLE
 * rows the keyboard navigates — in the same pass, so the arrows can never
 * select a row a collapsed group is hiding.
 */
export function groupSearchHits(
  hits: readonly SearchHit[],
  collapsed: ReadonlySet<string>,
): { groups: SearchResultGroup[]; flat: SearchHit[] } {
  const map = new Map<string, Omit<SearchResultGroup, "vaultId">>();
  const flat: SearchHit[] = [];
  for (const hit of hits) {
    let group = map.get(hit.vaultId);
    if (!group) {
      group = {
        vaultName: hit.vaultName,
        collapsed: collapsed.has(hit.vaultId),
        count: 0,
        rows: [],
      };
      map.set(hit.vaultId, group);
    }
    group.count++;
    if (!group.collapsed) {
      group.rows.push({ hit, i: flat.length });
      flat.push(hit);
    }
  }
  return {
    groups: [...map.entries()].map(([vaultId, g]) => ({ vaultId, ...g })),
    flat,
  };
}

/**
 * `N matches in M vaults` over the FULL response (pre-filter); `N+` when
 * the backend truncated. Null when there is nothing to summarise.
 */
export function searchSummary(
  hits: readonly SearchHit[],
  truncated: boolean,
): string | null {
  if (hits.length === 0) return null;
  const vaults = new Set(hits.map((h) => h.vaultId)).size;
  const n = truncated ? `${hits.length}+` : `${hits.length}`;
  return `${n} ${hits.length === 1 && !truncated ? "match" : "matches"} in ${vaults} ${vaults === 1 ? "vault" : "vaults"}`;
}
