<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { computed, onMounted, ref } from "vue";

import { useTaskActions } from "../composables/useTaskActions";
import { useTaskDisplay } from "../composables/useTaskDisplay";
import { useTaskLists } from "../composables/useTaskLists";
import { useTaskReorder } from "../composables/useTaskReorder";
import { useTaskReorderCommit } from "../composables/useTaskReorderCommit";
import { logWarning } from "../logging";
import { useNotificationsStore } from "../stores/notifications";
import { useVaultsStore } from "../stores/vaults";
import type { AggTask, TaskItem, Vault } from "../types";
import { crossListDropTargetKey } from "../utils/taskSections";
import AppIcon from "./AppIcon.vue";
import TaskComposer from "./TaskComposer.vue";
import TaskEditor from "./TaskEditor.vue";
import TaskRow from "./TaskRow.vue";
import TaskSectionMenu from "./TaskSectionMenu.vue";
import TaskViewControls from "./TaskViewControls.vue";
import Banner from "./ui/Banner.vue";
import EmptyState from "./ui/EmptyState.vue";

const props = defineProps<{ vaultId: string | null }>();
// Aggregate mode: one merged view across every vault (vaultId === null).
const isAggregate = computed(() => props.vaultId === null);

const notifications = useNotificationsStore();
const vaultsStore = useVaultsStore();

const loading = ref(true);
const loadError = ref<string | null>(null);
const tasks = ref<AggTask[]>([]);
const allVaults = ref<Vault[]>([]);
const adding = ref(false);
// The add composer owns its own draft field state and reports a parsed payload
// up via `submit`; the container keeps validation + the write + the reset call.
const composer = ref<InstanceType<typeof TaskComposer> | null>(null);
const vaultOptions = computed(() =>
  allVaults.value.map((v) => ({ value: v.id, label: v.name })),
);
// The view's state + IPC live in composables (LOC/churn split); rendering
// stays here. Order matters: the lists feed the display pipeline's buckets,
// and the pipeline's sortInPlace feeds the row/reorder writes.
const {
  listOrder,
  knownLists,
  archivedLists,
  creatingList,
  composerVaultId,
  composerLists,
  composerDefaultList,
  listsForEditor,
  loadVaultLists,
  loadVaultConfig,
  onComposerVaultChange,
  createList,
  renameList,
  deleteList,
  archiveList,
  // A rename/archive/delete of the composer's picked list must remap or clear
  // that pick, else the next Add would recreate the renamed folder or land in a
  // hidden/gone list (Codex, PR #59). The composer owns the touched-pick state.
} = useTaskLists(props.vaultId, (from, to) => composer.value?.remapPick(from, to));
// Read side: filter / sort / group → buckets (+ the shared sortInPlace).
const sortViewKey = props.vaultId ?? "all";
const {
  filter,
  tagFilter,
  showFilter,
  filterActive,
  sortPref,
  sortInPlace,
  setSortKey,
  flipSortDir,
  grouping,
  buckets,
  hasDisplayableLists,
  progress,
} = useTaskDisplay({ tasks, isAggregate, knownLists, listOrder, archivedLists, sortViewKey });
// Write side: row actions (toggle/archive/open/editor save) + the busy guard.
const {
  busy,
  isBusy,
  toggle,
  archive,
  openInObsidian,
  editingKey,
  rowKey,
  startEdit,
  cancelEdit,
  onEditorSave,
} = useTaskActions({ tasks, sortInPlace });

// New list flow: create + cache, then re-select here. `target` (the vault
// createList used) blocks a mid-create composer vault switch from adopting the
// old vault's list on the new one (Codex, PR #53).
async function onCreateList(name: string) {
  const target = composerVaultId.value;
  const created = await createList(name);
  if (created !== null && composerVaultId.value === target) composer.value?.setList(created);
}

// The Lists-view "New list" control: create + cache in this vault's lists so
// the empty section appears immediately (createList is composerVaultId ??
// vaultId scoped — in per-vault mode that is this vault). Failures are toasted
// by the composable. On SUCCESS bump the nonce so the control closes + clears
// its inline form; a failed create leaves the draft open for a retry (Codex
// PR #59).
const controlsListResetNonce = ref(0);
async function onControlsCreateList(name: string) {
  const created = await createList(name);
  if (created !== null) controlsListResetNonce.value += 1;
}

// The Lists-view section menu (rename/archive/delete). A per-list in-flight
// guard disables the menu during its write; one shared nonce closes the open
// popover on success (only one is open at a time — the TaskViewControls
// precedent).
const sectionBusy = ref(new Set<string>());
const sectionMenuResetNonce = ref(0);

