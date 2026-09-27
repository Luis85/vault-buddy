<script setup lang="ts">
/**
 * The tutorial editor's responsive workspace shell (Task 16, F-48;
 * SCREENS-AND-INTERACTIONS.md §02/§12; DESIGN-SYSTEM.md "Desktop shell:
 * application header, workspace with optional library/inspector, resizable
 * timeline, unobtrusive status footer").
 *
 * A CSS grid: header row; library | preview | inspector row; timeline row.
 * This task shipped the frame and lightweight placeholders for all five
 * named regions (`library`, `preview-toolbar`, `preview`, `inspector`,
 * `timeline`) so the grid, the responsive collapse and the header were all
 * real and testable before there was anything to put in them. Task 17
 * filled `preview-toolbar` with a real `PreviewToolbar` (below); later
 * tasks fill the four REMAINING region slots (`library`, `preview`,
 * `inspector`, `timeline`) with real content.
 *
 * **The frame (visual-parity Task 4; design D4, D5; concept spec §1.3–
 * §1.5, §6.1, §7).** The root IS the concept's `.app` grid: header /
 * workspace / 8px splitter / timeline / status bar, with 1px `line`
 * separators and no gutters or rounded region cards. The workspace row is
 * a second grid, library | preview | inspector; at or below 1080px wide
 * the inspector becomes an overlay drawer, at or below 860 the library
 * does too, and a window 760px tall or less shrinks the bars.
 * `useShellLayout` turns `editorWorkspace`'s panel state into those rows,
 * columns and drawer classes; every panel toggle — the preview toolbar's
 * library/inspector toggles and Focus preview — is a store action, a real grid change
 * at every width (D5, audit finding 2). The header is a SIBLING of the
 * workspace row, never inside it, and the drawers open below the preview
 * header, so no drawer can cover "the route back".
 *
 * The breakpoints come from `window.innerWidth`/`innerHeight` (kept in the
 * store by `useShellLayout`), not a CSS media query alone:
 * `tests/editorShell.test.ts` has to assert the collapse in Vitest, and
 * happy-dom has no layout engine to evaluate a media query against
 * (AGENTS.md's Testing conventions) — the REAL, pixel-measured version is
 * `tests/e2e/editorShell.spec.ts` and `tests/e2e/editorParity.spec.ts`,
 * driving the production bundle in real Chromium.
 *
 * **The keyboard shortcut dispatcher (Task 21)** lives here — the carried
 * Task 17 finding ("there are no clip elements to focus until the timeline
 * … CARRY the wiring to Task 20/21") lands on THIS root element rather than
 * `window`. A `@keydown` on this shell's own root only ever sees a
 * keystroke whose focus target is somewhere INSIDE this subtree (a toolbar
 * button, a timeline clip, an inspector field) — bubbling, no capture — and
 * calls `event.stopPropagation()` for every combo it actually handles, so a
 * shortcut this dispatcher claims reaches no `window`-level listener behind
 * it (the retired phase-4 editor bound its own Ctrl+Z there until Task 59,
 * which is why the rule was written); an
 * unmatched, currently-disabled, or nothing-to-send combo (F6 with the
 * guide closed, Ctrl+E with nothing on the timeline; F1/? and F6 are the
 * guide's, and Ctrl+S/Ctrl+E the header's Save and the toolbar's Review —
 * `onAppKey`, Task 57) is left alone to
 * bubble normally, never swallowed with nothing done. A keystroke whose
 * target sits inside an open `role="menu"`/`role="dialog"` is the menu's
 * (`shouldHandle`'s `menuOwnsKeys`): Delete pressed in the context menu
 * must not delete the selection behind it.
 *
 * **Height.** The grid fills the window (`EditorRoot`'s `main`); the
 * workspace row takes what the fixed rows leave (at least 170px, 160 in a
 * short window), and the preview stage takes what the workspace row leaves
 * — so the stage never pushes the header's Save off-screen (the
 * `tests/e2e/editorShell.spec.ts` contract).
 *
 * **`NotificationHost` (Task 32 fix round 1).** The editor window had no
 * toast surface at all until `PreviewToolbar`'s ratio control needed one —
 * mounted here, once, the same `useNotificationsStore`/`NotificationHost`
 * pair `ActionPanel.vue` already uses in the panel window (each webview
 * gets its own Pinia instance, AGENTS.md's window model, so this is a
 * distinct store from the panel's). Since visual-parity Task 6 it is passed
 * `variant="editor"` — concept-spec §9.12's fixed, centred, bottom-anchored
 * toast, restyled ONLY here; the panel window's own bottom-left stack keeps
 * its exact look under the component's default `variant="panel"`.
 *
 * **Feedback (visual-parity Task 7)** — `useEditorFeedback`, mounted
 * here once, turns the store's refusals and revision conflicts into those
 * toasts, and the dispatcher below hands it the registry's reason when a
 * matched shortcut is disabled (the keydown itself still does nothing and
 * still bubbles).
 *
 * **Guide progress (Task 55)** is read once per window when the shell first
 * mounts (`editorOnboarding.load()` — idempotent, and it never throws: an
 * unreadable store only flips the header's "Session only").
 *
 * **The guide (Task 56)** — `GuideInvitation` and `GuideCoach` — mounts
 * here, at the shell root: fixed-position layers OUTSIDE the preview
 * section, so nothing the guide draws can sit inside what a render mirrors
 * (A26). The dispatcher below answers the guide's keys itself: F1/?
 * (`help`) start or resume the walkthrough, F6 (`guideFocus`) moves focus
 * between the card and its control — even out of a text field, since the
 * control can be one and F6 types nothing — and an Escape nothing else answered
 * pauses it once no menu is open. A pending progress save is flushed when
 * the window hides and when the shell unmounts.
 */
