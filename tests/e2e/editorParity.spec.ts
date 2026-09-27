import { expect, type Page, test } from "@playwright/test";

import { PARITY_OPEN_RESULT, PARITY_REPLIES } from "./fixtures/parityProject";
import { box, composite, openParity, previewTool } from "./parity";
import { installTauriStub } from "./tauriStub";

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
  test("frame: header 56, library 244, inspector 276, status 25", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-workspace.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-workspace.png", "vs-02-workspace");
    expect((await box(page, "editor-header")).height).toBeCloseTo(56, 0);
    expect((await box(page, "editor-shell-library")).width).toBeCloseTo(244, 0);
    expect((await box(page, "editor-shell-inspector")).width).toBeCloseTo(276, 0);
    expect((await box(page, "editor-statusbar")).height).toBeCloseTo(25, 0);
    // §1.3: the splitter row and the default timeline height.
    expect((await box(page, "editor-splitter")).height).toBeCloseTo(8, 0);
    expect((await box(page, "editor-timeline")).height).toBeCloseTo(400, 0);
  });
});

// Task 8 (screens 01–02, concept spec §2): the header, with the welcome
// invitation over it as a fresh vault opens it.
test.describe("parity 1600x1000: the header (screen 01)", () => {
  test("brand, Project menu, title, save state and actions sit where §2 puts them", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 });
    await expect(page.getByTestId("guide-invitation")).toBeVisible();
    await page.screenshot({ path: "test-results/parity/built-01-welcome.png" });
    await composite(page, "01-welcome.png", "test-results/parity/built-01-welcome.png", "vs-01-welcome");

    const header = await box(page, "editor-header");
    expect(header.height).toBeCloseTo(56, 0);
    // §2: the CSS mark is 29x31, 16px in from the left, centred on the row.
    const mark = await box(page, "editor-header-brand-mark");
    expect(mark.width).toBeCloseTo(29, 0);
    expect(mark.height).toBeCloseTo(31, 0);
    expect(mark.x).toBeCloseTo(16, 0);
    expect(Math.abs(mark.y + mark.height / 2 - 28)).toBeLessThanOrEqual(1);
    await expect(page.getByTestId("editor-header-wordmark")).toBeVisible();
    // Brand → Project → title, left to right, 14px apart.
    const brand = await box(page, "editor-header-brand");
    const project = await box(page, "editor-header-project-menu");
    const title = await box(page, "editor-header-title");
    expect(Math.abs(project.x - (brand.x + brand.width) - 14)).toBeLessThanOrEqual(3);
    expect(title.x).toBeGreaterThan(project.x + project.width);
    await expect(page.getByTestId("editor-header-title")).toHaveText("Create your first project");
    // The actions are 34px tall, Render video last, 16px from the right.
    for (const id of ["editor-header-help", "editor-header-checks", "editor-header-save", "editor-header-render"]) {
      expect((await box(page, id)).height, id).toBeCloseTo(34, 0);
    }
    const render = await box(page, "editor-header-render");
    expect(Math.abs(render.x + render.width - (1600 - 16))).toBeLessThanOrEqual(1);
    // The sample project carries unsaved edits, as screen 01's does ("Save
    // project needed", D10's "Unsaved changes") — a gold dot.
    await expect(page.getByTestId("editor-header-save-state")).toHaveText("Unsaved changes");
    const dot = page.getByTestId("editor-header-save-dot");
    expect(await dot.evaluate((el) => getComputedStyle(el).backgroundColor)).toBe("rgb(235, 197, 130)");
    await expect(page.getByTestId("editor-header")).not.toContainText("vault-e2e");
  });

  test("the Project menu opens under its trigger with the native items", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("editor-header-project-menu").click();
    const menu = page.getByTestId("editor-project-menu");
    await expect(menu).toBeVisible();
    await expect(menu.locator('[role="menuitem"]')).toHaveText([
      "Open project…",
      "Open a project file…",
      "Rename tutorial…",
      "Workspace & rendered products",
      "Save a copy as project file…",
      "Discard project…",
    ]);
    const trigger = await box(page, "editor-header-project-menu");
    const panel = await box(page, "editor-project-menu");
    expect(panel.width).toBeCloseTo(282, 0);
    expect(Math.abs(panel.x - trigger.x)).toBeLessThanOrEqual(1);
    expect(panel.y).toBeGreaterThanOrEqual(trigger.y + trigger.height);
  });
});

