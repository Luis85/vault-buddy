import { expect, type Page, test } from "@playwright/test";

import { openParity } from "./parity";

/**
 * Design D16 across two surfaces (final whole-branch review, Important 1):
 * a dialog opened from a menu gives focus back to the menu's TRIGGER when
 * Escape closes it. The menu used to run its item while focus still sat on
 * the menu item about to be detached, so the dialog remembered that item
 * (or `body`) and Escape returned focus nowhere. Driven by keyboard alone,
 * in real Chromium, because the bug lived in the order focus moved between
 * two unmounting surfaces — happy-dom's focus model is not the evidence.
 */

const SIZE = { width: 1600, height: 1000 };

const focusedTestId = (page: Page) =>
  page.evaluate(() => document.activeElement?.getAttribute("data-testid") ?? document.activeElement?.tagName ?? null);

/** Press ArrowDown inside the open menu until `testid` has focus. */
async function arrowTo(page: Page, testid: string, limit = 20): Promise<void> {
  for (let i = 0; i < limit; i++) {
    if ((await focusedTestId(page)) === testid) return;
    await page.keyboard.press("ArrowDown");
  }
  throw new Error(`ArrowDown never reached ${testid}`);
}

/** Focus `trigger`, open its menu with `openKey`, choose `item` with Enter,
 * see a dialog, press Escape, and report where focus landed. */
async function menuDialogEscape(page: Page, trigger: string, openKey: string, item: string): Promise<string | null> {
  await page.getByTestId(trigger).focus();
  await page.keyboard.press(openKey);
  await arrowTo(page, item);
  await page.keyboard.press("Enter");
  const dialog = page.locator('[role="dialog"]');
  await expect(dialog).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  return focusedTestId(page);
}

test.describe("a dialog opened from a menu returns focus to the menu's trigger (D16)", () => {
  test.beforeEach(async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
  });

  test("Project menu → Rename tutorial… → Escape", async ({ page }) => {
    const landed = await menuDialogEscape(page, "editor-header-project-menu", "Enter", "editor-project-menu-item-rename");
    expect(landed).toBe("editor-header-project-menu");
  });

  test("Help → Learning center → Escape", async ({ page }) => {
    const landed = await menuDialogEscape(page, "editor-header-help", "Enter", "editor-help-menu-item-learningCenter");
    expect(landed).toBe("editor-header-help");
  });

  test("track menu (Shift+F10) → Remove track… → Escape", async ({ page }) => {
    const landed = await menuDialogEscape(page, "track-header-v2-badge", "Shift+F10", "editor-context-menu-item-track-remove");
    expect(landed).toBe("track-header-v2-badge");
  });
});
