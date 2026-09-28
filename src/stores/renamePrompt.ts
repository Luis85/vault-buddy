/** The capture store's post-save rename-prompt fields. */
export interface RenamePromptFields {
  lastSaved: unknown;
  lastSavedAtMs: number | null;
  renameError: string | null;
  renameTimer: ReturnType<typeof setTimeout> | null;
}

/** Drop the post-save rename prompt. A plain function, not a store action,
 * because the prompt's 30 s expiry timer calls it: Pinia's action wrapper
 * would setActivePinia(the capture store's pinia) whenever that timer fires —
 * in a test, in the middle of a LATER test (AGENTS.md > Testing conventions). */
export function clearRenamePrompt(s: RenamePromptFields) {
  if (s.renameTimer) {
    clearTimeout(s.renameTimer);
    s.renameTimer = null;
  }
  s.lastSaved = null;
  s.lastSavedAtMs = null;
  s.renameError = null;
}
