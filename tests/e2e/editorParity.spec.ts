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
    await page.getByTestId("library-asset-webcam").click({ button: "right" });
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
// controls) leave this row; since Task 20 the mixer opens from the timeline
// footer (ruling T12-1).
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

// Task 13 (screens 02 and 04, concept spec §5): the inspector's frame —
// the 48px heading, the selection card and the 2×3 category grid — and its
// empty state.
test.describe("parity 1600x1000: the inspector frame (screens 02, 04)", () => {
  test("a selected clip: 48px heading, 34px card glyph, a 3-column category grid", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await expect(page.getByTestId("inspector-empty")).toBeVisible();
    await expect(page.getByTestId("inspector-title")).toHaveText("Properties");

    await page.getByTestId("clip-c5").click();
    await page.getByTestId("inspector-tab-layout").click();
    // Let the tabs' colour transition settle before the picture is taken.
    await page.waitForTimeout(250);
    await page.screenshot({ path: "test-results/parity/built-02-inspector.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-inspector.png", "vs-02-inspector");

    const heading = await box(page, "inspector-heading");
    const column = await box(page, "editor-shell-inspector");
    expect(heading.height).toBeCloseTo(48, 0);
    expect(Math.abs(heading.width - (column.width - 1))).toBeLessThanOrEqual(1);
    await expect(page.getByTestId("inspector-title")).toHaveText("Clip properties");
    const glyph = await box(page, "inspector-card-glyph");
    expect(glyph.width).toBeCloseTo(34, 0);
    expect(glyph.height).toBeCloseTo(34, 0);
    await expect(page.getByTestId("inspector-card-name")).toHaveText("Presenter · demo");
    await expect(page.getByTestId("inspector-card-detail")).toHaveText("Webcam · presenter · 32.0s");
    const columns = await page
      .getByTestId("inspector-tablist")
      .evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").length);
    expect(columns).toBe(3);
    // Two rows of three; each tab at least 30px tall.
    const clipTab = await box(page, "inspector-tab-clip");
    const audioTab = await box(page, "inspector-tab-audio");
    expect(clipTab.height).toBeGreaterThanOrEqual(30);
    expect(audioTab.x).toBeCloseTo(clipTab.x, 0);
    expect(audioTab.y).toBeGreaterThan(clipTab.y + clipTab.height);
    await expect(page.getByTestId("inspector-tab-layout")).toHaveClass(/(^|\s)active(\s|$)/);

    await page.getByTestId("inspector-tab-fades").click();
    await page.waitForTimeout(250);
    await page.screenshot({ path: "test-results/parity/built-04-inspector.png" });
    await composite(page, "04-fades.png", "test-results/parity/built-04-inspector.png", "vs-04-inspector");
  });

  // Visual-parity Task 14 (§5 Layout, Clip): screen 02's Layout tab and the
  // Clip tab, in seconds.
  test("the Layout tab: Webcam overlay, position in percent, Size, a 2x2 corner grid", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("clip-c5").click();
    await page.getByTestId("inspector-tab-layout").click();
    await expect(page.getByTestId("inspector-body").locator("h3").first()).toHaveText("Webcam overlay");
    await expect(page.getByTestId("layout-preset-full")).toHaveText("Full frame");
    await expect(page.getByTestId("layout-preset-pip")).toHaveText("Picture-in-picture");
    await expect(page.getByTestId("layout-section-x")).toHaveValue("77.5");
    await expect(page.getByTestId("layout-section-y")).toHaveValue("6");
    await expect(page.getByTestId("layout-section-size-value")).toHaveText("19%");
    const x = await box(page, "layout-section-x");
    const y = await box(page, "layout-section-y");
    expect(y.y).toBeCloseTo(x.y, 0);
    expect(x.height).toBeGreaterThanOrEqual(34);
    const tl = await box(page, "layout-corner-tl");
    const tr = await box(page, "layout-corner-tr");
    const bl = await box(page, "layout-corner-bl");
    expect(tl.height).toBeGreaterThanOrEqual(34);
    expect(tr.y).toBeCloseTo(tl.y, 0);
    expect(bl.x).toBeCloseTo(tl.x, 0);
    expect(bl.y).toBeGreaterThan(tl.y + tl.height);
    // Nothing in the tab is wider than its column.
    const body = await box(page, "inspector-body");
    expect(tr.x + tr.width).toBeLessThanOrEqual(body.x + body.width + 1);
    await expect(page.getByTestId("layout-frame-crop")).not.toHaveAttribute("open", /.*/);
    await expect(page.getByTestId("layout-transform")).not.toHaveAttribute("open", /.*/);
  });

  // Split from the geometry test above (Task 14 fix round 1): one long test
  // ran past the 30 s budget under a loaded full run. Reduced motion stands in
  // for the old fixed wait for the tabs' colour transition.
  test("the Layout disclosures stay open for their clip, and the Clip tab is in seconds", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.getByTestId("clip-c5").click();
    await page.getByTestId("inspector-tab-layout").click();
    // A click on a summary opens it; the tab remembers that for this clip.
    await page.getByTestId("layout-frame-crop").locator("summary").click();
    await page.getByTestId("layout-transform").locator("summary").click();
    await expect(page.getByTestId("layout-section-mirror")).toBeVisible();
    // Ruling T14-1: the exact, non-proportional size lives here.
    await expect(page.getByTestId("layout-section-w")).toHaveValue("19");
    await expect(page.getByTestId("layout-section-h")).toHaveValue("33.78");
    await expect(page.getByTestId("layout-transform-rotate")).toBeVisible();
    await page.getByTestId("layout-transform-rotate").scrollIntoViewIfNeeded();
    await page.getByTestId("editor-shell-inspector").screenshot({ path: "test-results/parity/built-02-layout-disclosures.png" });

    await page.getByTestId("inspector-tab-clip").click();
    await expect(page.getByTestId("inspector-body").locator("h3")).toHaveText(["Placement", "Source range"]);
    await expect(page.getByTestId("inspector-body")).not.toContainText(/\bms\b/);
    await expect(page.getByTestId("inspector-tab-clip")).toHaveClass(/(^|\s)active(\s|$)/);
    await page.screenshot({ path: "test-results/parity/built-02-clip-tab.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-clip-tab.png", "vs-02-clip-tab");
    await page.getByTestId("inspector-tab-layout").click();
    await expect(page.getByTestId("layout-frame-crop")).toHaveAttribute("open", "");
    await expect(page.getByTestId("layout-transform")).toHaveAttribute("open", "");
  });

  // Visual-parity Task 15 (§5 Fades, Audio, Speed, Color): screen 04's
  // Fades tab, and the three tabs no screenshot shows.
  test("the Fades tab: the envelope, fields in seconds, the presets and Between two clips", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("clip-c5").click();
    await page.getByTestId("inspector-tab-fades").click();
    const body = page.getByTestId("inspector-body");
    await expect(body.locator("h3")).toHaveText(["A softer entrance. A cleaner exit.", "Between two clips"]);
    // §5: the 58px well draws the SVG inside its 1px border.
    expect((await box(page, "fades-section-graph")).height).toBeCloseTo(56, 0);
    await expect(page.getByTestId("fades-section-fade-in")).toHaveValue("0.6");
    await expect(page.getByTestId("fades-section-fade-out")).toHaveValue("0.6");
    const fadeIn = await box(page, "fades-section-fade-in");
    const fadeOut = await box(page, "fades-section-fade-out");
    expect(fadeOut.y).toBeCloseTo(fadeIn.y, 0);
    await expect(page.getByTestId("fades-section-preset-500")).toHaveText("Quick · 0.5s");
    expect((await box(page, "fades-section-preset-500")).height).toBeCloseTo(29, 0);
    await expect(page.getByTestId("fades-section-preview")).toHaveText("Preview entrance");
    await expect(body).not.toContainText(/\bms\b/);
    await page.waitForTimeout(250);
    await page.getByTestId("editor-shell-inspector").screenshot({ path: "test-results/parity/built-04-fades-tab.png" });
  });

  test("the Audio, Speed and Color tabs take the concept's sections", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("clip-c5").click();
    const body = page.getByTestId("inspector-body");
    await page.getByTestId("inspector-tab-audio").click();
    await expect(body.locator("h3")).toHaveText(["Clip audio", "Mix"]);
    await expect(page.getByTestId("audio-section-volume-value")).toHaveText("100%");
    await page.getByTestId("editor-shell-inspector").screenshot({ path: "test-results/parity/built-audio-tab.png" });

    await page.getByTestId("inspector-tab-speed").click();
    await expect(body.locator("h3")).toHaveText(["Keep the useful pace"]);
    await expect(page.getByTestId("speed-section-readout-speed")).toHaveText("1×");
    expect(await page.getByTestId("speed-section-readout-speed").evaluate((el) => getComputedStyle(el).fontSize)).toBe(
      "35px",
    );
    await page.getByTestId("editor-shell-inspector").screenshot({ path: "test-results/parity/built-speed-tab.png" });

    await page.getByTestId("inspector-tab-color").click();
    await expect(body.locator("h3")).toHaveText(["A consistent look", "Fine adjustments"]);
    const columns = await page
      .getByTestId("color-preset-original")
      .evaluate((el) => getComputedStyle(el.parentElement as Element).gridTemplateColumns.split(" ").length);
    expect(columns).toBe(3);
    await expect(page.getByTestId("color-section-brightness-value")).toHaveText("100%");
    await page.getByTestId("editor-shell-inspector").screenshot({ path: "test-results/parity/built-color-tab.png" });
  });

  test("the cue state: Teaching properties, its sections, 27px swatches, timing in seconds", async ({ page }) => {
    await openParity(
      page,
      { width: 1600, height: 1000 },
      { invitation: false, workspace: { selection_clip_ids: ["c1"], selected: { type: "effect", id: "fx1" } } },
    );
    await expect(page.getByTestId("inspector-title")).toHaveText("Teaching properties");
    await expect(page.getByTestId("inspector-card-name")).toHaveText("Text");
    await expect(page.getByTestId("inspector-card-detail")).toHaveText("Attached to Open your workspace");
    await expect(page.getByTestId("effect-section").locator("h3")).toHaveText([
      "Instruction", "Color", "Timing within this clip", "Position · % of this video",
    ]);
    const swatch = await box(page, "effect-swatch-ffffff");
    expect(swatch.width).toBeCloseTo(27, 0);
    expect(swatch.height).toBeCloseTo(27, 0);
    await expect(page.getByTestId("effect-swatch-ffffff")).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByTestId("effect-section")).not.toContainText(/\bms\b/);
    await page.getByTestId("editor-shell-inspector").screenshot({ path: "test-results/parity/built-cue-state.png" });
  });

  test("the ✕ hides the inspector and gives its column to the preview", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("inspector-hide").click();
    await expect.poll(() => collapsed(page, "editor-shell-inspector")).toBe(true);
  });
});

