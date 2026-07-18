// The buddy's voice: short, playful, first-person acknowledgements shown in
// its speech bubble. Pure strings — no Vue, no IPC — so the copy is testable
// everywhere and lives in one place. The bubble's max-width wraps long lines
// (e.g. a long vault name), so copy stays uncapped here.

/** Opening a vault — names it, with a generic fallback for a blank name. */
export function vaultOpenedMessage(vaultName: string): string {
  const name = vaultName.trim();
  return name ? `Opening ${name} ✨` : "Opening your vault ✨";
}

/** Opening today's daily note. */
export function dailyNoteOpenedMessage(): string {
  return "Here's today's note 📅";
}

/** Opening a note/file from search — names it, generic fallback for blank. */
export function noteOpenedMessage(name: string): string {
  const trimmed = name.trim();
  return trimmed ? `Opening ${trimmed} 📄` : "Opening your note 📄";
}

/** A recording just started. */
export function recordingStartedMessage(): string {
  return "Listening… 🎙️";
}

/** The recording was paused. */
export function recordingPausedMessage(): string {
  return "Taking a breather ⏸️";
}

/** The recording resumed after a pause. */
export function recordingResumedMessage(): string {
  return "Back to it! ▶️";
}

/** A recording finished and was saved to the vault. */
export function recordingSavedMessage(): string {
  return "Got it — saved! 🎧";
}

/** Background transcription started on a finished recording. */
export function transcribingMessage(): string {
  return "Writing it down… ✍️";
}

/** Transcription finished and the transcript is ready. */
export function transcribedMessage(): string {
  return "Transcript ready! ✨";
}

/**
 * The startup check found an update — the buddy asks; the bubble is clickable
 * and clicking it opens the dedicated update view where Install & restart is
 * the answer. Generic fallback so a blank version never renders a dangling
 * "Update v is ready".
 */
export function updateAvailableMessage(version: string): string {
  const v = version.trim();
  return v
    ? `Update v${v} is ready — click me! ⬆️`
    : "An update is ready — click me! ⬆️";
}

/** Cuts `s` to at most `n` characters, appending an ellipsis when it does. */
function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n)}…` : s;
}

/**
 * A recording or transcription failed. With a `reason` (the backend's error
 * message), the buddy speaks it — truncated so a long/unbounded backend
 * message can't blow out the speech bubble — instead of the generic line.
 */
export function failureMessage(reason?: string): string {
  return reason ? `Hmm — ${truncate(reason, 60)} 😕` : "Hmm, that didn't work 😕";
}

/** What an AI client just did in a vault, spoken by the buddy. */
export function mcpWriteMessage(payload: {
  kind: string;
  title: string;
  vaultName: string;
}): string {
  const { kind, vaultName } = payload;
  // The title is CLIENT-provided (an AI can send an unbounded one) —
  // truncate like failureMessage does, so it can't blow out the bubble.
  // vaultName is the user's own folder name and stays uncapped like the
  // rest of the file's copy.
  const title = truncate(payload.title, 60);
  if (kind === "addTask") return `Added task "${title}" to ${vaultName}`;
  if (kind === "setTaskStatus") return `Updated task "${title}" in ${vaultName}`;
  if (kind === "createDailyNote") return `Created today's note in ${vaultName}`;
  return `An AI client updated ${vaultName}`;
}
