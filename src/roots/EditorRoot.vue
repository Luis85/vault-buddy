<script setup lang="ts">
/**
 * The editor window's root (spec 8; tutorial-editor Task 15).
 *
 * Through phase 4 this was the one root that mirrored NO Rust state and
 * installed no store (AGENTS.md's "Frontend state" section says so at
 * length, and this task moves that claim to `RegionRoot`/
 * `RegionIndicatorRoot` alone). Task 15 makes it the store-driven entry point
 * into the new Rust editor session: it drains the stash Rust fills
 * (`take_editor_request`, unchanged — still not an `editor_*` command) and
 * hands the base straight to `editorProject.openStaged`, the ONE seam that
 * calls `editor_open_staged` (`src/editor/port.ts`'s own module doc pins
 * that scan).
 *
 * The phase-4 timeline/preview/export surface — everything this file used to
 * own for editing a staged capture in place — is extracted into
 * `LegacyCaptureEditor.vue` (F31) behind `SHOW_LEGACY_EDITOR`, a feature
 * switch Task 21 flips off once the new workspace can stand on its own. It
 * keeps calling `load_staged_capture` for its own preview (F3: a
 * `Project` opened through `editorProject.openStaged` carries no resolvable
 * asset path yet — Task 22's `editor_media_url`), so BOTH opens run for
 * every drained base, unconditionally: the store/pin/recovery invariants the
 * new session brings are exercised from this task on even though only the
 * legacy preview reads its result today.
 *
 * `editor:open` still carries no state — it is the edge that says "read the
 * stash again" — and mount still drains it too, for the reason
 * `editor_commands.rs`'s module doc gives at length: this webview mounts
 * exactly once per process (`window_close.rs` answers the editor's own X
 * with `prevent_close()` + `hide()`), so draining only from `onMounted` would
 * open the first capture and show it forever while every later request sat
 * unread in the stash.
 *
 * Task 18 fix round 1: a successful `openStaged` also hydrates
 * `editorWorkspace` with the session id `editorProject` just opened —
 * without this call nothing in production ever drove
 * `editor_get_workspace`/`editor_save_workspace` at all, so the store's
 * whole reason to exist (persisting selection/playhead/panel layout/theme)
 * was dead code. A FAILED open does not hydrate anything: there is no
 * session id to hydrate against, and `editorWorkspace`'s own fields already
 * reset to defaults the next time a real session opens (its `hydrate`'s own
 * doc).
 *
 * Task 37 (Part A): the window's X now reaches this root as
 * `editor:closeRequested` (emitted to the editor window alone) instead of a
 * hide, and `CloseGuardDialog` decides — a close with only the legacy
 * surface open still just hides. After every NEW session `RecoveryDialog`
 * checks for unsaved changes an earlier run left behind; a Resume or
 * Discard opens a different session, which re-hydrates `editorWorkspace`
 * exactly like a fresh open does.
 *
 * Task 37 (Part B): `take_editor_request` widened from a bare
 * `string | null` to `{kind: "staged" | "project", value: string} | null`,
 * so a drain can carry either a staged capture's base (from
 * `open_capture_editor`, the panel capture bar's/staged list's Edit) or a
 * tutorial project's id (from `open_project_editor`, the panel's new
 * "Tutorial projects" Resume — `StagedCaptureList.vue`). Only the `staged`
 * arm touches `legacyBase`/`legacyRequestSeq`: the legacy phase-4 surface
 * (`LegacyCaptureEditor`, `SHOW_LEGACY_EDITOR`) understands a staged
 * capture's base, never a project id. Both arms run `editorProject`'s open
 * and, on success, the same recovery check — a project opened plainly from
 * the panel (`openProject(id, false)`) must offer to resume its own
 * leftover `recovery.json` exactly like a freshly-opened capture does.
 *
 * Task 37 Part B, fix round 1 (review Critical #1): a Resume click used to
 * open a real session with nothing on screen — `EditorShell`'s render gate,
 * `sessionMatchesLegacy`, compares the store's `sourceBase` against
 * `legacyBase`, and a project-kind open never touches `legacyBase` at all,
 * so the shell stayed permanently hidden behind the legacy surface's "No
 * capture open" line. `sessionMatchesProject` is the project-kind
 * counterpart, built the SAME way for the SAME race-safety reason
 * `sessionMatchesLegacy` already has one: `openedProjectId` is set
 * unconditionally at drain time (mirroring `legacyBase`'s own timing), so a
 * slower, now-superseded project open resolving late can never make the
 * shell show a DIFFERENT project than the one this root most recently
 * asked for — and a FAILED open (of either kind) leaves the tracking ref
 * pointed at a target the store's own state can never match, hiding the
 * shell exactly like a failed staged open already does. `showLegacySurface`
 * additionally hides `LegacyCaptureEditor` itself while a project-kind
 * session is the one on screen — it understands only a staged capture's
 * base, so left mounted it would go on showing either "No capture open" or
 * a STALE previous staged capture underneath the real shell.
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import CloseGuardDialog from "../components/editor/dialogs/CloseGuardDialog.vue";
import RecoveryDialog from "../components/editor/dialogs/RecoveryDialog.vue";
import AudioSection from "../components/editor/inspector/AudioSection.vue";
import ClipSection from "../components/editor/inspector/ClipSection.vue";
import ColorSection from "../components/editor/inspector/ColorSection.vue";
import EffectSection from "../components/editor/inspector/EffectSection.vue";
import FadesSection from "../components/editor/inspector/FadesSection.vue";
import InspectorPanel from "../components/editor/inspector/InspectorPanel.vue";
import LayoutSection from "../components/editor/inspector/LayoutSection.vue";
import SpeedSection from "../components/editor/inspector/SpeedSection.vue";
import LegacyCaptureEditor from "../components/editor/LegacyCaptureEditor.vue";
import LibraryPanel from "../components/editor/library/LibraryPanel.vue";
import PreviewSurface from "../components/editor/preview/PreviewSurface.vue";
import EditorShell from "../components/editor/shell/EditorShell.vue";
import TimelineView from "../components/editor/timeline/TimelineView.vue";
import { importProjectPackage } from "../composables/useProjectPackage";
import { logWarning } from "../logging";
import { useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";
import { useNotificationsStore } from "../stores/notifications";

/** Task 21 replaces this component with the real workspace UI and flips this
 * off; until then the legacy phase-4 surface is the only visible editor. Not
 * user-configurable — a code-level placeholder for the cutover, not a
 * setting. */
