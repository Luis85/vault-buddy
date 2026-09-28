/**
 * The inspector's precision disclosures — "Frame & crop", "Transform
 * source" — remember whether they are open per selection (visual-parity
 * Task 14; concept `workspace-ui.js`'s `expandedSections`, keyed by the
 * selection and the section's title). View state only: never an edit,
 * never in `workspace.json`, and forgotten with the session that had it.
 */
import type { Ref } from "vue";
import { ref, watch } from "vue";

export function createDisclosures(sessionId: Ref<string | null>) {
  const open = ref<Record<string, boolean>>({});
  watch(sessionId, () => (open.value = {}));
  return {
    /** Whether the disclosure `key` was left open (closed by default). */
    disclosureOpen: (key: string): boolean => open.value[key] === true,
    setDisclosureOpen(key: string, value: boolean): void {
      open.value = { ...open.value, [key]: value };
    },
  };
}
