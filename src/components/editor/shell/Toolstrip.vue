<script setup lang="ts">
/**
 * The preview header's teaching-tool strip (visual-parity Task 11; concept
 * spec §4.1 item 4, §11): Text, Arrow, Highlight and Zoom with their icons,
 * then **More tools ⌄** — Spotlight, Numbered step and Privacy cover, in the
 * one `MenuPanel` (design D13). One `role="toolbar"`, one tab stop:
 * ←/→ move between its buttons and wrap, Home/End jump (roving tabindex,
 * D16). The strip's density comes from the header (`previewHeader.ts`):
 * medium reads "More" for "More tools"; compact moves Highlight and Zoom
 * into the menu.
 *
 * Every tool reads the ONE registry (`actions.ts`, `cueActions.ts` decide
 * which clip and which source span), so a disabled reason here, in the
 * menu and on a shortcut can never disagree. A disabled tool keeps its
 * place and its focus, carries the reason as its title, and says it in a
 * toast when pressed (D14). A successful add selects the new cue, so its
 * handles and the cue inspector are up at once.
 *
 * The strip is the guide's `preview.toolstrip` (the teaching-tools
 * lesson); should the lesson's Arrow ever leave the strip, the guide points
 * at More tools instead (the registry's overflow rule).
 */
import type { ComponentPublicInstance } from "vue";
import { computed, ref, watch } from "vue";

import { useActionRegistry } from "../../../composables/useActionRegistry";
import { useGuideOverflow, useGuideTarget } from "../../../composables/useGuideTarget";
import { useRovingTablist } from "../../../composables/useRovingTablist";
import type { ActionContext, ActionId } from "../../../editor/actions";
import type { commandFor } from "../../../editor/actions";
import { addedEffectId } from "../../../editor/cueActions";
import type { PreviewDensity, TeachingTool } from "../../../editor/previewHeader";
import { MORE_TOOLS_HEADING, MORE_TOOLS_SUBTITLE, moreToolsItems, stripTools } from "../../../editor/previewHeader";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import MenuPanel from "../menus/MenuPanel.vue";

const props = defineProps<{ density: PreviewDensity; context: ActionContext }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const { resolved, enabledCommand } = useActionRegistry(() => props.context);
const tools = computed(() => stripTools(props.density));
const moreLabel = computed(() => (props.density === "wide" ? "More tools" : "More"));

// ---- activation ---------------------------------------------------------

type AddCueCommand = Extract<NonNullable<ReturnType<typeof commandFor>>, { kind: "addEffect" }>;

/** A teaching tool: add the cue, then select it — the new id is whichever
 * effect the committed project has that the one before it did not. */
async function addCue(command: AddCueCommand): Promise<void> {
  const before = editorProject.project;
  if (!(await editorProject.execute(command))) return;
  const id = addedEffectId(before, editorProject.project);
  if (!id) return;
  workspace.select([command.clipId]);
  workspace.setSelected({ type: "effect", id });
}

function activate(id: ActionId): void {
  const command = enabledCommand(id);
  if (command?.kind === "addEffect") void addCue(command);
}

// ---- More tools ------------------------------------------------------------

const moreEl = ref<HTMLButtonElement | null>(null);
const moreOpen = ref(false);
const moreItems = computed(() => moreToolsItems(props.density, resolved.value, activate));

/** The open menu closes itself on a press outside it; the trigger is
 * outside it, so without this a press there would close and reopen it. */
function onMorePointerDown(event: PointerEvent): void {
  if (moreOpen.value) event.stopPropagation();
}

// ---- roving tabindex (the tablists' own composable: ←/→ wrap, Home/End) ----

const activeIndex = ref(0);
/** The strip's tools, then More tools. */
const count = computed(() => tools.value.length + 1);
watch(count, (n) => {
  if (activeIndex.value > n - 1) activeIndex.value = n - 1;
});
const { setTabRef: setButton, onKeydown } = useRovingTablist(
  () => count.value,
  () => activeIndex.value,
  (i) => (activeIndex.value = i),
);

// ---- guide target (Task 55) --------------------------------------------------

const stripTarget = useGuideTarget("preview.toolstrip");
const moreTarget = useGuideOverflow("preview.toolstrip", () => !tools.value.some((t) => t.id === "addArrow"));
function bindMore(el: Element | ComponentPublicInstance | null): void {
  moreEl.value = el as HTMLButtonElement | null;
  setButton(tools.value.length, el as Element | null);
  moreTarget(el);
}

/** Wide tools are 11px with 9px sides; medium and compact close up (§11). */
const sizeClass = computed(() => (props.density === "wide" ? "px-[9px] py-[5px] text-[11px]" : "px-1.5 py-[5px] text-[10px]"));
const stripGap = computed(() => (props.density === "compact" ? "gap-[3px]" : "gap-1.5"));
/** Where More tools opens: its trigger, while open. */
const moreAnchor = computed(() => (moreOpen.value ? moreEl.value : null));

function toolClass(id: ActionId): string[] {
  return [sizeClass.value, resolved.value[id].enabled ? "" : "cursor-not-allowed opacity-40"];
}
function toolTitle(tool: TeachingTool): string {
  return resolved.value[tool.id].reason ?? `Add ${tool.label.toLowerCase()} at the playhead`;
}
</script>

<template>
  <div
    :ref="stripTarget"
    data-testid="preview-toolstrip"
    role="toolbar"
    aria-label="Teaching tools"
    class="ml-auto flex min-w-0 shrink-0 items-center"
    :class="stripGap"
    @keydown="onKeydown"
  >
    <button
      v-for="(tool, i) in tools"
      :key="tool.id"
      :ref="(el) => setButton(i, el as Element | null)"
      type="button"
      :data-testid="`preview-tool-${tool.key}`"
      :tabindex="i === activeIndex ? 0 : -1"
      :aria-disabled="!resolved[tool.id].enabled"
      :title="toolTitle(tool)"
      class="inline-flex h-8 min-h-8 shrink-0 items-center gap-1.5 border border-transparent bg-transparent text-fg-secondary"
      :class="toolClass(tool.id)"
      @focus="activeIndex = i"
      @click="activate(tool.id)"
    >
      <EditorIcon
        :name="tool.icon"
        :size="15"
      />
      {{ tool.label }}
    </button>
    <button
      :ref="bindMore"
      type="button"
      data-testid="preview-more-tools"
      aria-haspopup="menu"
      :aria-expanded="moreOpen"
      :tabindex="activeIndex === tools.length ? 0 : -1"
      title="More teaching tools"
      class="ml-[5px] inline-flex h-8 min-h-8 shrink-0 items-center gap-1.5 rounded-l-none rounded-r-[6px] border-y-0 border-r-0 border-l border-line bg-transparent text-fg-secondary"
      :class="sizeClass"
      @focus="activeIndex = tools.length"
      @pointerdown="onMorePointerDown"
      @click="moreOpen = !moreOpen"
    >
      <EditorIcon
        name="plus"
        :size="15"
      />
      {{ moreLabel }}
      <EditorIcon
        name="chevronDown"
        :size="10"
      />
    </button>
  </div>
  <MenuPanel
    v-if="moreAnchor"
    testid="preview-more-tools-panel"
    :heading="MORE_TOOLS_HEADING"
    :subtitle="MORE_TOOLS_SUBTITLE"
    :items="moreItems"
    :anchor="moreAnchor"
    @close="moreOpen = false"
  />
</template>
