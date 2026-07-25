import type { TaskEditorPatch, TaskItem } from "../types";

// Pure task-field helpers shared by the Tasks container and its TaskRow /
// TaskEditor children — single-sourced here so a component split doesn't
// duplicate the date/tag parsing (the fallow clone ratchet would flag a copy).

// A due only counts when it's a plain YYYY-MM-DD — a hand-authored value like
// "tomorrow" degrades to no-date instead of erroring (defensive read).
const DUE_RE = /^\d{4}-\d{2}-\d{2}$/;
export const dueOf = (t: TaskItem): string | null =>
  t.due && DUE_RE.test(t.due) ? t.due : null;

// A scheduled (do) date counts only when it's a plain YYYY-MM-DD (defensive
// read, same shape gate as dueOf).
export const scheduledOf = (t: TaskItem): string | null =>
  t.scheduled && DUE_RE.test(t.scheduled) ? t.scheduled : null;

// The effective PLAN date the planner buckets by: the do-date if set, else the
// deadline. Setting a scheduled date is what moves a task's plan; a
// deadline-only task still buckets by its deadline (non-regressing).
export const plannerDateOf = (t: TaskItem): string | null => scheduledOf(t) ?? dueOf(t);

// LOCAL calendar date — never UTC/ISO slicing, matching add_task's local-date
// rule; near midnight UTC-derived "today" would mis-bucket by a day.
export function localToday(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// N days from local today, as YYYY-MM-DD (local calendar — never UTC slicing,
// matching localToday's rule so a near-midnight schedule doesn't slip a day).
export function localDatePlus(days: number): string {
  const d = new Date();
  d.setDate(d.getDate() + days);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

// The coming Saturday (today if today is Saturday), as YYYY-MM-DD.
export function comingSaturday(): string {
  const dow = new Date().getDay(); // 0=Sun … 6=Sat
  return localDatePlus((6 - dow + 7) % 7);
}

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

// A short, locale-independent date label ("Jul 15"). Shared by the row's due
// element and the do-date chip's far-date fallback.
export function shortDate(date: string): string {
  const [, m, day] = date.split("-");
  const month = MONTHS[Number(m) - 1];
  return month ? `${month} ${Number(day)}` : date;
}

// A friendly relative label for a plan date, given today (both YYYY-MM-DD):
// Today / Tomorrow / a weekday within the next 6 days ("Sat") / else shortDate.
// `today` is injected so it's deterministic and unit-testable (no clock mock).
export function relativeDateLabel(date: string, today: string): string {
  if (date === today) return "Today";
  const d = new Date(`${date}T00:00:00`);
  const t = new Date(`${today}T00:00:00`);
  // A shape-valid but calendar-invalid date (e.g. 2026-02-31 — the task contract
  // accepts it; is_valid_due does NO calendar check) normalizes into the next
  // month, so getDay()/day-diff would render a wrong weekday. Fall back to the
  // literal shortDate when the Date didn't round-trip.
  const [y, mo, day] = date.split("-").map(Number);
  if (d.getFullYear() !== y || d.getMonth() + 1 !== mo || d.getDate() !== day) {
    return shortDate(date);
  }
  const diffDays = Math.round((d.getTime() - t.getTime()) / 86_400_000);
  if (diffDays === 1) return "Tomorrow";
  if (diffDays > 1 && diffDays < 7) return WEEKDAYS[d.getDay()];
  return shortDate(date);
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

/** The editable draft the inline editor and the detail view both hold. */
export interface TaskDraft {
  title: string;
  due: string;
  scheduled: string;
  priority: string; // "high" | "normal" | "low"
  tags: string; // comma/space free-text input
  list: string;
}

// due and scheduled use the identical changed-fields shape (draft vs. original,
// blank means clear) — single-sourced so buildTaskPatch stays under the
// complexFunctions CRAP gate (GAP-74).
function diffDateField(
  patch: TaskEditorPatch,
  draft: string,
  original: string | null,
  setKey: "due" | "scheduled",
  clearKey: "clearDue" | "clearScheduled",
) {
  if (draft === (original ?? "")) return;
  if (draft === "") patch[clearKey] = true;
  else patch[setKey] = draft;
}

/**
 * The changed-fields patch shared by the inline editor and the detail view —
 * only keys whose draft differs from the task are emitted (an emptied date →
 * clear*). The detail view augments the result with `description` separately.
 */
export function buildTaskPatch(task: TaskItem, draft: TaskDraft): TaskEditorPatch {
  const patch: TaskEditorPatch = {};
  const title = draft.title.trim();
  if (title && title !== task.title) patch.title = title;
  diffDateField(patch, draft.due, dueOf(task), "due", "clearDue");
  diffDateField(patch, draft.scheduled, scheduledOf(task), "scheduled", "clearScheduled");
  const normPriority =
    task.priority === "high" || task.priority === "low" ? task.priority : "normal";
  if (draft.priority !== normPriority) patch.priority = draft.priority;
  const parsedTags = parseTagsInput(draft.tags);
  if (parsedTags.join(" ") !== task.tags.join(" ")) patch.tags = parsedTags;
  if (draft.list !== task.list) patch.list = draft.list;
  return patch;
}
