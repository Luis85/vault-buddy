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
 * render's (`editorJobs.renderError`), never the header's save error. From
 * that moment until Rust answers with the job the dialog cannot be closed
 * either (Task 47's carry): `running` is false until `jobId` is set, and a
 * close then would leave a review render nobody is following.
 *
 * Visual-parity Task 22 (concept spec §9.10): the footer is a
 * `DialogButton` — Cancel review while it runs, Close once it has ended —
 * and why the dialog cannot close yet is said there on screen
 * (`FooterReason`, D14), not only in the ✕'s tooltip.
 */
import { computed, ref, watch } from "vue";

import { useRenderJob } from "../../../composables/useRenderJob";
import { isComplete } from "../../../editor/renderProgress";
import type { RenderRange } from "../../../editorTypes";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import ProductPlayer from "../preview/ProductPlayer.vue";
import RenderProgress from "../preview/RenderProgress.vue";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import FooterReason from "./FooterReason.vue";

const props = defineProps<{ open: boolean; range: RenderRange | null }>();
const emit = defineEmits<{ (e: "close"): void }>();

const jobs = useEditorJobsStore();
const jobId = ref<string | null>(null);
/** The start request is in flight (no job id yet). */
const starting = ref(false);

async function begin(range: RenderRange | null): Promise<void> {
  jobId.value = null;
  jobs.renderError = null;
  if (!range) return;
  starting.value = true;
  try {
    jobId.value = await jobs.startRender({ name: "Review", range, quality: "balanced", review: true });
  } finally {
    starting.value = false;
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) void begin(props.range);
  },
  { immediate: true },
);

const { job, running, refusal } = useRenderJob(jobId);
const busy = computed(() => starting.value || running.value);
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
  if (!busy.value) emit("close");
}

/** Why the ✕ is disabled right now, or `null` while it isn't — computed
 * here rather than inline in the template so the dialog's own markup stays
 * a flat read of one value (fallow's template complexity gate treats a
 * whole `<template>` as one function). */
const closeReason = computed<string | null>(() => {
  if (!busy.value) return null;
  return starting.value ? "Starting the review…" : "A review is running.";
});
</script>

<template>
  <DialogHost
    :open="open"
    label="Review the rendered range"
    :closable="!busy"
    close-testid="review-dialog-close"
    :close-reason="closeReason"
    @close="close"
  >
    <template #title>
      Review
    </template>
    <template #subtitle>
      This range, rendered for real. A review is not saved as a product, and
      your project is not changed.
    </template>

    <div
      data-testid="review-dialog"
      class="flex flex-col gap-3"
    >
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
    </div>

    <template #footer>
      <FooterReason
        data-testid="review-dialog-reason"
        :text="closeReason"
      />
      <DialogButton
        v-if="running"
        data-testid="review-dialog-cancel"
        @click="jobId && jobs.cancel(jobId)"
      >
        Cancel review
      </DialogButton>
      <DialogButton
        v-else-if="!busy"
        data-testid="review-dialog-done"
        @click="close"
      >
        Close
      </DialogButton>
    </template>
  </DialogHost>
</template>
