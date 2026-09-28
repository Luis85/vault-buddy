<script setup lang="ts">
/**
 * The preview's transport row (Task 22; F-04, F-14, F-25), restyled to the
 * concept by visual-parity Task 12 (concept spec §4.3): 46px on `--bg`
 * under a `line` border (40px in a window 760px tall or less,
 * `editorWorkspace.shortWindow`).
 *
 * - **Left**: the monitor mute icon button, a 48×8 sample-peak meter and a
 *   borderless playback-rate select. The concept's own list is 0.5×/1×/
 *   1.5×/2×; fix round 1 (finding 2) restores 0.25× ahead of it, because
 *   `editorWorkspace.playbackRate` (`PLAYBACK_RATE_RANGE`, the store's own
 *   file) still clamps to `[0.25, 2.0]` — a workspace saved before this
 *   restyle, or restored from a portable project file, can carry a stored
 *   rate of 0.25, and a `<select>` with no matching `<option>` shows
 *   nothing selected rather than the rate that is actually playing (D14:
 *   the control must never silently misrepresent live state). "Audio
 *   mixer" and "Sound" leave this row (D10): the mixer opens from the
 *   timeline footer's "Audio mixer" (visual-parity Task 20, ruling T12-1,
 *   the guide's "audio" target with it), from View ⋯ → Audio mixer… and
 *   from the Audio tab.
 * - **Centre**: Go to start / Go to end (both move `editorWorkspace`'s
 *   playhead directly, exactly how a timeline click seeks), the 34px round
 *   Play/Pause (also Space), and the mono timecode `MM:SS.d / MM:SS.d`
 *   (`formatTimecode`, the concept's own `fmt(ms, true)` — current in ink,
 *   "/ total" muted).
 * - **Right**: the D10 canvas badge, "{W} × {H} · {fps} fps · PREVIEW" from
 *   the project's own canvas (replacing the concept's browser-only "SAMPLE
 *   PROJECT · LOCAL PREVIEW" wording) — hidden below the concept's 620px
 *   break (`TRANSPORT_BADGE_MIN_WIDTH`).
 *
 * Presentational for the clock — `playing`/`currentMs`/`durationMs` come in
 * as props from `PreviewSurface`, which owns the non-reactive
 * `PreviewController` — and store-backed for the workspace fields this row
 * touches (`monitor_muted`, `playback_rate`, `playhead_ms`). None of it is
 * an edit: muting the preview, changing its rate or moving the playhead is
 * `editorWorkspace` view state, never `editorProject.execute`.
 *
 * The concept's own transport carries no monitoring-VOLUME control, only
 * the mute toggle — this app's earlier volume slider (Task 22) left with
 * this restyle; monitoring now plays at full volume except when muted
 * (`PreviewSurface.vue`'s own doc says why).
 *
 * The peak meter samples `readPeak` — the preview controller's sample
 * peak — on a fixed interval (`usePolledValue`, the same poll
 * `MixerPeakMeter.vue` uses), never at frame rate, and reads silent
 * whenever `playing` is false, mirroring the concept's own
 * `if (!ui.playing) peakMeter.style.width = '0%'`.
 *
 * **Space** is bound on `window` (the preview has no single element that
 * would reliably hold focus), and deliberately yields wherever Space
 * already means something: text entry (`shouldHandle`), any control whose
 * own Space is its activation (a focused button would otherwise toggle
 * twice — its native click plus this), an open menu/dialog, and a
 * keystroke a focused timeline clip already claimed (`defaultPrevented`).
 */
import { computed, onBeforeUnmount, onMounted } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { usePolledValue } from "../../../composables/usePolledValue";
import { announceDisabled } from "../../../editor/disabledAnnouncer";
import { TRANSPORT_BADGE_MIN_WIDTH } from "../../../editor/panelLayout";
import { shouldHandle } from "../../../editor/shortcuts";
import type { Canvas } from "../../../editorTypes";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import type { EditorIconName } from "../icons/conceptIcons";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{
  playing: boolean;
  currentMs: number;
  durationMs: number;
  /** The project's canvas, for the D10 badge; `null` before a project is
   * known (never rendered as a false "0 × 0"). */
  canvas: Canvas | null;
  /** The preview's sample peak, for this row's meter. */
  readPeak?: () => number | null;
}>();
const emit = defineEmits<{
  (e: "toggle-play"): void;
}>();

