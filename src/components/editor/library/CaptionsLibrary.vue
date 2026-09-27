<script setup lang="ts">
/**
 * The caption workspace (Task 36; F-34, F-35; SCREENS-AND-INTERACTIONS.md §
 * 06; visual-parity Task 10, concept spec §3.4): "EVERY WORD, ACCESSIBLE"
 * with the caption count; the attached-source box naming the clip Add and
 * Import use; Add caption and Import SRT / VTT (Rust's own native dialog --
 * SRT, WebVTT or `.txt`); the collapsed "Caption appearance" settings; the
 * reading-density and overlap notices, each with a "Select cue" that
 * reveals the caption it names; every caption as a card in OUTPUT order
 * (`CaptionCueRow`: its time, edit, Split cue, delete); and the native
 * "Export timeline SRT / VTT".
 *
 * **Output time on screen, source time on the wire.** The list, the time
 * fields and the notices are all output time (`captionRules.captionRows`);
 * a typed time goes back to the clip's source time through
 * `timeMap.sourceAtClamped` before it is sent, the teaching cues' own
 * inspector precedent (Task 35). Rust is the authority for every edit.
 *
 * **Nothing is faked (R20).** There is no speech recognition anywhere in
 * this app, and this panel says so where a person would look for it, in
 * plain words, rather than leaving an empty list to imply one is coming.
 *
 * The list is windowed (`useVirtualRows`): a project may hold 2000
 * captions, each a card that can turn into fields. The tab is ONE scroller
 * (fix round 1): the root scrolls, and the window is measured from where
 * the list starts inside it (`listOffset`), so the list never scrolls on
 * its own inside a scrolling tab.
 */
import { computed, onMounted, ref, watch } from "vue";

import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useVirtualRows } from "../../../composables/useVirtualRows";
import { clipSpanOf, lockedTrackName } from "../../../editor/actionTargets";
import type { CaptionRow, Draft, SplitDraft } from "../../../editor/captionRules";
import {
  addCaptionAt,
  captionNotices,
  captionRows,
  captionTargetClip,
  DENSITY_LIMIT_CPS,
  splitCaptionCue,
} from "../../../editor/captionRules";
import { sourceAtClamped } from "../../../editor/timeMap";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import EditorIcon from "../icons/EditorIcon.vue";
import type { Revert } from "./CaptionCueRow.vue";
import CaptionCueRow from "./CaptionCueRow.vue";
import CaptionNotices from "./CaptionNotices.vue";
import type { CaptionSettingsPatch } from "./CaptionSettingsPanel.vue";
import CaptionSettingsPanel from "./CaptionSettingsPanel.vue";
import CaptionsExport from "./CaptionsExport.vue";
import CaptionsToolbar from "./CaptionsToolbar.vue";
import LibraryHeading from "./LibraryHeading.vue";

/** One card (§3.4 `.caption-card`) plus the 8px gap under it — fixed, so
 * the windowed list knows every row's place. */
const ROW_HEIGHT = 124;

type UpdateCaption = { captionId: string; startMs?: number; endMs?: number; text?: string };

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
/** The guide's `library.captions` (Task 55). */
const guideTarget = useGuideTarget("library.captions");

const rows = computed(() => captionRows(project.project));
const notices = computed(() => captionNotices(rows.value));
const selectedId = computed(() => (workspace.selected?.type === "caption" ? workspace.selected.id : null));

const addDraft = computed(() => addCaptionAt(project.project, workspace.selectionClipIds, workspace.playheadMs));
const reasonOf = (d: Draft): string | null => ("reason" in d ? d.reason : null);
/** The card whose time and text are fields right now. */
const editingId = ref<string | null>(null);
watch(rows, (list) => {
  if (editingId.value && !list.some((row) => row.cue.id === editingId.value)) editingId.value = null;
});

const importTarget = computed(() =>
  captionTargetClip(project.project, workspace.selectionClipIds, workspace.playheadMs),
);
const densityHelp = `Imported timing begins at 00:00 of the visible clip. Reading-speed notices use a ${DENSITY_LIMIT_CPS} characters/second editorial heuristic, not an accessibility certification.`;
const importReason = computed(() => {
  if (!project.project) return "No project is open.";
  const clip = importTarget.value;
  if (!clip) return "Select a clip, or move the playhead onto one, to import captions for it";
  const locked = lockedTrackName(project.project, [clip.id]);
  return locked ? `Track ${locked} is locked` : null;
});
const settings = computed(() => project.project?.captions ?? null);
const settingsReason = computed(() => (project.project ? null : "No project is open."));
/** Task 48: why there is nothing to export as SRT/WebVTT, or `null`. */
const exportReason = computed(() => (rows.value.length === 0 ? "There are no captions to export yet." : null));
const replace = ref(false);
const importing = ref(false);
const importStatus = ref<string | null>(null);

/** The list sits inside the tab's one scroller (the root); its top within
 * that scroller is where the window is measured from. */
const listEl = ref<HTMLElement | null>(null);
const { viewport, range, onScroll, scrollToIndex } = useVirtualRows(
  () => rows.value.length,
  ROW_HEIGHT,
  undefined,
  undefined,
  () => listEl.value?.offsetTop ?? 0,
);
const visible = computed(() => rows.value.slice(range.value.first, range.value.last));

function bindRoot(el: unknown): void {
  guideTarget(el as Element | null);
  viewport.value = (el as HTMLElement | null) ?? null;
}

function splitFor(row: CaptionRow): SplitDraft {
  return splitCaptionCue(project.project, row, workspace.playheadMs);
}

function send(draft: Draft): void {
  if ("command" in draft) void project.execute(draft.command);
}

