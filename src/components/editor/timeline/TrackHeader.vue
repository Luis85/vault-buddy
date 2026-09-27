<script setup lang="ts">
/**
 * One track's header (Task 23; F-06; visual-parity Task 17, concept spec
 * §6.4): a 28px mono badge (V3/V2/V1 top to bottom, A1…), the title, and a
 * row of 26px controls — the eye (video tracks only: audio has nothing to
 * hide), **M**, **S** and the padlock. An active control reads gold; the
 * eye is "active" while the track is HIDDEN, as the concept draws it.
 *
 * The badge and the title select the track (`editorWorkspace.selectTrack`)
 * and show the inspector, which then reads Track properties — the
 * concept's `data-select-track`. At a drawer width the inspector is closed,
 * so the click also reveals it: selecting a track the user cannot see
 * would be a click with no visible effect (D14).
 *
 * There is no ⋮ button (ruling P4): the track menu (`menuSets.trackMenu`)
 * opens from a right-click on the header, or Shift+F10 / the Menu key while
 * focus is in it — `TrackLane` owns that, since the label cell is its
 * element. Its "Rename track…" comes back here through
 * `revealBus.trackRenameRequest` and opens the inline rename; its "Remove
 * track…" goes through `trackRemoval.ts`'s confirm, the one removal path.
 * Track volume lives in the inspector's Track properties and the mixer.
 *
 * Every flag goes straight to `editorProject.execute` as `setTrackFlags`
 * (the `ClipItem.vue` precedent of a leaf calling the store), never a local
 * write (R14). **Locked-track gating mirrors Rust's rule
 * (`core::editor::commands::tracks`)**: every flag but the padlock itself is
 * refused while the track is locked — `aria-disabled`, never native
 * `disabled`, so the control stays reachable and its `title` says why, with
 * the same `lockedReason` text a locked track's clip actions give.
 *
 * Titles and classes live in computeds, keeping the template a flat list of
 * bindings (fallow's template-complexity gate).
 */
import { computed, nextTick, ref, watch } from "vue";

import { lockedReason } from "../../../editor/actionMeta";
import { trackRenameRequest } from "../../../editor/revealBus";
import type { Track } from "../../../editorTypes";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";

const props = defineProps<{
  track: Track;
  /** `V3`, `A1`… (`timelineLayout.trackBadges`). */
  badge: string;
}>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const locked = computed(() => props.track.locked);
const reason = computed(() => lockedReason(props.track.name));

type Flag = "visible" | "muted" | "solo" | "locked";

/** Flip one flag; everything but the padlock is refused while locked. */
function toggle(flag: Flag) {
  if (locked.value && flag !== "locked") return;
  const change: Partial<Record<Flag, boolean>> = { [flag]: !props.track[flag] };
  void editorProject.execute({ kind: "setTrackFlags", trackId: props.track.id, ...change });
}

function showProperties() {
  workspace.selectTrack(props.track.id);
  workspace.revealInspector();
}

// ---- computed titles / classes (kept OUT of the template) -----------------

const CONTROL = "inline-flex h-[26px] min-h-[26px] min-w-[26px] items-center justify-center rounded-[4px] p-1 text-[10px]";
const ACTIVE = "vb-track-active bg-gold-bg text-gold";
const IDLE = "text-fg-secondary hover:bg-hover";

function controlClass(active: boolean, gated: boolean): string[] {
  return [CONTROL, active ? ACTIVE : IDLE, gated && locked.value ? "cursor-default opacity-50" : "cursor-pointer"];
}

const badgeClass = computed(() => (props.track.kind === "audio" ? "bg-audio-bg text-audio" : "bg-video-bg text-video"));
const hidden = computed(() => !props.track.visible);
const eyeTitle = computed(() => (locked.value ? reason.value : hidden.value ? "Show video track" : "Hide video track"));
const muteTitle = computed(() => (locked.value ? reason.value : `Mute ${props.track.name}`));
const soloTitle = computed(() => (locked.value ? reason.value : `Solo audio on ${props.track.name}`));
const lockTitle = computed(() => `${locked.value ? "Unlock" : "Lock"} ${props.track.name}`);

