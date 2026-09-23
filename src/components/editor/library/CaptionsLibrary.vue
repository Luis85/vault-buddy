<script setup lang="ts">
/**
 * The caption workspace (Task 36; F-34, F-35; SCREENS-AND-INTERACTIONS.md §
 * 06): every caption in OUTPUT order, editable in place; Import (Rust's own
 * native dialog -- SRT, WebVTT or `.txt`), Add at playhead and Split at
 * playhead; the caption settings; and the reading-density and overlap
 * notices, each with a "Select cue" that reveals the caption it names.
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
 * captions, each row an editable text box and two time fields.
 */
import { computed, ref } from "vue";

import { useVirtualRows } from "../../../composables/useVirtualRows";
import { clipSpanOf, lockedTrackName } from "../../../editor/actionTargets";
import type { CaptionRow, Draft } from "../../../editor/captionRules";
import {
  addCaptionAt,
  captionNotices,
  captionRows,
  captionTargetClip,
  splitCaptionAt,
} from "../../../editor/captionRules";
import { sourceAtClamped } from "../../../editor/timeMap";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import type { Revert } from "./CaptionCueRow.vue";
import CaptionCueRow from "./CaptionCueRow.vue";
import CaptionNotices from "./CaptionNotices.vue";
import type { CaptionSettingsPatch } from "./CaptionSettingsPanel.vue";
import CaptionSettingsPanel from "./CaptionSettingsPanel.vue";
import CaptionsToolbar from "./CaptionsToolbar.vue";

const ROW_HEIGHT = 76;

type UpdateCaption = { captionId: string; startMs?: number; endMs?: number; text?: string };

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();

const rows = computed(() => captionRows(project.project));
const notices = computed(() => captionNotices(rows.value));
const selectedId = computed(() => (workspace.selected?.type === "caption" ? workspace.selected.id : null));

const addDraft = computed(() => addCaptionAt(project.project, workspace.selectionClipIds, workspace.playheadMs));
const splitDraft = computed(() => splitCaptionAt(rows.value, selectedId.value, workspace.playheadMs));
const reasonOf = (d: Draft): string | null => ("reason" in d ? d.reason : null);

const importTarget = computed(() =>
  captionTargetClip(project.project, workspace.selectionClipIds, workspace.playheadMs),
);
const importReason = computed(() => {
  if (!project.project) return "No project is open.";
  const clip = importTarget.value;
  if (!clip) return "Select a clip, or move the playhead onto one, to import captions for it";
  const locked = lockedTrackName(project.project, [clip.id]);
  return locked ? `Track ${locked} is locked` : null;
});
const settings = computed(() => project.project?.captions ?? null);
const settingsReason = computed(() => (project.project ? null : "No project is open."));
const replace = ref(false);
const importing = ref(false);
const importStatus = ref<string | null>(null);

const { viewport, range, onScroll, scrollToIndex } = useVirtualRows(() => rows.value.length, ROW_HEIGHT);
const visible = computed(() => rows.value.slice(range.value.first, range.value.last));

function setViewport(el: unknown): void {
  viewport.value = (el as HTMLElement | null) ?? null;
}

function send(draft: Draft): void {
  if ("command" in draft) void project.execute(draft.command);
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

/** "Select cue": select it, move the playhead onto it, scroll it in. */
function selectCue(row: CaptionRow): void {
  workspace.setSelected({ type: "caption", id: row.cue.id });
  workspace.setPlayhead(row.startMs);
  scrollToIndex(row.index - 1);
}
</script>

<template>
  <div
    data-testid="captions-library"
    class="flex h-full flex-col gap-2 text-micro text-fg-secondary"
  >
    <p
      data-testid="caption-transcription-note"
      class="text-fg-subtle"
    >
      Automatic transcription is not available. Write captions here, or
      import an SRT or WebVTT file whose times start at 00:00 of the clip.
    </p>
    <CaptionsToolbar
      v-model:replace="replace"
      :import-reason="importReason"
      :importing="importing"
      :add-reason="reasonOf(addDraft)"
      :split-reason="reasonOf(splitDraft)"
      @import="importFile"
      @add="send(addDraft)"
      @split="send(splitDraft)"
    />
    <p
      v-if="importStatus"
      data-testid="caption-import-status"
      role="status"
    >
      {{ importStatus }}
    </p>
    <CaptionNotices
      v-if="notices.length > 0"
      :notices="notices"
      @select="selectCue"
    />
    <CaptionSettingsPanel
      :settings="settings"
      :disabled-reason="settingsReason"
      @change="changeSettings"
    />
    <p
      v-if="rows.length === 0"
      class="text-fg-subtle"
    >
      No captions yet. Select a clip, then add or import captions.
    </p>
    <div
      v-else
      :ref="setViewport"
      data-testid="caption-list"
      class="min-h-0 flex-1 overflow-y-auto"
      @scroll="onScroll"
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
          @text="(value, revert) => updateText(row, value, revert)"
          @start="(ms, revert) => updateStart(row, ms, revert)"
          @end="(ms, revert) => updateEnd(row, ms, revert)"
          @remove="remove(row)"
        />
      </ul>
    </div>
  </div>
</template>