// Re-fetch this vault's tasks after a rename/delete relocates files on disk.
// Per-vault only (the section menu is hidden in the aggregate).
async function reloadTasks() {
  if (props.vaultId === null) return;
  const id = props.vaultId;
  try {
    const items = await invoke<TaskItem[]>("list_tasks", { id });
    tasks.value = items.map((t) => ({ ...t, vaultId: id, vaultName: "" }));
    sortInPlace();
  } catch (e) {
    logWarning(`list_tasks reload failed for vault ${id}: ${String(e)}`);
  }
}

async function runSectionAction(
  list: string,
  action: () => Promise<boolean>,
  reload: "onSuccess" | "always" | "never",
) {
  if (sectionBusy.value.has(list)) return;
  sectionBusy.value = new Set(sectionBusy.value).add(list);
  try {
    const ok = await action();
    if (ok) sectionMenuResetNonce.value += 1;
    if (reload === "always" || (reload === "onSuccess" && ok)) await reloadTasks();
  } finally {
    const next = new Set(sectionBusy.value);
    next.delete(list);
    sectionBusy.value = next;
  }
}
const onSectionRename = (list: string, to: string) =>
  runSectionAction(list, async () => (await renameList(list, to)) !== null, "onSuccess");
const onSectionArchive = (list: string) => runSectionAction(list, () => archiveList(list), "never");
// GAP-64: delete moves tasks one-by-one and can leave a PARTIAL state even on
// failure, so reload regardless — "Err ⇒ nothing happened" is false here.
const onSectionDelete = (list: string) => runSectionAction(list, () => deleteList(list), "always");

// Manual ordering: drag (or arrow-key) a row within its section; the landed
// slot becomes an `order` rank — one midpoint write in the common case, a
// section-wide materialization when ranks need seeding. Grips render in Manual
// sort with NO filter (a filtered subset would rank against invisible
// neighbors) and stay MOUNTED but inert during a rank write — unmounting (or
// `disabled`) drops keyboard focus on every Arrow step. The write side (rank
// writes + cross-list move) lives in useTaskReorderCommit; this view owns the
// interaction machine (useTaskReorder) and the DOM hit-tests.
const rootRef = ref<HTMLElement | null>(null);
const { reordering, commitReorder } = useTaskReorderCommit({ busy, sortInPlace, buckets, grouping });
// `filterActive` (not a hand-rolled empty-string check) is the gate: it
// applies the same showFilter rule the list itself uses, so STALE filter text
// left behind when archiving hid the input no longer blocks reordering — the
// list is unfiltered then, every neighbor visible (review, PR #59).
const reorderView = computed(
  () => !isAggregate.value && sortPref.value.key === "manual" && !filterActive.value,
);
const reorderEnabled = computed(() => reorderView.value && !reordering.value);
const { dragState, onHandlePointerDown, onHandleKeydown } = useTaskReorder({
  enabled: () => reorderEnabled.value,
  // Filter by dataset rather than interpolating sectionKey into an attribute
  // selector: a Lists-grouping key is `list:<name>`, and a list folder name
  // may contain a double quote (is_valid_list_name only bars /, \, leading
  // dot), which would make the selector invalid and throw. querySelectorAll
  // returns document order, and a section's rows are contiguous, so the
  // filtered list stays in visual order.
  rowsFor: (sectionKey) =>
    rootRef.value
      ? ([...rootRef.value.querySelectorAll<HTMLElement>("[data-reorder-section]")].filter(
          (el) => el.dataset.reorderSection === sectionKey,
        ))
      : [],
  // Which section wrapper (by key) sits under the pointer — hit-test the
  // rendered section containers so a drop onto another list's section becomes
  // a cross-list move (Task 11).
  sectionAt: (x, y) => {
    if (!rootRef.value) return null;
    for (const el of rootRef.value.querySelectorAll<HTMLElement>("[data-section-key]")) {
      const r = el.getBoundingClientRect();
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return el.dataset.sectionKey ?? null;
    }
    return null;
  },
  commit: commitReorder,
});

// The section a cross-list drop would land on — drives its highlight and
// suppresses the origin's now-misleading drop line (pure util, tested).
const crossListDropTarget = computed(() =>
  crossListDropTargetKey(dragState.value, grouping.value, buckets.value),
);