// Task 16 (screen 02, concept spec §6.2–6.3; design D12, D14): the timeline
// toolbar, the header row with its sticky label cell, the ruler's ticks and
// the gold chapter markers.
test.describe("parity 1600x1000: the timeline toolbar and ruler (screen 02, §6.2–6.3)", () => {
  test("toolbar 44, ruler row 32, label cell 196; 2 s ticks; the label cell stays put when the lanes scroll", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-timeline-toolbar.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-timeline-toolbar.png", "vs-02-timeline-toolbar");

    expect((await box(page, "timeline-toolbar")).height).toBeCloseTo(44, 0);
    expect((await box(page, "timeline-ruler")).height).toBeCloseTo(32, 0);
    const label = await box(page, "timeline-ruler-label");
    expect(label.width).toBeCloseTo(196, 0);

    // §6.2's order, left to right, with the zoom group pushed right.
    const xs: number[] = [];
    for (const id of ["undo", "redo", "split", "delete", "delete-mode", "marker", "more", "snap", "zoom-out", "zoom-range", "zoom-in", "fit"]) {
      xs.push((await box(page, `timeline-toolbar-${id}`)).x);
    }
    expect([...xs].sort((a, b) => a - b)).toEqual(xs);
    expect((await box(page, "timeline-toolbar-zoom-range")).width).toBeCloseTo(66, 0);
    const fit = await box(page, "timeline-toolbar-fit");
    const bar = await box(page, "timeline-toolbar");
    expect(bar.x + bar.width - (fit.x + fit.width)).toBeLessThanOrEqual(13);

    // 50 px/s at the default zoom: a 2 s step, 100px apart, 9px mono.
    const ticks = page.getByTestId("timeline-ruler-tick");
    await expect(ticks.nth(0)).toHaveText("00:00");
    await expect(ticks.nth(1)).toHaveText("00:02");
    expect((await ticks.nth(1).boundingBox())!.x - (await ticks.nth(0).boundingBox())!.x).toBeCloseTo(100, 0);
    expect(await ticks.nth(1).evaluate((el) => getComputedStyle(el).fontSize)).toBe("9px");

    // D12: scrolled sideways, the label cell keeps its place.
    await page.getByTestId("timeline-scroll").evaluate((el) => (el.scrollLeft = 300));
    await expect.poll(async () => (await box(page, "timeline-ruler-label")).x).toBeCloseTo(label.x, 0);
    await page.getByTestId("timeline-scroll").evaluate((el) => (el.scrollLeft = 0));
  });

  test("a chapter marker seeks; Add track sends the registry's addTrack", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    const marker = page.getByTestId("timeline-marker-m1");
    // Fix round 1: the markers sit beside the ruler's slider, never in it (a
    // slider's children are presentational), so each keeps its role and name.
    await expect(page.locator('[role="slider"] button')).toHaveCount(0);
    await expect(page.getByRole("button", { name: "Go to Create a project" })).toBeVisible();
    const hit = await box(page, "timeline-marker-m1");
    expect(hit.width).toBeCloseTo(24, 0);
    expect(hit.height).toBeCloseTo(24, 0);
    // The concept's gold (#ebc582) on the dark theme.
    expect(await marker.evaluate((el) => getComputedStyle(el).color)).toBe("rgb(235, 197, 130)");
    await marker.click();
    await expect(page.getByTestId("transport-current")).toHaveText("00:09.5");

    await page.getByTestId("timeline-add-track").click();
    await page.getByTestId("timeline-add-track-panel-item-addTrackVideo").click();
    const sent = await page.evaluate(() =>
      (window as unknown as { __calls: { cmd: string; args: { request?: { command?: { kind?: string } } } | null }[] }).__calls
        .filter((c) => c.cmd === "editor_execute")
        .map((c) => c.args?.request?.command),
    );
    expect(sent).toContainEqual(expect.objectContaining({ kind: "addTrack", trackKind: "video", index: 0 }));
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

  // Task 16: the whole toolbar fits the 960px floor, Fit included.
  test("the timeline toolbar fits without overflowing", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    const bar = page.getByTestId("timeline-toolbar");
    expect(await bar.evaluate((el) => el.scrollWidth - el.clientWidth)).toBeLessThanOrEqual(0);
    const fit = await box(page, "timeline-toolbar-fit");
    expect(fit.x + fit.width).toBeLessThanOrEqual(960);
    expect((await box(page, "timeline-toolbar-zoom-range")).width).toBeCloseTo(48, 0);
  });

  // The track label column is 174 at this width (§1.4; Task 17): the
  // ruler's cell and every track header, which stay pinned when scrolled.
  test("the timeline's label column is 174", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    expect((await box(page, "timeline-ruler-label")).width).toBeCloseTo(174, 0);
    const header = await box(page, "track-lane-header-v1");
    expect(header.width).toBeCloseTo(174, 0);
    await page.getByTestId("timeline-scroll").evaluate((el) => (el.scrollLeft = 400));
    await expect.poll(async () => (await box(page, "track-lane-header-v1")).x).toBeCloseTo(header.x, 0);
  });
});

