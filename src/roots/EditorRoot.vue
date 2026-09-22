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
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";

import LegacyCaptureEditor from "../components/editor/LegacyCaptureEditor.vue";
import { logWarning } from "../logging";
import { useEditorProjectStore } from "../stores/editorProject";
import { formatDuration } from "../utils/formatDuration";

/** Task 21 replaces this component with the real workspace UI and flips this
 * off; until then the legacy phase-4 surface is the only visible editor. Not
 * user-configurable — a code-level placeholder for the cutover, not a
 * setting. */
const SHOW_LEGACY_EDITOR = true;

const editorProject = useEditorProjectStore();

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

const shellDuration = computed(() => formatDuration(editorProject.durationMs));
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
 */
const sessionMatchesLegacy = computed(
  () => editorProject.sourceBase !== null && editorProject.sourceBase === legacyBase.value,
);
/** The project's OWN destination vault (A01: resolved by Rust from the
 * staged capture's sidecar, never from any store or UI state) — read
 * straight off the opened project, never from `screenCapture`'s `vaultId`.
 * That store mirrors the LAST capture the buddy/panel windows recorded, an
 * entirely different fact this window must not conflate with the project it
 * actually has open (`tests/screenCaptureEditHandoff.test.ts` pins this). */
const shellVault = computed(() => editorProject.project?.destination.vault ?? null);

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
    <!-- A temporary shell around the new session's own truth, until Task 21
         replaces the whole surface below it: title/duration/dirty/vault
         straight off `editorProject`, proving the new open path is really
         live rather than merely invoked. Gated on `sessionMatchesLegacy`
         (fix round 1) so a failed open never leaves this attributing a
         PREVIOUS capture's identity to whatever the legacy surface is now
         showing. -->
    <section
      v-if="editorProject.snapshot && sessionMatchesLegacy"
      data-testid="editor-shell"
      class="shrink-0 rounded-control border border-white/10 bg-white/5 px-3 py-2 text-micro text-fg-subtle"
    >
      <span data-testid="editor-shell-title">{{ editorProject.snapshot.title }}</span>
      ·
      <span data-testid="editor-shell-duration">{{ shellDuration }}</span>
      ·
      <span data-testid="editor-shell-dirty">{{ editorProject.dirty ? "Unsaved changes" : "Saved" }}</span>
      <template v-if="shellVault">
        ·
        <span data-testid="editor-shell-vault">{{ shellVault }}</span>
      </template>
    </section>
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
