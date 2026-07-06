import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logBreadcrumb, logWarning } from "../logging";
import { useNotificationsStore } from "./notifications";
import type {
  CaptureRenamed,
  CaptureSaved,
  CaptureStatus,
  CaptureTranscribed,
  CaptureTranscribeFailed,
  CaptureTranscribeSkipped,
  ModelDownload,
  ModelReady,
  Phase,
  TranscribeCancelled,
  TranscribeProgress,
  TranscriptionJob,
  TranscriptionQueueStatus,
} from "../types";

/** How long the post-save "Name this recording" window stays open. */
export const RENAME_PROMPT_MS = 30_000;

/** Phases in which a job occupies the single-worker transcription queue. */
const ACTIVE_PHASES: Phase[] = ["downloading", "preparing", "transcribing"];
/** Terminal phases surfaced in the finished list. */
const FINISHED_PHASES: Phase[] = ["done", "failed", "cancelled"];
/** `finishedTranscriptions` cap — an unbounded session-long history is never useful in the UI. */
const MAX_FINISHED = 20;

function clamp01(n: number): number {
  return Math.min(1, Math.max(0, n));
}

/**
 * Vault-relative display name: basename without the `.mp3` extension. Split
 * on both separators — capture output can carry Windows paths (`\`) even
 * though tests run on Unix (mirrors `acceptRename`'s basename logic below).
 */
function nameOf(mp3: string): string {
  const base = mp3.split(/[\\/]/).pop() ?? mp3;
  return base.replace(/\.mp3$/i, "");
}

/**
 * Seed-time progress for the active job from `transcription_queue_status`.
 * "preparing" always means null (a spinner, not a percent — matches the
 * modelReady/transcribing event mappings below); "downloading" prefers the
 * received/total byte ratio (same formula as the modelDownload event) and
 * falls back to the percent field only if that ratio isn't available.
 */
function activeSeedProgress(active: NonNullable<TranscriptionQueueStatus["active"]>): number | null {
  if (active.phase === "preparing") return null;
  if (active.phase === "downloading" && active.total) {
    return clamp01((active.received ?? 0) / active.total);
  }
  return active.progress != null ? clamp01(active.progress / 100) : null;
}

