/** Proposed native editor protocol. These editor_* commands are not yet in the repository. */
export interface EditorSnapshot {
  sessionId: string;
  projectId: string;
  revision: number;
  persistedRevision: number | null;
  title: string;
  durationMs: number;
  canUndo: boolean;
  canRedo: boolean;
}
export type EditorCommand =
  | { kind: 'rename'; title: string }
  | { kind: 'splitClip'; clipId: string; atMs: number }
  | { kind: 'deleteSelection'; clipIds: string[]; closeGap: boolean }
  | { kind: 'undo' }
  | { kind: 'redo' };
export interface ExecuteRequest {
  sessionId: string;
  expectedRevision: number;
  commandId: string;
  command: EditorCommand;
}
export interface SaveReceipt { sessionId: string; savedRevision: number; projectFileId: string }
export interface RenderRequest {
  sessionId: string;
  expectedRevision: number;
  range: { startMs: number; endMs: number } | null;
  presetId: string;
}
export interface RenderProgress {
  sessionId: string;
  jobId: string;
  sequence: number;
  phase: 'queued' | 'preparing' | 'rendering' | 'publishing' | 'complete' | 'cancelled' | 'failed';
  fraction: number;
}
export interface EditorPort {
  openStaged(stagedBase: string): Promise<EditorSnapshot>;
  execute(request: ExecuteRequest): Promise<EditorSnapshot>;
  save(sessionId: string, expectedRevision: number): Promise<SaveReceipt>;
}
export class ProtocolError extends Error {
  constructor(message: string) { super(message); this.name = 'ProtocolError'; }
}
function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) throw new ProtocolError('Invalid response object');
  return value as Record<string, unknown>;
}
function string(value: unknown, field: string, limit = 160): string {
  if (typeof value !== 'string' || !value.trim() || value.length > limit) throw new ProtocolError(`Invalid ${field}`);
  return value;
}
function integer(value: unknown, field: string): number {
  if (typeof value !== 'number' || !Number.isSafeInteger(value) || value < 0) throw new ProtocolError(`Invalid ${field}`);
  return value;
}
function bool(value: unknown, field: string): boolean {
  if (typeof value !== 'boolean') throw new ProtocolError(`Invalid ${field}`);
  return value;
}
export function decodeSnapshot(value: unknown): EditorSnapshot {
  const v = object(value);
  const revision = integer(v.revision, 'revision');
  const persistedRevision = v.persistedRevision === null ? null : integer(v.persistedRevision, 'persistedRevision');
  if (persistedRevision !== null && persistedRevision > revision) throw new ProtocolError('Saved revision exceeds current revision');
  const durationMs = integer(v.durationMs, 'durationMs');
  if (durationMs > 7_200_000) throw new ProtocolError('Duration exceeds the two-hour reference safety bound');
  return { sessionId: string(v.sessionId,'sessionId',100), projectId: string(v.projectId,'projectId',100), revision,
    persistedRevision, title: string(v.title,'title'), durationMs,
    canUndo: bool(v.canUndo,'canUndo'), canRedo: bool(v.canRedo,'canRedo') };
}
export function decodeSaveReceipt(value: unknown): SaveReceipt {
  const v = object(value);
  return { sessionId: string(v.sessionId,'sessionId',100), savedRevision: integer(v.savedRevision,'savedRevision'), projectFileId: string(v.projectFileId,'projectFileId',200) };
}
export function decodeProgress(value: unknown): RenderProgress {
  const v = object(value);
  const phases = ['queued','preparing','rendering','publishing','complete','cancelled','failed'] as const;
  if (typeof v.phase !== 'string' || !phases.includes(v.phase as typeof phases[number])) throw new ProtocolError('Invalid render phase');
  if (typeof v.fraction !== 'number' || !Number.isFinite(v.fraction) || v.fraction < 0 || v.fraction > 1) throw new ProtocolError('Invalid render progress');
  return { sessionId: string(v.sessionId,'sessionId',100), jobId: string(v.jobId,'jobId',100), sequence: integer(v.sequence,'sequence'), phase: v.phase as RenderProgress['phase'], fraction: v.fraction };
}
