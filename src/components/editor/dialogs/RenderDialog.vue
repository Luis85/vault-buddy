<script setup lang="ts">
/**
 * Render a video (Task 47; F-41, F-42; SCREENS 09 "Render and product
 * review"). Name the product (`<title> v<n>` by default), pick a quality,
 * render the whole project or an output-time range (defaulting to the
 * in/out range the caller hands in, when there is one), read the checks
 * summary and the originals statement (`RenderSettingsForm`), then start —
 * unless a BLOCKING check stands (Task 54, `editorChecks.blockedReason`:
 * only blocking findings block, a warning never does) —
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
 *
 * `resumeJobId` (hardening Task 15, GAP-208): a render that was already
 * running when the editor webview reloaded. The dialog opens on its
 * progress instead of the form and follows it by polling the job registry
 * (`editorJobs.follow`), since its Channel went with the old webview; the
 * name it was started under went too, so its completion line says so
 * without one — and a Review render (no product) says only that it
 * finished.
 *
 * Visual-parity Task 21 (concept spec §9.6, screen 09): the concept's form
 * (`RenderSettingsForm`) with the destination vault by NAME (design D6,
 * `useVaultName` — never the registry id), and its footer: **Save project
 * instead** — the header's own Save project (`useProjectSave`: no second
 * save while one runs), after which this dialog steps aside — and
 * **Render video**.
 */
import { computed, ref, watch } from "vue";

import { useProjectSave } from "../../../composables/useProjectSave";
import { useRenderJob } from "../../../composables/useRenderJob";
import { useVaultName } from "../../../composables/useVaultName";
import { completionText, isComplete } from "../../../editor/renderProgress";
import { msFromSeconds, secondsText } from "../../../editor/renderRanges";
import { openChecks } from "../../../editor/revealBus";
import { revealWorkspaceProducts } from "../../../editor/revealProducts";
import type { RenderQuality, RenderRange } from "../../../editorTypes";
import { useEditorChecksStore } from "../../../stores/editorChecks";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import { useEditorProductsStore } from "../../../stores/editorProducts";
import { useEditorProjectStore } from "../../../stores/editorProject";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";
import PublishDialog from "./PublishDialog.vue";
import RenderOutcome from "./RenderOutcome.vue";
import RenderSettingsForm from "./RenderSettingsForm.vue";

const props = defineProps<{ open: boolean; initialRange: RenderRange | null; resumeJobId?: string | null }>();
const emit = defineEmits<{ (e: "close"): void }>();

const editorProject = useEditorProjectStore();
const jobs = useEditorJobsStore();
const products = useEditorProductsStore();
const checks = useEditorChecksStore();
const projectSave = useProjectSave();
/** Looked up only while the dialog is open: it stays mounted, closed. */
const { label: vaultName, busy: vaultBusy } = useVaultName(() =>
  props.open ? (editorProject.project?.destination.vault ?? "") : "",
);
const canvas = computed(() => editorProject.project?.canvas ?? { width: 0, height: 0, fps: 0 });

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
    if (props.resumeJobId) {
      startedName.value = "";
      jobId.value = props.resumeJobId;
      void jobs.follow(props.resumeJobId);
    }
    void products.refresh();
    void checks.refresh();
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
  if (checks.blockedReason) return checks.blockedReason;
  if (editorProject.pending.size > 0) return "Wait for the last edit to finish.";
  if (!name.value.trim()) return "Name the rendered video.";
  if (scope.value === "range" && !chosenRange.value) {
    return `The range must end after it starts, within 0–${secondsText(durationMs.value)} s.`;
  }
  return null;
});

/** What the render will be as long as: the range, when one is chosen. */
const outputMs = computed(() => {
  const range = scope.value === "range" ? chosenRange.value : null;
  return range ? range.endMs - range.startMs : durationMs.value;
});

const showForm = computed(() => jobId.value === null && !refusal.value);
const productId = computed(() => (job.value && isComplete(job.value) ? (job.value.terminal?.productId ?? null) : null));

