/**
 * One render or review job as a dialog follows it (Task 47): the job the
 * dialog started (`jobId`, `null` before a start or after a refusal), read
 * from `editorJobs` — the store the job's Channel feeds — never a copy.
 *
 * `refusal` is a start Rust refused before any job existed; it reads the
 * render's own error (`editorJobs.renderError`), never the header's save
 * error (Task 46's carry).
 */
import type { Ref } from "vue";
import { computed } from "vue";

import { useEditorJobsStore } from "../stores/editorJobs";

export function useRenderJob(jobId: Ref<string | null>) {
  const jobs = useEditorJobsStore();
  const job = computed(() => (jobId.value ? (jobs.jobs[jobId.value] ?? null) : null));
  const running = computed(() => job.value !== null && job.value.terminal === null);
  const refusal = computed(() => (jobId.value === null ? jobs.renderError : null));
  return { job, running, refusal };
}
