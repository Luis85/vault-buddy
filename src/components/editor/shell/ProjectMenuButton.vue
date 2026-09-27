<script setup lang="ts">
/**
 * The header's **Project** menu (visual-parity Task 8; design D6, D7;
 * concept spec §2 `.project-menu-trigger`): folder icon, "Project" and a
 * 12px chevron, opening the one `MenuPanel` below the trigger with the
 * native items (`projectMenuItems`). It replaces Task 39's Save ▾ split
 * button, whose items moved here.
 *
 * The stored projects are listed as the menu opens, so "Open project…"
 * appears only when another project exists; a listing that fails leaves the
 * item out (logged by code, never by name) rather than offering a picker
 * that cannot answer. Each item's effect:
 * - Open project… — `OpenProjectDialog`; a chosen row is `open-project`,
 *   which `EditorRoot` opens (it owns which project the shell shows);
 * - Open a project file…, Discard project… — forwarded to `EditorRoot`
 *   (Task 39 and Task 59's paths, unchanged);
 * - Rename tutorial…, Save a copy as project file… — the header's own
 *   dialogs (`rename`, `save-copy`);
 * - Workspace & rendered products — `revealWorkspaceProducts`, the status
 *   bar's own path, which opens the library's Project section.
 *
 * The trigger is also the guide's `library.products` FALLBACK (visual-parity
 * Task 10): while the Project section is closed there is no tab to point
 * at, and this menu is the way in the lesson itself names.
 */
import { computed, ref } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { PROJECT_MENU_HEADING, PROJECT_MENU_SUBTITLE,projectMenuItems } from "../../../editor/projectMenu";
import { revealWorkspaceProducts } from "../../../editor/revealProducts";
import type { ProjectSummaryDto } from "../../../editorTypes";
import { logWarning } from "../../../logging";
import { toEditorError, useEditorProjectStore } from "../../../stores/editorProject";
import OpenProjectDialog from "../dialogs/OpenProjectDialog.vue";
import EditorIcon from "../icons/EditorIcon.vue";
import MenuPanel from "../menus/MenuPanel.vue";

const emit = defineEmits<{
  (e: "open-project", projectFileId: string): void;
  (e: "open-project-file"): void;
  (e: "rename"): void;
  (e: "save-copy"): void;
  (e: "discard-project"): void;
}>();

const editorProject = useEditorProjectStore();
const trigger = ref<HTMLButtonElement | null>(null);
const productsFallback = useGuideTarget("library.products", { fallback: true });
function setTrigger(el: unknown): void {
  trigger.value = (el as HTMLButtonElement | null) ?? null;
  productsFallback(trigger.value);
}
const open = ref(false);
const listing = ref(false);
const others = ref<ProjectSummaryDto[]>([]);
const pickerOpen = ref(false);

/** Every stored project but the one on screen; none when the store
 * cannot be listed. */
async function otherProjects(): Promise<ProjectSummaryDto[]> {
  try {
    const rows = await editorProject.port.listProjects();
    return rows.filter((row) => row.projectFileId !== editorProject.snapshot?.projectId);
  } catch (e) {
    logWarning(`editor: the Project menu could not list projects: ${toEditorError(e).code}`);
    return [];
  }
}

async function toggle(): Promise<void> {
  if (open.value) {
    open.value = false;
    return;
  }
  if (listing.value) return;
  listing.value = true;
  others.value = await otherProjects();
  listing.value = false;
  open.value = true;
}

/** The open menu closes itself on a press outside it — the trigger is
 * outside it, so without this a click there would close and reopen it. */
function onTriggerPointerDown(event: PointerEvent): void {
  if (open.value) event.stopPropagation();
}

const items = computed(() =>
  projectMenuItems({
    sessionOpen: editorProject.sessionId !== null,
    hasOtherProjects: others.value.length > 0,
    openProject: () => (pickerOpen.value = true),
    openProjectFile: () => emit("open-project-file"),
    rename: () => emit("rename"),
    showProducts: revealWorkspaceProducts,
    saveCopy: () => emit("save-copy"),
    discard: () => emit("discard-project"),
  }),
);

function onPick(projectFileId: string): void {
  pickerOpen.value = false;
  emit("open-project", projectFileId);
}
</script>

<template>
  <button
    :ref="setTrigger"
    type="button"
    data-testid="editor-header-project-menu"
    aria-haspopup="menu"
    :aria-expanded="open"
    title="Project: open, rename, save a copy or discard"
    class="flex shrink-0 items-center gap-[7px] border-transparent bg-transparent px-2 py-1.5 text-[11px] text-fg"
    @pointerdown="onTriggerPointerDown"
    @click="toggle"
  >
    <EditorIcon name="folder" />
    Project
    <EditorIcon
      name="chevronDown"
      :size="12"
    />
  </button>
  <MenuPanel
    v-if="open && trigger"
    testid="editor-project-menu"
    :heading="PROJECT_MENU_HEADING"
    :subtitle="PROJECT_MENU_SUBTITLE"
    :items="items"
    :anchor="trigger"
    @close="open = false"
  />
  <OpenProjectDialog
    :open="pickerOpen"
    :projects="others"
    @close="pickerOpen = false"
    @open="onPick"
  />
</template>
