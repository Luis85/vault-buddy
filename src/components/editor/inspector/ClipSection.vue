<script setup lang="ts">
/**
 * The Inspector's Clip category (Task 21; F-48; visual-parity Task 14,
 * concept spec §5 "Clip"). **Placement**: the clip's name (`updateClip`),
 * the Track select — the tracks of its own kind, a locked one suffixed
 * "· locked" and unavailable — which moves it there in place (`moveClips`
 * with `trackId`), its Timeline start (`moveClips` by the difference), and
 * Earlier / Later / Duplicate, the registry's own actions (`actions.ts`,
 * through `actionItem`, the context menu's path), so their commands and
 * disabled reasons exist once. **Source range**: In and Out (`trimClip`,
 * which keeps the clip's start), with the source's original length.
 *
 * Every time here is in SECONDS (a 0.1 step) and reaches Rust as whole
 * milliseconds (`secondsField`). One clip only: the Selection properties
 * state serves several. A clip on a locked track disables every field; the
 * inspector's frame says why once, and the fieldset carries the reason.
 *
 * **Fresh across external edits WITHOUT a remount** (Task 19's carried
 * finding): `useInspectorDraft` re-seeds a draft the user is not editing
 * whenever its committed value changes, and every value getter reads the
 * LIVE `clip` computed — so an Undo, or a drag on this same clip from the
 * timeline, shows here at once, while a value the user is typing is left
 * alone. The caller keys this component on the SELECTION only
 * (`EditorRoot.vue`).
 */
import { computed, nextTick, ref, watch } from "vue";

