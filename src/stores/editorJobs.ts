/**
 * The editor's background jobs as the webview knows them (Task 25; P05,
 * F-02; IPC-CONTRACTS.md "Progress, cancellation and reconciliation").
 *
 * Progress reaches this store only through the per-job Channel
 * `EditorPort.importMedia` hands a callback to — never an app event — and
 * every message passes four guards before it is installed:
 *
 *   - it names the job THIS Channel was opened for (`expectedJobId`) and
 *     the session that opened it — a stranger's message is ignored;
 *   - that session is still the one open (`editorProject.sessionId`);
 *   - its `sequence` is strictly greater than the last one installed —
 *     a duplicate or a late, older message is ignored;
 *   - the job is not already terminal — once a job has its one outcome,
 *     nothing (not even another terminal) replaces it.
 *
 * A Channel message can arrive BEFORE `importMedia`'s `{ jobId }` reply
 * does; those are held and replayed through the same guards once the id is
 * known, so an early `queued`/`preparing` is neither lost nor misfiled.
 *
 * `reconcile()` asks Rust's job registry (`editor_get_jobs`) and installs
 * its answer OVER whatever the Channel said for every job still running —
 * the registry is updated before every message is sent, so it is never
 * older than the stream AT READ TIME. A job the store already holds as
 * terminal is skipped: the reply travels separately from the Channel, so a
 * read taken before the terminal can land after it. The last sequence seen
 * is kept across a reconcile, so a stale message still in flight cannot
 * drag a reconciled job backwards.
 *
 * A terminal that imported assets asks `editorProject` to re-read the
 * projection: the batch's `AddAssets` landed in Rust before the terminal
 * was sent, so the refetch sees the new assets as one committed revision.
 * The port and the session are `editorProject`'s own — one session, one
 * port, never a second copy that could drift.
 */
import { defineStore } from "pinia";

import type {
  EditorError,
  JobKind,
  JobPhase,
  JobProgressDto,
  JobRecordDto,
  JobTerminal,
} from "../editorTypes";
import { toEditorError, useEditorProjectStore } from "./editorProject";

/** One job as this store holds it. `sequence` is the last Channel message
 * installed (0 before any, kept across a reconcile). */
interface JobView {
  jobId: string;
  sessionId: string;
  kind: JobKind;
  phase: JobPhase;
  fraction: number;
  terminal: JobTerminal | null;
  sequence: number;
}

function importedSomething(terminal: JobTerminal | null): boolean {
  return (terminal?.assetIds?.length ?? 0) > 0;
}

/** `jobs` without the session's RUNNING rows that the store already held
 * before the registry was read (`heldBefore`) and that the registry no
 * longer lists (`listed`). Rust forgets a job only once its result has
 * reached its own caller — a peaks decode answers in its command's reply
 * (Task 28) — so such a row would otherwise sit "running" forever. A row
 * installed AFTER the read began is kept: the reply may simply predate it. */
function withoutForgotten(
  jobs: Record<string, JobView>,
  sessionId: string,
  heldBefore: ReadonlySet<string>,
  listed: ReadonlySet<string>,
): Record<string, JobView> {
  const forgotten = (j: JobView) =>
    j.sessionId === sessionId && j.terminal === null && heldBefore.has(j.jobId) && !listed.has(j.jobId);
  return Object.fromEntries(Object.entries(jobs).filter(([, j]) => !forgotten(j)));
}

