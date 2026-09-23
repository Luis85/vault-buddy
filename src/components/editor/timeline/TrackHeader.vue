<script setup lang="ts">
/**
 * One track's own controls (Task 23; F-06): name (inline rename), the
 * visibility eye (video tracks only -- audio has nothing to hide, the
 * reference editor's own `t.kind==='video'` gate), lock, mute, solo, a
 * volume slider (audio tracks only -- video's own `Track.volume` exists in
 * the model but has no consumer here yet; a mixer landing later is where a
 * VIDEO track's volume would get a control), and a track menu (Move up/
 * Move down/Delete track). Replaces `TrackLane.vue`'s previous static
 * name-only label column.
 *
 * Every mutation goes straight to `editorProject.execute` (the
 * `ClipItem.vue`/`TimelineToolbar.vue` precedent of a leaf component
 * calling the store directly rather than emitting purely upward) --
 * `setTrackFlags`/`renameTrack`/`moveTrack`/`deleteTrack`, never a local
 * write (R14).
 *
 * **Locked-track gating stays entirely on THIS side of the wire, mirroring
 * Rust's own rule (`core::editor::commands::tracks`) rather than
 * re-deriving it**: every control except Lock itself is disabled
 * (`aria-disabled`, never the native `disabled` attribute -- the
 * `TimelineToolbar.vue`/`ContextMenu.vue` precedent of keeping a disabled
 * control reachable so its `title` explains why) with the SAME reason text
 * `actionMeta.ts`'s `lockedReason` already gives a locked track's clip
 * actions, so a locked track never shows two different explanations for
 * the same fact. Lock/unlock is the one control that is NEVER disabled --
 * the brief's own carve-out, and `setTrackFlags`' own Rust-side rule.
 *
 * **Every `title`/`class` ternary lives in a named computed, not inline in
 * the template** (fallow's own template-complexity gate flagged the first
 * draft CRITICAL at 26 cyclomatic once seven controls each carried two or
 * three inline conditionals) -- moving the branching into `<script>`
 * leaves the template itself close to a flat list of bindings, and each
 * computed here is one or two conditions, individually far under any
 * complexity threshold.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import type { Track } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";

const props = defineProps<{
  track: Track;
  /** This lane's own index and the total track count -- what the track
   * menu's Move up/Move down need to disable at either boundary. */
  trackIndex: number;
  trackCount: number;
}>();

const editorProject = useEditorProjectStore();

const locked = computed(() => props.track.locked);
const reason = computed(() => lockedReason(props.track.name));
/** Only the ONE reusable pair a disabled-while-locked control needs. */
const disabledClass = "cursor-default opacity-50";
const enabledClass = "cursor-pointer";

function setFlags(patch: {
  visible?: boolean;
  locked?: boolean;
  muted?: boolean;
  solo?: boolean;
  volume?: number;
}) {
  void editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, ...patch });
}

function toggleVisible() {
  if (locked.value) return;
  setFlags({ visible: !props.track.visible });
}
function toggleLocked() {
  setFlags({ locked: !props.track.locked });
}
function toggleMuted() {
  if (locked.value) return;
  setFlags({ muted: !props.track.muted });
}
function toggleSolo() {
  if (locked.value) return;
  setFlags({ solo: !props.track.solo });
}
function onVolumeChange(event: Event) {
  if (locked.value) return;
  const value = Number((event.target as HTMLInputElement).value);
  setFlags({ volume: value });
}

// ---- computed title/class (kept OUT of the template — see the module doc) --

const eyeTitle = computed(() => (locked.value ? reason.value : props.track.visible ? "Hide track" : "Show track"));
const eyeClass = computed(() => [
  locked.value ? disabledClass : enabledClass,
  props.track.visible ? "text-fg-secondary" : "text-fg-subtle",
]);
const lockTitle = computed(() => (props.track.locked ? "Unlock track" : "Lock track"));
const lockClass = computed(() => (props.track.locked ? "text-accent-fg" : "text-fg-secondary"));
const muteTitle = computed(() => (locked.value ? reason.value : "Mute track"));
const muteClass = computed(() => [
  locked.value ? disabledClass : enabledClass,
  props.track.muted ? "bg-accent/20 text-accent-fg" : "text-fg-secondary",
]);
const soloTitle = computed(() => (locked.value ? reason.value : "Solo track"));
const soloClass = computed(() => [
  locked.value ? disabledClass : enabledClass,
  props.track.solo ? "bg-accent/20 text-accent-fg" : "text-fg-secondary",
]);
const volumeTitle = computed(() => (locked.value ? reason.value : "Track volume"));
/** `aria-label`, not just `title` (fix round 1): the button controls carry
 * a visible glyph plus a title, but a bare `<input type="range">` has no
 * accessible name at all otherwise -- "volume" alone would be ambiguous
 * once more than one track exists on the timeline. */
