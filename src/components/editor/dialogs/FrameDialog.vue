<script setup lang="ts">
/**
 * **Frame your tutorial** (visual-parity Task 11; concept spec §9.11,
 * `contextual-ui.js: showCanvasDialog`): the preview header's ratio button
 * opens it. Four format cards — Landscape 16:9, Portrait 9:16, Square 1:1,
 * Classic 4:3 — each with its shape, its name and its size; the current one
 * is pressed. It replaces the toolbar's native `<select>` (Task 32).
 *
 * The sizes are `FRAME_FORMATS` (`editor/previewHeader.ts`), which mirror
 * `core::editor::limits::CANVASES` — Rust accepts exactly these four.
 * Choosing another format sends ONE `setCanvas` and closes; choosing the
 * current one, or "Keep current format", closes without an edit (Rust
 * would refuse a canvas that is already the project's). A refused change
 * keeps the dialog open; the shell's feedback toast says why (visual-parity
 * Task 7).
 *
 * A change raises a toast naming Checks, with **Open Checks** (Task 32's
 * F-38 "crop/caption warnings prompt a review", Task 54's action): the
 * canvas moved under every source, text cue and caption, and Checks lists
 * what no longer fits. A second change replaces the first toast rather
 * than stacking it (an actionable toast is never deduped by the store).
 * The concept's "30 fps browser-review output" sentence is browser-only
 * copy (design D10) and is not carried over.
 */
import { computed, ref } from "vue";

import type { FrameFormat } from "../../../editor/previewHeader";
import { FRAME_FORMATS } from "../../../editor/previewHeader";
import { openChecks } from "../../../editor/revealBus";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useNotificationsStore } from "../../../stores/notifications";
import DialogHost from "../shell/DialogHost.vue";
import DialogButton from "./DialogButton.vue";

defineProps<{ open: boolean }>();
const emit = defineEmits<(e: "close") => void>();

/** The concept's shape box: at most 65 x 68, in the format's own ratio. */
const SHAPE_MAX_W = 65;
const SHAPE_MAX_H = 68;
function shapeStyle(f: FrameFormat): Record<string, string> {
  const width = Math.min(SHAPE_MAX_W, (SHAPE_MAX_H * f.width) / f.height);
  return { width: `${Math.round(width)}px`, height: `${Math.round((width * f.height) / f.width)}px` };
}

const CANVAS_TOAST_MESSAGE = "Canvas changed. Review crop, text and caption placement in Checks.";
/** Long enough to reach the action, never sticky: the change is made. */
const CANVAS_TOAST_MS = 8_000;
/** The live canvas toast, module-wide, so a second change replaces it. */
let canvasToast: number | null = null;

const editorProject = useEditorProjectStore();
const notifications = useNotificationsStore();
const busy = ref(false);

const current = computed(() => editorProject.project?.canvas ?? null);
function isCurrent(f: FrameFormat): boolean {
  return current.value?.width === f.width && current.value?.height === f.height;
}

async function choose(f: FrameFormat): Promise<void> {
  if (busy.value) return;
  if (isCurrent(f)) {
    emit("close");
    return;
  }
  busy.value = true;
  const ok = await editorProject.execute({ kind: "setCanvas", width: f.width, height: f.height });
  busy.value = false;
  if (!ok) return;
  if (canvasToast !== null) notifications.dismiss(canvasToast);
  canvasToast = notifications.notify("info", CANVAS_TOAST_MESSAGE, {
    ttlMs: CANVAS_TOAST_MS,
    action: { label: "Open Checks", run: openChecks },
  });
  emit("close");
}

function keep(): void {
  if (!busy.value) emit("close");
}
</script>

<template>
  <DialogHost
    :open="open"
    label="Frame your tutorial"
    :closable="!busy"
    :close-reason="busy ? 'Changing the format…' : null"
    close-testid="frame-dialog-close"
    @close="keep"
  >
    <template #title>
      Frame your tutorial
    </template>

    <div
      data-testid="frame-dialog"
      class="grid grid-cols-4 gap-2.5 max-[800px]:grid-cols-2"
    >
      <button
        v-for="f in FRAME_FORMATS"
        :key="`${f.width}x${f.height}`"
        type="button"
        :data-testid="`frame-choice-${f.width}x${f.height}`"
        :aria-pressed="isCurrent(f)"
        :disabled="busy"
        class="flex min-h-[175px] flex-col items-center justify-center gap-[9px] rounded-[9px] border px-[5px] py-3 text-[11px]"
        :class="isCurrent(f) ? 'border-accent bg-accent-bg' : 'border-line bg-app'"
        @click="choose(f)"
      >
        <span
          aria-hidden="true"
          class="block rounded border-2 border-accent bg-raised"
          :style="shapeStyle(f)"
        />
        <b class="font-semibold">{{ f.name }} · {{ f.ratio }}</b>
        <small class="text-[9px] text-fg-muted">{{ f.width }} × {{ f.height }}</small>
      </button>
    </div>
    <p class="text-[10px] text-fg-muted">
      Original files stay untouched. Review crops, callout positions and caption wrapping after switching.
    </p>

    <template #footer>
      <DialogButton
        data-testid="frame-dialog-keep"
        :reason="busy ? 'Changing the format…' : null"
        @click="keep"
      >
        Keep current format
      </DialogButton>
    </template>
  </DialogHost>
</template>