const SHOW_LEGACY_EDITOR = true;

const editorProject = useEditorProjectStore();
const editorWorkspace = useEditorWorkspaceStore();

/** The base the legacy surface is showing. `null` until the first
 * successful drain — see `LegacyCaptureEditor`'s own `stagedBase` prop doc
 * for why an empty re-drain never resets this back to `null`. */
const legacyBase = ref<string | null>(null);
/**
 * Fix round 1. `legacyBase` alone cannot tell `LegacyCaptureEditor` "reload"
 * when a drain hands back the SAME base it is already showing — Vue's watch
 * never fires for a same-value reassignment, and that silence was two real
 * bugs: a `load_staged_capture` failure could never be retried by re-Editing
 * the same capture, and a stale `done` export bar survived a re-open after a
 * legacy Save (pinning, added by this task, keeps a capture staged rather
 * than removing it, so re-opening the SAME capture after Save is the
 * ORDINARY case now, not a corner). Bumped on every successful drain
 * regardless of whether the base changed; `LegacyCaptureEditor`'s multi-
 * source watch fires on this alone, and `load()` already flushes any
 * pending edit first, so reloading the same base is safe.
 */
const legacyRequestSeq = ref(0);

/** The project-kind counterpart of `legacyBase` (Task 37 Part B, fix round
 * 1): which project id this root's most recent `{kind:"project"}` drain
 * asked to open. Set unconditionally at drain time, before the open
 * resolves — the exact timing `legacyBase` already uses — so
 * `sessionMatchesProject` below gets the same race-safety guarantee. */
const openedProjectId = ref<string | null>(null);

/**
 * Fix round 1: a shell that goes on showing the PREVIOUS capture's identity
 * while a later open fails is worse than showing nothing — it attributes a
 * stale title/vault to whatever the legacy surface is now loading. The
 * store deliberately does NOT blank `snapshot`/`project` on a failed open
 * (`editorProject.ts`'s own doc: "it never blanks a working session over a
 * picker mis-click" — a real requirement for a future project picker), so
 * the guard belongs HERE: only trust the store's state when its own
 * `sourceBase` still agrees with the base the legacy surface is actually
 * showing. A failed second open leaves `sourceBase` pointed at whatever
 * opened last successfully, which is no longer `legacyBase` once the drain
 * that failed has updated it.
 *
 * Task 16: `EditorShell`/`EditorHeader` now read `editorProject` directly
 * for title/duration/vault (A01: resolved by Rust from the staged capture's
 * sidecar, never from `screenCapture`'s `vaultId` — a DIFFERENT fact, the
 * last capture the buddy/panel windows recorded —
 * `tests/screenCaptureEditHandoff.test.ts` pins this), so this file no
 * longer needs its own `shellDuration`/`shellVault` computeds; it keeps only
 * the gate deciding WHETHER to show the shell at all.
 */
