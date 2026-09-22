<script setup lang="ts">
/**
 * The phase-4 timeline/preview/export surface (spec 8), extracted out of
 * `EditorRoot.vue` in Task 15 (F31) behind that root's feature switch: it is
 * every line `EditorRoot.vue` used to own for editing a STAGED capture in
 * place, unchanged in behavior, now driven by a `stagedBase` prop instead of
 * draining `take_editor_request` itself — the root owns the stash now, so
 * this component only has to react to what base it was handed.
 *
 * F3: this stays the ONE place that still calls `load_staged_capture`. A
 * `Project` opened through `editorProject.openStaged` carries no resolvable
 * asset path until Task 22's `editor_media_url` lands, so the new session
 * cannot back this preview yet — the two paths run side by side (the root
 * opens both, unconditionally) until Task 21 replaces this component's
 * surface with the real workspace and Task 59 deletes it along with the
 * command.
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { computed, onBeforeUnmount, onMounted, shallowRef, watch } from "vue";

import { useEditorExport } from "../../composables/useEditorExport";
import { useEditorSelection } from "../../composables/useEditorSelection";
import { useEditorTimeline } from "../../composables/useEditorTimeline";
import type { StagedCaptureDetail, TimelineDto } from "../../types";
import { wholeTimeline } from "../../utils/timelineGeometry";
import AppButton from "../ui/AppButton.vue";
import Banner from "../ui/Banner.vue";
import CapturePreview from "./CapturePreview.vue";
import ExportBar from "./ExportBar.vue";
import TimelineStrip from "./TimelineStrip.vue";

/** `null` means "nothing to show" — either nothing has ever been requested,
 * or the root drained an empty stash. The root never clears this back to
 * `null` once it has been set to a real base (an empty re-drain is "nothing
 * NEW", not "close what is showing" — see `EditorRoot.vue`), so this
 * component sees only forward transitions to a real base, or none at all. */
const props = defineProps<{ stagedBase: string | null }>();

const detail = shallowRef<StagedCaptureDetail | null>(null);
const error = shallowRef<string | null>(null);

/** A shallowRef, not a plain `let`.
 *
 * The computeds below read `editor` during render. A non-reactive binding is
 * `null` on the first render, so the optional chain short-circuits, nothing
 * reactive is touched, and Vue never registers a dependency — the timeline
 * would render empty and Undo would stay disabled after a real edit until
 * some unrelated ref forced a re-render. `shallowRef` is the right depth:
 * the composable's own refs are already reactive, and `ref` would deep-wrap
 * them for nothing. */
const editor = shallowRef<ReturnType<typeof useEditorTimeline> | null>(null);
/** Read THROUGH the composable rather than mirroring it. A local copy kept
 * in step by hand is one missed call away from a stale render. */
const timeline = computed<TimelineDto>(() => editor.value?.timeline.value ?? { segments: [] });
/** The highlighted block and the playhead — window state, not timeline
 * state, and the seam every operation re-indexes underneath. This component
 * is the only place that knows both the operation and the selection, so the
 * remap rule is installed here and applied around each verb below. */
const { selected, playheadMs, editKeepingSelection, clearSelection, resetSelection } =
  useEditorSelection(timeline);
// `.value` is not optional here: the composable returns a PLAIN object whose
// fields happen to be refs, and a template reading `editor?.canUndo` would
// get the ref OBJECT — always truthy, so Undo would never disable.
const canUndo = computed(() => editor.value?.canUndo.value ?? false);
const canRedo = computed(() => editor.value?.canRedo.value ?? false);

/** `assetPath` is the staged file's own absolute path, and `convertFileSrc`
 * percent-encodes it onto the asset origin. It JOINS NOTHING — which is why
 * the DTO cannot hand us a bare file name (P-5): that produced a URL naming
 * no file on disk and matching no entry in the asset protocol's
 * `$APPLOCALDATA/screen-captures/*` scope, so the preview stayed blank with
 * no error anywhere. The scope is still the boundary; it lives in
 * `tauri.conf.json` and nothing on this side can widen it. */
const src = computed(() =>
  detail.value === null ? "" : convertFileSrc(detail.value.assetPath, "asset"),
);

/** Spec 10 promises "saved on each edit, so a crash loses at most the last
 * one". A failed sidecar write takes that promise away, and only the user
 * can act on it (a full disk is spec 10's own case). A log line alone left
 * it invisible; this is the surface AGENTS.md's diagnostics invariant asks
 * for. It renders INSIDE the editor branch rather than beside the load-error
 * banner, because the editor stays perfectly usable. */
const saveFailed = computed(() => editor.value?.saveFailed.value ?? false);

/** Spec 8.3's export — the state machine feeding `ExportBar`, its four
 * `screen:export*` listeners and the bar's verbs — lives in
 * `useEditorExport`. Discard is the one verb that changes what this
 * component shows, so it reports back rather than writing `detail` itself. */
