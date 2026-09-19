import { invoke, Channel } from '@tauri-apps/api/core';
import type { EditorPort, RenderProgress, RenderRequest } from './contracts';
import { decodeProgress, decodeSaveReceipt, decodeSnapshot, ProtocolError } from './contracts';

/** Keep all proposed command names in this adapter, not in Vue components. */
export const tauriEditorPort: EditorPort = {
  async openStaged(stagedBase) {
    return decodeSnapshot(await invoke<unknown>('editor_open_staged', { request: { stagedBase } }));
  },
  async execute(request) {
    return decodeSnapshot(await invoke<unknown>('editor_execute', { request }));
  },
  async save(sessionId, expectedRevision) {
    return decodeSaveReceipt(await invoke<unknown>('editor_save_project', { request: { sessionId, expectedRevision } }));
  },
};

/** Progress is small metadata, never raw media frames. The job remains queryable after a view reload. */
export async function beginRender(request: RenderRequest, onProgress: (progress: RenderProgress) => void,
  onProtocolError: (error: Error) => void): Promise<{ jobId: string; dispose: () => void }> {
  let disposed = false;
  let sequence = -1;
  let terminal = false;
  let observedJob: string | undefined;
  const progress = new Channel<unknown>();
  progress.onmessage = value => {
    if (disposed || terminal) return;
    try {
      const event = decodeProgress(value);
      if (event.sessionId !== request.sessionId) throw new ProtocolError('Unexpected render session');
      if (observedJob && observedJob !== event.jobId) throw new ProtocolError('Unexpected render job');
      observedJob = event.jobId;
      if (event.sequence <= sequence) return;
      sequence = event.sequence;
      terminal = ['complete', 'cancelled', 'failed'].includes(event.phase);
      onProgress(event);
    } catch (error) { onProtocolError(error instanceof Error ? error : new Error('Invalid render event')); }
  };
  try {
    const response = await invoke<unknown>('editor_start_render', { request, progress });
    if (!response || typeof response !== 'object' || !('jobId' in response) || typeof response.jobId !== 'string' || !response.jobId) throw new ProtocolError('Missing render job identifier');
    if (observedJob && observedJob !== response.jobId) throw new ProtocolError('Render response does not match progress');
    observedJob = response.jobId; // Validate events that arrive after the command reply too.
    return { jobId: response.jobId, dispose: () => { disposed = true; progress.onmessage = () => {}; } };
  } catch (error) { disposed = true; progress.onmessage = () => {}; throw error; }
}
