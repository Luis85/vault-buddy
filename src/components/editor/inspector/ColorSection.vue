<script setup lang="ts">
/**
 * The Inspector's Color category (Task 32; F-39; SCREENS-AND-INTERACTIONS.md
 * §04). Six presets and five numeric sliders (brightness/contrast/
 * saturation/sepia/grayscale), each one `setAdjustments` over the WHOLE
 * selection (`clipIds`) — the `setLayout` whole-selection/atomic precedent:
 * a multi-selection gets identical values in one command, and Rust applies
 * them atomically or not at all. The fields show the first selected clip's
 * values; `InspectorPanel` already states the scope.
 *
 * Colour applies to footage only — never a title card (its asset's
 * `builtin` kind is `card`, `previewLayers.ts`'s own `BUILTIN_HAS_FILE`
 * identification, task brief) and never an audio clip (nothing to colour).
 * Either one in the selection gets a note instead of controls, worded to
 * match Rust's own refusal so the two surfaces can never disagree
 * (`core::editor::commands::layout::check_color_targets`).
 */
import { computed } from "vue";

import { numberField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import type { ColorPresetId } from "../../../editor/colorPresets";
import { COLOR_PRESETS, findColorPreset } from "../../../editor/colorPresets";
import { colorRefusal } from "../../../editor/visualTargets";
import type { Adjustments } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import InspectorNumberInput from "./InspectorNumberInput.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const { clips, lockReason } = useSelectedClips(() => props.clipIds);

/** Why this selection cannot be colour-adjusted, or `null` when it can —
 * `check_color_targets`'s own two refusals, in its order
 * (`visualTargets.ts`, which the clip menus' Color treatment shares). */
const blockedReason = computed(() =>
  colorRefusal(
    editorProject.project,
    clips.value.map((c) => c.id),
  ),
);
const ready = computed(() => blockedReason.value === null);

const DEFAULT_ADJUSTMENTS: Adjustments = { brightness: 1, contrast: 1, saturation: 1, sepia: 0, grayscale: 0 };
const first = computed(() => clips.value[0]);
const current = computed<Adjustments>(() => first.value?.adjustments ?? DEFAULT_ADJUSTMENTS);

function send(adjustments: Adjustments | null): Promise<boolean> {
  if (lockReason.value || clips.value.length === 0) return Promise.resolve(false);
  return editorProject.execute({
    kind: "setAdjustments",
    clipIds: clips.value.map((c) => c.id),
    adjustments,
  });
}

/** Every numeric field sends the FULL five-field object (Rust rejects a
 * partial one once it is present at all) — only the named field moves. */
function patch(partial: Partial<Adjustments>): Promise<boolean> {
  return send({ ...current.value, ...partial });
}

const fields = {
  brightness: useInspectorDraft(
    numberField({ value: () => current.value.brightness, label: "Brightness", min: 0.25, max: 2, rangeLabel: "0.25× and 2×" }),
    (brightness) => patch({ brightness }),
  ),
  contrast: useInspectorDraft(
    numberField({ value: () => current.value.contrast, label: "Contrast", min: 0.25, max: 2, rangeLabel: "0.25× and 2×" }),
    (contrast) => patch({ contrast }),
  ),
  saturation: useInspectorDraft(
    numberField({ value: () => current.value.saturation, label: "Saturation", min: 0, max: 2, rangeLabel: "0 and 2×" }),
    (saturation) => patch({ saturation }),
  ),
  sepia: useInspectorDraft(
    numberField({ value: () => current.value.sepia, label: "Sepia", min: 0, max: 1, rangeLabel: "0 and 1" }),
    (sepia) => patch({ sepia }),
  ),
  grayscale: useInspectorDraft(
    numberField({ value: () => current.value.grayscale, label: "Grayscale", min: 0, max: 1, rangeLabel: "0 and 1" }),
    (grayscale) => patch({ grayscale }),
  ),
};

/** Fields matching an exact preset's values light that preset's button;
 * `none` lights whenever nothing is set. Anything else (a manual drag, or
 * a mixed multi-selection) lights none of them — never a false positive. */
function adjustmentsEqual(a: Adjustments, b: Adjustments): boolean {
  return a.brightness === b.brightness && a.contrast === b.contrast && a.saturation === b.saturation
    && a.sepia === b.sepia && a.grayscale === b.grayscale;
}
const activePreset = computed<ColorPresetId | null>(() => {
  const live = first.value?.adjustments;
  if (!live) return "none";
  const match = COLOR_PRESETS.find((p) => p.adjustments && adjustmentsEqual(p.adjustments, live));
  return match?.id ?? null;
});

function onPreset(id: ColorPresetId): void {
  void send(findColorPreset(id).adjustments);
}
</script>

<template>
  <div
    v-if="ready"
    data-testid="color-section"
    class="flex flex-col gap-2"
  >
    <fieldset
      data-testid="color-section-fields"
      :disabled="lockReason !== null"
      :title="lockReason ?? undefined"
      class="flex flex-col gap-2 disabled:opacity-50"
    >
      <div
        class="flex flex-wrap gap-1"
        role="group"
        aria-label="Colour presets"
      >
        <button
          v-for="preset in COLOR_PRESETS"
          :key="preset.id"
          type="button"
          :data-testid="`color-preset-${preset.id}`"
          :aria-pressed="preset.id === activePreset"
          :title="preset.label"
          class="cursor-pointer rounded border border-line px-1.5 py-0.5 hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          :class="preset.id === activePreset ? 'bg-accent/20 text-accent-fg' : 'text-fg'"
          @click="onPreset(preset.id)"
        >
          {{ preset.label }}
        </button>
      </div>
      <InspectorNumberInput
        :field="fields.brightness"
        label="Brightness (×)"
        testid="color-section-brightness"
      />
      <InspectorNumberInput
        :field="fields.contrast"
        label="Contrast (×)"
        testid="color-section-contrast"
      />
      <InspectorNumberInput
        :field="fields.saturation"
        label="Saturation (×)"
        testid="color-section-saturation"
      />
      <InspectorNumberInput
        :field="fields.sepia"
        label="Sepia"
        testid="color-section-sepia"
      />
      <InspectorNumberInput
        :field="fields.grayscale"
        label="Grayscale"
        testid="color-section-grayscale"
      />
    </fieldset>
  </div>
  <p
    v-else
    data-testid="color-section-note"
  >
    {{ blockedReason }}
  </p>
</template>
