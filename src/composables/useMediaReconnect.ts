/**
 * Reconnecting missing originals (Task 40; F-03; A19; SCREENS 07): the
 * state behind `ReconnectDialog`. Rust opens its OWN dialog and decides
 * every match (`editor_relink_media` → `core::editor::relink`); this file
 * only keeps what each call said about each asset, across the batch call
 * and the one-asset follow-ups, and installs the reply's projection and
 * remaining missing list into `editorProject`.
 *
 * **Nothing is guessed here.** An ambiguous asset stays missing and is
 * shown as ambiguous until a later call reports it matched; a mismatched
 * one becomes a replacement only through a second call the user starts
 * with `confirmReplace`. A reply that lands after the session changed is
 * dropped, the store's own generation guard.
 */
import { ref } from "vue";

import type { RelinkReport } from "../editorTypes";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";

export type ReconnectOutcome =
  | { kind: "matched"; file: string }
  | { kind: "replaced"; file: string }
  | { kind: "ambiguous"; files: string[] }
  | { kind: "unmatched" }
  | { kind: "mismatched"; file: string; reason: string };

interface Status {
  text: string;
  alert: boolean;
}

/** Every asset a report speaks about, keyed by asset id. A mismatched
 * asset can carry several stray files; the first is shown. */
function outcomesOf(report: RelinkReport): Record<string, ReconnectOutcome> {
  const out: Record<string, ReconnectOutcome> = {};
  for (const m of report.mismatched) {
    out[m.assetId] ??= { kind: "mismatched", file: m.file, reason: m.reason };
  }
  for (const id of report.unmatched) out[id] = { kind: "unmatched" };
  for (const a of report.ambiguous) out[a.assetId] = { kind: "ambiguous", files: a.files };
  for (const m of report.matched) out[m.assetId] = { kind: "matched", file: m.file };
  for (const r of report.replaced) out[r.assetId] = { kind: "replaced", file: r.file };
  return out;
}

function summary(report: RelinkReport): Status {
  const done = report.matched.length + report.replaced.length;
  const open = report.ambiguous.length + report.mismatched.length + report.unmatched.length;
  if (done === 0 && open === 0) return { text: "None of the chosen files could be examined.", alert: true };
  const parts = [done === 1 ? "1 original reconnected." : `${done} originals reconnected.`];
  if (open > 0) parts.push(open === 1 ? "1 still needs your choice." : `${open} still need your choice.`);
  return { text: parts.join(" "), alert: false };
}

export function useMediaReconnect() {
  const project = useEditorProjectStore();
  const outcomes = ref<Record<string, ReconnectOutcome>>({});
  const problems = ref<RelinkReport["perFile"]>([]);
  const status = ref<Status | null>(null);
  const busy = ref(false);

  function install(report: RelinkReport): void {
    project.applyExecuteResult(project.generation, report.projection);
    project.$patch({ missing: report.missing });
    outcomes.value = { ...outcomes.value, ...outcomesOf(report) };
    problems.value = report.perFile;
    status.value = summary(report);
  }

  /** One `editor_relink_media` round trip. Ignored while one is running. */
  async function run(assetIds: string[], confirmReplace: boolean): Promise<void> {
    const sessionId = project.sessionId;
    if (busy.value || !sessionId || assetIds.length === 0) return;
    const generation = project.generation;
    busy.value = true;
    status.value = null;
    try {
      const report = await project.port.relinkMedia(sessionId, assetIds, confirmReplace);
      if (generation !== project.generation || sessionId !== project.sessionId) return;
      if (report === null) {
        status.value = { text: "The file dialog was closed — nothing was reconnected.", alert: false };
      } else {
        install(report);
      }
    } catch (e) {
      if (generation === project.generation) status.value = { text: toEditorError(e).message, alert: true };
    } finally {
      busy.value = false;
    }
  }

  /** Back to a clean slate for a newly opened dialog. */
  function reset(): void {
    outcomes.value = {};
    problems.value = [];
    status.value = null;
  }

  return { outcomes, problems, status, busy, run, reset };
}
