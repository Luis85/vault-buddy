<script setup lang="ts">
/**
 * The Render dialog's checks (Task 54; visual-parity Task 21, concept spec
 * §9.6): the "N blockers · M review warnings" summary and every blocking
 * finding by name — the ones that keep Render disabled — with **Review all
 * checks** for the rest. A read that failed says so rather than implying a
 * pass (R20).
 */
import type { CheckFinding } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";

defineProps<{ summary: string; error: string | null; blocking: CheckFinding[] }>();
const emit = defineEmits<{ (e: "review-checks"): void }>();
</script>

<template>
  <section class="flex flex-col gap-2 rounded-[9px] border border-line bg-app p-3.5">
    <div class="flex items-center justify-between gap-2">
      <b
        data-testid="render-dialog-checks-summary"
        class="text-xs font-semibold text-fg"
      >{{ error ? `Checks could not be read. ${error}` : summary }}</b>
      <button
        type="button"
        data-testid="render-dialog-open-checks"
        class="inline-flex min-h-[30px] shrink-0 items-center gap-1.5 rounded-[7px] border border-line bg-panel px-2.5 text-[10px] text-fg"
        @click="emit('review-checks')"
      >
        Review all checks
        <EditorIcon
          name="chevronRight"
          :size="13"
        />
      </button>
    </div>
    <ul
      data-testid="render-dialog-checks"
      class="list-disc pl-4 text-[11px] text-fg-muted"
    >
      <li
        v-for="finding in blocking"
        :key="finding.id"
        class="text-danger-fg"
      >
        {{ finding.message }}
      </li>
      <li v-if="blocking.length === 0 && !error">
        Nothing blocks this render.
      </li>
    </ul>
  </section>
</template>
