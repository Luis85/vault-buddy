/**
 * Starting a media import (Task 25; visual-parity Task 10): the Media
 * tab's **Import media** and the Titles tab's **Import a still image**
 * (concept spec §3.3 — the import accepts PNG, JPEG and WebP) are the same
 * native action, so they share one refusal and one start. Rust opens its
 * OWN file dialog (`editor_import_media`); nothing here sends a path.
 */
import type { ComputedRef } from "vue";
import { computed } from "vue";

import { useEditorJobsStore } from "../stores/editorJobs";
import { useEditorProjectStore } from "../stores/editorProject";

export interface MediaImport {
  /** Why an import cannot start now, or `null`. */
  refusal: ComputedRef<string | null>;
  /** Starts one when it can; says whether it did. */
  start: () => boolean;
}

export function useMediaImport(): MediaImport {
  const project = useEditorProjectStore();
  const jobs = useEditorJobsStore();
  const refusal = computed<string | null>(() => {
    if (!project.sessionId) return "Open a project first.";
    if (jobs.activeImport) return "An import is already running.";
    return null;
  });
  function start(): boolean {
    if (refusal.value !== null) return false;
    void jobs.importMedia();
    return true;
  }
  return { refusal, start };
}