const volumeLabel = computed(() => `${props.track.name} volume`);
/** `aria-disabled`, never the native `disabled` attribute (fix round 1) --
 * this control had drifted from every sibling's own rule (see the module
 * doc): a native `disabled` range input drops out of the tab order and, in
 * most browsers/AT, stops reliably surfacing its `title`, making a locked
 * track's volume control LESS explorable than mute/solo/eye next to it.
 * `onVolumeChange`'s own `if (locked.value) return;` guard already refuses
 * the change while locked, so removing `disabled` does not reopen
 * anything `setTrackFlags` wouldn't refuse anyway. */
const volumeClass = computed(() => (locked.value ? disabledClass : enabledClass));

// ---- inline rename ----------------------------------------------------------

const editing = ref(false);
const draft = ref(props.track.name);
const nameInput = ref<HTMLInputElement | null>(null);

/** The draft follows the committed name whenever it changes from OUTSIDE
 * (an undo, a rename landing from elsewhere) -- unless the user is
 * mid-edit, the `useInspectorDraft.ts` precedent for a field that must not
 * clobber a live keystroke with a stale server echo. */
watch(
  () => props.track.name,
  (name) => {
    if (!editing.value) draft.value = name;
  },
);

const nameTitle = computed(() => (locked.value ? reason.value : "Rename track"));
const nameClass = computed(() => (locked.value ? "cursor-default opacity-70" : "cursor-pointer"));

function beginRename() {
  if (locked.value) return;
  editing.value = true;
  draft.value = props.track.name;
  void nextTick(() => nameInput.value?.select());
}
function commitRename() {
  editing.value = false;
  const trimmed = draft.value.trim();
  if (trimmed === "" || trimmed === props.track.name) {
    draft.value = props.track.name;
    return;
  }
  void editorProject.execute({ kind: "renameTrack", trackId: props.track.id, name: trimmed });
}
function cancelRename() {
  editing.value = false;
  draft.value = props.track.name;
}

// ---- track menu ---------------------------------------------------------

const menuOpen = ref(false);
const menuRoot = ref<HTMLElement | null>(null);

function onWindowPointerDown(event: PointerEvent) {
  if (!menuOpen.value) return;
  if (menuRoot.value && !menuRoot.value.contains(event.target as Node)) menuOpen.value = false;
}
function onWindowKeydown(event: KeyboardEvent) {
  if (menuOpen.value && event.key === "Escape") menuOpen.value = false;
}
onMounted(() => {
  window.addEventListener("pointerdown", onWindowPointerDown);
  window.addEventListener("keydown", onWindowKeydown);
});
onBeforeUnmount(() => {
  window.removeEventListener("pointerdown", onWindowPointerDown);
  window.removeEventListener("keydown", onWindowKeydown);
});

const canMoveUp = computed(() => !locked.value && props.trackIndex > 0);
const canMoveDown = computed(() => !locked.value && props.trackIndex < props.trackCount - 1);
const canDelete = computed(() => !locked.value);

const menuItemTitle = computed(() => (locked.value ? reason.value : undefined));
function menuItemClass(enabled: boolean): string {
  return enabled ? "cursor-pointer text-fg-secondary" : "cursor-default text-fg-subtle opacity-50";
}
const moveUpClass = computed(() => menuItemClass(canMoveUp.value));
const moveDownClass = computed(() => menuItemClass(canMoveDown.value));
const deleteClass = computed(() => (canDelete.value ? "cursor-pointer text-danger-fg" : "cursor-default text-fg-subtle opacity-50"));

function moveUp() {
  menuOpen.value = false;
  if (!canMoveUp.value) return;
  void editorProject.execute({ kind: "moveTrack", trackId: props.track.id, toIndex: props.trackIndex - 1 });
}
function moveDown() {
  menuOpen.value = false;
  if (!canMoveDown.value) return;
  void editorProject.execute({ kind: "moveTrack", trackId: props.track.id, toIndex: props.trackIndex + 1 });
}
function deleteTrack() {
  menuOpen.value = false;
  if (!canDelete.value) return;
  void editorProject.execute({ kind: "deleteTrack", trackId: props.track.id });
}
</script>

