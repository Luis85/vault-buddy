import { expect, type Page, test } from "@playwright/test";

import { box, composite, openParity } from "./parity";

/**
 * Visual-parity Task 19 (screen 02, concept spec §6.4, design D11, D12):
 * the Teaching layers row and the Captions row, measured in real Chromium
 * over the parity sample project (`fixtures/parityProject.ts`: six cues in
 * two rows, two captions on the presenter clip). The unit half is
 * `tests/editorTeachingLayers.test.ts`.
 */

const SIZE = { width: 1600, height: 1000 };

/** The commands the editor sent, in order. */
async function sentCommands(page: Page): Promise<unknown[]> {
  return page.evaluate(() =>
    (window as unknown as { __calls: { cmd: string; args: { request?: { command?: unknown } } | null }[] }).__calls
      .filter((c) => c.cmd === "editor_execute")
      .map((c) => c.args?.request?.command),
  );
}

/** What is painted at (`x`, `y`), the playhead made hit-testable for the
 * probe (`editorClips.spec.ts`' own helper). */
async function paintedAt(page: Page, x: number, y: number): Promise<string | null> {
  return page.evaluate(
    ([px, py]) => {
      const line = document.querySelector<HTMLElement>('[data-testid="timeline-playhead"]');
      if (line) line.style.pointerEvents = "auto";
      const hit = document.elementFromPoint(px, py);
      if (line) line.style.pointerEvents = "";
      return hit?.closest("[data-testid]")?.getAttribute("data-testid") ?? null;
    },
    [x, y] as const,
  );
}

test.describe("parity 1600x1000: the Teaching layers and Captions rows (screen 02, §6.4)", () => {
  test("the teaching row sits above V3, 49 tall, with six cues in two rows; captions sit above it", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-teaching-layers.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-teaching-layers.png", "vs-02-teaching-layers");

    const row = await box(page, "teaching-layers-row");
    const v3 = await box(page, "track-lane-v3");
    expect(Math.abs(row.height - 49)).toBeLessThanOrEqual(2);
    expect(row.y + row.height).toBeLessThanOrEqual(v3.y + 1);
    await expect(page.getByTestId("teaching-layers-lane").getByRole("option")).toHaveCount(6);

    const one = await box(page, "cue-fx1");
    const two = await box(page, "cue-fx2");
    expect(Math.round(one.height)).toBe(19);
    expect(one.y - row.y).toBeCloseTo(3, 0);
    expect(two.y - one.y).toBeCloseTo(22, 0);
    const zoom = page.getByTestId("cue-fx5");
    await expect(zoom).toHaveText("1.65× Focus");
    await expect(zoom).toHaveCSS("color", "rgb(235, 197, 130)");

    const captions = await box(page, "captions-row");
    const ruler = await box(page, "timeline-ruler");
    expect(captions.height).toBeCloseTo(35, 0);
    expect(captions.y).toBeCloseTo(ruler.y + ruler.height, 0);
    expect(captions.y + captions.height).toBeCloseTo(row.y, 0);
    await expect(page.getByTestId("captions-row-label")).toHaveText(/Captions\s*2/);
    expect(Math.round((await box(page, "caption-cue-cap1")).height)).toBe(24);
  });

  test("both label cells stay pinned when the lanes scroll, and the playhead slides under them", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false, workspace: { playhead_ms: 8_000 } });
    const scroller = await box(page, "timeline-scroll");
    const label = await box(page, "teaching-layers-label");
    expect(label.width).toBeCloseTo(196, 0);
    // Scrolled 500px, 8 s (596px in) sits 96px into the 196px label column.
    await page.getByTestId("timeline-scroll").evaluate((el) => (el.scrollLeft = 500));
    await expect.poll(async () => (await box(page, "teaching-layers-label")).x).toBeCloseTo(scroller.x, 0);
    expect((await box(page, "captions-row-label")).x).toBeCloseTo(scroller.x, 0);
    const x = (await box(page, "timeline-playhead")).x + 0.5;
    expect(x).toBeLessThan(scroller.x + 196);
    expect(await paintedAt(page, x, label.y + 20)).toMatch(/^teaching-layers-/);
    expect(await paintedAt(page, x, (await box(page, "captions-row-label")).y + 17)).toBe("captions-row-label");
  });

  test("a click shows Teaching properties; Shift+F10 opens the cue menu and Escape gives focus back", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    await page.getByTestId("cue-fx5").click();
    await expect(page.getByTestId("inspector-title")).toHaveText("Teaching properties");
    await expect(page.getByTestId("cue-fx5")).toHaveAttribute("aria-selected", "true");
    await page.keyboard.press("Shift+F10");
    await expect(page.getByTestId("editor-context-menu-heading")).toHaveText(/Zoom/);
    await page.keyboard.press("Escape");
    await expect(page.getByTestId("editor-context-menu")).toHaveCount(0);
    await expect(page.getByTestId("cue-fx5")).toBeFocused();
  });

  test("dragging a cue sends ONE updateEffect on release, in its clip's source time", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    const cue = await box(page, "cue-fx6"); // "Ready to find.": output 24–30 s on c3 (starts 23.5 s)
    const before = (await sentCommands(page)).length;
    await page.mouse.move(cue.x + cue.width / 2, cue.y + 9);
    await page.mouse.down();
    await page.mouse.move(cue.x + cue.width / 2 + 20, cue.y + 9);
    await page.mouse.move(cue.x + cue.width / 2 + 50, cue.y + 9);
    expect((await sentCommands(page)).length).toBe(before);
    await page.mouse.up();
    await expect.poll(async () => (await sentCommands(page)).slice(before)).toEqual([
      { kind: "updateEffect", effectId: "fx6", startMs: 1_500, endMs: 7_500 },
    ]);
  });

  test("a caption click opens the Captions tab on that caption", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    await page.getByTestId("caption-cue-cap2").click();
    await expect(page.getByTestId("captions-library")).toBeVisible();
    await expect(page.getByTestId("caption-row-cap2")).toHaveAttribute("aria-current", "true");
  });
});

test.describe("parity 960x640: the Teaching layers row", () => {
  test("pins its label at the 174px column and keeps its cues", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false });
    expect((await box(page, "teaching-layers-label")).width).toBeCloseTo(174, 0);
    await expect(page.getByTestId("teaching-layers-lane").getByRole("option")).toHaveCount(6);
  });
});