function toggleEdit(row: CaptionRow): void {
  editingId.value = editingId.value === row.cue.id ? null : row.cue.id;
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

async function importFile(): Promise<void> {
  const clip = importTarget.value;
  if (!clip || importReason.value !== null || importing.value) return;
  importing.value = true;
  importStatus.value = null;
  try {
    const result = await project.importCaptions(clip.id, replace.value);
    if (!result) return;
    const skipped = result.skipped
      ? ` ${plural(result.skipped, "cue", "cues")} fell outside the clip and ${result.skipped === 1 ? "was" : "were"} skipped.`
      : "";
    importStatus.value = `Imported ${plural(result.imported, "caption", "captions")} onto ${clip.name}.${skipped}`;
  } finally {
    importing.value = false;
  }
}

function toSource(row: CaptionRow, outputMs: number): number {
  return sourceAtClamped(clipSpanOf(row.clip), outputMs);
}

/** One `updateCaption`; a refusal (`execute` resolves `false`) puts the
 * field back to the stored value -- the `useInspectorDraft` rule. */
async function update(patch: UpdateCaption, revert: Revert): Promise<void> {
  if (!(await project.execute({ kind: "updateCaption", ...patch }))) revert();
}

function updateText(row: CaptionRow, text: string, revert: Revert): void {
  void update({ captionId: row.cue.id, text }, revert);
}

function updateStart(row: CaptionRow, outputMs: number, revert: Revert): void {
  void update({ captionId: row.cue.id, startMs: toSource(row, outputMs) }, revert);
}

function updateEnd(row: CaptionRow, outputMs: number, revert: Revert): void {
  void update({ captionId: row.cue.id, endMs: toSource(row, outputMs) }, revert);
}

function remove(row: CaptionRow): void {
  void project.execute({ kind: "removeCaptions", captionIds: [row.cue.id] });
}

function changeSettings(patch: CaptionSettingsPatch): void {
  void project.execute({ kind: "setCaptionSettings", ...patch });
}

/** Task 54: a caption finding opens this tab with its cue already
 * selected (`checkReveal.ts`) — scroll the cue into the list. */
onMounted(() => {
  const i = rows.value.findIndex((row) => row.cue.id === selectedId.value);
  if (i !== -1) scrollToIndex(i);
});

/** "Select cue": select it, move the playhead onto it, scroll it in. */
function selectCue(row: CaptionRow): void {
  workspace.setSelected({ type: "caption", id: row.cue.id });
  workspace.setPlayhead(row.startMs);
  scrollToIndex(row.index - 1);
}
</script>

<template>
  <div
    :ref="bindRoot"
    data-testid="captions-library"
    class="relative flex h-full flex-col overflow-y-auto text-fg"
    @scroll="onScroll"
  >
    <LibraryHeading
      label="EVERY WORD, ACCESSIBLE"
      :pill="String(rows.length)"
      testid="captions"
    />
    <p
      data-testid="caption-transcription-note"
      class="text-[11px] text-fg-muted"
    >
      Write or import captions. No speech service is connected.
    </p>
    <div
      data-testid="caption-source"
      class="my-[13px] flex gap-2 rounded-[7px] border border-line bg-app p-2.5 text-[10px] leading-relaxed"
    >
      <EditorIcon
        name="link"
        :size="14"
        class="mt-0.5 shrink-0 text-accent"
      />
      <span
        v-if="importTarget"
        class="min-w-0 truncate"
      >Attached to <b class="font-semibold">{{ importTarget.name }}</b></span>
      <span v-else>Select footage or audio to add captions.</span>
    </div>
    <CaptionsToolbar
      v-model:replace="replace"
      :import-reason="importReason"
      :importing="importing"
      :add-reason="reasonOf(addDraft)"
      @import="importFile"
      @add="send(addDraft)"
    />
    <p
      v-if="importStatus"
      data-testid="caption-import-status"
      role="status"
      class="mt-2 text-[11px] text-fg-secondary"
    >
      {{ importStatus }}
    </p>
    <CaptionSettingsPanel
      :settings="settings"
      :disabled-reason="settingsReason"
      @change="changeSettings"
    />
    <CaptionNotices
      v-if="notices.length > 0"
      :notices="notices"
      class="mb-2"
      @select="selectCue"
    />
    <p
      v-if="rows.length === 0"
      data-testid="caption-empty"
      class="py-5 text-center text-[12px] leading-[1.7] text-fg-muted"
    >
      Make your tutorial understandable without sound. Select a clip, then add or import captions.
    </p>
    <div
      v-else
      ref="listEl"
      data-testid="caption-list"
      class="shrink-0"
    >
      <ul
        aria-label="Captions"
        :style="{ paddingTop: `${range.padTop}px`, paddingBottom: `${range.padBottom}px` }"
      >
        <CaptionCueRow
          v-for="row in visible"
          :key="row.cue.id"
          :row="row"
          :height="ROW_HEIGHT"
          :selected="row.cue.id === selectedId"
          :editing="row.cue.id === editingId"
          :split="splitFor(row)"
          @text="(value, revert) => updateText(row, value, revert)"
          @start="(ms, revert) => updateStart(row, ms, revert)"
          @end="(ms, revert) => updateEnd(row, ms, revert)"
          @remove="remove(row)"
          @edit="toggleEdit(row)"
          @select="selectCue(row)"
          @split="send(splitFor(row))"
        />
      </ul>
    </div>
    <CaptionsExport :reason="exportReason" />
    <p class="mt-2 text-[11px] leading-relaxed text-fg-muted">
      {{ densityHelp }}
    </p>
  </div>
</template>
