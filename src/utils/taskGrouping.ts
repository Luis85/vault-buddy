import { logWarning } from "../logging";

export type Grouping = "dates" | "tags" | "lists";
const STORAGE_KEY = "vault-buddy:task-grouping";
const DEFAULT: Grouping = "lists";
const VALID = new Set<Grouping>(["dates", "tags", "lists"]);

function readAll(): Record<string, unknown> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) return parsed as Record<string, unknown>;
  } catch (e) {
    logWarning(`task grouping: load failed: ${String(e)}`);
  }
  return {};
}

export function loadGrouping(viewKey: string): Grouping {
  const v = readAll()[viewKey];
  return typeof v === "string" && VALID.has(v as Grouping) ? (v as Grouping) : DEFAULT;
}

export function saveGrouping(viewKey: string, value: Grouping): void {
  const all = readAll();
  all[viewKey] = value;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(all));
  } catch (e) {
    logWarning(`task grouping: save failed: ${String(e)}`);
  }
}