export const useEditorJobsStore = defineStore("editorJobs", {
  state: () => ({
    /** Keyed by job id; replaced per job, never deep-mutated. */
    jobs: {} as Record<string, JobView>,
    lastError: null as EditorError | null,
  }),
  getters: {
    /** The current session's jobs, in the order they were first seen. */
    sessionJobs(state): JobView[] {
      const sessionId = useEditorProjectStore().sessionId;
      return Object.values(state.jobs).filter((j) => j.sessionId === sessionId);
    },
    /** The current session's running import, if any. */
    activeImport(): JobView | null {
      return this.sessionJobs.find((j) => j.kind === "import" && j.terminal === null) ?? null;
    },
    /** The current session's most recent FINISHED import — the library's
     * summary line and per-file errors. */
    lastImport(): JobView | null {
      const done = this.sessionJobs.filter((j) => j.kind === "import" && j.terminal !== null);
      return done[done.length - 1] ?? null;
    },
  },
  actions: {
    /** Install one Channel message under the module doc's four guards.
     * Returns whether it was installed. */
    applyProgress(expectedSessionId: string, expectedJobId: string, m: JobProgressDto): boolean {
      if (m.sessionId !== expectedSessionId || m.jobId !== expectedJobId) return false;
      if (m.sessionId !== useEditorProjectStore().sessionId) return false;
      const current = this.jobs[m.jobId];
      if (current && (current.terminal !== null || m.sequence <= current.sequence)) return false;
      this.install(m, m.sequence);
      return true;
    },
    /** Replace one job's record; a terminal that imported assets refreshes
     * the committed projection (module doc). */
    install(next: Omit<JobView, "sequence">, sequence: number): void {
      const previous = this.jobs[next.jobId];
      const wasTerminal = previous !== undefined && previous.terminal !== null;
      this.jobs = {
        ...this.jobs,
        [next.jobId]: {
          jobId: next.jobId,
          sessionId: next.sessionId,
          kind: next.kind,
          phase: next.phase,
          fraction: next.fraction,
          terminal: next.terminal,
          sequence,
        },
      };
      if (!wasTerminal && importedSomething(next.terminal)) {
        void useEditorProjectStore().refresh();
      }
    },
    /** Start an import. Rust opens its own native file dialog; this only
     * wires the Channel. Resolves the job id, or `null` when there was no
     * session or the start was refused (`lastError`). */
    async importMedia(): Promise<string | null> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      if (!sessionId) return null;
      let jobId: string | null = null;
      const early: JobProgressDto[] = [];
      const onProgress = (m: JobProgressDto) => {
        if (jobId === null) early.push(m);
        else this.applyProgress(sessionId, jobId, m);
      };
      try {
        jobId = (await project.port.importMedia(sessionId, onProgress)).jobId;
      } catch (e) {
        this.lastError = toEditorError(e);
        return null;
      }
      this.lastError = null;
      if (!this.jobs[jobId]) {
        const queued = { jobId, sessionId, kind: "import" as const, phase: "queued" as const };
        this.install({ ...queued, fraction: 0, terminal: null }, 0);
      }
      for (const m of early) this.applyProgress(sessionId, jobId, m);
      return jobId;
    },
    /** Ask Rust to stop a job's FUTURE work (what it finished stays). */
    async cancel(jobId: string): Promise<void> {
      const project = useEditorProjectStore();
      if (!project.sessionId) return;
      try {
        await project.port.cancelJob(project.sessionId, jobId);
      } catch (e) {
        this.lastError = toEditorError(e);
      }
    },
    /** Install Rust's registry over the Channel's story (module doc). */
    async reconcile(): Promise<void> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      if (!sessionId) return;
      const heldBefore = new Set(Object.keys(this.jobs));
      let rows: JobRecordDto[];
      try {
        rows = await project.port.getJobs(sessionId);
      } catch (e) {
        this.lastError = toEditorError(e);
        return;
      }
      if (project.sessionId !== sessionId) return;
      this.jobs = withoutForgotten(this.jobs, sessionId, heldBefore, new Set(rows.map((r) => r.jobId)));
      for (const row of rows) {
        const held = this.jobs[row.jobId];
        // A job the store already holds as terminal keeps its outcome: the
        // reply and the Channel travel separately, so a registry read taken
        // BEFORE the terminal can land after it (fix round 1).
        if (held && held.terminal !== null) continue;
        this.install({ ...row, sessionId }, held?.sequence ?? 0);
      }
    },
  },
});