const {
  exportPhase,
  exportFraction,
  exportMessage,
  discardBusy,
  resetExport,
  onSave,
  onCancelExport,
  onDiscard,
  onOpenSaved,
  listenExportEvents,
} = useEditorExport(detail, () => {
  detail.value = null;
});

/** Is there any footage left to export? A UI HINT, not the rule:
 * `export::export_refusal` is the authority and refuses an empty timeline
 * server-side, and this exists only so Save reads as disabled rather than
 * failing on click — the task-hierarchy picker's posture exactly.
 *
 * It reads the composable's own `outputMs` deliberately. Spelling the
 * predicate out (`segments.some((s) => s.sourceEndMs > s.sourceStartMs)`)
 * would agree, but it would be a THIRD implementation of this feature's
 * segment arithmetic; `outputDurationMs` is the TypeScript half of the pair
 * `tests/fixtures/timeline-cases.json` holds against `core::timeline`, and a
 * hand-rolled predicate is held against nothing (docs/Gaps.md GAP-136). */
const canSave = computed(() => (editor.value?.outputMs.value ?? 0) > 0);

async function load(base: string) {
  // Let the capture we are LEAVING finish writing before we read anything.
  // `load` replaces the composable outright, abandoning its save chain — and
  // that chain targets a sidecar `load_staged_capture` is about to read. Close
  // and immediately reopen the same capture inside the write window (temp +
  // fsync + replacing rename) and the editor would seed from the PRE-write
  // content and then write an edit derived from it: one operation silently
  // lost (the phase review's m-8). The chain always settles — every error is
  // caught inside it — so this cannot hang the open.
  await editor.value?.flushPending();
  try {
    const loaded = await invoke<StagedCaptureDetail>("load_staged_capture", { base });
    // An untouched capture opens as ONE segment spanning the whole
    // recording: the timeline the sidecar does not carry yet. `wholeTimeline`
    // is the mirror of `core::timeline::Timeline::whole`, carrying its
    // zero-duration guard, rather than a fourth hand-written copy here.
    const seed: TimelineDto = loaded.timeline ?? wholeTimeline(loaded.durationMs);
    editor.value = useEditorTimeline(loaded.base, seed);
    detail.value = loaded;
    resetSelection();
    resetExport();
    error.value = null;
  } catch (e) {
    error.value = String(e);
  }
}

function onSplit() {
  editKeepingSelection(() => editor.value?.splitAt(playheadMs.value));
}
function onDelete() {
  if (selected.value === null) return;
  editor.value?.deleteSegment(selected.value);
  clearSelection();
}
/**
 * `slot` is an INSERTION slot in `[0, n]`, which is not what
 * `useEditorTimeline.reorder` takes — it takes a destination index in
 * `[0, n)` and REFUSES anything outside it rather than clamping. Dropping a
 * block past the last one is slot `n`, so handing the slot straight through
 * would make exactly that gesture do nothing at all, silently. The strip has
 * already rejected the two slots that mean "did not move".
 */
function onReorder(from: number, slot: number) {
  editKeepingSelection(() => editor.value?.reorder(from, slot > from ? slot - 1 : slot));
}
function onUndo() {
  editKeepingSelection(() => editor.value?.undo());
}
function onRedo() {
  editKeepingSelection(() => editor.value?.redo());
}

/** Spec 8.2's shortcuts.
 *
 * Ctrl+Y as well as Ctrl+Shift+Z: the spec names the latter, half of Windows
 * expects the former, and supporting both costs one clause. Bound on
 * `window` because the editor FILLS its own window — there is no narrower
 * focus target to scope to, and the strip and preview are the only
 * interactive surfaces in it.
 *
 * The modifier gate is not ceremony: an ungated `z` would rewrite the edit
 * from any keystroke.
 *
 * `altKey` is excluded because Windows reports AltGr as Ctrl+Alt, and on
 * several Central-European layouts AltGr+Z or AltGr+Y is how a character is
 * typed (Polish `ż`) — without the clause that keystroke both rewrites the
 * edit and is `preventDefault`ed, so the character never arrives either.
 *
 * The target check is the other half, and it is what the export bar's
 * controls finally make reachable: bound on `window`, this gate is in scope
 * for every control in the editor, so Ctrl+Z inside a text field would
 * rewrite the TIMELINE instead of the text — silently, since the typing is
 * untouched either way. The phase that adds the first field is the one it
 * guards; it is tested now, in BOTH directions, so the next author can
 * neither delete it as unreachable nor widen it into a gate that swallows
 * the shortcut everywhere.
 *
 * Both halves live in `isEditorShortcut` rather than inline: together they
 * put `onKeydown` over the complexity ratchet's threshold, and "is this
 * keystroke ours?" is one question with one answer. */
