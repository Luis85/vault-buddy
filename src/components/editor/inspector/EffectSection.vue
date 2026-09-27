<script setup lang="ts">
/**
 * The selected teaching cue's inspector (Task 35; F-27–F-33; visual-parity
 * Task 15, concept spec §5 "Teaching cue"). Fills `InspectorPanel`'s
 * `#effect` slot while a cue is selected (`cueActions.selectedEffectOf`):
 * the cue's card — its glyph, its kind, "Attached to {clip}" — then its
 * kind's sections in the concept's order (`effectFields.CUE_SECTIONS`):
 * Instruction (text, step number, text size, readable background), Focus
 * (magnification, ease in / out), Appearance (line weight or background
 * dim), Color (`CueColorRow`), Timing within this clip (`CueTiming`) and
 * Position in percent of the video (`CuePropertySection` each). Every
 * property is exactly what Rust's `patch_props` accepts for the kind, each
 * one `updateEffect` (`CueField`).
 *
 * **A privacy cover** always shows F-33's limitation notice — permanent,
 * never dismissible: a stationary opaque box is not redaction, and the
 * recording underneath is untouched. A cue on a locked track says so and
 * every control refuses.
 */
import { computed } from "vue";

import { useSelectedClips } from "../../../composables/useSelectedClips";
import type { EffectFieldSpec } from "../../../editor/effectFields";
import { CUE_SECTIONS, EFFECT_ICONS, EFFECT_NAMES, fieldsFor, MASK_WARNING } from "../../../editor/effectFields";
import type { Clip, Effect } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import CuePropertySection from "./CuePropertySection.vue";
import CueTiming from "./CueTiming.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorSection from "./InspectorSection.vue";
import SelectionCard from "./SelectionCard.vue";

const props = defineProps<{ effectId: string }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const effect = computed<Effect | null>(
  () => editorProject.project?.effects.find((e) => e.id === props.effectId) ?? null,
);
const clip = computed<Clip | null>(() => (effect.value ? (editorProject.clipById(effect.value.clip_id) ?? null) : null));
const { lockReason } = useSelectedClips(() => (clip.value ? [clip.value.id] : []));
const locked = computed(() => lockReason.value !== null);

/** A cue's kind never changes, so its sections are fixed per instance
 * (`EditorRoot` keys this section on the effect id). */
const specs: EffectFieldSpec[] = effect.value ? fieldsFor(effect.value.kind) : [];
const sections = CUE_SECTIONS.map((s) => ({ ...s, fields: specs.filter((f) => f.section === s.id) })).filter(
  (s) => s.fields.length > 0,
);
const before = sections.filter((s) => s.id !== "position");
const position = sections.find((s) => s.id === "position");

type Update = { startMs?: number; endMs?: number; props?: Record<string, unknown> };

function send(update: Update): Promise<boolean> {
  if (!effect.value || lockReason.value) return Promise.resolve(false);
  return editorProject.execute({ kind: "updateEffect", effectId: props.effectId, ...update });
}
function sendProps(patch: Record<string, unknown>): Promise<boolean> {
  return send({ props: patch });
}

async function remove(): Promise<void> {
  if (!effect.value || lockReason.value) return;
  if (await editorProject.execute({ kind: "removeEffect", effectId: props.effectId })) workspace.setSelected(null);
}
</script>

<template>
  <div
    v-if="effect && clip"
    data-testid="effect-section"
    class="flex min-w-0 flex-col"
  >
    <SelectionCard
      :icon="EFFECT_ICONS[effect.kind]"
      :name="EFFECT_NAMES[effect.kind]"
      :detail="`Attached to ${clip.name}`"
    />
    <p
      v-if="lockReason"
      data-testid="effect-section-locked"
      class="mb-3 rounded-md bg-gold-bg p-[9px] text-[10px] leading-[1.6] text-gold"
    >
      {{ lockReason }}. Unlock it before editing.
    </p>
    <p
      v-if="effect.kind === 'mask'"
      data-testid="effect-mask-warning"
      role="note"
      class="mb-3 rounded-[7px] bg-gold-bg p-[13px] text-[11px] leading-[1.7] text-gold"
    >
      {{ MASK_WARNING }}
    </p>
    <CuePropertySection
      v-for="s in before"
      :key="s.id"
      :section="s"
      :effect="effect"
      :lock-reason="lockReason"
      :send="sendProps"
    />
    <InspectorSection title="Timing within this clip">
      <CueTiming
        :effect="effect"
        :clip="clip"
        :disabled="locked"
        :send="send"
      />
    </InspectorSection>
    <CuePropertySection
      v-if="position"
      :section="position"
      :effect="effect"
      :lock-reason="lockReason"
      :send="sendProps"
    />
    <InspectorButton
      class="mt-1 self-start"
      icon="trash"
      data-testid="effect-remove"
      :reason="lockReason"
      @click="remove"
    >
      Remove cue
    </InspectorButton>
  </div>
</template>
