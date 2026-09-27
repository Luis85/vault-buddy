<script setup lang="ts">
/**
 * The media library (Task 25; F-02; visual-parity Task 9, concept spec
 * §3.2): a two-button actions row (Import media / Webcam), a search box,
 * the "SOURCE MEDIA" heading with an asset-count pill, and the project's
 * assets as searchable rows (`LibraryAssetCard`) -- plus the import's
 * progress/Cancel and per-file results (`ImportStatus`) and the missing-
 * originals banner (Task 40), both this app's own additions with no
 * concept counterpart.
 *
 * **Import never sends a path.** The button calls `editorJobs.importMedia`,
 * and Rust opens its OWN native multi-file dialog on the `editor-import`
 * thread — the dialog, not a string from this webview, is what grants
 * access to a file (ADR §3.3). Progress arrives on the job's Channel;
 * nothing here polls.
 *
 * **"+"/Enter inserts at the playhead onto a FREE track** (`placeOnFreeTrack`,
 * visual-parity Task 7, audit finding 1a — the one placement rule the
 * Titles cards and the asset menu share): the first unlocked track of the
 * asset's kind, top-down, with nothing in the clip's span, else a new
 * track (above the topmost video track, below the last audio track) and
 * the clip on it. Rust stays the authority; a refusal surfaces through the
 * editor's toast.
 *
 * **Dragging a card onto a lane (Task 26)** starts here — `LibraryAssetCard`
 * itself owns the native `dragstart` (the DOM node it renders), encoding
 * the asset's kind into the drag payload via `trackCompat.setAssetDragData`
 * — but the DROP is `TimelineView.vue`/`TrackLane.vue`'s: this component
 * originates the drag and never sees where it lands.
 *
 * **Right-click / Shift+F10 on a row (visual-parity Task 9, concept spec
 * §8)** opens the asset's context menu (`assetMenu`, Task 5) through the
 * SAME `ContextMenu`/`MenuPanel` the timeline uses — a `NOOP_VIEW` stands in
 * for the timeline's own `fitRange`/`fitTimeline`, which no asset-menu item
 * calls (`useEditorMenuContext`'s own doc: "any surface that mounts a menu
 * passes its own").
 *
 * `reconcile()` runs on mount: this webview mounts once per process, but a
 * reload mid-import would otherwise show nothing for a job still running in
 * Rust.
 *
 * **Reconnect… (Task 40)** appears while any original is missing (the
 * banner) and opens `ReconnectDialog`; a row's own link button (§3.2) opens
 * the same dialog. Like Import, it never sends a path.
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

import type { TimelineViewOps } from "../../../composables/useEditorMenuContext";
import { useGuideTarget } from "../../../composables/useGuideTarget";
import { useMediaImport } from "../../../composables/useMediaImport";
import { baseActionContext } from "../../../editor/actionContext";
import type { PointerTarget } from "../../../editor/actions";
import { announceDisabled } from "../../../editor/disabledAnnouncer";
import { insertAssetOnFreeTrack, placementLabel, placeOnFreeTrack } from "../../../editor/placeOnFreeTrack";
import { onReveal } from "../../../editor/revealBus";
import type { Asset } from "../../../editorTypes";
import { useEditorJobsStore } from "../../../stores/editorJobs";
import { useEditorProjectStore } from "../../../stores/editorProject";
import { useEditorWorkspaceStore } from "../../../stores/editorWorkspace";
import { formatDuration } from "../../../utils/formatDuration";
import ReconnectDialog from "../dialogs/ReconnectDialog.vue";
import WebcamDialog from "../dialogs/WebcamDialog.vue";
import EditorIcon from "../icons/EditorIcon.vue";
import ContextMenu from "../menus/ContextMenu.vue";
import ImportStatus from "./ImportStatus.vue";
import LibraryAssetCard from "./LibraryAssetCard.vue";
import LibraryHeading from "./LibraryHeading.vue";

const project = useEditorProjectStore();
const workspace = useEditorWorkspaceStore();
const jobs = useEditorJobsStore();
/** The guide's `library.import`/`library.webcam` (Task 55). */
const importTarget = useGuideTarget("library.import");
const webcamTarget = useGuideTarget("library.webcam");

