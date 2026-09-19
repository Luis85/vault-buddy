import { defineStore } from 'pinia';
export interface GuideProgress { schema: 'vault-buddy-guide-progress/1'; stepId: string; reviewed: string[]; status: 'new'|'paused'|'completed'; dim: boolean }
/** Small store skeleton. The complete step content and interaction rules are in contracts/onboarding.steps.json. */
export function createGuideStore(stepIds: readonly string[], id = 'editorOnboarding') {
  if (!stepIds.length || new Set(stepIds).size !== stepIds.length) throw new Error('Guide steps need unique stable IDs');
  const first = stepIds[0]!;
  return defineStore(id, {
    state: () => ({ progress: { schema: 'vault-buddy-guide-progress/1', stepId: first, reviewed: [], status: 'new', dim: true } as GuideProgress,
      active: false, minimized: false, suspended: false }),
    actions: {
      start(stepId = this.progress.stepId) {
        if (!stepIds.includes(stepId)) throw new Error('Unknown guide step');
        this.progress.stepId = stepId; this.progress.status = 'paused'; this.active = true; this.minimized = false;
      },
      pause() { this.active = false; this.minimized = false; },
      next() {
        if (!this.active || this.suspended) return;
        const index = stepIds.indexOf(this.progress.stepId);
        this.progress.reviewed = [...new Set([...this.progress.reviewed,this.progress.stepId])];
        const next = stepIds[index + 1];
        if (next) this.progress.stepId = next;
        else { this.progress.status = 'completed'; this.active = false; }
      },
      restore(value: GuideProgress) {
        if (value.schema !== 'vault-buddy-guide-progress/1' || !stepIds.includes(value.stepId) || !Array.isArray(value.reviewed) || value.reviewed.some(id => !stepIds.includes(id)) || typeof value.dim !== 'boolean' || !['new','paused','completed'].includes(value.status)) throw new Error('Invalid guide progress');
        const reviewed = [...new Set(value.reviewed)];
        if (value.status === 'completed' && reviewed.length !== stepIds.length) throw new Error('Incomplete guide cannot be marked completed');
        this.progress = {...value,reviewed}; this.active = false; this.minimized = false; this.suspended = false;
      },
    },
  });
}
