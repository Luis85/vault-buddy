/**
 * The tutorial editor's workspace view-preference store (Task 18; P03,
 * F-48/F-25/F-14; ARCHITECTURE-AND-STACK.md's Pinia state boundaries:
 * `editorProject` = Rust's committed truth, `editorWorkspace` = local view
 * preference — selection, playhead, zoom, panel layout, theme — R14's
 * split). It mirrors the 18 R16 fields `core::editor::workspace::Workspace`
 * sanitizes, plus `theme` (F16), persists them through
 * `editor_save_workspace` on a 750 ms debounce, and hydrates them back from
 * `editor_get_workspace` on session open.
 *
 * **A SETUP store** (Pinia's function-based `defineStore`) — the one
 * exception to this repo's options-store convention (every other store
 * under `src/stores/`). The one thing this store needs that an options
 * store cannot cleanly express is a `watch` tied to the store's OWN
 * lifetime: pruning `selectionClipIds` whenever `editorProject` installs a
 * new projection (this task's own named test — "pruned on every projection
 * install, watch on `editorProject.snapshot.revision`, not on the whole
 * graph"). A setup store is Pinia's documented way to hold that watcher
 * without a plugin registered in `main.ts`, which is out of this task's
 * file list.
 *
 * Every mutator here is presentation state ONLY: none of them call
 * `editorProject.execute` — `toggleMonitorMute`'s own named test pins this
 * explicitly for the one field a component author might mistake for a real
 * mixer control (muting the PREVIEW is not an edit to the project's master
 * gain or a clip's mix) — and `persist()` never touches the project graph,
 * the session's revision, or its undo/redo history, the same posture
 * `core::editor::workspace::sanitize` holds on the Rust side.
 *
 * Most of the actual logic below is plain, module-level functions taking
 * this store's `WorkspaceFields` bundle explicitly, rather than closures
 * inline inside the store's own setup function — `max-lines-per-function`
 * is real for a setup store's arrow just like any other function, and
 * pulling the field-by-field snapshot/apply/prune/persist/hydrate work out
 * keeps that arrow to orchestration (create the fields, wire the watch,
 * build the mutators, return them) rather than 250 lines of one function.
 */
import { defineStore } from "pinia";
import type { Ref } from "vue";
import { markRaw, ref, shallowRef, watch } from "vue";

import { createPanelControls, TIMELINE_DEFAULT_HEIGHT } from "../editor/panelLayout";
import type { EditorPort } from "../editor/port";
import { createTauriEditorPort } from "../editor/port";
import { fitRangeZoom, msToX, trackLabelWidthAt } from "../editor/timelineLayout";
import type { DeleteMode, Selected, Theme, Workspace } from "../editorTypes";
import { logWarning } from "../logging";
import { toEditorError, useEditorProjectStore } from "./editorProject";
import { createDisclosures } from "./workspaceDisclosures";
import { createSelection, pruneSelectedTrack } from "./workspaceSelection";

const PERSIST_DEBOUNCE_MS = 750;

// The same clamp ranges `core::editor::workspace::sanitize` enforces on the
// way back in — clamping here too means a value never visibly snaps to a
// different number only once the round trip through Rust completes. The
// timeline height's own, tighter range follows the window (visual-parity
// Task 4, `panelLayout.ts`), always inside Rust's 160–900.
export const ZOOM_RANGE = [0.1, 20.0] as const;
const PLAYBACK_RATE_RANGE = [0.25, 2.0] as const;

function clamp(n: number, [lo, hi]: readonly [number, number]): number {
  return Math.min(hi, Math.max(lo, n));
}

/** The 18 R16 fields plus `theme` (F16), each its own ref — a plain object
 * bundle so the module-level functions below can take "this store's
 * fields" as one parameter instead of eighteen. */
interface WorkspaceFields {
  selectionClipIds: Ref<string[]>;
  selected: Ref<Selected | null>;
  playheadMs: Ref<number>;
  libraryTab: Ref<string | null>;
  propertyTab: Ref<string | null>;
  timelineZoom: Ref<number>;
  timelineHeight: Ref<number>;
  timelineScrollLeft: Ref<number>;
  timelineScrollTop: Ref<number>;
  snap: Ref<boolean>;
  deleteMode: Ref<DeleteMode>;
  monitorMuted: Ref<boolean>;
  playbackRate: Ref<number>;
  libraryHidden: Ref<boolean>;
  propertiesHidden: Ref<boolean>;
  propertiesOpen: Ref<boolean>;
  focusPreview: Ref<boolean>;
  captionSettingsOpen: Ref<boolean>;
  theme: Ref<Theme>;
}

