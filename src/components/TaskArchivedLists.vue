<script setup lang="ts">
// The archived-lists rows in the Vault settings Tasks tab: each archived list
// with an Unarchive button. Presentational — the parent owns the archived set
// and the save; extracted so TaskListSettings' template stays under the
// complexity threshold. Renders nothing when there are none.
defineProps<{ lists: string[] }>();
defineEmits<{ (e: "unarchive", list: string): void }>();
</script>

<template>
  <template v-if="lists.length > 0">
    <p class="mb-1 mt-2 text-sm text-slate-200">
      Archived lists
      <span class="block text-xs text-fg-subtle">Hidden from the tasks view; unarchive to bring one back</span>
    </p>
    <ul class="flex flex-col gap-1">
      <li
        v-for="list in lists"
        :key="list"
        data-testid="archived-list-row"
        class="flex items-center gap-1 rounded-control border border-white/10 bg-white/5 px-2 py-0.5"
      >
        <span class="min-w-0 flex-1 truncate text-sm text-fg-muted">{{ list }}</span>
        <button
          type="button"
          :data-testid="`unarchive-${list}`"
          :aria-label="`Unarchive ${list}`"
          class="cursor-pointer rounded px-1.5 py-0.5 text-xs text-fg-secondary transition-colors hover:bg-white/10 hover:text-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="$emit('unarchive', list)"
        >
          Unarchive
        </button>
      </li>
    </ul>
  </template>
</template>