// Task 17 (screen 02, concept spec §6.4; design D12, ruling P4): the track
// rows. The label column is pinned in the one scroll container, so a clip
// scrolled into view by its own focus never takes the headers with it.
const TRACK_IDS = ["v3", "v2", "v1", "a1"];

test.describe("parity 1600x1000: the track rows (screen 02, §6.4)", () => {
  test("the headers stay pinned when a nudged clip's focus scrolls the lanes", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    const left = (await box(page, "timeline-scroll")).x;
    await page.getByTestId("clip-c3").click();
    for (let i = 0; i < 3; i += 1) await page.keyboard.press("ArrowRight");
    // The regression needs a scrolled timeline to mean anything.
    await expect.poll(() => page.getByTestId("timeline-scroll").evaluate((el) => el.scrollLeft)).toBeGreaterThan(0);
    for (const id of TRACK_IDS) {
      expect(Math.abs((await box(page, `track-lane-header-${id}`)).x - left), id).toBeLessThanOrEqual(1);
    }
    expect(Math.abs((await box(page, "timeline-ruler-label")).x - left)).toBeLessThanOrEqual(1);
  });

  test("header anatomy: 196 wide, rows ≈68, a 28px badge, 26px controls, an 11px title", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-track-rows.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-track-rows.png", "vs-02-track-rows");

    const badges: string[] = [];
    for (const id of TRACK_IDS) {
      expect((await box(page, `track-lane-header-${id}`)).width).toBeCloseTo(196, 0);
      expect(Math.abs((await box(page, `track-lane-${id}`)).height - 68)).toBeLessThanOrEqual(3);
      const badge = await box(page, `track-header-${id}-badge`);
      expect([badge.width, badge.height]).toEqual([28, 28]);
      badges.push((await page.getByTestId(`track-header-${id}-badge`).textContent())?.trim() ?? "");
    }
    expect(badges).toEqual(["V3", "V2", "V1", "A1"]);
    for (const control of ["visible", "mute", "solo", "lock"]) {
      const b = await box(page, `track-header-v1-${control}`);
      expect([Math.round(b.width), Math.round(b.height)], control).toEqual([26, 26]);
    }
    const name = page.getByTestId("track-header-v1-name");
    expect(await name.evaluate((el) => [getComputedStyle(el).fontSize, getComputedStyle(el).fontWeight])).toEqual(["11px", "550"]);
    await expect(page.getByTestId("track-header-v1-menu")).toHaveCount(0);
  });

  test("right-click or Shift+F10 on a header opens its track menu; Escape gives focus back", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("track-lane-header-v2").click({ button: "right", position: { x: 150, y: 20 } });
    await expect(page.getByTestId("editor-context-menu-heading")).toHaveText("Detail overlay");
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("editor-context-menu")).toHaveCount(0);

    await page.getByTestId("track-header-a1-badge").focus();
    await page.keyboard.press("Shift+F10");
    await expect(page.getByTestId("editor-context-menu-heading")).toHaveText("Guide cues");
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("track-header-a1-badge")).toBeFocused();

    await page.getByTestId("track-header-v2-badge").click();
    await expect(page.getByTestId("inspector-title")).toHaveText("Track properties");
  });
});