function createFields(): WorkspaceFields {
  return {
    selectionClipIds: ref<string[]>([]),
    selected: ref<Selected | null>(null),
    playheadMs: ref(0),
    libraryTab: ref<string | null>(null),
    propertyTab: ref<string | null>(null),
    timelineZoom: ref(1),
    timelineHeight: ref(TIMELINE_DEFAULT_HEIGHT),
    timelineScrollLeft: ref(0),
    timelineScrollTop: ref(0),
    snap: ref(true),
    deleteMode: ref<DeleteMode>("gap"),
    monitorMuted: ref(false),
    playbackRate: ref(1),
    libraryHidden: ref(false),
    propertiesHidden: ref(false),
    propertiesOpen: ref(false),
    focusPreview: ref(false),
    captionSettingsOpen: ref(false),
    // Dark whatever the OS prefers (visual-parity design D1: the concept
    // opens dark and light is a View-menu choice); a saved theme still wins
    // on hydrate.
    theme: ref<Theme>("dark"),
  };
}

/** The exact wire shape `editor_save_workspace` takes — built fresh on
 * every persist so it always reflects whatever just changed. */
function snapshotWorkspace(f: WorkspaceFields): Workspace {
  return {
    selection_clip_ids: f.selectionClipIds.value,
    ...(f.selected.value ? { selected: f.selected.value } : {}),
    playhead_ms: f.playheadMs.value,
    ...(f.libraryTab.value !== null ? { library_tab: f.libraryTab.value } : {}),
    ...(f.propertyTab.value !== null ? { property_tab: f.propertyTab.value } : {}),
    timeline_zoom: f.timelineZoom.value,
    timeline_height: f.timelineHeight.value,
    timeline_scroll_left: f.timelineScrollLeft.value,
    timeline_scroll_top: f.timelineScrollTop.value,
    snap: f.snap.value,
    delete_mode: f.deleteMode.value,
    monitor_muted: f.monitorMuted.value,
    playback_rate: f.playbackRate.value,
    library_hidden: f.libraryHidden.value,
    properties_hidden: f.propertiesHidden.value,
    properties_open: f.propertiesOpen.value,
    focus_preview: f.focusPreview.value,
    caption_settings_open: f.captionSettingsOpen.value,
    theme: f.theme.value,
  };
}

type WorkspaceSetter<K extends keyof Workspace> = (
  f: WorkspaceFields,
  value: NonNullable<Workspace[K]>,
) => void;

/** One setter per `Workspace` key, keyed the same way — a DATA table, not a
 * chain of branches, precisely so `applyWorkspace` below stays a single
 * small loop instead of an 18-way if-chain (a flat sequence of independent
 * conditions is exactly what trips the cyclomatic-complexity ratchet, even
 * though none of them nest). */
const WORKSPACE_SETTERS: { [K in keyof Required<Workspace>]: WorkspaceSetter<K> } = {
  selection_clip_ids: (f, v) => (f.selectionClipIds.value = v),
  selected: (f, v) => (f.selected.value = v),
  playhead_ms: (f, v) => (f.playheadMs.value = v),
  library_tab: (f, v) => (f.libraryTab.value = v),
  property_tab: (f, v) => (f.propertyTab.value = v),
  timeline_zoom: (f, v) => (f.timelineZoom.value = v),
  timeline_height: (f, v) => (f.timelineHeight.value = v),
  timeline_scroll_left: (f, v) => (f.timelineScrollLeft.value = v),
  timeline_scroll_top: (f, v) => (f.timelineScrollTop.value = v),
  snap: (f, v) => (f.snap.value = v),
  delete_mode: (f, v) => (f.deleteMode.value = v),
  monitor_muted: (f, v) => (f.monitorMuted.value = v),
  playback_rate: (f, v) => (f.playbackRate.value = v),
  library_hidden: (f, v) => (f.libraryHidden.value = v),
  properties_hidden: (f, v) => (f.propertiesHidden.value = v),
  properties_open: (f, v) => (f.propertiesOpen.value = v),
  focus_preview: (f, v) => (f.focusPreview.value = v),
  caption_settings_open: (f, v) => (f.captionSettingsOpen.value = v),
  theme: (f, v) => (f.theme.value = v),
};

