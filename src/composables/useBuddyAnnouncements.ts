import { watch } from "vue";
import { useCaptureStore } from "../stores/capture";
import { announce } from "../announce";
import {
  recordingStartedMessage,
  recordingPausedMessage,
  recordingResumedMessage,
  recordingSavedMessage,
  transcribingMessage,
  transcribedMessage,
  failureMessage,
} from "../buddyMessages";

/**
 * Wires capture-store transitions to the buddy's spoken acknowledgements.
 *
 * INVARIANT: this lives ONLY in the buddy window (always alive), so each
 * capture event is announced exactly once. The panel window also calls
 * `capture.init()` and sees the same events — wiring announcements there too
 * would double every bubble.
 *
 * Watchers are non-immediate on purpose: on launch the store may already hold a
 * state (e.g. a recording in progress after a reveal) that must not re-announce.
 */
export function useBuddyAnnouncements(): void {
  const capture = useCaptureStore();

  watch(
    () => capture.status,
    (status, prev) => {
      if (status === "recording" && prev !== "recording") {
        announce(recordingStartedMessage());
      }
    },
  );
  watch(
    () => capture.lastSavedFile,
    (file, prev) => {
      if (file && file !== prev) announce(recordingSavedMessage());
    },
  );
  // Pause/resume both flip `capture.paused`, but stopping a paused recording
  // also clears it (resetRecordingState) — announce "resumed" only while the
  // recording is genuinely still going, or a stop-while-paused would speak a
  // spurious resume right before "saved".
  watch(
    () => capture.paused,
    (paused, prev) => {
      if (paused && !prev) {
        announce(recordingPausedMessage());
      } else if (!paused && prev && capture.status === "recording") {
        announce(recordingResumedMessage());
      }
    },
  );
  // `activeTranscription` is non-null while a job occupies the worker (the
  // old singular `transcribing` flag, now derived from the per-job map).
  watch(
    () => capture.activeTranscription !== null,
    (active) => {
      if (active) announce(transcribingMessage());
    },
  );
  // A recording failure sets `error`. A transcription's outcome is now a
  // phase on its job rather than a separate singular field — watch the
  // newest finished job's (mp3, phase) as one primitive so an unrelated
  // re-render can't retrigger it, and branch on phase so success/failure
  // still announce exactly once each (never both for the same job).
  watch(
    () => {
      const job = capture.finishedTranscriptions[0];
      return job ? `${job.mp3}:${job.phase}` : null;
    },
    (curr, prev) => {
      if (!curr || curr === prev) return;
      const job = capture.finishedTranscriptions[0];
      if (job?.phase === "done") {
        // A skipped job (capture:transcribeSkipped) is "done" in the sense
        // that a complete transcript exists, but nothing was regenerated —
        // the skip already raised its own "kept your existing transcript…"
        // notification, so the cheery "ready" line would be redundant.
        if (!job.skipped) announce(transcribedMessage());
      } else if (job?.phase === "failed") {
        announce(failureMessage(job.error ?? undefined));
      }
    },
  );
  watch(
    () => capture.error,
    (err, prev) => {
      if (err && err !== prev) announce(failureMessage(err));
    },
  );
}
