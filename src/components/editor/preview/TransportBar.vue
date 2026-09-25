<script setup lang="ts">
/**
 * The preview's transport row (Task 22; F-04, F-14, F-25): play/pause
 * (also Space), current/total time, monitoring volume + mute, playback rate.
 *
 * Presentational for the clock — `playing`/`currentMs`/`durationMs` come in
 * as props from `PreviewSurface`, which owns the non-reactive
 * `PreviewController` — and store-backed for the two MONITORING fields the
 * workspace persists (`monitor_muted`, `playback_rate`). Both are
 * `editorWorkspace` view state: muting the preview or slowing it down is
 * not an edit, so nothing here ever reaches `editorProject.execute`.
 *
 * The audio mixer (Task 27, `MixerPopover`) opens from this row too, beside
 * the speaker, and receives the preview's `readPeak` for its peak meter.
 *
 * The monitoring VOLUME is a prop/`update:volume` pair, not a workspace
 * field: R16's sanitized `workspace.json` has no volume field, so it lives
 * for the window's lifetime only rather than being quietly dropped on save.
 *
 * **Space** is bound on `window` (the preview has no single element that
 * would reliably hold focus), and deliberately yields wherever Space
 * already means something: text entry (`shouldHandle`), any control whose
 * own Space is its activation (a focused button would otherwise toggle
 * twice — its native click plus this), an open menu/dialog, and a
 * keystroke a focused timeline clip already claimed (`defaultPrevented`).
 */
import { onBeforeUnmount, onMounted } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { shouldHandle } from "../../../editor/shortcuts";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";
import MixerPopover from "../shell/MixerPopover.vue";

defineProps<{
  playing: boolean;
  currentMs: number;
  durationMs: number;
  /** Monitoring volume, 0..1. */
  volume: number;
  /** The preview's sample peak, for the mixer's meter. */
  readPeak?: () => number | null;
}>();
const emit = defineEmits<{
  (e: "toggle-play"): void;
  (e: "update:volume", value: number): void;
}>();

/** The workspace clamps to 0.25..2 (`editorWorkspace`'s own range). */
const RATES = [0.25, 0.5, 1, 1.5, 2] as const;

const workspace = useEditorWorkspaceStore();
/** The guide's `transport` (Task 55). */
const transportTarget = useGuideTarget("transport");

/** Elements whose own Space is their activation or their text. */
const OWNS_SPACE =
  'button, select, a, video, audio, [role="button"], [role="option"], [role="menu"], ' +
  '[role="menuitem"], [role="dialog"], [role="slider"], [role="checkbox"], [role="switch"], [role="tab"]';

function spaceBelongsElsewhere(event: KeyboardEvent): boolean {
  const target = event.target;
  return target instanceof Element && target.closest(OWNS_SPACE) !== null;
}

function onWindowKeydown(event: KeyboardEvent) {
  if (event.key !== " " || event.defaultPrevented || event.ctrlKey || event.altKey || event.metaKey) return;
  if (!shouldHandle(event) || spaceBelongsElsewhere(event)) return;
  event.preventDefault();
  emit("toggle-play");
}
onMounted(() => window.addEventListener("keydown", onWindowKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onWindowKeydown));

function onRate(event: Event) {
  workspace.setPlaybackRate(Number((event.target as HTMLSelectElement).value));
}
function onVolume(event: Event) {
  emit("update:volume", Number((event.target as HTMLInputElement).value));
}
</script>

<template>
  <div
    :ref="transportTarget"
    data-testid="transport-bar"
    class="flex flex-wrap items-center gap-2 text-micro text-fg-muted"
  >
    <button
      type="button"
      data-testid="transport-play"
      class="rounded-control border border-line bg-raised px-2 py-0.5 text-fg hover:bg-panel focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :aria-label="playing ? 'Pause' : 'Play'"
      :title="playing ? 'Pause (Space)' : 'Play (Space)'"
      @click="emit('toggle-play')"
    >
      {{ playing ? "❚❚" : "▶" }}
    </button>
    <span class="tabular-nums">
      <span data-testid="transport-current">{{ formatDuration(currentMs) }}</span>
      /
      <span data-testid="transport-total">{{ formatDuration(durationMs) }}</span>
    </span>
    <span class="ml-auto flex items-center gap-2">
      <MixerPopover :read-peak="readPeak" />
      <button
        type="button"
        data-testid="transport-mute"
        class="rounded-control border border-line px-2 py-0.5 hover:bg-panel focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :class="workspace.monitorMuted ? 'bg-raised text-fg' : 'text-fg-muted'"
        :aria-pressed="workspace.monitorMuted ? 'true' : 'false'"
        aria-label="Mute preview monitoring"
        title="Mutes what you hear while previewing; the project's own mix is unchanged"
        @click="workspace.toggleMonitorMute()"
      >
        {{ workspace.monitorMuted ? "Muted" : "Sound" }}
      </button>
      <input
        data-testid="transport-volume"
        type="range"
        min="0"
        max="1"
        step="0.05"
        class="w-20 accent-violet-500"
        aria-label="Preview monitoring volume"
        :value="volume"
        @input="onVolume"
      >
      <select
        data-testid="transport-rate"
        class="rounded-control border border-line bg-raised px-1 py-0.5 text-fg"
        aria-label="Playback rate"
        :value="String(workspace.playbackRate)"
        @change="onRate"
      >
        <option
          v-for="r in RATES"
          :key="r"
          :value="String(r)"
        >
          {{ r }}×
        </option>
      </select>
    </span>
  </div>
</template>
