import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { logBreadcrumb, logWarning } from "../logging";
import type {
  CaptureRenamed,
  CaptureSaved,
  CaptureStatus,
  CaptureTranscribed,
  CaptureTranscribeFailed,
  ModelDownload,
} from "../types";

/** How long the post-save "Name this recording" window stays open. */
export const RENAME_PROMPT_MS = 30_000;

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
    transcribing: false as boolean,
    transcriptError: null as string | null,
    transcriptFailedMp3: null as string | null,
    modelDownload: null as { model: string; received: number; total: number | null } | null,
    /** Post-save rename window; null once renamed/dismissed/expired. */
    lastSaved: null as { mp3: string; note: string | null } | null,
    renameError: null as string | null,
    renameTimer: null as ReturnType<typeof setTimeout> | null,
  }),
  actions: {
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
      });
      await listen<{ message: string }>("capture:failed", (event) => {
        this.resetRecordingState();
        this.error = event.payload.message;
      });
      await listen<{ message: string }>("capture:warning", (event) => {
        this.warning = event.payload.message;
      });
      await listen<{ mp3: string }>("capture:transcribing", () => {
        this.transcribing = true;
        this.transcriptError = null;
      });
      await listen<CaptureTranscribed>("capture:transcribed", () => {
        this.transcribing = false;
        this.modelDownload = null;
      });
      await listen<CaptureTranscribeFailed>("capture:transcribeFailed", (event) => {
        this.transcribing = false;
        this.modelDownload = null;
        this.transcriptError = event.payload.message;
        this.transcriptFailedMp3 = event.payload.mp3;
      });
      await listen<ModelDownload>("capture:modelDownload", (event) => {
        this.modelDownload = event.payload;
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
        logWarning(`capture stop rejected: ${String(e)}`);
      }
    },
    async retryTranscription() {
      if (!this.transcriptFailedMp3) return;
      const path = this.transcriptFailedMp3;
      this.transcriptError = null;
      try {
        await invoke("transcribe_recording_now", { path });
        this.transcribing = true;
      } catch (e) {
        this.transcriptError = String(e);
      }
    },
    async pause() {
      if (this.status !== "recording" || this.paused) return;
      try {
        await invoke("pause_capture");
        // capture:paused flips the state — Rust owns the truth.
      } catch (e) {
        this.error = String(e);
        logWarning(`capture pause rejected: ${String(e)}`);
      }
    },
    async resume() {
      if (this.status !== "recording" || !this.paused) return;
      try {
        await invoke("resume_capture");
      } catch (e) {
        this.error = String(e);
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
