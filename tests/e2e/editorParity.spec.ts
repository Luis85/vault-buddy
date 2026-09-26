import { expect, test } from "@playwright/test";

import { box, composite, openParity } from "./parity";

/**
 * Concept-parity checks for the tutorial editor's UI port
 * (`docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-design.md`).
 * Each region's task appends its own `test(...)` block here, built on the
 * shared harness in `parity.ts` and the populated sample project in
 * `fixtures/parityProject.ts` — see Task 1's report for the harness
 * itself. A composite PNG comparing the matching concept screenshot
 * (`docs/concepts/vault-buddy-editor/screens/*.png`) against the built app
 * lands in `test-results/parity/` for every test here, pass or fail.
 */

test.describe("parity 1600x1000", () => {
  // Confirmed failing today (Task 1's RED step): the header renders at 54px,
  // not 56, and `editor-statusbar` does not exist yet. `test.fail()` marks
  // that as the EXPECTED result so the suite stays green between tasks —
  // Task 3 (tokens/frame sizing) and Task 4 (the status bar) turn each
  // assertion green in turn.
  // turned green by Task 4 (frame)
  test.fail(
    "frame: header 56, library 244, inspector 276, status 25",
    async ({ page }) => {
      await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
      await page.screenshot({ path: "test-results/parity/built-02-workspace.png" });
      await composite(page, "02-workspace.png", "test-results/parity/built-02-workspace.png", "vs-02-workspace");
      expect((await box(page, "editor-header")).height).toBeCloseTo(56, 0);
      expect((await box(page, "editor-shell-library")).width).toBeCloseTo(244, 0);
      expect((await box(page, "editor-shell-inspector")).width).toBeCloseTo(276, 0);
      expect((await box(page, "editor-statusbar")).height).toBeCloseTo(25, 0);
    },
  );
});
