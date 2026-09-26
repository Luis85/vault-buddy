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
 * - **Discard** removes the journal (`discardRecovery`) and reopens the
 *   saved project — UNLESS the journal cannot be read at all, in which case
 *   its bytes are kept, moved aside rather than deleted (GAP-180 / R7,
 *   hardening Task 10 fix round 1).
 * - A journal that cannot be read is reported as TEXT (A27: the saved
 *   project was not changed, and nothing is interpolated as markup).
 *   Neither this reopen ("Open saved project") NOR the failed Resume
 *   attempt that got here moves the file — opening a project never touches
 *   `recovery.json` either way (R7: the ORDINARY panel open also passes
 *   `useRecovery: false`, and quarantining at open time silently erased
 *   this whole dialog's own report before the user ever saw it). The file
 *   is set aside — readable or not, since the reopened session did not
 *   resume it (R12, final review I-2) — only by that session's own first
 *   journal write, by a save of it, or by Discard above; so "Their file is
 *   kept in the project folder." holds for every failed Resume.
 * - A REFUSED Discard (Rust could not read or set the journal aside right
 *   now) leaves the Rust session live while the store has already
 *   forgotten it, so the project is reattached (`reattach`, `EditorRoot`'s
 *   own): the editor stays usable behind the dialog and a retry reaches
 *   Rust (final review I-1).
 */
import { ref } from "vue";

import type { ProjectSummaryDto } from "../editorTypes";
import { logWarning } from "../logging";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";

export function useEditorRecovery(
  onSessionChanged: () => void,
  reattach: (projectId: string) => Promise<boolean>,
) {
  const project = useEditorProjectStore();
  const offer = ref<ProjectSummaryDto | null>(null);
  const failure = ref<string | null>(null);
  /** Whether a Resume failed: the dialog then says the journal's file is
   * kept and offers "Open saved project" instead of Resume. A refused
   * Discard alone is not a failed Resume. */
  const resumeFailed = ref(false);
  const busy = ref(false);

  async function check(): Promise<void> {
    const snapshot = project.snapshot;
    if (!snapshot || project.dirty) return;
    let rows: ProjectSummaryDto[];
    try {
      rows = await project.port.listProjects();
    } catch (e) {
      // S-15 (hardening Task 12 fix round 1): by code and operationId,
      // never by message — which can carry a capture's own name in plain
      // text.
      const error = toEditorError(e);
      logWarning(`editor recovery: could not list projects: ${error.code} (${error.operationId})`);
      return;
    }
    // The session may have changed while the listing was in flight — or
    // taken an edit (fix round 1): a dirty session's journal is its own, and
    // Resume's `keep` would flush it over the earlier run's file.
    if (project.snapshot?.sessionId !== snapshot.sessionId || project.dirty) return;
    const row = rows.find((r) => r.projectFileId === snapshot.projectId && r.hasRecovery);
    if (!row) return;
    failure.value = null;
    resumeFailed.value = false;
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
        resumeFailed.value = false;
        onSessionChanged();
      }
    } finally {
      busy.value = false;
    }
  }

  const resume = () =>
    run(async () => {
      await project.close("keep");
      if (await reopen(true)) return true;
      resumeFailed.value = true;
      return false;
    });

  /** Needs a session to discard the journal through: after a failed
   * Resume there is none, so the saved project is opened first. */
  const discard = () =>
    run(async () => {
      if (!project.sessionId && !(await reopen(false))) return false;
      const projectId = project.snapshot?.projectId ?? null;
      await project.close("discardRecovery");
      if (project.lastError) {
        failure.value = project.lastError.message;
        if (projectId !== null) await reattach(projectId);
        return false;
      }
      return reopen(false);
    });

  const openSaved = () => run(() => reopen(false));

  return { offer, failure, resumeFailed, busy, check, resume, discard, openSaved };
}
