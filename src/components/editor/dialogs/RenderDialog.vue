<script setup lang="ts">
/**
 * Render a video (Task 47; F-41, F-42; SCREENS 09 "Render and product
 * review"). Name the product (`<title> v<n>` by default), pick a quality,
 * render the whole project or an output-time range (defaulting to the
 * in/out range the caller hands in, when there is one), read the checks
 * summary and the originals statement (`RenderSettingsForm`), then start —
 * Rust freezes the revision on screen and renders it on its own thread
 * (`editor_start_render`).
 *
 * Progress is the job's Channel, through `editorJobs` (`RenderOutcome`):
 * its phases in words and a bar that reaches 100 % ONLY on the `complete`
 * terminal (R20). Cancel asks Rust to stop (`editor_cancel_job`); a cancel
 * is not a failure and never reads as one. Completion offers Watch rendered
 * file, Publish to vault… (Task 48, `PublishDialog` over this one) and
 * Render another.
 *
 * Every error here is the RENDER's (`editorJobs.renderError`, the job's
 * terminal) — never `editorProject.saveError`, so a refused render cannot
 * read as "Save failed" in the header (Task 46's carry). While a render is
 * running — and from the Render click until Rust has answered with its job
 * (Task 47's carry: `running` is false until `jobId` is set, so a close
 * then would leave a render nobody is following) — the dialog cannot be
 * dismissed by Close, Escape or the backdrop: Cancel is the explicit way
 * out (`DialogHost`'s `closable`).
 */
import { computed, ref, watch } from "vue";

import { useRenderJob } from "../../../composables/useRenderJob";
import { isComplete } from "../../../editor/renderProgress";
import { msFromSeconds, secondsText } from "../../../editor/renderRanges";
import type { RenderQuality, RenderRange } from "../../../editorTypes";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import { useEditorProductsStore } from "../../../stores/editorProducts";
import { useEditorProjectStore } from "../../../stores/editorProject";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";
import PublishDialog from "./PublishDialog.vue";
import RenderOutcome from "./RenderOutcome.vue";
import RenderSettingsForm from "./RenderSettingsForm.vue";

const props = defineProps<{ open: boolean; initialRange: RenderRange | null }>();
const emit = defineEmits<{ (e: "close"): void }>();

const editorProject = useEditorProjectStore();
const jobs = useEditorJobsStore();
const products = useEditorProductsStore();

const nameDraft = ref<string | null>(null);
const quality = ref<RenderQuality>("balanced");
const scope = ref<"whole" | "range">("whole");
/** The range fields, in seconds as typed (a number once edited). */
const startText = ref<string | number>("0");
const endText = ref<string | number>("0");
const jobId = ref<string | null>(null);
/** A start is in flight: one click, one render. */
const starting = ref(false);
/** The name the render was started under — the default name moves on as
 * soon as the new product is listed. */
const startedName = ref("");

const { job, running, refusal } = useRenderJob(jobId);
/** A start in flight OR a render running: nothing may close the dialog. */
const busy = computed(() => starting.value || running.value);
/** Task 48: the Publish dialog, over this one, for the new product. */
const publishOpen = ref(false);

const durationMs = computed(() => editorProject.durationMs);
const defaultName = computed(
  () => `${editorProject.snapshot?.title ?? "Untitled"} v${products.current.length + 1}`,
);
const name = computed({
  get: () => nameDraft.value ?? defaultName.value,
  set: (value: string) => {
    nameDraft.value = value;
  },
});

function another(): void {
  publishOpen.value = false;
  jobId.value = null;
  nameDraft.value = null;
  jobs.renderError = null;
}

function reset(): void {
  const range = props.initialRange ?? { startMs: 0, endMs: durationMs.value };
  quality.value = "balanced";
  scope.value = "whole";
  startText.value = secondsText(range.startMs);
  endText.value = secondsText(range.endMs);
  another();
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    reset();
    void products.refresh();
  },
  { immediate: true },
);

/** The chosen range in output ms, or `null` when the fields do not make
 * one inside the project. */