const closeGuard = ref<InstanceType<typeof CloseGuardDialog> | null>(null);
const recovery = ref<InstanceType<typeof RecoveryDialog> | null>(null);

/** Hydrate `editorWorkspace` for the session `editorProject` now holds —
 * only when it is a genuinely NEW one (Task 18 fix round 2's rule, below). */
function hydrateNewSession(): boolean {
  if (!editorProject.sessionId || editorProject.sessionId === editorWorkspace.sessionId) return false;
  void editorWorkspace.hydrate(editorProject.sessionId);
  return true;
}

const sessionMatchesLegacy = computed(
  () => editorProject.sourceBase !== null && editorProject.sourceBase === legacyBase.value,
);

/** Task 37 Part B, fix round 1: true once the store's OWN reply for the
 * CURRENT session agrees with which project id this root most recently
 * asked to open — `sessionMatchesLegacy`'s project-kind counterpart, needed
 * because a project-kind open has no staged base to compare `legacyBase`
 * against at all. */
const sessionMatchesProject = computed(
  () => editorProject.snapshot?.projectId != null
    && editorProject.snapshot.projectId === openedProjectId.value,
);

/** `LegacyCaptureEditor` understands only a staged capture's base, so it is
 * hidden entirely — not merely blank — while a project-kind session is the
 * one actually on screen: left mounted it would show either its "No capture
 * open" empty state (nothing was ever staged this process) or a STALE
 * previous staged capture (`legacyBase` untouched by the project-kind
 * arm), neither of which describes what the shell beside it is showing. */
const showLegacySurface = computed(() => SHOW_LEGACY_EDITOR && !sessionMatchesProject.value);

/** `take_editor_request`'s widened reply (Task 37 Part B) — declared here,
 * not in `editorTypes.ts`, because it is not one of the `editor_*` DTOs
 * `src/editor/decode.ts` decodes: this command deliberately does not start
 * with `editor_` and is drained straight into `invoke`'s own generic, the
 * same posture the bare-string shape it replaces already had. */
type EditorRequest = { kind: "staged" | "project"; value: string };

/** Drain the stash and open whatever it held. Runs on mount AND on every
 * `editor:open`. An empty drain means "nothing new", never "close what is
 * showing" — blanking a live edit on a spurious or double-fired event would
 * be strictly worse than doing nothing, so `null` is simply never forwarded
 * to `legacyBase` or either `editorProject` open call. */
async function openRequested() {
  let request: EditorRequest | null = null;
  try {
    request = await invoke<EditorRequest | null>("take_editor_request");
  } catch (e) {
    logWarning(`take_editor_request failed: ${String(e)}`);
  }
  if (request === null) return;
  // Unconditional alongside the legacy load (this task's own Behavior
  // section): both open a session so the store/pin/recovery invariants are
  // exercised from here on. The store's own same-base guard (Task 15) is
  // what makes calling this on every drain safe rather than a duplicate
  // `editor_open_staged` round trip on a re-`editor:open` for the capture
  // already showing. Only a STAGED open touches the legacy surface:
  // `LegacyCaptureEditor` understands a staged capture's own base, never a
  // tutorial project's id — `openedProjectId` (below) is that arm's own
  // tracking ref, set at the SAME point in the flow for the SAME
  // race-safety reason `legacyBase` already is.
  if (request.kind === "staged") {
    legacyBase.value = request.value;
    legacyRequestSeq.value += 1;
    await editorProject.openStaged(request.value);
  } else {
    openedProjectId.value = request.value;
    await editorProject.openProject(request.value, false);
  }
  // Fix round 1: a failed open used to be silent — `lastError` was set on
  // the store and nothing else happened, so the only trace was whatever
  // `sessionMatchesLegacy`/`sessionMatchesProject` now hide. Logged here,
  // not inside the store,
  // because the store's own `openWith` doc is explicit that a failure is a
  // normal, expected outcome for some callers (a picker probing a project
  // that no longer exists) and must not itself become a warning line for
  // every one of them — this IS the one caller for which it always is.
  if (editorProject.lastError) {
    const cmd = request.kind === "staged" ? "editor_open_staged" : "editor_open_project";
    logWarning(`${cmd} failed for ${request.value}: ${editorProject.lastError.message}`);
    return;
  }
  // Task 18 fix round 1 (controller ruling): without this call nothing in
  // production ever invoked `editor_get_workspace`/`editor_save_workspace`
  // — `editorWorkspace.persist()` early-returns while `sessionId` is null,
  // so the selection/playhead/theme/etc this store exists to save were
  // silently never written or read back. `editorProject.sessionId` is the
  // store's own post-open session id, not the `base` this function was
  // handed — the same id `editor_save_workspace` keys its file on.
  //
  // Task 18 fix round 2 (controller ruling): hydrate ONLY for a genuinely
  // NEW session, never on every resolved `openStaged` call.
  // `editorProject.openStaged` short-circuits for a duplicate open of the
  // capture already showing (its own same-base guard, `editorProject.ts`)
  // without touching `sessionId` at all — so a duplicate `editor:open`
  // resolves here with the SAME `sessionId` it already hydrated. Hydrating
  // again would reset every field to defaults and re-apply the (up to
  // 750ms stale) persisted blob, silently discarding a change made since
  // the last debounce flush — exactly `editorProject.ts`'s own documented
  // "never blanks a working session" hazard, one layer up. Comparing
  // against `editorWorkspace.sessionId` (the id its OWN last `hydrate`
  // call set, synchronously, before any await) is what tells a genuinely
  // new session apart from a duplicate resolve of the same one.
  if (hydrateNewSession()) await recovery.value?.check();
}

