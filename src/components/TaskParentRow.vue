<script setup lang="ts">
import { computed, nextTick, ref } from "vue";

import type { AggTask } from "../types";
import TaskParentPicker from "./TaskParentPicker.vue";
import Chip from "./ui/Chip.vue";

// The Task Detail Parent row (Task 8): the parent's title as a clickable chip
// (navigation only — the caller decides what that means), Change (opens the
// search picker below) / Clear, or "Set parent" when none is resolved yet.
// Self-contained like TaskScheduleMenu/TaskSectionMenu: owns its own
// open/close + focus/Escape discipline (GAP-27 class — swallow only ITS OWN
// Escape, take focus on open, return it to Change on a keyboard close) so
// TaskDetail.vue stays a single-line caller instead of growing its own
// template past the complexity gate (fallow flagged the inline version).
const props = defineProps<{
  parent: AggTask | null;
  busy: boolean;
  allTasks: AggTask[];
  invalidPaths: string[];
  /** False while the vault's archived-list set is still being read. Gates
   * ASSIGNMENT only — Clear stays available, since removing a relationship
   * never needs the archived set. The frontend is the SOLE enforcement point
   * for the archived-list rule (core deliberately validates only archived
   * STATUS), so offering candidates before that set is known would let a pick
   * write a real, persisted relationship the rule should have excluded
   * (Codex P2, PR #78). */
  canAssign: boolean;
}>();
const emit = defineEmits<{
  (e: "openParent"): void;
  (e: "select", path: string | null): void;
}>();

const changing = ref(false);
const changeBtn = ref<HTMLButtonElement | null>(null);
const picker = ref<InstanceType<typeof TaskParentPicker> | null>(null);

// One combined status label instead of two template-level v-if/v-else-if
// branches (fallow flagged the extra branch — extracted here, matching the
// class-level comment above about keeping this component's own template
// simple). An archived parent still resolves (Fix 1) but must not render
// identically to an active one — a silent "no relationship" read is exactly
// the misreading that fix closed, so it stays legible; `null` means the chip
// alone (an active parent) already says everything there is to say.
const statusLabel = computed(() => {
  if (!props.parent) return "No parent";
  if (props.parent.status === "archived") return "(archived)";
  return null;
});

async function open() {
  // Defensive: the trigger below is already :disabled="busy || !canAssign" (a
  // disabled native button can't dispatch click), so this only matters if that
  // ever drifts — same posture as the sibling Save/Delete guards in TaskDetail.
  if (props.busy || !props.canAssign) return;
  changing.value = true;
  await nextTick();
  picker.value?.focus();
}
async function close() {
  changing.value = false;
  await nextTick();
  changeBtn.value?.focus();
}
// Close FIRST, then let the caller write: mirrors TaskScheduleMenu's choose()
// (emit, then close+refocus) — this row's job ends the moment a pick is
// made; the actual write's busy guard lives in the caller (useTaskDetail's
// shared ref), not here.
function onSelect(path: string | null) {
  void close();
  emit("select", path);
}
function onRootKeydown(e: KeyboardEvent) {
  // GAP-27 class: swallow Escape only while the picker is open, so a closed
  // row lets it bubble to the panel's own close-on-Escape handler.
  if (e.key !== "Escape" || !changing.value) return;
  e.stopPropagation();
  void close();
}
</script>

<template>
  <div
    class="flex flex-col gap-1"
    @keydown="onRootKeydown"
  >
    <span class="text-micro uppercase tracking-wider text-fg-subtle">Parent</span>
    <div
      v-if="!changing"
      class="flex flex-wrap items-center gap-1.5"
    >
      <Chip
        v-if="parent"
        variant="interactive"
        data-testid="task-detail-parent-chip"
        :title="`Open &quot;${parent.title}&quot;`"
        @click="emit('openParent')"
      >
        {{ parent.title }}
      </Chip>
      <span
        v-if="statusLabel"
        data-testid="task-detail-parent-status"
        class="text-xs text-fg-muted"
      >{{ statusLabel }}</span>
      <button
        ref="changeBtn"
        type="button"
        data-testid="task-detail-parent-change"
        :disabled="busy || !canAssign"
        class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        @click="open"
      >
        {{ parent ? "Change" : "Set parent" }}
      </button>
      <button
        v-if="parent"
        type="button"
        data-testid="task-detail-parent-clear"
        :disabled="busy"
        class="cursor-pointer rounded-control border border-white/10 bg-white/5 px-2 py-0.5 text-xs text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default disabled:opacity-50"
        @click="emit('select', null)"
      >
        Clear
      </button>
    </div>
    <TaskParentPicker
      v-else
      ref="picker"
      :tasks="allTasks"
      :current-path="parent?.path ?? null"
      :invalid-paths="invalidPaths"
      @select="onSelect"
    />
  </div>
</template>