const chosenRange = computed<RenderRange | null>(() => {
  const startMs = msFromSeconds(String(startText.value));
  const endMs = msFromSeconds(String(endText.value));
  if (startMs === null || endMs === null) return null;
  return endMs > startMs && endMs <= durationMs.value ? { startMs, endMs } : null;
});

const startReason = computed<string | null>(() => {
  if (!editorProject.sessionId) return "No project is open.";
  if (starting.value) return "Starting the render…";
  if (durationMs.value === 0) return "There is nothing to render yet. Place a clip on the timeline first.";
  if (editorProject.pending.size > 0) return "Wait for the last edit to finish.";
  if (!name.value.trim()) return "Name the rendered video.";
  if (scope.value === "range" && !chosenRange.value) {
    return `The range must end after it starts, within 0–${secondsText(durationMs.value)} s.`;
  }
  return null;
});

const showForm = computed(() => jobId.value === null && !refusal.value);
const productId = computed(() => (job.value && isComplete(job.value) ? (job.value.terminal?.productId ?? null) : null));

/** The one line once the render ended (or never started). */
const status = computed<{ text: string; alert: boolean } | null>(() => {
  if (refusal.value) return { text: `The render could not start. ${refusal.value.message}`, alert: true };
  const j = job.value;
  if (!j || j.terminal === null) return null;
  if (isComplete(j)) return { text: `Render complete. “${startedName.value}” is now one of this project's products.`, alert: false };
  if (j.phase === "cancelled") return { text: "Render cancelled. Nothing was created, and your project is unchanged.", alert: false };
  return { text: `The render did not finish. ${j.terminal.error?.message ?? ""}`.trim(), alert: true };
});

async function start(): Promise<void> {
  if (startReason.value) return;
  jobs.renderError = null;
  starting.value = true;
  startedName.value = name.value.trim();
  try {
    jobId.value = await jobs.startRender({
      name: startedName.value,
      range: scope.value === "range" ? chosenRange.value : null,
      quality: quality.value,
    });
  } finally {
    starting.value = false;
  }
}

function cancel(): void {
  if (jobId.value) void jobs.cancel(jobId.value);
}

function close(): void {
  if (!busy.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Render a video"
    :closable="!busy"
    @close="close"
  >
    <div
      data-testid="render-dialog"
      class="flex w-[32rem] max-w-full flex-col gap-3"
    >
      <header class="sticky -top-4 z-10 -mx-4 -mt-4 flex items-start justify-between border-b border-line bg-panel px-4 pb-3 pt-4">
        <div>
          <h2 class="text-sm font-semibold text-fg">
            Render a video
          </h2>
          <p class="text-xs text-fg-muted">
            A finished video from your editable workspace.
          </p>
        </div>
        <AppButton
          variant="ghost"
          size="sm"
          data-testid="render-dialog-close"
          :disabled="busy"
          @click="close"
        >
          Close
        </AppButton>
      </header>

      <template v-if="showForm">
        <RenderSettingsForm
          v-model:name="name"
          v-model:quality="quality"
          v-model:scope="scope"
          v-model:start="startText"
          v-model:end="endText"
        />
        <div class="flex items-center justify-end gap-2">
          <span
            v-if="startReason"
            data-testid="render-dialog-start-reason"
            class="text-micro text-fg-subtle"
          >{{ startReason }}</span>
          <AppButton
            variant="primary"
            size="sm"
            data-testid="render-dialog-start"
            :disabled="Boolean(startReason)"
            @click="start"
          >
            Render video
          </AppButton>
        </div>
      </template>
      <RenderOutcome
        v-else
        :job="job"
        :running="running"
        :status="status"
        :product-id="productId"
        :name="startedName"
        @cancel="cancel"
        @another="another"
        @publish="publishOpen = true"
      />
      <PublishDialog
        :open="publishOpen"
        :product-id="productId"
        :product-name="startedName"
        @close="publishOpen = false"
      />
    </div>
  </DialogHost>
</template>
