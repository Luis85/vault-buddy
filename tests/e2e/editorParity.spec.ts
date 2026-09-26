import { expect, type Page, test } from "@playwright/test";

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
  ["--primary", "--color-primary", "#8b6ad4", "#7853b8"],
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