import { onBeforeUnmount, onMounted, ref, watch } from "vue";

import { useEditorFeedback } from "../../../composables/useEditorFeedback";
import { useShellLayout } from "../../../composables/useShellLayout";
import { baseActionContext } from "../../../editor/actionContext";
import type { ActionId } from "../../../editor/actionMeta";
import type { ActionContext } from "../../../editor/actions";
import { resolveActions } from "../../../editor/actions";
import { activateEditorAction } from "../../../editor/clipboard";
import { requestReveal } from "../../../editor/revealBus";
import { isGuideDismissKey, matchShortcut, shouldHandle } from "../../../editor/shortcuts";
import { useEditorOnboardingStore } from "../../../stores/editorOnboarding";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import NotificationHost from "../../NotificationHost.vue";
import GuideCoach from "../guide/GuideCoach.vue";
import GuideInvitation from "../guide/GuideInvitation.vue";
import TimelineSplitter from "../timeline/TimelineSplitter.vue";
import EditorHeader from "./EditorHeader.vue";
import EditorStatusBar from "./EditorStatusBar.vue";
import PreviewToolbar from "./PreviewToolbar.vue";

/** The header's Project menu — Open project… (visual-parity Task 8), Open
 * a project file (Task 39) and Discard project (Task 59) — forwarded to
 * `EditorRoot`, which owns which project the shell is showing. */
const emit = defineEmits<{
  (e: "open-project", projectFileId: string): void;
  (e: "open-project-file"): void;
  (e: "discard-project"): void;
}>();

const { frameStyle, workspaceStyle, libraryClass, inspectorClass } = useShellLayout();

const onboarding = useEditorOnboardingStore();
onMounted(() => void onboarding.load());

/** A hidden window may never show again (the X hides it; a quit follows):
 * the last lesson change must not wait out the save debounce. */
function onVisibilityChange() {
  if (document.visibilityState === "hidden") void onboarding.flush();
}
onMounted(() => document.addEventListener("visibilitychange", onVisibilityChange));
onBeforeUnmount(() => {
  document.removeEventListener("visibilitychange", onVisibilityChange);
  void onboarding.flush();
});

/**
 * Theme: Task 16 kept this as a local ref seeded from
 * `prefers-color-scheme`, with its own module doc promising "Task 18
 * persists it [in `editorWorkspace`]". Task 18 moved the STATE itself
 * into `editorWorkspace` (persisted through
 * `editor_save_workspace` — F16 — so the toggle survives a reopen); this
 * component's own job shrinks to the one thing that stays view-local:
 * applying `document.documentElement`'s `data-theme`, which
 * `src/style.css`'s `[data-theme]` blocks read. The seed is no longer the
 * OS preference: the editor opens dark (visual-parity design D1). The
 * editor window is its own webview, so this touches no other window.
 */
