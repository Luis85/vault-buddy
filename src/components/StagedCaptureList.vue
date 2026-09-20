<script setup lang="ts">
/**
 * The resume-or-discard list (spec §10): "Opening Record Screen with staged
 * captures present shows them first: each with its source, duration and age,
 * offering Resume editing or Discard."
 *
 * PRESENTATIONAL — no `invoke`, no store, no event listener. `ScreenSourcePicker`
 * owns `list_staged_captures` / `open_capture_editor` / `discard_staged_capture`,
 * which is what keeps that file under the frontend LOC cap and follows how
 * `ScreenRegionPicker` and `ScreenAudioPicker` were extracted from it.
 */
import { ref, watch } from "vue";

import type { StagedCaptureSummary } from "../types";
import { formatDuration } from "../utils/formatDuration";
import { relativeAgeLabel } from "../utils/relativeAge";
import AppButton from "./ui/AppButton.vue";
import Chip from "./ui/Chip.vue";
import SectionHeader from "./ui/SectionHeader.vue";

const props = defineProps<{
  captures: StagedCaptureSummary[];
  /** The one row whose write is in flight, if any. Keyed on the base rather
   * than a bare boolean so a discard cannot grey out the rows it is not
   * touching. */
  busyBase: string | null;
}>();
const emit = defineEmits<{ resume: [base: string]; discard: [base: string] }>();

/**
 * Which row's Discard is armed — a BASE, never a boolean.
 *
 * Discard destroys the only copy of a recording (spec §10: "Discard is
 * confirm-gated, being irreversible. Nothing is ever deleted silently"), so
 * it takes two clicks, the `ExportBar` / `TaskSectionMenu` precedent rather
 * than a native dialog (which steals OS focus, and `DIALOG_ACTIVE` is a
 * process-wide bool with two drivers already — docs/Gaps.md GAP-128).
 *
 * A shared boolean would arm EVERY row at once, so the next single click on
 * any other row would delete a recording the user never confirmed.
 */
const armed = ref<string | null>(null);

/** An armed confirm that cannot be disarmed is a trap. It drops when the
 * list is re-read underneath it as well as on "Keep it": after a discard or
 * a refusal the rows may not be the ones that were armed against. */
watch(
  () => props.captures,
  () => {
    armed.value = null;
  },
);

/**
 * The armed-state derivations, and the Discard click itself, live HERE
 * rather than as `armed === c.base ? … : …` expressions repeated across four
 * attributes — the same reason `lengthLabel` below is composed in the script.
 * Four copies of one comparison is four places to get the two-click confirm
 * wrong, and the per-row markup is what pushes this component's template
 * past the complexity ratchet.
 */
const isArmed = (base: string) => armed.value === base;

/** First click arms this row; second click destroys the recording. */
const onDiscardClick = (base: string) => {
  if (isArmed(base)) emit("discard", base);
  else armed.value = base;
};

const discardLabel = (base: string) => (isArmed(base) ? "Delete it" : "Discard");

/** The chip is accent-toned only for an edit — a recovered capture carries
 * no edit to highlight. */
const lengthVariant = (c: StagedCaptureSummary) => (c.edited ? "accent" : "neutral");

/**
 * The chip: what an export will PRODUCE for an edited capture, and simply
 * what was recorded otherwise — the two are equal when nothing was cut.
 *
 * Composed here rather than as a pair of template branches because the
 * per-row markup is what pushes this component's template past the
 * complexity ratchet; the rule is the same either way, and stated once.
 */
const lengthLabel = (c: StagedCaptureSummary) => {
  // A recovered capture's sidecar was rebuilt from the file alone, which
  // records no duration at all — so `0:00` would be a claim, not a length.
  if (c.recovered) return "recovered · length unknown";
  return c.edited ? `edited · ${formatDuration(c.outputDurationMs)}` : formatDuration(c.durationMs);
};

/**
 * The second line: the recorded length — worth saying only when an edit
 * makes it differ from the exported one — and the age.
 *
 * Each part is dropped when it has nothing to say, so an unreadable
 * timestamp (the sidecar is hand-editable) leaves no stranded separator and
 * never the literal "NaN ago".
 *
 * Evaluated per render rather than captured at setup, so a list re-read
 * after a discard reports ages against the current clock.
 */
const subLabel = (c: StagedCaptureSummary) =>
  [
    c.edited ? `${formatDuration(c.durationMs)} recorded` : "",
    relativeAgeLabel(c.recordedAt, Date.now()),
  ]
    .filter((part) => part !== "")
    .join(" · ");
</script>

<template>
  <section
    v-if="captures.length > 0"
    data-testid="staged-list"
    class="flex flex-col gap-1"
  >
    <SectionHeader>Not saved yet</SectionHeader>
    <ul class="flex flex-col gap-1">
      <li
        v-for="c in captures"
        :key="c.base"
        :data-testid="`staged-row-${c.base}`"
        class="rounded-control border border-white/10 bg-white/5 px-3 py-2"
      >
        <div class="flex items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-sm font-medium text-fg">
            {{ c.sourceTitle }}
          </span>
          <!-- The length the EXPORT will produce, which is what the user is
               deciding about. An unedited capture's two durations are equal,
               so this is also the recorded length wherever it matters. -->
          <Chip :variant="lengthVariant(c)">
            {{ lengthLabel(c) }}
          </Chip>
        </div>
        <div class="mt-0.5 flex items-center gap-2">
          <span class="min-w-0 flex-1 truncate text-micro text-fg-subtle">
            {{ subLabel(c) }}
          </span>
          <!-- Resume is offered only where it leads somewhere. A recovered
               capture carries no vault id, so `export_worker::prepare`
               refuses its Save outright and the editor would open on a
               zero-length timeline: the only honest action left is Discard,
               and the row already says where the file is. -->
          <AppButton
            v-if="!c.recovered"
            :data-testid="`staged-resume-${c.base}`"
            size="sm"
            variant="secondary"
            :disabled="busyBase === c.base"
            @click="emit('resume', c.base)"
          >
            Resume editing
          </AppButton>
          <AppButton
            :data-testid="`staged-discard-${c.base}`"
            size="sm"
            variant="danger"
            :disabled="busyBase === c.base"
            @click="onDiscardClick(c.base)"
          >
            {{ discardLabel(c.base) }}
          </AppButton>
          <AppButton
            v-if="isArmed(c.base)"
            :data-testid="`staged-keep-${c.base}`"
            size="sm"
            variant="ghost"
            @click="armed = null"
          >
            Keep it
          </AppButton>
        </div>
      </li>
    </ul>
  </section>
</template>
