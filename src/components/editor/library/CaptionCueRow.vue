<script setup lang="ts">
/**
 * One caption card in `CaptionsLibrary`'s list (Task 36; visual-parity
 * Task 10, concept spec §3.4 `.caption-card`): its OUTPUT span as a gold
 * mono time (a click selects the caption and moves the playhead onto it),
 * an edit button, the text, **Split cue** and delete.
 *
 * **Editing.** The edit button (or a click on the text) turns the card's
 * time and text into fields, in the same fixed height so the windowed list
 * keeps its geometry; pressing it again leaves. Presentational -- it emits
 * what the user committed (on `change`, i.e. blur or Enter, never per
 * keystroke) and `CaptionsLibrary` turns that into the `updateCaption`,
 * converting output time back to the clip's source time. A time that does
 * not parse is not emitted; the field just shows the stored time again.
 * Each emit carries a `revert` that puts the stored value back, which the
 * parent calls when Rust refuses the edit (fix round 1: the projection
 * does not change then, so Vue would never re-patch `:value` and the field
 * would keep showing a value that is not the stored one).
 */
import type { CaptionRow } from "../../../editor/captionRules";
import { formatOutputTime } from "../../../editor/captionRules";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{
  row: CaptionRow;
  selected: boolean;
  height: number;
  editing: boolean;
  /** Why "Split cue" cannot run, or `null`. */
  splitReason: string | null;
}>();

/** Puts the stored value back into the field that emitted. */
export type Revert = () => void;

const emit = defineEmits<{
  (e: "text", value: string, revert: Revert): void;
  (e: "start", outputMs: number, revert: Revert): void;
  (e: "end", outputMs: number, revert: Revert): void;
  (e: "remove"): void;
  (e: "edit"): void;
  (e: "select"): void;
  (e: "split"): void;
}>();

const ICON_BUTTON =
  "flex h-[25px] min-h-0 w-[25px] shrink-0 items-center justify-center rounded-[7px] p-1 text-fg-muted hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus";
const TIME_FIELD = "h-6 min-h-0 w-16 px-1 py-0 font-mono text-[10px] text-fg";

function seconds(ms: number): string {
  return (ms / 1_000).toFixed(3);
}

function onText(event: Event): void {
  const input = event.target as HTMLTextAreaElement;
  if (input.value.trim() === "" || input.value === props.row.cue.text) {
    input.value = props.row.cue.text;
    return;
  }
  emit("text", input.value, () => (input.value = props.row.cue.text));
}

function onTime(which: "start" | "end", event: Event): void {
  const input = event.target as HTMLInputElement;
  const ms = Math.round(Number(input.value) * 1_000);
  const current = which === "start" ? props.row.startMs : props.row.endMs;
  if (input.value.trim() === "" || !Number.isFinite(ms) || ms < 0 || ms === current) {
    input.value = seconds(current);
    return;
  }
  const revert = () => (input.value = seconds(current));
  if (which === "start") emit("start", ms, revert);
  else emit("end", ms, revert);
}
</script>

<template>
  <li
    :data-testid="`caption-row-${row.cue.id}`"
    :aria-current="selected ? 'true' : undefined"
    class="flex flex-col gap-1 overflow-hidden rounded-lg border bg-app p-2.5"
    :class="selected ? 'border-focus' : 'border-line'"
    :style="{ height: `${height - 8}px`, marginBottom: '8px' }"
  >
    <div class="flex h-[25px] items-center justify-between gap-1">
      <div
        v-if="editing"
        class="flex items-center gap-1"
      >
        <input
          :data-testid="`caption-start-${row.cue.id}`"
          type="number"
          min="0"
          step="0.001"
          :value="seconds(row.startMs)"
          :aria-label="`Caption ${row.index} start, seconds`"
          :class="TIME_FIELD"
          @change="onTime('start', $event)"
        >
        <span aria-hidden="true">–</span>
        <input
          :data-testid="`caption-end-${row.cue.id}`"
          type="number"
          min="0"
          step="0.001"
          :value="seconds(row.endMs)"
          :aria-label="`Caption ${row.index} end, seconds`"
          :class="TIME_FIELD"
          @change="onTime('end', $event)"
        >
      </div>
      <button
        v-else
        type="button"
        :data-testid="`caption-time-${row.cue.id}`"
        :title="`Select caption ${row.index} and move the playhead to it`"
        class="min-h-6 truncate rounded px-0 font-mono text-[8px] text-gold focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="emit('select')"
      >
        {{ formatOutputTime(row.startMs) }} — {{ formatOutputTime(row.endMs) }}
      </button>
      <button
        type="button"
        :data-testid="`caption-edit-${row.cue.id}`"
        :aria-label="editing ? `Finish editing caption ${row.index}` : `Edit caption ${row.index}`"
        :aria-pressed="editing"
        :class="ICON_BUTTON"
        @click="emit('edit')"
      >
        <EditorIcon
          :name="editing ? 'check' : 'edit'"
          :size="12"
        />
      </button>
    </div>
    <textarea
      v-if="editing"
      :data-testid="`caption-text-${row.cue.id}`"
      :value="row.cue.text"
      :aria-label="`Caption ${row.index} text`"
      rows="2"
      maxlength="500"
      class="min-h-0 flex-1 resize-none px-1.5 py-1 text-[11px] leading-[1.6] text-fg"
      @change="onText"
    />
    <button
      v-else
      type="button"
      :data-testid="`caption-body-${row.cue.id}`"
      :aria-label="`Edit caption ${row.index} text`"
      class="flex min-h-0 flex-1 items-start rounded p-0 text-left text-[11px] leading-[1.6] text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      @click="emit('edit')"
    >
      <span class="line-clamp-2">{{ row.cue.text }}</span>
    </button>
    <div class="flex h-[25px] items-center justify-between">
      <button
        type="button"
        :data-testid="`caption-split-${row.cue.id}`"
        :disabled="splitReason !== null"
        :title="splitReason ?? 'Split text and divide its duration'"
        class="min-h-0 rounded px-0 py-[3px] text-[9px] text-fg-secondary hover:bg-transparent hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="emit('split')"
      >
        Split cue
      </button>
      <button
        type="button"
        :data-testid="`caption-delete-${row.cue.id}`"
        :aria-label="`Delete caption ${row.index}`"
        :class="ICON_BUTTON"
        class="hover:text-danger-fg"
        @click="emit('remove')"
      >
        <EditorIcon
          name="trash"
          :size="12"
        />
      </button>
    </div>
  </li>
</template>
