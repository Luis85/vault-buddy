<script setup lang="ts">
/**
 * The Render dialog's settings (Task 47; SCREENS 09): the product's name,
 * its quality, whole-or-range (numeric start/end in output seconds), the
 * checks summary and the originals statement. Presentational — every
 * choice is a `v-model` the dialog owns; nothing here starts anything.
 *
 * The checks summary is a placeholder list until Task 54 builds the
 * pre-render checks, and says so rather than implying a pass (R20).
 */
import type { RenderQuality } from "../../../editorTypes";

const name = defineModel<string>("name", { required: true });
const quality = defineModel<RenderQuality>("quality", { required: true });
const scope = defineModel<"whole" | "range">("scope", { required: true });
/** Seconds as typed — a number once a number field has been edited. */
const start = defineModel<string | number>("start", { required: true });
const end = defineModel<string | number>("end", { required: true });

const QUALITIES: { id: RenderQuality; label: string; hint: string }[] = [
  { id: "low", label: "Low", hint: "Smallest file, fastest render." },
  { id: "balanced", label: "Balanced", hint: "Good quality at a moderate size." },
  { id: "high", label: "High", hint: "Best quality, largest file." },
];
const CHECKS_PLACEHOLDER = [
  "Pre-render checks arrive in a later update.",
  "Until then, review crop, captions and audio in the preview before rendering.",
];
const FIELD = "rounded-control border border-line bg-raised px-2 py-1 text-sm text-fg";
</script>

<template>
  <label class="flex flex-col gap-1 text-xs text-fg-secondary">
    Rendered video name
    <input
      v-model="name"
      data-testid="render-dialog-name"
      :class="FIELD"
    >
  </label>

  <fieldset class="flex flex-col gap-1">
    <legend class="text-xs text-fg-secondary">
      Quality
    </legend>
    <label
      v-for="q in QUALITIES"
      :key="q.id"
      class="flex cursor-pointer items-center gap-2 text-sm text-fg"
    >
      <input
        v-model="quality"
        type="radio"
        name="render-quality"
        :value="q.id"
        :data-testid="`render-dialog-quality-${q.id}`"
        class="h-4 w-4 accent-violet-500"
      >
      {{ q.label }} <span class="text-xs text-fg-subtle">{{ q.hint }}</span>
    </label>
  </fieldset>

  <fieldset class="flex flex-col gap-1">
    <legend class="text-xs text-fg-secondary">
      What to render
    </legend>
    <label class="flex cursor-pointer items-center gap-2 text-sm text-fg">
      <input
        v-model="scope"
        type="radio"
        name="render-scope"
        value="whole"
        data-testid="render-dialog-scope-whole"
        class="h-4 w-4 accent-violet-500"
      >
      The whole project
    </label>
    <label class="flex cursor-pointer items-center gap-2 text-sm text-fg">
      <input
        v-model="scope"
        type="radio"
        name="render-scope"
        value="range"
        data-testid="render-dialog-scope-range"
        class="h-4 w-4 accent-violet-500"
      >
      A range of the output
    </label>
    <div
      v-if="scope === 'range'"
      class="grid grid-cols-2 gap-2"
    >
      <label class="flex flex-col gap-1 text-xs text-fg-secondary">
        From (seconds)
        <input
          v-model="start"
          type="number"
          min="0"
          step="0.1"
          data-testid="render-dialog-range-start"
          :class="FIELD"
        >
      </label>
      <label class="flex flex-col gap-1 text-xs text-fg-secondary">
        To (seconds)
        <input
          v-model="end"
          type="number"
          min="0"
          step="0.1"
          data-testid="render-dialog-range-end"
          :class="FIELD"
        >
      </label>
    </div>
  </fieldset>

  <section class="rounded-control border border-line p-2">
    <h3 class="text-xs font-semibold text-fg-secondary">
      Checks
    </h3>
    <ul
      data-testid="render-dialog-checks"
      class="list-disc pl-4 text-xs text-fg-muted"
    >
      <li
        v-for="line in CHECKS_PLACEHOLDER"
        :key="line"
      >
        {{ line }}
      </li>
    </ul>
  </section>

  <p
    data-testid="render-dialog-originals"
    class="rounded-control border border-accent/30 bg-accent/10 p-2 text-xs text-accent-fg"
  >
    Your originals and this project are not changed. The render becomes a new
    product; the project stays editable.
  </p>
</template>
