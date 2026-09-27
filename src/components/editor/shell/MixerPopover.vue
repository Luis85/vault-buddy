<script setup lang="ts">
/**
 * The audio mixer (Task 27; F-05, F-25; onboarding step 14's
 * `[data-action="mixer"]` target) — per-track volume/mute/solo, the master
 * gain, a monitoring-only mute, and the preview's sample-peak meter, in a
 * popover opened from the timeline footer's "Audio mixer".
 *
 * **Two kinds of control, deliberately worded apart** (USER-GUIDE.md: "The
 * speaker beside Play mutes your monitoring only; clip and track mute
 * affect the video output"): every track control and the master gain is a
 * RENDER-AFFECTING edit (`setTrackFlags` / `setMasterGain`, one command
 * each), while "Mute preview (does not affect the video)" is the
 * workspace's `monitor_muted` — the SAME flag the transport's speaker
 * toggles — and never reaches `editorProject.execute`.
 *
 * **Solo** reads `mixRules.isTrackAudible`, the one copy of Rust's
 * documented rule (`commands::tracks`) the preview's monitoring also uses,
 * so "Silenced by solo" here is exactly the set the preview silences.
 *
 * **Sliders commit once** (`MixerSlider`): dragging only moves the
 * readout; the release sends one command, and a refused value snaps back.
 *
 * **The meter is a PEAK** (`MixerPeakMeter`): the preview controller's
 * sample peak, polled only while the popover is open.
 *
 * Split into `MixerTrackRow`/`MixerSlider`/`MixerPeakMeter` so no one
 * template carries every branch (the fallow template-complexity ratchet).
 *
 * **The trigger is the timeline footer's "Audio mixer"** (visual-parity
 * Task 20, concept spec §7, ruling T12-1): the sliders icon, the label and
 * a pill counting the live project's audio tracks — the guide's "audio"
 * lesson target (`[data-action="mixer"]`, `useGuideTarget("mixer")`). The
 * transport row carried an icon-only trigger until then. The panel opens
 * ABOVE the button and is `position: fixed`, placed from the button's box
 * when it opens: the timeline section clips its overflow, so an absolutely
 * placed panel would be cut off at the timeline's top edge. Its height is
 * held to the room above the button, and it scrolls past that.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useWindowDismiss } from "../../../composables/useWindowDismiss";
import { onReveal } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import MixerPeakMeter from "./MixerPeakMeter.vue";
import MixerSlider from "./MixerSlider.vue";
import MixerTrackRow from "./MixerTrackRow.vue";

defineProps<{
  /** The preview's current sample peak (linear), or `null` when nothing
   * is measured. Absent: no meter at all. */
  readPeak?: () => number | null;
}>();

/** Mirrors Rust's master gain range `[0,1]`. */
const MASTER_MAX = 1;

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const open = ref(false);
/** Where the fixed panel sits: above the trigger, right-aligned with it. */
const panelStyle = ref<Record<string, string>>({});
const root = ref<HTMLElement | null>(null);
const triggerRef = ref<HTMLButtonElement | null>(null);
/** The guide's `mixer` (Task 55) — the same button. */
const mixerTarget = useGuideTarget("mixer");
function bindTrigger(el: Element | ComponentPublicInstance | null): void {
  triggerRef.value = el as HTMLButtonElement | null;
  mixerTarget(el);
}
const tracks = computed(() => editorProject.project?.tracks ?? []);
/** The concept's pill (`#audioTrackCount`): the live project's audio tracks. */
const audioTrackCount = computed(() => tracks.value.filter((t) => t.kind === "audio").length);
const triggerLabel = computed(() => {
  const n = audioTrackCount.value;
  return `Audio mixer, ${n} audio ${n === 1 ? "track" : "tracks"}`;
});
const masterGain = computed(() => editorProject.project?.master_gain ?? 1);

function commitMaster(gain: number): Promise<boolean> {
  return editorProject.execute({ kind: "setMasterGain", gain });
}

/** The gap between the trigger and the panel, and the least room kept
 * between the panel and the window's top edge. */
const PANEL_GAP_PX = 4;
const VIEWPORT_MARGIN_PX = 8;

