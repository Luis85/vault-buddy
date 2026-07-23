<script setup lang="ts">
// Presentational "Task IDs" card for the Tasks settings tab: a toggle plus,
// once enabled, a property-name input and its field error. No store/invoke/
// autosave of its own — state, autosave scheduling, and the on-mount load
// all stay in TasksConfigTab.vue; this component only renders the card and
// emits the raw user input back up. Extracted (Task 9 review) so the
// parent's template drops back under fallow's complexity threshold without
// loosening the quality-ratchet baseline — mirrors this tab's own
// VaultFolderSetting.vue precedent of a presentational child card (unlike
// TaskListSettings.vue, which owns its own invoke/autosave/load and is not
// presentational).
defineProps<{
  enabled: boolean;
  property: string;
  error: string | null;
  // Default property name shown as the input's placeholder (never
  // pre-filled) — the parent is the single source of truth for this
  // default so it can't drift from the load-time ternary that decides
  // when to show a persisted value instead.
  placeholder: string;
}>();

const emit = defineEmits<{
  "update:enabled": [value: boolean];
  "update:property": [value: string];
  blur: [];
}>();
</script>

<template>
  <section>
    <h2 class="mb-1.5 text-xs font-semibold uppercase tracking-wide text-fg-muted">
      Task IDs
    </h2>
    <div class="rounded-xl border border-white/10 bg-white/5 p-2">
      <div class="flex items-center justify-between gap-2">
        <label
          for="task-id-enabled"
          class="text-sm text-slate-200"
        >
          Generate an ID for each task
          <span class="block text-xs text-fg-subtle">Written to new tasks and stamped on the next edit</span>
        </label>
        <input
          id="task-id-enabled"
          data-testid="task-id-enabled"
          type="checkbox"
          class="h-4 w-4 accent-violet-500"
          :checked="enabled"
          @change="emit('update:enabled', ($event.target as HTMLInputElement).checked)"
        >
      </div>
      <div
        v-if="enabled"
        class="mt-2"
      >
        <label
          for="task-id-property"
          class="mb-1 block text-sm text-slate-200"
        >
          Property name
          <span class="block text-xs text-fg-subtle">Frontmatter key the ID is saved under</span>
        </label>
        <input
          id="task-id-property"
          data-testid="task-id-property"
          type="text"
          :placeholder="placeholder"
          class="w-full rounded-control border border-white/10 bg-white/5 px-2 py-1 text-sm text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
          :value="property"
          @input="emit('update:property', ($event.target as HTMLInputElement).value)"
          @blur="emit('blur')"
        >
        <p
          v-if="error"
          data-testid="task-id-error"
          class="mt-1 text-xs text-danger-fg"
        >
          {{ error }}
        </p>
      </div>
    </div>
  </section>
</template>
