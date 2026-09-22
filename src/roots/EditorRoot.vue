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
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import LegacyCaptureEditor from "../components/editor/LegacyCaptureEditor.vue";
import EditorShell from "../components/editor/shell/EditorShell.vue";
import { logWarning } from "../logging";
import { useEditorProjectStore } from "../stores/editorProject";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";

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
const sessionMatchesLegacy = computed(
  () => editorProject.sourceBase !== null && editorProject.sourceBase === legacyBase.value,
);

/** Drain the stash and open whatever it held. Runs on mount AND on every
 * `editor:open`. An empty drain means "nothing new", never "close what is
 * showing" — blanking a live edit on a spurious or double-fired event would
 * be strictly worse than doing nothing, so `null` is simply never forwarded
 * to `legacyBase` or `editorProject.openStaged`. */
async function openRequested() {
  let base: string | null = null;
  try {
    base = await invoke<string | null>("take_editor_request");
  } catch (e) {
    logWarning(`take_editor_request failed: ${String(e)}`);
  }
  if (base === null) return;
  legacyBase.value = base;
  legacyRequestSeq.value += 1;
  // Unconditional alongside the legacy load (this task's own Behavior
  // section): both open a session so the store/pin/recovery invariants are
  // exercised from here on. The store's own same-base guard (Task 15) is
  // what makes calling this on every drain safe rather than a duplicate
  // `editor_open_staged` round trip on a re-`editor:open` for the capture
  // already showing.
  await editorProject.openStaged(base);
  // Fix round 1: a failed open used to be silent — `lastError` was set on
  // the store and nothing else happened, so the only trace was whatever
  // `sessionMatchesLegacy` now hides. Logged here, not inside the store,
  // because the store's own `openWith` doc is explicit that a failure is a
  // normal, expected outcome for some callers (a picker probing a project
  // that no longer exists) and must not itself become a warning line for
  // every one of them — this IS the one caller for which it always is.
  if (editorProject.lastError) {
    logWarning(`editor_open_staged failed for ${base}: ${editorProject.lastError.message}`);
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
  if (editorProject.sessionId && editorProject.sessionId !== editorWorkspace.sessionId) {
    void editorWorkspace.hydrate(editorProject.sessionId);
  }
}

const unlisteners: (() => void)[] = [];

onMounted(async () => {
  // Subscribed BEFORE the first drain, so a request arriving while that
  // drain is in flight is not lost — the `region:begin` rule applied here.
  unlisteners.push(await listen("editor:open", () => void openRequested()));
  await openRequested();
});
onBeforeUnmount(() => {
  for (const off of unlisteners) off();
});
</script>

<template>
  <main
    class="flex h-screen w-screen flex-col gap-3 overflow-y-auto bg-slate-900 p-4 text-fg"
  >
    <!-- Task 16 (F-48): the real responsive shell/header, replacing Task
         15's temporary title/duration/dirty/vault bar. Gated on
         `sessionMatchesLegacy` (fix round 1) so a failed open never leaves
         this attributing a PREVIOUS capture's identity to whatever the
         legacy surface is now showing — `EditorShell`/`EditorHeader` read
         `editorProject` directly and always render once mounted, so the
         v-if here (not inside the shell) is what makes it disappear on a
         failed open, exactly like the bar it replaces. -->
    <EditorShell v-if="editorProject.snapshot && sessionMatchesLegacy" />
    <!-- A multi-root component: its own root nodes (header, preview, strip,
         verbs, export bar — or the single "No capture open" line) land as
         DIRECT children of `main` in the DOM, exactly where they sat before
         this task's extraction, so `main`'s flex-column layout contract
         (AGENTS.md's Testing conventions, the e2e rules) is unchanged.
         `request-seq` (fix round 1) is what makes a re-drain of the SAME
         base reload — see `legacyRequestSeq`'s own doc. -->
    <LegacyCaptureEditor
      v-if="SHOW_LEGACY_EDITOR"
      :staged-base="legacyBase"
      :request-seq="legacyRequestSeq"
    />
  </main>
</template>