// Task 17, review focus 3 (extreme content): twenty tracks scroll under the
// sticky ruler with the label column pinned, and a 120-character track name
// ellipsizes inside its header.
test.describe("the timeline with twenty tracks and a 120-character track name", () => {
  test("the ruler stays on top, the headers stay left, and the long name never pushes a control out", async ({ page }) => {
    const longName = "T".repeat(120);
    const tracks = Array.from({ length: 20 }, (_, i) => ({
      id: `t${i}`,
      kind: i < 14 ? ("video" as const) : ("audio" as const),
      name: i === 0 ? longName : `Track ${i}`,
      visible: true,
      locked: false,
      muted: false,
      solo: false,
      volume: 1,
    }));
    const openResult = { ...PARITY_OPEN_RESULT, project: { ...PARITY_OPEN_RESULT.project, tracks, clips: [], effects: [], markers: [], transitions: [] } };
    await installTauriStub(page, { openResult, replies: PARITY_REPLIES });
    await page.setViewportSize({ width: 1600, height: 1000 });
    await page.goto("/");
    await page.getByTestId("editor-shell").waitFor();
    await page.waitForTimeout(800);
    await page.getByTestId("guide-invitation-dismiss").click();

    const cell = await box(page, "track-lane-header-t0");
    const name = await box(page, "track-header-t0-name");
    expect(name.x + name.width).toBeLessThanOrEqual(cell.x + cell.width);
    expect(await page.getByTestId("track-header-t0-name").evaluate((el) => el.scrollWidth > el.clientWidth)).toBe(true);
    const lock = await box(page, "track-header-t0-lock");
    expect(lock.x + lock.width).toBeLessThanOrEqual(cell.x + cell.width);
    await expect(page.getByTestId("track-lane-empty-t0")).toHaveText("Drop video here · or add from Media");

    const scroller = await box(page, "timeline-scroll");
    await page.getByTestId("timeline-scroll").evaluate((el) => {
      el.scrollTop = el.scrollHeight;
      el.scrollLeft = 300;
    });
    await expect.poll(async () => (await box(page, "timeline-ruler")).y).toBeCloseTo(scroller.y, 0);
    const last = await box(page, "track-lane-header-t19");
    expect(Math.abs(last.x - scroller.x)).toBeLessThanOrEqual(1);
    expect(last.y + last.height).toBeLessThanOrEqual(scroller.y + scroller.height + 1);
    expect(last.y).toBeGreaterThanOrEqual(scroller.y + 32 - 1);
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
    // Polled, not read once (Task 14 fix round 1): under a loaded run the
    // first read could land before the theme's tokens had been applied. It
    // still fails, naming the tokens, if they never arrive.
    const expected = Object.fromEntries(
      CONCEPT_TOKENS.map(([, token, dark, light]) => [token, theme === "dark" ? dark : light]),
    );
    await expect.poll(() => rootTokens(page), { message: `the concept's ${theme} tokens` }).toEqual(expected);
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

/** Whether the mixer panel's top edge is really painted where its box
 * says — not cut off by the timeline section's clipped overflow, which a
 * bounding box alone would never show. */
function mixerTopIsPainted(page: Page): Promise<boolean> {
  return page.getByTestId("mixer-popover").evaluate((el) => {
    const r = el.getBoundingClientRect();
    return document.elementFromPoint(r.left + r.width / 2, r.top + 6)?.closest('[data-testid="mixer-popover"]') === el;
  });
}

// Task 20 (screen 02, concept spec §7; design D10, ruling T12-1): the
// timeline footer — the live edit hint left, "Audio mixer" with the audio-
// track count right — over the 25 px status bar. The mixer's one trigger
// lives here now, and its panel must clear the timeline's clipped edge.
test.describe("parity 1600x1000: the timeline footer and status bar (screen 02, §7)", () => {
  test("footer 27 over status 25; hint left, Audio mixer with its count right", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-footer.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-footer.png", "vs-02-footer");

    const footer = await box(page, "timeline-footer");
    const status = await box(page, "editor-statusbar");
    expect(footer.height).toBeCloseTo(27, 0);
    expect(status.height).toBeCloseTo(25, 0);
    // The footer is the timeline's last row, right on the status bar.
    expect(footer.y + footer.height).toBeCloseTo(status.y, 0);
    await expect(page.getByTestId("timeline-footer-hint")).toHaveText("Callouts follow their clip.");
    expect(await page.getByTestId("timeline-footer").evaluate((el) => getComputedStyle(el).fontSize)).toBe("10px");

    const audioTracks = PARITY_OPEN_RESULT.project.tracks.filter((t) => t.kind === "audio").length;
    await expect(page.getByTestId("mixer-toggle-count")).toHaveText(String(audioTracks));
    const mixer = await box(page, "mixer-toggle");
    expect(footer.x + footer.width - (mixer.x + mixer.width)).toBeLessThanOrEqual(14);
    // The transport row keeps no mixer of its own.
    await expect(page.getByTestId("transport-bar").locator('[data-action="mixer"]')).toHaveCount(0);

    await expect(page.getByTestId("editor-statusbar-local")).toHaveText("Local only. No media is uploaded.");
    // The sample project opens unsaved; the centre slot runs Save project
    // (its receipt and "All changes saved" are editorKeyboard.spec.ts's).
    const recovery = page.getByTestId("editor-statusbar-recovery");
    await expect(recovery).toHaveText("Unsaved changes are journaled for recovery");
    await expect(recovery).toHaveAttribute("title", "Save project (Ctrl+S)");
    await recovery.click();
    await expect
      .poll(() => page.evaluate(() => (window as unknown as { __invoked: string[] }).__invoked.filter((c) => c === "editor_save_project").length))
      .toBe(1);
    await expect(page.getByTestId("editor-statusbar-products")).toHaveText(/rendered videos? · Workspace & rendered products$/);
  });

  test("Audio mixer opens its whole panel above itself; Escape gives focus back", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("mixer-toggle").click();
    const panel = page.getByTestId("mixer-popover");
    await expect(panel).toBeVisible();
    const p = await box(page, "mixer-popover");
    const toggle = await box(page, "mixer-toggle");
    expect(p.y + p.height).toBeLessThanOrEqual(toggle.y);
    expect(p.y).toBeGreaterThanOrEqual(0);
    expect(await mixerTopIsPainted(page)).toBe(true);
    await page.keyboard.press("Escape");
    await expect(panel).toHaveCount(0);
    await expect(page.getByTestId("mixer-toggle")).toBeFocused();
  });

  // Fix round 1: a tall mixer (twenty tracks) opened in a tall window, then
  // the window shrinks — its first rows must stay reachable.
  test("an open mixer keeps its top in the window when the window shrinks", async ({ page }) => {
    const tracks = Array.from({ length: 20 }, (_, i) => ({
      id: `t${i}`, kind: "audio" as const, name: `Track ${i}`, visible: true, locked: false, muted: false, solo: false, volume: 1,
    }));
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false, project: { tracks, clips: [], effects: [], markers: [], transitions: [], captions: null } });
    await page.getByTestId("mixer-toggle").click();
    await expect(page.getByTestId("mixer-popover")).toBeVisible();
    await page.setViewportSize({ width: 1600, height: 640 });
    await expect.poll(async () => (await box(page, "mixer-popover")).y).toBeGreaterThanOrEqual(0);
    const p = await box(page, "mixer-popover");
    expect(p.y + p.height).toBeLessThanOrEqual((await box(page, "mixer-toggle")).y);
    expect(await mixerTopIsPainted(page)).toBe(true);
  });

  test("the hint follows a real clip drag, and Escape puts it back", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    // c4, alone on v2 and wholly in view (editorClips.spec.ts's own grip).
    const clip = await box(page, "clip-c4");
    const hint = page.getByTestId("timeline-footer-hint");
    await page.mouse.move(clip.x + 40, clip.y + 20);
    await page.mouse.down();
    await page.mouse.move(clip.x + 60, clip.y + 20);
    await page.mouse.move(clip.x + 100, clip.y + 20);
    await expect(hint).toHaveText(/^Starts at \d\d:\d\d\.\d · release to place$/);
    await page.keyboard.press("Escape");
    await expect(hint).toHaveText("Callouts follow their clip.");
    await page.mouse.up();
  });
});