// Task 9 (screen 02, concept spec §3.1–3.2): the library tabs and the
// Media tab's actions row, search, heading and asset rows.
test.describe("parity 1600x1000: the media library (screen 02)", () => {
  test("tabs are 48px, actions are 40px, thumbnails are 58x40, and the heading reads SOURCE MEDIA · 5 assets", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });

    expect((await box(page, "library-tablist")).height).toBeCloseTo(48, 0);
    expect((await box(page, "library-import")).height).toBeCloseTo(40, 0);
    expect((await box(page, "library-webcam")).height).toBeCloseTo(40, 0);

    const thumb = await box(page, "library-asset-capture-thumb");
    expect(thumb.width).toBeCloseTo(58, 0);
    expect(thumb.height).toBeCloseTo(40, 0);

    await expect(page.getByTestId("library-body")).toContainText("SOURCE MEDIA");
    await expect(page.getByTestId("library-body")).toContainText("5 assets");
  });

  test("right-clicking a row opens the asset menu, headed by its own name", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("library-asset-presenter").click({ button: "right" });
    await expect(page.getByTestId("editor-context-menu")).toBeVisible();
    await expect(page.getByTestId("editor-context-menu-heading")).toHaveText("Presenter · demo");
  });
});

// Task 9, review focus 3 (extreme content): a 120-character asset name must
// never widen the library column — measured here, since happy-dom's own
// unit suite structurally cannot (AGENTS.md's Testing conventions).
test.describe("the media library with a 120-character asset name", () => {
  test("the row stays inside the library column; the thumbnail and the trailing button keep their fixed size", async ({ page }) => {
    const longName = `${"A".repeat(120)}.mp4`;
    const openResult = {
      ...PARITY_OPEN_RESULT,
      project: {
        ...PARITY_OPEN_RESULT.project,
        assets: [
          ...PARITY_OPEN_RESULT.project.assets,
          { id: "long", kind: "video" as const, name: longName, duration_ms: 4_000 },
        ],
      },
    };
    await installTauriStub(page, { openResult, replies: PARITY_REPLIES });
    await page.setViewportSize({ width: 1600, height: 1000 });
    await page.goto("/");
    await page.getByTestId("editor-shell").waitFor();
    await page.waitForTimeout(800);
    await page.getByTestId("guide-invitation-dismiss").click();

    const library = await box(page, "editor-shell-library");
    const row = await box(page, "library-asset-long");
    expect(row.width).toBeLessThanOrEqual(library.width + 1);
    const thumb = await box(page, "library-asset-long-thumb");
    expect(thumb.width).toBeCloseTo(58, 0);
    const addBtn = await box(page, "library-asset-long-add");
    expect(addBtn.width).toBeCloseTo(28, 0);
  });
});

