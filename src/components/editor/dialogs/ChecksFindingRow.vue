<script setup lang="ts">
/**
 * One before-you-share finding in the Checks dialog (Task 54; SCREENS 07;
 * visual-parity Task 21, concept spec §9.4 `.issue-row`): its kind chip —
 * FIX, REVIEW or NOTE, mono 9px — its heading (`checkTitles.CHECK_TITLES`,
 * by the finding's code), Rust's sentence, and the one button that reveals
 * what it is about (`checkReveal.CHECK_ACTION_LABELS`) with a chevron.
 * Presentational — the dialog owns what the button does. A finding with no
 * action (an empty timeline) has no button rather than a disabled one that
 * goes nowhere.
 */
import { computed } from "vue";

import { CHECK_ACTION_LABELS } from "../../../editor/checkReveal";
import { CHECK_KINDS, CHECK_TITLES } from "../../../editor/checkTitles";
import type { CheckFinding } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{ finding: CheckFinding }>();
const emit = defineEmits<{ (e: "act"): void }>();

const kind = computed(() => CHECK_KINDS[props.finding.severity]);
</script>

<template>
  <li
    :data-testid="`check-${finding.id}`"
    class="flex gap-3.5 border-b border-line py-3.5"
  >
    <span
      :data-testid="`check-kind-${finding.id}`"
      class="vb-mono h-max min-w-12 shrink-0 rounded px-1.5 py-1 text-center text-[9px] tracking-[0.5px]"
      :class="kind.chip"
    >{{ kind.tag }}</span>
    <div class="flex min-w-0 flex-1 flex-col items-start">
      <h3 class="mb-1.5 text-xs font-semibold text-fg">
        {{ CHECK_TITLES[finding.code] }}
      </h3>
      <p class="mb-2.5 text-[11px] leading-[1.7] break-words text-fg-muted">
        {{ finding.message }}
      </p>
      <button
        v-if="finding.action"
        type="button"
        :data-testid="`check-action-${finding.id}`"
        class="inline-flex min-h-[30px] items-center gap-1.5 rounded-[7px] border border-line bg-panel px-2.5 text-[10px] text-fg"
        @click="emit('act')"
      >
        {{ CHECK_ACTION_LABELS[finding.action] }}
        <EditorIcon
          name="chevronRight"
          :size="13"
        />
      </button>
    </div>
  </li>
</template>
