<script setup lang="ts">
/**
 * The editor window's root (spec 8; tutorial-editor Tasks 15, 59).
 *
 * It drains the stash Rust fills (`take_editor_request` — not an `editor_*`
 * command) and opens what it held in the tutorial editor's session: a
 * staged capture's base through `editorProject.openStaged` (the ONE seam
 * that calls `editor_open_staged`, `src/editor/port.ts`'s own module doc),
 * a tutorial project's id through `openProject`. Task 59 retired the
 * phase-4 editor (`LegacyCaptureEditor`, its sidecar read and timeline
 * write, the phase-5 export bar) that used to sit beside the shell here;
 * what it answered on its own — "No capture open", a failed load — this
 * root now says itself, so an empty or refused open is never a blank
 * window.
 *
 * `editor:open` carries no state — it is the edge that says "read the
 * stash again" — and mount drains it too, for the reason
 * `editor_commands.rs`'s module doc gives at length: this webview mounts
 * exactly once per process (`window_close.rs` answers the editor's own X
 * with `prevent_close()`), so draining only from `onMounted` would open the
 * first capture and show it forever while every later request sat unread
 * in the stash.
 *
 * **The shell's gate (Task 15, Task 37 Part B fix round 1).** The store
 * deliberately does NOT blank `snapshot`/`project` on a failed open
 * (`editorProject.ts`: "it never blanks a working session over a picker
 * mis-click"), so the shell is shown only while the store's OWN reply
 * agrees with the request this root most recently drained — the staged
 * capture's base (`sourceBase`) or the project's id. `requested` is set at
 * drain time, before the open resolves, so a slower, superseded open
 * resolving late can never make the shell show a DIFFERENT capture than
 * the one most recently asked for, and a failed open (of either kind)
 * leaves `requested` pointed at a target the store can never match.
 *
 * Task 18 fix rounds 1–2: a successful open hydrates `editorWorkspace` —
 * only for a genuinely NEW session (`hydrateNewSession`). Task 37: the
 * window's X reaches this root as `editor:closeRequested`, and
 * `CloseGuardDialog` decides; after every new session `RecoveryDialog`
 * checks for unsaved changes an earlier run left behind. Task 59: the
 * project menu's Discard project opens `DiscardProjectDialog` here, outside
 * the shell a discard unmounts.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import CloseGuardDialog from "../components/editor/dialogs/CloseGuardDialog.vue";
import DiscardProjectDialog from "../components/editor/dialogs/DiscardProjectDialog.vue";
import RecoveryDialog from "../components/editor/dialogs/RecoveryDialog.vue";
import AudioSection from "../components/editor/inspector/AudioSection.vue";
import ClipSection from "../components/editor/inspector/ClipSection.vue";
import ColorSection from "../components/editor/inspector/ColorSection.vue";
import EffectSection from "../components/editor/inspector/EffectSection.vue";
import FadesSection from "../components/editor/inspector/FadesSection.vue";
import InspectorPanel from "../components/editor/inspector/InspectorPanel.vue";
import LayoutSection from "../components/editor/inspector/LayoutSection.vue";
import SpeedSection from "../components/editor/inspector/SpeedSection.vue";
import LibraryPanel from "../components/editor/library/LibraryPanel.vue";
import PreviewSurface from "../components/editor/preview/PreviewSurface.vue";
import EditorShell from "../components/editor/shell/EditorShell.vue";
import TimelineView from "../components/editor/timeline/TimelineView.vue";
import { importProjectPackage } from "../composables/useProjectPackage";
import { logWarning } from "../logging";
import { useEditorOnboardingStore } from "../stores/editorOnboarding";
import { toEditorError, useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";
import { useNotificationsStore } from "../stores/notifications";

const editorProject = useEditorProjectStore();
const editorWorkspace = useEditorWorkspaceStore();

/** `take_editor_request`'s reply (Task 37 Part B) — declared here, not in
 * `editorTypes.ts`, because it is not one of the `editor_*` DTOs
 * `src/editor/decode.ts` decodes: this command deliberately does not start
 * with `editor_` and is drained straight into `invoke`'s own generic. */
type EditorRequest = { kind: "staged" | "project"; value: string };

/** What this root most recently asked to open — see the module doc's
 * "shell's gate". `null` until the first non-empty drain. */
const requested = ref<EditorRequest | null>(null);

const closeGuard = ref<InstanceType<typeof CloseGuardDialog> | null>(null);
const recovery = ref<InstanceType<typeof RecoveryDialog> | null>(null);
const discardOpen = ref(false);

/** Hydrate `editorWorkspace` for the session `editorProject` now holds —
 * only when it is a genuinely NEW one (Task 18 fix round 2's rule, below). */
function hydrateNewSession(): boolean {
  if (!editorProject.sessionId || editorProject.sessionId === editorWorkspace.sessionId) return false;
  void editorWorkspace.hydrate(editorProject.sessionId);
  return true;
}

