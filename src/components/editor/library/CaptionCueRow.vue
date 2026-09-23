<script setup lang="ts">
/**
 * One caption in `CaptionsLibrary`'s list (Task 36): its text and its
 * OUTPUT-time span, editable in place. Presentational -- it emits what the
 * user committed (on `change`, i.e. blur or Enter, never per keystroke)
 * and `CaptionsLibrary` turns that into the `updateCaption`, converting
 * output time back to the clip's source time. A time that does not parse
 * is not emitted; the field just shows the stored time again. Each emit
 * carries a `revert` that puts the stored value back, which the parent
 * calls when Rust refuses the edit (fix round 1: the projection does not
 * change then, so Vue would never re-patch `:value` and the field would
 * keep showing a value that is not the stored one).
 */
import type { CaptionRow } from "../../../editor/captionRules";

const props = defineProps<{
  row: CaptionRow;
  selected: boolean;
  height: number;
}>();

/** Puts the stored value back into the field that emitted. */
export type Revert = () => void;

const emit = defineEmits<{
  (e: "text", value: string, revert: Revert): void;
  (e: "start", outputMs: number, revert: Revert): void;
  (e: "end", outputMs: number, revert: Revert): void;
  (e: "remove"): void;
}>();

function seconds(ms: number): string {
  return (ms / 1_000).toFixed(3);
}

function onText(event: Event): void {
  const value = (event.target as HTMLTextAreaElement).value;
  if (value.trim() === "" || value === props.row.cue.text) {
    (event.target as HTMLTextAreaElement).value = props.row.cue.text;
    return;
  }
  const input = event.target as HTMLTextAreaElement;
  emit("text", value, () => (input.value = props.row.cue.text));
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
    class="flex flex-col gap-0.5 overflow-hidden rounded border px-1 py-0.5"
    :class="selected ? 'border-focus bg-accent/10' : 'border-line'"
    :style="{ height: `${height - 4}px`, marginBottom: '4px' }"
  >
    <div class="flex items-center gap-1">
      <span class="w-6 shrink-0 text-fg-subtle">{{ row.index }}</span>
      <input
        :data-testid="`caption-start-${row.cue.id}`"
        type="number"
        min="0"
        step="0.001"
        :value="seconds(row.startMs)"
        :aria-label="`Caption ${row.index} start, seconds`"
        class="w-16 rounded border border-line bg-stage px-0.5 font-mono text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
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
        class="w-16 rounded border border-line bg-stage px-0.5 font-mono text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
        @change="onTime('end', $event)"
      >
      <button
        type="button"
        :data-testid="`caption-delete-${row.cue.id}`"
        :aria-label="`Delete caption ${row.index}`"
        class="ml-auto shrink-0 rounded px-1 text-fg-subtle hover:bg-white/10 hover:text-danger-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
        @click="emit('remove')"
      >
        ✕
      </button>
    </div>
    <textarea
      :data-testid="`caption-text-${row.cue.id}`"
      :value="row.cue.text"
      :aria-label="`Caption ${row.index} text`"
      rows="2"
      maxlength="500"
      class="min-h-0 flex-1 resize-none rounded border border-line bg-stage px-1 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      @change="onText"
    />
  </li>
</template>
