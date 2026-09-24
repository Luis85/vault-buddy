<script setup lang="ts">
/**
 * The chapter list (Task 36; F-36): chapter markers in DERIVED output time
 * (`captionRules.chapterRows` -- a marker is stored in its clip's source
 * time, so a trim, a move or a speed change re-times the list with no edit
 * to the marker itself), an "Add at playhead" on the TOPMOST clip there,
 * and per-row rename, delete and jump.
 *
 * Every edit goes through `editorProject.execute` (Rust stays the
 * authority -- a locked track or an over-long title comes back as the
 * store's error); jumping moves the playhead only, which is view state.
 * A rename commits on `change` (blur/Enter), never per keystroke, and a
 * blank or unchanged title is not sent, and one Rust refuses is not kept:
 * either way the field shows the stored title again (fix round 1).
 */
import { computed } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { addMarkerAt, chapterRows, formatOutputTime } from "../../../editor/captionRules";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";

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

async function rename(markerId: string, current: string, event: Event): Promise<void> {
  const input = event.target as HTMLInputElement;
  const title = input.value.trim();
  if (title && title !== current && (await project.execute({ kind: "updateMarker", markerId, title }))) return;
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
    class="flex h-full flex-col gap-2 text-micro text-fg-secondary"
  >
    <button
      type="button"
      data-testid="chapter-add"
      :disabled="addReason !== null"
      :title="addReason ?? 'Add a chapter at the playhead'"
      class="rounded border border-line px-2 py-0.5 text-left text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus disabled:cursor-not-allowed disabled:opacity-50"
      @click="add"
    >
      Add chapter at playhead
    </button>
    <p
      v-if="rows.length === 0"
      class="text-fg-subtle"
    >
      No chapters yet. Chapters follow the footage they are placed on and
      appear in the companion note.
    </p>
    <ul
      v-else
      aria-label="Chapters"
      class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto"
    >
      <li
        v-for="row in rows"
        :key="row.marker.id"
        :data-testid="`chapter-row-${row.marker.id}`"
        class="flex items-center gap-1"
      >
        <button
          type="button"
          :data-testid="`chapter-jump-${row.marker.id}`"
          :aria-label="`Jump to ${row.marker.title}`"
          class="shrink-0 rounded px-1 font-mono text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @click="workspace.setPlayhead(row.outputMs)"
        >
          {{ formatOutputTime(row.outputMs) }}
        </button>
        <input
          :data-testid="`chapter-title-${row.marker.id}`"
          type="text"
          :value="row.marker.title"
          :aria-label="`Chapter title at ${formatOutputTime(row.outputMs)}`"
          maxlength="160"
          class="min-w-0 flex-1 rounded border border-line bg-stage px-1 py-0.5 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @change="rename(row.marker.id, row.marker.title, $event)"
        >
        <button
          type="button"
          :data-testid="`chapter-delete-${row.marker.id}`"
          :aria-label="`Delete chapter ${row.marker.title}`"
          class="shrink-0 rounded px-1 text-fg-subtle hover:bg-white/10 hover:text-danger-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @click="remove(row.marker.id)"
        >
          ✕
        </button>
      </li>
    </ul>
  </div>
</template>
