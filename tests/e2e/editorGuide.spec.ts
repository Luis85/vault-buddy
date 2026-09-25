import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, type Page, test } from "@playwright/test";

import { FIXTURE_VIDEO_URL, installTauriStub } from "./tauriStub";

/**
 * The guided walkthrough (Task 56; F-46, F-49), measured in a real browser
 * against the PRODUCTION bundle. `tests/editorGuideCoach.test.ts` proves the
 * walkthrough's logic in happy-dom, where every box is stubbed; only here is
 * there a layout for the coach to get wrong: a lesson's control inside a
 * closed compact drawer, below the fold of the scroll container, or a card
 * that lands on top of the very control it is pointing at.
 *
 * 960x640 is the editor window's floor (`minWidth`/`minHeight`) — the
 * narrowest real layout, with both side panels as drawers, and below the
 * coach's 1100 px docking width.
 *
 * The project has real clips and tracks: the stub's default project is
 * empty, and an empty timeline has no selected clip, no track menu and no
 * inspector section — the walkthrough would "pass" by explaining instead
 * of highlighting.
 */

const FIXTURE = fileURLToPath(new URL("./fixtures/capture-1920x1080.webm", import.meta.url));

function clip(id: string, start: number, trackId = "v1") {
  return {
    id, asset_id: "capture", track_id: trackId, name: id, start_ms: start, in_ms: start, out_ms: start + 2000,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false,
    x: 0, y: 0, w: 1, h: 1,
  };
}

const OPEN_RESULT = {
  snapshot: {
    sessionId: "ses-guide", projectId: "project-guide", revision: 1, persistedRevision: 1, title: "Guide",
    durationMs: 6000, canUndo: false, canRedo: false, undoLabel: null, redoLabel: null,
  },
  project: {
    schema: "vault-buddy-video-project/3",
    id: "project-guide",
    title: "Guide",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "capture", kind: "video", name: "cap one", duration_ms: 6000 }],
    tracks: [
      { id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      { id: "a1", kind: "audio", name: "Audio", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    ],
    clips: [clip("intro", 0), clip("body", 2000), clip("outro", 4000)],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-e2e", folder: "", dated: false },
  },
  workspace: {},
  missing: [],
  sourceBase: "2026-09-21 0848 Screen Capture",
  recovered: false,
};

const FRESH_PROGRESS = {
  contentRevision: 1, currentStepId: null, reviewed: [], explored: [], invitationDismissed: false,
  active: false, collapsed: false, completed: false, preferences: { dimming: true, motion: "system" },
};

const STEP_IDS = [
  "welcome", "media", "preview", "timeline", "select", "split", "undo", "arrange", "context",
  "tracks", "webcam", "layout", "fades", "audio", "callouts", "captions", "chapters",
  "checks", "save", "render", "products", "help",
];

async function openEditor(page: Page) {
  await installTauriStub(page, {
    openResult: OPEN_RESULT,
    replies: {
      editor_get_guide_progress: FRESH_PROGRESS,
      editor_save_guide_progress: null,
      editor_get_workspace: {},
      editor_save_workspace: null,
      editor_get_checks: [],
      editor_get_products: [],
    },
  });
  await page.route(`**${FIXTURE_VIDEO_URL}`, (route) =>
    route.fulfill({ contentType: "video/webm", body: readFileSync(FIXTURE) }),
  );
  await page.setViewportSize({ width: 960, height: 640 });
  await page.goto("/");
  await expect(page.getByTestId("editor-shell")).toBeAttached();
}

type Box = { x: number; y: number; width: number; height: number };
const overlaps = (a: Box, b: Box) =>
  a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

test("coach resolves every target at 960x640", async ({ page }) => {
  await openEditor(page);
  await page.getByTestId("guide-invitation-start").click();
  const coach = page.getByTestId("guide-coach");

  for (const id of STEP_IDS) {
    await expect(coach).toHaveAttribute("data-step-id", id);
    // The lesson's REAL control was found and has a box on screen — not
    // registered-but-hidden in a closed drawer, not missing.
    await expect(coach, `lesson ${id}`).toHaveAttribute("data-target-state", /^(direct|overflow)$/);
    await expect(coach).toHaveAttribute("data-placement", "docked");

    const ring = page.getByTestId("guide-ring");
    await expect(ring, `lesson ${id}: ring`).toBeInViewport();
    // Measured after the ring has settled on this lesson's control.
    await expect
      .poll(async () => {
        const r = await ring.boundingBox();
        const c = await coach.boundingBox();
        return r !== null && c !== null && !overlaps(r, c);
      }, { message: `lesson ${id}: the card covers the control it points at` })
      .toBe(true);
    const card = (await coach.boundingBox())!;
    expect(card.x, `lesson ${id}: card off the left`).toBeGreaterThanOrEqual(0);
    expect(card.y, `lesson ${id}: card off the top`).toBeGreaterThanOrEqual(0);
    expect(card.x + card.width, `lesson ${id}: card off the right`).toBeLessThanOrEqual(960);
    expect(card.y + card.height, `lesson ${id}: card off the bottom`).toBeLessThanOrEqual(640);
    // Next must stay reachable however small the card was squeezed.
    await expect(page.getByTestId("guide-next")).toBeInViewport();

    await page.getByTestId("guide-next").click();
  }

  await expect(coach).toHaveCount(0);
  const invoked = await page.evaluate(() => (window as unknown as { __invoked: string[] }).__invoked);
  // A23 in the real bundle: reading the whole guide edited nothing and
  // opened no dialog, file or device surface.
  const forbidden = invoked.filter((cmd) =>
    /^(editor_execute|editor_save_project|editor_start_render|editor_import_|editor_export_|editor_webcam_|editor_relink_|editor_publish_)/.test(cmd),
  );
  expect(forbidden).toEqual([]);
});

test("the invitation leaves the editor usable and does not take focus", async ({ page }) => {
  await openEditor(page);
  const invitation = page.getByTestId("guide-invitation");
  await expect(invitation).toBeVisible();
  // Focus is wherever the page left it — never inside the invitation.
  expect(await invitation.evaluate((el) => el.contains(document.activeElement))).toBe(false);
  // The header behind it still answers.
  await page.getByTestId("editor-header-library-toggle").click();
  await expect(page.getByTestId("editor-header-library-toggle")).toHaveAttribute("aria-expanded", "true");
  await expect(invitation).toBeVisible();
});
