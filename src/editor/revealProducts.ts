/**
 * "Workspace & rendered products" (design D9, D10): where a person sees the
 * videos this project has rendered. The status bar's right slot and the
 * header's Project menu both go through this one helper, so they cannot
 * disagree about where that is. Until the library's Project section exists
 * (visual-parity Task 10, which retargets this) it is the library's
 * Products tab, shown through the same reveal a Checks finding uses — a
 * hidden column comes back, a closed drawer opens.
 */
import { useEditorWorkspaceStore } from "../stores/editorWorkspace";
import { requestReveal } from "./revealBus";

export function revealWorkspaceProducts(): void {
  useEditorWorkspaceStore().setLibraryTab("products");
  requestReveal("library");
}
