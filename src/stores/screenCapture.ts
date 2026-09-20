import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { defineStore } from "pinia";

import { logWarning } from "../logging";
import type { ExportResult, ScreenCaptureStatus, StagedCapture } from "../types";
import { useNotificationsStore } from "./notifications";

/** The view's three-valued capture state. Rust reports `capturing` and
 * `paused` as two independent booleans (`ScreenStatusPayload`); this is the
 * one place that mapping happens, so no component has to re-derive it and
 * get the paused arm wrong. */
type ScreenStatus = "idle" | "capturing" | "paused";

function statusFrom(s: ScreenCaptureStatus): ScreenStatus {
  if (!s.capturing) return "idle";
  return s.paused ? "paused" : "capturing";
}

export const useScreenCaptureStore = defineStore("screenCapture", {
  state: () => ({
    status: "idle" as ScreenStatus,
    /** Which vault the capture will be filed into — drives the vault-row
     * indicator, exactly like the audio store's own `vaultId`. */
    vaultId: null as string | null,
    sourceTitle: null as string | null,
    startedAtMs: null as number | null,
    /** Accumulated pause time, authoritative from Rust's `screen:resumed`. */
    pausedTotalMs: 0,
    /** Start of the current pause span; null while not paused. */
    pausedSinceMs: null as number | null,
    /** Advisory frame stats (~2 Hz, lossy by design — spec 11). */
    fps: 0,
    dropped: 0,
    /** The last finished capture's staged `.mp4`, for the editor (phase 3). */
    lastStaged: null as StagedCapture | null,
    error: null as string | null,
    warning: null as string | null,
    /** The surviving `.part` a failed finalize kept (`ScreenError::Retained`).
     * Typed data, not prose to parse back out of `error` — the fMP4 design
     * exists precisely so that file is still playable. */
    retainedPath: null as string | null,
    /**
     * A stop has been asked for and the capture has not ended yet. Finalize
     * is unbounded — `stop_screen_capture` can answer `stillSaving` while the
     * session is still tearing down — so this cannot clear on the command's
     * reply; it clears when the capture is actually gone (every route to
     * idle) or when the stop was rejected (see `stop()`'s catch).
     *
     * Documented residual, deliberately not defended against: `resync()` /
     * `applyStatus` re-derive every other live-capture field from Rust, but
     * not this one — a status payload cannot tell a capture whose stop is in
     * flight from one whose is not, which is exactly why the flag is local.
     * So a terminal event that never arrived would latch it for that
     * capture's life, Pause and Stop both disabled until the webview
     * reloads (the tray still stops the capture, so nothing is strandable).
     * No path reaches that: the `screen-capture-monitor` thread is the sole
     * `done_rx` consumer, covers the explicit stop AND self-finalization,
     * and BOTH of its match arms emit — stopped or failed. Clearing the flag
     * speculatively on a `capturing` status would instead re-arm Stop
     * mid-finalize, the very thing it exists to prevent.
     */
    stopping: false,
    /**
     * Bumped by every transition this store applies locally. A `resync()`
     * captures it before awaiting and discards its (by then stale) answer if
     * it changed — otherwise a status read issued just before a
     * `screen:stopped` lands could resurrect the capture it was told ended.
     */
    seq: 0,
  }),
  getters: {
    /** Mirror of Rust's `paused` boolean, derived rather than stored: two
     * copies of one fact are two chances to desync. */
    paused(state): boolean {
      return state.status === "paused";
    },
  },
  actions: {
    /**
     * Real elapsed footage at `now`: wall time since the start, minus the
     * banked pause time, minus the pause currently open. Without the second
     * subtraction a two-minute pause reads as two minutes of recording that
     * is not in the file. The open-pause term is gated on actually being
     * paused — applying a stale `pausedSinceMs` while capturing would freeze
     * the clock after the first resume.
     */
    elapsedMs(now: number): number {
      if (this.startedAtMs === null) return 0;
      const open =
        this.status === "paused" && this.pausedSinceMs !== null
          ? now - this.pausedSinceMs
          : 0;
      return Math.max(0, now - this.startedAtMs - this.pausedTotalMs - open);
    },
    /** Apply a `ScreenStatusPayload` wholesale — the authoritative shape. */
    applyStatus(s: ScreenCaptureStatus) {
      this.seq++;
      this.status = statusFrom(s);
      // Frame stats belong to a capture; a status that reports none leaves
      // them describing nothing. `reset()` clears them, but a capture can
      // also end through this path (the refused-start reconcile and the
      // pause/resume/stop failure resyncs all land here), and the counters
      // surviving that would open the NEXT capture's bar already reporting
      // the previous capture's dropped frames.
      if (this.status === "idle") {
        this.fps = 0;
        this.dropped = 0;
        this.stopping = false;
      }
      this.vaultId = s.vaultId;
      this.sourceTitle = s.sourceTitle;
      this.startedAtMs = s.startedAtMs;
      this.pausedTotalMs = s.pausedTotalMs ?? 0;
      this.pausedSinceMs = s.pausedSinceMs ?? null;
    },
    /** Back to "nothing is running" — every live-capture field cleared, so no
     * phantom bar can count up from a capture that already ended. */
    reset() {
      this.seq++;
      this.status = "idle";
      this.vaultId = null;
      this.sourceTitle = null;
      this.startedAtMs = null;
      this.pausedTotalMs = 0;
      this.pausedSinceMs = null;
      this.fps = 0;
      this.dropped = 0;
      this.stopping = false;
    },
    /**
     * The staged capture `base` is no longer in the staging directory — it
     * was exported into a vault, or discarded.
     *
     * The SECOND clear site for `lastStaged`, and safe to add beside the
     * first because it is keyed on IDENTITY rather than on lifecycle: it
     * fires only for the capture that genuinely no longer exists. The events
     * behind it are app-wide, so an unkeyed clear would throw away the only
     * handle anything holds on a DIFFERENT capture's footage.
     *
     * It deliberately does NOT bump `seq`. An export finishes long after its
     * capture ended and is not a capture-lifecycle transition, so bumping
     * would make an in-flight `start()` or `resync()` discard a reply that
     * is still true.
     *
     * Without it the panel's capture bar keeps offering Edit on a base that
     * is gone, and `open_capture_editor` -> `load_staged_capture` answers
     * with a banner the user cannot act on. Rust emits `screen:discarded`
     * for exactly this (`export_commands::emit_discarded`).
     */
    forgetStaged(base: string) {
      if (this.lastStaged?.base === base) {
        this.lastStaged = null;
      }
    },
    applyStopped(staged: StagedCapture) {
      this.reset();
      this.lastStaged = staged;
      this.error = null;
      this.warning = null;
    },
    /**
     * Re-read the authoritative status. Used at init (a reloaded webview must
     * not render blank over a live capture) and as the `screen:started`
     * handler, because that event can arrive AFTER `screen:stopped` for the
     * same capture — the monitor thread is live before `start_screen_capture`
     * emits, so a source closing in that window finishes first
     * (screen_commands.rs names this and points here). Deriving state from
     * arrival order would leave a capture bar over no session.
     */
    async resync() {
      const seq = this.seq;
      try {
        const s = await invoke<ScreenCaptureStatus>("screen_capture_status");
        // A locally-applied transition landed while this was in flight: the
        // answer describes a moment that has already passed.
        if (seq !== this.seq) return;
        if (s) this.applyStatus(s);
      } catch (e) {
        // Best-effort resync only (the panel may not be under Tauri in
        // tests); never throw out of an event handler or onMounted.
        logWarning(`screen_capture_status failed: ${String(e)}`);
      }
    },
    async init() {
      await listen("screen:started", () => void this.resync());
      await listen<{ atMs: number }>("screen:paused", (event) => {
        // Only a live capture can be paused. `status` is one tri-state here
        // rather than Rust's two booleans, so — unlike the audio store, where
        // a stale `capture:paused` can only set a flag — an ungated pause
        // would raise a paused bar out of idle with no startedAtMs behind it.
        if (this.status === "idle") return;
        this.seq++;
        this.status = "paused";
        this.pausedSinceMs = event.payload.atMs;
      });
      await listen<{ pausedTotalMs: number }>("screen:resumed", (event) => {
        // Same reasoning as the pause handler: a resume that outlived its
        // capture would report a capture running.
        if (this.status === "idle") return;
        this.seq++;
        this.status = "capturing";
        this.pausedTotalMs = event.payload.pausedTotalMs ?? 0;
        this.pausedSinceMs = null;
      });
      await listen<StagedCapture>("screen:stopped", (event) => {
        this.applyStopped(event.payload);
      });
      await listen<{ message: string; retainedPath: string | null }>(
        "screen:failed",
        (event) => {
          this.reset();
          this.error = event.payload.message;
          this.retainedPath = event.payload.retainedPath ?? null;
          useNotificationsStore().error(event.payload.message);
        },
      );
      await listen<{ message: string }>("screen:warning", (event) => {
        // Spec 14: a vanished source or device warns and the capture
        // finalizes cleanly — never a teardown. The capture bar shows this
        // inline while capturing, so only a warning outside a live capture
        // needs a toast (the audio domain's own posture).
        this.warning = event.payload.message;
        if (this.status === "idle") {
          useNotificationsStore().warning(event.payload.message);
        }
      });
      await listen<ExportResult>("screen:exported", (event) => {
        this.forgetStaged(event.payload.base);
      });
      await listen<{ base: string }>("screen:discarded", (event) => {
        this.forgetStaged(event.payload.base);
      });
      await listen<{ fps: number; dropped: number }>("screen:frames", (event) => {
        this.fps = event.payload.fps;
        this.dropped = event.payload.dropped;
      });
      // Seed from backend truth LAST, so a live capture is reflected in a
      // webview that was reloaded (or only just mounted) mid-capture.
      await this.resync();
    },
    /**
     * Start a capture. A refusal (spec 14's typed errors — alreadyCapturing,
     * sourceGone, encoderUnavailable) must leave the store agreeing with
     * Rust: believing a capture is running when none is renders the bar over
     * nothing and gives Stop no session to stop. It reconciles by re-reading
     * the authoritative status rather than by resetting, because the refusal
     * this codepath sees MOST is `alreadyCapturing` — and blanking the store
     * there would erase the bar of the very capture that caused the refusal.
     * The error is rethrown so the picker can render it inline and refresh
     * its source list.
     */
    async start(
      vaultId: string,
      sourceId: string,
      inputs: string[],
      outputs: string[],
    ) {
      this.error = null;
      this.warning = null;
      // A retained path belongs to the capture that produced it; carrying it
      // into the next one would offer a stale file as this capture's own.
      this.retainedPath = null;
      const seq = this.seq;
      try {
        const s = await invoke<ScreenCaptureStatus>("start_screen_capture", {
          id: vaultId,
          sourceId,
          inputs,
          outputs,
        });
        // The third route into the started-after-stopped race, and the only
        // one that writes state without consulting the generation: the
        // monitor thread is live before this command's tail returns, so a
        // source closing in that window emits `screen:stopped` FIRST and this
        // reply is already stale. Applying it would raise a bar over a
        // finished capture — and `lastStaged = null` below would discard the
        // staged .mp4 that `screen:stopped` had just delivered, losing the
        // only handle anything has on the footage.
        if (seq !== this.seq) return;
        this.applyStatus(s);
        this.lastStaged = null;
      } catch (e) {
        this.error = String(e);
        await this.resync();
        throw e;
      }
    },
    async pause() {
      try {
        await invoke("pause_screen_capture");
      } catch (e) {
        logWarning(`pause_screen_capture failed: ${String(e)}`);
        useNotificationsStore().error(String(e));
        await this.resync();
      }
    },
    async resume() {
      try {
        await invoke("resume_screen_capture");
      } catch (e) {
        logWarning(`resume_screen_capture failed: ${String(e)}`);
        useNotificationsStore().error(String(e));
        await this.resync();
      }
    },
    /** Stop and let `screen:stopped` / `screen:failed` finish the story — a
     * `stillSaving` reply means the bounded wait expired while finalize was
     * still running, NOT that anything failed. */
    async stop() {
      this.stopping = true;
      try {
        await invoke<{ stillSaving: boolean }>("stop_screen_capture");
      } catch (e) {
        logWarning(`stop_screen_capture failed: ${String(e)}`);
        useNotificationsStore().error(String(e));
        // Re-arm Stop. The rejection this sees most — `is_capturing` saying
        // no (screen_commands.rs) — really does mean nothing is finalizing,
        // but the command's `JoinError` arm ("Stop failed — see the logs for
        // details.") can reject AFTER `Control::Stop` was already sent, so
        // what this catch actually knows is "the stop did not report
        // success", not "nothing is finalizing". Re-arming there costs at
        // worst a duplicate Stop — a fire-and-forget send on a channel the
        // session is already draining — while latching the flag would
        // dead-end both controls on a rejection that changed nothing.
        // Cleared BEFORE the resync, which may itself land on idle and clear
        // it anyway, and must not be able to re-set it.
        this.stopping = false;
        await this.resync();
      }
    },
  },
});