// Ruling T12-1: a before-you-share finding's "Open the mixer" (the View
// menu's Audio mixer… asks the same reveal) opens the footer's panel at the
// drawer breakpoints too.
for (const size of [{ width: 1080, height: 760 }, { width: 860, height: 640 }, { width: 960, height: 640 }]) {
  test(`the mixer opens from its reveal at ${size.width}x${size.height}, inside the window`, async ({ page }) => {
    await openParity(page, size, { invitation: false });
    const footer = await box(page, "timeline-footer");
    expect(footer.height).toBeCloseTo(27, 0);
    const mixer = await box(page, "mixer-toggle");
    expect(mixer.x + mixer.width).toBeLessThanOrEqual(size.width);
    await page.getByTestId("preview-view-menu").click();
    await page.getByTestId("preview-view-panel-item-mixer").click();
    const panel = await box(page, "mixer-popover");
    expect(panel.x).toBeGreaterThanOrEqual(0);
    expect(panel.y).toBeGreaterThanOrEqual(0);
    expect(panel.x + panel.width).toBeLessThanOrEqual(size.width);
    expect(panel.y + panel.height).toBeLessThanOrEqual(mixer.y);
    expect(await mixerTopIsPainted(page)).toBe(true);
  });
}

// Task 21 (screens 07–09, concept spec §9.4–9.6; design D6, D8, D10): the
// share dialogs — Before you share, Save a copy as project file, Render a
// video — in the concept's shared chrome, measured in the built bundle.
const PARITY_FINDINGS = [
  {
    id: "chk-gap-c4",
    severity: "warning",
    code: "gap",
    message: "No clip on Screen recording from 0:12.0 to 0:13.5.",
    target: { kind: "clip", id: "c4" },
    action: "select",
  },
  {
    id: "chk-excludedCaptions-project",
    severity: "info",
    code: "excludedCaptions",
    message: "Captions are turned off, so 2 captions will not appear in the render.",
    target: { kind: "project", id: null },
    action: "openCaptions",
  },
];