import { secondsField, textField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { useInspectorMenuContext } from "../../../composables/useInspectorMenu";
import { useSelectedClips } from "../../../composables/useSelectedClips";
import type { ActionId } from "../../../editor/actionMeta";
import { trackMoveRefusal, trackOptions } from "../../../editor/clipPlacement";
import { actionItem } from "../../../editor/menuContext";
import { clipNameFocus } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import type { EditorIconName } from "../icons/conceptIcons";
import { formatMenuTime } from "../menus/menuModel";
import InspectorButton from "./InspectorButton.vue";
import InspectorNumberInput from "./InspectorNumberInput.vue";
import InspectorSection from "./InspectorSection.vue";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const menuContext = useInspectorMenuContext();
const { lockReason } = useSelectedClips(() => props.clipIds);

/** The LIVE clip — re-read on every projection the store installs. Whether
 * the fields exist at all is decided once at setup: the selection the
 * caller keys on cannot change under this instance. */
const clip = computed(() => (props.clipIds.length === 1 ? editorProject.clipById(props.clipIds[0]) : undefined));

/** Mirror `core::editor::mod::limits::{MAX_NAME_CHARS, MAX_DURATION_MS}`. */
const MAX_NAME_CHARS = 300;
const MAX_DURATION_MS = 7_200_000;

/** An image may extend up to `MAX_DURATION_MS` (Rust's `out_ms_bound`: "images
 * have no source bound beyond that"), anything else to its own length; an
 * asset that does not resolve degrades to the maximum. */
const asset = editorProject.project?.assets.find((a) => a.id === clip.value?.asset_id);
const sourceMaxMs = asset === undefined || asset.media_type === "image" ? MAX_DURATION_MS : asset.duration_ms;
const SOURCE_HELP = "Numeric source trims keep this clip’s timeline start fixed.";
const sourceHelp = asset ? `Original length ${formatMenuTime(asset.duration_ms)}. ${SOURCE_HELP}` : SOURCE_HELP;

type LiveClip = NonNullable<typeof clip.value>;

function run(build: (c: LiveClip) => Parameters<typeof editorProject.execute>[0]): Promise<boolean> {
  const c = clip.value;
  return c ? editorProject.execute(build(c)) : Promise.resolve(false);
}
/** `trimClip` sets placement and source range directly, never a delta, so
 * the untouched fields go back at their committed values. */
function trim(overrides: { inMs?: number; outMs?: number }): Promise<boolean> {
  return run((c) => ({ kind: "trimClip", clipId: c.id, startMs: c.start_ms, inMs: c.in_ms, outMs: c.out_ms, ...overrides }));
}
function moveTo(trackId: string | null, startMs?: number): Promise<boolean> {
  return run((c) => ({ kind: "moveClips", clipIds: [c.id], deltaMs: (startMs ?? c.start_ms) - c.start_ms, trackId }));
}

function seconds(label: string, read: (c: LiveClip) => number, maxMs: number, commit: (ms: number) => Promise<boolean>) {
  return useInspectorDraft(secondsField({ value: () => (clip.value ? read(clip.value) : 0), label, maxMs }), commit);
}

const drafts = clip.value
  ? {
      name: useInspectorDraft(
        textField({ value: () => clip.value?.name ?? "", label: "Name", maxLength: MAX_NAME_CHARS }),
        (name: string) => run((c) => ({ kind: "updateClip", clipId: c.id, name })),
      ),
      start: seconds("Timeline start", (c) => c.start_ms, MAX_DURATION_MS, (ms) => moveTo(null, ms)),
      in: seconds("In", (c) => c.in_ms, sourceMaxMs, (inMs) => trim({ inMs })),
      out: seconds("Out", (c) => c.out_ms, sourceMaxMs, (outMs) => trim({ outMs })),
    }
  : null;

const tracks = computed(() => (clip.value ? trackOptions(editorProject.project, clip.value) : []));
const trackRefusal = computed(() => (clip.value ? trackMoveRefusal(editorProject.project, clip.value) : null));
/** A refused move cannot reach here: the select is disabled with its reason. */
function onTrack(event: Event): void {
  void moveTo((event.target as HTMLSelectElement).value);
}

const PRESETS: { id: ActionId; label: string; icon: EditorIconName }[] = [
  { id: "earlier", label: "Earlier", icon: "arrowLeft" },
  { id: "later", label: "Later", icon: "arrowRight" },
  { id: "duplicate", label: "Duplicate", icon: "copy" },
];
const presets = computed(() => PRESETS.map((p) => actionItem(menuContext.value, p.id, p)));

/** The clip menu's "Rename…" (visual-parity Task 5): focus this clip's
 * name field when the request names it. */
const sectionRoot = ref<HTMLElement | null>(null);
watch(
  clipNameFocus,
  (id) => {
    if (id === null || id !== clip.value?.id) return;
    clipNameFocus.value = null;
    void nextTick(() => {
      const input = sectionRoot.value?.querySelector<HTMLInputElement>('[data-testid="clip-section-name"]');
      input?.focus();
      input?.select();
    });
  },
  { immediate: true },
);
</script>

<template>
  <fieldset
    v-if="drafts && clip"
    ref="sectionRoot"
    data-testid="clip-section"
    :disabled="lockReason !== null"
    :title="lockReason ?? undefined"
    class="flex min-w-0 flex-col"
  >
    <InspectorSection title="Placement">
      <InspectorNumberInput
        :field="drafts.name"
        label="Clip name"
        testid="clip-section-name"
        inputmode="text"
      />
      <label class="flex flex-col gap-[5px] text-[10px] text-fg-secondary">
        Track
        <select
          data-testid="clip-section-track"
          class="text-[11px] text-fg disabled:opacity-50"
          :value="clip.track_id"
          :disabled="trackRefusal !== null"
          :title="trackRefusal ?? undefined"
          @change="onTrack"
        >
          <option
            v-for="t in tracks"
            :key="t.id"
            :value="t.id"
            :disabled="t.disabled"
          >
            {{ t.label }}
          </option>
        </select>
      </label>
      <InspectorNumberInput
        :field="drafts.start"
        label="Timeline start (s)"
        testid="clip-section-start"
        :step="0.1"
        :min="0"
      />
      <div class="flex flex-wrap gap-[5px]">
        <InspectorButton
          v-for="(item, i) in presets"
          :key="item.id"
          preset
          :icon="PRESETS[i].icon"
          :data-testid="`clip-section-${item.id}`"
          :reason="item.disabledReason"
          @click="item.run?.()"
        >
          {{ item.label }}
        </InspectorButton>
      </div>
      <p
        data-testid="clip-section-placement-help"
        class="text-[10px] leading-[1.6] text-fg-muted"
      >
        Earlier / Later swaps adjacent clips. Other tracks are not moved.
      </p>
    </InspectorSection>
    <InspectorSection title="Source range">
      <div class="grid grid-cols-2 gap-[9px]">
        <InspectorNumberInput
          :field="drafts.in"
          label="In (s)"
          testid="clip-section-in"
          :step="0.1"
          :min="0"
        />
        <InspectorNumberInput
          :field="drafts.out"
          label="Out (s)"
          testid="clip-section-out"
          :step="0.1"
          :min="0"
        />
      </div>
      <p
        data-testid="clip-section-source-help"
        class="text-[10px] leading-[1.6] text-fg-muted"
      >
        {{ sourceHelp }}
      </p>
    </InspectorSection>
  </fieldset>
  <p
    v-else
    data-testid="clip-section-multi"
    class="text-fg-subtle"
  >
    Select a single clip to edit its name, start, in and out.
  </p>
</template>
