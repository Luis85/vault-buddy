/**
 * The render and product wire types (Task 46; Contract reference
 * `RenderRequest` / `ProductDto`), split out of `editorTypes.ts` for its
 * 500-line cap the way `editorCommandTypes.ts` is, and re-exported from
 * there so `editorTypes.ts` stays the one import site every caller uses.
 * IPC envelopes, so camelCase throughout (`render_jobs.rs`' DTOs).
 */

/** A render's encoder quality (`ScreenQuality`'s keys). */
export type RenderQuality = "low" | "balanced" | "high";

/** An output-time range, `[startMs, endMs)`. */
export interface RenderRange {
  startMs: number;
  endMs: number;
}

/** Contract reference `RenderRequest` — `editor_start_render`'s argument.
 * `range: null` renders the whole project. */
export interface RenderRequest {
  sessionId: string;
  expectedRevision: number;
  name: string;
  range: RenderRange | null;
  quality: RenderQuality;
  /** Task 47 (F18): a disposable Review render -- kept in the project's
   * cache, never a product. Omitted for a product render. */
  review?: boolean;
}

/** `editor_start_render`'s immediate reply: the job, and the revision it
 * froze. Progress travels on the job's Channel (`JobProgressDto`). */
export interface RenderStarted {
  jobId: string;
  revision: number;
}

/** Contract reference `ProductDto`: one entry of the product ledger, plus
 * whether its file is on disk. `renderRange` is null for a whole render. */
export interface ProductDto {
  id: string;
  projectId: string;
  name: string;
  filename: string;
  mime: string;
  revision: number;
  durationMs: number;
  createdAt: string;
  editFingerprint: string;
  renderRange: RenderRange | null;
  available: boolean;
}

/** Contract reference `PublishDestination` (Task 48) —
 * `editor_publish_product`'s destination. `folder` blank means the vault's
 * screen-capture folder; `vaultId` is Obsidian's vault ID, never a name. */
export interface PublishDestination {
  vaultId: string;
  folder: string;
  dated: boolean;
  createNote: boolean;
}

/** Contract reference `PublishReceipt`: where the video (and its note)
 * landed — absolute paths, for `open_screen_capture` only. `warning` is set
 * when the video landed and its note did not. */
export interface PublishReceipt {
  videoPath: string;
  notePath: string | null;
  vaultId: string;
  vaultName: string;
  warning: string | null;
}

/** `editor_export_subtitles`' `format`. */
export type SubtitleFormat = "srt" | "vtt";

/** One Obsidian vault the Publish dialog can offer (`list_vaults`' id and
 * display name; the path never reaches the webview's UI). */
/** A vault's Screen settings as the Publish dialog's DEFAULTS (Task 59 fix
 * round 1, GAP-211): *Date folders* → `dated`, *Write a companion note* →
 * `createNote`, read from `get_screen_capture_config`'s reply. */
export interface PublishDefaults {
  dated: boolean;
  createNote: boolean;
}

export interface VaultChoice {
  id: string;
  name: string;
}
