import { expect, type Page, test } from "@playwright/test";

import { openParity } from "./parity";

/**
 * Two things the final whole-branch review found only a real browser shows
 * (the must-fix deferrals):
 *
 * - The light theme's NATIVE controls — a `<select>`'s popup, a colour
 *   input, the scrollbars — follow `color-scheme`, and `body` and `#app`
 *   pinned it to `dark`, so a person who picked Light in the View menu on a
 *   dark Windows got dark native controls inside a light editor.
 * - A teaching cue's colour swatches in forced colours (a Windows contrast
 *   theme): every fill was repainted to the page colour, five identical
 *   empty circles named by hex code.
 */

const SIZE = { width: 1600, height: 1000 };
/** The parity project's text cue, so the inspector shows its colour row. */
const CUE_SELECTED = { selection_clip_ids: ["c1"], selected: { type: "effect", id: "fx1" } };

const colorSchemeOf = (page: Page, selector: string) =>
  page.locator(selector).first().evaluate((el) => getComputedStyle(el).colorScheme);

for (const theme of ["light", "dark"] as const) {
  test(`the ${theme} theme's native controls are ${theme}, whatever Windows prefers`, async ({ page }) => {
    // The OS says the opposite, so only the editor's own theme can win.
    await openParity(page, SIZE, {
      invitation: false,
      theme,
      osScheme: theme === "light" ? "dark" : "light",
      workspace: CUE_SELECTED,
    });
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    for (const selector of ["html", "body", "#app", '[data-testid="transport-rate"]', '[data-testid="effect-field-color"]']) {
      expect(await colorSchemeOf(page, selector), selector).toBe(theme);
    }
  });
}

test("forced colours keep a cue's colour swatches in their colours, named, the chosen one marked", async ({ page }) => {
  await openParity(page, SIZE, { invitation: false, workspace: CUE_SELECTED });
  await page.emulateMedia({ forcedColors: "active" });
  const swatches = page.locator('[data-testid^="effect-swatch-"]');
  await expect(swatches).toHaveCount(5);
  const read = await swatches.evaluateAll((buttons) =>
    buttons.map((b) => {
      const fill = b.querySelector("i") as HTMLElement;
      const f = getComputedStyle(fill);
      return {
        name: b.getAttribute("aria-label"),
        pressed: b.getAttribute("aria-pressed"),
        adjust: f.forcedColorAdjust,
        fill: f.backgroundColor,
        edge: f.borderTopWidth,
        outline: getComputedStyle(b).outlineStyle,
      };
    }),
  );
  expect(read.map((s) => s.name)).toEqual([
    "Set color gold", "Set color white", "Set color violet", "Set color teal", "Set color pink",
  ]);
  expect(read.map((s) => s.fill)).toEqual([
    "rgb(255, 210, 121)", "rgb(255, 255, 255)", "rgb(172, 147, 241)", "rgb(123, 212, 188)", "rgb(242, 151, 167)",
  ]);
  for (const s of read) {
    expect(s.adjust, `${s.name} is repainted to the theme`).toBe("none");
    expect(parseFloat(s.edge), `${s.name} has no system-colour edge`).toBeGreaterThanOrEqual(1);
    expect(s.outline, `${s.name}: only the chosen colour is marked`).toBe(s.pressed === "true" ? "solid" : "none");
  }
  expect(read.filter((s) => s.pressed === "true")).toHaveLength(1);
});
