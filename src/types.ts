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
  /** Already emitted (since increment 3): true when the recording was
   * stopped early (e.g. disk full) rather than a normal user-initiated stop. */
  endedEarly?: boolean;
  /** Dual-purpose, backend-formed text meant to be shown verbatim: an
   * early-stop reason (endedEarly: true) or — pending a backend change — a
   * post-save issue such as a failed companion note (endedEarly: false).
   * Optional so an older/plain emitter sending neither stays valid and quiet. */
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
  /** True when a "done" job is a preserved existing transcript we chose not
   * to overwrite (capture:transcribeSkipped) rather than a freshly
   * regenerated one. Lets the buddy skip its "ready" announcement (the skip
   * already raised its own notification) without changing how a "done" job
   * renders in the Recordings/Transcriptions lists. */
  skipped?: boolean;
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

export interface TaskItem {
  path: string;
  title: string;
  status: string;
  created: string;
  done: boolean;
  due: string | null;
  priority: string | null;
  tags: string[];
  /** The task's List: parent folder relative to the tasks root, `/`-joined,
   * "" at the root. */
  list: string;
  /** Manual rank from the `order:` frontmatter number; null = unranked. */
  order: number | null;
}

/** A TaskItem enriched with its owning vault — the ONE internal shape the
 * Tasks views use for both per-vault and aggregate modes, so every row action
 * reads the row's own vaultId and needs no mode branches. */
export type AggTask = TaskItem & { vaultId: string; vaultName: string };

/** Patch for the update_task command; only present fields are written. */
export interface TaskPatch {
  title?: string;
  due?: string;
  clearDue?: boolean;
  priority?: string;
  tags?: string[];
  /** Manual rank write (drag-to-reorder). Finite; nothing un-ranks a task. */
  order?: number;
}

/** What the inline editor emits: the update_task patch plus an optional list
 * move — the container strips `list` and routes it to move_task_to_list (a
 * file move is not a frontmatter write). */
export type TaskEditorPatch = TaskPatch & { list?: string };

export interface TasksConfig {
  tasksFolder: string | null;
  /** Lists settings object: where unpicked new tasks land (null = the tasks
   * root) and the display order for list sections/pickers. */
  defaultList: string | null;
  listOrder: string[];
}

/** Per-vault imported-documents folder — get_documents_config/
 * set_documents_config, the same shape as TasksConfig/get_tasks_config. */
export interface DocumentsConfig {
  documentsFolder: string | null;
}

/** App-global Pandoc install status (detect_pandoc). configuredPath seeds
 * the path-override field; sandboxSupported gates the "too old" message
 * (Pandoc 2.15+ for --sandbox). */
export interface PandocStatus {
  installed: boolean;
  version: string | null;
  path: string | null;
  sandboxSupported: boolean;
  configuredPath: string | null;
}

export interface SearchHit {
  vaultId: string;
  vaultName: string;
  /** Display name: file stem for notes, full filename for attachments. */
  name: string;
  /** Vault-relative parent folder ("" at the vault root), for display. */
  folder: string;
  /** The obsidian://open `file` parameter (extension dropped for notes,
   * kept for attachments) — pass through to open_search_result verbatim. */
  file: string;
  /** First matching content line; null for filename-only matches. */
  snippet: string | null;
  /** Note (any-case .md) vs attachment — drives the row icon and key. */
  isNote: boolean;
}

export interface SearchResponse {
  hits: SearchHit[];
  truncated: boolean;
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
