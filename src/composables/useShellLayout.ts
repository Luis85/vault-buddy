/**
 * The editor frame's grid (visual-parity Task 4; design D4, D5; concept
 * spec §1.3–§1.5): `EditorShell`'s rows and workspace columns, and where
 * each side panel sits — a grid column above its drawer breakpoint, an
 * overlay drawer at or below it. The panel STATE is `editorWorkspace`'s
 * (`panelLayout.ts` holds its rules); this composable keeps the store's
 * window size current and turns that state into styles and classes.
 *
 * Rows: `56 / minmax(170px,1fr) / 8 / var(--timeline) / 25`, or
 * `52 / minmax(160px,1fr) / 8 / … / 23` in a window 760px tall or less.
 * Columns: library (244, or 232 at ≤1080) / preview / inspector (276), each
 * 0 when its panel is hidden or is a drawer. Every grid child keeps its own
 * column (`col-start-*`), so a hidden sibling never shifts the others.
 * Drawers open below the preview header (48px, 44 in a short window), so
 * the panel toggles in it stay reachable while one is open.
 */
import { computed, onBeforeUnmount, onMounted } from "vue";

import { onReveal } from "../editor/revealBus";
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";

const ROWS = "56px minmax(170px,1fr) 8px var(--timeline) 25px";
const SHORT_ROWS = "52px minmax(160px,1fr) 8px var(--timeline) 23px";

export function useShellLayout() {
  const workspace = useEditorWorkspaceStore();

  function syncViewport(): void {
    workspace.setViewport(window.innerWidth, window.innerHeight);
  }
  onMounted(() => {
    syncViewport();
    window.addEventListener("resize", syncViewport);
  });
  onBeforeUnmount(() => window.removeEventListener("resize", syncViewport));

  // A before-you-share finding (Task 54) or a guide lesson (Task 56) asks
  // for a panel: show it wherever it lives at this width.
  onReveal("library", () => workspace.revealLibrary());
  onReveal("inspector", () => workspace.revealInspector());

  const frameStyle = computed(() => ({
    gridTemplateRows: workspace.shortWindow ? SHORT_ROWS : ROWS,
    "--timeline": `${workspace.timelineHeightPx}px`,
  }));

  const libraryColumn = computed(() => {
    if (workspace.libraryIsDrawer || !workspace.libraryVisible) return "0px";
    return workspace.inspectorIsDrawer ? "var(--editor-sidebar-compact)" : "var(--editor-sidebar)";
  });
  const inspectorColumn = computed(() =>
    !workspace.inspectorIsDrawer && workspace.inspectorVisible ? "var(--editor-inspector)" : "0px",
  );
  const workspaceStyle = computed(() => ({
    gridTemplateColumns: `${libraryColumn.value} minmax(0,1fr) ${inspectorColumn.value}`,
    "--drawer-top": workspace.shortWindow ? "44px" : "48px",
  }));

  const libraryClass = computed(() => (workspace.libraryIsDrawer ? "library-drawer" : "col-start-1 row-start-1"));
  const inspectorClass = computed(() =>
    workspace.inspectorIsDrawer ? "inspector-drawer" : "col-start-3 row-start-1",
  );

  return { frameStyle, workspaceStyle, libraryClass, inspectorClass };
}
