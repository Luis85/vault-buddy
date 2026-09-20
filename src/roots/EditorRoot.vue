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
import TimelineStrip from "../components/editor/TimelineStrip.vue";
import AppButton from "../components/ui/AppButton.vue";
import Banner from "../components/ui/Banner.vue";
import { useEditorTimeline } from "../composables/useEditorTimeline";
import { logWarning } from "../logging";
import type { StagedCaptureDetail, TimelineDto } from "../types";

const detail = ref<StagedCaptureDetail | null>(null);
const error = ref<string | null>(null);
const noCapture = ref(false);
const selected = ref<number | null>(null);
const playheadMs = ref(0);

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

async function load(base: string) {
  try {
    const loaded = await invoke<StagedCaptureDetail>("load_staged_capture", { base });
    // An untouched capture opens as ONE segment spanning the whole
    // recording: the timeline the sidecar does not carry yet.
    const seed: TimelineDto = loaded.timeline ?? {
      segments: [{ sourceStartMs: 0, sourceEndMs: loaded.durationMs }],
    };
    editor.value = useEditorTimeline(loaded.base, seed);
    detail.value = loaded;
    selected.value = null;
    playheadMs.value = 0;
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
  editor.value?.splitAt(playheadMs.value);
}
function onDelete() {
  if (selected.value === null) return;
  editor.value?.deleteSegment(selected.value);
  selected.value = null;
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
  editor.value?.reorder(from, slot > from ? slot - 1 : slot);
  selected.value = null;
}
function onUndo() {
  editor.value?.undo();
}
function onRedo() {
  editor.value?.redo();
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
 * from any keystroke, including one typed into a field a later phase adds
 * (spec 10's Save dialog is exactly that). */
function onKeydown(e: KeyboardEvent) {
  if (!e.ctrlKey && !e.metaKey) return;
  const key = e.key.toLowerCase();
  if (key === "z" && !e.shiftKey) {
    e.preventDefault();
    onUndo();
  } else if ((key === "z" && e.shiftKey) || key === "y") {
    e.preventDefault();
    onRedo();
  }
}

let unlistenOpen: (() => void) | undefined;

onMounted(async () => {
  // Registered synchronously, ahead of the awaits below: a `listen` that
  // never resolves must not cost the user their keyboard.
  window.addEventListener("keydown", onKeydown);
  // Subscribed BEFORE the first drain, so a request arriving while that
  // drain is in flight is not lost.
  unlistenOpen = await listen("editor:open", () => void openRequested());
  await openRequested();
});
onBeforeUnmount(() => {
  unlistenOpen?.();
  // The listener is on `window`, which outlives this component. Leaving it
  // behind would keep an unmounted editor editing — and PERSISTING, through
  // the composable this closure still holds — a timeline nobody can see.
  window.removeEventListener("keydown", onKeydown);
});
</script>

<template>
  <main class="flex h-screen w-screen flex-col gap-3 bg-slate-900 p-4 text-fg">
    <Banner
      v-if="error"
      data-testid="editor-error"
      tone="danger"
    >
      {{ error }}
    </Banner>
    <p
      v-else-if="noCapture"
      class="m-auto text-sm text-fg-muted"
    >
      No capture open. Pick one from Record Screen.
    </p>
    <template v-else-if="detail">
      <header class="flex items-baseline justify-between">
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
        :src="src"
        :timeline="timeline"
        :output-ms="playheadMs"
        @update:output-ms="playheadMs = $event"
      />
      <TimelineStrip
        :timeline="timeline"
        :selected="selected"
        :playhead-ms="playheadMs"
        @select="selected = $event"
        @reorder="onReorder"
      />
      <div class="flex gap-2">
        <AppButton
          data-testid="editor-split"
          variant="secondary"
          @click="onSplit"
        >
          Split
        </AppButton>
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
      <p class="text-micro text-fg-subtle">
        Saving into a vault arrives in a later update.
      </p>
    </template>
    <p
      v-else
      class="m-auto text-sm text-fg-muted"
    >
      Loading capture…
    </p>
  </main>
</template>
