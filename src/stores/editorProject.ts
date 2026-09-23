/**
 * The tutorial editor's project-authority store (Task 14; P03, F-13/F-40/
 * F-44; ARCHITECTURE-AND-STACK.md "Pinia state boundaries": `editorProject`
 * = "Composition committed by Rust — current revision, validated graph/
 * projection, saved revision, undo/redo availability"). It owns Rust's
 * COMMITTED truth for one editor session — snapshot, the full project
 * graph, the missing-media list, and the staged capture's source base —
 * and nothing else: view-only state (selection, playhead, zoom, panel
 * layout) belongs to a later `editorWorkspace` store, never here (R14's
 * split between committed truth and local view preference).
 *
 * Every mutation this store makes comes from an ACKNOWLEDGED Rust reply,
 * never a local guess — and even an acknowledged reply is installed only
 * when every guard for its kind holds:
 *
 *   - `generation` (bumped on every open/close) must be unchanged since the
 *     request was sent — a slower reply from a session this store has
 *     since moved on from is discarded no matter what it says;
 *   - the reply's own `sessionId` must match this store's CURRENT session —
 *     `generation` alone cannot see a same-generation reply that names a
 *     different session, and the invariant is cheap to state unconditionally
 *     even though real opens never reuse an id;
 *   - an edit's `revision` may only move the story FORWARD — Rust replays a
 *     repeated `commandId` with the CURRENT snapshot unchanged, so a resent
 *     command must never regress state it already advanced past;
 *   - a save receipt's `savedRevision` may not be AHEAD of the live
 *     revision (A18) — a receipt for an older revision than the one now
 *     open still gets recorded (it proves that much reached disk), it just
 *     cannot clear `dirty` if a newer edit landed while the save was still
 *     in flight.
 *
 * `project` is replaced wholesale on every accepted reply, never
 * deep-mutated in place — the `shallowRef` discipline this task's brief
 * names. Pinia's own `reactive()` still wraps the state object, but nothing
 * in this file ever assigns into `project.clips`/`project.tracks`/etc.;
 * only `this.project = <a whole new object from Rust>`. That is what keeps
 * "the project graph is exactly what Rust last said" true by construction
 * rather than by discipline nobody can check.
 *
 * A `revisionConflict` neither installs a reply nor resends the command
 * automatically (R20: no silent retry) — it refetches the authoritative
 * snapshot via `getSnapshot` and parks the rejected command in
 * `conflictIntent` so a caller can offer an explicit Retry
 * (`store.retryConflict()`). `conflictIntent` is cleared the moment any
 * `execute()` reply actually installs — a retry of the SAME command or a
 * genuinely unrelated edit both count as forward progress past whatever
 * revision the conflict was rejected against (fix round 1).
 */
import { defineStore } from "pinia";
import { markRaw } from "vue";

import type { EditorPort } from "../editor/port";
import { createTauriEditorPort, EditorPortError } from "../editor/port";
import type {
  Clip,
  CloseDisposition,
  EditorCommand,
  EditorError,
  EditorOpenResult,
  EditorProjection,
  EditorSnapshot,
  MissingMedia,
  Project,
  SaveReceipt,
  Track,
} from "../editorTypes";

/**
 * Mints a `commandId` matching the backend's `^[a-zA-Z0-9_-]{1,100}$`
 * regex without `crypto.randomUUID()`: every character this produces
 * (base36 digits/letters plus a literal `-` separator) is inside that
 * alphabet by construction, so there is nothing to validate afterward. A
 * monotonic counter rides alongside the clock and a random tail so two
 * commands minted in the same millisecond (a fast double-edit) still get
 * distinct ids.
 */
let commandSequence = 0;
function nextCommandId(): string {
  commandSequence += 1;
  const time = Date.now().toString(36);
  const seq = commandSequence.toString(36);
  const rand = Math.floor(Math.random() * 46656).toString(36); // 36^3, base36
  return `cmd-${time}-${seq}-${rand}`;
}

