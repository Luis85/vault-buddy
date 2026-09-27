<script setup lang="ts">
/**
 * Selection properties (visual-parity Task 13; concept spec §5
 * "Multi-clip", `contextual-ui.js`): several clips at once. **Edit
 * together** lists up to eight of them and offers Copy, Duplicate and Group
 * (Ungroup when they already are one group); **Shared fades** None / 0.5s /
 * 1s at both edges; **Shared color** the concept's treatments (only when a
 * selected clip has a picture); **Sound** Mute all / Unmute all.
 *
 * Every button is an item of the multi-clip context menu (`multiClipMenu`),
 * run through its own `run` and refused with its own reason, so the rules
 * — the lock, a mute that would change nothing, a fade kept within half a
 * clip — exist once.
 */
import { computed } from "vue";

import { findMenuAction, reasonOf, useInspectorMenuContext } from "../../../composables/useInspectorMenu";
import { targetClips } from "../../../editor/actionTargets";
import { COLOR_TREATMENTS, isTreatment } from "../../../editor/colorPresets";
import { lockReason } from "../../../editor/menuContext";
import { multiClipMenu } from "../../../editor/menuSetsClip";
import EditorIcon from "../icons/EditorIcon.vue";
import type { MenuAction } from "../menus/menuModel";
import ColorTreatmentGrid from "./ColorTreatmentGrid.vue";
import InspectorButton from "./InspectorButton.vue";
import InspectorSection from "./InspectorSection.vue";
import SelectionCard from "./SelectionCard.vue";

const LIST_MAX = 8;
const SHARED_FADES = [
  { ms: 0, label: "None" },
  { ms: 500, label: "0.5s in & out" },
  { ms: 1_000, label: "1s in & out" },
] as const;

const menuContext = useInspectorMenuContext();

const project = computed(() => menuContext.value.action.project);
const clips = computed(() => targetClips(menuContext.value.action));
const items = computed(() => multiClipMenu(menuContext.value));
const item = (id: string): MenuAction | null => findMenuAction(items.value, id);

const locked = computed(() => lockReason(project.value, clips.value.map((c) => c.id)));
const grouped = computed(() => {
  const group = clips.value[0]?.group_id;
  return Boolean(group) && clips.value.every((c) => c.group_id === group);
});
const groupAction = computed(() => item(grouped.value ? "ungroup" : "group"));
const pictures = computed(() =>
  clips.value.some((c) => project.value?.assets.find((a) => a.id === c.asset_id)?.kind !== "audio"),
);
const activeTreatment = computed(
  () => COLOR_TREATMENTS.find((t) => clips.value.every((c) => isTreatment(c.adjustments, t.adjustments)))?.id ?? null,
);

function iconOf(assetId: string): "video" | "music" {
  return project.value?.assets.find((a) => a.id === assetId)?.kind === "audio" ? "music" : "video";
}
function run(action: MenuAction | null): void {
  action?.run?.();
}
function pickTreatment(id: string): void {
  run(item(`color-${id}`));
}
</script>

<template>
  <SelectionCard
    icon="layers"
    :name="`${clips.length} clips selected`"
    :detail="grouped ? 'Grouped for synchronized movement' : 'Shift-click to change selection'"
  />
  <p
    v-if="locked"
    data-testid="multi-inspector-locked"
    class="mb-3 rounded-md bg-gold-bg p-[9px] text-[10px] leading-[1.6] text-gold"
  >
    Unlock selected tracks before editing.
  </p>
  <InspectorSection title="Edit together">
    <ul
      data-testid="multi-inspector-list"
      class="flex flex-col gap-2 rounded-[7px] border border-line bg-app p-2.5"
    >
      <li
        v-for="c in clips.slice(0, LIST_MAX)"
        :key="c.id"
        class="flex min-w-0 items-center gap-[7px] text-[10px] leading-[1.4] text-fg-secondary"
      >
        <EditorIcon
          :name="iconOf(c.asset_id)"
          :size="13"
          class="shrink-0 text-accent"
        />
        <span class="truncate">{{ c.name }}</span>
      </li>
    </ul>
    <div class="flex flex-wrap gap-[5px]">
      <InspectorButton
        preset
        icon="copy"
        data-testid="multi-inspector-copy"
        :reason="reasonOf(item('copy'))"
        @click="run(item('copy'))"
      >
        Copy
      </InspectorButton>
      <InspectorButton
        preset
        icon="copy"
        data-testid="multi-inspector-duplicate"
        :reason="reasonOf(item('duplicate'))"
        @click="run(item('duplicate'))"
      >
        Duplicate
      </InspectorButton>
      <InspectorButton
        preset
        icon="group"
        data-testid="multi-inspector-group"
        :reason="reasonOf(groupAction)"
        @click="run(groupAction)"
      >
        {{ grouped ? "Ungroup" : "Group" }}
      </InspectorButton>
    </div>
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      Group movement keeps tracks and relative times fixed. Explicit edge trims edit one clip. Grouping does not remove
      transitions.
    </p>
  </InspectorSection>
  <InspectorSection title="Shared fades">
    <div class="flex flex-wrap gap-[5px]">
      <InspectorButton
        v-for="f in SHARED_FADES"
        :key="f.ms"
        preset
        :data-testid="`multi-inspector-fade-${f.ms}`"
        :reason="reasonOf(item(`fades-${f.ms}`), item('fades'))"
        @click="run(item(`fades-${f.ms}`))"
      >
        {{ f.label }}
      </InspectorButton>
    </div>
  </InspectorSection>
  <InspectorSection
    v-if="pictures"
    title="Shared color"
  >
    <ColorTreatmentGrid
      :active-id="activeTreatment"
      :reason="reasonOf(item('color'))"
      @pick="(t) => pickTreatment(t.id)"
    />
  </InspectorSection>
  <InspectorSection title="Sound">
    <div class="flex flex-wrap gap-[5px]">
      <InspectorButton
        preset
        icon="muted"
        data-testid="multi-inspector-mute"
        :reason="reasonOf(item('muteSelection'))"
        @click="run(item('muteSelection'))"
      >
        Mute all
      </InspectorButton>
      <InspectorButton
        preset
        icon="volume"
        data-testid="multi-inspector-unmute"
        :reason="reasonOf(item('unmuteSelection'))"
        @click="run(item('unmuteSelection'))"
      >
        Unmute all
      </InspectorButton>
    </div>
  </InspectorSection>
</template>
