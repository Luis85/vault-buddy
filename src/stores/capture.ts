import { defineStore } from "pinia";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CaptureSaved, CaptureStatus } from "../types";

export const useCaptureStore = defineStore("capture", {
  state: () => ({
    status: "idle" as "idle" | "recording" | "saving",
    vaultId: null as string | null,
    startedAtMs: null as number | null,
    error: null as string | null,
    warning: null as string | null,
    lastSavedFile: null as string | null,
  }),
  actions: {
    async init() {
      await listen<CaptureSaved>("capture:saved", (event) => {
        this.status = "idle";
        this.vaultId = null;
        this.startedAtMs = null;
        this.lastSavedFile = event.payload.mp3;
      });
      await listen<{ message: string }>("capture:failed", (event) => {
        this.status = "idle";
        this.vaultId = null;
        this.startedAtMs = null;
        this.error = event.payload.message;
      });
      await listen<{ message: string }>("capture:warning", (event) => {
        this.warning = event.payload.message;
      });
      // Resync: the webview can reload while Rust keeps recording.
      try {
        const s = await invoke<CaptureStatus>("capture_status");
        if (s.recording) {
          this.status = "recording";
          this.vaultId = s.vaultId;
          this.startedAtMs = s.startedAtMs;
        }
      } catch {
        // not running under Tauri (unit tests without a status mock)
      }
    },
    async start(vaultId: string) {
      this.error = null;
      this.warning = null;
      try {
        const s = await invoke<CaptureStatus>("start_capture", { id: vaultId });
        this.status = "recording";
        this.vaultId = s.vaultId;
        this.startedAtMs = s.startedAtMs;
      } catch (e) {
        this.status = "idle";
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
  },
});
