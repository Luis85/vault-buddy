<script setup lang="ts">
import { computed, ref, watch } from "vue";

import { parseTagsInput } from "../utils/taskFields";
import SelectMenu from "./SelectMenu.vue";
import TaskListPicker from "./TaskListPicker.vue";

// Presentational add-task composer: owns its own draft field state (title,
// due, priority, tags, list, showAddOptions, and — in aggregate mode — the
// selected vault). No store/invoke access: it reports a parsed payload up via
// `submit` and the container resolves the real target vault, validates, and
// writes; list creation is likewise emitted up (`createList`).
const props = defineProps<{
  isAggregate: boolean;
  vaultOptions: { value: string; label: string }[];
  adding: boolean;
  /** The target vault's lists (display order) and configured default list —
   * the container feeds the CURRENT target vault's values. */
  lists: string[];
  defaultList: string;
  creatingList: boolean;
}>();
const emit = defineEmits<{
  (
    e: "submit",
    payload: {
      title: string;
      due: string;
      priority: string;
      tags: string[];
      // undefined = "let the backend apply the configured default" — see submit().
      list: string | undefined;
      vaultId: string | null;
    },
  ): void;
  (e: "createList", name: string): void;
  (e: "vaultChange", vaultId: string): void;
}>();

const title = ref("");
const showAddOptions = ref(false);
const addDue = ref("");
const addPriority = ref("normal");
const addTags = ref("");
// The user's explicit list pick ("" = an explicit No-list override); only
// meaningful once listTouched. An UNTOUCHED picker derives its displayed
// value from the vault's configured default REACTIVELY (displayList below),
// rather than mirroring it into a ref via a watcher — a watcher only fires
// on a value CHANGE, so switching between two vaults whose default is the
// same string left the ref stale, showing "No list"/a carried-over pick
// while the backend used the real default (Codex, PR #53 re-review). The
// computed always tracks the current default, so display and effective
// target can't drift.
const addList = ref("");
const listTouched = ref(false);
const displayList = computed(() => (listTouched.value ? addList.value : props.defaultList));
function onListPicked(list: string) {
  listTouched.value = true;
  addList.value = list;
}
// The container resolves the created list and re-selects it here.
function setList(list: string) {
  listTouched.value = true;
  addList.value = list;
}
// Aggregate add: which vault receives the new task. Defaults to the first
// vault once the options arrive (they load async in the container), re-homing
// if the current pick vanishes; component-local, no persistence across opens.
// The container listens for changes to feed this vault's lists/default in.
const addVaultId = ref("");
watch(
  () => props.vaultOptions,
  (opts) => {
    if (!opts.some((o) => o.value === addVaultId.value)) addVaultId.value = opts[0]?.value ?? "";
  },
  { immediate: true },
);
watch(addVaultId, (id) => {
  if (id) {
    // A new target vault means a new lists universe — drop the manual pick so
    // displayList falls back to the new vault's configured default (which the
    // container feeds in via props.defaultList). No addList assignment: the
    // computed tracks the default reactively, so there is no stale ref to
    // clear or repopulate.
    listTouched.value = false;
    emit("vaultChange", id);
  }
});

function submit() {
  emit("submit", {
    title: title.value,
    due: addDue.value,
    priority: addPriority.value,
    // Client-side lenient parse; the shell strictly validates the charset.
    tags: parseTagsInput(addTags.value),
    // Send a list ONLY when the user explicitly picked one ("" = an explicit
    // No-list override). An untouched picker is merely mirroring the vault's
    // configured default — omit it (undefined) so add_task's list: None path
    // applies that default through its read-LENIENT branch, which degrades a
    // hand-edited unsafe default (".hidden", "../x", …) to the tasks root
    // instead of failing. Sending the resolved default here would force it
    // onto the write-STRICT path and error every quick add until the setting
    // was repaired (Codex, PR #53 re-review). This also covers the
    // config-not-yet-loaded and vault-switch windows: untouched ⇒ omit.
    list: listTouched.value ? addList.value : undefined,
    // The container uses props.vaultId in single-vault mode; only the aggregate
    // picker's value is meaningful here.
    vaultId: props.isAggregate ? addVaultId.value : null,
  });
}