export const useCaptureStore = defineStore("capture", {
  state: () => ({
    status: "idle" as "idle" | "starting" | "recording" | "saving",
    startedAtMs: null as number | null,
    /** Which vault is recording — drives the per-vault row indicator. */
    vaultId: null as string | null,
    paused: false,
    /** Accumulated pause time (authoritative value from Rust events). */
    pausedTotalMs: 0,
    /** Start of the current pause span; null while not paused. */
    pausedSinceMs: null as number | null,
    /** Advisory level meter, 0..1 (~5 Hz from capture:level). */
    level: 0,
    error: null as string | null,
    warning: null as string | null,
    lastSavedFile: null as string | null,
    /**
     * Every transcription job this window knows about, keyed by mp3 path —
     * backend-seeded on init (`transcription_queue_status`) and kept live by
     * the capture:* events. Replaces the old scattered singular fields
     * (transcribing/modelDownload/transcriptError/transcriptFailedMp3/
     * transcribingVaultId/lastTranscribed): those lived only in this store's
     * transient memory, so a panel remount (Recordings.vue is re-created
     * every time its view is left and reopened) lost track of an in-flight
     * or just-finished job. A backend-seeded keyed map survives that remount.
     */
    transcriptions: {} as Record<string, TranscriptionJob>,
    /** True when the worker has queued/regenerable work but no recording is
     * active yet to transcribe (mirrors the backend's own wait state). */
    waitingForRecording: false,
    /** Post-save rename window; null once renamed/dismissed/expired. */
    lastSaved: null as { mp3: string; note: string | null } | null,
    renameError: null as string | null,
    renameTimer: null as ReturnType<typeof setTimeout> | null,
  }),
  getters: {
    /** The one job (if any) occupying the worker right now — the queue runs
     * one job at a time, so at most one entry is ever in an active phase. */
    activeTranscription(state): TranscriptionJob | null {
      for (const job of Object.values(state.transcriptions)) {
        if (ACTIVE_PHASES.includes(job.phase)) return job;
      }
      return null;
    },
    queuedTranscriptions(state): TranscriptionJob[] {
      return Object.values(state.transcriptions).filter(
        (job) => job.phase === "queued",
      );
    },
    /**
     * Done/failed/cancelled jobs seen this session, newest-first and capped.
     * `startedAtMs` is the best available ordering proxy (there's no separate
     * finishedAt field) — the single-worker queue processes jobs serially, so
     * start order and finish order agree in practice.
     */
    finishedTranscriptions(state): TranscriptionJob[] {
      return Object.values(state.transcriptions)
        .filter((job) => FINISHED_PHASES.includes(job.phase))
        .sort((a, b) => (b.startedAtMs ?? 0) - (a.startedAtMs ?? 0))
        .slice(0, MAX_FINISHED);
    },
    /** Which vault is currently transcribing — drives the vault-row dot.
     * Derived from the map, not stored: it follows whichever job is active. */
    transcribingVaultId(): string | null {
      return this.activeTranscription?.vaultId ?? null;
    },
  },
  actions: {
    /**
     * Internal: merge `patch` into the job at `mp3` (creating a default shape
     * for a not-yet-seen job), replacing the map entry with a NEW object so
     * Vue's reactivity tracks the change — mutating a job object returned to
     * a component would not notify anything holding the old reference.
     */
    upsert(mp3: string, patch: Partial<TranscriptionJob>) {
      const prev = this.transcriptions[mp3];
      const base: TranscriptionJob = prev ?? {
        mp3,
        vaultId: "",
        name: nameOf(mp3),
        phase: "queued",
        progress: null,
        model: null,
        error: null,
        startedAtMs: null,
      };
      this.transcriptions[mp3] = { ...base, ...patch };
    },
    resetRecordingState() {
      this.status = "idle";
      this.startedAtMs = null;
      this.vaultId = null;
      this.paused = false;
      this.pausedTotalMs = 0;
      this.pausedSinceMs = null;
      this.level = 0;
    },
    async init() {
      // capture:started broadcasts to EVERY window; a non-initiating window
      // (the buddy, the bubble) never calls start(), so this broadcast is the
      // only way it learns a recording began. Without this listener the buddy
      // stayed idle and its red rec-dot + blink animation (both gated on
      // status==='recording') never appeared, even though Rust emitted the
      // event and force-showed the buddy — the broadcast was dropped.
      await listen<CaptureStatus>("capture:started", (event) => {
        const s = event.payload;
        this.status = "recording";
        this.startedAtMs = s.startedAtMs;
        this.vaultId = s.vaultId;
        this.paused = s.paused;
        this.pausedTotalMs = s.pausedTotalMs ?? 0;
        this.pausedSinceMs = s.pausedSinceMs ?? null;
        this.level = 0;
      });
      await listen<CaptureSaved>("capture:saved", (event) => {
        this.resetRecordingState();
        this.lastSavedFile = event.payload.mp3;
        // A previous stop/failure may have left a stale banner up —
        // a fresh successful save means neither is still relevant.
        this.error = null;
        this.warning = null;
        this.lastSaved = { mp3: event.payload.mp3, note: event.payload.note };
        this.renameError = null;
        this.armRenameExpiry();
        // capture:saved may carry a warning worth surfacing — an early-stop
        // reason (endedEarly, already emitted since increment 3) or a
        // post-save issue such as a failed companion note (the backend adds
        // that text). The backend forms the complete, user-ready sentence,
        // so show it verbatim rather than prefixing it here (a note-write
        // failure must NOT read "Recording ended early"). An early stop
        // with no specific reason still gets a generic note.
        if (event.payload.warning) {
          useNotificationsStore().warning(event.payload.warning);
        } else if (event.payload.endedEarly) {
          useNotificationsStore().warning("Recording ended early — saved what we had.");
        }
      });
      await listen<{ message: string }>("capture:failed", (event) => {
        this.resetRecordingState();
        this.error = event.payload.message;
        useNotificationsStore().error(event.payload.message);
      });
      await listen<{ message: string }>("capture:warning", (event) => {
        this.warning = event.payload.message;
        // Live warnings stay in the RecordingBar only — a notification would
        // pile atop a UI that already surfaces this during an active recording.
        if (this.status !== "recording") useNotificationsStore().warning(event.payload.message);
      });
      await listen<{ mp3: string; vaultId: string }>("capture:transcribing", (event) => {
        const { mp3, vaultId } = event.payload;
        // Neutral "getting ready" state, not yet "transcribing" — we don't
        // know here whether a model download is needed first. Landing this
        // as "transcribing" (the old behavior) is what produced a progress
        // bar stuck at 100% through the download; a real transcribeProgress
        // event is what earns that phase now.
        this.upsert(mp3, {
          phase: "preparing",
          vaultId,
          name: nameOf(mp3),
          progress: null,
          error: null,
          startedAtMs: Date.now(),
        });
      });
      await listen<CaptureTranscribed>("capture:transcribed", (event) => {
        this.upsert(event.payload.mp3, { phase: "done", progress: 1 });
      });
      await listen<CaptureTranscribeFailed>("capture:transcribeFailed", (event) => {
        const { mp3, message } = event.payload;
        this.upsert(mp3, { phase: "failed", error: message, progress: null });
        useNotificationsStore().error(`Transcription failed: ${message}`);
      });
      // A Complete/hand-edited transcript we refused to overwrite: a
      // complete transcript DOES exist (phase "done", like a real write),
      // but it's the user's own file, not a fresh transcription — warn
      // rather than staying silent so they know it was preserved.
      await listen<CaptureTranscribeSkipped>("capture:transcribeSkipped", (event) => {
        this.upsert(event.payload.mp3, { phase: "done", progress: 1 });
        useNotificationsStore().warning(event.payload.message);
      });
      await listen<ModelDownload>("capture:modelDownload", (event) => {
        const { mp3, model, received, total } = event.payload;
        this.upsert(mp3, {
          phase: "downloading",
          model,
          progress: total ? clamp01(received / total) : null,
        });
      });
      // The model finished downloading but inference hasn't reported
      // progress yet — back to "preparing" (not a stale 100% download bar)
      // until the first transcribeProgress event lands.
      await listen<ModelReady>("capture:modelReady", (event) => {
        this.upsert(event.payload.mp3, { phase: "preparing", progress: null });
      });
      await listen<TranscribeProgress>("capture:transcribeProgress", (event) => {
        const { mp3, progress } = event.payload;
        this.upsert(mp3, { phase: "transcribing", progress: clamp01(progress / 100) });
      });
      await listen<TranscribeCancelled>("capture:transcribeCancelled", (event) => {
        this.upsert(event.payload.mp3, { phase: "cancelled", progress: null });
      });
      await listen<{ atMs: number }>("capture:paused", (event) => {
        this.paused = true;
        this.pausedSinceMs = event.payload.atMs ?? Date.now();
        // No capture:level events arrive while paused — without this the
        // meter would freeze showing the last live peak under "Paused".
        this.level = 0;
      });
      await listen<{ pausedTotalMs: number }>("capture:resumed", (event) => {
        this.paused = false;
        this.pausedSinceMs = null;
        this.pausedTotalMs = event.payload.pausedTotalMs ?? this.pausedTotalMs;
      });
      await listen<{ peak: number }>("capture:level", (event) => {
        this.level = Math.min(1, Math.max(0, event.payload.peak ?? 0));
      });
      // Resync: the webview can reload while Rust keeps recording.
      try {
        const s = await invoke<CaptureStatus>("capture_status");
        if (s.recording) {
          this.status = "recording";
          this.startedAtMs = s.startedAtMs;
          this.vaultId = s.vaultId;
          this.paused = s.paused;
          this.pausedTotalMs = s.pausedTotalMs ?? 0;
          this.pausedSinceMs = s.pausedSinceMs ?? null;
        }
      } catch {
        // not running under Tauri (unit tests without a status mock)
      }
      // Resync: the transcription worker survives a webview reload too (and
      // a fresh panel/buddy mount otherwise starts with an empty map even
      // though a job is active/queued in Rust).
      try {
        const q = await invoke<TranscriptionQueueStatus>("transcription_queue_status");
        // Defensive like the capture_status resync above: an unmocked command
        // in tests resolves `undefined` rather than rejecting, and a
        // malformed/absent response must never crash init — just seed nothing.
        if (q) {
          this.waitingForRecording = q.waitingForRecording ?? false;
          if (q.active) {
            const a = q.active;
            this.upsert(a.mp3, {
              vaultId: a.vaultId,
              name: nameOf(a.mp3),
              phase: a.phase,
              progress: activeSeedProgress(a),
              startedAtMs: a.startedAtMs,
            });
          }
          for (const job of q.queued ?? []) {
            this.upsert(job.mp3, {
              vaultId: job.vaultId,
              name: nameOf(job.mp3),
              phase: "queued",
              progress: null,
            });
          }
        }
      } catch {
        // not running under Tauri (unit tests without a queue-status mock)
      }
    },
    async start(
      vaultId: string,
      mode: "meeting" | "voice-note" | null = null,
    ) {
      // Synchronous guard + "starting" state: without it a double-click
      // fires start_capture twice during device setup, and the second
      // call's "already running" rejection would reset the UI to idle
      // while Rust keeps recording.
      if (this.status !== "idle") return;
      this.status = "starting";
      this.error = null;
      this.warning = null;
      // New recording: the previous save's rename window is over.
      this.dismissRename();
      try {
        logBreadcrumb(`capture: start requested (vault ${vaultId})`);
        const s = await invoke<CaptureStatus>("start_capture", {
          id: vaultId,
          mode,
        });
        this.status = "recording";
        this.startedAtMs = s.startedAtMs;
        this.vaultId = s.vaultId;
        this.paused = false;
        this.pausedTotalMs = 0;
        this.pausedSinceMs = null;
        this.level = 0;
      } catch (e) {
        // Only downgrade if this attempt still owns the state — an event
        // may have moved it on in the meantime.
        if (this.status === "starting") this.status = "idle";
        this.error = String(e);
        useNotificationsStore().error(String(e));
        logWarning(`capture start rejected: ${String(e)}`);
      }
    },
    async stop() {
      if (this.status !== "recording") return;
      this.status = "saving";
      try {
        logBreadcrumb("capture: stop requested");
        await invoke("stop_capture");
        // capture:saved / capture:failed events complete the transition.
      } catch (e) {
        this.status = "idle";
        this.error = String(e);
        useNotificationsStore().error(String(e));
        logWarning(`capture stop rejected: ${String(e)}`);
      }
    },
    async cancelTranscription(mp3: string) {
      try {
        await invoke("cancel_transcription", { path: mp3 });
        // capture:transcribeCancelled completes the transition — Rust owns
        // the truth on whether/when the job actually stopped.
      } catch (e) {
        useNotificationsStore().error(`Couldn't cancel transcription: ${String(e)}`);
        logWarning(`cancel transcription rejected: ${String(e)}`);
      }
    },
    async retranscribe(mp3: string) {
      try {
        await invoke("retranscribe", { path: mp3 });
      } catch (e) {
        useNotificationsStore().error(`Couldn't re-transcribe: ${String(e)}`);
        logWarning(`retranscribe rejected: ${String(e)}`);
      }
    },
    async openTranscript(mp3: string) {
      try {
        await invoke("open_transcript", { path: mp3 });
      } catch (e) {
        // A failed open (recording moved, launch error) is non-fatal — warn
        // and leave the finished job in place so the user can retry.
        this.warning = String(e);
        useNotificationsStore().error(`Couldn't open transcript: ${String(e)}`);
        logWarning(`open transcript rejected: ${String(e)}`);
      }
    },
    async pause() {
      if (this.status !== "recording" || this.paused) return;
      try {
        await invoke("pause_capture");
        // capture:paused flips the state — Rust owns the truth.
      } catch (e) {
        this.error = String(e);
        useNotificationsStore().error(String(e));
        logWarning(`capture pause rejected: ${String(e)}`);
      }
    },
    async resume() {
      if (this.status !== "recording" || !this.paused) return;
      try {
        await invoke("resume_capture");
      } catch (e) {
        this.error = String(e);
        useNotificationsStore().error(String(e));
        logWarning(`capture resume rejected: ${String(e)}`);
      }
    },
    armRenameExpiry() {
      if (this.renameTimer) clearTimeout(this.renameTimer);
      this.renameTimer = setTimeout(() => this.dismissRename(), RENAME_PROMPT_MS);
    },
    dismissRename() {
      if (this.renameTimer) {
        clearTimeout(this.renameTimer);
        this.renameTimer = null;
      }
      this.lastSaved = null;
      this.renameError = null;
    },
    async rename(title: string) {
      if (!this.lastSaved) return;
      this.renameError = null;
      try {
        const r = await invoke<CaptureRenamed>("rename_capture", {
          mp3: this.lastSaved.mp3,
          title,
        });
        this.lastSavedFile = r.mp3;
        if (r.warning) this.warning = r.warning;
        this.dismissRename();
      } catch (e) {
        // Prompt stays up so the user can fix the title and retry.
        this.renameError = String(e);
        logWarning(`capture rename rejected: ${String(e)}`);
      }
    },
    /**
     * The prompt's single Accept button: an unchanged or emptied title
     * means "keep the timestamp name" — the pair already exists on disk,
     * so there is nothing to do but close the prompt. Only a real edit
     * calls rename_capture.
     */
    async acceptRename(title: string) {
      if (!this.lastSaved) return;
      const base = (this.lastSaved.mp3.split(/[\\/]/).pop() ?? "").replace(
        /\.mp3$/i,
        "",
      );
      const trimmed = title.trim();
      if (!trimmed || trimmed === base) {
        this.dismissRename();
        return;
      }
      await this.rename(trimmed);
    },
  },
});