/** Apply only the fields `ws` actually carries — an absent key (never
 * saved, or sanitized away) keeps this store's current value rather than
 * being reset, since a JSON reply omits an absent optional key entirely
 * (`core::editor::workspace::Workspace`'s own `skip_serializing_if`). */
function applyWorkspace(f: WorkspaceFields, ws: Workspace): void {
  for (const key of Object.keys(ws) as (keyof Workspace)[]) {
    const value = ws[key];
    if (value === undefined) continue;
    const setter = WORKSPACE_SETTERS[key] as (f: WorkspaceFields, v: unknown) => void;
    setter(f, value);
  }
}

/** Drop any selected id that is no longer a real clip. Returns whether
 * anything changed, so the caller only re-persists when it actually did. */
function pruneSelection(f: WorkspaceFields, knownClipIds: ReadonlySet<string>): boolean {
  if (f.selectionClipIds.value.length === 0) return false;
  const pruned = f.selectionClipIds.value.filter((id) => knownClipIds.has(id));
  if (pruned.length === f.selectionClipIds.value.length) return false;
  f.selectionClipIds.value = pruned;
  return true;
}

/** A debounced `editor_save_workspace` sender, its timer private to the
 * closure this factory returns — each store instance calls this exactly
 * once, so the timer is never SHARED across instances. It can still
 * OUTLIVE one: nothing cancels a pending save when a test resets with
 * `setActivePinia(createPinia())`, so it fires up to 750 ms later against
 * that instance's own port and session — a test that changes a workspace
 * field must give its port a `saveWorkspace`. Its callback reads refs and
 * calls the port directly, never a store action (AGENTS.md § Testing
 * conventions), so a late fire re-activates no stale pinia. */
function createPersister(
  port: Ref<EditorPort>,
  sessionId: Ref<string | null>,
  fields: WorkspaceFields,
): () => void {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return function persist(): void {
    if (timer !== null) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      const sid = sessionId.value;
      if (!sid) return;
      port.value.saveWorkspace(sid, snapshotWorkspace(fields)).catch((e: unknown) => {
        // S-15 (hardening Task 12): by code and operationId, never by
        // message — which can carry a capture's own name in plain text.
        const failure = toEditorError(e);
        logWarning(
          `editorWorkspace: failed to persist workspace for session ${sid}: ${failure.code} (${failure.operationId})`,
        );
      });
    }, PERSIST_DEBOUNCE_MS);
  };
}

/** Reset every field to `createFields()`'s own defaults — straight-line
 * assignments, no branches, so this stays cheap on the complexity ratchet
 * `applyWorkspace`'s data table exists to respect. `createFields()` is the
 * single source of truth for what a default IS; this just copies its
 * output into the ALREADY-CREATED refs the store returned, since nothing
 * downstream can be handed a brand-new set of ref objects mid-session. */
function applyDefaults(f: WorkspaceFields): void {
  const d = createFields();
  f.selectionClipIds.value = d.selectionClipIds.value;
  f.selected.value = d.selected.value;
  f.playheadMs.value = d.playheadMs.value;
  f.libraryTab.value = d.libraryTab.value;
  f.propertyTab.value = d.propertyTab.value;
  f.timelineZoom.value = d.timelineZoom.value;
  f.timelineHeight.value = d.timelineHeight.value;
  f.timelineScrollLeft.value = d.timelineScrollLeft.value;
  f.timelineScrollTop.value = d.timelineScrollTop.value;
  f.snap.value = d.snap.value;
  f.deleteMode.value = d.deleteMode.value;
  f.monitorMuted.value = d.monitorMuted.value;
  f.playbackRate.value = d.playbackRate.value;
  f.libraryHidden.value = d.libraryHidden.value;
  f.propertiesHidden.value = d.propertiesHidden.value;
  f.propertiesOpen.value = d.propertiesOpen.value;
  f.focusPreview.value = d.focusPreview.value;
  f.captionSettingsOpen.value = d.captionSettingsOpen.value;
  f.theme.value = d.theme.value;
}

