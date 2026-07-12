<script setup lang="ts">
import { nextTick, ref } from "vue";

// A reusable tab container. Every panel is mounted and only the active one is
// shown (v-show), so each tab keeps its own self-contained load, a pending
// debounced save keeps running when you switch tabs (no unmount), and the
// content is in the DOM for tests to read. Slots are named by tab id.
const props = defineProps<{
  tabs: { id: string; label: string }[];
  initial?: string;
}>();

const active = ref(props.initial ?? props.tabs[0]?.id ?? "");

function select(id: string) {
  active.value = id;
}

// Roving-tabindex arrow-key navigation over the tab bar; wraps at both ends.
async function onKeydown(event: KeyboardEvent, index: number) {
  const n = props.tabs.length;
  let target: number;
  if (event.key === "ArrowRight" || event.key === "ArrowDown") target = (index + 1) % n;
  else if (event.key === "ArrowLeft" || event.key === "ArrowUp") target = (index - 1 + n) % n;
  else if (event.key === "Home") target = 0;
  else if (event.key === "End") target = n - 1;
  else return;
  event.preventDefault();
  // Capture the tab bar BEFORE awaiting: the DOM nulls event.currentTarget once
  // synchronous dispatch ends, so reading it after nextTick() would throw.
  const bar = (event.currentTarget as HTMLElement).parentElement;
  active.value = props.tabs[target].id;
  await nextTick();
  // Move focus to the newly selected tab so keyboard nav keeps flowing.
  bar?.querySelectorAll<HTMLElement>('[role="tab"]')[target]?.focus();
}
</script>

<template>
  <div>
    <div
      role="tablist"
      class="mb-3 flex gap-1 border-b border-white/10"
    >
      <button
        v-for="(t, i) in tabs"
        :id="`tab-${t.id}`"
        :key="t.id"
        type="button"
        role="tab"
        :data-testid="`tab-${t.id}`"
        :aria-selected="active === t.id"
        :aria-controls="`panel-${t.id}`"
        :tabindex="active === t.id ? 0 : -1"
        class="-mb-px cursor-pointer border-b-2 px-2 py-1 text-xs font-semibold transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="
          active === t.id
            ? 'border-violet-400 text-slate-100'
            : 'border-transparent text-slate-400 hover:text-slate-200'
        "
        @click="select(t.id)"
        @keydown="onKeydown($event, i)"
      >
        {{ t.label }}
      </button>
    </div>
    <div
      v-for="t in tabs"
      v-show="active === t.id"
      :id="`panel-${t.id}`"
      :key="t.id"
      role="tabpanel"
      :data-testid="`panel-${t.id}`"
      :aria-labelledby="`tab-${t.id}`"
    >
      <slot :name="t.id" />
    </div>
  </div>
</template>