// Task 10 (screen 06, concept spec §3.4): the Captions tab with the
// presenter clip selected — the heading and count, the attached-source box,
// the two actions, the appearance disclosure and the caption cards.
test.describe("parity 1600x1000: the captions library (screen 06)", () => {
  test("EVERY WORD, ACCESSIBLE, attached to the presenter clip, with its caption cards", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("library-tab-captions").click();
    await page.getByTestId("clip-c5").click();
    await expect(page.getByTestId("caption-source")).toHaveText("Attached to Presenter · demo");
    await page.screenshot({ path: "test-results/parity/built-06-captions.png" });
    await composite(page, "06-captions.png", "test-results/parity/built-06-captions.png", "vs-06-captions");

    await expect(page.getByTestId("captions-heading")).toHaveText("EVERY WORD, ACCESSIBLE");
    await expect(page.getByTestId("captions-pill")).toHaveText("2");
    // §3.4: the actions are full-width 32px buttons, left-aligned.
    const library = await box(page, "editor-shell-library");
    for (const id of ["caption-add", "caption-import"]) {
      const b = await box(page, id);
      expect(b.height, id).toBeCloseTo(32, 0);
      expect(b.width, id).toBeGreaterThan(library.width - 40);
    }
    await expect(page.getByTestId("caption-settings").locator("summary")).toHaveText("Caption appearance");
    await expect(page.getByTestId("caption-time-cap1")).toHaveCSS("color", "rgb(235, 197, 130)");
    await expect(page.getByTestId("caption-export-srt")).toHaveText("Export timeline SRT");
    // One scroller (fix round 1): the tab scrolls, the windowed list inside
    // it does not scroll on its own.
    expect(await page.getByTestId("caption-list").evaluate((el) => getComputedStyle(el).overflowY)).toBe("visible");
    expect(await page.getByTestId("captions-library").evaluate((el) => getComputedStyle(el).overflowY)).toBe("auto");
  });

  // A scrolling flex column shrank the intro button (min-height 32 from the
  // base styles) and drew its second line outside it — measured, since
  // happy-dom has no layout.
  test("Titles: the intro button and the template cards hold their whole content", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("library-tab-titles").click();
    const intro = await box(page, "titles-insert-intro");
    const line = await page.getByTestId("titles-insert-intro").locator("small").boundingBox();
    expect(line).not.toBeNull();
    expect(line!.y + line!.height).toBeLessThanOrEqual(intro.y + intro.height);
    const canvas = await box(page, "titles-canvas-intro");
    expect(Math.abs(canvas.width / canvas.height - 16 / 9)).toBeLessThan(0.02);
  });

  test("the Project section opens from the status bar and goes back to Media", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("editor-statusbar-products").click();
    await expect(page.getByTestId("library-project-section")).toBeVisible();
    await expect(page.getByTestId("project-section-heading")).toHaveText("YOUR WORKSPACE");
    await expect(page.getByTestId("project-section-pill")).toHaveText("r3");
    await expect(page.getByTestId("product-card-prod1")).toBeVisible();
    await page.getByTestId("library-project-back").click();
    await expect(page.getByTestId("media-library")).toBeVisible();
  });
});

// Task 5 (screen 03, concept spec §8): the clip context menu through the
// one MenuPanel.
test.describe("parity 1600x1000: the clip context menu (screen 03)", () => {
  test("right-clicking c2 opens its 282px menu with the concept's items", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("clip-c2").click({ button: "right" });
    const menu = page.getByTestId("editor-context-menu");
    await expect(menu).toBeVisible();
    await page.screenshot({ path: "test-results/parity/built-03-context-menu.png" });
    await composite(page, "03-context-menu.png", "test-results/parity/built-03-context-menu.png", "vs-03-context-menu");

    expect(Math.abs((await box(page, "editor-context-menu")).width - 282)).toBeLessThanOrEqual(1);
    await expect(page.getByTestId("editor-context-menu-heading")).toHaveText("Create a project");
    const items = menu.locator('[role^="menuitem"]');
    await expect(items.first()).toHaveText("Go to this clip");
    await expect(menu.locator(".danger", { hasText: "Delete · leave gap" })).toHaveCount(1);
    const count = await items.count();
    expect(count).toBeGreaterThan(10);
    for (let i = 0; i < count; i += 1) await expect(items.nth(i).locator("svg").first()).toBeAttached();
    // The panel stays fully inside the window (clamped 8px in).
    const b = await box(page, "editor-context-menu");
    expect(b.x).toBeGreaterThanOrEqual(8);
    expect(b.y + b.height).toBeLessThanOrEqual(1000 - 8 + 1);
  });
});

// Review focus 4: the menu without a mouse, in a real browser.
test.describe("the context menu from the keyboard", () => {
  test("Shift+F10 opens it; → and Escape walk a submenu; Escape returns focus to the clip", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    const clip = page.getByTestId("clip-c2");
    await clip.focus();
    await page.keyboard.press("Shift+F10");
    await expect(page.getByTestId("editor-context-menu-item-goTo")).toBeFocused();

    const speed = page.getByTestId("editor-context-menu-item-speed");
    for (let i = 0; i < 20 && !(await speed.evaluate((el) => el === document.activeElement)); i += 1) {
      await page.keyboard.press("ArrowDown");
    }
    await page.keyboard.press("ArrowRight");
    await expect(page.getByTestId("editor-context-menu-submenu")).toBeVisible();
    await expect(page.getByTestId("editor-context-menu-item-speed-0.25")).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(page.getByTestId("editor-context-menu-submenu")).toHaveCount(0);
    await expect(speed).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(page.getByTestId("editor-context-menu")).toHaveCount(0);
    await expect(clip).toBeFocused();
  });
});

