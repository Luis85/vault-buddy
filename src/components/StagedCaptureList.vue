<script setup lang="ts">
/**
 * The resume-or-discard list (spec §10): "Opening Record Screen with staged
 * captures present shows them first: each with its source, duration and age,
 * offering Resume editing or Discard." Since Task 59 retired the phase-5
 * Save, editing is how a capture reaches a vault (Render + Publish), so the
 * action says so: "Edit to render and publish".
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
  /** Bumped by the picker whenever a discard ENDED without the list changing
   * — i.e. a refusal. `list_staged_captures` is deliberately not re-read
   * there ("the list on screen is still true"), so `captures` keeps its
   * identity and the watch below never fires; without this the refused row
   * stays armed and the next single click deletes the recording with no
   * second confirm. Optional so the picker is the only caller that has to
   * know about it. */
  disarmNonce?: number;
}>();
const emit = defineEmits<{ resume: [base: string]; discard: [base: string] }>();

/**
 * Which row's Discard is armed — a BASE, never a boolean.
 *
 * Discard destroys the only copy of a recording (spec §10: "Discard is
 * confirm-gated, being irreversible. Nothing is ever deleted silently"), so
 * it takes two clicks, the `TaskSectionMenu` precedent rather
 * than a native dialog (which steals OS focus, and `DIALOG_ACTIVE` is a
 * process-wide bool with two drivers already — docs/Gaps.md GAP-128).
 *
 * A shared boolean would arm EVERY row at once, so the next single click on
 * any other row would delete a recording the user never confirmed.
 */
const armed = ref<string | null>(null);

/**
 * An armed confirm that cannot be disarmed is a trap. It drops on "Keep it",
 * when the list is re-read underneath it (a successful discard re-reads, so
 * the rows are no longer the ones that were armed against), and on the
 * picker's `disarmNonce`.
 *
 * Both triggers are needed, and the second is not redundant: a REFUSED
 * discard leaves the same array on screen on purpose, so array identity
 * alone would leave the row armed after the one outcome that keeps it
 * visible.
 */
watch(
  () => [props.captures, props.disarmNonce],
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

/**
 * A capture pinned to a tutorial project (R6) opens the same editor as an
 * ordinary Resume — `emit("resume", base)` is unchanged — but "Edit" is the
 * honest verb once a project is already open on it, and Discard is not
 * offered at all: `staged_commands::discard_conflict` refuses it server-side
 * ("discard the project first"), so a button that always failed would be
 * worse than none.
 */
const primaryActionLabel = (c: StagedCaptureSummary) => (c.projectId ? "Edit" : "Edit to render and publish");

/** The chip is accent-toned only for an edit — a recovered capture carries
 * no edit to highlight. */
const lengthVariant = (c: StagedCaptureSummary) => (c.edited ? "accent" : "neutral");

/**
 * The chip: what a phase-4 edit's saved cut PRODUCES for an edited capture
 * (the cut the editor migrates), and simply what was recorded otherwise —
 * the two are equal when nothing was cut.
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
 * Where a recovered capture's video really is.
 *
 * `screen_recovery` promotes a `.part` to `<base>.mp4` in the staging
 * directory ONLY after `mp4_boxes` confirms it holds real footage, so this
 * file exists and plays — which is the whole point of the fragmented-MP4
 * container. The row has to say so: a recovered capture carries no vault id,
 * so the editor refuses it (F7) and Edit is not rendered, leaving Discard as
 * the only BUTTON on a real recording.
 *
 * The literal `%LOCALAPPDATA%` form is what the user can paste into Explorer
 * or the Run box; the DTO carries no absolute path, and no IPC command opens
 * this folder (`commands::open_logs_folder` reveals its sibling), so the text
 * itself is the affordance — hence `select-all` on the span.
 */
const STAGING_DIR = "%LOCALAPPDATA%\\com.vaultbuddy.desktop\\screen-captures";
const stagedPath = (c: StagedCaptureSummary) => `${STAGING_DIR}\\${c.base}.mp4`;

/**
 * The second line: the recorded length — worth saying only when an edit
 * makes it differ from the cut one — and the age.
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
    // R6: folded in here rather than a separate template branch — one more
    // `v-if` in `<template>` pushed its own cognitive complexity over the
    // ratchet, and this says the same thing with no extra control flow.
    c.projectId ? "In a tutorial project" : "",
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
    <SectionHeader>Not published yet</SectionHeader>
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
          <!-- The length the saved cut produces, which is what the user is
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
          <!-- The primary action is offered only where it leads somewhere.
               A recovered capture carries no vault id and no length, so
               `editor_open_staged` refuses it outright (F7) — true whether
               or not it also carries a pin. -->
          <AppButton
            v-if="!c.recovered"
            :data-testid="`staged-resume-${c.base}`"
            size="sm"
            variant="secondary"
            :disabled="busyBase === c.base"
            @click="emit('resume', c.base)"
          >
            {{ primaryActionLabel(c) }}
          </AppButton>
          <!-- Pinned rows say so in `subLabel` above instead of offering
               Discard (R6): the server refuses that discard anyway
               ("discard the project first"), so a button here would only
               ever fail. -->
          <AppButton
            v-if="!c.projectId"
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
        <!-- The third line, recovered rows only: the file is real and
             playable, and this is where it is. -->
        <p
          v-if="c.recovered"
          class="mt-1 text-micro text-fg-muted"
        >
          The video is still on disk and plays:
          <span
            :data-testid="`staged-path-${c.base}`"
            class="select-all break-all text-fg-secondary"
          >{{ stagedPath(c) }}</span>
        </p>
      </li>
    </ul>
  </section>
</template>
