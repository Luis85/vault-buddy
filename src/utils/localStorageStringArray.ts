import { logWarning } from "../logging";

/**
 * The defensive localStorage-string-array read shared by favoriteVaults.ts
 * and recentSearches.ts (fallow clone group — both grew the identical
 * getItem → JSON.parse → Array.isArray → filter-to-strings load
 * independently). Storage failures and corrupted values degrade to an empty
 * list — with a warning, never a throw into the component (no swallowed
 * errors). `warnLabel` is logged verbatim ahead of the error so each caller
 * keeps its own existing message text (e.g. "favoriteVaults: load failed").
 */
export function loadStringArray(key: string, warnLabel: string): string[] {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((v): v is string => typeof v === "string");
  } catch (e) {
    logWarning(`${warnLabel}: ${String(e)}`);
    return [];
  }
}
