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

test.describe("parity 960x640 (12-compact)", () => {
  test("frame: header 52, preview header 44, transport 40, timeline 270, status 23", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-12-compact.png" });
    await composite(page, "12-compact.png", "test-results/parity/built-12-compact.png", "vs-12-compact");
    expect((await box(page, "editor-header")).height).toBeCloseTo(52, 0);
    // §1.4: the wordmark is gone at or below 1350px; the mark stays.
    await expect(page.getByTestId("editor-header-wordmark")).toBeHidden();
    await expect(page.getByTestId("editor-header-brand-mark")).toBeVisible();
    expect((await box(page, "preview-toolbar")).height).toBeCloseTo(44, 0);
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
