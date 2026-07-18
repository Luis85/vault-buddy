import { logWarning } from "../logging";

/**
 * localStorage key; a JSON string array of favorited vault ids. Pure
 * frontend state — panel-list ordering only, so Rust never needs it (the
 * recentSearches.ts precedent).
 */
const KEY = "vault-buddy:favorite-vaults";

/**
 * The favorited vault ids. Storage failures and corrupted values degrade to
 * an empty list — with a warning, never a throw into the component (no
 * swallowed errors), same defensive posture as recentSearches.ts.
 */
export function loadFavorites(): string[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((v): v is string => typeof v === "string");
  } catch (e) {
    logWarning(`favoriteVaults: load failed: ${String(e)}`);
    return [];
  }
}

function save(ids: string[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(ids));
  } catch (e) {
    logWarning(`favoriteVaults: save failed: ${String(e)}`);
  }
}

/**
 * Toggle a vault's favorite state (add if absent, remove if present).
 * Persists the result and returns the updated list so the caller can render
 * without a second load.
 */
export function toggleFavorite(id: string): string[] {
  const ids = loadFavorites();
  const next = ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
  save(next);
  return next;
}
