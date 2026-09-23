/**
 * Recovery on open (Task 37 Part A; F-44; PERSISTENCE-AND-SECURITY.md "On
 * app start, show recoverable work … Resume … or explicit Discard"; A27).
 *
 * `check()` runs after the editor opens a NEW session. If that session is
 * clean but its project still has a `recovery.json` (`hasRecovery` in the
 * project listing), an earlier process left unsaved edits behind, and the
 * user decides before editing — the next edit would otherwise journal over
 * them. A DIRTY session is skipped: its journal is its own (a window kept
 * for later and reopened).
 *
 * - **Resume** closes the clean session (`keep`, which flushes nothing: a
 *   clean session has no pending journal) and reopens the project with the
 *   journal as its working copy — dirty, persisted at the saved revision.
 * - **Discard** deletes only the journal (`discardRecovery`) and reopens
 *   the saved project.
 * - A journal that cannot be read is reported as TEXT (A27: the saved
 *   project was not changed, and nothing is interpolated as markup), with
 *   Discard or "Open saved project" (the journal stays on disk) as the way
 *   forward.
 */
import { ref } from "vue";

import type { ProjectSummaryDto } from "../editorTypes";
import { logWarning } from "../logging";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";

export function useEditorRecovery(onSessionChanged: () => void) {
  const project = useEditorProjectStore();
  const offer = ref<ProjectSummaryDto | null>(null);
  const failure = ref<string | null>(null);
  const busy = ref(false);

  async function check(): Promise<void> {
    const snapshot = project.snapshot;
    if (!snapshot || project.dirty) return;
    let rows: ProjectSummaryDto[];
    try {
      rows = await project.port.listProjects();
    } catch (e) {
      logWarning(`editor recovery: could not list projects: ${toEditorError(e).message}`);
      return;
    }
    // The session may have changed while the listing was in flight.
    if (project.snapshot?.sessionId !== snapshot.sessionId) return;
    const row = rows.find((r) => r.projectFileId === snapshot.projectId && r.hasRecovery);
    if (!row) return;
    failure.value = null;
    offer.value = row;
  }

  /** Reopen the offered project, with or without its journal. `true` when
   * the reopen succeeded; a refusal is kept in `failure` for the dialog. */
  async function reopen(useRecovery: boolean): Promise<boolean> {
    const id = offer.value?.projectFileId;
    if (!id) return false;
    await project.openProject(id, useRecovery);
    if (project.lastError) {
      failure.value = project.lastError.message;
      return false;
    }
    return true;
  }

  async function run(step: () => Promise<boolean>): Promise<void> {
    busy.value = true;
    try {
      if (await step()) {
        offer.value = null;
        failure.value = null;
        onSessionChanged();
      }
    } finally {
      busy.value = false;
    }
  }

  const resume = () =>
    run(async () => {
      await project.close("keep");
      return reopen(true);
    });

  /** Needs a session to discard the journal through: after a failed
   * Resume there is none, so the saved project is opened first. */
  const discard = () =>
    run(async () => {
      if (!project.sessionId && !(await reopen(false))) return false;
      await project.close("discardRecovery");
      if (project.lastError) {
        failure.value = project.lastError.message;
        return false;
      }
      return reopen(false);
    });

  const openSaved = () => run(() => reopen(false));

  return { offer, failure, busy, check, resume, discard, openSaved };
}
