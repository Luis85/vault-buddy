export interface Vault {
  id: string;
  name: string;
  path: string;
  /** Currently open in Obsidian (from obsidian.json's `open` flag). */
  open: boolean;
}

export interface CaptureStatus {
  recording: boolean;
  vaultId: string | null;
  startedAtMs: number | null;
  paused: boolean;
  pausedTotalMs: number;
  pausedSinceMs: number | null;
}

export interface CaptureRenamed {
  mp3: string;
  note: string | null;
  warning: string | null;
}

export interface CaptureSaved {
  mp3: string;
  note: string | null;
  /** Optional: Task 12's backend adds these for an early-stopped save; an
   * older/plain emitter sending neither must stay valid and quiet. */
  endedEarly?: boolean;
  warning?: string | null;
}

export interface CaptureTranscribed {
  mp3: string;
  transcript: string;
}

export interface CaptureTranscribeFailed {
  mp3: string;
  message: string;
}

/** A Complete/hand-edited transcript we refused to overwrite — a warning,
 * not a failure (the sidecar the user already has is preserved). */
export interface CaptureTranscribeSkipped {
  mp3: string;
  message: string;
}

export interface ModelDownload {
  mp3: string;
  model: string;
  received: number;
  total: number | null;
}

export type Phase = "queued" | "downloading" | "preparing" | "transcribing" | "done" | "failed" | "cancelled";

export interface TranscriptionJob {
  mp3: string;
  vaultId: string;
  name: string;
  phase: Phase;
  progress: number | null;
  model: string | null;
  error: string | null;
  startedAtMs: number | null;
}

export interface TranscriptionQueueStatus {
  active: { mp3: string; vaultId: string; phase: "downloading" | "preparing" | "transcribing"; progress: number; received: number | null; total: number | null; startedAtMs: number } | null;
  queued: { mp3: string; vaultId: string }[];
  waitingForRecording: boolean;
}

export interface TranscribeProgress {
  mp3: string;
  progress: number;
}

export interface ModelReady {
  mp3: string;
}

export interface TranscribeCancelled {
  mp3: string;
}

export interface CaptureConfig {
  mode: "meeting" | "voice-note";
  recordingFolder: string | null;
  bitrateKbps: number;
  createNote: boolean;
  inputDevice: string | null;
  outputDevice: string | null;
  transcribe: boolean;
  transcriptionModel: string;
  transcriptionLanguage: string | null;
  transcriptTimestamps: boolean;
  followUpTemplate: boolean;
}

export interface AudioDevice {
  name: string;
  isDefault: boolean;
}

export interface AudioDevices {
  inputs: AudioDevice[];
  outputs: AudioDevice[];
}

export interface Recording {
  mp3: string;
  title: string;
  recordedAt: string;
  /** From the companion note's frontmatter; null when there's no note. */
  duration: string | null;
  /** Recording type from the companion note; null → "Ungrouped". */
  type: string | null;
  /** Sidecar state — drives the row indicator + re-transcribe confirm. */
  transcriptStatus: "none" | "pending" | "failed" | "complete" | "cancelled";
}