/** The open dialog's footer buttons, in order. */
function footerLabels(page: Page): Promise<string[]> {
  return page.getByTestId("dialog-host-content").locator("footer button").allInnerTexts();
}

test.describe("parity 1600x1000: the share dialogs (screens 07–09, §9.4–9.6)", () => {
  test("Before you share: 680 wide, summary box, issue rows, footer of three", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, {
      invitation: false,
      replies: { editor_get_checks: PARITY_FINDINGS },
    });
    await page.getByTestId("editor-header-checks").click();
    await expect(page.getByTestId("checks-dialog")).toBeVisible();
    await page.screenshot({ path: "test-results/parity/built-07-checks.png" });
    await composite(page, "07-checks.png", "test-results/parity/built-07-checks.png", "vs-07-checks");

    expect((await box(page, "dialog-host-content")).width).toBeCloseTo(680, 0);
    await expect(page.getByTestId("checks-summary").locator("b")).toHaveText("0 blockers · 1 review warning");
    // §9.4: the kind chip is at least 48 wide, mono 9px.
    const chip = await box(page, "check-kind-chk-excludedCaptions-project");
    expect(chip.width).toBeGreaterThanOrEqual(48);
    await expect(page.getByTestId("check-kind-chk-excludedCaptions-project")).toHaveText("NOTE");
    await expect(page.getByTestId("check-kind-chk-gap-c4")).toHaveText("REVIEW");
    // The action button is 30 tall, with its chevron.
    expect((await box(page, "check-action-chk-excludedCaptions-project")).height).toBeCloseTo(30, 0);
    await expect(page.getByTestId("check-action-chk-excludedCaptions-project")).toHaveText("Open Captions");
    expect(await footerLabels(page)).toEqual(["Export diagnostics", "Back to edit", "Continue to render"]);
    await expect(page.getByTestId("checks-dialog")).not.toContainText(/download/i);
  });

  test("Save a copy: intro, name, two option cards, the checklist, Keep editing · Save copy", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("editor-header-project-menu").click();
    await page.getByTestId("editor-project-menu-item-saveCopy").click();
    await expect(page.getByTestId("save-project-dialog")).toBeVisible();
    await page.screenshot({ path: "test-results/parity/built-08-save-project.png" });
    await composite(page, "08-save-project.png", "test-results/parity/built-08-save-project.png", "vs-08-save-project");

    expect((await box(page, "dialog-host-content")).width).toBeCloseTo(560, 0);
    await expect(page.getByTestId("save-project-name")).toHaveValue("Create your first project");
    const portable = await box(page, "save-project-option-portable");
    const light = await box(page, "save-project-option-lightweight");
    expect(portable.width).toBeCloseTo(light.width, 0);
    expect(light.y).toBeGreaterThan(portable.y + portable.height);
    // §9.5: the checklist is 2×2.
    const items = page.getByTestId("save-project-checklist").locator("li");
    await expect(items).toHaveCount(4);
    const xs = await items.evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().left)));
    expect(new Set(xs).size).toBe(2);
    expect(await footerLabels(page)).toEqual(["Keep editing", "Save copy"]);
  });

  test("Render a video: range box, parent card, profile, callout, Save project instead · Render video", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("editor-header-render").click();
    await expect(page.getByTestId("render-dialog")).toBeVisible();
    await page.screenshot({ path: "test-results/parity/built-09-render.png" });
    await composite(page, "09-render.png", "test-results/parity/built-09-render.png", "vs-09-render");

    expect((await box(page, "dialog-host-content")).width).toBeCloseTo(560, 0);
    await expect(page.getByTestId("render-dialog-revision")).toHaveText(/^PROJECT · r\d+$/);
    // D6: the destination vault by its NAME.
    await expect(page.getByTestId("render-dialog-destination")).toContainText("Knowledge vault");
    await expect(page.getByTestId("render-dialog")).not.toContainText("vault-e2e");
    // D10: the profile card holds native quality radios, no browser review.
    await expect(page.getByTestId("render-dialog-profile").locator('input[type="radio"]')).toHaveCount(3);
    await expect(page.getByTestId("render-dialog")).not.toContainText(/browser/i);
    await expect(page.getByTestId("render-dialog-originals").locator("b")).toHaveText("A new output, never an overwrite.");
    expect(await footerLabels(page)).toEqual(["Save project instead", "Render video"]);
    // The footer never scrolls away: it sits at the dialog's bottom edge.
    const dialog = await box(page, "dialog-host-content");
    const start = await box(page, "render-dialog-start");
    expect(start.y + start.height).toBeLessThanOrEqual(dialog.y + dialog.height);
  });
});