// ---- inline rename (opened by the track menu's "Rename track…") -----------

const editing = ref(false);
const draft = ref(props.track.name);
const nameInput = ref<HTMLInputElement | null>(null);

/** The draft follows the committed name whenever it changes from OUTSIDE
 * (an undo, a rename landing from elsewhere) — unless the user is mid-edit
 * (the `useInspectorDraft.ts` precedent). */
watch(
  () => props.track.name,
  (name) => {
    if (!editing.value) draft.value = name;
  },
);

function beginRename() {
  if (locked.value) return;
  editing.value = true;
  draft.value = props.track.name;
  void nextTick(() => nameInput.value?.select());
}
watch(
  trackRenameRequest,
  (id) => {
    if (id !== props.track.id) return;
    trackRenameRequest.value = null;
    beginRename();
  },
  { immediate: true },
);
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
</script>

<template>
  <div
    :data-testid="`track-header-${track.id}`"
    class="flex min-w-0 flex-1 items-center gap-2"
  >
    <button
      type="button"
      :data-testid="`track-header-${track.id}-badge`"
      :aria-label="`${track.name} track properties`"
      :title="`${track.name} · track properties`"
      class="grid h-7 min-h-7 w-7 shrink-0 cursor-pointer place-items-center rounded-[6px] p-0 vb-mono text-[10px] focus:outline-none focus-visible:ring-2 focus-visible:ring-focus"
      :class="badgeClass"
      @click="showProperties"
    >
      {{ badge }}
    </button>
    <div class="min-w-0 flex-1">
      <div class="truncate text-[11px] font-[550] text-fg">
        <input
          v-if="editing"
          ref="nameInput"
          :data-testid="`track-header-${track.id}-name-input`"
          :aria-label="`Rename ${track.name}`"
          class="h-6 w-full min-w-0 rounded border border-line bg-stage px-1 text-[11px] text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          :value="draft"
          @input="draft = ($event.target as HTMLInputElement).value"
          @keydown.enter="commitRename"
          @keydown.escape.stop="cancelRename"
          @blur="commitRename"
        >
        <button
          v-else
          type="button"
          :data-testid="`track-header-${track.id}-name`"
          :title="track.name"
          class="block min-h-6 max-w-full cursor-pointer truncate rounded p-0 text-left text-[11px] font-[550] text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
          @click="showProperties"
        >
          {{ track.name }}
        </button>
      </div>
      <div
        :data-testid="`track-header-${track.id}-controls`"
        class="mt-px flex items-center gap-1"
      >
        <button
          v-if="track.kind === 'video'"
          type="button"
          :data-testid="`track-header-${track.id}-visible`"
          :aria-pressed="hidden"
          :aria-disabled="locked"
          :aria-label="`Hide ${track.name}`"
          :title="eyeTitle"
          :class="controlClass(hidden, true)"
          @click="toggle('visible')"
        >
          <EditorIcon
            :name="hidden ? 'eyeOff' : 'eye'"
            :size="12"
          />
        </button>
        <button
          type="button"
          :data-testid="`track-header-${track.id}-mute`"
          :aria-pressed="track.muted"
          :aria-disabled="locked"
          :aria-label="`Mute ${track.name}`"
          :title="muteTitle"
          :class="controlClass(track.muted, true)"
          @click="toggle('muted')"
        >
          M
        </button>
        <button
          type="button"
          :data-testid="`track-header-${track.id}-solo`"
          :aria-pressed="track.solo"
          :aria-disabled="locked"
          :aria-label="`Solo ${track.name}`"
          :title="soloTitle"
          :class="controlClass(track.solo, true)"
          @click="toggle('solo')"
        >
          S
        </button>
        <button
          type="button"
          :data-testid="`track-header-${track.id}-lock`"
          :aria-pressed="locked"
          :aria-label="`Lock ${track.name}`"
          :title="lockTitle"
          :class="controlClass(locked, false)"
          @click="toggle('locked')"
        >
          <EditorIcon
            :name="locked ? 'lock' : 'unlock'"
            :size="12"
          />
        </button>
      </div>
    </div>
  </div>
</template>