/** §4.3's own list (0.5×/1×/1.5×/2×) plus 0.25× (fix round 1, finding 2):
 * the workspace's stored rate still clamps to `PLAYBACK_RATE_RANGE`
 * (`editorWorkspace.ts`), `[0.25, 2.0]`, so the select needs an option for
 * every value that range actually allows. */
const RATES = [0.25, 0.5, 1, 1.5, 2] as const;
/** The concept's dB floor for the meter's fill (`(db+60)/60`) and its
 * "hot" threshold (`peak-hot`, `session-safety.js`'s `value >= .98`). */
const METER_DB_FLOOR = -60;
const METER_HOT_THRESHOLD = 0.98;
const PEAK_POLL_MS = 100;

const workspace = useEditorWorkspaceStore();
/** The guide's `transport` (Task 55). */
const transportTarget = useGuideTarget("transport");

// ---- Space toggles Play/Pause -----------------------------------------------

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

// ---- centre: seek, play, timecode -------------------------------------------

/** At either end the matching button would move nothing, so it says so
 * (design D14) — `aria-disabled`, not `disabled`, so a keyboard user who
 * just pressed it keeps focus on it. */
const START_REASON = "Already at the start.";
const END_REASON = "Already at the end.";
const atStart = computed(() => props.currentMs <= 0);
const atEnd = computed(() => props.currentMs >= props.durationMs);
function goToStart(): void {
  if (atStart.value) announceDisabled(START_REASON);
  else workspace.setPlayhead(0);
}
function goToEnd(): void {
  if (atEnd.value) announceDisabled(END_REASON);
  else workspace.setPlayhead(props.durationMs);
}

/** The concept's own `fmt(ms, decimal=true)`: `MM:SS.d`, both fields
 * zero-padded, one decisecond. */
function formatTimecode(ms: number): string {
  const clamped = Math.max(0, ms);
  const totalSeconds = Math.floor(clamped / 1000);
  const minutes = String(Math.floor(totalSeconds / 60)).padStart(2, "0");
  const seconds = String(totalSeconds % 60).padStart(2, "0");
  const tenths = Math.floor((clamped % 1000) / 100);
  return `${minutes}:${seconds}.${tenths}`;
}
const timecodeCurrent = computed(() => formatTimecode(props.currentMs));
const timecodeTotal = computed(() => formatTimecode(props.durationMs));

// ---- left: mute, peak meter, rate --------------------------------------------

const monitorLabel = computed(() => (workspace.monitorMuted ? "Unmute monitoring" : "Mute monitoring"));
const monitorIcon = computed(() => (workspace.monitorMuted ? "muted" : "volume"));

function onRate(event: Event) {
  workspace.setPlaybackRate(Number((event.target as HTMLSelectElement).value));
}

const peak = usePolledValue(() => (props.readPeak ? props.readPeak() : null), PEAK_POLL_MS);
/** Silent whenever nothing is playing — the concept's own rule, so a
 * leftover sample from before a pause never lingers on the bar. */
const peakFraction = computed(() => {
  if (!props.playing) return 0;
  const value = peak.value;
  if (value === null || value <= 0) return 0;
  const db = 20 * Math.log10(value);
  return Math.min(1, Math.max(0, (db - METER_DB_FLOOR) / -METER_DB_FLOOR));
});
const peakFillClass = computed(() => {
  const hot = props.playing && (peak.value ?? 0) >= METER_HOT_THRESHOLD;
  return hot ? "bg-danger" : "bg-audio";
});

// ---- centre: Play/Pause, kept as one computed so the template carries no
// per-field ternary of its own (fallow's template-complexity ratchet). ----

const playState = computed<{ icon: EditorIconName; label: string; title: string }>(() => ({
  icon: props.playing ? "pause" : "play",
  label: props.playing ? "Pause" : "Play",
  title: props.playing ? "Pause (Space)" : "Play (Space)",
}));

