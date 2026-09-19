import { defineStore } from 'pinia';
import type { EditorCommand, EditorPort, EditorSnapshot } from './contracts';
import { decodeSaveReceipt, decodeSnapshot, ProtocolError } from './contracts';
import { tauriEditorPort } from './tauriEditorPort';

/** Factory permits mockIPC/test injection without serializing native handles into Pinia. */
export function createEditorProjectStore(port: EditorPort, id = 'editorProject') {
  return defineStore(id, {
    state: () => ({ snapshot: null as EditorSnapshot | null, busy: false, saving: false,
      error: null as string | null, generation: 0 }),
    getters: {
      dirty: state => !!state.snapshot && state.snapshot.persistedRevision !== state.snapshot.revision,
    },
    actions: {
      async openStaged(stagedBase: string): Promise<void> {
        if (this.busy || this.saving) throw new Error('Wait for the active editor operation');
        const generation = ++this.generation;
        this.busy = true; this.error = null;
        try {
          const snapshot = decodeSnapshot(await port.openStaged(stagedBase));
          if (generation === this.generation) this.snapshot = snapshot;
        } catch (error) {
          if (generation === this.generation) this.error = error instanceof Error ? error.message : 'Could not open staged capture';
          throw error;
        } finally { if (generation === this.generation) this.busy = false; }
      },
      async execute(command: EditorCommand): Promise<void> {
        const current = this.snapshot;
        if (!current || this.busy || this.saving) throw new Error('The editor is not ready for an edit');
        if (command.kind === 'rename' && (!command.title.trim() || command.title.trim().length > 160)) throw new Error('Use a title between 1 and 160 characters');
        const generation = this.generation;
        this.busy = true; this.error = null;
        try {
          const next = decodeSnapshot(await port.execute({ sessionId: current.sessionId,
            expectedRevision: current.revision, commandId: crypto.randomUUID(), command }));
          if (generation !== this.generation) return;
          if (next.sessionId !== current.sessionId || next.projectId !== current.projectId || next.revision < current.revision) throw new ProtocolError('Stale or unrelated edit response');
          this.snapshot = next;
        } catch (error) {
          if (generation === this.generation) this.error = error instanceof Error ? error.message : 'Could not apply the edit';
          throw error;
        } finally { if (generation === this.generation) this.busy = false; }
      },
      async save(): Promise<void> {
        const current = this.snapshot;
        if (!current || this.busy || this.saving) throw new Error('The editor is not ready to save');
        const generation = this.generation;
        this.saving = true; this.error = null;
        try {
          const receipt = decodeSaveReceipt(await port.save(current.sessionId, current.revision));
          if (generation !== this.generation) return;
          if (receipt.sessionId !== current.sessionId || receipt.savedRevision !== current.revision) throw new ProtocolError('Save receipt does not match the requested revision');
          this.snapshot = { ...current, persistedRevision: receipt.savedRevision };
        } catch (error) {
          if (generation === this.generation) this.error = error instanceof Error ? error.message : 'Project was not saved';
          throw error;
        } finally { if (generation === this.generation) this.saving = false; }
      },
      /** Local view detach only: does not cancel a job or delete a backend session. */
      detach(): void { this.generation++; this.snapshot = null; this.busy = false; this.saving = false; this.error = null; },
    },
  });
}
export const useEditorProject = createEditorProjectStore(tauriEditorPort);
