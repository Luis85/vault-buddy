/**
 * The open session's before-you-share findings (Task 54; F-45; SCREENS
 * 07), as Rust last computed them (`editor_get_checks`). Nothing here is
 * judged locally: which finding blocks a render is Rust's `severity`, never
 * a second rule in the webview.
 *
 * Re-read whenever the project's revision, the session or the number of
 * open webcam takes changes (`EditorHeader` drives it, since the header's
 * badge is always on screen), and when the Checks or Render dialog opens.
 * Scoped to the session it was read for, and a read that lands after a
 * newer one was asked for is dropped — the `editorProducts` discipline.
 *
 * A failed read is kept in `error` and shown as such: a check that could
 * not look must never read as "nothing found".
 */
import { defineStore } from "pinia";

import type { CheckFinding, EditorError } from "../editorTypes";
import { toEditorError, useEditorProjectStore } from "./editorProject";

export const useEditorChecksStore = defineStore("editorChecks", {
  state: () => ({
    /** The session `findings` was read for. */
    sessionId: null as string | null,
    findings: [] as CheckFinding[],
    /** The last failed read, for the open session. */
    error: null as EditorError | null,
    /** Bumped per read; only the newest read installs. */
    ticket: 0,
  }),
  getters: {
    /** The open session's findings, in Rust's order. */
    current(state): CheckFinding[] {
      return state.sessionId !== null && state.sessionId === useEditorProjectStore().sessionId
        ? state.findings
        : [];
    },
    /** The last failed read, when it was the open session's. */
    currentError(state): EditorError | null {
      return state.sessionId === useEditorProjectStore().sessionId ? state.error : null;
    },
    blocking(): CheckFinding[] {
      return this.current.filter((f) => f.severity === "blocking");
    },
    warnings(): CheckFinding[] {
      return this.current.filter((f) => f.severity === "warning");
    },
    /** Blockers and warnings: what the header's badge counts. Notes are
     * information, not something to act on before sharing. */
    toReview(): number {
      return this.blocking.length + this.warnings.length;
    },
    /** Why a render may not start, or `null`: only blocking findings
     * block (SCREENS 07: "Warnings are not all export blockers"). */
    blockedReason(): string | null {
      const n = this.blocking.length;
      if (n === 0) return null;
      return n === 1 ? "Fix the blocking check first." : "Fix the blocking checks first.";
    },
    /** "1 blocker · 2 review warnings" — the dialog's and the Render
     * dialog's one summary line (SCREENS 07), never a score. */
    summary(): string {
      const b = this.blocking.length;
      const w = this.warnings.length;
      return `${b} blocker${b === 1 ? "" : "s"} · ${w} review warning${w === 1 ? "" : "s"}`;
    },
  },
  actions: {
    /** Re-read the findings for the open session. */
    async refresh(): Promise<void> {
      const project = useEditorProjectStore();
      const sessionId = project.sessionId;
      if (!sessionId) return;
      this.ticket += 1;
      const ticket = this.ticket;
      try {
        const findings = await project.port.getChecks(sessionId);
        if (ticket !== this.ticket || project.sessionId !== sessionId) return;
        this.sessionId = sessionId;
        this.findings = findings;
        this.error = null;
      } catch (e) {
        if (ticket !== this.ticket) return;
        this.sessionId = sessionId;
        this.findings = [];
        this.error = toEditorError(e);
      }
    },
  },
});