const workspace = useEditorWorkspaceStore();
watch(
  () => workspace.theme,
  (t) => {
    document.documentElement.dataset.theme = t;
  },
  { immediate: true },
);
function toggleTheme() {
  workspace.toggleTheme();
}

// ---- keyboard shortcut dispatcher (Task 21) --------------------------------

const editorProject = useEditorProjectStore();
const { announceDisabled } = useEditorFeedback();

/**
 * A single selection gives the dispatcher a real `pointerTarget` — the
 * `primaryTargetClip`/`targetTrackId` "falls back to a single selected clip"
 * rule `actions.ts` already establishes for a pointer-target-less caller —
 * so, for instance, Ctrl+V pastes onto the selected clip's own track rather
 * than reading "Select a track first" whenever nothing was right-clicked.
 */
function menuOwnsKeys(event: KeyboardEvent): boolean {
  const target = event.target;
  return target instanceof Element && target.closest('[role="menu"], [role="dialog"]') !== null;
}

const coach = ref<InstanceType<typeof GuideCoach> | null>(null);

/** The guide's own keys (Task 56). True when one was acted on. */
function onGuideKey(event: KeyboardEvent, actionId: ActionId | null): boolean {
  if (actionId === "help") {
    onboarding.start();
    return true;
  }
  if (actionId === "guideFocus") return coach.value?.toggleFocus() ?? false;
  return isGuideDismissKey(event) && (coach.value?.dismiss() ?? false);
}

/** Ctrl+S and Ctrl+E (Task 57): the two listed keys no wire command
 * answers. Save is the header's Save — never a second one while a save is
 * in flight (still claimed: the key's job is already being done); Review
 * asks the preview toolbar to open its dialog. Only while the action is
 * enabled; otherwise the key bubbles like any unavailable one. */
function onAppKey(actionId: ActionId, ctx: ActionContext): boolean {
  if ((actionId !== "save" && actionId !== "render") || !resolveActions(ctx)[actionId].enabled) return false;
  if (actionId === "render") requestReveal("review");
  else if (!editorProject.saving) void editorProject.save();
  return true;
}

/** F6 and F1 type nothing, so they may leave a text field: the guide's
 * highlighted control can BE one (the fades and layout lessons land in an
 * input), and without this neither key could ever reach the guide from
 * there (F1, review finding F-M3: a keyboard user resting in the rename
 * field must still be able to open help). This is keyed on the actual KEY,
 * not the resolved action id — `help` is also bound to `?`, which DOES type
 * a character and must keep respecting the text-field rule, or typing "?"
 * while renaming a clip would open the guide instead of typing "?". Only an
 * open menu or dialog still owns either key; every other key keeps
 * `shouldHandle`'s text-field rule. */
function bypassesTextFieldGate(event: KeyboardEvent, actionId: ActionId | null): boolean {
  return actionId === "guideFocus" || (actionId === "help" && event.key === "F1");
}
function gateAllows(event: KeyboardEvent, actionId: ActionId | null): boolean {
  const ownsKeys = menuOwnsKeys(event);
  if (bypassesTextFieldGate(event, actionId)) return !ownsKeys;
  return shouldHandle(event, { menuOwnsKeys: ownsKeys });
}

/** A matched shortcut that is disabled says why (audit finding 9) — the
 * reason was only ever a hover tooltip. The guide's own keys are not
 * editing actions and stay silent when they have nothing to do. */
function announceIfDisabled(actionId: ActionId, ctx: ActionContext): void {
  if (actionId === "help" || actionId === "guideFocus") return;
  const verdict = resolveActions(ctx)[actionId];
  if (!verdict.enabled) announceDisabled(verdict.reason);
}