// Task 11 (screen 02, concept spec §4.1–4.2, §11; design D15): the preview
// header — panel toggle, Preview and the ratio button left, the tool strip
// centred, Review / View / properties right — and the stage around the
// canvas; a click on the picture selects the clip under it.
test.describe("parity 1600x1000: the preview header and stage (screen 02)", () => {
  test("48px header with the tool strip centred; the canvas is 692x389 at (438,118)", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-preview.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-preview.png", "vs-02-preview");

    const header = await box(page, "preview-header");
    expect(header.height).toBeCloseTo(48, 0);
    expect(header.x).toBeCloseTo(244, 0);
    expect(header.width).toBeCloseTo(1080, 0);
    // Left: the toggle 12px in, then "Preview", then the mono ratio.
    const toggle = await box(page, "preview-library-toggle");
    expect(toggle.x - header.x).toBeCloseTo(12, 0);
    await expect(page.getByTestId("preview-heading")).toHaveText("Preview");
    await expect(page.getByTestId("preview-ratio")).toHaveText("16:9");
    const ratio = await box(page, "preview-ratio");
    // Right: the properties toggle ends 12px from the header's right edge.
    const props = await box(page, "preview-properties-toggle");
    expect(Math.abs(props.x + props.width - (header.x + header.width - 12))).toBeLessThanOrEqual(1);
    const review = await box(page, "preview-review");
    // The strip sits centred in the room between the two groups.
    const strip = await box(page, "preview-toolstrip");
    const room = (ratio.x + ratio.width + review.x) / 2;
    expect(Math.abs(strip.x + strip.width / 2 - room)).toBeLessThanOrEqual(2);
    for (const id of ["preview-tool-text", "preview-tool-arrow", "preview-tool-highlight", "preview-tool-zoom", "preview-more-tools"]) {
      expect((await box(page, id)).height, id).toBeCloseTo(32, 0);
    }
    await expect(page.getByTestId("preview-more-tools")).toHaveText("More tools");

    // §4.2: 14px/20px of stage around the fitted canvas.
    const canvas = await box(page, "preview-canvas-frame");
    expect(Math.abs(canvas.width - 692)).toBeLessThanOrEqual(2);
    expect(Math.abs(canvas.height - 389)).toBeLessThanOrEqual(2);
    expect(Math.abs(canvas.x - 438)).toBeLessThanOrEqual(2);
    expect(Math.abs(canvas.y - 118)).toBeLessThanOrEqual(2);
  });

  test("the View menu opens under its trigger with the view items", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("preview-view-menu").click();
    const menu = page.getByTestId("preview-view-panel");
    await expect(menu).toBeVisible();
    await expect(menu.locator('[role^="menuitem"]')).toHaveText([
      "Show media library",
      "Show properties",
      "Focus preview",
      "Reset panel layout",
      "Light theme",
      "Audio mixer…",
      "Keyboard shortcuts & help…",
    ]);
    expect((await box(page, "preview-view-panel")).width).toBeCloseTo(282, 0);
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(page.getByTestId("preview-view-menu")).toBeFocused();
  });

  test("Light theme from the View menu turns the editor light", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("preview-view-menu").click();
    await page.getByTestId("preview-view-panel-item-lightTheme").click();
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  });

  test("clicking the picture selects the clip under it; the empty stage clears it (D15)", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    const canvas = await box(page, "preview-canvas-frame");
    // At 00:00 only c1, the screen recording, is on screen.
    await page.mouse.click(canvas.x + canvas.width / 2, canvas.y + canvas.height / 2);
    await expect(page.getByTestId("clip-c1")).toHaveAttribute("aria-selected", "true");
    // The stage beside the canvas is no picture.
    await page.mouse.click(canvas.x - 60, canvas.y + canvas.height / 2);
    await expect(page.getByTestId("clip-c1")).toHaveAttribute("aria-selected", "false");
  });
});