/** True once the store's OWN reply for the current session agrees with the
 * request this root most recently drained. */
const sessionMatchesRequest = computed(() => {
  const r = requested.value;
  if (r === null || !editorProject.snapshot) return false;
  return r.kind === "staged"
    ? editorProject.sourceBase === r.value
    : editorProject.snapshot.projectId === r.value;
});

/** Why this root's most recent OPEN was refused, in the store's role
 * wording (no redaction handle — `src/editor/errorCopy.ts`), or `null`.
 * Set by an open alone, never read off the store's shared `lastError`
 * (Task 59 fix round 1): a refused DISCARD also lands there, and a project
 * that opened fine must never be reported as one that could not be. */
const openError = ref<string | null>(null);

/** Shown only while the shell is not: a refused picker probe over a
 * working session is not this window's news. */
const openFailure = computed(() => (sessionMatchesRequest.value ? null : openError.value));

/** Drain the stash and open whatever it held. Runs on mount AND on every
 * `editor:open`. An empty drain means "nothing new", never "close what is
 * showing" — blanking a live edit on a spurious or double-fired event would
 * be strictly worse than doing nothing, so `null` changes nothing. */
async function openRequested() {
  let request: EditorRequest | null = null;
  try {
    request = await invoke<EditorRequest | null>("take_editor_request");
  } catch (e) {
    logWarning(`take_editor_request failed: ${String(e)}`);
  }
  if (request === null) return;
  // Set at drain time, before the open resolves — the gate's race safety.
  // The store's own same-base guard (Task 15) is what makes a re-drain of
  // the capture already showing safe rather than a duplicate
  // `editor_open_staged` round trip.
  requested.value = request;
  if (request.kind === "staged") await editorProject.openStaged(request.value);
  else await editorProject.openProject(request.value, false);
  openError.value = editorProject.lastError?.message ?? null;
  // A failed open is logged here, not inside the store, because the
  // store's own `openWith` doc is explicit that a failure is a normal,
  // expected outcome for some callers (a picker probing a project that no
  // longer exists) — this IS the one caller for which it always is news.
  if (editorProject.lastError) {
    const cmd = request.kind === "staged" ? "editor_open_staged" : "editor_open_project";
    logWarning(`${cmd} failed: ${editorProject.lastError.message}`);
    return;
  }
  // Task 18 fix rounds 1–2: hydrate ONLY for a genuinely NEW session. A
  // duplicate open of the capture already showing short-circuits in the
  // store without touching `sessionId`, and hydrating again would reset
  // every workspace field to defaults and re-apply the (up to 750ms stale)
  // persisted blob, silently discarding a change made since the last
  // debounce flush. `editorWorkspace.sessionId` — set synchronously by its
  // own last `hydrate` — is what tells the two apart.
  if (hydrateNewSession()) await recovery.value?.check();
}

/** Task 39: the header's "Open a project file". Rust opens its own dialog
 * and installs the file as a project; `requested` is set in the SAME tick
 * as the store's install (`importProjectPackage`'s own contract), so the
 * shell's gate never drops the shell for a frame. A refusal is said in a
 * toast and leaves the open project exactly as it was; a dismissed dialog
 * says nothing. */
async function openProjectFile() {
  const outcome = await importProjectPackage((projectId) => {
    requested.value = { kind: "project", value: projectId };
  });
  if (outcome === "cancelled") return;
  if (outcome !== "opened") {
    useNotificationsStore().notify("error", `The project file could not be opened. ${outcome.message}`);
    return;
  }
  if (hydrateNewSession()) await recovery.value?.check();
}

/** Task 59 fix round 1: a REFUSED discard left the Rust session live while
 * the store forgot it, so the project is reopened (Rust reuses the live
 * session) and the shell's gate pointed at it. `true` when a session is
 * back. */
async function reattach(projectId: string): Promise<boolean> {
  requested.value = { kind: "project", value: projectId };
  await editorProject.openProject(projectId, false);
  if (editorProject.lastError) {
    logWarning(`editor_open_project failed after a refused discard: ${editorProject.lastError.message}`);
    return false;
  }
  openError.value = null;
  if (hydrateNewSession()) await recovery.value?.check();
  return true;
}

/** Task 59: a discarded project leaves nothing in this window to show, so
 * it hides — after the guide's debounced progress save lands, the close
 * guard's own order (a hidden editor may never run its timer again). */
async function onDiscarded() {
  discardOpen.value = false;
  await useEditorOnboardingStore().flush();
  try {
    await editorProject.port.hideWindow();
  } catch (e) {
    logWarning(`editor: could not hide the window after a discard: ${toEditorError(e).message}`);
  }
}

const unlisteners: (() => void)[] = [];

onMounted(async () => {
  // Subscribed BEFORE the first drain, so a request arriving while that
  // drain is in flight is not lost — the `region:begin` rule applied here.
  unlisteners.push(await listen("editor:open", () => void openRequested()));
  unlisteners.push(
    await listen("editor:closeRequested", () => void closeGuard.value?.request()),
  );
  await openRequested();
});
onBeforeUnmount(() => {
  for (const off of unlisteners) off();
});
</script>

