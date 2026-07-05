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

/** A recording just started. */
export function recordingStartedMessage(): string {
  return "Listening… 🎙️";
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

/** A recording or transcription failed. */
export function failureMessage(): string {
  return "Hmm, that didn't work 😕";
}
