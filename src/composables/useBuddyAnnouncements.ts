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
  watch(
    () => capture.transcribing,
    (transcribing) => {
      if (transcribing) announce(transcribingMessage());
    },
  );
  watch(
    () => capture.lastTranscribed,
    (done, prev) => {
      if (done && done !== prev) announce(transcribedMessage());
    },
  );
  // A recording failure sets `error`; a transcription failure sets
  // `transcriptError`. They are distinct fields, so a single failure announces
  // once (never both).
  watch(
    () => capture.error,
    (err, prev) => {
      if (err && err !== prev) announce(failureMessage());
    },
  );
  watch(
    () => capture.transcriptError,
    (err, prev) => {
      if (err && err !== prev) announce(failureMessage());
    },
  );
}
