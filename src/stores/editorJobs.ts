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
 *
 * **Renders (Task 47)** ride the same Channel discipline (`track`). A
 * terminal naming a `productId` asks `editorProducts` to re-read the
 * ledger; a Review's terminal names none (F18), so a review never touches
 * the product library. A render's refusal lands in `renderError`, never in
 * `lastError` (the media library's import line) and never in
 * `editorProject.saveError` (the header's "Save failed") — Task 46's carry.
 *
 * **A render nobody is following (hardening Task 15, GAP-208).** After a
 * reload of the editor webview a render Rust is still running has no
 * Channel here; the reconcile that runs for every new session is the first
 * to see it. A RUNNING render the store did not hold before that read is
 * `adopted` — `RenderVideoButton` opens the Render dialog on it, and
 * `follow` polls `editor_get_jobs` every `FOLLOW_INTERVAL_MS` until its
 * terminal, the only progress source left. `follow`'s own polling reads
 * never touch `lastError` — the media library's import line — even on a
 * failed read (`reconcile`'s `silent` option, hardening Task 16 review
 * carry): a render nobody's library card is even about must not spend that
 * field once a second for as long as the registry stays unreachable.
 *
 * `track`'s own start is a WINDOW a reconcile can race, too (hardening Task
 * 16 review carry): Rust can already list a render job's row before this
 * webview's `start()` call gets its `{jobId}` reply (`editor_start_render`
 * "answers at once"), and adopting it then would be this webview's OWN
 * render stacking the Render dialog under whatever started it. `startingRenders`
 * counts render starts in flight and blanks `unfollowedRenders` while any is
 * — safe because only one render job runs per session, so any render row
 * seen during that window can only be the one being started.
 *
 * **Session hygiene (hardening Task 16, C-4).** `install` never writes a job
 * whose `sessionId` is not the CURRENTLY open session — `track`'s `{jobId}`
 * reply can land after the session that started it has since closed or been
 * superseded, and installing it anyway would leave a row `sessionJobs` can
 * never surface again (nothing ever reads a stale session's rows) sitting in
 * this store forever. `forgetSession(sessionId)` removes what a session
 * already accumulated before that guard existed to stop it — a session that
 * opens a render, say, and is discarded before it finishes. It is called
 * from `EditorRoot`'s `wireJobHygiene` (an `editorProject.$onAction`
 * subscription over `beginOpen`/`close`, not a static import of this module
 * into `editorProject.ts`: that would be a back-edge into the cycle this
 * module's own import of `editorProject` already runs the other way).
 */
import { defineStore } from "pinia";

import type {
  EditorError,
  JobKind,
  JobPhase,
  JobProgressDto,
  JobRecordDto,
  JobTerminal,
  RenderQuality,
  RenderRange,
  RenderRequest,
} from "../editorTypes";
import { useEditorProductsStore } from "./editorProducts";
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

/** What a caller chooses for a render; the session and the revision to
 * freeze are this store's to fill in. */
interface RenderOptions {
  name: string;
  range: RenderRange | null;
  quality: RenderQuality;
  review?: boolean;
}

type StartJob = (onProgress: (m: JobProgressDto) => void) => Promise<{ jobId: string }>;

/** How often `follow` re-reads the registry for a render with no Channel. */
const FOLLOW_INTERVAL_MS = 1000;

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** The registry's running renders the store does not hold — renders this
 * webview has no Channel for (module doc). Read against what the store holds
 * when the reply lands, so a render whose Channel opened while the read was
 * in flight is not mistaken for one. `startingRenders` blanks the whole
 * result while a render start from THIS webview is still in flight (module
 * doc's "track's own start is a window a reconcile can race, too") — only
 * one render job runs per session, so any render row seen during that window
 * can only be the one being started, never a genuine orphan. */
function unfollowedRenders(rows: JobRecordDto[], held: Record<string, JobView>, startingRenders: number): string[] {
  if (startingRenders > 0) return [];
  return rows.filter((r) => r.kind === "render" && r.terminal === null && !held[r.jobId]).map((r) => r.jobId);
}

/** Whether a registry ROW should be skipped rather than installed over what
 * the store already holds for that job — split out of `reconcile`'s own
 * loop to keep its branch count down (hardening Task 16, F-M6). Two cases:
 * the store already holds the job's outcome (a terminal is never replaced —
 * the reply and the Channel travel separately, so a registry read taken
 * BEFORE the terminal can land after it, fix round 1), or the row is a
 * STALE read behind a Channel-tracked job's own progress — a registry read
 * can race a Channel message already applied at a HIGHER sequence, since
 * the module doc's "never older than the stream AT READ TIME" holds at
 * Rust's SEND, not at this reply's ARRIVAL. */
function staleRegistryRow(held: JobView | undefined, row: JobRecordDto): boolean {
  if (!held) return false;
  if (held.terminal !== null) return true;
  return held.sequence > 0 && row.fraction < held.fraction;
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
    /** The last refused render or review start (or a refused cancel of
     * one) — the Render/Review dialogs' own, never `lastError`. */
    renderError: null as EditorError | null,
    /** Running renders a reconcile found with no Channel in this webview
     * (module doc) and nothing follows yet. */
    adopted: [] as string[],
    /** Render starts from THIS webview currently awaiting their `{jobId}`
     * reply — see `unfollowedRenders`' own doc. Incremented/decremented by
     * `track`, never read outside this module. */
    startingRenders: 0,
  }),
  getters: {
    /** The current session's jobs, in the order they were first seen. */
    sessionJobs(state): JobView[] {
      const sessionId = useEditorProjectStore().sessionId;
      return Object.values(state.jobs).filter((j) => j.sessionId === sessionId);
    },
    /** The current session's first adopted render still running, if any. */
    adoptedRender(): string | null {
      const job = this.sessionJobs.find((j) => this.adopted.includes(j.jobId) && j.terminal === null);
      return job?.jobId ?? null;
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
     * the committed projection (module doc). Never writes for a session that
     * is not the one CURRENTLY open (module doc's "session hygiene") — a
     * late `track` reply for a session already closed or superseded must
     * install nothing, or it leaves a row `sessionJobs` can never surface
     * again sitting in this store forever. */
    install(next: Omit<JobView, "sequence">, sequence: number): void {
      if (next.sessionId !== useEditorProjectStore().sessionId) return;
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
      if (!wasTerminal && next.terminal?.productId) {
        void useEditorProductsStore().refresh();
      }
    },
    /** Open one job's Channel through `start` and resolve its id. Messages
     * that beat the `{ jobId }` reply are held and replayed through the
     * same guards; a refused start rejects (the caller files the error). */
    async track(kind: JobKind, sessionId: string, start: StartJob): Promise<string> {
      let jobId: string | null = null;
      const early: JobProgressDto[] = [];
      const onProgress = (m: JobProgressDto) => {
        if (jobId === null) early.push(m);
        else this.applyProgress(sessionId, jobId, m);
      };
      // Counted for the whole round trip, render starts only — a reconcile
      // racing THIS window must not adopt the row Rust already lists for a
      // job this webview is itself about to claim (`unfollowedRenders`' own
      // doc). Decremented in `finally` so a refused start never leaks it.
      if (kind === "render") this.startingRenders += 1;
      try {
        jobId = (await start(onProgress)).jobId;
      } finally {
        if (kind === "render") this.startingRenders -= 1;
      }
      if (!this.jobs[jobId]) {
        this.install({ jobId, sessionId, kind, phase: "queued", fraction: 0, terminal: null }, 0);
      }
      for (const m of early) this.applyProgress(sessionId, jobId, m);
      return jobId;
    },
    /** Start an import. Rust opens its own native file dialog; this only
     * wires the Channel. Resolves the job id, or `null` when there was no
     * session or the start was refused (`lastError`). */
    async importMedia(): Promise<string | null> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      if (!sessionId) return null;
      try {
        const jobId = await this.track("import", sessionId, (cb) => project.port.importMedia(sessionId, cb));
        this.lastError = null;
        return jobId;
      } catch (e) {
        this.lastError = toEditorError(e);
        return null;
      }
    },
    /** Start a render (or, with `review`, a Review render) of the revision
     * on screen — Rust refuses a pending edit (`revisionConflict`).
     * Resolves the job id, or `null` when there was no session or it was
     * refused (`renderError`). */
    async startRender(options: RenderOptions): Promise<string | null> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      const revision = project.snapshot?.revision;
      if (!sessionId || revision === undefined) return null;
      const { review, ...chosen } = options;
      const request: RenderRequest = {
        sessionId,
        expectedRevision: revision,
        ...chosen,
        ...(review ? { review: true } : {}),
      };
      try {
        const jobId = await this.track("render", sessionId, (cb) => project.port.startRender(request, cb));
        this.renderError = null;
        return jobId;
      } catch (e) {
        this.renderError = toEditorError(e);
        return null;
      }
    },
    /** Ask Rust to stop a job's FUTURE work (what it finished stays). A
     * refused cancel of a render is the render's error, not the library's. */
    async cancel(jobId: string): Promise<void> {
      const project = useEditorProjectStore();
      if (!project.sessionId) return;
      try {
        await project.port.cancelJob(project.sessionId, jobId);
      } catch (e) {
        if (this.jobs[jobId]?.kind === "render") this.renderError = toEditorError(e);
        else this.lastError = toEditorError(e);
      }
    },
    /** Install Rust's registry over the Channel's story (module doc).
     * `silent` (module doc's "follow's own polling reads") skips a failed
     * read's usual `lastError` write — `follow`'s own use, so a render
     * nobody's library card is about does not spend that field once a
     * second for as long as a stalled registry stays unreachable. */
    async reconcile(opts: { silent?: boolean } = {}): Promise<void> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      if (!sessionId) return;
      const heldBefore = new Set(Object.keys(this.jobs));
      let rows: JobRecordDto[];
      try {
        rows = await project.port.getJobs(sessionId);
      } catch (e) {
        if (!opts.silent) this.lastError = toEditorError(e);
        return;
      }
      if (project.sessionId !== sessionId) return;
      this.jobs = withoutForgotten(this.jobs, sessionId, heldBefore, new Set(rows.map((r) => r.jobId)));
      this.adopted = [...this.adopted, ...unfollowedRenders(rows, this.jobs, this.startingRenders)];
      for (const row of rows) {
        const held = this.jobs[row.jobId];
        if (staleRegistryRow(held, row)) continue;
        this.install({ ...row, sessionId }, held?.sequence ?? 0);
      }
    },
    /** Follow an adopted render (module doc): re-read the registry every
     * `FOLLOW_INTERVAL_MS` until the job has its terminal, leaves the
     * registry or the session changes. A failed read is retried on the next
     * tick — only a terminal ends a render — and never touches `lastError`
     * (`reconcile`'s `silent` option; hardening Task 16 review carry). */
    async follow(jobId: string): Promise<void> {
      this.adopted = this.adopted.filter((id) => id !== jobId);
      const sessionId = useEditorProjectStore().sessionId;
      const following = () => this.jobs[jobId]?.terminal === null && useEditorProjectStore().sessionId === sessionId;
      while (following()) {
        await sleep(FOLLOW_INTERVAL_MS);
        await this.reconcile({ silent: true });
      }
    },
    /** Forget a closed or superseded session's jobs and any adoption record
     * of them (module doc's "session hygiene", hardening Task 16, C-4) — for
     * the session being left behind, nothing ever reads a stale session's
     * rows again, so leaving them in `jobs` is a pure, permanent leak. */
    forgetSession(sessionId: string): void {
      this.jobs = Object.fromEntries(Object.entries(this.jobs).filter(([, j]) => j.sessionId !== sessionId));
      this.adopted = this.adopted.filter((id) => id in this.jobs);
    },
  },
});
