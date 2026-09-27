<script setup lang="ts">
/**
 * The chapter list (Task 36; F-36; visual-parity Task 10, concept spec
 * §3.5): "TUTORIAL CHAPTERS" with the count, chapter markers in DERIVED
 * output time (`captionRules.chapterRows` -- a marker is stored in its
 * clip's source time, so a trim, a move or a speed change re-times the list
 * with no edit to the marker itself) as numbered rows, "Add chapter at
 * playhead" on the TOPMOST clip there, and the companion-note tip.
 *
 * A row's body jumps the playhead to the chapter (view state); its edit
 * button turns the title into a field, and its trash deletes it. Every edit
 * goes through `editorProject.execute` (Rust stays the authority -- a
 * locked track or an over-long title comes back as the store's error). A
 * rename commits on `change` (blur/Enter), never per keystroke, and leaves
 * the field once it is stored or unchanged; a blank title is not sent, and
 * one Rust refuses is not kept: either way the field shows the stored
 * title again and stays open (fix round 1). Escape leaves without sending.
 */
import { computed, nextTick, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { addMarkerAt, chapterRows, formatOutputTime } from "../../../editor/captionRules";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import LibraryButton from "./LibraryButton.vue";
import LibraryHeading from "./LibraryHeading.vue";

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
/** The guide's `library.chapters` (Task 55). */
const guideTarget = useGuideTarget("library.chapters");

const rows = computed(() => chapterRows(project.project));
const draft = computed(() => addMarkerAt(project.project, workspace.playheadMs));
const addReason = computed(() => ("reason" in draft.value ? draft.value.reason : null));

function add(): void {
  if ("command" in draft.value) void project.execute(draft.value.command);
}

/** The row whose title is a field right now. */
const editingId = ref<string | null>(null);

async function focusTestid(testid: string): Promise<void> {
  await nextTick();
  document.querySelector<HTMLElement>(`[data-testid="${testid}"]`)?.focus();
}

function startEdit(markerId: string): void {
  editingId.value = editingId.value === markerId ? null : markerId;
  void focusTestid(`chapter-title-${markerId}`);
}

/** Escape leaves the field and hands focus back to its edit button. */
function stopEdit(markerId: string, event: KeyboardEvent): void {
  if (event.key !== "Escape") return;
  event.stopPropagation();
  editingId.value = null;
  void focusTestid(`chapter-edit-${markerId}`);
}

async function rename(markerId: string, current: string, event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const title = input.value.trim();
  if (title === current || (title && (await project.execute({ kind: "updateMarker", markerId, title })))) {
    editingId.value = null;
    return;
  }
  input.value = current;
}

function remove(markerId: string): void {
  void project.execute({ kind: "removeMarker", markerId });
}
</script>

<template>
  <div
    :ref="guideTarget"
    data-testid="chapters-library"
    class="h-full overflow-y-auto text-fg"
  >
    <LibraryHeading
      label="TUTORIAL CHAPTERS"
      :pill="String(rows.length)"
      testid="chapters"
    />
    <p class="mb-2 text-[11px] text-fg-muted">
      Chapters follow their source clip.
    </p>
    <p
      v-if="rows.length === 0"
      data-testid="chapter-empty"
      class="py-5 text-center text-[12px] leading-[1.7] text-fg-muted"
    >
      Add chapters to turn your recording into a reusable guide.
    </p>
    <ul
      v-else
      aria-label="Chapters"
      class="flex flex-col"
    >
      <li
        v-for="(row, i) in rows"
        :key="row.marker.id"
        :data-testid="`chapter-row-${row.marker.id}`"
        class="flex items-center gap-0.5 border-b border-line"
      >
        <input
          v-if="editingId === row.marker.id"
          :data-testid="`chapter-title-${row.marker.id}`"
          type="text"
          :value="row.marker.title"
          :aria-label="`Chapter title at ${formatOutputTime(row.outputMs)}`"
          maxlength="160"
          class="my-1.5 h-8 min-h-0 min-w-0 flex-1 px-2 py-1 text-[11px] text-fg"
          @change="rename(row.marker.id, row.marker.title, $event)"
          @keydown="stopEdit(row.marker.id, $event)"
        >
        <button
          v-else
          type="button"
          :data-testid="`chapter-jump-${row.marker.id}`"
          :aria-label="`Jump to ${row.marker.title}`"
          class="flex min-w-0 flex-1 items-center gap-2.5 rounded-none px-1.5 py-2.5 text-left focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="workspace.setPlayhead(row.outputMs)"
        >
          <span
            :data-testid="`chapter-num-${row.marker.id}`"
            class="grid h-[26px] w-[26px] shrink-0 place-items-center rounded-full bg-accent-bg text-[11px] text-accent-ink"
          >{{ i + 1 }}</span>
          <span class="min-w-0">
            <b class="block truncate text-[11px] font-medium">{{ row.marker.title }}</b>
            <small class="block font-mono text-[10px] text-fg-muted">{{ formatOutputTime(row.outputMs) }}</small>
          </span>
        </button>
        <button
          type="button"
          :data-testid="`chapter-edit-${row.marker.id}`"
          :aria-label="`Edit chapter ${row.marker.title}`"
          :aria-pressed="editingId === row.marker.id"
          class="flex h-8 w-8 shrink-0 items-center justify-center p-1.5 text-fg-muted focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="startEdit(row.marker.id)"
        >
          <EditorIcon
            name="edit"
            :size="14"
          />
        </button>
        <button
          type="button"
          :data-testid="`chapter-delete-${row.marker.id}`"
          :aria-label="`Delete chapter ${row.marker.title}`"
          class="flex h-8 w-8 shrink-0 items-center justify-center p-1.5 text-fg-muted hover:text-danger-fg focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
          @click="remove(row.marker.id)"
        >
          <EditorIcon
            name="trash"
            :size="14"
          />
        </button>
      </li>
    </ul>
    <LibraryButton
      icon="bookmark"
      data-testid="chapter-add"
      class="mt-[15px] w-full"
      :disabled="addReason !== null"
      :title="addReason ?? 'Add a chapter at the playhead'"
      @click="add"
    >
      Add chapter at playhead
    </LibraryButton>
    <div
      data-testid="chapter-tip"
      class="mt-[18px] rounded-lg border border-line bg-raised p-3 text-[11px] leading-[1.6] text-fg-muted"
    >
      <b class="mb-[3px] block font-medium text-fg-secondary">Keep the context.</b>
      Publishing a video to a vault writes a companion note with these chapter timestamps.
    </div>
  </div>
</template>
