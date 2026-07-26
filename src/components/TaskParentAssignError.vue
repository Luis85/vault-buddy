<script setup lang="ts">
import AppButton from "./ui/AppButton.vue";
import Banner from "./ui/Banner.vue";

// Extracted out of TaskParentRow.vue (Fix 1, PR #78) purely to keep that
// template's cyclomatic/cognitive complexity under the quality-ratchet
// threshold — adding this branch inline pushed TaskParentRow's <template>
// over it. Presentational only: TaskDetail's loadListsConfig owns the
// retry logic, TaskParentRow owns the open/close disclosure; this renders
// the failure and forwards one `retry` emit, nothing else.
//
// Unconditionally MOUNTED by the caller (no `v-if` at the call site) — the
// `v-if="error"` lives in THIS component's own template instead. Extracting
// the markup alone left the caller's branch COUNT unchanged (a `v-if` on the
// call site costs the same whether it wraps a `<div>` or a child component),
// which is what still tripped the ratchet after the first extraction attempt
// — moving the branch itself, not just the markup, is what actually lowers
// TaskParentRow's own complexity score.
defineProps<{
  /** The raw error text from the failed config read — shown verbatim so a
   * report/screenshot carries the actual failure, matching how sibling
   * settings cards (e.g. DocumentImportSettings) surface a failed probe.
   * `null` renders nothing. */
  error: string | null;
  /** True while the retry (or the initial load) is in flight — disables the
   * button so a second click can't queue up a concurrent read. */
  retrying: boolean;
}>();
const emit = defineEmits<{ (e: "retry"): void }>();
</script>

<template>
  <div
    v-if="error"
    class="flex w-full flex-wrap items-center gap-1.5"
  >
    <Banner
      tone="danger"
      data-testid="task-detail-parent-error"
      class="flex-1"
    >
      Couldn't check archived lists — assigning a parent is on hold. {{ error }}
    </Banner>
    <AppButton
      variant="secondary"
      size="sm"
      data-testid="task-detail-parent-retry"
      :disabled="retrying"
      @click="emit('retry')"
    >
      Retry
    </AppButton>
  </div>
</template>
