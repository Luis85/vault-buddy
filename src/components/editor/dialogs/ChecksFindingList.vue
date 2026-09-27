<script setup lang="ts">
/**
 * The Checks dialog's body (Task 54; SCREENS 07; visual-parity Task 21,
 * concept spec §9.4): the summary box ("N blockers · M review warnings"
 * and what the checks inspect), then one list of issue rows — FIX, then
 * REVIEW, then NOTE — each with its reveal button; an empty list is the
 * concept's callout; a failed read says so and nothing that could read as
 * a pass. Each severity keeps its own labelled group, so a screen reader
 * still hears which ones block. Split out of `ChecksDialog` for the
 * template-complexity ratchet; the dialog owns what a row's button does.
 */
import { computed } from "vue";

import type { CheckFinding, CheckSeverity } from "../../../editorTypes";
import { useEditorChecksStore } from "../../../stores/editorChecks";
import ChecksFindingRow from "./ChecksFindingRow.vue";

const emit = defineEmits<{ (e: "act", finding: CheckFinding): void }>();

const checks = useEditorChecksStore();

const GROUPS: { severity: CheckSeverity; title: string }[] = [
  { severity: "blocking", title: "Fix before rendering" },
  { severity: "warning", title: "Review" },
  { severity: "info", title: "Notes" },
];

const groups = computed(() =>
  GROUPS.map((g) => ({ ...g, items: checks.current.filter((f) => f.severity === g.severity) })).filter(
    (g) => g.items.length > 0,
  ),
);
</script>

<template>
  <p
    v-if="checks.currentError"
    role="alert"
    data-testid="checks-error"
    class="rounded-[9px] border border-danger/40 p-3.5 text-xs text-danger-fg"
  >
    Checks could not be read. {{ checks.currentError.message }}
  </p>
  <template v-else>
    <div
      data-testid="checks-summary"
      class="flex flex-col gap-[7px] rounded-[9px] border border-line bg-app p-3.5"
    >
      <b class="text-[13px] font-semibold text-fg">{{ checks.summary }}</b>
      <span class="text-[11px] text-fg-muted">These checks inspect the edit, not the meaning of your tutorial.</span>
    </div>
    <p
      v-if="groups.length === 0"
      data-testid="checks-empty"
      class="rounded-[7px] border border-accent/20 bg-accent-bg p-3.5 text-[11px] text-fg-secondary"
    >
      <b class="text-accent-ink">No structural issues found.</b>
      Watch the rendered file and check its sound, captions and private details before sharing.
    </p>
    <div v-else>
      <section
        v-for="group in groups"
        :key="group.severity"
        :data-testid="`checks-group-${group.severity}`"
        :aria-label="group.title"
      >
        <ul>
          <ChecksFindingRow
            v-for="finding in group.items"
            :key="finding.id"
            :finding="finding"
            @act="emit('act', finding)"
          />
        </ul>
      </section>
    </div>
  </template>
</template>
