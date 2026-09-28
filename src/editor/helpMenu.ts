/**
 * The header's Help menu (Tasks 55–58; visual-parity Task 23, ruling T8-1:
 * the Help dropdown renders through the one `MenuPanel`, like the Project
 * and View menus). Four items, each with a native effect the header's
 * `GuideHelpButton` supplies: the learning center, resuming the walkthrough
 * at its saved lesson (F1 does the same), the learning center's shortcut
 * table, and Export diagnostics (Rust's own save dialog, counts and codes
 * only). Nothing here is an edit.
 */
import type { MenuItem } from "../components/editor/menus/menuModel";
import { SEPARATOR } from "../components/editor/menus/menuModel";

export const HELP_MENU_HEADING = "Help";
export const HELP_MENU_SUBTITLE = "Learn the editor · nothing here edits";

export interface HelpMenuContext {
  openLearningCenter: () => void;
  resumeWalkthrough: () => void;
  openShortcuts: () => void;
  exportDiagnostics: () => void;
}

export function helpMenuItems(ctx: HelpMenuContext): MenuItem[] {
  return [
    { id: "learningCenter", label: "Learning center", icon: "book", run: ctx.openLearningCenter },
    { id: "resume", label: "Resume walkthrough", icon: "bookmark", kbd: "F1", run: ctx.resumeWalkthrough },
    { id: "shortcuts", label: "Keyboard shortcuts", icon: "info", run: ctx.openShortcuts },
    SEPARATOR,
    { id: "diagnostics", label: "Export diagnostics", icon: "file", run: ctx.exportDiagnostics },
  ];
}
