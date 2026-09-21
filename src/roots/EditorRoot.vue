<script setup lang="ts">
/**
 * The editor window's root (spec 8).
 *
 * The one root that mirrors NO Rust state and installs no store: it is handed
 * exactly one staged capture and edits it locally, persisting each operation
 * through `save_capture_timeline`. There is no state stream to subscribe to,
 * which is why `init()`-per-window (the rule the buddy and panel roots follow
 * for the capture stores) does not apply here.
 *
 * It does subscribe to ONE event, and that is not an exception to the above:
 * `editor:open` carries no state, it is the edge that says "read the stash
 * again". `window_close.rs` answers this window's close with
 * `prevent_close()` + `hide()`, so the webview mounts exactly ONCE per
 * process — draining `take_editor_request` only from `onMounted` would open
 * the first capture and then show it forever while every later request sat
 * in the stash unread (`editor_commands.rs`'s module doc, at length).
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { computed, onBeforeUnmount, onMounted, ref, shallowRef } from "vue";

import CapturePreview from "../components/editor/CapturePreview.vue";
import ExportBar from "../components/editor/ExportBar.vue";
import TimelineStrip from "../components/editor/TimelineStrip.vue";
import AppButton from "../components/ui/AppButton.vue";
import Banner from "../components/ui/Banner.vue";
import { useEditorSelection } from "../composables/useEditorSelection";
import { useEditorTimeline } from "../composables/useEditorTimeline";
import { logWarning } from "../logging";
import type {
  ExportFailure,
  ExportProgress,
  ExportResult,
  StagedCaptureDetail,
  TimelineDto,
} from "../types";
import { wholeTimeline } from "../utils/timelineGeometry";

const detail = ref<StagedCaptureDetail | null>(null);
const error = ref<string | null>(null);
const noCapture = ref(false);

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
 * state, and the seam every operation re-indexes underneath. `EditorRoot` is
 * the only place that knows both the operation and the selection, so the
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

/** Spec 8.3's export, as this window sees it. EVERY one of these is reset by
 * `load()` (`resetExport`): the editor window is HIDDEN and REUSED, so a ref
 * that survives a capture is a chance to show the previous one's state under
 * the new one's title — and a stale `done` is the sharpest of them, because
 * `done` is the one state with no Save button in it at all. */
const exportPhase = ref<"idle" | "exporting" | "done" | "failed">("idle");
const exportFraction = ref(0);
const exportMessage = ref<string | null>(null);
/** What Open launches: the NOTE when the vault writes one, the video
 * otherwise. The note is the richer destination — it embeds the video, the
 * way the audio domain's note embeds its audio — and with notes turned off
 * there is none, so the video is the only answer.
 * `staged_commands::capture_file_param` takes either: it drops the extension
 * for exactly-`.md` and keeps it otherwise. */
const savedPath = ref<string | null>(null);
const savedVaultId = ref<string | null>(null);
const discardBusy = ref(false);

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

/** Every `screen:export*` event is emitted APP-WIDE and carries the base it
 * is about. This window edits exactly one capture and is reused, so an editor
 * reopened on B while A is still exporting would otherwise render A's
 * progress — and, worse, A's "Saved" under B's title. */
function addressesOpenCapture(base: string): boolean {
  return detail.value !== null && detail.value.base === base;
}

function onExportProgress(p: ExportProgress) {
  if (!addressesOpenCapture(p.base)) return;
  // A progress tick IS an export running, so it drives the phase as well as
  // the number: the bar cannot show a fraction it is not rendering a bar for.
  exportPhase.value = "exporting";
  exportFraction.value = p.fraction;
}

function onExported(p: ExportResult) {
  if (!addressesOpenCapture(p.base)) return;
  exportPhase.value = "done";
  exportFraction.value = 1;
  savedPath.value = p.notePath ?? p.videoPath;
  savedVaultId.value = p.vaultId;
  // A warning means the video landed and its note did not: a degraded
  // SUCCESS. Dropping it would leave the user believing in a note that is
  // not there.
  // `vaultName` and not `vaultId`: the id is Obsidian's opaque hex registry
  // key, which names nothing to a human. A payload written by an older build
  // (or a test) carries no name, so fall back rather than render "undefined".
  exportMessage.value =
    p.warning ??
    (p.vaultName ? `Saved ${p.base} to ${p.vaultName}.` : `Saved ${p.base} into your vault.`);
}

/** Spec 14: a cancel keeps the staged capture AND its timeline. There is
 * nothing to apologise for, so no message and no banner — the bar returns to
 * exactly the state Save was pressed from. */
function onExportCancelled(p: { base: string }) {
  if (!addressesOpenCapture(p.base)) return;
  resetExport();
}