/** Task 39: the header's "Open a project file". Rust opens its own dialog
 * and installs the file as a project; `openedProjectId` is set in the SAME
 * tick as the store's install (`importProjectPackage`'s own contract), so
 * the shell's gate never drops the shell for a frame. A refusal is said in
 * a toast and leaves the open project exactly as it was; a dismissed dialog
 * says nothing. */
async function openProjectFile() {
  const outcome = await importProjectPackage((projectId) => {
    openedProjectId.value = projectId;
  });
  if (outcome === "cancelled") return;
  if (outcome !== "opened") {
    useNotificationsStore().notify("error", `The project file could not be opened. ${outcome.message}`);
    return;
  }
  if (hydrateNewSession()) await recovery.value?.check();
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
    <!-- Task 16 (F-48): the real responsive shell/header, replacing Task
         15's temporary title/duration/dirty/vault bar. Gated on
         `sessionMatchesLegacy` OR `sessionMatchesProject` (fix round 1) so a
         failed open of EITHER kind never leaves this attributing a stale
         identity to whatever is now (or is not) open —
         `EditorShell`/`EditorHeader` read `editorProject` directly and
         always render once mounted, so the v-if here (not inside the shell)
         is what makes it disappear on a failed open, exactly like the bar
         it replaces. -->
    <EditorShell
      v-if="editorProject.snapshot && (sessionMatchesLegacy || sessionMatchesProject)"
      @open-project-file="openProjectFile"
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
           (`PreviewSurface.vue`'s own doc); the legacy preview below keeps
           its own `load_staged_capture` path until Task 59 (F3). -->
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
    <!-- A multi-root component: its own root nodes (header, preview, strip,
         verbs, export bar — or the single "No capture open" line) land as
         DIRECT children of `main` in the DOM, exactly where they sat before
         this task's extraction, so `main`'s flex-column layout contract
         (AGENTS.md's Testing conventions, the e2e rules) is unchanged.
         `request-seq` (fix round 1) is what makes a re-drain of the SAME
         base reload — see `legacyRequestSeq`'s own doc. Gated on
         `showLegacySurface`, not the bare `SHOW_LEGACY_EDITOR` flag (Task 37
         Part B, fix round 1): while a project-kind session is the one on
         screen this surface understands neither it nor the "No capture
         open" line it would otherwise show, so it is unmounted entirely
         rather than left showing something untrue. -->
    <LegacyCaptureEditor
      v-if="showLegacySurface"
      :staged-base="legacyBase"
      :request-seq="legacyRequestSeq"
    />
    <!-- Task 37: both render nothing until they open, and `DialogHost` is
         fixed-position when they do, so neither adds a flex child the
         layout contract above measures. -->
    <CloseGuardDialog ref="closeGuard" />
    <RecoveryDialog
      ref="recovery"
      @session-changed="hydrateNewSession"
    />
  </main>
</template>
