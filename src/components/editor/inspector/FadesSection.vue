<script setup lang="ts">
/**
 * The Inspector's Fades category (Task 29; F-17, F-18; visual-parity Task
 * 15, concept spec §5 "Fades", screen 04). "A softer entrance. A cleaner
 * exit." ("Let the sound arrive naturally." for a sound): the envelope
 * (`FadeGraph`), Fade in / Fade out in seconds, the Curve, None / Quick ·
 * 0.5s / Gentle · 1s, and Preview entrance, which plays the clip from its
 * start; then "Between two clips" (`ClipTransitions`).
 *
 * The fields are the numeric ALTERNATIVE to dragging the clip's own gold
 * handles (`ClipItem.vue`) — "Handle and numeric edits produce equivalent
 * opacity envelopes" is why both send the identical `setFades`, typed in
 * seconds and sent as whole milliseconds (`secondsField`). The presets are
 * the clip menu's own Fades items (`menuSetsClip.ts`: both edges, never
 * past half the clip), run through their own `run` with their own reasons.
 *
 * The half-duration LIMIT is read once at setup (the caller keys this
 * component on the SELECTION): a preview of the refusal, never a substitute
 * for Rust's own — `set_fades` re-checks the clip's CURRENT duration. One
 * clip only: a multi-selection gets a note (R20). A locked track disables
 * every edit — the inspector's frame says why, each control carries the
 * reason — while Preview entrance, which changes nothing, still plays.
 */
import { computed } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { secondsField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import { requestPlaybackFrom } from "../../../editor/revealBus";
import { clipOutputDuration } from "../../../editor/timeMap";
import type { FadeCurve } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import ClipTransitions from "./ClipTransitions.vue";
import FadeGraph from "./FadeGraph.vue";
import FadePresets from "./FadePresets.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorNumberInput from "./InspectorNumberInput.vue";
import InspectorSection from "./InspectorSection.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const { lockReason } = useSelectedClips(() => props.clipIds);
/** The guide's `inspector.fades` (Task 55): the open section wins over its tab. */
const sectionTarget = useGuideTarget("inspector.fades");

const clip = computed(() => (props.clipIds.length === 1 ? editorProject.clipById(props.clipIds[0]) : undefined));
const sound = computed(() => editorProject.project?.assets.find((a) => a.id === clip.value?.asset_id)?.kind === "audio");
/** The concept's heading and help, for a picture or for a sound. */
const copy = computed(() =>
  sound.value
    ? { title: "Let the sound arrive naturally.", help: "Fade volume up from silence, then down again." }
    : {
        title: "A softer entrance. A cleaner exit.",
        help: "Fade to reveal the layer underneath. On the bottom track, fade to black.",
      },
);
const locked = computed(() => lockReason.value !== null);
const durationMs = computed(() => (clip.value ? clipOutputDuration(clip.value.in_ms, clip.value.out_ms, clip.value.speed ?? 1) : 0));

/** Mirrors `core::editor::commands::fades::fade_limit` (`duration / 2`,
 * floored), read once. */
const halfDurationMs = Math.floor(durationMs.value / 2);

function commit(fades: { fadeInMs?: number; fadeOutMs?: number; fadeCurve?: FadeCurve }): Promise<boolean> {
  const c = clip.value;
  if (!c || lockReason.value) return Promise.resolve(false);
  return editorProject.execute({ kind: "setFades", clipId: c.id, ...fades });
}

const drafts = clip.value
  ? {
      fadeIn: useInspectorDraft(
        secondsField({ value: () => clip.value?.fade_in_ms ?? 0, label: "Fade in", maxMs: halfDurationMs }),
        (fadeInMs) => commit({ fadeInMs }),
      ),
      fadeOut: useInspectorDraft(
        secondsField({ value: () => clip.value?.fade_out_ms ?? 0, label: "Fade out", maxMs: halfDurationMs }),
        (fadeOutMs) => commit({ fadeOutMs }),
      ),
    }
  : null;

const FADE_CURVES: { value: FadeCurve; label: string }[] = [
  { value: "linear", label: "Linear" },
  { value: "smooth", label: "Smooth" },
  { value: "equal-power", label: "Equal power (audio)" },
];

function onCurve(event: Event): void {
  const value = (event.target as HTMLSelectElement).value as FadeCurve;
  if (value !== clip.value?.fade_curve) void commit({ fadeCurve: value });
}

function previewEntrance(): void {
  const c = clip.value;
  if (!c) return;
  workspace.setPlayhead(c.start_ms);
  requestPlaybackFrom(c.start_ms);
}
</script>

<template>
  <div
    v-if="drafts && clip"
    :ref="sectionTarget"
    data-testid="fades-section"
    class="flex min-w-0 flex-col"
  >
    <InspectorSection :title="copy.title">
      <FadeGraph
        :fade-in-ms="clip.fade_in_ms"
        :fade-out-ms="clip.fade_out_ms"
        :duration-ms="durationMs"
      />
      <p
        data-testid="fades-section-help"
        class="text-[10px] leading-[1.6] text-fg-muted"
      >
        {{ copy.help }}
      </p>
      <div class="grid grid-cols-2 gap-[9px]">
        <InspectorNumberInput
          :field="drafts.fadeIn"
          label="Fade in (s)"
          testid="fades-section-fade-in"
          :disabled="locked"
          :step="0.1"
          :min="0"
        />
        <InspectorNumberInput
          :field="drafts.fadeOut"
          label="Fade out (s)"
          testid="fades-section-fade-out"
          :disabled="locked"
          :step="0.1"
          :min="0"
        />
      </div>
      <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
        Curve
        <select
          data-testid="fades-section-curve"
          class="text-[11px] text-fg disabled:opacity-50"
          :disabled="locked"
          :title="lockReason ?? undefined"
          :value="clip.fade_curve"
          @change="onCurve"
        >
          <option
            v-for="c in FADE_CURVES"
            :key="c.value"
            :value="c.value"
          >
            {{ c.label }}
          </option>
        </select>
      </label>
      <FadePresets
        :fade-in-ms="clip.fade_in_ms"
        :fade-out-ms="clip.fade_out_ms"
        :half-duration-ms="halfDurationMs"
      />
      <InspectorButton
        class="self-start"
        icon="play"
        data-testid="fades-section-preview"
        @click="previewEntrance"
      >
        Preview entrance
      </InspectorButton>
      <p class="text-[10px] leading-[1.6] text-fg-muted">
        Gold handles on the clip do the same thing. Each fade is limited to half the clip.
      </p>
    </InspectorSection>
    <ClipTransitions
      :clip-id="clip.id"
      :lock-reason="lockReason"
    />
  </div>
  <p
    v-else
    :ref="sectionTarget"
    data-testid="fades-section-multi"
    class="text-fg-subtle"
  >
    Select a single clip to set its fade in, fade out and curve.
  </p>
</template>
