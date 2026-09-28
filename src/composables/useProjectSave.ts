/**
 * Save project — the header's button and (visual-parity Task 4, design
 * D10) the status bar's recovery slot share this one path, so the two can
 * never disagree about when a save is possible or do it two ways. A save
 * commits the session to the project store (`editorProject.save`); the
 * disabled reason is shown beside the control that cannot act (R20: "a
 * disabled control carries a reason string").
 */
import { computed } from "vue";

import { useEditorProjectStore } from "../stores/editorProject";

export function useProjectSave() {
  const editorProject = useEditorProjectStore();
  const disabledReason = computed<string | null>(() => {
    if (!editorProject.sessionId) return "No project is open.";
    if (editorProject.saving) return "Saving…";
    return null;
  });
  function save(): void {
    void editorProject.save();
  }
  return { disabledReason, save };
}
