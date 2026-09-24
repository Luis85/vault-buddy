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