function place(): void {
  const rect = triggerRef.value?.getBoundingClientRect();
  if (!rect) return;
  panelStyle.value = {
    right: `${Math.max(VIEWPORT_MARGIN_PX, window.innerWidth - rect.right)}px`,
    bottom: `${window.innerHeight - rect.top + PANEL_GAP_PX}px`,
    maxHeight: `${Math.max(0, rect.top - PANEL_GAP_PX - VIEWPORT_MARGIN_PX)}px`,
  };
}

function show(): void {
  place();
  open.value = true;
}

function toggle(): void {
  if (open.value) open.value = false;
  else show();
}

function close(): void {
  open.value = false;
  triggerRef.value?.focus();
}

/** Task 54: a muted-or-clipping finding's "Open the mixer". */
onReveal("mixer", show);

/** F-M8: closing while open is not scoped to focus being inside the
 * popover — a click anywhere else on the editor (`root` wraps the trigger
 * AND the popover, so re-clicking the trigger itself is never "outside")
 * or an Escape pressed with focus on the timeline must both close it, the
 * precedent of the track header's former ⋮ menu. `stopImmediatePropagation` (not
 * only `preventDefault`) is what actually keeps a keystroke the mixer
 * already spent from ALSO reaching a `window`-level listener registered
 * after this one — the same reason `ContextMenu.vue`'s own Escape handler
 * stops the event outright rather than trusting `defaultPrevented` alone;
 * `isGuideDismissKey` (`shortcuts.ts`) already refuses to pause the guide
 * while this popover's `role="dialog"` is still in the DOM, but the guard
 * here means the guide never even has to make that call for this key. */
function onWindowPointerDown(event: PointerEvent): void {
  if (!open.value) return;
  if (root.value && !root.value.contains(event.target as Node)) close();
}
function onWindowKeydown(event: KeyboardEvent): void {
  if (!open.value || event.key !== "Escape") return;
  event.preventDefault();
  event.stopImmediatePropagation();
  close();
}
useWindowDismiss(onWindowPointerDown, onWindowKeydown);
</script>

<template>
  <span
    ref="root"
    class="relative"
  >
    <button
      :ref="bindTrigger"
      type="button"
      data-action="mixer"
      data-testid="mixer-toggle"
      class="inline-flex min-h-[25px] shrink-0 items-center gap-[7px] rounded-control border border-transparent px-[5px] py-[3px] text-[10px] text-fg-muted hover:bg-hover hover:text-fg focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :aria-expanded="open ? 'true' : 'false'"
      aria-controls="editor-mixer"
      :aria-label="triggerLabel"
      title="Audio mixer: track levels, mute and solo"
      @click="toggle"
    >
      <EditorIcon name="sliders" />
      Audio mixer
      <span
        data-testid="mixer-toggle-count"
        class="rounded border border-line bg-raised px-1.5 py-0.5 text-[9px] leading-none tracking-[0.3px] text-fg-secondary"
      >{{ audioTrackCount }}</span>
    </button>
    <div
      v-if="open"
      id="editor-mixer"
      role="dialog"
      aria-label="Audio mixer"
      data-testid="mixer-popover"
      class="fixed z-30 flex w-72 max-w-[calc(100vw-16px)] flex-col gap-2 overflow-y-auto rounded-control border border-line bg-panel p-2 text-micro text-fg-muted shadow-lg"
      :style="panelStyle"
    >
      <p class="text-fg-subtle">
        Track and master levels change the rendered video.
      </p>
      <MixerTrackRow
        v-for="track in tracks"
        :key="track.id"
        :track="track"
        :tracks="tracks"
      />
      <div class="border-t border-line pt-2">
        <MixerSlider
          label="Master"
          slider-label="Master gain"
          testid="mixer-master"
          :value="masterGain"
          :max="MASTER_MAX"
          :commit="commitMaster"
        />
      </div>
      <label class="flex items-center gap-1 border-t border-line pt-2">
        <input
          data-testid="mixer-monitor-mute"
          type="checkbox"
          class="accent-violet-500"
          :checked="workspace.monitorMuted"
          @change="workspace.toggleMonitorMute()"
        >
        Mute preview (does not affect the video)
      </label>
      <MixerPeakMeter
        v-if="readPeak"
        :read-peak="readPeak"
      />
      <button
        type="button"
        data-testid="mixer-close"
        class="self-end rounded px-1.5 py-0.5 hover:bg-hover"
        @click="close"
      >
        Close
      </button>
    </div>
  </span>
</template>
