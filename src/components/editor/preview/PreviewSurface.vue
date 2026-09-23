<script setup lang="ts">
/**
 * The editor's preview stage (Task 22; F-04, F-14, F-25; NATIVE-MEDIA.md §
 * Preview architecture). A stage sized to the preview column, the project's
 * canvas `contain`-fitted inside it (letterboxed or pillarboxed), and one
 * pooled media element per active clip, all owned by a
 * `PreviewController` — a plain class held in a plain `let`, NEVER in Pinia
 * and never in a `ref` (its own module doc says why). Vue sees only the
 * controller's low-rate reports: `playing`, and the 10 Hz time that also
 * drives `editorWorkspace.playheadMs`.
 *
 * **Media paths come only from Rust.** `resolveUrl` asks
 * `editor_media_url` (through the port) for a REGISTERED asset by id and
 * hands the answer to `convertFileSrc`; this file never builds a path, and
 * the asset protocol's enumerated scope (ADR R7, pinned by `tray.rs`) is
 * the actual boundary. A layer whose media cannot be resolved is NAMED in a
 * status line rather than left as a silent black box (R20).
 *
 * The stage has no height of its own: it `grow`s into whatever the shell's
 * preview column leaves (`EditorShell.vue`'s "Height" note) and the canvas
 * is letterboxed inside whatever that turns out to be, down to zero at the
 * window's floor size — the transport row stays reachable regardless.
 *
 * The preview APPROXIMATES the render (docs/Gaps.md GAP-173): it shows
 * source media placed, stacked, faded by opacity and mixed for monitoring;
 * it does not show effects, captions, cards, transitions or fades, and its
 * sync is element-seek accurate, not frame accurate.
 *
 * `clientToCanvas` (`editor/previewGeometry.ts`, pure and tested) maps a
 * pointer on the stage into output-canvas pixels, undoing the letterbox;
 * the result is emitted as `canvas-pointerdown` for the canvas tools that
 * arrive in later tasks.
 *
 * **Layout handles** (Task 31): `LayoutHandles` is a SIBLING of the stage
 * in one shared wrapper, never inside it — the stage is where the picture
 * is composed. While a handle is dragged it hands back a transient project
 * the controller shows (so the picture moves with the handles); the store
 * is never touched until the one `setLayout` on release, and a `null`
 * hands the controller back the store's own project.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { EditorPortError } from "../../../editor/port";
import type { AudioContextLike } from "../../../editor/previewController";
import { PreviewController } from "../../../editor/previewController";
import { clientToCanvas, containRect } from "../../../editor/previewGeometry";
import type { Project } from "../../../editorTypes";
import { logWarning } from "../../../logging";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import LayoutHandles from "./LayoutHandles.vue";
import TransportBar from "./TransportBar.vue";

const props = defineProps<{
  /** Test seam: happy-dom has no Web Audio. Production leaves it unset and
   * the controller creates a real `AudioContext` on first use. */
  createAudioContext?: () => AudioContextLike | null;
}>();
const emit = defineEmits<{
  (e: "canvas-pointerdown", point: { x: number; y: number }): void;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const stageRef = ref<HTMLElement | null>(null);
const layerHostRef = ref<HTMLElement | null>(null);
const stageSize = ref({ width: 0, height: 0 });
const playing = ref(false);
const currentMs = ref(workspace.playheadMs);
const volume = ref(1);
/** Display names of layers whose media could not be resolved. */
const unavailable = ref<string[]>([]);

let controller: PreviewController | null = null;
/** Bumped every time the controller is rebuilt (a new session). A lookup
 * started under an older generation that settles late must not write its
 * failure into the NEW session's status line. */
let generation = 0;
let observer: ResizeObserver | null = null;

const canvas = computed(() => editorProject.project?.canvas ?? { width: 16, height: 9 });
const frame = computed(() => containRect(canvas.value, stageSize.value));

function assetName(assetId: string): string {
  return editorProject.project?.assets.find((a) => a.id === assetId)?.name ?? assetId;
}

/** `editor_media_url` → `convertFileSrc`, or `null` (and a named status
 * line) when Rust refuses or the file is gone. */
async function resolveUrl(assetId: string): Promise<string | null> {
  const sessionId = editorProject.sessionId;
  const mine = generation;
  if (!sessionId) return null;
  try {
    return convertFileSrc(await editorProject.port.mediaUrl(sessionId, { assetId }), "asset");
  } catch (e) {
    const reason = e instanceof EditorPortError ? e.error.code : String(e);
    logWarning(`preview: no media for asset ${assetId} in session ${sessionId} (${reason})`);
    if (mine !== generation) return null;
    const name = assetName(assetId);
    if (!unavailable.value.includes(name)) unavailable.value = [...unavailable.value, name];
    return null;
  }
}

function createController(): void {
  controller?.destroy();
  generation += 1;
  unavailable.value = [];
  if (!layerHostRef.value) return;
  controller = new PreviewController({
    container: layerHostRef.value,
    resolveUrl,
    createAudioContext: props.createAudioContext,
    onTime: (ms) => {
      currentMs.value = ms;
      workspace.setPlayhead(ms);
    },
    onPlayingChange: (p) => {
      playing.value = p;
    },
  });
  controller.setStage(stageSize.value);
  controller.setMonitor({ muted: workspace.monitorMuted, volume: volume.value });
  controller.setRate(workspace.playbackRate);
  if (editorProject.project) controller.layout(editorProject.project, workspace.playheadMs);
}

function measure(): void {
  const el = stageRef.value;
  if (!el) return;
  stageSize.value = { width: el.clientWidth, height: el.clientHeight };
  controller?.setStage(stageSize.value);
}

onMounted(() => {
  measure();
  createController();
  if (typeof ResizeObserver === "function" && stageRef.value) {
    observer = new ResizeObserver(() => measure());
    observer.observe(stageRef.value);
  }
});
onBeforeUnmount(() => {
  observer?.disconnect();
  controller?.destroy();
  controller = null;
});

// A different session is a different project directory: its URL cache and
// media elements must not carry over.
watch(() => editorProject.sessionId, () => createController());
watch(
  () => editorProject.project,
  (p) => controller?.setProject(p),
);
watch(
  () => [workspace.monitorMuted, volume.value] as const,
  ([muted, v]) => controller?.setMonitor({ muted, volume: v }),
);
watch(
  () => workspace.playbackRate,
  (rate) => controller?.setRate(rate),
);
// The playhead moved somewhere else (the ruler, a timeline click, an undo),
// playing or not: seek. The controller's own 10 Hz report lands here too
// and is a no-op, because it already IS the controller's time (rounded).
watch(
  () => workspace.playheadMs,
  (ms) => {
    currentMs.value = ms;
    if (controller && Math.abs(ms - controller.timeMs) >= 1) void controller.seek(ms);
  },
);

/** A layout drag's transient project, or back to the committed one. */
function onLayoutPreview(preview: Project | null): void {
  controller?.setProject(preview ?? editorProject.project);
}

/** The mixer's peak meter reads the live controller, never a copy. */
function readPeak(): number | null {
  return controller?.readPeak() ?? null;
}

function togglePlay(): void {
  if (!controller) return;
  if (controller.playing) controller.pause();
  else controller.play();
}

function onPointerDown(event: PointerEvent): void {
  const el = stageRef.value;
  if (!el) return;
  emit("canvas-pointerdown", clientToCanvas(event, el.getBoundingClientRect(), canvas.value));
}
</script>

<template>
  <div
    data-testid="preview-surface"
    class="flex min-h-0 grow flex-col gap-1"
  >
    <div class="relative min-h-0 w-full grow basis-0">
      <div
        ref="stageRef"
        data-testid="preview-stage"
        class="absolute inset-0 overflow-hidden rounded-control bg-stage"
        @pointerdown="onPointerDown"
      >
        <div
          data-testid="preview-canvas-frame"
          class="absolute bg-black"
          :style="{
            left: `${frame.left}px`,
            top: `${frame.top}px`,
            width: `${frame.width}px`,
            height: `${frame.height}px`,
          }"
        />
        <div
          ref="layerHostRef"
          data-testid="preview-layers"
          class="absolute inset-0"
        />
      </div>
      <LayoutHandles
        :frame="frame"
        :canvas="canvas"
        @preview="onLayoutPreview"
      />
    </div>
    <p
      v-if="unavailable.length > 0"
      data-testid="preview-unavailable"
      role="status"
      class="text-micro text-danger-fg"
    >
      Not shown in the preview (media unavailable): {{ unavailable.join(", ") }}
    </p>
    <TransportBar
      v-model:volume="volume"
      :playing="playing"
      :current-ms="currentMs"
      :duration-ms="editorProject.durationMs"
      :read-peak="readPeak"
      @toggle-play="togglePlay"
    />
  </div>
</template>
