<script setup lang="ts">
/**
 * The media library (Task 25; F-02; SCREENS-AND-INTERACTIONS.md's library
 * column): the project's assets as searchable cards (kind, duration,
 * availability — `LibraryAssetCard`), the Import button, and the import's
 * progress/Cancel and per-file results (`ImportStatus`).
 *
 * **Import never sends a path.** The button calls `editorJobs.importMedia`,
 * and Rust opens its OWN native multi-file dialog on the `editor-import`
 * thread — the dialog, not a string from this webview, is what grants
 * access to a file (ADR §3.3). Progress arrives on the job's Channel;
 * nothing here polls.
 *
 * **"+" inserts at the playhead onto the FIRST compatible, unlocked track**
 * (`trackCompat.firstAcceptingTrack` — the one copy of the rule the
 * timeline drag applies, so "+" and a drop can never disagree about where a
 * clip may go), as one `insertClip` through `editorProject.execute`; Rust
 * stays the authority (an overlap there is refused and surfaces as the
 * store's error). A control that cannot act says why in its `title`
 * (`aria-disabled`, never the native attribute, so the reason stays
 * reachable — the `TrackHeader.vue` precedent).
 *
 * **Dragging a card onto a lane (Task 26)** starts here — `LibraryAssetCard`
 * itself owns the native `dragstart` (the DOM node it renders), encoding
 * the asset's kind into the drag payload via `trackCompat.setAssetDragData`
 * — but the DROP is `TimelineView.vue`/`TrackLane.vue`'s: this component
 * originates the drag and never sees where it lands.
 *
 * `reconcile()` runs on mount: this webview mounts once per process, but a
 * reload mid-import would otherwise show nothing for a job still running in
 * Rust.
 *
 * **Reconnect… (Task 40)** appears while any original is missing and opens
 * `ReconnectDialog`; like Import, it never sends a path.
 *
 * **Webcam… (Task 50)** opens `WebcamDialog` — opening it touches no
 * device; the dialog's own *Enable camera* is the first thing that does.
 *
 * **Both open on a before-you-share finding too (Task 54):** a missing
 * original's "Reconnect media" and an open take's "Open Webcam"
 * (`checkReveal.onReveal`), after the same reveal switched the library to
 * this tab.
 */
import { computed, onMounted, ref } from "vue";

import { onReveal } from "../../../editor/revealBus";
import { firstAcceptingTrack } from "../../../editor/trackCompat";
import type { Asset, Track } from "../../../editorTypes";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";
import ReconnectDialog from "../dialogs/ReconnectDialog.vue";
import WebcamDialog from "../dialogs/WebcamDialog.vue";
import ImportStatus from "./ImportStatus.vue";
import LibraryAssetCard from "./LibraryAssetCard.vue";

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const jobs = useEditorJobsStore();

const query = ref("");
const reconnectOpen = ref(false);
const webcamOpen = ref(false);

interface AssetRow {
  asset: Asset;
  kindLabel: string;
  duration: string;
  missing: boolean;
  target: Track | undefined;
  refusal: string | null;
}

function kindLabel(asset: Asset): string {
  if (asset.media_type === "image") return "Image";
  return asset.kind === "audio" ? "Audio" : "Video";
}

function refusalFor(missing: boolean, target: Track | undefined, asset: Asset): string | null {
  if (missing) return "This media's file is missing. Reconnect it before inserting it.";
  if (!target) return `Add an unlocked ${asset.kind} track first.`;
  return null;
}

const missingIds = computed(() => new Set(project.missing.map((m) => m.assetId)));

function toRow(asset: Asset): AssetRow {
  const missing = missingIds.value.has(asset.id);
  const target = firstAcceptingTrack(project.project, asset.kind);
  return {
    asset,
    kindLabel: kindLabel(asset),
    duration: formatDuration(asset.duration_ms),
    missing,
    target,
    refusal: refusalFor(missing, target, asset),
  };
}

