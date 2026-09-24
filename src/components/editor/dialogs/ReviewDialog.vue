<script setup lang="ts">
/**
 * A Review render (Task 47; F-42; pre-flight F18): the preview toolbar's
 * Review renders `range` — the selection, or 5 s either side of the
 * playhead — with the REAL renderer and plays the encoded file back, the
 * one bridge from the webview's approximate preview to what a render will
 * actually contain (ADR GAP-N3).
 *
 * A review is not a product. Rust keeps it in the project's cache
 * (`cache\review-<jobId>.mp4`), never in `products\` or the ledger, drops
 * it when the next review lands or the session closes, and its terminal
 * names no `productId` — so it never reaches the product library or its
 * 40-product cap. It is still a render job: one at a time per session,
 * cancellable here, and counted by the quit/update gate.
 *
 * The review starts the moment the dialog opens; its errors are the
 * render's (`editorJobs.renderError`), never the header's save error.
 */
import { computed, ref, watch } from "vue";

import { useRenderJob } from "../../../composables/useRenderJob";
import { isComplete } from "../../../editor/renderProgress";
import type { RenderRange } from "../../../editorTypes";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import AppButton from "../../ui/AppButton.vue";
import ProductPlayer from "../preview/ProductPlayer.vue";
import RenderProgress from "../preview/RenderProgress.vue";
import DialogHost from "../shell/DialogHost.vue";

const props = defineProps<{ open: boolean; range: RenderRange | null }>();
const emit = defineEmits<{ (e: "close"): void }>();

const jobs = useEditorJobsStore();
const jobId = ref<string | null>(null);

async function begin(range: RenderRange | null): Promise<void> {
  jobId.value = null;
  jobs.renderError = null;
  if (!range) return;
  jobId.value = await jobs.startRender({ name: "Review", range, quality: "balanced", review: true });
}

watch(
  () => props.open,
  (open) => {
    if (open) void begin(props.range);
  },
  { immediate: true },
);

const { job, running, refusal } = useRenderJob(jobId);
const media = computed(() => (job.value && isComplete(job.value) ? { reviewJobId: job.value.jobId } : null));
const cancelled = computed(() => job.value?.phase === "cancelled");
/** A failed job's own message (`null` unless it failed). */
const failure = computed(() => (job.value?.phase === "failed" ? (job.value.terminal?.error?.message ?? "") : null));

/** Why nothing is playing, when something went wrong (no range, a refused
 * start, a failed job); a cancel is not a problem and says so plainly. */
const problem = computed<string | null>(() => {
  if (!props.range) return "There is nothing to review here yet.";
  if (refusal.value) return refusal.value.message;
  return failure.value === null ? null : `The review did not finish. ${failure.value}`.trim();
});

function close(): void {
  if (!running.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Review the rendered range"
    :closable="!running"
    @close="close"
  >
    <div
      data-testid="review-dialog"
      class="flex w-[32rem] max-w-full flex-col gap-3"
    >
      <header class="flex items-start justify-between gap-2">
        <div>
          <h2 class="text-sm font-semibold text-fg">
            Review
          </h2>
          <p class="text-xs text-fg-muted">
            This range, rendered for real. A review is not saved as a product, and
            your project is not changed.
          </p>
        </div>
        <AppButton
          variant="ghost"
          size="sm"
          data-testid="review-dialog-close"
          :disabled="running"
          @click="close"
        >
          Close
        </AppButton>
      </header>
      <RenderProgress
        v-if="job && !media"
        :job="job"
      />
      <ProductPlayer
        v-if="media"
        :media="media"
        label="Rendered review"
      />
      <p
        v-if="problem"
        role="alert"
        data-testid="review-dialog-problem"
        class="text-xs text-danger-fg"
      >
        {{ problem }}
      </p>
      <p
        v-if="cancelled"
        role="status"
        class="text-xs text-fg-secondary"
      >
        Review cancelled.
      </p>
      <div
        v-if="running"
        class="flex justify-end"
      >
        <AppButton
          variant="secondary"
          size="sm"
          data-testid="review-dialog-cancel"
          @click="jobId && jobs.cancel(jobId)"
        >
          Cancel review
        </AppButton>
      </div>
    </div>
  </DialogHost>
</template>