/** Reads `editor_get_workspace` back and applies it. `token` guards a
 * slower reply from a session this store has since moved on from — the
 * `editorProject` store's `generation` guard, scoped to this one async
 * operation.
 *
 * **Resets to defaults FIRST** (Task 18 fix round 1, finding 3): the editor
 * webview is reused across captures (`EditorRoot.vue`'s own doc), and
 * `applyWorkspace` only overwrites the keys a reply actually carries — so
 * without this reset, opening project B right after project A would leave
 * B reading A's leftover playhead/zoom/tabs/scroll/selection whenever B's
 * `workspace.json` is missing or only partially populated, and B's very
 * first edit would then persist A's values into B's own file. The reset
 * runs synchronously, in the same tick as the `token` bump above it, so it
 * can never race a guard: nothing else can execute between the two lines. */
function createHydrator(
  port: Ref<EditorPort>,
  sessionId: Ref<string | null>,
  fields: WorkspaceFields,
): (id: string) => Promise<void> {
  let token = 0;
  return async function hydrate(id: string): Promise<void> {
    sessionId.value = id;
    const myToken = ++token;
    applyDefaults(fields);
    try {
      const ws = await port.value.getWorkspace(id);
      if (myToken !== token || sessionId.value !== id) return;
      applyWorkspace(fields, ws);
    } catch (e) {
      if (myToken !== token) return;
      const failure = toEditorError(e);
      logWarning(
        `editorWorkspace: failed to hydrate workspace for session ${id}: ${failure.code} (${failure.operationId})`,
      );
    }
  };
}

/** Every mutator — presentation state only, never `editorProject.execute`
 * (this task's own `toggleMonitorMute` test). Every one calls `persist`.
 * The selection's own live in `workspaceSelection.ts`. */
function createMutators(f: WorkspaceFields, persist: () => void, getDurationMs: () => number) {
  return {
    setPlayhead(ms: number): void {
      f.playheadMs.value = clamp(ms, [0, getDurationMs()]);
      persist();
    },
    setZoom(zoom: number): void {
      f.timelineZoom.value = clamp(zoom, ZOOM_RANGE);
      persist();
    },
    /**
     * "Fit" the timeline to the current duration. No pixel viewport width
     * is known at the store layer — the timeline view itself arrives in a
     * later task — so this honestly resets to the default zoom and left
     * scroll rather than fake a "fits the whole duration" computation
     * against a viewport that does not exist yet (R20: nothing is faked).
     * A later task that DOES know the timeline's rendered width can
     * compute a real fit value and call `setZoom` directly instead.
     */
    fit(): void {
      f.timelineZoom.value = 1;
      f.timelineScrollLeft.value = 0;
      persist();
    },
    toggleSnap(): void {
      f.snap.value = !f.snap.value;
      persist();
    },
    setDeleteMode(mode: DeleteMode): void {
      f.deleteMode.value = mode;
      persist();
    },
    /** Local-only: never calls `editorProject.execute`. Muting the preview
     * monitor is not an edit to the project. */
    toggleMonitorMute(): void {
      f.monitorMuted.value = !f.monitorMuted.value;
      persist();
    },
    setLibraryTab(tab: string | null): void {
      f.libraryTab.value = tab;
      persist();
    },
    setPropertyTab(tab: string | null): void {
      f.propertyTab.value = tab;
      persist();
    },
    setTimelineScroll(left: number, top: number): void {
      f.timelineScrollLeft.value = left;
      f.timelineScrollTop.value = top;
      persist();
    },
    setPlaybackRate(rate: number): void {
      f.playbackRate.value = clamp(rate, PLAYBACK_RATE_RANGE);
      persist();
    },
    toggleLibraryHidden(): void {
      f.libraryHidden.value = !f.libraryHidden.value;
      persist();
    },
    togglePropertiesHidden(): void {
      f.propertiesHidden.value = !f.propertiesHidden.value;
      persist();
    },
    togglePropertiesOpen(): void {
      f.propertiesOpen.value = !f.propertiesOpen.value;
      persist();
    },
    toggleCaptionSettingsOpen(): void {
      f.captionSettingsOpen.value = !f.captionSettingsOpen.value;
      persist();
    },
    setTheme(next: Theme): void {
      f.theme.value = next;
      persist();
    },
    toggleTheme(): void {
      f.theme.value = f.theme.value === "light" ? "dark" : "light";
      persist();
    },
  };
}

/** The concept's fitted-range framing (`goToSelection(true)`): the range
 * fills the lanes (`timelineLayout.fitRangeZoom`, beside Fit's own rule)
 * and starts 25 px in from their left. */
const RANGE_LEAD_PX = 25;