// Task 12 (screen 02, concept spec §4.3; design D10): the transport row —
// monitor mute, peak meter and rate left; seek/Play/timecode centred; the
// D10 canvas badge right. "Audio mixer" and "Sound" (Task 27's own labelled
// controls) leave this row; the mixer's own icon-only trigger stays in it
// only until Task 20's timeline footer gives it a real home (TransportBar's
// own module doc says why).
test.describe("parity 1600x1000: the transport (screen 02, §4.3)", () => {
  test("46px row; left mute/meter/rate, centre seek/Play/timecode, right the D10 badge", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-transport.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-transport.png", "vs-02-transport");

    const bar = await box(page, "transport-bar");
    expect(bar.height).toBeCloseTo(46, 0);

    // Left cluster: the mute icon-button, the 48x8 peak meter, the rate select.
    const mute = await box(page, "transport-mute");
    const meter = await box(page, "transport-peak");
    expect(meter.width).toBeCloseTo(48, 0);
    expect(meter.height).toBeCloseTo(8, 0);
    expect(meter.x).toBeGreaterThan(mute.x + mute.width);

    // Centre: skipBack, the 34px round Play, skipForward, then the timecode.
    const start = await box(page, "transport-start");
    const play = await box(page, "transport-play");
    const end = await box(page, "transport-end");
    expect(play.width).toBeCloseTo(34, 0);
    expect(play.height).toBeCloseTo(34, 0);
    expect(start.x).toBeLessThan(play.x);
    expect(end.x).toBeGreaterThan(play.x + play.width);
    await expect(page.getByTestId("transport-current")).toHaveText("00:00.0");
    // The sample project's own duration (fixtures/parityProject.ts's `C3_END`).
    await expect(page.getByTestId("transport-total")).toHaveText("00:33.5");

    // Right: the D10 badge, from the project's own canvas.
    await expect(page.getByTestId("transport-badge")).toHaveText("1280 × 720 · 30 fps · PREVIEW");
  });

  test("every control has an effect: seek, mute, rate and Play all change real state", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });

    await page.getByTestId("transport-end").click();
    await expect(page.getByTestId("transport-current")).toHaveText("00:33.5");
    await page.getByTestId("transport-start").click();
    await expect(page.getByTestId("transport-current")).toHaveText("00:00.0");

    const mute = page.getByTestId("transport-mute");
    await expect(mute).toHaveAttribute("aria-pressed", "false");
    await mute.click();
    await expect(mute).toHaveAttribute("aria-pressed", "true");

    const rate = page.getByTestId("transport-rate");
    await expect(rate).toHaveValue("1");
    await rate.selectOption("1.5");
    await expect(rate).toHaveValue("1.5");

    const play = page.getByTestId("transport-play");
    await expect(play).toHaveAttribute("aria-label", "Play");
    await play.click();
    await expect(play).toHaveAttribute("aria-label", "Pause");
  });

  // §1.4 "≤620w": the badge leaves first, before anything else in the row.
  test("the D10 badge hides below the concept's 620px break", async ({ page }) => {
    await openParity(page, { width: 700, height: 1000 }, { invitation: false });
    await expect(page.getByTestId("transport-badge")).toBeVisible();
    await page.setViewportSize({ width: 600, height: 1000 });
    await expect(page.getByTestId("transport-badge")).toHaveCount(0);
    // The rest of the row is still there.
    await expect(page.getByTestId("transport-play")).toBeVisible();
  });
});

test.describe("parity 960x640 (12-compact)", () => {
  test("frame: header 52, preview header 44, transport 40, timeline 270, status 23", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-12-compact.png" });
    await composite(page, "12-compact.png", "test-results/parity/built-12-compact.png", "vs-12-compact");
    expect((await box(page, "editor-header")).height).toBeCloseTo(52, 0);
    // §1.4: the wordmark is gone at or below 1350px; the mark stays.
    await expect(page.getByTestId("editor-header-wordmark")).toBeHidden();
    await expect(page.getByTestId("editor-header-brand-mark")).toBeVisible();
    expect((await box(page, "preview-header")).height).toBeCloseTo(44, 0);
    expect((await box(page, "transport-bar")).height).toBeCloseTo(40, 0);
    expect(Math.abs((await box(page, "editor-timeline")).height - 270)).toBeLessThanOrEqual(2);
    expect((await box(page, "editor-statusbar")).height).toBeCloseTo(23, 0);
    // §1.4: the library is still a 232px column here; the inspector is a
    // closed drawer.
    expect((await box(page, "editor-shell-library")).width).toBeCloseTo(232, 0);
    await expect(page.getByTestId("editor-shell-inspector")).toBeHidden();
  });

  // The track label column is 174 at this width (§1.4); Task 17 builds it.
  test.fixme("the timeline's label column is 174 (Task 17)", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    expect((await box(page, "timeline-label-column")).width).toBeCloseTo(174, 0);
  });
});

async function collapsed(page: Page, testId: string): Promise<boolean> {
  const b = await page.getByTestId(testId).boundingBox();
  return b === null || b.width === 0;
}

