<script setup lang="ts">
/**
 * The selected teaching cue's inspector (Task 35; F-27–F-33;
 * SCREENS-AND-INTERACTIONS.md: "The inspector owns selected-object details").
 * Fills `InspectorPanel`'s `#effect` slot, which it shows in place of the
 * clip categories while a cue is selected (`cueActions.selectedEffectOf`).
 *
 * Every property of the cue's kind is editable (`effectFields.fieldsFor` —
 * the kind's own wire props, with `validate.rs`'s bounds), each through a
 * `useInspectorDraft` buffer: Enter/blur commits ONE `updateEffect`, Escape
 * reverts, an out-of-range entry stays visible with its correction (R20:
 * nothing is silently clamped). Colour and the text background commit on
 * change.
 *
 * **Times.** A cue's `start_ms`/`end_ms` are SOURCE time (Task 34); here they
 * are shown in OUTPUT time — where the user sees the cue on the timeline,
 * `timeMap.cueOutputSpan` at the clip's speed — and converted back with
 * `effectFields.outputToSource` on commit. A span Rust refuses (start not
 * before end) reverts the field to the committed value.
 *
 * **A privacy cover** always shows F-33's limitation notice — permanent,
 * never dismissible: a stationary opaque box is not redaction, and the
 * recording underneath is untouched.
 */
import { computed } from "vue";

