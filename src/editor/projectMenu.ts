/**
 * The header's Project menu (visual-parity Task 8; design D7; concept spec
 * §2): the concept's slots with the native items. "New project…" and
 * "Restore sample project…" have no native equivalent and are omitted —
 * never shown disabled (D14) — and so is "Open project…" when no other
 * stored project exists to open. The items only say what to do; the
 * header's `ProjectMenuButton` supplies each effect.
 */
import type { MenuItem } from "../components/editor/menus/menuModel";
import { SEPARATOR } from "../components/editor/menus/menuModel";

export const PROJECT_MENU_HEADING = "Project";
export const PROJECT_MENU_SUBTITLE = "Working files, originals and rendered products";

const NO_PROJECT = "No project is open.";

export interface ProjectMenuContext {
  /** A session is open (rename, save a copy and discard need one). */
  sessionOpen: boolean;
  /** Another stored project exists besides the open one. */
  hasOtherProjects: boolean;
  openProject: () => void;
  openProjectFile: () => void;
  rename: () => void;
  showProducts: () => void;
  saveCopy: () => void;
  discard: () => void;
}

export function projectMenuItems(ctx: ProjectMenuContext): MenuItem[] {
  const needsSession = ctx.sessionOpen ? null : NO_PROJECT;
  const open: MenuItem[] = ctx.hasOtherProjects
    ? [{ id: "open", label: "Open project…", icon: "folder", run: ctx.openProject }]
    : [];
  return [
    ...open,
    { id: "openFile", label: "Open a project file…", icon: "upload", run: ctx.openProjectFile },
    { id: "rename", label: "Rename tutorial…", icon: "edit", disabledReason: needsSession, run: ctx.rename },
    SEPARATOR,
    { id: "products", label: "Workspace & rendered products", icon: "layers", run: ctx.showProducts },
    { id: "saveCopy", label: "Save a copy as project file…", icon: "save", disabledReason: needsSession, run: ctx.saveCopy },
    SEPARATOR,
    { id: "discard", label: "Discard project…", icon: "trash", danger: true, disabledReason: needsSession, run: ctx.discard },
  ];
}
