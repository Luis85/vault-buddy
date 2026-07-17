import { logWarning } from "../logging";

// Generic per-view localStorage store: a JSON object keyed by view ("all" or
// a vault id), used to persist one UI preference at a time (sort order,
// grouping mode, …) across the task views. Both taskSort.ts and
// taskGrouping.ts used to hand-roll this exact read/parse/guard/write
// envelope — fallow's duplicate-code gate caught the clone (see
// scripts/quality-baseline.json) — so it now lives here once, and each
// caller supplies only its storage key, a sanitizer, and a default.

/** A `load`/`save` pair over one localStorage key. */
interface PerViewStore<T> {
  /** The persisted value for a view; a missing or corrupt (per `sanitize`)
   * entry degrades to the default — with a warning, never a throw into the
   * component. */
  load(viewKey: string): T;
  /** Merges `value` into the per-view map and persists it. */
  save(viewKey: string, value: T): void;
}

/** Returns a fresh copy of an object-shaped default so two callers can never
 * share (and one mutate in place) the same default instance; a primitive
 * default is returned as-is — it's already immutable. */
function cloneDefault<T>(value: T): T {
  return value !== null && typeof value === "object" ? ({ ...value } as T) : value;
}

/**
 * Builds a `{load, save}` pair over one localStorage key holding a JSON
 * object keyed per view.
 *
 * @param storageKey    The localStorage key.
 * @param sanitize      Validates/narrows a raw stored entry to `T`; return
 *                       `null` for anything that isn't a well-formed `T`
 *                       (missing entry, wrong shape, invalid enum value, …)
 *                       to fall back to `defaultValue`.
 * @param defaultValue   What `load` returns for a missing/corrupt entry.
 * @param label          Short name used in the `logWarning` messages (e.g.
 *                       "task sort").
 */
export function createPerViewStore<T>(
  storageKey: string,
  sanitize: (raw: unknown) => T | null,
  defaultValue: T,
  label: string,
): PerViewStore<T> {
  function readAll(): Record<string, unknown> {
    try {
      const raw = localStorage.getItem(storageKey);
      if (!raw) return {};
      const parsed: unknown = JSON.parse(raw);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed))
        return parsed as Record<string, unknown>;
    } catch (e) {
      logWarning(`${label}: load failed: ${String(e)}`);
    }
    return {};
  }

  return {
    load(viewKey) {
      return sanitize(readAll()[viewKey]) ?? cloneDefault(defaultValue);
    },
    save(viewKey, value) {
      const all = readAll();
      all[viewKey] = value;
      try {
        localStorage.setItem(storageKey, JSON.stringify(all));
      } catch (e) {
        logWarning(`${label}: save failed: ${String(e)}`);
      }
    },
  };
}