// ---- right: the D10 canvas badge --------------------------------------------

const showBadge = computed(() => workspace.viewportWidth > TRANSPORT_BADGE_MIN_WIDTH);
const badgeText = computed(() => {
  const c = props.canvas;
  return c ? `${c.width} × ${c.height} · ${c.fps} fps · PREVIEW` : "PREVIEW";
});

const rowHeightClass = computed(() => (workspace.shortWindow ? "h-10" : "h-[46px]"));
</script>

<template>
  <div
    :ref="transportTarget"
    data-testid="transport-bar"
    class="flex shrink-0 items-center justify-between gap-2 overflow-hidden border-t border-line bg-app px-[15px] text-micro text-fg-muted"
    :class="rowHeightClass"
  >
    <div class="flex shrink-0 items-center gap-1.5">
      <button
        type="button"
        data-testid="transport-mute"
        class="flex h-8 w-8 shrink-0 items-center justify-center border border-transparent bg-transparent text-fg-muted hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :aria-pressed="workspace.monitorMuted"
        :aria-label="monitorLabel"
        :title="monitorLabel"
        @click="workspace.toggleMonitorMute()"
      >
        <EditorIcon :name="monitorIcon" />
      </button>
      <span
        data-testid="transport-peak"
        role="meter"
        aria-label="Live master sample peak, not loudness"
        aria-valuemin="0"
        aria-valuemax="1"
        :aria-valuenow="peakFraction"
        title="Live master sample peak, not loudness"
        class="inline-flex h-2 w-12 shrink-0 overflow-hidden rounded-[3px] bg-line"
      >
        <span
          class="h-full"
          :class="peakFillClass"
          :style="{ width: `${peakFraction * 100}%` }"
        />
      </span>
      <select
        data-testid="transport-rate"
        class="h-[30px] shrink-0 border-0 bg-transparent px-0.5 text-[11px] text-fg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        aria-label="Preview playback speed"
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
    </div>

    <div class="flex shrink-0 items-center gap-2">
      <button
        type="button"
        data-testid="transport-start"
        class="flex h-8 w-8 shrink-0 items-center justify-center border border-transparent bg-transparent text-fg-muted hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus aria-disabled:cursor-default aria-disabled:opacity-50 aria-disabled:hover:text-fg-muted"
        aria-label="Go to start"
        :aria-disabled="atStart"
        :title="atStart ? START_REASON : 'Go to start (Home)'"
        @click="goToStart"
      >
        <EditorIcon name="skipBack" />
      </button>
      <button
        type="button"
        data-testid="transport-play"
        class="flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-full border border-line bg-accent-bg text-accent-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :aria-label="playState.label"
        :title="playState.title"
        @click="emit('toggle-play')"
      >
        <EditorIcon :name="playState.icon" />
      </button>
      <button
        type="button"
        data-testid="transport-end"
        class="flex h-8 w-8 shrink-0 items-center justify-center border border-transparent bg-transparent text-fg-muted hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus aria-disabled:cursor-default aria-disabled:opacity-50 aria-disabled:hover:text-fg-muted"
        aria-label="Go to end"
        :aria-disabled="atEnd"
        :title="atEnd ? END_REASON : 'Go to end (End)'"
        @click="goToEnd"
      >
        <EditorIcon name="skipForward" />
      </button>
      <span class="vb-mono min-w-[137px] shrink-0 text-[11px]">
        <span
          data-testid="transport-current"
          class="text-fg"
        >{{ timecodeCurrent }}</span>
        <span class="text-fg-muted"> / <span data-testid="transport-total">{{ timecodeTotal }}</span></span>
      </span>
    </div>

    <div
      v-if="showBadge"
      data-testid="transport-badge"
      class="max-w-[160px] shrink-0 truncate text-right text-[9px] uppercase tracking-[0.1px] text-fg-muted"
      title="Preview label only. This label is not included in rendered video."
    >
      {{ badgeText }}
    </div>
  </div>
</template>
