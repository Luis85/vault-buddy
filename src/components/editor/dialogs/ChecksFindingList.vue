<script setup lang="ts">
/**
 * The Checks dialog's body (Task 54; SCREENS 07): the "N blockers · M
 * review warnings" summary, then the findings grouped Fix / Review / Note,
 * each row with its reveal button — or, when the read failed, that failure
 * and nothing that could read as a pass. Split out of `ChecksDialog` for
 * the template-complexity ratchet; the dialog owns what a row's button
 * does.
 */
import { computed } from "vue";

import type { CheckFinding, CheckSeverity } from "../../../editorTypes";
import { useEditorChecksStore } from "../../../stores/editorChecks";
import ChecksFindingRow from "./ChecksFindingRow.vue";

const emit = defineEmits<{ (e: "act", finding: CheckFinding): void }>();

const checks = useEditorChecksStore();

const GROUPS: { severity: CheckSeverity; title: string; tag: string }[] = [
  { severity: "blocking", title: "Fix before rendering", tag: "FIX" },
  { severity: "warning", title: "Review", tag: "REVIEW" },
  { severity: "info", title: "Notes", tag: "NOTE" },
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
    class="rounded-control border border-danger/40 p-2 text-xs text-danger-fg"
  >
    Checks could not be read. {{ checks.currentError.message }}
  </p>
  <template v-else>
    <div
      data-testid="checks-summary"
      class="flex flex-col gap-0.5 rounded-control border border-line p-3"
    >
      <p class="text-sm font-semibold text-fg">
        {{ checks.summary }}
      </p>
      <p class="text-xs text-fg-muted">
        These checks inspect the edit, not the meaning of your tutorial.
      </p>
    </div>
    <p
      v-if="groups.length === 0"
      data-testid="checks-empty"
      class="text-xs text-fg-secondary"
    >
      Nothing to fix or review. Watch the rendered file and check its sound,
      captions and private details before sharing.
    </p>
    <section
      v-for="group in groups"
      :key="group.severity"
      :data-testid="`checks-group-${group.severity}`"
      :aria-label="group.title"
    >
      <h3 class="text-xs font-semibold text-fg-secondary">
        {{ group.title }}
      </h3>
      <ul>
        <ChecksFindingRow
          v-for="finding in group.items"
          :key="finding.id"
          :finding="finding"
          :tag="group.tag"
          @act="emit('act', finding)"
        />
      </ul>
    </section>
  </template>
</template>
