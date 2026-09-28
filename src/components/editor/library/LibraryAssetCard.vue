<script setup lang="ts">
/**
 * One asset row in the media library (Task 25; visual-parity Task 9,
 * concept spec §3.2): a 58×40 thumbnail (or a teal audio tile), the name and
 * meta line, and a trailing button that either adds the asset at the
 * playhead or, for a missing original, reconnects it. Presentational --
 * `MediaLibrary.vue` computes the meta text, the thumbnail instant and the
 * "+" target name, and owns the actual insert/reconnect commands; this only
 * renders them and emits the three things a row can do.
 *
 * **Native drag source (Task 26)**: unchanged -- the whole `<li>` is
 * `draggable` unless the asset is `missing`; dropping it onto a compatible
 * timeline lane, or below the last one, is the second way to place it.
 *
 * **Right-click / Shift+F10 (visual-parity Task 9, concept spec §8)** opens
 * the asset's context menu (`assetMenu`, Task 5) through
 * `MediaLibrary.vue`'s own `ContextMenu` -- this only reports WHERE, via the
 * same `context-menu` payload shape `ClipItem.vue` emits, so both surfaces
 * share one handler shape rather than growing a second one.
 *
 * **Focusable, Enter adds** (concept spec §3.2 point 4): the row itself
 * takes focus, and Enter/Space (when the row itself, not its trailing
 * button, has focus -- the button already answers its own) runs the row's
 * primary action: add, or reconnect while missing.
 */
import { computed } from "vue";

import { useAssetThumbnail } from "../../../composables/useAssetThumbnail";
import { isContextMenuShortcut } from "../../../editor/shortcuts";
import { setAssetDragData } from "../../../editor/trackCompat";
import type { AssetKind } from "../../../editorTypes";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{
  id: string;
  name: string;
  kind: AssetKind;
  /** "MM:SS · W × H" / "MM:SS · Local audio" / "Missing source" (§3.2). */
  meta: string;
  missing: boolean;
  /** The instant (ms) to thumbnail at, or `null` for an audio or missing
   * row, which never asks Rust for one. */
  thumbAtMs: number | null;
  /** The track "+"/Enter would insert onto (for the title). */
  targetName: string;
}>();

const emit = defineEmits<{
  (e: "insert"): void;
  (e: "reconnect"): void;
  (e: "context-menu", payload: { clientX: number; clientY: number }): void;
}>();

const { src } = useAssetThumbnail(
  () => props.id,
  () => props.thumbAtMs,
);

const fallbackIcon = computed(() => (props.kind === "audio" ? "music" : "video"));
const addTitle = computed(() => `Add at the playhead on ${props.targetName}`);

function primaryAction(): void {
  if (props.missing) emit("reconnect");
  else emit("insert");
}

function onDragStart(event: DragEvent): void {
  if (props.missing || !event.dataTransfer) return;
  setAssetDragData(event.dataTransfer, props.id, props.kind);
}

function onContextMenu(event: MouseEvent): void {
  emit("context-menu", { clientX: event.clientX, clientY: event.clientY });
}

/** Shift+F10/Menu opens the context menu wherever focus is in the row;
 * Enter/Space runs the row's own primary action, but only when the ROW
 * itself is focused -- the trailing button already answers its own Enter/
 * Space, and letting this bubble handler answer too would double-fire it. */
function onKeydown(event: KeyboardEvent): void {
  if (isContextMenuShortcut(event)) {
    event.preventDefault();
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    emit("context-menu", { clientX: rect.left, clientY: rect.bottom });
    return;
  }
  if (event.target !== event.currentTarget) return;
  if (event.key === "Enter" || event.key === " ") {
    event.preventDefault();
    primaryAction();
  }
}
</script>

<template>
  <li
    :data-testid="`library-asset-${id}`"
    :draggable="!missing"
    tabindex="0"
    class="flex items-center gap-[9px] border-b border-line py-[9px] focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
    @dragstart="onDragStart"
    @contextmenu.prevent="onContextMenu"
    @keydown="onKeydown"
  >
    <div
      :data-testid="`library-asset-${id}-thumb`"
      class="flex h-10 w-[58px] shrink-0 items-center justify-center overflow-hidden rounded-[5px]"
      :class="kind === 'audio' ? 'bg-audio-bg text-audio' : 'bg-video-bg text-video'"
    >
      <img
        v-if="src"
        :src="src"
        alt=""
        draggable="false"
        class="h-full w-full object-cover"
      >
      <EditorIcon
        v-else
        :name="fallbackIcon"
        :size="20"
      />
    </div>
    <div class="min-w-0 flex-1">
      <b
        :title="name"
        class="block truncate text-[11px] font-medium leading-[1.35] text-fg"
      >{{ name }}</b>
      <div
        class="mt-1 truncate text-[9px] text-fg-muted"
        :class="{ 'text-danger-fg': missing }"
      >
        {{ meta }}
      </div>
    </div>
    <button
      v-if="!missing"
      type="button"
      :data-testid="`library-asset-${id}-add`"
      :aria-label="`Add ${name} to timeline`"
      :title="addTitle"
      class="flex h-[30px] w-7 shrink-0 items-center justify-center rounded p-1 text-fg hover:bg-hover focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      @click="emit('insert')"
    >
      <EditorIcon
        name="plus"
        :size="16"
      />
    </button>
    <button
      v-else
      type="button"
      :data-testid="`library-asset-${id}-add`"
      :aria-label="`Reconnect ${name}`"
      title="Reconnect original media"
      class="flex h-[30px] w-7 shrink-0 items-center justify-center rounded p-1 text-fg hover:bg-hover focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      @click="emit('reconnect')"
    >
      <EditorIcon
        name="link"
        :size="16"
      />
    </button>
  </li>
</template>
