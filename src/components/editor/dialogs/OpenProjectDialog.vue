<script setup lang="ts">
/**
 * Open project (visual-parity Task 8; design D7): the Project menu's
 * picker over the stored tutorial projects (`editor_list_projects`), the
 * open one already left out by the menu, which also omits the item when
 * nothing else is stored. Choosing a row asks `EditorRoot` to open that
 * project (`open`), which owns which project the shell shows; the picker
 * closes on the choice. Each row names the project and when it was last
 * saved, and says so when it holds unsaved changes the editor kept for
 * recovery (the open offers Resume or Discard for them). Titles render as
 * text (mustache), never markup.
 */
import type { ProjectSummaryDto } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";

defineProps<{ open: boolean; projects: ProjectSummaryDto[] }>();
const emit = defineEmits<{ (e: "close"): void; (e: "open", projectFileId: string): void }>();

const DATE = new Intl.DateTimeFormat(undefined, { dateStyle: "medium", timeStyle: "short" });

/** "Saved <date>", or nothing for a timestamp that does not parse. */
function savedAt(iso: string): string {
  const at = new Date(iso);
  return Number.isNaN(at.getTime()) ? "" : `Saved ${DATE.format(at)}`;
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Open project"
    close-testid="open-project-close"
    @close="emit('close')"
  >
    <template #title>
      Open project
    </template>
    <template #subtitle>
      Continue a tutorial stored on this computer.
    </template>

    <ul
      data-testid="open-project-dialog"
      class="flex flex-col gap-1.5"
    >
      <li
        v-for="p in projects"
        :key="p.projectFileId"
      >
        <button
          type="button"
          :data-testid="`open-project-row-${p.projectFileId}`"
          class="flex w-full items-center gap-3 rounded-[9px] border border-line bg-panel px-3 py-2.5 text-left"
          @click="emit('open', p.projectFileId)"
        >
          <EditorIcon
            name="folder"
            class="text-accent"
          />
          <span class="flex min-w-0 flex-1 flex-col gap-0.5">
            <span class="truncate text-xs font-medium text-fg">{{ p.title }}</span>
            <span class="truncate text-[10px] text-fg-muted">
              {{ savedAt(p.updatedAt) }}<template v-if="p.hasRecovery"> · Unsaved changes kept for recovery</template>
            </span>
          </span>
        </button>
      </li>
    </ul>

    <template #footer>
      <DialogButton
        data-testid="open-project-cancel"
        @click="emit('close')"
      >
        Cancel
      </DialogButton>
    </template>
  </DialogHost>
</template>
