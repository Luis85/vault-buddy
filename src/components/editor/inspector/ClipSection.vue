<script setup lang="ts">
/**
 * The Inspector's Clip category (Task 21; F-48; SCREENS-AND-INTERACTIONS.md
 * §04: "Inspector categories are Clip, Layout, Fades, Audio, Speed and
 * Color"). Fills `InspectorPanel.vue`'s `#clip` slot (`EditorRoot.vue` wires
 * it, the same seam `PreviewToolbar`/`InspectorPanel`/`TimelineView` were
 * dropped into in earlier tasks) with the numeric-entry ALTERNATIVE to
 * dragging (SCREENS-AND-INTERACTIONS.md §04): Name, Start, In, Out, and a
 * derived (read-only) Duration, plus Earlier/Later — the exact same
 * `reorderClip` verbs the timeline toolbar and context menu already offer,
 * read from the SAME `actions.ts` registry so all three surfaces can never
 * disagree about whether a reorder is available right now.
 *
 * `InspectorPanel` only renders this slot once there IS a selection and
 * hands it `clipIds` — but this section can only ever edit ONE clip at a
 * time (`updateClip`/`trimClip`/`reorderClip` all take a single `clipId`),
 * so a multi-selection (`clipIds.length > 1`) renders a short, honest note
 * instead of silently acting on `clipIds[0]` (R20: no faked scope) —
 * `InspectorPanel`'s own "N clips selected" banner already sits above this
 * slot, so the note here does not repeat that count.
 *
 * **Fresh across external edits WITHOUT a remount** (Task 19's carried
 * finding: "the draft does not react to the committed value changing from
 * outside (undo elsewhere) unless the composable is re-keyed per
 * selection"). `useInspectorDraft` now re-seeds a draft the user is not
 * editing whenever its field's committed value changes, and every value
 * getter below reads the LIVE `clip` computed — so an Undo, or a drag or
 * nudge on this same clip from the timeline, shows here at once, while a
 * value the user is mid-way through typing is left alone. The caller still
 * keys this component on the SELECTION (`EditorRoot.vue`), because a
 * different clip is a different set of drafts; it deliberately does NOT key
 * on the revision any more, which remounted the section — discarding the
 * user's in-progress keystrokes and focus — on every unrelated command.
 */
import { computed } from "vue";

import type { InspectorDraft } from "../../../composables/useInspectorDraft";
import { numberField, textField, useInspectorDraft } from "../../../composables/useInspectorDraft";
import { baseActionContext } from "../../../editor/actionContext";
import { commandFor, resolveActions } from "../../../editor/actions";
import { clipOutputDuration } from "../../../editor/timeMap";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";

const props = defineProps<{ clipIds: string[] }>();

const editorProject = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

/** The LIVE clip — re-read on every projection the store installs. Whether
 * the fields exist at all is decided once at setup (`single` below): the
 * selection the caller keys on cannot change under this instance. */
const clip = computed(() => (props.clipIds.length === 1 ? editorProject.clipById(props.clipIds[0]) : undefined));
const single = clip.value !== undefined;

/** Mirrors `core::editor::mod::limits::{MAX_NAME_CHARS, MAX_DURATION_MS}` —
 * read from the Rust source, `useInspectorDraft.ts`'s own rule for every
 * client-side mirror of a server bound. */
const MAX_NAME_CHARS = 300;
const MAX_DURATION_MS = 7_200_000;

/** The clip's own asset duration, bounding `in`/`out` — Task 21's carried
 * finding, fixed here (Task 26 defines the image rule this bound now
 * mirrors): an IMAGE asset may extend up to `MAX_DURATION_MS` (Rust's own
 * `insertClip`/`trimClip` exemption, `clips.rs`'s `out_ms_bound` — "images
 * have no source bound beyond that"), never its own (import-time-default)
 * `duration_ms`, which a numeric-entry Out past 5000ms would otherwise
 * refuse client-side even though Rust would accept it. Degrades to
 * `MAX_DURATION_MS` (never a crash) when the asset does not resolve at
 * all, the same defensive-read posture the rest of this codebase's
 * client-side mirrors take. */
const clipAsset = editorProject.project?.assets.find((a) => a.id === clip.value?.asset_id);
const assetDurationMs =
  clipAsset === undefined
    ? MAX_DURATION_MS
    : clipAsset.media_type === "image"
      ? MAX_DURATION_MS
      : clipAsset.duration_ms;

/** Both commits return `execute`'s outcome, so `useInspectorDraft` can drop
 * a value Rust refused instead of showing it as committed (fix round 1). */
function commitRename(name: string): Promise<boolean> {
  const c = clip.value;
  return c ? editorProject.execute({ kind: "updateClip", clipId: c.id, name }) : Promise.resolve(false);
}
/** Every numeric field sends the SAME `trimClip` shape carrying `startMs`/
 * `inMs`/`outMs`, changing only the one field the user actually edited —
 * `trimClip`'s own contract ("sets the clip's placement and source range
 * directly, never a delta") takes all three every time, so an untouched
 * field is sent back at its own current committed value. */
function commitTrim(overrides: { startMs?: number; inMs?: number; outMs?: number }): Promise<boolean> {
  const c = clip.value;
  if (!c) return Promise.resolve(false);
  return editorProject.execute({
    kind: "trimClip",
    clipId: c.id,
    startMs: overrides.startMs ?? c.start_ms,
    inMs: overrides.inMs ?? c.in_ms,
    outMs: overrides.outMs ?? c.out_ms,
  });
}

type LiveClip = NonNullable<typeof clip.value>;

