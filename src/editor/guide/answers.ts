/**
 * The learning center's quick answers and shortcut table (Task 57; F-47;
 * ONBOARDING.md: "searchable quick answers, shortcuts"; SCREENS 11:
 * "Current navigation answers explain where controls are, not how the UI
 * changed historically").
 *
 * **Answers are the lessons, as the coach tells them.** One answer per
 * lesson: its title as the question and `lessonCopy(id)` — the verbatim
 * concept text WITH this app's corrections (`LESSON_COPY_OVERRIDES`,
 * docs/Gaps.md GAP-203) — as the answer. Never a raw `steps.json` field
 * beyond the title: the untrue browser-reference sentences (a download, a
 * three-minute render, a sample project) must not come back through search.
 * There is no second, hand-written answer list to drift from the lessons.
 *
 * **Search** is a case- and diacritic-insensitive substring match over the
 * question, the control's label, the answer and its tip (`normalize`).
 *
 * **Shortcuts come from `shortcuts.ts`.** `SHORTCUT_TABLE` is built from
 * `SHORTCUTS` itself — one row per action, every combo bound to it — so a
 * binding added, changed or removed there is the table the user reads.
 * Only the row's wording is chosen here (`ACTION_LABELS`, with the three
 * guide/app keys worded for what they do). `OTHER_KEYS` are the two keys
 * `shortcuts.ts` answers with a predicate rather than a table entry
 * (`isContextMenuShortcut`, `isGuideDismissKey`).
 */
import type { ActionId } from "../actionMeta";
import { ACTION_LABELS } from "../actionMeta";
import { SHORTCUTS } from "../shortcuts";
import type { GuideStepId } from "./content";
import { GUIDE_STEPS, lessonCopy } from "./content";

export interface QuickAnswer {
  stepId: GuideStepId;
  question: string;
  label: string;
  answer: string;
  tip: string;
}

export const QUICK_ANSWERS: readonly QuickAnswer[] = GUIDE_STEPS.map((step) => {
  const copy = lessonCopy(step.id);
  return { stepId: step.id, question: step.title, label: copy.label, answer: copy.body, tip: copy.tip };
});

/** Lower case with every combining mark removed ("Fädé" → "fade"). */
function normalize(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();
}

const HAYSTACKS = QUICK_ANSWERS.map((a) => normalize([a.question, a.label, a.answer, a.tip].join(" ")));

/** The answers whose text contains `query`; all of them for a blank one. */
export function searchAnswers(query: string): QuickAnswer[] {
  const needle = normalize(query.trim());
  if (!needle) return [...QUICK_ANSWERS];
  return QUICK_ANSWERS.filter((_, i) => HAYSTACKS[i].includes(needle));
}

export interface ShortcutRow {
  actionId: ActionId;
  label: string;
  keys: string[];
}

/** What a key does, where the action's own button label does not say. */
const PURPOSE: Partial<Record<ActionId, string>> = {
  help: "Start or resume the guided walkthrough",
  guideFocus: "Switch between the guide and its highlighted control",
  render: "Review part of the video as a real render",
};

/** `ctrl+shift+z` → `Ctrl+Shift+Z`, `?` → `?`, `f1` → `F1`. */
export function displayCombo(combo: string): string {
  return combo
    .split("+")
    .map((part) => part[0].toUpperCase() + part.slice(1))
    .join("+");
}

function buildTable(): ShortcutRow[] {
  const rows = new Map<ActionId, ShortcutRow>();
  for (const [combo, actionId] of SHORTCUTS) {
    const row = rows.get(actionId) ?? { actionId, label: PURPOSE[actionId] ?? ACTION_LABELS[actionId], keys: [] };
    row.keys.push(displayCombo(combo));
    rows.set(actionId, row);
  }
  return [...rows.values()];
}

export const SHORTCUT_TABLE: readonly ShortcutRow[] = buildTable();

export const OTHER_KEYS: readonly { label: string; keys: string[] }[] = [
  { label: "Open the focused clip's menu", keys: ["Shift+F10", "Menu"] },
  { label: "Pause the guide (an open menu closes first)", keys: ["Esc"] },
];
