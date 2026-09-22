import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, type Page,test } from "@playwright/test";

import { FIXTURE_VIDEO_URL, installTauriStub } from "./tauriStub";

/**
 * The editor window at real sizes, measured in a real browser
 * (docs/Gaps.md GAP-162).
 *
 * WHY THIS FILE EXISTS. `tests/editorLayout.test.ts` pins the same layout in
 * Vitest, and says in its own header that happy-dom has no layout engine, so
 * it asserts the CLASS CONTRACT rather than pixels. That is the strongest
 * thing available there and it is not enough: a layout defect that does not
 * change a class — a competing `min-height` on an ancestor, a specificity
 * collision, a Tailwind class purged out of the production build — is
 * undetectable by construction. Two window-sizing defects shipped through it,
 * both found by a human dragging the window.
 *
 * So this measures. Every assertion below reads a real `boundingBox()` or a
 * real scroll height out of Chromium, against the PRODUCTION bundle.
 *
 * THE FIXTURE IS THE CONTROL, and without it this file would be theatre. The
 * defect is that the preview `<video>` was `w-full` with NO height bound, so
 * it sized itself from its width at its own intrinsic aspect ratio — widen
 * the window, and it grows taller and squeezes everything below it. A
 * `<video>` with no media has no intrinsic ratio: it renders at the CSS
 * default 300x150 and the bug cannot reproduce. `the_fixture_really_loads`
 * is therefore the first test in the file, and every other test waits for
 * `videoWidth > 0` before it measures anything.
 */

const FIXTURE = fileURLToPath(new URL("./fixtures/capture-1920x1080.webm", import.meta.url));

/** The strip is `h-16` — 64px. A few px of tolerance for sub-pixel layout,
 *  but nowhere near the hairline it collapsed to. */
const MIN_STRIP_HEIGHT = 60;

/**
 * Open the built editor at `size` and wait until the preview's media really
 * governs the layout.
 */
async function openEditor(page: Page, size: { width: number; height: number }) {
  await installTauriStub(page);
  await page.route(`**${FIXTURE_VIDEO_URL}`, (route) =>
    route.fulfill({ contentType: "video/webm", body: readFileSync(FIXTURE) }),
  );
  await page.setViewportSize(size);
  await page.goto("/");

  // The editor really mounted — not BuddyRoot, and not an error banner.
  //
  // ATTACHED, not visible, and deliberately NOT the strip. Playwright calls a
  // zero-area element hidden, and a collapsed strip is the very defect under
  // test: checking its visibility here reported the reported bug as "the
  // editor did not mount" and stopped before a single measurement ran. The
  // sanity check has to be something the layout cannot swallow.
  await expect(page.getByTestId("export-save")).toBeAttached();
  await expect(page.getByTestId("timeline-strip")).toBeAttached();
  // The media dictates the box. Without this the measurements below are of a
  // 300x150 placeholder and would pass against the defect.
  await page.waitForFunction(() => {
    const v = document.querySelector<HTMLVideoElement>('[data-testid="preview-video"]');
    return !!v && v.videoWidth > 0;
  });
}

test("the fixture really loads, so the preview has an intrinsic aspect ratio", async ({
  page,
}) => {
  await openEditor(page, { width: 1280, height: 720 });
  const dims = await page.evaluate(() => {
    const v = document.querySelector<HTMLVideoElement>('[data-testid="preview-video"]');
    return { w: v?.videoWidth ?? 0, h: v?.videoHeight ?? 0 };
  });
  // If this ever reads 0x0 the rest of the file proves nothing.
  expect(dims).toEqual({ w: 1920, h: 1080 });
});

// The two defects a human reported on a 3840x2400 display, at the sizes that
// produced them. 960x640 is the OLD floor (still the editor's `minWidth`/
// `minHeight` since tutorial-editor Task 15 widened the default open size to
// 1280x820 — the new workspace UI needs more room than the phase-4 timeline
// editor did); 1280x820 is that new default; the wider two are "dragged
// wider" and "maximised".
for (const size of [
  { width: 960, height: 640 },
  { width: 1280, height: 820 },
  { width: 1600, height: 1000 },
  { width: 1920, height: 1080 },
]) {
  const label = `${size.width}x${size.height}`;

  test(`${label}: Save stays on screen and the strip keeps its height`, async ({ page }) => {
    await openEditor(page, size);

    // DEFECT 1: the strip collapsed to a hairline as the window widened,
    // because `h-16` is a height and a flex item with no intrinsic content
    // height yields its space first.
    const strip = await page.getByTestId("timeline-strip").boundingBox();
    expect(strip, "the strip has no box at all").not.toBeNull();
    expect(
      strip!.height,
      `the timeline strip collapsed to ${strip!.height}px at ${label}`,
    ).toBeGreaterThanOrEqual(MIN_STRIP_HEIGHT);

    // DEFECT 2: maximised, Save to vault and Discard were pushed past the
    // bottom edge with nothing to scroll.
    await expect(page.getByTestId("export-save")).toBeInViewport();
    await expect(page.getByTestId("export-discard")).toBeInViewport();

    // And the column really FITS rather than merely being scrollable to.
    //
    // Measured on `main`, which is the scroll container (`h-screen` +
    // `overflow-y-auto`), NOT on `document.documentElement`. The document
    // never scrolls in this window, so the documentElement spelling of this
    // assertion reads 0 at every size and cannot fail — it passed against
    // the defect it was written for.
    const overflow = await page.evaluate(() => {
      const m = document.querySelector("main")!;
      return m.scrollHeight - m.clientHeight;
    });
    expect(
      overflow,
      `the editor column overflowed by ${overflow}px at ${label}`,
    ).toBeLessThanOrEqual(1);
  });
}

/**
 * The preview can give up all of its height, so the column can only overflow
 * when the header, strip, verbs and bar ALONE exceed the window. Losing the
 * save THERE would be the same defect in a smaller window, so `main` scrolls.
 *
 * 900x160 is measured, not guessed: that chrome comes to 230px, so 900x360 —
 * the size this test was first written at — leaves Save comfortably on screen
 * and the assertion passed without the fallback ever engaging. The
 * precondition below is what keeps it honest: it fails if a future layout
 * change makes the chrome fit again, rather than quietly proving nothing.
 *
 * (The test that stood between this one and the loop above is GONE on
 * purpose. It asserted `getComputedStyle(video).objectFit === "contain"`,
 * which is Chromium's UA-stylesheet default for `<video>` — probed, not
 * assumed. It could not fail, with or without the `object-contain` class.)
 */
test("a window too short for the chrome scrolls to the save instead of hiding it", async ({
  page,
}) => {
  await openEditor(page, { width: 900, height: 160 });
  const save = page.getByTestId("export-save");

  // PRECONDITION, asserted rather than assumed: Save is genuinely off the
  // bottom of the scroll container before we scroll. Without this the test
  // passes on any window big enough to fit the chrome, having exercised
  // nothing — which is exactly what 900x360 did.
  const hidden = await page.evaluate(() => {
    const m = document.querySelector("main")!;
    const s = document.querySelector('[data-testid="export-save"]')!;
    return s.getBoundingClientRect().bottom - m.getBoundingClientRect().bottom;
  });
  expect(
    hidden,
    "Save already fits, so this window never reaches the scrolling fallback",
  ).toBeGreaterThan(0);

  // And the fallback carries it back: without `overflow-y-auto` on `main`
  // there is nothing to scroll and Save stays permanently unreachable.
  await save.scrollIntoViewIfNeeded();
  await expect(save, "the column did not scroll, so Save cannot be reached").toBeInViewport();
});
