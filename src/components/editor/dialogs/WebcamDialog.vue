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
 * Stop & review turns the camera and microphone off as well (Ruling F-1),
 * which is what the privacy line promises; Retake turns the same ones back
 * on.
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
 *
 * **ffmpeg is pre-flighted** through the app's cached `useFfmpegStore`
 * probe before the countdown (fix round 1), so a KNOWN-missing ffmpeg is
 * reported at once. An unknown status (a failed probe) blocks nothing:
 * `editor_webcam_begin`'s own `encoderUnavailable` stays the authority.
 *
 * **Visual-parity Task 22** (concept spec §9.7, screen 05): 960 wide; the
 * 16:9 camera view and its status row left (`WebcamLive`, or the finished
 * take in `WebcamReview`), "Set up your take" right (`WebcamSettings`), the
 * privacy strip under both, and a footer per phase (`WebcamControls`, or the
 * close question, `WebcamCloseConfirm`). No "Try demo overlay" (D10). A
 * refused Add to timeline is claimed for this dialog while its own request
 * is in flight (`useInlineLastError().track`, ruling T7-1): said here, not
 * toasted as well; anything else still toasts.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";

import { useInlineLastError } from "../../../composables/useInlineLastError";
import { placePresenterTake } from "../../../editor/placeTake";
import { insertTimeMs, type WebcamAction, webcamBusyReason } from "../../../editor/webcamPhase";
import {
  ENCODER_UNAVAILABLE_TEXT,
  type RecorderConstructor,
  type WebcamProblem,
  WebcamRecorder,
  type WebcamView,
} from "../../../editor/webcamRecorder";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { useFfmpegStore } from "../../../stores/ffmpeg";
import EditorIcon from "../icons/EditorIcon.vue";
import DialogHost from "../shell/DialogHost.vue";
import WebcamCloseConfirm from "./WebcamCloseConfirm.vue";
import WebcamControls from "./WebcamControls.vue";
import WebcamLive from "./WebcamLive.vue";
import WebcamReview from "./WebcamReview.vue";
import WebcamSettings from "./WebcamSettings.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ (e: "close"): void }>();

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const ffmpeg = useFfmpegStore();
const inline = useInlineLastError();

const IDLE: WebcamView = { state: "idle", count: null, cameras: [], microphones: [], take: null, problem: null };
const view = shallowRef<WebcamView>(IDLE);
const withMic = ref(false);
const cameraId = ref("");
const micId = ref("");
/** Insert at timeline time, seconds as typed; the playhead at each open. */
const insertAt = ref<string | number>("0");
/** Mirror the live preview, and the placed clip's own flag. */
const mirror = ref(false);
/** The close question is showing. */
const asking = ref(false);
/** Why the last Add to timeline did not land, or `null`. */
const placeError = ref<string | null>(null);
let recorder: WebcamRecorder | null = null;

/** A take needs ffmpeg to be finished; refuse early only when the probe
 * positively says it is missing. */
async function ffmpegPreflight(): Promise<WebcamProblem | null> {
  await ffmpeg.ensureDetected();
  return ffmpeg.status?.installed === false ? { kind: "encoderUnavailable", message: ENCODER_UNAVAILABLE_TEXT } : null;
}

/** A recorder whose changes reach the view only while it is THIS
 * opening's: one released on close (a take finishing, a device listing
 * ending) must not write over the view of the one that replaced it. */
function create(): WebcamRecorder {
  const created: WebcamRecorder = new WebcamRecorder({
    port: project.port,
    sessionId: () => project.sessionId,
    mediaDevices: navigator.mediaDevices,
    Recorder: (globalThis as { MediaRecorder?: RecorderConstructor }).MediaRecorder,
    preflight: ffmpegPreflight,
    onChange: (next) => {
      if (recorder === created) view.value = next;
    },
  });
  return created;
}