<template>
  <main
    class="flex h-screen w-screen flex-col gap-3 overflow-y-auto bg-app p-4 text-fg"
  >
    <!-- The responsive shell/header (Task 16, F-48), gated on the store's
         own reply matching the request this root drained (the module
         doc's "shell's gate"), so a failed open of either kind never
         leaves it attributing a stale identity to what is now (or is not)
         open. -->
    <EditorShell
      v-if="sessionMatchesRequest"
      @open-project-file="openProjectFile"
      @discard-project="discardOpen = true"
    >
      <!-- Task 19: the inspector shell (six category tabs + the shared
           draft composable later sections build on) fills the shell's
           `inspector` slot from here, the same seam `PreviewToolbar`
           filled in Task 17. Task 21 fills its FIRST real category slot,
           `#clip`, with `ClipSection` — keyed on the SELECTION only (a
           different clip is a different set of drafts); an undo/drag/nudge
           on the same clip reaches its drafts live, without a remount
           (`ClipSection.vue`'s own module doc). -->
      <template #inspector>
        <InspectorPanel>
          <template #clip="{ clipIds }">
            <ClipSection
              :key="clipIds.join(',')"
              :clip-ids="clipIds"
            />
          </template>
          <!-- Task 27: the Audio category, keyed the same way. -->
          <template #audio="{ clipIds }">
            <AudioSection
              :key="clipIds.join(',')"
              :clip-ids="clipIds"
            />
          </template>
          <!-- Task 29: the Fades category, keyed the same way. -->
          <template #fades="{ clipIds }">
            <FadesSection
              :key="clipIds.join(',')"
              :clip-ids="clipIds"
            />
          </template>
          <!-- Task 31: the Layout and Speed categories, keyed the same way. -->
          <template #layout="{ clipIds }">
            <LayoutSection
              :key="clipIds.join(',')"
              :clip-ids="clipIds"
            />
          </template>
          <template #speed="{ clipIds }">
            <SpeedSection
              :key="clipIds.join(',')"
              :clip-ids="clipIds"
            />
          </template>
          <!-- Task 32: the Color category, keyed the same way. -->
          <template #color="{ clipIds }">
            <ColorSection
              :key="clipIds.join(',')"
              :clip-ids="clipIds"
            />
          </template>
          <!-- Task 35: a selected teaching cue, keyed on the cue itself (a
               cue's kind fixes its field list). -->
          <template #effect="{ effectId }">
            <EffectSection
              :key="effectId"
              :effect-id="effectId"
            />
          </template>
        </InspectorPanel>
      </template>
      <!-- Task 20: the virtualized multi-track timeline fills the shell's
           `timeline` slot, the same seam `PreviewToolbar`/`InspectorPanel`
           filled in Tasks 17/19. -->
      <template #timeline>
        <TimelineView />
      </template>
      <!-- Task 22: the layered preview stage + transport fill the shell's
           `preview` slot. Media paths come from `editor_media_url` only
           (`PreviewSurface.vue`'s own doc). -->
      <template #preview>
        <PreviewSurface />
      </template>
      <!-- Task 25: the media library (search, asset cards, Import through
           Rust's own dialog, per-file results, "+" at the playhead) fills
           the shell's `library` slot, the last of its four region slots --
           Task 33 fix round 1 WRAPPED it in `LibraryPanel`'s Media/Titles
           tablist rather than replacing it, so `TitlesLibrary.vue` (built
           in Task 33 itself, unreachable until this fix round) has
           somewhere to mount. -->
      <template #library>
        <LibraryPanel />
      </template>
    </EditorShell>
    <!-- Task 59: the window's own words when no session is on screen —
         the retired phase-4 surface used to say them, and without them an
         empty or refused open would be a blank window. -->
    <p
      v-else-if="openFailure"
      data-testid="editor-open-failed"
      role="alert"
      class="rounded-control border border-line bg-panel px-3 py-2 text-sm text-danger-fg"
    >
      This could not be opened. {{ openFailure }}
    </p>
    <p
      v-else
      data-testid="editor-empty"
      class="px-1 text-sm text-fg-muted"
    >
      No capture open. Choose Edit on a staged capture in the panel, or resume a
      tutorial project there.
    </p>
    <!-- Task 37 (and Task 59's discard): each renders nothing until it
         opens, and `DialogHost` is fixed-position when it does, so none
         adds a flex child the layout contract measures. -->
    <CloseGuardDialog ref="closeGuard" />
    <DiscardProjectDialog
      :open="discardOpen"
      :reattach="reattach"
      @close="discardOpen = false"
      @discarded="onDiscarded"
    />
    <RecoveryDialog
      ref="recovery"
      @session-changed="hydrateNewSession"
    />
  </main>
</template>