const rows = computed<AssetRow[]>(() => {
  const needle = query.value.trim().toLowerCase();
  return (project.project?.assets ?? [])
    .filter((a) => a.name.toLowerCase().includes(needle))
    .map(toRow);
});

const emptyText = computed(() =>
  query.value ? "No media matches the search." : "No media yet. Import video, audio or images.",
);

function insert(row: AssetRow): void {
  if (row.refusal !== null || !row.target) return;
  void project.execute({
    kind: "insertClip",
    assetId: row.asset.id,
    trackId: row.target.id,
    startMs: workspace.playheadMs,
    inMs: 0,
    outMs: row.asset.duration_ms,
  });
}

const importRefusal = computed<string | null>(() => {
  if (!project.sessionId) return "Open a project first.";
  if (jobs.activeImport) return "An import is already running.";
  return null;
});
const importTitle = computed(() => importRefusal.value ?? "Import video, audio or images");

const webcamRefusal = computed<string | null>(() => (project.sessionId ? null : "Open a project first."));
const webcamTitle = computed(() => webcamRefusal.value ?? "Record a webcam take");

function openWebcam(): void {
  if (webcamRefusal.value === null) webcamOpen.value = true;
}

function startImport(): void {
  if (importRefusal.value === null) void jobs.importMedia();
}

onMounted(() => {
  void jobs.reconcile();
});
onReveal("reconnect", () => {
  reconnectOpen.value = true;
});
onReveal("webcam", openWebcam);
</script>

<template>
  <div
    data-testid="media-library"
    class="flex h-full flex-col gap-2 text-micro text-fg-secondary"
  >
    <div class="flex items-center gap-1">
      <input
        v-model="query"
        data-testid="library-search"
        type="search"
        aria-label="Search media"
        placeholder="Search media"
        class="w-0 min-w-0 flex-1 rounded border border-line bg-stage px-1 py-0.5 text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      >
      <button
        type="button"
        data-testid="library-import"
        :aria-disabled="importRefusal !== null"
        :title="importTitle"
        class="shrink-0 rounded border border-line px-2 py-0.5 text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
        @click="startImport"
      >
        Import…
      </button>
      <button
        type="button"
        data-testid="library-webcam"
        :aria-disabled="webcamRefusal !== null"
        :title="webcamTitle"
        class="shrink-0 rounded border border-line px-2 py-0.5 text-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
        @click="openWebcam"
      >
        Webcam…
      </button>
    </div>
    <WebcamDialog
      :open="webcamOpen"
      @close="webcamOpen = false"
    />

    <button
      v-if="project.missing.length > 0"
      type="button"
      data-testid="library-reconnect"
      class="rounded border border-danger/40 px-2 py-0.5 text-left text-danger-fg hover:bg-white/10 focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      @click="reconnectOpen = true"
    >
      {{ project.missing.length === 1 ? "1 original is missing" : `${project.missing.length} originals are missing` }} — Reconnect…
    </button>
    <ReconnectDialog
      :open="reconnectOpen"
      @close="reconnectOpen = false"
    />

    <p
      v-if="jobs.lastError"
      data-testid="library-error"
      role="alert"
      class="rounded border border-danger/40 px-1 py-0.5 text-danger-fg"
    >
      {{ jobs.lastError.message }}
    </p>

    <ImportStatus />

    <ul
      class="flex min-h-0 flex-1 flex-col gap-1 overflow-y-auto"
      aria-label="Project media"
    >
      <LibraryAssetCard
        v-for="row in rows"
        :id="row.asset.id"
        :key="row.asset.id"
        :name="row.asset.name"
        :kind="row.asset.kind"
        :kind-label="row.kindLabel"
        :duration="row.duration"
        :missing="row.missing"
        :refusal="row.refusal"
        :target-name="row.target?.name ?? ''"
        @insert="insert(row)"
      />
      <li
        v-if="rows.length === 0"
        class="text-fg-subtle"
      >
        {{ emptyText }}
      </li>
    </ul>
  </div>
</template>
