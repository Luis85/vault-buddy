import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, type Page, test } from "@playwright/test";

import { FIXTURE_VIDEO_URL, installTauriStub } from "./tauriStub";

/**
 * `EditorShell`/`EditorHeader` (Task 16, F-48), measured in a real browser
 * against the production bundle — `tests/editorShell.test.ts` pins the
 * CLASS contract (happy-dom has no layout engine, AGENTS.md's Testing
 * conventions); this file is where a real box is measured, the same reason
 * `editorLayout.spec.ts` exists.
 *
 * The shell renders ALONGSIDE `LegacyCaptureEditor` (Task 21 retires the
 * legacy surface; until then both are on screen at once), so this reuses
 * the same fixture video and stub as `editorLayout.spec.ts` — the media
 * still has to have a real intrinsic size for the layout to be genuinely
 * exercised, per that file's own header.
 */

const FIXTURE = fileURLToPath(new URL("./fixtures/capture-1920x1080.webm", import.meta.url));

async function openEditor(page: Page, size: { width: number; height: number }) {
  await installTauriStub(page);
  await page.route(`**${FIXTURE_VIDEO_URL}`, (route) =>
    route.fulfill({ contentType: "video/webm", body: readFileSync(FIXTURE) }),
  );
  await page.setViewportSize(size);
  await page.goto("/");

  // The new shell really mounted (not just the legacy surface it sits
  // beside) — `sessionMatchesLegacy` gates it, so this also proves the
  // store's own `openStaged` round trip landed against the stub.
  await expect(page.getByTestId("editor-shell")).toBeAttached();
  await expect(page.getByTestId("editor-header")).toBeAttached();
}

test("exactly one preview toolbar row at 960x640", async ({ page }) => {
  await openEditor(page, { width: 960, height: 640 });

  // A placeholder until Task 17 replaces its content — the wrapper itself
  // is what SCREENS-AND-INTERACTIONS.md §02/§12 pins ("one preview toolbar
  // row"), so a later task that fills it in must keep exactly one.
  await expect(page.getByTestId("preview-toolbar")).toHaveCount(1);
});

test("no horizontal page scroll at 960x640", async ({ page }) => {
  await openEditor(page, { width: 960, height: 640 });

  // Measured on `main`, the editor's own scroll container (AGENTS.md's
  // Testing conventions: "Measure the real scroll container") — the
  // `editorLayout.spec.ts` precedent, now with the new shell's own grid
  // (library/inspector drawers collapsed at this width, per §12) stacked
  // above the legacy surface it still renders beside.
  const overflow = await page.evaluate(() => {
    const m = document.querySelector("main")!;
    return m.scrollWidth - m.clientWidth;
  });
  expect(
    overflow,
    `the editor shell overflowed horizontally by ${overflow}px at 960x640`,
  ).toBeLessThanOrEqual(0);
});

test("below 1180px the library toggle is a real drawer control and the header stays on screen", async ({
  page,
}) => {
  await openEditor(page, { width: 960, height: 640 });

  const toggle = page.getByTestId("editor-header-library-toggle");
  await expect(toggle).toBeVisible();
  await expect(toggle).toHaveAttribute("aria-expanded", "false");
  await expect(page.getByTestId("editor-shell-library")).toBeHidden();

  await toggle.click();

  await expect(toggle).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByTestId("editor-shell-library")).toBeVisible();
  // Opening the drawer must never cover or hide the header — "the route
  // back" (this task's Behavior section).
  await expect(page.getByTestId("editor-header")).toBeInViewport();
});
