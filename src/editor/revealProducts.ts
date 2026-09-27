/**
 * "Workspace & rendered products" (design D9, D10): where a person sees the
 * videos this project has rendered — the library's Project section
 * ("YOUR WORKSPACE", concept spec §3.6, visual-parity Task 10). The status
 * bar's right slot, the header's Project menu and the Render dialog's
 * completion all go through this one helper, so they cannot disagree about
 * where that is: `LibraryPanel` answers the `projectSection` reveal by
 * showing the section, and the `library` reveal brings a hidden column
 * back or opens a closed drawer — the same reveal a Checks finding uses.
 */
import { requestReveal } from "./revealBus";

export function revealWorkspaceProducts(): void {
  requestReveal("projectSection");
  requestReveal("library");
}
