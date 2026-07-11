import type { TaskItem } from "../types";

// Pure task-field helpers shared by the Tasks container and its TaskRow /
// TaskEditor children — single-sourced here so a component split doesn't
// duplicate the date/tag parsing (the fallow clone ratchet would flag a copy).

// A due only counts when it's a plain YYYY-MM-DD — a hand-authored value like
// "tomorrow" degrades to no-date instead of erroring (defensive read).
const DUE_RE = /^\d{4}-\d{2}-\d{2}$/;
export const dueOf = (t: TaskItem): string | null =>
  t.due && DUE_RE.test(t.due) ? t.due : null;

// LOCAL calendar date — never UTC/ISO slicing, matching add_task's local-date
// rule; near midnight UTC-derived "today" would mis-bucket by a day.
export function localToday(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// Split a free-text tags field on commas/whitespace, strip leading `#`s,
// drop empties, dedupe case-insensitively keeping the first casing.
// Client-side parsing is lenient; the shell strictly validates the charset
// and errors on a bad token.
export function parseTagsInput(s: string): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const raw of s.split(/[\s,]+/)) {
    const t = raw.replace(/^#+/, "");
    if (!t || seen.has(t.toLowerCase())) continue;
    seen.add(t.toLowerCase());
    out.push(t);
  }
  return out;
}