const query = ref("");
const reconnectOpen = ref(false);
const webcamOpen = ref(false);

interface AssetRow {
  asset: Asset;
  /** "MM:SS · W × H" / "MM:SS · Local audio" / "Missing source" (§3.2). */
  meta: string;
  missing: boolean;
  /** The instant to thumbnail at, or `null` for audio/missing (§3.2). */
  thumbAtMs: number | null;
  /** The track "+"/Enter lands on now, or "a new … track". */
  targetName: string;
}

/** The video/image thumbnail instant: 1 s in, or the asset's own midpoint
 * when it is shorter than 2 s (visual-parity Task 9 brief). */
function thumbnailAtMs(durationMs: number): number {
  return Math.min(1_000, Math.floor(durationMs / 2));
}

function metaFor(asset: Asset, missing: boolean): string {
  if (missing) return "Missing source";
  const duration = formatDuration(asset.duration_ms);
  if (asset.kind === "audio") return `${duration} · Local audio`;
  if (asset.width && asset.height) return `${duration} · ${asset.width} × ${asset.height}`;
  return `${duration} · ${asset.media_type === "image" ? "Image" : "Video"}`;
}

const missingIds = computed(() => new Set(project.missing.map((m) => m.assetId)));

/** Where "+"/Enter would put `asset` right now (it follows the playhead). */
function targetNameFor(asset: Asset): string {
  const p = project.project;
  if (!p) return "";
  return placementLabel(p, placeOnFreeTrack(p, asset.kind, workspace.playheadMs, asset.duration_ms), asset.kind);
}

function toRow(asset: Asset): AssetRow {
  const missing = missingIds.value.has(asset.id);
  return {
    asset,
    meta: metaFor(asset, missing),
    missing,
    thumbAtMs: missing || asset.kind === "audio" ? null : thumbnailAtMs(asset.duration_ms),
    targetName: targetNameFor(asset),
  };
}

/** The heading's own count is every source asset, never the filtered list
 * below it (the concept's own `project.assets.length`, unaffected by its
 * search filtering only what is SHOWN). */
const assetCount = computed(() => project.project?.assets.length ?? 0);

const rows = computed<AssetRow[]>(() => {
  const needle = query.value.trim().toLowerCase();
  return (project.project?.assets ?? []).filter((a) => a.name.toLowerCase().includes(needle)).map(toRow);
});

const emptyText = computed(() =>
  query.value ? "No media matches the search." : "No media yet. Import video, audio or images.",
);

/** A missing row's button is Reconnect (`LibraryAssetCard`), so this
 * only ever runs for an asset whose file is there. */
function insert(row: AssetRow): void {
  void insertAssetOnFreeTrack((command) => project.execute(command), () => project.project, row.asset, workspace.playheadMs);
}

const { refusal: importRefusal, start: startImport } = useMediaImport();
const importTitle = computed(() => importRefusal.value ?? "Import video, audio or images");

const webcamRefusal = computed<string | null>(() => (project.sessionId ? null : "Open a project first."));
const webcamTitle = computed(() => webcamRefusal.value ?? "Record a webcam take");

function openWebcam(): void {
  if (webcamRefusal.value === null) webcamOpen.value = true;
  else announceDisabled(webcamRefusal.value);
}
function onImport(): void {
  if (!startImport()) announceDisabled(importRefusal.value);
}

// ---- the one asset context menu (visual-parity Task 9, concept spec §8) ---

/** Neither `assetMenu` item calls a view change, so this stands in for the
 * timeline's own `fitRange`/`fitTimeline` (`useEditorMenuContext`'s doc,
 * which `ContextMenu.vue` itself calls with this `view`). */
const NOOP_VIEW: TimelineViewOps = { fitRange: () => {}, fitTimeline: () => {} };

const menuOpen = ref(false);
const menuX = ref(0);
const menuY = ref(0);
const menuTarget = ref<PointerTarget | null>(null);
const menuActionContext = computed(() =>
  baseActionContext(project.project, project.snapshot, workspace.playheadMs, workspace.selectionClipIds, menuTarget.value),
);

