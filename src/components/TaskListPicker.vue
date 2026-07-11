<script setup lang="ts">
import { ref, watch } from "vue";

import SelectMenu from "./SelectMenu.vue";

// Presentational list picker shared by the composer, the inline editor, and
// the vault settings: No list (the tasks root) + the lists in display order
// + an optional "New list…" flow. No store/invoke access — creation is
// emitted up and the PARENT selects the new list once it lands, which is
// also what pops this component out of new-list mode (the modelValue watch).
const props = withDefaults(
  defineProps<{
    modelValue: string;
    lists: string[];
    /** Disables the confirm while the parent's create call is in flight. */
    busy?: boolean;
    /** Bumped by the parent on a successful create to leave new-list mode even
     * when the re-selected value is unchanged (idempotent same-name create). */
    resetNonce?: number;
    /** The settings picker offers existing lists only. */
    allowCreate?: boolean;
    ariaLabel?: string;
    dataTestid?: string;
  }>(),
  {
    busy: false,
    allowCreate: true,
    ariaLabel: undefined,
    dataTestid: undefined,
    resetNonce: 0,
  },
);
const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "create", name: string): void;
}>();

// The "New list…" action rides the SelectMenu value channel, so its sentinel
// MUST NOT collide with a real list value. is_valid_list_name accepts any
// non-empty segment without a slash or leading dot, so a bare "__new__" is
// itself a creatable/hand-addable list — picking that real list would wrongly
// flip into create mode (and it couldn't be selected at all when allowCreate
// is false). A leading dot makes the sentinel uncollidable: the backend's
// task_lists walk skips dot-directories and create_task_list rejects leading
// dots, so a dot-led value can never appear in props.lists (Codex, PR #53
// re-review).
const NEW_SENTINEL = ".__new__";
const newMode = ref(false);
const newName = ref("");

const options = () => {
  const base = [
    { value: "", label: "No list" },
    ...props.lists.map((l) => ({ value: l, label: l })),
  ];
  if (props.allowCreate) base.push({ value: NEW_SENTINEL, label: "New list…" });
  return base;
};

function onPick(value: string | number) {
  // Gate on allowCreate too: only the create-enabled picker renders the
  // sentinel option, so nothing else can ever be mistaken for the action.
  if (props.allowCreate && value === NEW_SENTINEL) {
    newMode.value = true;
    newName.value = "";
    return;
  }
  emit("update:modelValue", String(value));
}

// The parent selecting the created list (a modelValue change) ends new-list
// mode. resetNonce covers the idempotent same-name create, where the parent
// re-selects the SAME value and modelValue never changes — the bump still
// closes the form. A failed create bumps neither, so it stays open for retry.
watch([() => props.modelValue, () => props.resetNonce], () => {
  newMode.value = false;
});

function confirmNew() {
  const name = newName.value.trim();
  if (!name || props.busy) return;
  emit("create", name);
}

function onNameEnter(e: KeyboardEvent) {
  // GAP-31 class: an IME candidate commit fires Enter with isComposing=true —
  // that selects the candidate, never fires the create.
  if (e.isComposing) return;
  e.preventDefault();
  confirmNew();
}

function onNameEscape(e: KeyboardEvent) {
  if (e.isComposing) return;
  // GAP-27 class: dismissing the inline input must not bubble to the
  // window handler that closes the whole panel.
  e.stopPropagation();
  newMode.value = false;
}
</script>

<template>
  <div
    v-if="newMode"
    class="flex min-w-0 items-center gap-1"
  >
    <input
      v-model="newName"
      :data-testid="dataTestid ? `${dataTestid}-new-name` : undefined"
      type="text"
      placeholder="List name"
      aria-label="New list name"
      class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      @keydown.enter="onNameEnter"
      @keydown.esc="onNameEscape"
    >
    <button
      type="button"
      :data-testid="dataTestid ? `${dataTestid}-new-confirm` : undefined"
      :disabled="busy || newName.trim() === ''"
      aria-label="Create list"
      title="Create list"
      class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
      @click="confirmNew"
    >
      ✓
    </button>
    <button
      type="button"
      :data-testid="dataTestid ? `${dataTestid}-new-cancel` : undefined"
      aria-label="Cancel new list"
      title="Cancel"
      class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      @click="newMode = false"
    >
      ✕
    </button>
  </div>
  <SelectMenu
    v-else
    :model-value="modelValue"
    :options="options()"
    :aria-label="ariaLabel ?? 'Task list'"
    :data-testid="dataTestid"
    @update:model-value="onPick"
  />
</template>