import type { InspectorDraft, InspectorField } from "../../../composables/useInspectorDraft";
import { numberField, percentField, textField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import { clipSpanOf } from "../../../editor/actionTargets";
import type { EffectFieldKey, EffectFieldSpec } from "../../../editor/effectFields";
import {
  EFFECT_NAMES,
  fieldsFor,
  MASK_WARNING,
  MAX_EFFECT_TEXT_CHARS,
  outputToSource,
} from "../../../editor/effectFields";
import { clipOutputEnd, cueOutputSpan } from "../../../editor/timeMap";
import type { Clip, Effect } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import InspectorNumberInput from "./InspectorNumberInput.vue";

const props = defineProps<{ effectId: string }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const effect = computed<Effect | null>(
  () => editorProject.project?.effects.find((e) => e.id === props.effectId) ?? null,
);
const clip = computed<Clip | null>(() => (effect.value ? (editorProject.clipById(effect.value.clip_id) ?? null) : null));
const { lockReason } = useSelectedClips(() => (clip.value ? [clip.value.id] : []));

/** A cue's kind never changes, so its field list is fixed per instance
 * (`EditorRoot` keys this section on the effect id). */
const specs: EffectFieldSpec[] = effect.value ? fieldsFor(effect.value.kind) : [];

type Update = { startMs?: number; endMs?: number; props?: Record<string, unknown> };

function send(update: Update): Promise<boolean> {
  if (!effect.value || lockReason.value) return Promise.resolve(false);
  return editorProject.execute({ kind: "updateEffect", effectId: props.effectId, ...update });
}

// ---- start/end, in output time ---------------------------------------------

const span = computed<[number, number]>(() => {
  const [c, e] = [clip.value, effect.value];
  if (!c || !e) return [0, 0];
  return cueOutputSpan(clipSpanOf(c), e.start_ms, e.end_ms) ?? [c.start_ms, c.start_ms];
});

/** An output-time field whose bounds are the clip's CURRENT output span,
 * read at parse time (a clip can move while the draft is open). */
function timeField(label: string, index: 0 | 1): InspectorField<number> {
  return {
    value: () => span.value[index],
    format: (v) => String(v),
    parse(raw) {
      const c = clip.value;
      const [min, max] = c ? [c.start_ms, clipOutputEnd(clipSpanOf(c))] : [0, 0];
      return numberField({ value: () => 0, label, min, max, rangeLabel: `${min} and ${max} ms`, integer: true }).parse(raw);
    },
  };
}
const startField = useInspectorDraft(timeField("Start", 0), (t) =>
  clip.value ? send({ startMs: outputToSource(clip.value, t) }) : false,
);
const endField = useInspectorDraft(timeField("End", 1), (t) =>
  clip.value ? send({ endMs: outputToSource(clip.value, t) }) : false,
);

// ---- the kind's own props ----------------------------------------------------

function numeric(key: EffectFieldKey): number {
  const v = effect.value?.[key as keyof Effect];
  return typeof v === "number" ? v : 0;
}

function fieldFor(spec: EffectFieldSpec): InspectorField<number> | InspectorField<string> | null {
  if (spec.kind === "percent") {
    return percentField({ value: () => numeric(spec.key), label: spec.label, min: () => spec.min, max: () => spec.max });
  }
  if (spec.kind === "number") {
    const { label, min, max, integer } = spec;
    return numberField({ value: () => numeric(spec.key), label, min, max, integer, rangeLabel: `${min} and ${max}` });
  }
  if (spec.kind === "text") {
    return textField({ value: () => effect.value?.text ?? "", label: spec.label, maxLength: MAX_EFFECT_TEXT_CHARS });
  }
  return null;
}

const drafts: Partial<Record<EffectFieldKey, InspectorDraft>> = {};
for (const spec of specs) {
  const field = fieldFor(spec) as InspectorField<unknown> | null;
  if (field) drafts[spec.key] = useInspectorDraft(field, (value) => send({ props: { [spec.key]: value } }));
}

function displayLabel(spec: EffectFieldSpec): string {
  return spec.kind === "percent" ? `${spec.label} (%)` : spec.label;
}
function inputModeOf(spec: EffectFieldSpec): "text" | "decimal" {
  return spec.kind === "text" ? "text" : "decimal";
}

// Template helpers, so the template itself stays a flat list of fields.
const locked = computed(() => lockReason.value !== null);
const heading = computed(() => `${effect.value ? EFFECT_NAMES[effect.value.kind] : ""} on ${clip.value?.name ?? ""}`);
const hasBackground = computed(() => effect.value?.background ?? false);
const removeTitle = computed(() => lockReason.value ?? "Remove this cue");

/** Re-picking the colour the cue already has is not an edit. */
function onColor(event: Event): void {
  const color = (event.target as HTMLInputElement).value;
  if (color !== effect.value?.color) void send({ props: { color } });
}
function onBackground(event: Event): void {
  void send({ props: { background: (event.target as HTMLInputElement).checked } });
}

async function remove(): Promise<void> {
  if (!effect.value || lockReason.value) return;
  if (await editorProject.execute({ kind: "removeEffect", effectId: props.effectId })) workspace.setSelected(null);
}
</script>

<template>
  <div
    v-if="effect"
    data-testid="effect-section"
    class="flex flex-col gap-2"
  >
    <p class="text-fg-muted">
      {{ heading }}
    </p>
    <p
      v-if="effect.kind === 'mask'"
      data-testid="effect-mask-warning"
      role="note"
      class="text-danger-fg"
    >
      {{ MASK_WARNING }}
    </p>
    <p
      v-if="lockReason"
      data-testid="effect-section-locked"
    >
      {{ lockReason }} — unlock it to change this cue.
    </p>
    <div class="grid grid-cols-2 gap-1.5">
      <InspectorNumberInput
        :field="startField"
        label="Starts at (ms)"
        testid="effect-field-start"
        :disabled="locked"
      />
      <InspectorNumberInput
        :field="endField"
        label="Ends at (ms)"
        testid="effect-field-end"
        :disabled="locked"
      />
      <template
        v-for="spec in specs"
        :key="spec.key"
      >
        <InspectorNumberInput
          v-if="drafts[spec.key]"
          :field="drafts[spec.key]!"
          :label="displayLabel(spec)"
          :testid="`effect-field-${spec.key}`"
          :inputmode="inputModeOf(spec)"
          :disabled="locked"
        />
        <label
          v-else-if="spec.kind === 'color'"
          class="flex flex-col gap-0.5"
        >
          <span class="text-fg-subtle">{{ spec.label }}</span>
          <input
            data-testid="effect-field-color"
            type="color"
            class="h-6 w-full cursor-pointer rounded border border-line bg-stage disabled:opacity-50"
            :value="effect.color"
            :disabled="locked"
            @change="onColor"
          >
        </label>
        <label
          v-else
          class="flex items-center gap-1.5"
        >
          <input
            data-testid="effect-field-background"
            type="checkbox"
            :checked="hasBackground"
            :disabled="locked"
            @change="onBackground"
          >
          <span>{{ spec.label }}</span>
        </label>
      </template>
    </div>
    <button
      type="button"
      data-testid="effect-remove"
      :disabled="locked"
      :title="removeTitle"
      class="cursor-pointer self-start rounded border border-line px-1.5 py-0.5 text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-50"
      @click="remove"
    >
      Remove cue
    </button>
  </div>
</template>
