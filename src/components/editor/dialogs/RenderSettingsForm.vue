<script setup lang="ts">
/**
 * The Render dialog's settings (Task 47; SCREENS 09; visual-parity Task
 * 21, concept spec §9.6 top to bottom): the review-range box (a checkbox,
 * its help, From/To in output seconds), the render's parent — the
 * `PROJECT · r{revision}` pill, the title and what stays editable — the
 * product's name, where a publish goes (the vault by NAME, design D6; read
 * only here, the Publish dialog picks per publish), the output profile with
 * the native quality radios (`RenderProfileCard`), the "new output" callout
 * and the checks (`RenderChecksSummary`). Presentational — every choice is
 * a `v-model` the dialog owns; nothing here starts anything.
 */
import type { CheckFinding, RenderQuality } from "../../../editorTypes";
import RenderChecksSummary from "./RenderChecksSummary.vue";
import RenderProfileCard from "./RenderProfileCard.vue";

defineProps<{
  checksSummary: string;
  checksError: string | null;
  blocking: CheckFinding[];
  revision: number;
  title: string;
  /** The destination vault's name ("…" while it loads), or `null` when
   * none is set. */
  vaultName: string | null;
  vaultBusy: boolean;
  folder: string;
  canvas: { width: number; height: number; fps: number };
  /** How long the render will be: the range, when one is chosen. */
  durationMs: number;
}>();
const emit = defineEmits<{ (e: "review-checks"): void }>();

const name = defineModel<string>("name", { required: true });
const quality = defineModel<RenderQuality>("quality", { required: true });
const scope = defineModel<"whole" | "range">("scope", { required: true });
/** Seconds as typed — a number once a number field has been edited. */
const start = defineModel<string | number>("start", { required: true });
const end = defineModel<string | number>("end", { required: true });

const FIELD = "flex min-w-0 flex-col gap-[5px] text-[10px] text-fg-secondary";
</script>

<template>
  <section
    data-testid="render-dialog-range-box"
    class="rounded-[9px] border border-line bg-app p-3.5"
  >
    <label class="flex items-center gap-2 text-xs font-semibold text-fg">
      <input
        v-model="scope"
        type="checkbox"
        true-value="range"
        false-value="whole"
        data-testid="render-dialog-scope-range"
      >
      Render a short review range
    </label>
    <p class="mt-2 text-[10px] leading-[1.6] text-fg-muted">
      Test a section before rendering everything. All layers in this time range are included; the editable project
      is not trimmed.
    </p>
    <div
      v-if="scope === 'range'"
      class="mt-3 grid grid-cols-2 gap-[9px]"
    >
      <label :class="FIELD">
        From (seconds)
        <input
          v-model="start"
          type="number"
          min="0"
          step="0.1"
          data-testid="render-dialog-range-start"
          class="text-xs"
        >
      </label>
      <label :class="FIELD">
        To (seconds)
        <input
          v-model="end"
          type="number"
          min="0"
          step="0.1"
          data-testid="render-dialog-range-end"
          class="text-xs"
        >
      </label>
    </div>
  </section>

  <section
    data-testid="render-dialog-parent"
    class="flex flex-col items-start gap-2.5 rounded-[9px] border border-line p-[17px]"
  >
    <span
      data-testid="render-dialog-revision"
      class="rounded bg-accent-bg px-1.5 py-0.5 text-[9px] tracking-[0.3px] text-accent-ink"
    >PROJECT · r{{ revision }}</span>
    <b class="max-w-full text-sm font-semibold break-words text-fg">{{ title }}</b>
    <small class="text-[11px] text-fg-muted">This render becomes a product. The project stays editable.</small>
  </section>

  <label :class="FIELD">
    Rendered video name
    <input
      v-model="name"
      data-testid="render-dialog-name"
      class="text-xs"
    >
  </label>

  <dl
    data-testid="render-dialog-destination"
    class="grid grid-cols-2 gap-[9px] text-[10px] text-fg-secondary"
  >
    <div class="flex min-w-0 flex-col gap-[5px]">
      <dt>Destination vault</dt>
      <dd
        data-testid="render-dialog-destination-vault"
        :aria-busy="vaultBusy"
        class="truncate text-xs text-fg"
      >
        {{ vaultName ?? "Not chosen yet" }}
      </dd>
    </div>
    <div class="flex min-w-0 flex-col gap-[5px]">
      <dt>Folder inside vault</dt>
      <dd class="truncate text-xs text-fg">
        {{ folder || "The vault's screen-capture folder" }}
      </dd>
    </div>
  </dl>

  <RenderProfileCard
    v-model:quality="quality"
    :canvas="canvas"
    :duration-ms="durationMs"
  />

  <p
    data-testid="render-dialog-originals"
    class="flex flex-col gap-1 rounded-[7px] border border-accent/20 bg-accent-bg p-3.5 text-[11px] leading-[1.7] text-fg-secondary"
  >
    <b class="text-accent-ink">A new output, never an overwrite.</b>
    Your originals and this project are not changed. The render becomes a new product in this project's workspace,
    with the exact edit it was made from.
  </p>

  <RenderChecksSummary
    :summary="checksSummary"
    :error="checksError"
    :blocking="blocking"
    @review-checks="emit('review-checks')"
  />
</template>
