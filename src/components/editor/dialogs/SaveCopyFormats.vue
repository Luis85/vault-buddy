<script setup lang="ts">
/**
 * The Save a copy dialog's two format cards (visual-parity Task 21; concept
 * spec §9.5 `.save-option`, screen 08): **Portable project**, marked
 * Recommended, and **Lightweight project file** — full-size radios inside
 * full-width labels; the checked card takes the accent border and tint.
 * The sentences are the native truth (design D10): what each file holds
 * and how it opens again, never a browser download. Presentational — one
 * `v-model`.
 */
import type { PackageFormat } from "../../../editorTypes";

defineProps<{ disabled: boolean }>();
const format = defineModel<PackageFormat>({ required: true });

const OPTIONS: { id: PackageFormat; title: string; text: string }[] = [
  {
    id: "portable",
    title: "Portable project",
    text:
      "Workspace + available originals, including those a render still uses, in one ZIP (up to 200 MiB). " +
      "Open it with “Open a project file” on any computer.",
  },
  {
    id: "lightweight",
    title: "Lightweight project file",
    text: "Edits and workspace as JSON. Keep your originals; reconnect them when needed.",
  },
];
</script>

<template>
  <fieldset class="flex flex-col gap-[9px]">
    <legend class="sr-only">
      Project file format
    </legend>
    <label
      v-for="option in OPTIONS"
      :key="option.id"
      :data-testid="`save-project-option-${option.id}`"
      class="flex cursor-pointer items-start gap-3 rounded-[9px] border p-3.5"
      :class="format === option.id ? 'border-accent bg-accent-bg' : 'border-line hover:bg-hover-subtle'"
    >
      <input
        v-model="format"
        type="radio"
        name="save-project-format"
        :value="option.id"
        :disabled="disabled"
        :data-testid="`save-project-format-${option.id}`"
        class="my-0.5 h-[18px] w-[18px] shrink-0"
      >
      <span class="min-w-0 flex-1">
        <b class="flex items-center gap-1.5 text-xs font-semibold text-fg">
          {{ option.title }}
          <span
            v-if="option.id === 'portable'"
            data-testid="save-project-recommended"
            class="vb-mono rounded bg-audio-bg px-[5px] text-[9px] leading-4 font-normal text-audio"
          >Recommended</span>
        </b>
        <small class="mt-1.5 block text-[11px] leading-[1.6] text-fg-muted">{{ option.text }}</small>
      </span>
    </label>
  </fieldset>
</template>
