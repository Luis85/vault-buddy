import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  CaptureSaved,
  CaptureStatus,
  CaptureTranscribed,
  CaptureTranscribeFailed,
  ModelDownload,
} from "../types";

export const useCaptureStore = defineStore("capture", {
  state: () => ({
    status: "idle" as "idle" | "starting" | "recording" | "saving",
    startedAtMs: null as number | null,
    error: null as string | null,
    warning: null as string | null,
    lastSavedFile: null as string | null,
    transcribing: false as boolean,
    transcriptError: null as string | null,
    transcriptFailedMp3: null as string | null,
    modelDownload: null as { model: string; received: number; total: number | null } | null,
  }),
  actions: {
    async init() {
      await listen<CaptureSaved>("capture:saved", (event) => {
        this.status = "idle";
        this.startedAtMs = null;
        this.lastSavedFile = event.payload.mp3;
        // A previous stop/failure may have left a stale banner up —
        // a fresh successful save means neither is still relevant.
        this.error = null;
        this.warning = null;
      });
      await listen<{ message: string }>("capture:failed", (event) => {
        this.status = "idle";
        this.startedAtMs = null;
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
      // Resync: the webview can reload while Rust keeps recording.
      try {
        const s = await invoke<CaptureStatus>("capture_status");
        if (s.recording) {
          this.status = "recording";
          this.startedAtMs = s.startedAtMs;
        }
      } catch {
        // not running under Tauri (unit tests without a status mock)
      }
    },
    async start(vaultId: string) {
      // Synchronous guard + "starting" state: without it a double-click
      // fires start_capture twice during device setup, and the second
      // call's "already running" rejection would reset the UI to idle
      // while Rust keeps recording.
      if (this.status !== "idle") return;
      this.status = "starting";
      this.error = null;
      this.warning = null;
      try {
        const s = await invoke<CaptureStatus>("start_capture", { id: vaultId });
        this.status = "recording";
        this.startedAtMs = s.startedAtMs;
      } catch (e) {
        // Only downgrade if this attempt still owns the state — an event
        // may have moved it on in the meantime.
        if (this.status === "starting") this.status = "idle";
        this.error = String(e);
      }
    },
    async stop() {
      if (this.status !== "recording") return;
      this.status = "saving";
      try {
        await invoke("stop_capture");
        // capture:saved / capture:failed events complete the transition.
      } catch (e) {
        this.status = "idle";
        this.error = String(e);
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
  },
});