onMounted(async () => {
  try {
    if (props.vaultId !== null) {
      const id = props.vaultId;
      const [items] = await Promise.all([
        invoke<TaskItem[]>("list_tasks", { id }),
        // Lists + config feed the Lists grouping and the composer's picker;
        // a failed read degrades (log-only, same posture as the tasks load).
        loadVaultLists(id),
        loadVaultConfig(id),
      ]);
      tasks.value = items.map((t) => ({ ...t, vaultId: id, vaultName: "" }));
      // Core hands back Default order; a persisted non-default sort must
      // apply to the initial load too, not only after edits.
      sortInPlace();
    } else {
      // Aggregate: fan out over every vault, best-effort per vault — the
      // same posture as the store's taskCounts load. A failed vault
      // contributes nothing and is named in ONE toast; the blocking banner
      // is reserved for list_vaults failing or EVERY vault failing.
      const vaults = await invoke<Vault[]>("list_vaults");
      allVaults.value = vaults;
      const failed: string[] = [];
      const results = await Promise.all(
        vaults.map(async (v) => {
          // The list enumeration rides the same fan-out (its own catch —
          // a lists failure must not mark the vault's TASKS as failed).
          void loadVaultLists(v.id);
          try {
            const items = await invoke<TaskItem[]>("list_tasks", { id: v.id });
            return items.map((t) => ({ ...t, vaultId: v.id, vaultName: v.name }));
          } catch (e) {
            failed.push(v.name);
            logWarning(`list_tasks failed for vault ${v.id}: ${String(e)}`);
            return [];
          }
        }),
      );
      if (vaults.length > 0 && failed.length === vaults.length) {
        loadError.value = "Couldn't load tasks from any vault.";
      } else {
        tasks.value = results.flat();
        sortInPlace();
        if (failed.length > 0) {
          notifications.error(`Couldn't load tasks from ${failed.join(", ")}.`);
        }
      }
    }
  } catch (e) {
    loadError.value = String(e);
  } finally {
    loading.value = false;
  }
});

type AddPayload = {
  title: string;
  due: string;
  priority: string;
  tags: string[];
  // undefined = the composer had no explicit pick and its default hadn't
  // loaded — let the backend apply the configured default (see the composer).
  list: string | undefined;
  vaultId: string | null;
};

async function add(payload: AddPayload) {
  const title = payload.title.trim();
  // Aggregate mode resolves the target from the payload's picked vault; a
  // single-vault view always writes to its own vault.
  const targetVault = isAggregate.value ? payload.vaultId : props.vaultId;
  if (!title || adding.value || !targetVault) return;
  adding.value = true;
  try {
    const args: Record<string, unknown> = { id: targetVault, title };
    if (payload.due) args.due = payload.due;
    if (payload.priority !== "normal") args.priority = payload.priority;
    if (payload.tags.length > 0) args.tags = payload.tags;
    // A defined list (incl. "" = an explicit No list override) is sent as-is;
    // undefined is omitted so the backend applies the vault's configured
    // default — the composer only omits it before its default has loaded, so
    // a quick add during that window still lands in the default list.
    if (payload.list !== undefined) args.list = payload.list;
    const created = await invoke<TaskItem>("add_task", args);
    tasks.value.unshift({
      ...created,
      vaultId: targetVault,
      vaultName: allVaults.value.find((v) => v.id === targetVault)?.name ?? "",
    });
    sortInPlace();
    // GAP-32 / Codex PR #46: the vault-row badge only reloaded on
    // panel-shown, going stale after an add until the panel reopened.
    void vaultsStore.refreshTaskCount(targetVault);
    // Fields clear only on success — a failed add keeps the user's input.
    composer.value?.reset();
  } catch (e) {
    notifications.error(String(e));
    logWarning(`add_task failed: ${String(e)}`);
  } finally {
    adding.value = false;
  }
}

</script>

