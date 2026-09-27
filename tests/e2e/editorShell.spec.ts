import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, type Page, test } from "@playwright/test";

import { previewTool } from "./parity";
import { FIXTURE_VIDEO_URL, installTauriStub } from "./tauriStub";

/**
 * `EditorShell`/`EditorHeader` (Task 16, F-48), measured in a real browser
 * against the production bundle — `tests/editorShell.test.ts` pins the
 * CLASS contract (happy-dom has no layout engine, AGENTS.md's Testing
 * conventions); this file is where a real box is measured (docs/Gaps.md
 * GAP-162).
 *
 * Task 59 folded `editorLayout.spec.ts` in here. That file measured the
 * retired phase-4 surface (its strip, its Save to vault) stacked beneath
 * this shell; what outlives it is the window-size contract itself — at the
 * editor's floor, its default and two larger sizes the header's Save and
 * Render video stay on screen and the column FITS (measured on `main`, the
 * real scroll container, never `document.documentElement`, which reads 0
 * at every size and cannot fail).
 */

const FIXTURE = fileURLToPath(new URL("./fixtures/capture-1920x1080.webm", import.meta.url));

async function openEditor(page: Page, size: { width: number; height: number }) {
  await installTauriStub(page);
  await page.route(`**${FIXTURE_VIDEO_URL}`, (route) =>
    route.fulfill({ contentType: "video/webm", body: readFileSync(FIXTURE) }),
  );
  await page.setViewportSize(size);
  await page.goto("/");

  // The shell really mounted — `EditorRoot` gates it on the store's own
  // reply matching the drained request, so this also proves the store's
  // `openStaged` round trip landed against the stub.
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
  // retired `editorLayout.spec.ts`'s rule, with the shell's own grid
  // (the inspector a closed drawer at this width, concept spec §1.4).
  const overflow = await page.evaluate(() => {
    const m = document.querySelector("main")!;
    return m.scrollWidth - m.clientWidth;
  });
  expect(
    overflow,
    `the editor shell overflowed horizontally by ${overflow}px at 960x640`,
  ).toBeLessThanOrEqual(0);
});

// Visual-parity Task 4 (design D4, D5; concept spec §1.4): the inspector
// becomes an overlay drawer at or below 1080px wide, the library at or
// below 860px. Each drawer toggle is a real control (the preview
// toolbar's, visual-parity Task 8 — the header's left), the drawer opens
// BELOW the preview header (so the toggles that close it stay reachable),
// and the header — "the route back" — is never covered.
test("at or below 1080px the inspector is a real drawer and the header stays on screen", async ({ page }) => {
  await openEditor(page, { width: 960, height: 640 });

  await expect(page.getByTestId("editor-shell-inspector")).toBeHidden();
  await expect(page.getByTestId("editor-shell-library")).toBeVisible();

  await previewTool(page, "toggleInspector");

  const drawer = page.getByTestId("editor-shell-inspector");
  await expect(drawer).toBeVisible();
  expect((await drawer.boundingBox())!.width).toBeCloseTo(276, 0);
  const toolbar = (await page.getByTestId("preview-toolbar").boundingBox())!;
  expect((await drawer.boundingBox())!.y).toBeGreaterThanOrEqual(toolbar.y + toolbar.height - 1);
  await expect(page.getByTestId("editor-header")).toBeInViewport();
});

test("at or below 860px the library is a 250px drawer under the preview header", async ({ page }) => {
  await openEditor(page, { width: 840, height: 640 });

  await expect(page.getByTestId("editor-shell-library")).toBeHidden();
  await previewTool(page, "toggleLibrary");

  const drawer = page.getByTestId("editor-shell-library");
  await expect(drawer).toBeVisible();
  const b = (await drawer.boundingBox())!;
  expect(b.x).toBeCloseTo(0, 0);
  expect(b.width).toBeCloseTo(250, 0);
  const toolbar = (await page.getByTestId("preview-toolbar").boundingBox())!;
  expect(b.y).toBeGreaterThanOrEqual(toolbar.y + toolbar.height - 1);
  await expect(page.getByTestId("editor-header")).toBeInViewport();
});

// Ruling T4-1: at the editor's default size the header's save text is
// hidden (concept spec §1.4), but a refused save never is. The stub has no
// reply for `editor_save_project`, so the port refuses the save.
test("1280x820: a failed save is visible in the header and the status bar", async ({ page }) => {
  await openEditor(page, { width: 1280, height: 820 });
  await expect(page.getByTestId("editor-header-save-state")).toBeHidden();

  await page.getByTestId("editor-header-save").click();

  const header = page.getByTestId("editor-header-save-state");
  await expect(header).toBeVisible();
  await expect(header).toHaveText("Save failed");
  const recovery = page.getByTestId("editor-statusbar-recovery");
  await expect(recovery).toBeVisible();
  await expect(recovery).toHaveText("Save failed");
  expect(await recovery.getAttribute("title")).not.toMatch(/<(path|name):#/);
});

// The editor's floor (its `minWidth`/`minHeight`, tutorial-editor Task 15),
// its default open size, and two larger ones ("dragged wider" and
// "maximised" — the sizes GAP-162's two defects were reported at).
for (const size of [
  { width: 960, height: 640 },
  { width: 1280, height: 820 },
  { width: 1600, height: 1000 },
  { width: 1920, height: 1080 },
]) {
  const label = `${size.width}x${size.height}`;

  test(`${label}: Save and Render stay on screen and the column fits`, async ({ page }) => {
    await openEditor(page, size);

    await expect(page.getByTestId("editor-header-save")).toBeInViewport();
    await expect(page.getByTestId("editor-header-render")).toBeInViewport();

    const overflow = await page.evaluate(() => {
      const m = document.querySelector("main")!;
      return m.scrollHeight - m.clientHeight;
    });
    // Task 59: the retired phase-4 surface is gone from beneath the shell,
    // so the floor's widened tolerance (30px, while both surfaces were
    // stacked) goes back to 1 at every size, as that file's own lower-bound
    // check asked.
    expect(overflow, `the editor column overflowed by ${overflow}px at ${label}`).toBeLessThanOrEqual(1);

    const sideways = await page.evaluate(() => {
      const m = document.querySelector("main")!;
      return m.scrollWidth - m.clientWidth;
    });
    expect(sideways, `the editor overflowed sideways by ${sideways}px at ${label}`).toBeLessThanOrEqual(0);
    await expect(page.getByTestId("preview-toolbar")).toHaveCount(1);
  });
}