/** Every `EditorPort` method throws only `EditorPortError` (its own module
 * doc) — unwrap the decoded `EditorError` it carries. Anything else caught
 * here (a bug in a test double, a non-Error throw) becomes a synthetic
 * `internal` error rather than crashing an action's catch block. */
export function toEditorError(e: unknown): EditorError {
  if (e instanceof EditorPortError) return e.error;
  const message = e instanceof Error ? e.message : String(e);
  return { code: "internal", message, retryable: false, operationId: "store-local" };
}

export const useEditorProjectStore = defineStore("editorProject", {
  state: () => ({
    /**
     * The injected `EditorPort` (F31, R14). `markRaw` so Vue's reactive
     * proxy never wraps it — the same reason `stores/updates.ts` wraps its
     * native `Update` handle: this holds no state worth tracking, and
     * proxying a test double's closures (or, in production, `invoke`'s own
     * plumbing) is exactly the surprise `markRaw` exists to prevent.
     * Defaults to the real Tauri port so production code never has to call
     * `setPort`; tests swap it for a fake before driving any action.
     */
    port: markRaw(createTauriEditorPort()) as EditorPort,
    sessionId: null as string | null,
    /** Bumped on every open/close — see the module doc's first guard. */
    generation: 0,
    snapshot: null as EditorSnapshot | null,
    /** The full project graph. Replace wholesale; never deep-mutate. */
    project: null as Project | null,
    missing: [] as MissingMedia[],
    sourceBase: null as string | null,
    /** `commandId`s currently awaiting a Rust reply. More than one can be
     * in flight at once — Rust's replay-on-repeat-id makes a stale command
     * a safe no-op, so this store never needs to serialize `execute`
     * calls against each other, only guard which REPLY it installs. */
    pending: new Set<string>(),
    lastError: null as EditorError | null,
    /** The command a `revisionConflict` rejected, kept for an explicit
     * Retry. `execute` never re-sends it on its own. */
    conflictIntent: null as EditorCommand | null,
    /**
     * True only while `save()`'s round trip is genuinely outstanding (Task
     * 16, F-48) — `EditorHeader`'s "Saving…" status text derives from this
     * rather than a timer, so a save that never resolves (a hung write) never
     * silently reads "Saved" and a fast one never gets stuck showing
     * "Saving…" past its own receipt. Reset on `openWith`/`close` too, so a
     * session switch mid-save can never leave a NEW session reading
     * `saving: true` for a request it never sent.
     */
    saving: false,
  }),
  getters: {
    /** Derived, never stored: two copies of "does disk match memory" are
     * two chances to disagree (see the "dirty is derived, not stored"
     * test). `null !== number` is true by JS's own rule, so an unsaved
     * (`persistedRevision: null`) project reads dirty with no extra null
     * check. */
    dirty(state): boolean {
      if (!state.snapshot) return false;
      return state.snapshot.persistedRevision !== state.snapshot.revision;
    },
    canUndo(state): boolean {
      return state.snapshot?.canUndo ?? false;
    },
    canRedo(state): boolean {
      return state.snapshot?.canRedo ?? false;
    },
    durationMs(state): number {
      return state.snapshot?.durationMs ?? 0;
    },
    clipById(state) {
      return (id: string): Clip | undefined => state.project?.clips.find((c) => c.id === id);
    },
    trackById(state) {
      return (id: string): Track | undefined => state.project?.tracks.find((t) => t.id === id);
    },
  },
  actions: {
    /** Test-only dependency injection (the `editorPort.test.ts` fake-port
     * precedent, applied here as an action rather than a constructor
     * argument so it fits this repo's options-store style). Production
     * code never calls this — the real Tauri port from `state()` stands. */
    setPort(port: EditorPort) {
      this.port = markRaw(port);
    },
    /**
     * Shared open path for `openStaged`/`openProject`. `generation` bumps
     * FIRST, before the request is even sent — so even a FAILED open
     * invalidates every command still in flight from whatever was open
     * before (a stale command landing just because the open that
     * superseded it didn't pan out would be exactly the "silent success"
     * R20 forbids). A failure keeps whatever was open before (which may be
     * nothing) and surfaces `lastError` — it never blanks a working
     * session over a picker mis-click.
     */
    async openWith(run: () => Promise<EditorOpenResult>): Promise<void> {
      this.generation += 1;
      const generation = this.generation;
      this.pending.clear();
      this.conflictIntent = null;
      this.lastError = null;
      this.saving = false;
      try {
        const result = await run();
        if (generation !== this.generation) return;
        this.sessionId = result.snapshot.sessionId;
        this.snapshot = result.snapshot;
        this.project = result.project;
        this.missing = result.missing;
        this.sourceBase = result.sourceBase;
      } catch (e) {
        if (generation !== this.generation) return;
        this.lastError = toEditorError(e);
      }
    },
    openProject(id: string, useRecovery: boolean): Promise<void> {
      return this.openWith(() => this.port.openProject(id, useRecovery));
    },
    /**
     * Task 15: `EditorRoot` calls this UNCONDITIONALLY on every base it
     * drains from the stash — both `mount` and every `editor:open` — rather
     * than tracking "did I already open this" itself, so the guard against
     * reopening a LIVE session for the same capture has to live here. A
     * duplicate Edit click, or a second `open_capture_editor` for the
     * capture already showing, re-stashes the same base and re-emits
     * `editor:open`; without this a re-drain would spend a second
     * `editor_open_staged` round trip re-minting local state Rust's own
     * `open_staged_session_reuses_a_live_session` already keeps idempotent
     * server-side, and would needlessly reset `missing`/`workspace` under
     * the caller's feet.
     *
     * `sourceBase` (not the `base` argument of some LAST call) is the field
     * to compare against, because it is what the PREVIOUS open's own reply
     * reported — the same round-trip guarantee `sessionId` gives the rest of
     * this store. `sessionId !== null` is the other half: a base can equal
     * `sourceBase` while no session is open at all (a fresh store, or one
     * `close()` just cleared), and `close()` nulls `sourceBase` for exactly
     * this reason, so a reopen after closing is never short-circuited.
     */
    openStaged(base: string): Promise<void> {
      if (this.sourceBase === base && this.sessionId !== null) return Promise.resolve();
      return this.openWith(() => this.port.openStaged(base));
    },
    /**
     * Install a successful `execute()` reply only when every module-doc
     * guard holds: same generation, same session, and a strictly forward
     * revision. Split out of `execute` so each guard reads as one line
     * here rather than adding to that function's own branch count.
     */
    applyExecuteResult(generation: number, result: EditorProjection): void {
      if (generation !== this.generation) return;
      if (result.snapshot.sessionId !== this.sessionId) return;
      if (!this.snapshot || result.snapshot.revision <= this.snapshot.revision) return;
      this.snapshot = result.snapshot;
      this.project = result.project;
      this.lastError = null;
      // Forward progress supersedes a pending conflict (fix round 1): a
      // command that installs — whether it's a retry of the parked
      // `conflictIntent` or a genuinely unrelated edit — means the
      // revision the conflict was rejected against is no longer the story.
      // Leaving `conflictIntent` set here is what let a resolved conflict
      // keep looking unresolved, and a stale Retry affordance re-send an
      // already-applied command with a FRESH `commandId` that Rust's
      // commandId-keyed replay dedup cannot recognize as a repeat.
      this.conflictIntent = null;
    },
    /**
     * After a `revisionConflict`, refetch the authoritative snapshot — the
     * command itself is never resent here, only the truth it was rejected
     * against. Installs the refetched pair under the same generation/
     * session guards as every other reply; a refetch failure surfaces
     * through `lastError` instead of leaving the conflict silently
     * unresolved.
     */
    async refetchAfterConflict(generation: number, sessionId: string): Promise<void> {
      try {
        const fresh = await this.port.getSnapshot(sessionId, null);
        if (generation !== this.generation) return;
        if (fresh.snapshot.sessionId === this.sessionId) {
          this.snapshot = fresh.snapshot;
          this.project = fresh.project;
        }
      } catch (e) {
        if (generation === this.generation) this.lastError = toEditorError(e);
      }
    },
    /**
     * Re-read the committed projection after an edit Rust made on its OWN
     * (Task 25: a finished import's `AddAssets`, which no `execute` reply
     * carries). Installed under the usual generation/session guards, and
     * never BEHIND the revision already shown — a refresh racing a newer
     * `execute` reply must not roll it back.
     */
    async refresh(): Promise<void> {
      if (!this.sessionId) return;
      const generation = this.generation;
      const sessionId = this.sessionId;
      try {
        const fresh = await this.port.getSnapshot(sessionId, null);
        if (generation !== this.generation || fresh.snapshot.sessionId !== this.sessionId) return;
        if (this.snapshot && fresh.snapshot.revision < this.snapshot.revision) return;
        this.snapshot = fresh.snapshot;
        this.project = fresh.project;
      } catch (e) {
        if (generation === this.generation) this.lastError = toEditorError(e);
      }
    },
    /**
     * Route a rejected `execute()` call: a `revisionConflict` parks
     * `command` in `conflictIntent` (never re-sent automatically, R20) and
     * refetches; every other error just surfaces as `lastError`, leaving
     * the current project/snapshot untouched.
     */
    async handleExecuteError(
      generation: number,
      sessionId: string,
      command: EditorCommand,
      e: unknown,
    ): Promise<void> {
      if (generation !== this.generation) return;
      const error = toEditorError(e);
      if (error.code !== "revisionConflict") {
        this.lastError = error;
        return;
      }
      this.conflictIntent = command;
      await this.refetchAfterConflict(generation, sessionId);
    },
    /**
     * Send one edit. `applyExecuteResult`/`handleExecuteError` carry the
     * actual guard logic (see their own docs); this is only the request/
     * response plumbing — mint a `commandId`, track it in `pending` while
     * the round trip is outstanding, and dispatch the reply. Resolves
     * `true` when Rust acknowledged the command and `false` when it was
     * refused (or there was no session to send it to) — the refusal itself
     * still surfaces through `lastError`; the boolean only lets a caller
     * holding a provisional value (an inspector draft) drop it (Task 21 fix
     * round 1).
     */
    async execute(command: EditorCommand): Promise<boolean> {
      if (!this.snapshot || !this.sessionId) return false;
      const generation = this.generation;
      const sessionId = this.sessionId;
      const expectedRevision = this.snapshot.revision;
      const commandId = nextCommandId();
      this.pending.add(commandId);
      try {
        const result = await this.port.execute({ sessionId, expectedRevision, commandId, command });
        this.applyExecuteResult(generation, result);
        return true;
      } catch (e) {
        await this.handleExecuteError(generation, sessionId, command, e);
        return false;
      } finally {
        this.pending.delete(commandId);
      }
    },
    /**
     * Import a subtitle file onto `clipId` (Task 36): Rust opens its own
     * dialog and applies the whole file as ONE edit, so the reply installs
     * exactly like an `execute` reply (same generation/session/revision
     * guards). Resolves the counts for the caller's status line, or `null`
     * -- a cancelled dialog, no session, or a refusal (which, like every
     * other refused edit, surfaces through `lastError`).
     */
    async importCaptions(clipId: string, replace: boolean): Promise<{ imported: number; skipped: number } | null> {
      if (!this.snapshot || !this.sessionId) return null;
      const generation = this.generation;
      try {
        const result = await this.port.importCaptions(this.sessionId, clipId, replace);
        if (!result) return null;
        this.applyExecuteResult(generation, result.projection);
        return { imported: result.imported, skipped: result.skipped };
      } catch (e) {
        if (generation === this.generation) this.lastError = toEditorError(e);
        return null;
      }
    },
    /**
     * Resend the command parked in `conflictIntent` (fix round 1) — the
     * explicit Retry affordance `execute` itself deliberately never drives
     * on its own (R20). Clears `conflictIntent` BEFORE awaiting `execute`,
     * not after: a double-click fires this twice back to back with no
     * `await` between the calls, and since the clear happens synchronously
     * at the start of the function body, the SECOND call already reads
     * `conflictIntent` as null and returns without ever touching the port
     * — a fresh `commandId` on a second send is exactly what Rust's
     * commandId-keyed replay dedup cannot catch, so this is the only place
     * that dedup has to happen. A no-op when nothing is parked.
     */
    async retryConflict(): Promise<void> {
      const command = this.conflictIntent;
      if (!command) return;
      this.conflictIntent = null;
      await this.execute(command);
    },
    /**
     * Install a successful `save()` receipt, guarded exactly like
     * `applyExecuteResult`: same generation, same session, and A18 (a
     * receipt may not report a revision AHEAD of what's live). Split out of
     * `save` for the same reason `applyExecuteResult` is split out of
     * `execute` — one guard per line here, not a branch added to `save`'s
     * own count (Task 16 pushed `save` over the fallow complexity
     * threshold by inlining these checks alongside the new `saving` guard).
     */
    applySaveReceipt(generation: number, receipt: SaveReceipt): void {
      if (generation !== this.generation) return;
      if (receipt.sessionId !== this.sessionId) return;
      if (!this.snapshot || receipt.savedRevision > this.snapshot.revision) return;
      this.snapshot = { ...this.snapshot, persistedRevision: receipt.savedRevision };
      this.lastError = null;
    },
    /**
     * Persist the current revision. Installs `persistedRevision` from the
     * receipt only when its `sessionId` matches this session AND
     * `savedRevision` is not AHEAD of the live revision (A18) — a receipt
     * for an older revision than the one now open is still recorded (it
     * proves that much reached disk); it just cannot clear `dirty` on its
     * own if a newer edit landed while the save was in flight.
     */
    async save(): Promise<void> {
      if (!this.snapshot || !this.sessionId) return;
      const generation = this.generation;
      const sessionId = this.sessionId;
      const expectedRevision = this.snapshot.revision;
      this.saving = true;
      try {
        const receipt = await this.port.save(sessionId, expectedRevision);
        this.applySaveReceipt(generation, receipt);
      } catch (e) {
        if (generation === this.generation) this.lastError = toEditorError(e);
      } finally {
        // Guarded like every other post-await write in this file: a save
        // from a superseded generation must not clear `saving` for whatever
        // session/open is current now (it would race `openWith`'s own reset
        // the other way and could clear a flag a NEWER save just set).
        if (generation === this.generation) this.saving = false;
      }
    },
    /**
     * Tell Rust the session is done and drop every local trace of it.
     * `generation` bumps FIRST, the same reasoning as `openWith`: any
     * command or save still in flight from this session must land as a
     * no-op regardless of whether the close call itself succeeds. Local
     * state clears synchronously (before the first `await`) so a caller
     * never waits on the network to see the session gone; the close
     * request is still awaited so a failure (e.g. a refused
     * `discardProject`) can surface through `lastError`, even though the
     * local session it describes is already empty.
     */
    async close(disposition: CloseDisposition): Promise<void> {
      const sessionId = this.sessionId;
      if (!sessionId) return;
      this.generation += 1;
      const generation = this.generation;
      this.pending.clear();
      this.conflictIntent = null;
      this.sessionId = null;
      this.snapshot = null;
      this.project = null;
      this.missing = [];
      this.sourceBase = null;
      this.lastError = null;
      this.saving = false;
      try {
        await this.port.closeSession(sessionId, disposition);
      } catch (e) {
        if (generation === this.generation) this.lastError = toEditorError(e);
      }
    },
  },
});
