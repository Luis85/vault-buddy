<script setup lang="ts">
/**
 * The Inspector's Fades category (Task 29; F-17, F-18;
 * SCREENS-AND-INTERACTIONS.md §04: "Inspector categories are Clip, Layout,
 * Fades, Audio, Speed and Color … Fades are different from crossfades; show
 * numeric duration/curve and warn about track shortening where applicable").
 * Fills `InspectorPanel.vue`'s `#fades` slot (`EditorRoot.vue` wires it, the
 * `ClipSection`/`AudioSection` seam) with the numeric-entry ALTERNATIVE to
 * dragging the clip's own gold handles (`ClipItem.vue`) — F-17/F-18's own
 * acceptance line, "Handle and numeric edits produce equivalent opacity
 * envelopes", is exactly why both paths send the identical `setFades`
 * command rather than two different ones.
 *
 * `InspectorPanel` only renders this slot once there IS a selection and
 * hands it `clipIds` — but `setFades` takes a single `clipId`, so a
 * multi-selection (`clipIds.length > 1`) renders a short note instead of
 * silently acting on `clipIds[0]` (R20: no faked scope), the
 * `ClipSection`/`AudioSection` precedent.
 *
 * **The half-duration LIMIT is read once at setup**, the `ClipSection`
 * precedent (`assetDurationMs` there): a trim shrinking the clip while this
 * tab stays mounted (no remount — the caller keys this component on the
 * SELECTION, not the revision) can make the client-side bound here stale,
 * but that is a preview clamp, never a substitute for Rust's own refusal —
 * `fades.rs`'s `set_fades` re-checks against the clip's CURRENT duration on
 * every commit regardless.
 *
 * **Transitions** (Task 30, F-19) sit below the edge fades, visibly
 * separate from them — `ClipTransitions`, which owns its own action
 * context and rows.
 */
import { computed } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { numberField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import type { EditorCommand } from "../../../editor/editorCommandTypes";
import { clipOutputDuration } from "../../../editor/timeMap";
import type { FadeCurve } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import ClipTransitions from "./ClipTransitions.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
/** The guide's `inspector.fades` (Task 55): the open section wins over its tab. */
const sectionTarget = useGuideTarget("inspector.fades");

const clip = computed(() => (props.clipIds.length === 1 ? editorProject.clipById(props.clipIds[0]) : undefined));
const single = clip.value !== undefined;

/** Mirrors `core::editor::commands::fades::fade_limit` (`duration / 2`,
 * floored) — read from the Rust source, `useInspectorDraft.ts`'s own rule
 * for every client-side mirror of a server bound. */
const halfDurationMs = clip.value
  ? Math.floor(clipOutputDuration(clip.value.in_ms, clip.value.out_ms, clip.value.speed ?? 1) / 2)
  : 0;

function commitFade(overrides: { fadeInMs?: number; fadeOutMs?: number }): Promise<boolean> {
  const c = clip.value;
  if (!c) return Promise.resolve(false);
  const command: EditorCommand = { kind: "setFades", clipId: c.id, ...overrides };
  return editorProject.execute(command);
}

/** One integer-millisecond field over the live clip's `read`-selected
 * property, both bounded by the same `halfDurationMs` -- the
 * `ClipSection.vue` `msDraft` precedent, narrowed to this section's one
 * shared bound (unlike `ClipSection`'s per-field `max`). */
function msDraft(label: string, read: (fadeInMs: number, fadeOutMs: number) => number, commit: (v: number) => Promise<boolean>) {
  const value = () => (clip.value ? read(clip.value.fade_in_ms, clip.value.fade_out_ms) : 0);
  return useInspectorDraft(
    numberField({ value, label, min: 0, max: halfDurationMs, rangeLabel: `0 and ${halfDurationMs} ms`, integer: true }),
    commit,
  );
}

const drafts = single
  ? {
      fadeIn: msDraft("Fade in", (fadeIn) => fadeIn, (v) => commitFade({ fadeInMs: v })),
      fadeOut: msDraft("Fade out", (_fadeIn, fadeOut) => fadeOut, (v) => commitFade({ fadeOutMs: v })),
    }
  : null;

const FADE_CURVES: { value: FadeCurve; label: string }[] = [
  { value: "linear", label: "Linear" },
  { value: "smooth", label: "Smooth" },
  { value: "equal-power", label: "Equal power" },
];

function onCurveChange(event: Event): void {
  const c = clip.value;
  if (!c) return;
  const value = (event.target as HTMLSelectElement).value as FadeCurve;
  if (value === c.fade_curve) return;
  void editorProject.execute({ kind: "setFades", clipId: c.id, fadeCurve: value });
}
</script>

<template>
  <div
    v-if="drafts && clip"
    :ref="sectionTarget"
    data-testid="fades-section"
    class="flex flex-col gap-2"
  >
    <label class="flex flex-col gap-0.5">
      <span class="text-fg-subtle">Fade in (ms)</span>
      <input
        data-testid="fades-section-fade-in"
        type="text"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
        :value="drafts.fadeIn.draft.value"
        @input="drafts.fadeIn.draft.value = ($event.target as HTMLInputElement).value"
        @keydown.enter="drafts.fadeIn.submit()"
        @keydown.escape="drafts.fadeIn.revert()"
        @blur="drafts.fadeIn.submit()"
      >
      <span
        v-if="drafts.fadeIn.error.value"
        data-testid="fades-section-fade-in-error"
        class="text-danger"
      >{{ drafts.fadeIn.error.value }}</span>
    </label>

    <label class="flex flex-col gap-0.5">
      <span class="text-fg-subtle">Fade out (ms)</span>
      <input
        data-testid="fades-section-fade-out"
        type="text"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
        :value="drafts.fadeOut.draft.value"
        @input="drafts.fadeOut.draft.value = ($event.target as HTMLInputElement).value"
        @keydown.enter="drafts.fadeOut.submit()"
        @keydown.escape="drafts.fadeOut.revert()"
        @blur="drafts.fadeOut.submit()"
      >
      <span
        v-if="drafts.fadeOut.error.value"
        data-testid="fades-section-fade-out-error"
        class="text-danger"
      >{{ drafts.fadeOut.error.value }}</span>
    </label>

    <label class="flex flex-col gap-0.5">
      <span class="text-fg-subtle">Curve</span>
      <select
        data-testid="fades-section-curve"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
        :value="clip.fade_curve"
        @change="onCurveChange"
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

    <ClipTransitions :clip-id="clip.id" />
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