function onTitleEnter(e: KeyboardEvent) {
  // GAP-31: committing an IME candidate fires Enter with isComposing=true —
  // that must select the candidate, never create a task document (a vault
  // write) from the half-composed title. The Search view's handlers are the
  // precedent. preventDefault lives HERE, after the guard (mirrors TaskEditor),
  // so it never cancels the candidate-commit Enter's default.
  if (e.isComposing) return;
  e.preventDefault();
  submit();
}

// Cleared by the container after a SUCCESSFUL add only — a failed add keeps the
// user's input. The selected vault AND list are deliberately NOT reset, so a
// burst of adds into one list stays there.
function reset() {
  title.value = "";
  addDue.value = "";
  addPriority.value = "normal";
  addTags.value = "";
  showAddOptions.value = false;
}
defineExpose({ reset, setList });
</script>

<template>
  <div class="flex flex-col gap-2">
    <div class="flex items-center gap-1">
      <SelectMenu
        v-if="isAggregate"
        v-model="addVaultId"
        :options="vaultOptions"
        aria-label="Vault for the new task"
        data-testid="task-add-vault"
      />
      <input
        v-model="title"
        data-testid="task-input"
        type="text"
        placeholder="Add a task…"
        aria-label="New task title"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
        @keydown.enter="onTitleEnter"
      >
      <button
        type="button"
        data-testid="task-add-options"
        :aria-label="showAddOptions ? 'Hide task options' : 'Set due date or priority'"
        :aria-expanded="showAddOptions"
        title="Due date / priority"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
        :class="showAddOptions ? 'border-violet-400 text-slate-100' : ''"
        @click="showAddOptions = !showAddOptions"
      >
        ⋯
      </button>
      <button
        type="button"
        data-testid="task-add"
        :disabled="adding || title.trim() === ''"
        class="shrink-0 cursor-pointer rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-sm text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
        @click="submit"
      >
        Add
      </button>
    </div>

    <div
      v-if="showAddOptions"
      class="flex items-center gap-1"
    >
      <input
        v-model="addDue"
        data-testid="task-add-due"
        type="date"
        aria-label="Due date"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 focus:border-violet-400 focus:outline-none"
      >
      <div
        class="flex gap-0.5"
        role="radiogroup"
        aria-label="Priority"
      >
        <button
          v-for="p in ['high', 'normal', 'low']"
          :key="p"
          type="button"
          role="radio"
          :data-testid="`task-add-priority-${p}`"
          :aria-checked="addPriority === p"
          class="cursor-pointer rounded-lg border px-1.5 py-0.5 text-[10px] capitalize transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :class="
            addPriority === p
              ? 'border-violet-400 bg-violet-500/20 text-slate-100'
              : 'border-white/10 bg-white/5 text-slate-300 hover:bg-white/10'
          "
          @click="addPriority = p"
        >
          {{ p }}
        </button>
      </div>
      <input
        v-model="addTags"
        data-testid="task-add-tags"
        type="text"
        placeholder="#tags"
        aria-label="Tags"
        class="min-w-0 flex-1 rounded-lg border border-white/10 bg-white/5 px-2 py-1 text-xs text-slate-100 placeholder:text-slate-500 focus:border-violet-400 focus:outline-none"
      >
    </div>

    <div
      v-if="showAddOptions"
      class="flex items-center gap-1"
    >
      <span class="shrink-0 text-[10px] uppercase tracking-wider text-slate-500">List</span>
      <TaskListPicker
        :model-value="displayList"
        :lists="lists"
        :busy="creatingList"
        aria-label="List for the new task"
        data-testid="task-add-list"
        @update:model-value="onListPicked"
        @create="emit('createList', $event)"
      />
    </div>
  </div>
</template>