function onShellKeydown(event: KeyboardEvent) {
  const actionId = matchShortcut(event);
  if (!gateAllows(event, actionId)) return;
  if (onGuideKey(event, actionId)) {
    event.preventDefault();
    event.stopPropagation();
    return;
  }
  if (!actionId) return;
  const selection = workspace.selectionClipIds;
  const clip = selection.length === 1 ? editorProject.clipById(selection[0]) : undefined;
  const ctx = baseActionContext(
    editorProject.project,
    editorProject.snapshot,
    workspace.playheadMs,
    selection,
    clip ? { kind: "clip", id: clip.id, timeMs: workspace.playheadMs } : null,
  );
  const acted = onAppKey(actionId, ctx) || activateEditorAction(actionId, ctx, (command) => editorProject.execute(command));
  if (!acted) {
    announceIfDisabled(actionId, ctx);
    return;
  }
  // Claimed: stop it here so no `window`-level listener behind the shell
  // double-handles the same keystroke (see the module doc). A
  // disabled/unmatched/nothing-to-send combo returns above WITHOUT this, so
  // it keeps bubbling exactly as it did before this dispatcher existed.
  event.preventDefault();
  event.stopPropagation();
}
</script>

<template>
  <div
    data-testid="editor-shell"
    class="relative grid h-full min-h-[520px] min-w-0 grid-cols-[minmax(0,1fr)] text-fg"
    :style="frameStyle"
    @keydown="onShellKeydown"
  >
    <EditorHeader
      :theme="workspace.theme"
      @toggle-theme="toggleTheme"
      @open-project="(id) => emit('open-project', id)"
      @open-project-file="emit('open-project-file')"
      @discard-project="emit('discard-project')"
    />

    <div
      data-testid="editor-workspace"
      class="relative grid min-h-0 min-w-0"
      :style="workspaceStyle"
    >
      <aside
        v-show="workspace.libraryVisible"
        data-testid="editor-shell-library"
        :aria-hidden="!workspace.libraryVisible"
        class="flex min-h-0 min-w-0 flex-col overflow-y-auto border-r border-line bg-panel p-2 text-micro text-fg-subtle"
        :class="libraryClass"
      >
        <slot name="library">
          Library — arrives in a later task.
        </slot>
      </aside>

      <section
        data-testid="editor-shell-preview"
        class="col-start-2 row-start-1 flex min-h-0 min-w-0 flex-col overflow-hidden bg-stage"
      >
        <!-- The preview's one header row (§4.1). `PreviewToolbar` owns the
             `data-testid="preview-toolbar"` row itself, so the Playwright
             "exactly one row" count holds on its root. -->
        <PreviewToolbar
          :library-open="workspace.libraryVisible"
          :inspector-open="workspace.inspectorVisible"
          @toggle-library="workspace.toggleLibrary()"
          @toggle-inspector="workspace.toggleInspector()"
          @focus-preview="workspace.toggleFocusPreview()"
        />
        <div class="flex min-h-0 grow flex-col text-micro text-fg-subtle">
          <slot name="preview">
            Preview — filled by the root (`PreviewSurface`, Task 22).
          </slot>
        </div>
      </section>

      <aside
        v-show="workspace.inspectorVisible"
        data-testid="editor-shell-inspector"
        :aria-hidden="!workspace.inspectorVisible"
        class="flex min-h-0 min-w-0 flex-col overflow-y-auto border-l border-line bg-panel p-2 text-micro text-fg-subtle"
        :class="inspectorClass"
      >
        <slot name="inspector">
          Inspector — arrives in a later task.
        </slot>
      </aside>
    </div>

    <TimelineSplitter />

    <section
      data-testid="editor-timeline"
      class="min-h-0 min-w-0 overflow-hidden bg-panel text-micro text-fg-subtle"
    >
      <slot name="timeline">
        Timeline — arrives in a later task.
      </slot>
    </section>

    <EditorStatusBar />

    <NotificationHost variant="editor" />
    <GuideInvitation />
    <GuideCoach ref="coach" />
  </div>
</template>

<style scoped>
/* The overlay drawers (concept spec §1.4): below the preview header
   (`--drawer-top`, set by `useShellLayout`), over the preview. */
.library-drawer,
.inspector-drawer {
  position: absolute;
  top: var(--drawer-top);
  bottom: 0;
  box-shadow: var(--editor-shadow);
}
.library-drawer {
  left: 0;
  width: var(--editor-library-drawer);
  z-index: 20;
}
.inspector-drawer {
  right: 0;
  width: var(--editor-inspector);
  z-index: 18;
}
</style>
