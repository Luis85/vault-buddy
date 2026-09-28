/**
 * The webcam take wire types (Task 49; Contract reference `TakeDto`), split
 * out of `editorTypes.ts` for its 500-line cap the way
 * `editorRenderTypes.ts` is, and re-exported from there so `editorTypes.ts`
 * stays the one import site every caller uses. IPC envelopes, so camelCase
 * throughout (`webcam_commands.rs`' DTOs).
 */

/** `editor_webcam_begin`'s answer. */
export interface TakeStarted {
  takeId: string;
}

/** Contract reference `TakeDto` — `editor_webcam_finish`'s answer: the
 * take, registered as its own asset (`assetId`), with its probed facts. */
export interface TakeDto {
  takeId: string;
  assetId: string;
  durationMs: number;
  width: number;
  height: number;
  hasAudio: boolean;
}
