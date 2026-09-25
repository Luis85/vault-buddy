/**
 * The editor window's close guard (Task 37 Part A; F-44). Rust answers the
 * window's X with `editor:closeRequested` (emitted to this window alone)
 * rather than hiding it, and `request()` decides what that close means:
 *
 * - no session open (an empty window, or a refused open) → hide at once;
 * - a render or publish job still running → the render copy ("keep it
 *   running in the background, or cancel it"); nothing is cancelled unless
 *   the user asks — never on a close, never on unmount;
 * - a webcam take still recording (Task 49: begun, never finished — it
 *   exists only as a `.part` the closing session would remove) → "You have
 *   an unsaved webcam take": Cancel, or Discard the take (discarded
 *   natively, then the dirty/clean rule below);
 * - unsaved edits → Save project / Keep for later (the session and its
 *   recovery journal stay, the window hides) / Discard changes (the journal
 *   goes, then the window hides) / Cancel;
 * - otherwise → hide at once.
 *
 * Which jobs are live is read from Rust's job REGISTRY (`getJobs`), by
 * kind — the authoritative record, never a local guess — so the render and
 * publish jobs Tasks 46/48 add are covered the day they land. Imports and
 * waveform decodes never change the close copy.
 */
import { ref } from "vue";

import { openWebcamTakes } from "../editor/webcamTakes";
import type { JobPhase, JobRecordDto } from "../editorTypes";
import { logWarning } from "../logging";
import { useEditorOnboardingStore } from "../stores/editorOnboarding";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";

type CloseGuardMode = "dirty" | "render" | "take";

const TERMINAL: ReadonlySet<JobPhase> = new Set<JobPhase>(["complete", "cancelled", "failed"]);

/** The ids of the render/publish jobs among `records` that are still
 * running. */
function liveRenderJobIds(records: JobRecordDto[]): string[] {
  return records
    .filter((r) => (r.kind === "render" || r.kind === "publish") && !TERMINAL.has(r.phase))
    .map((r) => r.jobId);
}

export function useEditorCloseGuard() {
  const project = useEditorProjectStore();
  const mode = ref<CloseGuardMode | null>(null);
  const busy = ref(false);
  const error = ref<string | null>(null);
  let renderJobs: string[] = [];

  async function hide(): Promise<void> {
    mode.value = null;
    // The guide's debounced progress save must land before the window goes
    // (Task 56): a hidden editor may never run its timer again.
    await useEditorOnboardingStore().flush();
    try {
      await project.port.hideWindow();
    } catch (e) {
      logWarning(`editor close guard: could not hide the editor: ${toEditorError(e).message}`);
    }
  }

  /** An unreadable registry must not strand the window: it is logged and
   * treated as "no render running", and the dirty/clean rule decides. */
  async function liveRenders(sessionId: string): Promise<string[]> {
    try {
      return liveRenderJobIds(await project.port.getJobs(sessionId));
    } catch (e) {
      logWarning(`editor close guard: could not read the job registry: ${toEditorError(e).message}`);
      return [];
    }
  }

  /** The open-take and dirty/clean half, shared by a close, a cancelled
   * render and a discarded take. */
  async function settle(): Promise<void> {
    const sessionId = project.sessionId;
    if (sessionId && openWebcamTakes(sessionId).length > 0) {
      mode.value = "take";
      return;
    }
    if (project.dirty) {
      mode.value = "dirty";
      return;
    }
    await hide();
  }

  async function request(): Promise<void> {
    if (mode.value !== null || busy.value) return;
    error.value = null;
    const sessionId = project.sessionId;
    if (!sessionId) {
      await hide();
      return;
    }
    renderJobs = await liveRenders(sessionId);
    if (renderJobs.length > 0) {
      mode.value = "render";
      return;
    }
    await settle();
  }

  /** Run one choice under `busy`, surfacing a refusal inline and keeping
   * the dialog open when `step` reports it could not finish. */
  async function run(step: () => Promise<boolean>): Promise<void> {
    busy.value = true;
    error.value = null;
    try {
      if (await step()) await hide();
    } finally {
      busy.value = false;
    }
  }

  function failed(): boolean {
    if (!project.lastError) return false;
    error.value = project.lastError.message;
    return true;
  }

  const save = () =>
    run(async () => {
      await project.save();
      if (failed()) return false;
      if (!project.dirty) return true;
      error.value = "Another change arrived while saving. Save again, or keep it for later.";
      return false;
    });

  const discard = () =>
    run(async () => {
      await project.close("discardRecovery");
      return !failed();
    });

  const keep = () => run(async () => true);

  async function cancelRender(): Promise<void> {
    const sessionId = project.sessionId;
    busy.value = true;
    try {
      for (const jobId of renderJobs) {
        if (!sessionId) break;
        try {
          await project.port.cancelJob(sessionId, jobId);
        } catch (e) {
          logWarning(`editor close guard: could not cancel ${jobId}: ${toEditorError(e).message}`);
        }
      }
      renderJobs = [];
      mode.value = null;
      await settle();
    } finally {
      busy.value = false;
    }
  }

  /** Discard every open take of this session natively; a refusal keeps the
   * dialog open and says why. */
  async function discardTakes(): Promise<void> {
    const sessionId = project.sessionId;
    busy.value = true;
    error.value = null;
    try {
      for (const takeId of sessionId ? openWebcamTakes(sessionId) : []) {
        await project.port.webcamDiscard(sessionId as string, takeId);
      }
    } catch (e) {
      error.value = toEditorError(e).message;
      return;
    } finally {
      busy.value = false;
    }
    mode.value = null;
    await settle();
  }

  function dismiss(): void {
    if (busy.value) return;
    mode.value = null;
    error.value = null;
  }

  return { mode, busy, error, request, save, keep, discard, cancelRender, discardTakes, dismiss };
}
