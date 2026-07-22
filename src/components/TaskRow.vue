<script setup lang="ts">
import type { AggTask } from "../types";
import { dueOf, localToday } from "../utils/taskFields";
import AppIcon from "./AppIcon.vue";
import TaskDragHandle from "./TaskDragHandle.vue";
import Avatar from "./ui/Avatar.vue";
import Chip from "./ui/Chip.vue";
import IconButton from "./ui/IconButton.vue";
import StatusDot from "./ui/StatusDot.vue";

// Presentational task row: the container owns all state and side effects; this
// component only renders and reports intent up. When `editing`, it yields its
// body to the slot (the container places a TaskEditor there) so the inline
// editor's save/cancel bind to container handlers directly. `reorderable`
// shows the grip handle (Manual sort, no filters); the raw pointer/key
// events travel up — the container's reorder composable owns the drag.
// `reorderBusy` (a view-wide rank write in flight) makes the grip inert
// without unmounting it, so keyboard focus survives the write.
withDefaults(
  defineProps<{
    task: AggTask;
    busy: boolean;
    isAggregate: boolean;
    editing: boolean;
    reorderable?: boolean;
    reorderBusy?: boolean;
    dragging?: boolean;
    dropTarget?: boolean;
  }>(),
  { reorderable: false, reorderBusy: false, dragging: false, dropTarget: false },
);
defineEmits<{
  (e: "toggle"): void;
  (e: "archive"): void;
  (e: "edit"): void;
  (e: "open"): void;
  (e: "tagClick", tag: string): void;
  (e: "reorderPointerDown", ev: PointerEvent): void;
  (e: "reorderKeydown", ev: KeyboardEvent): void;
}>();

// Deterministic short label (no locale dependence): "Jul 15".
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
function dueLabel(d: string): string {
  const [, m, day] = d.split("-");
  const month = MONTHS[Number(m) - 1];
  return month ? `${month} ${Number(day)}` : d;
}
const isOverdue = (t: AggTask): boolean => {
  const d = dueOf(t);
  return d !== null && !t.done && d < localToday();
};
</script>

<template>
  <li
    data-testid="task-row"
    class="flex items-center gap-2 rounded-lg border bg-white/5 px-2 py-1"
    :class="[
      dragging ? 'opacity-50' : '',
      dropTarget ? 'border-violet-400' : 'border-white/10',
    ]"
  >
    <slot v-if="editing" />
    <template v-else>
      <TaskDragHandle
        v-if="reorderable"
        :title="task.title"
        :busy="busy || reorderBusy"
        @handle-pointer-down="$emit('reorderPointerDown', $event)"
        @handle-keydown="$emit('reorderKeydown', $event)"
      />
      <input
        type="checkbox"
        data-testid="task-checkbox"
        :checked="task.done"
        :disabled="busy"
        :aria-label="`Mark ${task.title} ${task.done ? 'not done' : 'done'}`"
        class="shrink-0 cursor-pointer accent-violet-500 disabled:cursor-default disabled:opacity-50"
        @change="$emit('toggle')"
      >
      <!-- The open button, tag chips, and due chip share one flex-1 group so
           the title truncates and the chips sit at the right — but the chips
           are SIBLINGS of the open button, not descendants. A focusable
           button nested in another button is invalid interactive content
           (Codex, PR #46): browsers expose it inconsistently and a chip
           activation could be swallowed by the parent open button. -->
      <div class="flex min-w-0 flex-1 items-center gap-1.5">
        <button
          type="button"
          data-testid="task-open"
          class="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-400"
          :aria-label="`Open ${task.title} in Obsidian`"
          :title="`Open ${task.title} in Obsidian`"
          @click="$emit('open')"
        >
          <Avatar
            v-if="isAggregate"
            :name="task.vaultName"
            size="sm"
            :title="task.vaultName"
            data-testid="task-vault"
          />
          <StatusDot
            v-if="task.priority === 'high' || task.priority === 'low'"
            :tone="task.priority === 'high' ? 'priority-high' : 'priority-low'"
            :title="task.priority === 'high' ? 'High priority' : 'Low priority'"
            data-testid="task-priority"
          />
          <span
            class="min-w-0 flex-1 truncate text-sm"
            :class="task.done ? 'text-slate-500 line-through' : 'text-slate-100'"
          >
            {{ task.title }}
          </span>
        </button>
        <Chip
          v-for="tag in task.tags"
          :key="tag"
          variant="interactive"
          :label="`Filter by tag ${tag}`"
          data-testid="task-tag"
          @click="$emit('tagClick', tag)"
        >
          #{{ tag }}
        </Chip>
        <span
          v-if="dueOf(task)"
          data-testid="task-due"
          class="shrink-0 text-[10px] tabular-nums"
          :class="isOverdue(task) ? 'font-semibold text-red-300' : 'text-slate-400'"
        >{{ dueLabel(dueOf(task)!) }}</span>
      </div>
      <IconButton
        size="sm"
        data-testid="task-edit"
        :disabled="busy"
        :label="`Edit ${task.title}`"
        title="Edit"
        @click="$emit('edit')"
      >
        <AppIcon :size="14">
          <path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z" />
        </AppIcon>
      </IconButton>
      <IconButton
        size="sm"
        data-testid="task-archive"
        :disabled="busy"
        :label="`Archive ${task.title}`"
        title="Archive"
        @click="$emit('archive')"
      >
        <AppIcon :size="14">
          <rect
            x="3"
            y="4"
            width="18"
            height="4"
            rx="1"
          />
          <path d="M5 8v11a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V8M10 12h4" />
        </AppIcon>
      </IconButton>
    </template>
  </li>
</template>
