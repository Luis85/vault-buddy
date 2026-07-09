import { logWarning } from "../logging";

/** localStorage key; a JSON string array, most recent first. */
const KEY = "vault-buddy:recent-searches";
export const MAX_RECENT_SEARCHES = 5;

/**
 * The recent successful queries, most recent first. Storage failures and
 * corrupted values degrade to an empty list — with a warning, never a throw
 * into the component (no swallowed errors).
 */
export function loadRecentSearches(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter((q): q is string => typeof q === "string")
      .slice(0, MAX_RECENT_SEARCHES);
  } catch (e) {
    logWarning(`recent searches: load failed: ${String(e)}`);
    return [];
  }
}

/**
 * Record a successful query: moved to the front, deduped case-insensitively
 * (latest casing wins), capped at MAX_RECENT_SEARCHES. Returns the updated
 * list so the caller can render without a second load.
 */
export function pushRecentSearch(query: string): string[] {
  const lower = query.toLowerCase();
  const next = [
    query,
    ...loadRecentSearches().filter((q) => q.toLowerCase() !== lower),
  ].slice(0, MAX_RECENT_SEARCHES);
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch (e) {
    logWarning(`recent searches: save failed: ${String(e)}`);
  }
  return next;
}

export function clearRecentSearches(): void {
  try {
    localStorage.removeItem(KEY);
  } catch (e) {
    logWarning(`recent searches: clear failed: ${String(e)}`);
  }
}
