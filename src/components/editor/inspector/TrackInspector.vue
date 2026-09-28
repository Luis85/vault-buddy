<script setup lang="ts">
/**
 * Track properties (visual-parity Task 13; concept spec §5 "Track"): the
 * track's card, then **Track** — Name (`renameTrack`), Track volume
 * (`setTrackFlags`), Up / Down (`moveTrack`) — and **Manage** — Remove
 * track…, which asks first when the track holds clips (`trackRemoval.ts`).
 *
 * Up, Down and Remove are the track menu's own items (`trackMenu`), so the
 * edge reasons ("Already the top track") and the lock reason are the menu's
 * and exist once. A locked track sends nothing from here: its name reads
 * only, and every other control carries the lock reason (Rust's
 * `setTrackFlags`/`renameTrack` refuse the same).
 */
import { computed } from "vue";

import { numberField, textField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { findMenuAction, reasonOf, useInspectorMenuContext } from "../../../composables/useInspectorMenu";
import { trackLockReason } from "../../../editor/menuContext";
import { trackMenu } from "../../../editor/menuSets";
import type { Track } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import type { MenuAction } from "../menus/menuModel";
import InspectorButton from "./InspectorButton.vue";
import InspectorSection from "./InspectorSection.vue";
import SelectionCard from "./SelectionCard.vue";

const props = defineProps<{ track: Track }>();

/** Mirrors `core::editor::mod::limits::MAX_NAME_CHARS`. */
const MAX_NAME_CHARS = 300;
/** Rust's track volume range `[0,2]`, shown as a percentage. */
const VOLUME_MAX_PCT = 200;

const editorProject = useEditorProjectStore();
const menuContext = useInspectorMenuContext();

const locked = computed(() => trackLockReason(props.track));
/** A locked track's name and slider: marked unavailable, with the reason. */
const lockedAttrs = computed<{ "aria-disabled"?: boolean; title?: string }>(() =>
  locked.value ? { "aria-disabled": true, title: locked.value } : {},
);
const items = computed(() => trackMenu(menuContext.value, props.track.id));
const up = computed(() => findMenuAction(items.value, "track-up"));
const down = computed(() => findMenuAction(items.value, "track-down"));
const remove = computed(() => findMenuAction(items.value, "track-remove"));

const card = computed(() =>
  props.track.kind === "video"
    ? { icon: "layers" as const, detail: "Upper tracks appear in front" }
    : { icon: "music" as const, detail: "Audio is mixed with other tracks" },
);

const name = useInspectorDraft(
  textField({ value: () => props.track.name, label: "Name", maxLength: MAX_NAME_CHARS }),
  (value: string) => editorProject.execute({ kind: "renameTrack", trackId: props.track.id, name: value }),
);
const volume = useInspectorDraft(
  numberField({
    value: () => Math.round(props.track.volume * 100),
    label: "Track volume",
    min: 0,
    max: VOLUME_MAX_PCT,
    rangeLabel: "0% and 200%",
  }),
  (pct: number) => editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, volume: pct / 100 }),
);

function run(action: MenuAction | null): void {
  action?.run?.();
}
function submitName(): void {
  if (!locked.value) name.submit();
}
/** A locked track's slider snaps back, so its release has nothing to send. */
function onVolumeInput(event: Event): void {
  const el = event.target as HTMLInputElement;
  if (locked.value) el.value = volume.draft.value;
  else volume.draft.value = el.value;
}
</script>

<template>
  <SelectionCard
    :icon="card.icon"
    :name="track.name"
    :detail="card.detail"
  />
  <InspectorSection title="Track">
    <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
      Name
      <input
        data-testid="track-inspector-name"
        type="text"
        class="rounded-md border border-line bg-stage px-2 py-1.5 text-[11px] text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        :value="name.draft.value"
        :readonly="Boolean(locked)"
        v-bind="lockedAttrs"
        @input="name.draft.value = ($event.target as HTMLInputElement).value"
        @keydown.enter="submitName"
        @keydown.escape="name.revert()"
        @blur="submitName"
      >
      <span
        v-if="name.error.value"
        role="alert"
        class="text-danger-fg"
      >{{ name.error.value }}</span>
    </label>
    <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
      <span class="flex justify-between">
        Track volume
        <span
          data-testid="track-inspector-volume-value"
          class="font-mono text-fg-secondary"
        >{{ volume.draft.value }}%</span>
      </span>
      <input
        data-testid="track-inspector-volume"
        type="range"
        min="0"
        :max="VOLUME_MAX_PCT"
        step="1"
        class="accent-accent"
        :aria-label="`${track.name} volume`"
        v-bind="lockedAttrs"
        :value="volume.draft.value"
        @input="onVolumeInput"
        @change="volume.submit()"
      >
    </label>
    <div class="flex flex-wrap gap-[5px]">
      <InspectorButton
        preset
        icon="up"
        data-testid="track-inspector-up"
        :reason="reasonOf(up)"
        @click="run(up)"
      >
        Up
      </InspectorButton>
      <InspectorButton
        preset
        icon="down"
        data-testid="track-inspector-down"
        :reason="reasonOf(down)"
        @click="run(down)"
      >
        Down
      </InspectorButton>
    </div>
    <p class="text-[10px] leading-[1.6] text-fg-muted">
      Lock prevents edits, not playback. Solo affects sound only.
    </p>
  </InspectorSection>
  <InspectorSection title="Manage">
    <InspectorButton
      icon="trash"
      class="self-start"
      data-testid="track-inspector-remove"
      :reason="reasonOf(remove)"
      @click="run(remove)"
    >
      Remove track…
    </InspectorButton>
  </InspectorSection>
</template>