<template>
  <div
    ref="rootRef"
    class="flex flex-col gap-1.5 min-h-0 flex-1"
  >
    <div
      v-if="!loading && !loadError && progress.total > 0"
      data-testid="task-progress"
      class="flex items-center gap-1.5"
      :title="`${progress.done} / ${progress.total} done`"
    >
      <div class="h-0.5 min-w-0 flex-1 overflow-hidden rounded-full bg-white/10">
        <div
          class="h-full rounded-full bg-accent transition-all"
          :style="{ width: `${progress.pct}%` }"
        />
      </div>
      <span class="shrink-0 text-micro tabular-nums text-fg-muted">
        {{ progress.done }} / {{ progress.total }}
      </span>
    </div>

    <input
      v-if="showFilter"
      v-model="filter"
      data-testid="task-filter"
      type="search"
      placeholder="Filter tasks…"
      aria-label="Filter tasks"
      class="rounded-control border border-white/10 bg-white/5 px-2 py-1 text-xs text-fg placeholder:text-fg-subtle focus:border-focus focus:outline-none"
    >

    <div
      v-if="tagFilter"
      data-testid="task-tag-filter"
      class="flex items-center gap-1 self-start rounded-full bg-accent/20 py-0.5 pl-2 pr-1 text-xs text-accent-fg"
    >
      <span>#{{ tagFilter }}</span>
      <button
        type="button"
        data-testid="task-tag-filter-clear"
        aria-label="Clear tag filter"
        class="cursor-pointer rounded-full px-1 text-violet-300 transition-colors hover:bg-white/10 hover:text-white focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
        @click="tagFilter = null"
      >
        ✕
      </button>
    </div>

    <TaskComposer
      ref="composer"
      :is-aggregate="isAggregate"
      :vault-options="vaultOptions"
      :adding="adding"
      :lists="composerLists"
      :default-list="composerDefaultList"
      :creating-list="creatingList"
      @submit="add"
      @create-list="onCreateList"
      @vault-change="onComposerVaultChange"
    />

    <TaskViewControls
      v-if="!loading && !loadError && (tasks.length > 0 || hasDisplayableLists)"
      :grouping="grouping"
      :sort-pref="sortPref"
      :is-aggregate="isAggregate"
      :creating-list="creatingList"
      :reset-nonce="controlsListResetNonce"
      @update:grouping="grouping = $event"
      @set-sort-key="setSortKey"
      @flip-sort-dir="flipSortDir"
      @create-list="onControlsCreateList"
    />

    <div class="panel-scroll min-h-0 flex-1 overflow-y-auto pr-1">
      <p
        v-if="loading"
        class="text-xs text-fg-muted"
      >
        Loading…
      </p>
      <Banner
        v-else-if="loadError"
        tone="danger"
      >
        {{ loadError }}
      </Banner>
      <EmptyState
        v-else-if="tasks.length === 0 && buckets.length === 0"
        title="No tasks yet."
      >
        <template #icon>
          <AppIcon :size="28">
            <path d="M9 11l3 3 8-8" />
            <path d="M20 12v6a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h9" />
          </AppIcon>
        </template>
      </EmptyState>
      <EmptyState
        v-else-if="buckets.length === 0"
        :title="`No tasks match${tagFilter ? ` #${tagFilter}` : ''}${showFilter && filter ? ` &quot;${filter}&quot;` : ''}.`"
      />
      <template v-else>
        <div
          v-for="bucket in buckets"
          :key="bucket.key"
          :data-section-key="bucket.key"
          class="mt-1 rounded-control"
          :class="bucket.key === crossListDropTarget ? 'bg-accent/10 ring-2 ring-focus/60' : ''"
        >
          <div
            v-if="bucket.label"
            class="mb-1 flex items-center gap-1 px-1"
          >
            <h3
              data-testid="task-bucket-header"
              class="text-micro font-semibold uppercase tracking-wider"
              :class="bucket.key === 'overdue' ? 'text-danger-fg' : 'text-fg-subtle'"
            >
              {{ bucket.label }}
            </h3>
            <TaskSectionMenu
              v-if="bucket.list && grouping === 'lists' && !isAggregate"
              :list="bucket.list!"
              :busy="sectionBusy.has(bucket.list!)"
              :reset-nonce="sectionMenuResetNonce"
              @rename="onSectionRename(bucket.list!, $event)"
              @archive="onSectionArchive(bucket.list!)"
              @delete="onSectionDelete(bucket.list!)"
            />
          </div>
          <ul class="flex flex-col gap-1">
            <TaskRow
              v-for="(task, i) in bucket.tasks"
              :key="rowKey(bucket.key, task)"
              :task="task"
              :busy="isBusy(task.path)"
              :is-aggregate="isAggregate"
              :editing="editingKey === rowKey(bucket.key, task)"
              :reorderable="reorderView"
              :reorder-busy="reordering"
              :dragging="dragState?.sectionKey === bucket.key && dragState.fromIndex === i"
              :drop-target="
                dragState?.sectionKey === bucket.key &&
                  dragState.toIndex === i &&
                  dragState.fromIndex !== i &&
                  dragState.overSectionKey === dragState.sectionKey
              "
              :data-reorder-section="bucket.key"
              @toggle="toggle(task)"
              @archive="archive(task)"
              @edit="startEdit(task, bucket.key)"
              @open="openInObsidian(task)"
              @tag-click="tagFilter = $event"
              @reorder-pointer-down="onHandlePointerDown($event, bucket.key, i)"
              @reorder-keydown="onHandleKeydown($event, bucket.key, i)"
            >
              <TaskEditor
                :task="task"
                :busy="isBusy(task.path)"
                :lists="listsForEditor(task.vaultId, task.list)"
                @save="onEditorSave(task, $event)"
                @cancel="cancelEdit"
              />
            </TaskRow>
          </ul>
        </div>
      </template>
    </div>
  </div>
</template>