function onExportFailed(p: ExportFailure) {
  if (!addressesOpenCapture(p.base)) return;
  exportPhase.value = "failed";
  exportFraction.value = 0;
  exportMessage.value = p.message;
}

function resetExport() {
  exportPhase.value = "idle";
  exportFraction.value = 0;
  exportMessage.value = null;
  savedPath.value = null;
  savedVaultId.value = null;
}

async function onSave() {
  const base = detail.value?.base;
  if (base === undefined) return;
  exportPhase.value = "exporting";
  exportFraction.value = 0;
  exportMessage.value = null;
  try {
    await invoke("export_and_save_capture", { base });
  } catch (e) {
    // The command REJECTS for anything it decides before a worker starts — no
    // ffmpeg, a refused base, no disk space. No `screen:exportFailed` follows
    // one of those, so a root that only listened would sit at "exporting"
    // forever.
    exportPhase.value = "failed";
    exportMessage.value = String(e);
  }
}

async function onCancelExport() {
  try {
    await invoke("cancel_export");
  } catch (e) {
    // The terminal state still arrives as `screen:exportCancelled` or, if
    // the export had already finished, as `screen:exported`.
    logWarning(`cancel_export failed: ${String(e)}`);
  }
}

/** The bar confirms first; this is the second click. The staged capture is
 * gone afterwards, so the window stops offering it rather than leaving Save
 * pointed at a sidecar that no longer exists. */
async function onDiscard() {
  const base = detail.value?.base;
  if (base === undefined) return;
  discardBusy.value = true;
  try {
    await invoke("discard_staged_capture", { base });
    detail.value = null;
    noCapture.value = true;
    resetExport();
  } catch (e) {
    exportPhase.value = "failed";
    exportMessage.value = String(e);
  } finally {
    discardBusy.value = false;
  }
}

async function onOpenSaved() {
  const path = savedPath.value;
  const id = savedVaultId.value;
  if (path === null || id === null) return;
  try {
    await invoke("open_screen_capture", { id, path });
  } catch (e) {
    exportPhase.value = "failed";
    exportMessage.value = String(e);
  }
}

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
    noCapture.value = false;
    error.value = null;
  } catch (e) {
    error.value = String(e);
  }
}

/** Drain the stash and open whatever it held. Runs on mount AND on every
 * `editor:open`. */
async function openRequested() {
  let base: string | null = null;
  try {
    base = await invoke<string | null>("take_editor_request");
  } catch (e) {
    logWarning(`take_editor_request failed: ${String(e)}`);
  }
  if (base === null) {
    // An empty stash means "nothing new", never "close what you are
    // showing": blanking a live edit on a spurious or double-fired event
    // would be strictly worse than doing nothing.
    noCapture.value = detail.value === null;
    return;
  }
  await load(base);
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
  // Subscribed BEFORE the first drain, so a request arriving while that
  // drain is in flight is not lost. The export subscriptions ride the same
  // rule: `export_and_save_capture` can only be pressed from a loaded
  // capture, but an export left running when this window was last hidden is
  // still emitting, and the bar has to be able to see it finish.
  unlisteners.push(
    await listen("editor:open", () => void openRequested()),
    await listen<ExportProgress>("screen:exportProgress", (e) => onExportProgress(e.payload)),
    await listen<ExportResult>("screen:exported", (e) => onExported(e.payload)),
    await listen<{ base: string }>("screen:exportCancelled", (e) => onExportCancelled(e.payload)),
    await listen<ExportFailure>("screen:exportFailed", (e) => onExportFailed(e.payload)),
  );
  await openRequested();
});
onBeforeUnmount(() => {
  for (const off of unlisteners) off();
  // The listener is on `window`, which outlives this component. Leaving it
  // behind would keep an unmounted editor editing — and PERSISTING, through
  // the composable this closure still holds — a timeline nobody can see.
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <main
    class="flex h-screen w-screen flex-col gap-3 overflow-y-auto bg-slate-900 p-4 text-fg"
  >
    <!-- A load failure REPLACES the editor rather than sitting beside it:
         there is no capture behind it to edit. The save-failure banner
         further down is the opposite case and renders INSIDE the editor,
         because there the edit is on screen and still usable. -->
    <Banner
      v-if="error"
      data-testid="editor-error"
      tone="danger"
    >
      {{ error }}
    </Banner>
    <!-- Not an error: the window is hidden and reused, so the user can
         alt-tab back to an editor whose stash is empty. -->
    <p
      v-else-if="noCapture"
      class="m-auto text-sm text-fg-muted"
    >
      No capture open. Pick one from Record Screen.
    </p>
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
      Loading capture…
    </p>
  </main>
</template>