/** One integer-millisecond field over the live clip. `in`/`out` are bounded
 * by the asset's own duration, `start` by the project maximum. */
function msDraft(label: string, read: (c: LiveClip) => number, max: number, commit: (v: number) => Promise<boolean>) {
  const value = () => (clip.value ? read(clip.value) : 0);
  return useInspectorDraft(
    numberField({ value, label, min: 0, max, rangeLabel: `0 and ${max} ms`, integer: true }),
    commit,
  );
}

const drafts = single
  ? {
      name: useInspectorDraft(
        textField({ value: () => clip.value?.name ?? "", label: "Name", maxLength: MAX_NAME_CHARS }),
        commitRename,
      ),
      start: msDraft("Start", (c) => c.start_ms, MAX_DURATION_MS, (v) => commitTrim({ startMs: v })),
      in: msDraft("In", (c) => c.in_ms, assetDurationMs, (v) => commitTrim({ inMs: v })),
      out: msDraft("Out", (c) => c.out_ms, assetDurationMs, (v) => commitTrim({ outMs: v })),
    }
  : null;

const durationMs = computed(() => {
  const c = clip.value;
  return c ? clipOutputDuration(c.in_ms, c.out_ms, c.speed ?? 1) : 0;
});

/**
 * A `v-for`-driven ONE template block for all four fields, rather than four
 * near-identical inline blocks — the fallow quality ratchet's own template-
 * complexity threshold is why (four repeated blocks pushed this file's
 * `<template>` over it). `InspectorDraft` is not itself generic (`draft`/
 * `error` are always plain `Ref<string>`/`Ref<string|null>` regardless of
 * the field's underlying value type — `useInspectorDraft.ts`'s own
 * interface), so the four drafts genuinely share one shape here; iterating
 * over them writes `field.draft.draft.value` on a LOCAL array, never a
 * `defineProps` value, so `vue/no-mutating-props` has nothing to flag. */
interface FieldRow {
  key: string;
  label: string;
  testid: string;
  draft: InspectorDraft;
}
const fieldRows: FieldRow[] = drafts
  ? [
      { key: "name", label: "Name", testid: "clip-section-name", draft: drafts.name },
      { key: "start", label: "Start (ms)", testid: "clip-section-start", draft: drafts.start },
      { key: "in", label: "In (ms)", testid: "clip-section-in", draft: drafts.in },
      { key: "out", label: "Out (ms)", testid: "clip-section-out", draft: drafts.out },
    ]
  : [];
function onFieldInput(event: Event, field: FieldRow) {
  field.draft.draft.value = (event.target as HTMLInputElement).value;
}

// ---- Earlier / Later --------------------------------------------------
// The SAME `actions.ts` registry the timeline toolbar and context menu
// read (`baseActionContext`, the `PreviewToolbar`/`TimelineToolbar`
// precedent) — this section's `clipIds` prop already equals
// `workspace.selectionClipIds` whenever it renders (`InspectorPanel`'s
// own template), so `primaryTargetClip`'s "falls back to a single
// selected clip" rule resolves correctly with no `pointerTarget` needed.
const reorderContext = computed(() =>
  baseActionContext(editorProject.project, editorProject.snapshot, workspace.playheadMs, workspace.selectionClipIds),
);
const reorderResolved = computed(() => resolveActions(reorderContext.value));
function onReorder(direction: "earlier" | "later") {
  if (!reorderResolved.value[direction].enabled) return;
  const command = commandFor(direction, reorderContext.value);
  if (command) void editorProject.execute(command);
}
const REORDER_DIRECTIONS = ["earlier", "later"] as const;
function reorderTitle(direction: "earlier" | "later"): string {
  return reorderResolved.value[direction].reason ?? reorderResolved.value[direction].label;
}
function reorderClass(direction: "earlier" | "later"): string {
  return reorderResolved.value[direction].enabled ? "text-fg-secondary" : "cursor-default opacity-50";
}
</script>

<template>
  <div
    v-if="drafts && clip"
    data-testid="clip-section"
    class="flex flex-col gap-2"
  >
    <label
      v-for="field in fieldRows"
      :key="field.key"
      class="flex flex-col gap-0.5"
    >
      <span class="text-fg-subtle">{{ field.label }}</span>
      <input
        :data-testid="field.testid"
        type="text"
        class="rounded border border-line bg-stage px-1 py-0.5 text-fg"
        :value="field.draft.draft.value"
        @input="onFieldInput($event, field)"
        @keydown.enter="field.draft.submit()"
        @keydown.escape="field.draft.revert()"
        @blur="field.draft.submit()"
      >
      <span
        v-if="field.draft.error.value"
        :data-testid="`${field.testid}-error`"
        class="text-danger"
      >{{ field.draft.error.value }}</span>
    </label>

    <p
      data-testid="clip-section-duration"
      class="text-fg-subtle"
    >
      Duration: {{ formatDuration(durationMs) }}
    </p>

    <div class="flex gap-1">
      <button
        v-for="direction in REORDER_DIRECTIONS"
        :key="direction"
        type="button"
        :data-testid="`clip-section-${direction}`"
        :aria-disabled="!reorderResolved[direction].enabled"
        :title="reorderTitle(direction)"
        class="cursor-pointer rounded px-1.5 py-0.5 transition-colors hover:bg-white/10"
        :class="reorderClass(direction)"
        @click="onReorder(direction)"
      >
        {{ reorderResolved[direction].label }}
      </button>
    </div>
  </div>
  <p
    v-else
    data-testid="clip-section-multi"
    class="text-fg-subtle"
  >
    Select a single clip to edit its name, start, in and out.
  </p>
</template>