/** "Fit this clip" / "Fit selection" (visual-parity Task 5): zoom so the
 * range fills the visible lanes, and scroll it into view — a view change
 * only, never an edit. Returns the new scroll offset, which the timeline
 * applies to its scroller (the store cannot reach the element). */
function createRangeZoom(f: WorkspaceFields, persist: () => void, windowWidth: Ref<number>) {
  return {
    zoomToRange(startMs: number, endMs: number, viewportPx: number): number {
      const zoom = clamp(fitRangeZoom(endMs - startMs, viewportPx, trackLabelWidthAt(windowWidth.value)), ZOOM_RANGE);
      const left = Math.max(0, Math.round(msToX(startMs, zoom) - RANGE_LEAD_PX));
      f.timelineZoom.value = zoom;
      f.timelineScrollLeft.value = left;
      persist();
      return left;
    },
  };
}

export const useEditorWorkspaceStore = defineStore("editorWorkspace", () => {
  // `markRaw`/`shallowRef`, the `editorProject` store's own precedent: this
  // holds no state worth making reactive, and proxying a test double's
  // closures (or, in production, `invoke`'s own plumbing) is exactly the
  // surprise `markRaw` exists to prevent.
  const port = shallowRef<EditorPort>(markRaw(createTauriEditorPort()));
  function setPort(p: EditorPort): void {
    port.value = markRaw(p);
  }

  /** The project this store's state is scoped to, set by `hydrate()`.
   * `persist()` is a no-op while this is `null`. */
  const sessionId = ref<string | null>(null);
  const fields = createFields();

  const persist = createPersister(port, sessionId, fields);
  const hydrate = createHydrator(port, sessionId, fields);

  // Task 18's own named case: a clip deleted (or undone away) out from
  // under a live multi-selection must not leave `selectionClipIds`
  // pointing at entities that no longer exist. Watching `snapshot.revision`
  // rather than the whole `project` graph means this fires once per
  // installed projection, not once per field inside it.
  const editorProject = useEditorProjectStore();
  const mutators = createMutators(fields, persist, () => editorProject.durationMs);
  // Visual-parity Task 13: the selected track (view state, never persisted;
  // `workspaceSelection.ts`), forgotten with the session that had it.
  const selectedTrackId = ref<string | null>(null);
  const selection = createSelection({ ...fields, selectedTrackId }, persist);
  watch(sessionId, () => (selectedTrackId.value = null));

  // Visual-parity Task 4 (D4, D5): the window's size (session state, never
  // persisted — the shell keeps it current) and the library drawer, which
  // the concept does not persist either; the panel rules live in
  // `panelLayout.ts`.
  const viewportWidth = ref(window.innerWidth);
  const viewportHeight = ref(window.innerHeight);
  const libraryDrawerOpen = ref(false);
  const panels = createPanelControls(
    { ...fields, libraryDrawerOpen, viewportWidth, viewportHeight },
    persist,
  );
  watch(
    () => editorProject.snapshot?.revision,
    () => {
      const knownIds = new Set((editorProject.project?.clips ?? []).map((c) => c.id));
      if (pruneSelection(fields, knownIds)) persist();
      pruneSelectedTrack(selectedTrackId, (editorProject.project?.tracks ?? []).map((t) => t.id));
    },
  );

  return {
    sessionId,
    selectionClipIds: fields.selectionClipIds,
    selected: fields.selected,
    selectedTrackId,
    playheadMs: fields.playheadMs,
    libraryTab: fields.libraryTab,
    propertyTab: fields.propertyTab,
    timelineZoom: fields.timelineZoom,
    timelineHeight: fields.timelineHeight,
    timelineScrollLeft: fields.timelineScrollLeft,
    timelineScrollTop: fields.timelineScrollTop,
    snap: fields.snap,
    deleteMode: fields.deleteMode,
    monitorMuted: fields.monitorMuted,
    playbackRate: fields.playbackRate,
    libraryHidden: fields.libraryHidden,
    propertiesHidden: fields.propertiesHidden,
    propertiesOpen: fields.propertiesOpen,
    focusPreview: fields.focusPreview,
    captionSettingsOpen: fields.captionSettingsOpen,
    theme: fields.theme,
    viewportWidth,
    viewportHeight,
    libraryDrawerOpen,
    setPort,
    hydrate,
    persist,
    ...mutators,
    ...selection,
    ...createRangeZoom(fields, persist, viewportWidth),
    ...panels,
    ...createDisclosures(sessionId),
  };
});