test.describe("D5: the panel toggles work at full width (1600x1000)", () => {
  test("Focus preview collapses both panels and gives their room to the preview", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    const before = (await box(page, "editor-shell-preview")).width;

    await previewTool(page, "focusPreview");

    await expect.poll(() => collapsed(page, "editor-shell-library")).toBe(true);
    await expect.poll(() => collapsed(page, "editor-shell-inspector")).toBe(true);
    expect((await box(page, "editor-shell-preview")).width - before).toBeGreaterThanOrEqual(500);
  });

  test("the library toggle alone gives the preview the library's 244px", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    const before = (await box(page, "editor-shell-preview")).width;

    await previewTool(page, "toggleLibrary");

    await expect.poll(() => collapsed(page, "editor-shell-library")).toBe(true);
    await expect(page.getByTestId("editor-shell-inspector")).toBeVisible();
    expect(Math.abs((await box(page, "editor-shell-preview")).width - before - 244)).toBeLessThanOrEqual(1);
  });
});

/** Concept §1.1's surface/brand roles, as `--color-*` root tokens: the
 * concept's own name, the token this app spells it as, and its dark and
 * light hex. Tasks 8/21 restyle the header and the primary button on top
 * of these, so the tokens are what this checks (controller ruling P1). */
const CONCEPT_TOKENS: [role: string, token: string, dark: string, light: string][] = [
  ["--bg", "--color-app", "#18191e", "#f3f3f7"],
  ["--panel", "--color-panel", "#202127", "#fff"],
  ["--raised", "--color-raised", "#292a33", "#f1f0f5"],
  ["--stage", "--color-stage", "#131419", "#e8e7ef"],
  ["--line", "--color-line", "#353640", "#dcdce5"],
  ["--hover", "--color-hover", "#30303c", "#eeebf5"],
  // Dark: the concept's #8b6ad4 darkened (every channel x0.945) so white
  // 11px/600 labels reach 4.54:1 (visual-parity Task 8, ruling T3-1).
  ["--primary", "--color-primary", "#8364c8", "#7853b8"],
  ["--accent", "--color-accent", "#b6a2f5", "#7250ad"],
  ["--accent-bg", "--color-accent-bg", "#393049", "#ece5f8"],
  ["--accent-ink", "--color-accent-ink", "#dacdff", "#603696"],
  ["--ring", "--color-ring", "#d4c1ff", "#7853b8"],
  ["--guide-edge", "--color-guide-edge", "#ac8feb", "#7952ba"],
];

async function rootTokens(page: Page): Promise<Record<string, string>> {
  const names = CONCEPT_TOKENS.map(([, token]) => token);
  return page.evaluate((list) => {
    const style = getComputedStyle(document.documentElement);
    return Object.fromEntries(list.map((n) => [n, style.getPropertyValue(n).trim().toLowerCase()]));
  }, names);
}

for (const theme of ["dark", "light"] as const) {
  test(`tokens: the concept's ${theme} palette on the editor's root`, async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { theme, invitation: false });
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const tokens = await rootTokens(page);
    for (const [role, token, dark, light] of CONCEPT_TOKENS) {
      expect(tokens[token], `${role} (${token}) in ${theme}`).toBe(theme === "dark" ? dark : light);
    }
  });
}

test("type and frame: the editor opens dark at 12px Segoe UI on --bg", async ({ page }) => {
  // No theme is saved: the window opens dark even though the OS asks for
  // light (design D1).
  await installDarkByDefault(page);
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  const root = page.locator(".vb-editor");
  await expect(root).toHaveCount(1);
  const style = await root.evaluate((el) => {
    const s = getComputedStyle(el);
    return { fontSize: s.fontSize, lineHeight: s.lineHeight, fontFamily: s.fontFamily, background: s.backgroundColor };
  });
  expect(style.fontSize).toBe("12px");
  expect(style.lineHeight).toBe("18px");
  expect(style.fontFamily).toMatch(/^"Segoe UI"/);
  expect(style.background).toBe("rgb(24, 25, 30)");
});

/** `openParity` with NO saved theme and an OS that prefers light — the
 * case D1 is about. */
async function installDarkByDefault(page: Page): Promise<void> {
  await openParity(page, { width: 1600, height: 1000 }, { invitation: false, theme: null, osScheme: "light" });
}
