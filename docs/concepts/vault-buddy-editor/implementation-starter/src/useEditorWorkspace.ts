import { defineStore } from 'pinia';
/** View state never participates in render revisioning or project-edit Undo. */
export const useEditorWorkspace = defineStore('editorWorkspace', {
  state: () => ({ playheadMs: 0, selectedClipIds: [] as string[], timelineZoom: 1,
    propertyTab: 'clip' as 'clip'|'layout'|'fades'|'audio'|'speed'|'color',
    libraryTab: 'media' as 'media'|'titles'|'captions'|'chapters',
    monitorMuted: false, snap: true, propertiesVisible: true, libraryVisible: true }),
  actions: {
    seek(value: number, durationMs: number) {
      if (!Number.isFinite(value) || !Number.isSafeInteger(durationMs) || durationMs < 0) throw new Error('Invalid playhead');
      this.playheadMs = Math.round(Math.min(Math.max(0,value),durationMs));
    },
    select(ids: readonly string[]) { this.selectedClipIds = [...new Set(ids)]; },
  },
});
