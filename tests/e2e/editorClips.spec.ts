import { expect, type Page, test } from "@playwright/test";

import { box, composite, openParity } from "./parity";

/**
 * Visual-parity Task 18 (screen 04, concept spec §6.3, §6.5): a clip's
 * anatomy, the lanes' "Close gap" hints, the playhead and the snap guide,
 * measured in real Chromium over the parity sample project
 * (`fixtures/parityProject.ts`). The class-level checks and the 3px clip
 * are `tests/editorClipAnatomy.test.ts`'.
 */

const SIZE = { width: 1600, height: 1000 };

/** The `data-testid` of the element painted at (`x`, `y`) — the playhead
 * and its head take no pointer events, so for the length of this probe
 * they do, and `elementFromPoint` then answers with whatever is really on
 * top there (hit-testing follows paint order). */
async function paintedAt(page: Page, x: number, y: number): Promise<string | null> {
  return page.evaluate(
    ([px, py]) => {
      const probes = Array.from(
        document.querySelectorAll<HTMLElement>('[data-testid="timeline-playhead"], [data-testid="timeline-playhead-head"]'),
      );
      for (const el of probes) el.style.pointerEvents = "auto";
      const hit = document.elementFromPoint(px, py);
      for (const el of probes) el.style.pointerEvents = "";
      return hit?.closest("[data-testid]")?.getAttribute("data-testid") ?? null;
    },
    [x, y] as const,
  );
}

