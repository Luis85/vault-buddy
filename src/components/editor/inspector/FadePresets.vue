<script setup lang="ts">
/**
 * The Fades tab's None / Quick · 0.5s / Gentle · 1s (visual-parity Task 15;
 * concept spec §5 "Fades"). Each is the selected clip's context-menu Fades
 * item of that length (`menuSetsClip.ts`: both edges, never past half the
 * clip), run through its own `run` and refused with its own reason — a
 * locked track is the Fades menu's. The preset the clip already wears
 * (both edges at that length, as the menu would clamp it) reads as pressed,
 * and pressing it again sends nothing.
 */
import { computed } from "vue";

import { findMenuAction, reasonOf, useInspectorMenuContext } from "../../../composables/useInspectorMenu";
import { clipMenu } from "../../../editor/menuSetsClip";
import InspectorButton from "./InspectorButton.vue";

const props = defineProps<{ fadeInMs: number; fadeOutMs: number; halfDurationMs: number }>();

const PRESETS = [
  { ms: 0, label: "None" },
  { ms: 500, label: "Quick · 0.5s" },
  { ms: 1_000, label: "Gentle · 1s" },
] as const;

const menuContext = useInspectorMenuContext();
const presets = computed(() => {
  const menu = clipMenu(menuContext.value);
  const fades = findMenuAction(menu, "fades");
  return PRESETS.map((p) => {
    const item = findMenuAction(menu, `fades-${p.ms}`);
    const both = Math.min(p.ms, props.halfDurationMs);
    const pressed = props.fadeInMs === both && props.fadeOutMs === both;
    // The preset the clip already wears is not an edit.
    return { ...p, run: () => pressed || item?.run?.(), reason: reasonOf(item, fades), pressed };
  });
});
</script>

<template>
  <div class="flex flex-wrap gap-[5px]">
    <InspectorButton
      v-for="p in presets"
      :key="p.ms"
      preset
      :data-testid="`fades-section-preset-${p.ms}`"
      :reason="p.reason"
      :pressed="p.pressed"
      @click="p.run"
    >
      {{ p.label }}
    </InspectorButton>
  </div>
</template>
