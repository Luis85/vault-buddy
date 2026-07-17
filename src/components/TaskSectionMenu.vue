<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";

// The ⋯ menu on a Lists-grouping section header: Rename / Archive / Delete the
// list. Presentational — it owns only its open/sub-state; the container runs
// the commands and bumps `resetNonce` on success to close the popover (the
// TaskViewControls resetNonce precedent, Codex PR #59). Rename edits the
// list's LEAF (rename_task_list renames the leaf at the same parent) and
// mirrors TaskListPicker's inline input: IME-guarded Enter, and Escape that
// stops propagation so it doesn't bubble to the panel's own Escape handler.
const props = defineProps<{ list: string; busy: boolean; resetNonce?: number }>();
const emit = defineEmits<{
  (e: "rename", name: string): void;
  (e: "archive"): void;
  (e: "delete"): void;
}>();

type Mode = "closed" | "menu" | "rename" | "confirmDelete";
const mode = ref<Mode>("closed");
const leaf = computed(() => props.list.split("/").pop() || props.list);
const renameValue = ref("");
const root = ref<HTMLElement | null>(null);

function toggle() {
  mode.value = mode.value === "closed" ? "menu" : "closed";
}
function close() {
  mode.value = "closed";
}
function startRename() {
  renameValue.value = leaf.value;
  mode.value = "rename";
}
function confirmRename() {
  if (props.busy) return;
  const to = renameValue.value.trim();
  // Unchanged or empty is a no-op — the core rename refuses a same-name/self
  // collision (Task 2 review), so don't even ask it to.
  if (!to || to === leaf.value) {
    close();
    return;
  }
  emit("rename", to); // the parent runs it; a success bumps resetNonce → close
}
function confirmDelete() {
  if (props.busy) return;
  emit("delete");
}
function onRenameEnter(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.preventDefault();
  confirmRename();
}
function onRenameEscape(e: KeyboardEvent) {
  if (e.isComposing) return;
  e.stopPropagation();
  mode.value = "menu";
}
// The parent bumps resetNonce after a successful command → close.
watch(
  () => props.resetNonce,
  () => close(),
);
// A click outside the open popover closes it. Guarded on being open so idle
// section menus don't each field every window click.
function onWindowPointerDown(e: PointerEvent) {
  if (mode.value === "closed") return;
  if (root.value && !root.value.contains(e.target as Node)) close();
}
onMounted(() => window.addEventListener("pointerdown", onWindowPointerDown));
onBeforeUnmount(() => window.removeEventListener("pointerdown", onWindowPointerDown));

const itemClass =
  "cursor-pointer rounded px-1.5 py-0.5 text-left text-[10px] text-slate-300 transition-colors hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40";
</script>

<template>
  <div
    ref="root"
    class="relative inline-flex"
  >
    <button
      type="button"
      :data-testid="`task-section-menu-${list}`"
      :aria-label="`List actions for ${list}`"
      title="List actions"
      class="cursor-pointer rounded px-1 leading-none text-slate-500 transition-colors hover:bg-white/10 hover:text-slate-300 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
      @click.stop="toggle"
    >
      ⋯
    </button>
    <div
      v-if="mode !== 'closed'"
      class="absolute right-0 top-full z-10 mt-1 flex min-w-36 flex-col gap-0.5 rounded-lg border border-white/10 bg-slate-800 p-1 shadow-lg"
      @click.stop
    >
      <template v-if="mode === 'menu'">
        <button
          type="button"
          :data-testid="`task-section-rename-${list}`"
          :class="itemClass"
          @click="startRename"
        >
          Rename
        </button>
        <button
          type="button"
          :data-testid="`task-section-archive-${list}`"
          :disabled="busy"
          :class="itemClass"
          @click="$emit('archive')"
        >
          Archive
        </button>
        <button
          type="button"
          :data-testid="`task-section-delete-${list}`"
          :class="itemClass"
          @click="mode = 'confirmDelete'"
        >
          Delete
        </button>
      </template>
      <template v-else-if="mode === 'rename'">
        <input
          v-model="renameValue"
          :data-testid="`task-section-rename-input-${list}`"
          type="text"
          aria-label="New list name"
          class="min-w-0 rounded border border-white/10 bg-white/5 px-1.5 py-0.5 text-[10px] text-slate-100 focus:border-violet-400 focus:outline-none"
          @keydown.enter="onRenameEnter"
          @keydown.esc="onRenameEscape"
        >
        <div class="flex gap-0.5">
          <button
            type="button"
            :data-testid="`task-section-rename-confirm-${list}`"
            :disabled="busy || renameValue.trim() === ''"
            :class="itemClass"
            @click="confirmRename"
          >
            Save
          </button>
          <button
            type="button"
            :class="itemClass"
            @click="mode = 'menu'"
          >
            Cancel
          </button>
        </div>
      </template>
      <template v-else>
        <span class="px-1.5 py-0.5 text-[10px] text-slate-400">
          Delete "{{ leaf }}"? Its tasks move to No list.
        </span>
        <div class="flex gap-0.5">
          <button
            type="button"
            :data-testid="`task-section-delete-confirm-${list}`"
            :disabled="busy"
            class="cursor-pointer rounded px-1.5 py-0.5 text-[10px] text-red-300 transition-colors hover:bg-red-500/20 focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400 disabled:cursor-default disabled:opacity-40"
            @click="confirmDelete"
          >
            Delete
          </button>
          <button
            type="button"
            :class="itemClass"
            @click="mode = 'menu'"
          >
            Cancel
          </button>
        </div>
      </template>
    </div>
  </div>
</template>
