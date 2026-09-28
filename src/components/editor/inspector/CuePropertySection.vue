<script setup lang="ts">
/**
 * One section of a teaching cue's inspector (visual-parity Task 15; concept
 * spec §5 "Teaching cue"): the Color row, or the section's fields
 * (`CueField`) — side by side in pairs for the position, stacked
 * otherwise. `EffectSection` decides which sections a cue's kind has.
 */
import type { CueSection, EffectFieldSpec } from "../../../editor/effectFields";
import type { Effect } from "../../../editorTypes";
import CueColorRow from "./CueColorRow.vue";
import CueField from "./CueField.vue";
import InspectorSection from "./InspectorSection.vue";

defineProps<{
  section: { id: CueSection; title: string; fields: EffectFieldSpec[] };
  effect: Effect;
  lockReason: string | null;
  send: (patch: Record<string, unknown>) => Promise<boolean>;
}>();
</script>

<template>
  <InspectorSection :title="section.title">
    <CueColorRow
      v-if="section.id === 'color'"
      :color="effect.color ?? ''"
      :reason="lockReason"
      @pick="(color) => send({ color })"
    />
    <div
      v-else
      :class="section.id === 'position' ? 'grid grid-cols-2 gap-[9px]' : 'contents'"
    >
      <CueField
        v-for="f in section.fields"
        :key="f.key"
        :spec="f"
        :effect="effect"
        :disabled="lockReason !== null"
        :send="send"
      />
    </div>
  </InspectorSection>
</template>
