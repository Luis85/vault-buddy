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
 * held to the room above the button in the window's own terms
 * (`calc(100vh - …)`), it scrolls past that, and it is placed again on
 * every window resize while it is open (fix round 1), so shrinking the
 * window never strands its first rows above the top.
 *
 * **Escape and a revealed mixer.** Escape always gives focus back to this
 * trigger — also when the panel was opened by a reveal (View → Audio
 * mixer…, a before-you-share finding's "Open the mixer", the Audio tab's
 * "Open audio mixer"). By then the opener is gone (the View menu and the
 * Checks dialog have closed) or is not the mixer's home, and the trigger is
 * the control that opens the panel again, so it is where focus belongs.
 *
 * **Visual-parity Task 22** (concept spec §9.8, `shell.html:
 * id="mixerDialog"`): the concept mixer's look inside the same popover —
 * a header with "Audio mixer", its subtitle and a ✕, one `125px 1fr 52px`
 * row per track (`MixerTrackRow`), and "Master output" with its level as a
 * mono percent. Its "Play / pause preview" button is left out: the
 * transport's Play (and Space) already does exactly that, and the popover
 * sits over the timeline, not the preview.
 */
import type { ComponentPublicInstance } from "vue";
import { computed, onBeforeUnmount, ref, watch } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useWindowDismiss } from "../../../composables/useWindowDismiss";
import { onReveal } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import IconButton from "../../ui/IconButton.vue";
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

function percent(gain: number): string {
  return `${Math.round(gain * 100)}%`;
}

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
  const bottom = window.innerHeight - rect.top + PANEL_GAP_PX;
  panelStyle.value = {
    right: `${Math.max(VIEWPORT_MARGIN_PX, window.innerWidth - rect.right)}px`,
    bottom: `${bottom}px`,
    maxHeight: `calc(100vh - ${bottom + VIEWPORT_MARGIN_PX}px)`,
  };
}

/** Placed again whenever the window changes size while the panel is open,
 * and not listened for at all while it is closed. */
watch(open, (isOpen) => {
  if (isOpen) window.addEventListener("resize", place);
  else window.removeEventListener("resize", place);
});
onBeforeUnmount(() => window.removeEventListener("resize", place));

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
      class="fixed z-30 flex w-[420px] max-w-[calc(100vw-16px)] flex-col overflow-hidden rounded-[13px] border border-line bg-panel text-fg shadow-[var(--editor-shadow)]"
      :style="panelStyle"
    >
      <header class="flex shrink-0 items-start justify-between gap-3 border-b border-line px-4 pb-2.5 pt-3">
        <div class="min-w-0">
          <h2 class="text-sm font-semibold leading-[1.4]">
            Audio mixer
          </h2>
          <p class="text-[11px] text-fg-muted">
            Track levels change both the preview and the rendered video.
          </p>
        </div>
        <IconButton
          label="Close audio mixer"
          data-testid="mixer-close"
          class="h-8 w-8 shrink-0"
          @click="close"
        >
          <EditorIcon name="x" />
        </IconButton>
      </header>
      <div class="flex min-h-0 flex-col gap-3 overflow-y-auto px-4 pb-4">
        <div>
          <MixerTrackRow
            v-for="track in tracks"
            :key="track.id"
            :track="track"
            :tracks="tracks"
          />
          <div
            data-testid="mixer-master-row"
            class="grid grid-cols-[125px_1fr_52px] items-center gap-2.5 py-3"
          >
            <b class="text-[11px] font-semibold">Master output</b>
            <MixerSlider
              slider-label="Master output"
              testid="mixer-master"
              readout-testid="mixer-master-readout"
              :value="masterGain"
              :max="MASTER_MAX"
              :format="percent"
              :commit="commitMaster"
            />
          </div>
        </div>
        <label class="flex items-center gap-2 text-[11px] text-fg-secondary">
          <input
            data-testid="mixer-monitor-mute"
            type="checkbox"
            :checked="workspace.monitorMuted"
            @change="workspace.toggleMonitorMute()"
          >
          Mute preview (does not affect the video)
        </label>
        <MixerPeakMeter
          v-if="readPeak"
          :read-peak="readPeak"
        />
        <p class="text-[10px] leading-[1.6] text-fg-muted">
          M = mute. S = solo. Track and master levels are part of the edit; Mute preview changes only what you hear.
        </p>
      </div>
    </div>
  </span>
</template>