// Visual-parity Task 22 (concept spec §9.7, screen 05): the webcam dialog,
// opened with the camera OFF — `getUserMedia` is replaced by a counter so
// the test proves nothing asked for the camera (checklist T32's premise).
test.describe("parity 1600x1000: the webcam dialog (screen 05, §9.7)", () => {
  test("960 wide: the 16:9 view, Set up your take, the privacy strip, Enable camera alone", async ({ page }) => {
    await page.addInitScript(() => {
      const w = window as unknown as { __cameraAsks: number };
      w.__cameraAsks = 0;
      if (navigator.mediaDevices) {
        navigator.mediaDevices.getUserMedia = () => {
          w.__cameraAsks += 1;
          return new Promise(() => {});
        };
      }
    });
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("library-webcam").click();
    await expect(page.getByTestId("webcam-empty")).toBeVisible();
    await page.screenshot({ path: "test-results/parity/built-05-webcam.png" });
    await composite(page, "05-webcam.png", "test-results/parity/built-05-webcam.png", "vs-05-webcam");

    expect((await box(page, "dialog-host-content")).width).toBeCloseTo(960, 0);
    const view = await box(page, "webcam-view");
    expect(view.width / view.height).toBeCloseTo(16 / 9, 1);
    expect((await box(page, "webcam-settings")).width).toBeCloseTo(230, 0);
    await expect(page.getByTestId("webcam-empty").locator("b")).toHaveText("Camera is off");
    await expect(page.getByTestId("webcam-settings").locator("h3")).toHaveText("Set up your take");
    const privacy = await box(page, "webcam-privacy");
    expect(privacy.y).toBeGreaterThan(view.y + view.height);
    expect(await footerLabels(page)).toEqual(["Enable camera"]);
    await expect(page.getByTestId("dialog-host-content")).not.toContainText(/demo/i);
    expect(await page.evaluate(() => (window as unknown as { __cameraAsks: number }).__cameraAsks)).toBe(0);
  });

  test("the webcam dialog fits 960x640 with Enable camera on screen", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    await page.getByTestId("library-webcam").click();
    await expect(page.getByTestId("webcam-dialog")).toBeVisible();
    const host = await box(page, "dialog-host-content");
    expect(host.x).toBeGreaterThanOrEqual(0);
    expect(host.y).toBeGreaterThanOrEqual(0);
    expect(host.x + host.width).toBeLessThanOrEqual(960);
    expect(host.y + host.height).toBeLessThanOrEqual(640);
    const enable = await box(page, "webcam-enable");
    expect(enable.y + enable.height).toBeLessThanOrEqual(640);
  });
});