/** The one line once the render ended (or never started). */
const status = computed<{ text: string; alert: boolean } | null>(() => {
  if (refusal.value) return { text: `The render could not start. ${refusal.value.message}`, alert: true };
  const j = job.value;
  if (!j || j.terminal === null) return null;
  if (isComplete(j)) return { text: completionText(startedName.value, j.terminal.productId), alert: false };
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

/** Task 54: the whole list lives in the Checks dialog; this one steps
 * aside so a revealed object is not hidden behind it. */
function reviewChecks(): void {
  close();
  openChecks();
}

const checksError = computed(() => checks.currentError?.message ?? null);
const closeReason = computed(() => {
  if (!busy.value) return null;
  return starting.value ? "Starting the render…" : "A render is running.";
});
/** The render's parent and where a publish goes (the form's facts). */
const facts = computed(() => ({
  revision: editorProject.snapshot?.revision ?? 0,
  title: editorProject.snapshot?.title ?? "",
  folder: editorProject.project?.destination.folder ?? "",
}));

/** Save project instead: the header's Save project, then out of the way.
 * Never while a start is in flight — that render still needs following. */
const saveInsteadReason = computed(() => (busy.value ? "Starting the render…" : projectSave.disabledReason.value));
/** Save project instead's reason when Render's own line does not already
 * say it — on screen, not only in the tooltip (D14, fix round 1). */
const saveOnlyReason = computed(() =>
  saveInsteadReason.value === startReason.value ? null : saveInsteadReason.value,
);
function saveInstead(): void {
  if (saveInsteadReason.value) return;
  projectSave.save();
  close();
}

/** Visual-parity Task 10 (D9): the library's Project section, through the
 * Project menu's and the status bar's own helper. */
function showProducts(): void {
  close();
  revealWorkspaceProducts();
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Render a video"
    :closable="!busy"
    close-testid="render-dialog-close"
    :close-reason="closeReason"
    @close="close"
  >
    <template #title>
      Render a video
    </template>
    <template #subtitle>
      A finished output from your editable workspace.
    </template>

    <div
      data-testid="render-dialog"
      class="flex flex-col gap-4"
    >
      <template v-if="showForm">
        <RenderSettingsForm
          v-model:name="name"
          v-model:quality="quality"
          v-model:scope="scope"
          v-model:start="startText"
          v-model:end="endText"
          :checks-summary="checks.summary"
          :checks-error="checksError"
          :blocking="checks.blocking"
          :revision="facts.revision"
          :title="facts.title"
          :vault-name="vaultName"
          :vault-busy="vaultBusy"
          :folder="facts.folder"
          :canvas="canvas"
          :duration-ms="outputMs"
          @review-checks="reviewChecks"
        />
      </template>
      <RenderOutcome
        v-else
        :job="job"
        :running="running"
        :status="status"
        :product-id="productId"
        :name="startedName"
        :vault-name="vaultName"
        @cancel="cancel"
        @another="another"
        @publish="publishOpen = true"
        @products="showProducts"
      />
      <PublishDialog
        :open="publishOpen"
        :product-id="productId"
        :product-name="startedName"
        @close="publishOpen = false"
      />
    </div>

    <template
      v-if="showForm"
      #footer
    >
      <span class="mr-auto flex flex-col text-[10px] text-fg-muted">
        <span
          data-testid="render-dialog-start-reason"
          role="status"
          aria-live="polite"
        >{{ startReason ?? "" }}</span>
        <span
          data-testid="render-dialog-save-reason"
          role="status"
          aria-live="polite"
        >{{ saveOnlyReason ?? "" }}</span>
      </span>
      <DialogButton
        data-testid="render-dialog-save-instead"
        :reason="saveInsteadReason"
        @click="saveInstead"
      >
        Save project instead
      </DialogButton>
      <DialogButton
        variant="primary"
        icon="video"
        data-testid="render-dialog-start"
        :reason="startReason"
        @click="start"
      >
        Render video
      </DialogButton>
    </template>
  </DialogHost>
</template>
