<script setup lang="ts">
/**
 * Record a webcam take and place it as the presenter (Task 50; F-20, F-21;
 * SCREENS 05; ADR R10). The camera's life is `webcamRecorder.ts`'s; this
 * component owns one recorder per opening, renders its states through
 * `WebcamLive`/`WebcamControls`/`WebcamReview`/`WebcamCloseConfirm`, and
 * routes the user's choices.
 *
 * **Opening touches no device.** A recorder is created when the dialog
 * opens, and creating one requests nothing; the idle state explains what
 * happens and waits for *Enable camera* — the only thing that calls the
 * recorder's `enable`, the one `getUserMedia` call in the editor.
 *
 * **Every way out stops the camera**: Close, Escape, a successful Add to
 * timeline, `open` turning false, `pagehide` and unmount all `dispose()`
 * the recorder, and every error disposes it too (inside the recorder).
 *
 * **A finished take is never deleted** (GAP-195): it became a library asset
 * the moment Rust finished it. So Retake says the earlier take stays in the
 * library, and closing with a take not yet on the timeline asks — Back to
 * the take, or Keep in library and close — rather than offering a Discard
 * that could not delete anything. A take still RECORDING is different:
 * closing then asks to discard the recording, which removes its `.part`.
 *
 * **Add to timeline** is `placeTake.placePresenterTake`: a new top video
 * track, the take as its own clip at the playhead, the ADR's presenter
 * placement — three labelled undo steps. Rust stays the authority; a
 * refused step keeps the dialog open with the store's error.
 */
import { computed, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import { placePresenterTake } from "../../../editor/placeTake";
import { type RecorderConstructor, WebcamRecorder, type WebcamView } from "../../../editor/webcamRecorder";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import AppButton from "../../ui/AppButton.vue";
import DialogHost from "../shell/DialogHost.vue";
import WebcamCloseConfirm from "./WebcamCloseConfirm.vue";
import WebcamControls from "./WebcamControls.vue";
import WebcamLive from "./WebcamLive.vue";
import WebcamReview from "./WebcamReview.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const IDLE: WebcamView = { state: "idle", count: null, cameras: [], take: null, problem: null };
const view = shallowRef<WebcamView>(IDLE);
const withMic = ref(false);
const cameraId = ref("");
/** The close question is showing. */
const asking = ref(false);
/** Why the last Add to timeline did not land, or `null`. */
const placeError = ref<string | null>(null);
let recorder: WebcamRecorder | null = null;

function create(): WebcamRecorder {
  return new WebcamRecorder({
    port: project.port,
    sessionId: () => project.sessionId,
    mediaDevices: navigator.mediaDevices,
    Recorder: (globalThis as { MediaRecorder?: RecorderConstructor }).MediaRecorder,
    onChange: (next) => {
      view.value = next;
    },
  });
}

const state = computed(() => view.value.state);
const reviewing = computed(() => state.value === "review" || state.value === "committing");
const finishing = computed(() => state.value === "review" && view.value.take === null);
const busy = computed(() => state.value === "requesting" || state.value === "committing" || finishing.value);
const recording = computed(() => state.value === "recording");
const introducing = computed(() => state.value === "idle" || state.value === "requesting");
/** Re-read whenever the view changes (the recorder itself is not reactive). */
const stream = computed(() => (view.value && recorder ? recorder.stream : null));

function enable(): void {
  void recorder?.enable(cameraId.value || undefined, withMic.value);
}

/** A camera or microphone change asks again — only once enabled. */
function reselect(): void {
  if (state.value === "ready") enable();
}

function record(): void {
  void recorder?.start();
}

function cancel(): void {
  void recorder?.cancel();
}

function retake(): void {
  void recorder?.retake();
}

async function stop(): Promise<void> {
  await recorder?.stop();
  // The finished take is a new asset Rust registered on its own.
  if (view.value.take) void project.refresh();
}

async function add(): Promise<void> {
  if (!recorder) return;
  placeError.value = null;
  const placed = await recorder.commit((take) => placePresenterTake(project, take, workspace.playheadMs));
  if (placed) finishClose();
  else placeError.value = project.lastError?.message ?? "";
}

/** Close now, or ask first when a take would be left behind. */
function requestClose(): void {
  if (busy.value) return;
  if (recording.value || view.value.take) asking.value = true;
  else finishClose();
}

function release(): void {
  asking.value = false;
  placeError.value = null;
  recorder?.dispose();
  recorder = null;
  view.value = IDLE;
}

function finishClose(): void {
  release();
  emit("close");
}

watch(
  () => props.open,
  (open) => {
    if (!open) release();
    else if (!recorder) {
      recorder = create();
      view.value = recorder.view;
    }
  },
  { immediate: true },
);

function onPageHide(): void {
  recorder?.dispose();
}
onMounted(() => window.addEventListener("pagehide", onPageHide));
onBeforeUnmount(() => {
  window.removeEventListener("pagehide", onPageHide);
  release();
});
</script>

<template>
  <DialogHost
    :open="open"
    label="Webcam"
    :closable="!busy"
    @close="requestClose"
  >
    <div
      data-testid="webcam-dialog"
      class="flex w-[32rem] max-w-full flex-col gap-3 text-xs text-fg-secondary"
    >
      <header class="flex items-start justify-between gap-2">
        <div>
          <h2 class="text-sm font-semibold text-fg">
            Webcam
          </h2>
          <p class="text-fg-muted">
            Record yourself as a presenter over your screen. The take becomes its own clip you can move and resize.
          </p>
        </div>
        <AppButton
          variant="ghost"
          size="sm"
          data-testid="webcam-close"
          :disabled="busy"
          @click="requestClose"
        >
          Close
        </AppButton>
      </header>
      <p
        v-if="view.problem"
        role="alert"
        data-testid="webcam-problem"
        class="rounded border border-danger/40 px-2 py-1 text-danger-fg"
      >
        {{ view.problem.message }}
      </p>
      <WebcamReview
        v-if="reviewing"
        :view="view"
        :place-error="placeError"
        @retake="retake"
        @add="add"
      />
      <WebcamLive
        v-else
        v-model:camera-id="cameraId"
        v-model:with-mic="withMic"
        :view="view"
        :stream="stream"
        @reselect="reselect"
      />
      <WebcamCloseConfirm
        v-if="asking"
        :recording="recording"
        @back="asking = false"
        @confirm="finishClose"
      />
      <template v-else-if="!reviewing">
        <p v-if="introducing">
          Nothing is accessed until you press Enable camera. Windows may ask you to allow the camera (and the
          microphone, if you choose it).
        </p>
        <WebcamControls
          :state="state"
          @enable="enable"
          @record="record"
          @cancel="cancel"
          @stop="stop"
        />
      </template>
    </div>
  </DialogHost>
</template>