function isEditorShortcut(e: KeyboardEvent): boolean {
  if ((!e.ctrlKey && !e.metaKey) || e.altKey) return false;
  // `e.target` is not always an element: a keystroke dispatched at `window`
  // itself has a target with no `closest` at all, so a cast to `HTMLElement`
  // would throw right through the shortcut rather than guarding it.
  const target = e.target;
  return !(target instanceof Element && target.closest("input, textarea, [contenteditable='true']"));
}

function onKeydown(e: KeyboardEvent) {
  if (!isEditorShortcut(e)) return;
  const key = e.key.toLowerCase();
  if (key === "z" && !e.shiftKey) {
    e.preventDefault();
    onUndo();
  } else if ((key === "z" && e.shiftKey) || key === "y") {
    e.preventDefault();
    onRedo();
  }
}

const unlisteners: (() => void)[] = [];

onMounted(async () => {
  // Registered synchronously, ahead of the awaits below: a `listen` that
  // never resolves must not cost the user their keyboard.
  window.addEventListener("keydown", onKeydown);
  unlisteners.push(...(await listenExportEvents()));
  if (props.stagedBase !== null) await load(props.stagedBase);
});
onBeforeUnmount(() => {
  for (const off of unlisteners) off();
  // The listener is on `window`, which outlives this component. Leaving it
  // behind would keep an unmounted editor editing — and PERSISTING, through
  // the composable this closure still holds — a timeline nobody can see.
  window.removeEventListener("keydown", onKeydown);
});

// The root drains the stash and hands over each new base in turn — see this
// prop's own doc comment for why a re-assignment to the SAME string never
// fires this watcher (Vue's default equality check on the getter's return
// value), which is what keeps a duplicate `editor:open` for the capture
// already open from re-reading its own sidecar out from under an in-flight
// edit.
watch(
  () => props.stagedBase,
  (base) => {
    if (base !== null) void load(base);
  },
);
</script>

<template>
  <template v-if="error">
    <!-- A load failure REPLACES the editor rather than sitting beside it:
         there is no capture behind it to edit. The save-failure banner
         further down is the opposite case and renders INSIDE the editor,
         because there the edit is on screen and still usable. -->
    <Banner
      data-testid="editor-error"
      tone="danger"
    >
      {{ error }}
    </Banner>
  </template>
  <template v-else-if="detail">
    <header class="flex shrink-0 items-baseline justify-between">
      <h1 class="truncate text-sm font-medium">
        {{ detail.sourceTitle }}
      </h1>
      <p class="text-micro text-fg-subtle">
        {{ detail.width }}x{{ detail.height }}
      </p>
    </header>
    <Banner
      v-if="saveFailed"
      data-testid="editor-save-failed"
      tone="warning"
    >
      The last edit could not be saved to this capture's staging file. Your
      edits are still on screen, but a crash would lose them.
    </Banner>
    <CapturePreview
      class="min-h-0 flex-1"
      :src="src"
      :timeline="timeline"
      :output-ms="playheadMs"
      @update:output-ms="playheadMs = $event"
    />
    <!-- The strip emits a SELECT index and an insertion SLOT; `onReorder`
         converts the slot, and `selected` is remapped around every
         operation by `useEditorSelection` (a bare index would otherwise go
         on pointing at whatever footage took its number). -->
    <TimelineStrip
      :timeline="timeline"
      :selected="selected"
      :playhead-ms="playheadMs"
      @select="selected = $event"
      @reorder="onReorder"
    />
    <div class="flex shrink-0 gap-2">
      <AppButton
        data-testid="editor-split"
        variant="secondary"
        @click="onSplit"
      >
        Split
      </AppButton>
      <!-- Delete is the only verb that needs a selection, which is why it
           is the only one disabled without one; Split acts on the playhead
           and is a no-op on a boundary by spec 8.1. -->
      <AppButton
        data-testid="editor-delete"
        variant="secondary"
        :disabled="selected === null"
        @click="onDelete"
      >
        Delete
      </AppButton>
      <AppButton
        data-testid="editor-undo"
        variant="ghost"
        :disabled="!canUndo"
        @click="onUndo"
      >
        Undo
      </AppButton>
      <AppButton
        data-testid="editor-redo"
        variant="ghost"
        :disabled="!canRedo"
        @click="onRedo"
      >
        Redo
      </AppButton>
    </div>
    <!-- Keyed on the capture: the bar owns one piece of state of its own
         (the discard confirm), and the editor window is reused, so an
         armed confirm must not survive into the next capture. -->
    <ExportBar
      :key="detail.base"
      :phase="exportPhase"
      :fraction="exportFraction"
      :message="exportMessage"
      :can-save="canSave"
      :busy="discardBusy"
      @save="onSave"
      @discard="onDiscard"
      @cancel="onCancelExport"
      @open="onOpenSaved"
    />
  </template>
  <p
    v-else
    class="m-auto text-sm text-fg-muted"
  >
    No capture open. Pick one from Record Screen.
  </p>
</template>