// Visual-parity Task 22 (concept spec §9.8): the mixer's rows are the
// concept's `125px 1fr 52px` grid, with Master output under them.
test("the audio mixer's rows are 125 | range | 52, Master output last", async ({ page }) => {
  await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
  await page.getByTestId("mixer-toggle").click();
  const row = page.getByTestId("mixer-popover").locator('[data-testid^="mixer-track-"]').first();
  const columns = await row.evaluate((el) => getComputedStyle(el).gridTemplateColumns.split(" ").map(parseFloat));
  expect(columns[0]).toBeCloseTo(125, 0);
  expect(columns[2]).toBeCloseTo(52, 0);
  await expect(page.getByTestId("mixer-master-row").locator("b")).toHaveText("Master output");
  await expect(page.getByTestId("mixer-master-readout")).toHaveText(/^\d+%$/);
});

// D16 / the 960×640 floor: each share dialog fits the window, its primary
// action on screen without scrolling the page.
for (const [open, dialog, primary] of [
  ["editor-header-checks", "checks-dialog", "checks-render"],
  ["editor-header-render", "render-dialog", "render-dialog-start"],
] as const) {
  test(`${dialog} fits 960x640 with its primary action on screen`, async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    await page.getByTestId(open).click();
    await expect(page.getByTestId(dialog)).toBeVisible();
    const host = await box(page, "dialog-host-content");
    expect(host.x).toBeGreaterThanOrEqual(0);
    expect(host.y).toBeGreaterThanOrEqual(0);
    expect(host.x + host.width).toBeLessThanOrEqual(960);
    expect(host.y + host.height).toBeLessThanOrEqual(640);
    const button = await box(page, primary);
    expect(button.y + button.height).toBeLessThanOrEqual(640);
  });
}

// Visual-parity Task 23 (concept spec §9.2–9.3, screens 10–11): the coach
// on the Fades lesson, docked left of the inspector, and the learning
// center.
test.describe("parity 1600x1000: the guide (screens 10–11, §9.2–9.3)", () => {
  test("the coach on Fades: 362 wide, left of the inspector, the ring and its label", async ({ page }) => {
    // A dismissed invitation and a walkthrough paused on Fades (13 / 22).
    await openParity(page, { width: 1600, height: 1000 }, {
      replies: {
        editor_get_guide_progress: {
          ...(PARITY_REPLIES.editor_get_guide_progress as object),
          invitationDismissed: true,
          currentStepId: "fades",
          reviewed: ["welcome", "media", "preview", "timeline", "select", "split", "undo", "arrange", "context", "tracks", "webcam", "layout"],
        },
      },
    });
    await page.getByTestId("editor-header-help").click();
    await page.getByTestId("editor-help-menu-item-resume").click();
    const coach = page.getByTestId("guide-coach");
    await expect(coach).toHaveAttribute("data-step-id", "fades");
    await page.waitForTimeout(400);
    await page.screenshot({ path: "test-results/parity/built-10-onboarding.png" });
    await composite(page, "10-onboarding.png", "test-results/parity/built-10-onboarding.png", "vs-10-onboarding");

    await expect(coach).toHaveAttribute("data-placement", "left");
    const card = await box(page, "guide-coach");
    expect(card.width).toBeCloseTo(362, 0);
    expect(card.x + card.width).toBeLessThanOrEqual((await box(page, "editor-shell-inspector")).x);
    await expect(page.getByTestId("guide-coach-count")).toHaveText("13 / 22");
    await expect(page.getByTestId("guide-coach-contents")).toHaveText("Smooth the edges");
    await expect(page.getByTestId("guide-coach-title")).toHaveCSS("font-size", "23px");
    await expect(page.getByTestId("guide-coach-task")).toHaveAttribute("data-voice", "edit");
    await expect(page.getByTestId("guide-coach-storage")).toHaveText("Progress remembered on this PC");
    await expect(page.getByTestId("guide-target-label")).toHaveText("Fade controls");
    expect((await box(page, "guide-coach-progress")).height).toBeCloseTo(3, 0);
  });

  test("the learning center: 870 wide, the hero and its 128px ring, three tabs, two columns of chapters", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.getByTestId("editor-header-help").click();
    await page.getByTestId("editor-help-menu-item-learningCenter").click();
    await expect(page.getByTestId("learning-center")).toBeVisible();
    await page.waitForTimeout(300);
    await page.screenshot({ path: "test-results/parity/built-11-learning-center.png" });
    await composite(page, "11-learning-center.png", "test-results/parity/built-11-learning-center.png", "vs-11-learning-center");

    expect((await box(page, "dialog-host-content")).width).toBeCloseTo(870, 0);
    const ring = await box(page, "learning-progress");
    expect(ring.width).toBeCloseTo(128, 0);
    expect(ring.height).toBeCloseTo(128, 0);
    await expect(page.getByTestId("learning-progress")).toContainText("0%");
    await expect(page.getByTestId("learning-tablist").getByRole("tab")).toHaveText(["Walkthrough", "Quick answers", "Shortcuts"]);
    const orient = await box(page, "learning-card-orient");
    const edit = await box(page, "learning-card-edit");
    const last = await box(page, "learning-card-return");
    expect(Math.abs(orient.y - edit.y)).toBeLessThanOrEqual(1);
    expect(edit.x).toBeGreaterThan(orient.x + orient.width);
    expect(last.width).toBeCloseTo(edit.x + edit.width - orient.x, 0);
    await expect(page.getByTestId("learning-center")).not.toContainText(/this device|download/i);
  });
});