<template>
  <div
    :data-testid="`track-header-${track.id}`"
    class="flex items-center gap-1 truncate px-1"
  >
    <input
      v-if="editing"
      ref="nameInput"
      :data-testid="`track-header-${track.id}-name-input`"
      class="w-0 min-w-0 flex-1 rounded border border-line bg-stage px-1 text-micro text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      :value="draft"
      @input="draft = ($event.target as HTMLInputElement).value"
      @keydown.enter="commitRename"
      @keydown.escape="cancelRename"
      @blur="commitRename"
    >
    <button
      v-else
      type="button"
      :data-testid="`track-header-${track.id}-name`"
      :aria-pressed="editing"
      :aria-disabled="locked"
      :title="nameTitle"
      class="min-w-0 flex-1 truncate rounded px-0.5 text-left text-micro text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      :class="nameClass"
      @click="beginRename"
    >
      {{ track.name }}
    </button>

    <button
      v-if="track.kind === 'video'"
      type="button"
      :data-testid="`track-header-${track.id}-visible`"
      :aria-pressed="track.visible"
      :aria-disabled="locked"
      :title="eyeTitle"
      class="shrink-0 rounded px-1 hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      :class="eyeClass"
      @click="toggleVisible"
    >
      &#128065;
    </button>

    <button
      type="button"
      :data-testid="`track-header-${track.id}-lock`"
      :aria-pressed="track.locked"
      :title="lockTitle"
      class="shrink-0 cursor-pointer rounded px-1 hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      :class="lockClass"
      @click="toggleLocked"
    >
      &#128274;
    </button>

    <button
      type="button"
      :data-testid="`track-header-${track.id}-mute`"
      :aria-pressed="track.muted"
      :aria-disabled="locked"
      :title="muteTitle"
      class="shrink-0 rounded px-1 text-micro hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      :class="muteClass"
      @click="toggleMuted"
    >
      M
    </button>
    <button
      type="button"
      :data-testid="`track-header-${track.id}-solo`"
      :aria-pressed="track.solo"
      :aria-disabled="locked"
      :title="soloTitle"
      class="shrink-0 rounded px-1 text-micro hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      :class="soloClass"
      @click="toggleSolo"
    >
      S
    </button>

    <input
      v-if="track.kind === 'audio'"
      :data-testid="`track-header-${track.id}-volume`"
      type="range"
      min="0"
      max="2"
      step="0.01"
      :value="track.volume"
      :aria-disabled="locked"
      :aria-label="volumeLabel"
      :title="volumeTitle"
      class="w-10 shrink-0 accent-violet-500"
      :class="volumeClass"
      @change="onVolumeChange"
    >

    <div
      ref="menuRoot"
      class="relative shrink-0"
    >
      <button
        type="button"
        :data-testid="`track-header-${track.id}-menu`"
        aria-haspopup="menu"
        :aria-expanded="menuOpen"
        :aria-pressed="menuOpen"
        title="Track menu"
        class="cursor-pointer rounded px-1 text-fg-secondary hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
        @click="menuOpen = !menuOpen"
      >
        &#8942;
      </button>
      <div
        v-if="menuOpen"
        role="menu"
        :data-testid="`track-header-${track.id}-menu-list`"
        class="absolute right-0 top-full z-40 mt-1 flex min-w-32 flex-col gap-0.5 rounded-control border border-white/10 bg-slate-800 p-1 text-micro shadow-lg"
      >
        <button
          type="button"
          role="menuitem"
          :data-testid="`track-header-${track.id}-move-up`"
          :aria-disabled="!canMoveUp"
          :title="menuItemTitle"
          class="rounded px-1.5 py-0.5 text-left hover:bg-white/10"
          :class="moveUpClass"
          @click="moveUp"
        >
          Move up
        </button>
        <button
          type="button"
          role="menuitem"
          :data-testid="`track-header-${track.id}-move-down`"
          :aria-disabled="!canMoveDown"
          :title="menuItemTitle"
          class="rounded px-1.5 py-0.5 text-left hover:bg-white/10"
          :class="moveDownClass"
          @click="moveDown"
        >
          Move down
        </button>
        <button
          type="button"
          role="menuitem"
          :data-testid="`track-header-${track.id}-delete`"
          :aria-disabled="!canDelete"
          :title="menuItemTitle"
          class="rounded px-1.5 py-0.5 text-left hover:bg-white/10"
          :class="deleteClass"
          @click="deleteTrack"
        >
          Delete track
        </button>
      </div>
    </div>
  </div>
</template>