test.describe("parity 1600x1000: the clips (screen 04, §6.5)", () => {
  test("a selected clip: 47px at top 5 of its row, the accent outline, the pill, the fade handles", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false, workspace: { playhead_ms: 13_400, selection_clip_ids: ["c5"] } });
    await page.screenshot({ path: "test-results/parity/built-04-clips.png" });
    await composite(page, "04-fades.png", "test-results/parity/built-04-clips.png", "vs-04-clips");

    const row = await box(page, "track-lane-v3");
    const body = await box(page, "clip-c5");
    expect(body.height).toBeCloseTo(47, 0);
    expect(body.y - row.y).toBeCloseTo(5, 0);
    const outline = await page.getByTestId("clip-c5").evaluate((el) => {
      const s = getComputedStyle(el);
      return [s.outlineStyle, s.outlineWidth, s.outlineColor, s.borderRadius];
    });
    // The concept's accent (#b6a2f5) on the dark theme.
    expect(outline).toEqual(["solid", "2px", "rgb(182, 162, 245)", "5px"]);

    await expect(page.getByTestId("clip-c5-duration")).toHaveText("32.0s");
    await expect(page.getByTestId("clip-c1-duration")).toHaveText("9.5s");
    const knee = await box(page, "clip-c5-fade-in-handle");
    expect([Math.round(knee.width), Math.round(knee.height)]).toEqual([13, 13]);
    // On the TOP edge: `top:-4px` from inside the 1px border, as the concept.
    expect(knee.y - body.y).toBeCloseTo(-3, 0);
    await expect(page.getByTestId("clip-c5-fade-in-handle")).toHaveCSS("opacity", "1");
    // An unselected clip keeps them hidden until hovered.
    await expect(page.getByTestId("clip-c4-fade-in-handle")).toHaveCSS("opacity", "0");
    await page.getByTestId("clip-c4").hover();
    await expect(page.getByTestId("clip-c4-fade-in-handle")).toHaveCSS("opacity", "1");
    const bar = await box(page, "clip-c4-trim-start-bar");
    expect(Math.round(bar.width)).toBe(2);
  });

  test("the filmstrip repeats one frame; the audio clip draws bars and names itself at the top", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    const film = page.getByTestId("clip-c1-film");
    await expect(film).toHaveCSS("opacity", "0.32");
    expect(await film.evaluate((el) => getComputedStyle(el).backgroundImage)).toMatch(/^url\(/);
    expect(await film.evaluate((el) => getComputedStyle(el).backgroundRepeat)).toBe("repeat-x");

    const name = await box(page, "clip-c1-name");
    const c1 = await box(page, "clip-c1");
    expect(c1.y + c1.height - (name.y + name.height)).toBeLessThanOrEqual(8);
    expect(await page.getByTestId("clip-c1-name").evaluate((el) => getComputedStyle(el).fontSize)).toBe("10px");

    const audioName = await box(page, "clip-c6-name");
    const c6 = await box(page, "clip-c6");
    expect(audioName.y - c6.y).toBeLessThanOrEqual(5);
    const bars = await page.getByTestId("clip-c6-waveform").locator("path").getAttribute("d");
    const count = (bars ?? "").split("M").length - 1;
    expect(count).toBeGreaterThan(5);
    expect(count).toBeLessThanOrEqual(130);
  });

  test("a gap hint shows on lane hover and closes its gap through moveClips", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    const hint = page.getByTestId("lane-gap-v2-0");
    await expect(hint).toHaveCSS("opacity", "0.16");
    await expect(hint).toHaveText("Close gap");
    await page.getByTestId("track-lane-body-v2").hover({ position: { x: 20, y: 5 } });
    await expect(hint).toHaveCSS("opacity", "1");
    await hint.click();
    const sent = await page.evaluate(() =>
      (window as unknown as { __calls: { cmd: string; args: { request?: { command?: unknown } } | null }[] }).__calls
        .filter((c) => c.cmd === "editor_execute")
        .map((c) => c.args?.request?.command),
    );
    expect(sent).toContainEqual({ kind: "moveClips", clipIds: ["c4"], deltaMs: -11_000, trackId: null });
  });

  test("a snapping drag shows a dashed gold guide at the target; Escape takes it away", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false });
    const c4 = await box(page, "clip-c4");
    const content = await box(page, "timeline-content");
    // c4 starts at 11 s; 73px left is 9.54 s, 2px from c2's start (9.5 s).
    await page.mouse.move(c4.x + 40, c4.y + 20);
    await page.mouse.down();
    await page.mouse.move(c4.x + 20, c4.y + 20);
    await page.mouse.move(c4.x - 33, c4.y + 20);
    const guide = page.getByTestId("timeline-snap-guide");
    await expect(guide).toBeVisible();
    const g = await box(page, "timeline-snap-guide");
    expect(g.x - content.x).toBeCloseTo(196 + 475, 0);
    expect(await guide.evaluate((el) => [getComputedStyle(el).borderLeftStyle, getComputedStyle(el).borderLeftColor])).toEqual([
      "dashed",
      "rgb(235, 197, 130)",
    ]);
    await page.keyboard.press("Escape");
    await expect(guide).toHaveCount(0);
    await page.mouse.up();
  });

  test("the playhead starts at the ruler's foot, paints over clips, and slides under the pinned label column", async ({ page }) => {
    await openParity(page, SIZE, { invitation: false, workspace: { playhead_ms: 8_000 } });
    const ruler = await box(page, "timeline-ruler");
    const line = await box(page, "timeline-playhead");
    expect(line.y).toBeCloseTo(ruler.y + ruler.height, 0);
    const head = await box(page, "timeline-playhead-head");
    expect([Math.round(head.width), Math.round(head.height)]).toEqual([11, 10]);
    expect(head.y + head.height).toBeCloseTo(ruler.y + ruler.height, 0);
    expect(head.x + head.width / 2).toBeCloseTo(line.x + 0.5, 0);

    // 8 s is inside c1 on V1: unscrolled, the line is on top of the clip.
    const header = await box(page, "track-lane-header-v1");
    const rowY = header.y + 30;
    expect(await paintedAt(page, line.x + 0.5, rowY)).toBe("timeline-playhead");

    // Scrolled 500px, 8 s (596px in) sits 96px into the 196px label column:
    // the headers and the ruler's label cell stay on top of the line and head.
    await page.getByTestId("timeline-scroll").evaluate((el) => (el.scrollLeft = 500));
    await expect.poll(async () => (await box(page, "timeline-playhead")).x).toBeLessThan(header.x + header.width);
    const x = (await box(page, "timeline-playhead")).x + 0.5;
    expect(await paintedAt(page, x, rowY)).toMatch(/^track-(lane-)?header-v1/);
    expect(await paintedAt(page, x, ruler.y + ruler.height - 3)).toMatch(/^timeline-(ruler-label|add-track)/);
  });
});

test.describe("parity 960x640: the clips", () => {
  test("clips keep their 47px body and the playhead its head at the compact size", async ({ page }) => {
    await openParity(page, { width: 960, height: 640 }, { invitation: false, workspace: { playhead_ms: 2_000 } });
    expect((await box(page, "clip-c1")).height).toBeCloseTo(47, 0);
    await expect(page.getByTestId("timeline-playhead-head")).toBeVisible();
    // The label column is 174 here: 2 s is 100px past it.
    const content = await box(page, "timeline-content");
    expect((await box(page, "timeline-playhead")).x - content.x).toBeCloseTo(174 + 100, 0);
  });
});
