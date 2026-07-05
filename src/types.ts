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
}

export interface CaptureSaved {
  mp3: string;
  note: string | null;
  endedEarly: boolean;
}

export interface CaptureTranscribed {
  mp3: string;
  transcript: string;
}

export interface CaptureTranscribeFailed {
  mp3: string;
  message: string;
}

export interface ModelDownload {
  model: string;
  received: number;
  total: number | null;
}
