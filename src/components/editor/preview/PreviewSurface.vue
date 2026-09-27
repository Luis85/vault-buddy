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
 * status line rather than left as a silent black box (R20). A reconnect
 * (Task 40) that takes an asset off the missing list makes the controller
 * forget that asset's failed lookup, so it is asked for again.
 *
 * The stage has no height of its own: it `grow`s into whatever the shell's
 * preview column leaves (`EditorShell.vue`'s "Height" note) and the canvas
 * is letterboxed inside whatever that turns out to be, down to zero at the
 * window's floor size — the transport row stays reachable regardless.
 *
 * The preview APPROXIMATES the render (docs/Gaps.md GAP-173): it shows
 * source media placed, stacked, faded by opacity and mixed for monitoring,
 * plus teaching cues (below); it does not show captions or transitions, and
 * its sync is element-seek accurate, not frame accurate.
 *
 * **Clicking the picture selects** (visual-parity Task 11; design D15, the
 * no-op audit's finding 5 — this used to emit a `canvas-pointerdown` nobody
 * listened to): a primary press on the stage maps the pointer into the
 * canvas (`cueGeometry.pointerToCanvas`, which undoes the letterbox and an
 * active zoom) and selects the topmost visible clip under it
 * (`stageHit.clipAtPoint`); a press on no picture clears the selection. The
 * layout box and the cue handles are siblings drawn over the stage, so a
 * press on them never reaches it: their own drags win on their own areas.
 *
 * **The stage's frame** (§4.2): 14px/20px of `stage` around the canvas
 * (10px/14px in a window 760px tall or less), the canvas itself on the
 * concept's near-black with a 6px radius and a soft shadow.
 *
 * **Layout handles** (Task 31): `LayoutHandles` is a SIBLING of the stage
 * in one shared wrapper, never inside it — the stage is where the picture
 * is composed. While a handle is dragged it hands back a transient project
 * the controller shows (so the picture moves with the handles); the store
 * is never touched until the one `setLayout` on release, and a `null`
 * hands the controller back the store's own project.
 *
 * **Teaching cues** (Task 35; F-27–F-33) come in two layers, split exactly
 * like the layout handles: `CueOverlay` (the cues as rendered) sits INSIDE
 * the stage above the media (with Task 36's `CaptionOverlay` above the
 * cues, unzoomed: a caption sits on the frame, not on the footage), and
 * `CueHandles` (selection, grab handles, the
 * zoom's focal marker) is a sibling AFTER `LayoutHandles`, so a cue over a
 * full-frame clip stays reachable. An active zoom cue scales the media
 * layers and the cue overlay alike (`cueGeometry.activeZoom`), clipped to
 * the canvas frame so a zoom never spills into the letterbox; it follows
 * the controller's 10 Hz time, so the ramp steps rather than glides.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { activeCues, activeZoom, pointerToCanvas } from "../../../editor/cueGeometry";
import { EditorPortError } from "../../../editor/port";
import type { AudioContextLike } from "../../../editor/previewController";
import { PreviewController } from "../../../editor/previewController";
import { containRect } from "../../../editor/previewGeometry";
import { clipAtPoint } from "../../../editor/stageHit";
import type { Effect, Project } from "../../../editorTypes";
import { logWarning } from "../../../logging";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import CaptionOverlay from "./CaptionOverlay.vue";
import CueHandles from "./CueHandles.vue";
import CueOverlay from "./CueOverlay.vue";
import LayoutHandles from "./LayoutHandles.vue";
import TransportBar from "./TransportBar.vue";

const props = defineProps<{
  /** Test seam: happy-dom has no Web Audio. Production leaves it unset and
   * the controller creates a real `AudioContext` on first use. */
  createAudioContext?: () => AudioContextLike | null;
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

/** A cue drag's transient copy of one effect (R14), or `null`. */
const cueDraft = ref<Effect | null>(null);
const zoom = computed(() => activeZoom(activeCues(editorProject.project, currentMs.value), currentMs.value));

/** The media layers under the zoom, scaled about the canvas frame's origin. */
const layersStyle = computed(() => {
  const [z, f] = [zoom.value, frame.value];
  return {
    transformOrigin: `${f.left}px ${f.top}px`,
    transform: `translate(${z.tx * f.width}px, ${z.ty * f.height}px) scale(${z.scale})`,
  };
});
/** While zoomed in, the scaled layers are clipped to the canvas frame. */
const zoomClip = computed(() => {
  if (zoom.value.scale <= 1) return {};
  const f = frame.value;
  const right = stageSize.value.width - f.left - f.width;
  const bottom = stageSize.value.height - f.top - f.height;
  return { clipPath: `inset(${f.top}px ${right}px ${bottom}px ${f.left}px)` };
});

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
// Task 40: an asset that stopped being missing was reconnected. Its failed
// lookup must not outlive that: the controller forgets it and asks again,
// and its name leaves the status line.
watch(
  () => editorProject.missing,
  (now, before) => {
    const still = new Set(now.map((m) => m.assetId));
    const found = before.filter((m) => !still.has(m.assetId));
    if (found.length === 0) return;
    const names = new Set(found.map((m) => assetName(m.assetId)));
    unavailable.value = unavailable.value.filter((n) => !names.has(n));
    controller?.forgetMedia(found.map((m) => m.assetId));
  },
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

/** D15: select the topmost visible clip under a primary press, or clear
 * the selection (and any selected cue) on empty stage. */
function onPointerDown(event: PointerEvent): void {
  const el = stageRef.value;
  const project = editorProject.project;
  if (event.button !== 0 || !el || !project) return;
  const point = pointerToCanvas(event, el.getBoundingClientRect(), canvas.value, zoom.value);
  const hit = clipAtPoint(project, currentMs.value, point);
  workspace.select(hit ? [hit] : []);
  workspace.setSelected(null);
}
</script>

<template>
  <div
    data-testid="preview-surface"
    class="flex min-h-0 grow flex-col"
  >
    <div
      class="min-h-0 w-full grow basis-0 bg-stage"
      :class="workspace.shortWindow ? 'px-3.5 py-2.5' : 'px-5 py-3.5'"
    >
      <div class="relative h-full w-full">
        <div
          ref="stageRef"
          data-testid="preview-stage"
          class="absolute inset-0 overflow-hidden"
          @pointerdown="onPointerDown"
        >
          <div
            data-testid="preview-canvas-frame"
            class="absolute rounded-[6px] bg-canvas shadow-[var(--editor-canvas-shadow)]"
            :style="{
              left: `${frame.left}px`,
              top: `${frame.top}px`,
              width: `${frame.width}px`,
              height: `${frame.height}px`,
            }"
          />
          <div
            class="absolute inset-0"
            :style="zoomClip"
          >
            <div
              ref="layerHostRef"
              data-testid="preview-layers"
              class="absolute inset-0"
              :style="layersStyle"
            />
          </div>
          <CueOverlay
            :project="editorProject.project"
            :time-ms="currentMs"
            :frame="frame"
            :zoom="zoom"
            :draft="cueDraft"
          />
          <CaptionOverlay
            :project="editorProject.project"
            :time-ms="currentMs"
            :frame="frame"
          />
        </div>
        <LayoutHandles
          :frame="frame"
          :canvas="canvas"
          :zoom="zoom"
          @preview="onLayoutPreview"
        />
        <CueHandles
          :frame="frame"
          :canvas="canvas"
          :time-ms="currentMs"
          :zoom="zoom"
          @preview="cueDraft = $event"
        />
      </div>
    </div>
    <p
      v-if="unavailable.length > 0"
      data-testid="preview-unavailable"
      role="status"
      class="px-3 text-micro text-danger-fg"
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
