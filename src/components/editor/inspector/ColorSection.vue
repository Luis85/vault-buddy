<script setup lang="ts">
/**
 * The Inspector's Color category (Task 32; F-39; visual-parity Task 15,
 * concept spec §5 "Color"). "A consistent look": the five treatments as
 * picture tiles (`ColorTreatmentGrid`, the one preset set the clip menus
 * and the multi inspector share, ruling T5-3); "Fine adjustments":
 * Brightness, Contrast and Saturation as percent sliders; then Reset color.
 * Each is one `setAdjustments` over the WHOLE selection (`clipIds`) — the
 * Adjust all state hands it several clips — and every slider sends the FULL
 * five-field object (Rust rejects a partial one), moving only its own field.
 * The sliders show the first selected clip's values.
 *
 * Colour applies to footage only — never a title card and never an audio
 * clip. Either one in the selection gets a note instead of controls,
 * worded as Rust's own refusal (`visualTargets.ts`, the clip menus' check).
 * A locked track disables everything; the inspector's frame says why and
 * the fieldset carries the reason.
 */
import { computed } from "vue";

import { useSelectedClips } from "../../../composables/useSelectedClips";
import { COLOR_TREATMENTS, isTreatment, ORIGINAL_ADJUSTMENTS } from "../../../editor/colorPresets";
import { colorRefusal } from "../../../editor/visualTargets";
import type { Adjustments } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import ColorTreatmentGrid from "./ColorTreatmentGrid.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorRange from "./InspectorRange.vue";
import InspectorSection from "./InspectorSection.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const { clips, lockReason } = useSelectedClips(() => props.clipIds);

/** Why this selection cannot be colour-adjusted, or `null` when it can. */
const blockedReason = computed(() =>
  colorRefusal(
    editorProject.project,
    clips.value.map((c) => c.id),
  ),
);

const current = computed<Adjustments>(() => clips.value[0]?.adjustments ?? ORIGINAL_ADJUSTMENTS);
/** The tile every selected clip wears, or none (custom values, a mix). */
const activeId = computed(
  () => COLOR_TREATMENTS.find((t) => clips.value.every((c) => isTreatment(c.adjustments, t.adjustments)))?.id ?? null,
);
const resetReason = computed(() =>
  clips.value.every((c) => !c.adjustments) ? "The colour is already the original." : null,
);

/** The concept's three sliders, in percent of the stored factor. */
const SLIDERS: { key: keyof Adjustments; label: string; min: number }[] = [
  { key: "brightness", label: "Brightness", min: 25 },
  { key: "contrast", label: "Contrast", min: 25 },
  { key: "saturation", label: "Saturation", min: 0 },
];

function send(adjustments: Adjustments | null): Promise<boolean> {
  if (lockReason.value || clips.value.length === 0) return Promise.resolve(false);
  return editorProject.execute({ kind: "setAdjustments", clipIds: clips.value.map((c) => c.id), adjustments });
}

function slide(key: keyof Adjustments, percent: number): Promise<boolean> {
  return send({ ...current.value, [key]: percent / 100 });
}
</script>

<template>
  <fieldset
    v-if="blockedReason === null"
    data-testid="color-section"
    :disabled="lockReason !== null"
    :title="lockReason ?? undefined"
    class="flex min-w-0 flex-col"
  >
    <InspectorSection title="A consistent look">
      <ColorTreatmentGrid
        prefix="color-preset"
        :active-id="activeId"
        :reason="lockReason"
        @pick="(t) => send(t.adjustments)"
      />
    </InspectorSection>
    <InspectorSection title="Fine adjustments">
      <InspectorRange
        v-for="s in SLIDERS"
        :key="s.key"
        :label="s.label"
        :testid="`color-section-${s.key}`"
        :value="Math.round(current[s.key] * 100)"
        :min="s.min"
        :max="200"
        :step="1"
        suffix="%"
        :commit="(v) => slide(s.key, v)"
      />
      <InspectorButton
        class="self-start"
        icon="undo"
        data-testid="color-section-reset"
        :reason="lockReason ?? resetReason"
        @click="send(null)"
      >
        Reset color
      </InspectorButton>
      <p class="text-[10px] leading-[1.6] text-fg-muted">
        Treats the footage, not the teaching cues or captions. It is included in the rendered video, but is not a
        color-managed mastering process.
      </p>
    </InspectorSection>
  </fieldset>
  <p
    v-else
    data-testid="color-section-note"
  >
    {{ blockedReason }}
  </p>
</template>