const state = computed(() => view.value.state);
const reviewing = computed(() => state.value === "review" || state.value === "committing");
/** Why the dialog is mid-operation (the ✕ and the footer wait), or `null`. */
const busyReason = computed(() => webcamBusyReason(state.value, view.value.take !== null));
const busy = computed(() => busyReason.value !== null);
const insertAtMs = computed(() => insertTimeMs(insertAt.value, project.durationMs));
/** Why the chosen insert time cannot be used, or `null`. */
const placeReason = computed(() => ("reason" in insertAtMs.value ? insertAtMs.value.reason : null));
const recording = computed(() => state.value === "recording");
/** Re-read whenever the view changes (the recorder itself is not reactive). */
const stream = computed(() => (view.value && recorder ? recorder.stream : null));

function enable(): void {
  void recorder?.enable(cameraId.value || undefined, withMic.value, micId.value || undefined);
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
  const at = insertAtMs.value;
  if (!("ms" in at)) return;
  const active = recorder;
  const options = { mirror: mirror.value };
  const placed = await inline.track(() => active.commit((take) => placePresenterTake(project, take, at.ms, options)));
  if (placed) finishClose();
  else placeError.value = inline.error.value?.message ?? "";
}

const ACTIONS: Record<WebcamAction, () => unknown> = {
  enable,
  // Closes at once; the recorder turns a late answer straight off.
  "cancel-request": finishClose,
  record,
  cancel,
  stop,
  retake,
  add,
};
function act(action: WebcamAction): void {
  void ACTIONS[action]();
}

/** Close now, or ask first when a take would be left behind. */
function requestClose(): void {
  if (busy.value) return;
  if (recording.value || view.value.take) asking.value = true;
  else finishClose();
}

const body = ref<HTMLElement | null>(null);

/** Leave the close question. Its buttons go with it, so focus moves to the
 * footer's first live button rather than falling to the page. */
async function backToTake(): Promise<void> {
  asking.value = false;
  await nextTick();
  body.value?.closest('[role="dialog"]')?.querySelector<HTMLElement>("footer button:not([disabled])")?.focus();
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
      insertAt.value = (workspace.playheadMs / 1000).toFixed(2);
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
    label="Bring yourself into the tutorial"
    :width="960"
    :closable="!busy"
    close-testid="webcam-close"
    :close-reason="busyReason"
    @close="requestClose"
  >
    <template #title>
      Bring yourself into the tutorial
    </template>
    <template #subtitle>
      Your camera. A separate, editable layer.
    </template>

    <div
      ref="body"
      data-testid="webcam-dialog"
      class="flex flex-col gap-5 text-xs text-fg-secondary"
    >
      <p
        v-if="view.problem"
        role="alert"
        data-testid="webcam-problem"
        class="rounded-[7px] border border-danger/40 px-3 py-2 text-danger-fg"
      >
        {{ view.problem.message }}
      </p>
      <div
        data-testid="webcam-body"
        class="grid grid-cols-[minmax(0,1fr)_230px] gap-6 max-[760px]:grid-cols-1"
      >
        <WebcamReview
          v-if="reviewing"
          :view="view"
          :place-error="placeError"
        />
        <WebcamLive
          v-else
          :view="view"
          :stream="stream"
          :mirror="mirror"
        />
        <WebcamSettings
          v-model:camera-id="cameraId"
          v-model:with-mic="withMic"
          v-model:mic-id="micId"
          v-model:insert-at="insertAt"
          v-model:mirror="mirror"
          :view="view"
          :stream="stream"
          :duration-ms="project.durationMs"
          @reselect="reselect"
        />
      </div>
      <p
        data-testid="webcam-privacy"
        class="flex items-start gap-2.5 rounded-[7px] border border-line bg-app px-[15px] py-[13px] text-[11px] leading-[1.7] text-fg-muted"
      >
        <EditorIcon
          name="shield"
          :size="16"
          class="mt-0.5 shrink-0 text-audio"
        />
        Permission is explicit. Camera and microphone stop after recording or closing. The take is kept in this project
        as soon as you stop; rendering is optional.
      </p>
    </div>

    <template #footer>
      <WebcamCloseConfirm
        v-if="asking"
        :recording="recording"
        @back="backToTake"
        @confirm="finishClose"
      />
      <WebcamControls
        v-else
        :state="state"
        :has-take="view.take !== null"
        :place-reason="placeReason"
        @act="act"
      />
    </template>
  </DialogHost>
</template>