function openAssetMenu(assetId: string, x: number, y: number): void {
  menuTarget.value = { kind: "asset", id: assetId, timeMs: null };
  menuX.value = x;
  menuY.value = y;
  menuOpen.value = true;
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
    class="flex h-full flex-col text-micro text-fg-secondary"
  >
    <div class="mb-3 flex gap-[7px]">
      <button
        :ref="importTarget"
        type="button"
        data-testid="library-import"
        :aria-disabled="importRefusal !== null"
        :title="importTitle"
        class="flex h-10 flex-1 items-center justify-center gap-[7px] rounded-[7px] border-0 bg-primary px-2 text-[11px] font-semibold text-white hover:bg-primary-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
        @click="onImport"
      >
        <EditorIcon
          name="upload"
          :size="16"
        />
        Import media
      </button>
      <button
        :ref="webcamTarget"
        type="button"
        data-testid="library-webcam"
        :aria-disabled="webcamRefusal !== null"
        :title="webcamTitle"
        class="flex h-10 items-center justify-center gap-1.5 rounded-[7px] border border-line bg-app px-2 text-[11px] text-fg-secondary hover:bg-hover focus:outline-none focus-visible:ring-2 focus-visible:ring-focus aria-disabled:cursor-not-allowed aria-disabled:opacity-50"
        @click="openWebcam"
      >
        <EditorIcon
          name="webcam"
          :size="16"
        />
        Webcam
      </button>
    </div>
    <WebcamDialog
      :open="webcamOpen"
      @close="webcamOpen = false"
    />

    <ImportStatus />

    <p
      v-if="jobs.lastError"
      data-testid="library-error"
      role="alert"
      class="mb-2 rounded border border-danger/40 px-1 py-0.5 text-danger-fg"
    >
      {{ jobs.lastError.message }}
    </p>

    <button
      v-if="project.missing.length > 0"
      type="button"
      data-testid="library-reconnect"
      class="mb-2 rounded border border-danger/40 px-2 py-0.5 text-left text-danger-fg hover:bg-hover focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      @click="reconnectOpen = true"
    >
      {{ project.missing.length === 1 ? "1 original is missing" : `${project.missing.length} originals are missing` }} — Reconnect…
    </button>
    <ReconnectDialog
      :open="reconnectOpen"
      @close="reconnectOpen = false"
    />

    <div class="relative mb-3.5">
      <EditorIcon
        name="search"
        :size="14"
        class="pointer-events-none absolute left-2.5 top-2.5 text-fg-muted"
      />
      <input
        v-model="query"
        data-testid="library-search"
        type="search"
        aria-label="Find media"
        placeholder="Find media…"
        class="h-8 w-full rounded border border-line bg-stage pl-8 pr-2 text-[11px] text-fg focus:outline-none focus-visible:ring-1 focus-visible:ring-focus"
      >
    </div>

    <LibraryHeading
      label="SOURCE MEDIA"
      :pill="`${assetCount} assets`"
      testid="media"
    />

    <ul
      class="flex min-h-0 flex-1 flex-col overflow-y-auto"
      aria-label="Project media"
    >
      <LibraryAssetCard
        v-for="row in rows"
        :id="row.asset.id"
        :key="row.asset.id"
        :name="row.asset.name"
        :kind="row.asset.kind"
        :meta="row.meta"
        :missing="row.missing"
        :thumb-at-ms="row.thumbAtMs"
        :target-name="row.targetName"
        @insert="insert(row)"
        @reconnect="reconnectOpen = true"
        @context-menu="(p) => openAssetMenu(row.asset.id, p.clientX, p.clientY)"
      />
      <li
        v-if="rows.length === 0"
        class="py-2 text-center leading-relaxed text-fg-subtle"
      >
        {{ emptyText }}
      </li>
    </ul>

    <ContextMenu
      :open="menuOpen"
      :context="menuActionContext"
      :x="menuX"
      :y="menuY"
      :view="NOOP_VIEW"
      @close="menuOpen = false"
    />
  </div>
</template>
