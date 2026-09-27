import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, type Page, test } from "@playwright/test";

import { openParity } from "./parity";
import { FIXTURE_VIDEO_URL, installTauriStub } from "./tauriStub";

/**
 * Keyboard, forced colours and contrast (Task 58; F-49, F-48), in a real
 * browser against the PRODUCTION bundle — happy-dom has no focus order, no
 * forced-colours mode and no colours to measure (`tests/editorA11y.test.ts`
 * holds the names and the focus-return rules).
 *
 * The runtime is stubbed exactly as the other specs stub it
 * (`tauriStub.ts`: `window.__TAURI_INTERNALS__` before the module graph
 * runs), and the edits Rust would answer are a SEQUENCE of projections the
 * stub hands back in order — no test hook ships in the app (F28, and the
 * last test here checks the build for one).
 */

const FIXTURE = fileURLToPath(new URL("./fixtures/capture-1920x1080.webm", import.meta.url));
const DIST = fileURLToPath(new URL("../../dist", import.meta.url));

function clip(id: string, start: number, end: number) {
  return {
    id, asset_id: "capture", track_id: "v1", name: id, start_ms: start, in_ms: start, out_ms: end,
    fade_in_ms: 0, fade_out_ms: 0, fade_curve: "linear", opacity: 1, volume: 1, muted: false,
    x: 0, y: 0, w: 1, h: 1,
  };
}

function project(clips: unknown[], effects: unknown[] = []) {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-keys",
    title: "Keys",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "capture", kind: "video", name: "cap one", duration_ms: 6000 }],
    tracks: [
      { id: "v1", kind: "video", name: "Video", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      { id: "a1", kind: "audio", name: "Audio", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    ],
    clips,
    effects,
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-e2e", folder: "", dated: false },
  };
}

function snapshot(revision: number, persistedRevision: number, durationMs = 6000) {
  return {
    sessionId: "ses-keys", projectId: "project-keys", revision, persistedRevision, title: "Keys",
    durationMs, canUndo: revision > 1, canRedo: false, undoLabel: revision > 1 ? "Edit" : null, redoLabel: null,
  };
}

const INTRO = clip("intro", 0, 2000);
const OUTRO = clip("outro", 4000, 6000);
const TEXT_CUE = {
  id: "cue-1", clip_id: "body-b", kind: "text", start_ms: 3000, end_ms: 4000, x: 0.1, y: 0.1, color: "#ffffff",
  text: "Text",
};

/** What Rust answers, edit by edit: split `body` at 3 s, delete its left
 * half, add a text cue on the right half, set the cue's text. */
const EDITS = [
  { snapshot: snapshot(2, 1), project: project([INTRO, clip("body", 2000, 3000), clip("body-b", 3000, 4000), OUTRO]) },
  { snapshot: snapshot(3, 1), project: project([INTRO, clip("body-b", 3000, 4000), OUTRO]) },
  { snapshot: snapshot(4, 1), project: project([INTRO, clip("body-b", 3000, 4000), OUTRO], [TEXT_CUE]) },
  {
    snapshot: snapshot(5, 1),
    project: project([INTRO, clip("body-b", 3000, 4000), OUTRO], [{ ...TEXT_CUE, text: "Press Save" }]),
  },
];

const FRESH_PROGRESS = {
  contentRevision: 1, currentStepId: null, reviewed: [], explored: [], invitationDismissed: true,
  active: false, collapsed: false, completed: false, preferences: { dimming: true, motion: "system" },
};

async function openEditor(
  page: Page,
  theme: "dark" | "light",
  size = { width: 1280, height: 820 },
  extraReplies: Record<string, unknown> = {},
) {
  const workspace = { playhead_ms: 3000, theme };
  await installTauriStub(page, {
    openResult: {
      snapshot: snapshot(1, 1),
      project: project([INTRO, clip("body", 2000, 4000), OUTRO]),
      workspace,
      missing: [],
      sourceBase: "2026-09-21 0848 Screen Capture",
      recovered: false,
    },
    replies: {
      editor_get_guide_progress: FRESH_PROGRESS,
      editor_save_guide_progress: null,
      editor_get_workspace: workspace,
      editor_save_workspace: null,
      editor_get_checks: [],
      editor_get_products: [],
      editor_save_project: { sessionId: "ses-keys", savedRevision: 5, projectFileId: "project-keys" },
      ...extraReplies,
    },
    sequences: { editor_execute: EDITS.map((e) => structuredClone(e)) },
  });
  await page.route(`**${FIXTURE_VIDEO_URL}`, (route) =>
    route.fulfill({ contentType: "video/webm", body: readFileSync(FIXTURE) }),
  );
  await page.setViewportSize(size);
  await page.goto("/");
  await expect(page.getByTestId("editor-shell")).toBeAttached();
}

const focusedTestId = (page: Page) =>
  page.evaluate(() => document.activeElement?.getAttribute("data-testid") ?? null);

/** Press Tab until the focused element is `testid` — how a keyboard user
 * gets anywhere. Fails naming where focus went if it never arrives. */
async function tabTo(page: Page, testid: string, limit = 200) {
  const seen: (string | null)[] = [];
  for (let i = 0; i < limit; i++) {
    if ((await focusedTestId(page)) === testid) return;
    await page.keyboard.press("Tab");
    seen.push(await focusedTestId(page));
  }
  throw new Error(`Tab never reached ${testid}; focus visited ${seen.filter(Boolean).join(", ")}`);
}

/** Press ArrowDown inside an open menu until `testid` has focus — menu
 * items are one Tab stop, walked with the arrows. */
async function arrowTo(page: Page, testid: string, limit = 20) {
  for (let i = 0; i < limit; i++) {
    if ((await focusedTestId(page)) === testid) return;
    await page.keyboard.press("ArrowDown");
  }
  throw new Error(`ArrowDown never reached ${testid}`);
}

/** Press Tab until a button has focus. */
async function tabToButton(page: Page, limit = 40) {
  for (let i = 0; i < limit; i++) {
    await page.keyboard.press("Tab");
    if (await page.evaluate(() => document.activeElement?.tagName === "BUTTON")) return;
  }
  throw new Error("Tab never reached a button");
}

type Call = { cmd: string; args: { request?: { command?: unknown } } | null };
const executed = async (page: Page) =>
  (await page.evaluate(() => (window as unknown as { __calls: Call[] }).__calls))
    .filter((c) => c.cmd === "editor_execute")
    .map((c) => c.args?.request?.command);

test("keyboard-only journey completes", async ({ page }) => {
  await openEditor(page, "dark");
  // Proof that nothing below used a pointer: any pointer or mouse press
  // anywhere on the page is counted.
  await page.evaluate(() => {
    const w = window as unknown as { __pointerPresses: number };
    w.__pointerPresses = 0;
    for (const type of ["pointerdown", "mousedown"]) {
      document.addEventListener(type, () => (w.__pointerPresses += 1), true);
    }
  });

  // Tab to the timeline and select a clip.
  await tabTo(page, "clip-body");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("clip-body")).toHaveAttribute("aria-selected", "true");

  // S splits the selected clip at the playhead; Delete removes it.
  await page.keyboard.press("s");
  await expect(page.getByTestId("clip-body-b")).toBeAttached();
  await expect(page.getByTestId("clip-body")).toBeFocused();
  await page.keyboard.press("Delete");
  await expect(page.getByTestId("clip-body")).toHaveCount(0);

  // Add a text cue from the preview header's tool strip with Enter, then type its text.
  await tabTo(page, "preview-tool-text");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("effect-section")).toBeVisible();
  await tabTo(page, "effect-field-text");
  await page.keyboard.press("Control+A");
  await page.keyboard.type("Press Save");
  await page.keyboard.press("Enter");
  // At 1280 the header drops its save text (concept spec §1.4); the status
  // bar's centre slot is what the user sees.
  const recovery = page.getByTestId("editor-statusbar-recovery");
  await expect(recovery).toBeVisible();
  await expect(recovery).toHaveText("Unsaved changes are journaled for recovery");

  // Ctrl+S from outside a text field (a field keeps its own keys, so the
  // user Tabs on to the next button): the header's Save, and the status
  // bar says so once Rust's receipt lands.
  await tabToButton(page);
  await page.keyboard.press("Control+s");
  await expect(recovery).toHaveText("All changes saved");

  expect(await executed(page)).toEqual([
    { kind: "splitClip", clipId: "body", atMs: 3000 },
    { kind: "deleteClips", clipIds: ["body"], closeGap: false },
    { kind: "addEffect", clipId: "body-b", startMs: 3000, endMs: 4000, effectKind: "text", props: {} },
    { kind: "updateEffect", effectId: "cue-1", props: { text: "Press Save" } },
  ]);
  const invoked = await page.evaluate(() => (window as unknown as { __invoked: string[] }).__invoked);
  expect(invoked.filter((c) => c === "editor_save_project")).toHaveLength(1);
  expect(await page.evaluate(() => (window as unknown as { __pointerPresses: number }).__pointerPresses)).toBe(0);
});

/** An element's computed outline, in forced colours. */
const outline = (page: Page, testid: string) =>
  page.getByTestId(testid).evaluate((el) => {
    const s = getComputedStyle(el);
    return { style: s.outlineStyle, width: parseFloat(s.outlineWidth) };
  });

test("forced colors keep selection visible", async ({ page }) => {
  await page.emulateMedia({ forcedColors: "active" });
  await openEditor(page, "dark");

  // The focus ring on a clip.
  await tabTo(page, "clip-body");
  const focusRing = await outline(page, "clip-body");
  expect(focusRing.style, "the focused clip has no outline in forced colours").not.toBe("none");
  expect(focusRing.width).toBeGreaterThanOrEqual(2);

  // The selection, once focus has moved on: a box-shadow ring is dropped
  // in forced colours, so this has to be an outline too.
  await page.keyboard.press("Enter");
  await page.keyboard.press("Tab");
  expect(await focusedTestId(page)).not.toBe("clip-body");
  const selected = await outline(page, "clip-body");
  expect(selected.style, "the selected clip is not marked in forced colours").not.toBe("none");
  expect(selected.width).toBeGreaterThanOrEqual(2);
  expect((await outline(page, "clip-intro")).style, "an unselected clip stays unmarked").toBe("none");

  // The playhead and a clip's trim/fade handles keep their own colour.
  for (const testid of ["timeline-playhead", "timeline-playhead-head", "clip-body-trim-start-bar", "clip-body-fade-in-handle"]) {
    const adjust = await page
      .getByTestId(testid)
      .evaluate((el) => ({ adjust: getComputedStyle(el).forcedColorAdjust, bg: getComputedStyle(el).backgroundColor }));
    expect(adjust.adjust, `${testid} is repainted to the page colour in forced colours`).toBe("none");
    expect(adjust.bg).not.toMatch(/rgba\(0, 0, 0, 0\)|transparent/);
  }
});

/** Every visible text on `root`'s surfaces whose contrast with what is
 * behind it is under 4.5:1 — measured on the composited colours, skipping
 * disabled controls (WCAG exempts them) and anything over the video. */
async function lowContrast(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    type Rgba = [number, number, number, number];
    // Tailwind 4's palette is oklch, which computed styles report as-is;
    // a 1x1 canvas converts any CSS colour to sRGB bytes.
    const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
    const parse = (c: string): Rgba => {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = "rgba(0, 0, 0, 0)";
      ctx.fillStyle = c;
      ctx.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
      return [r, g, b, a / 255];
    };
    const over = (top: Rgba, bottom: Rgba): Rgba => {
      const a = top[3] + bottom[3] * (1 - top[3]);
      if (a === 0) return [0, 0, 0, 0];
      const mix = (i: number) => (top[i] * top[3] + bottom[i] * bottom[3] * (1 - top[3])) / a;
      return [mix(0), mix(1), mix(2), a];
    };
    const background = (el: Element | null): Rgba => {
      const layers: Rgba[] = [];
      for (let n = el; n; n = n.parentElement) layers.push(parse(getComputedStyle(n).backgroundColor));
      return layers.reduceRight<Rgba>((under, layer) => over(layer, under), [255, 255, 255, 1]);
    };
    const lum = (c: Rgba) => {
      const f = (v: number) => {
        const s = v / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
    };
    const faded = (el: Element) => {
      for (let n: Element | null = el; n; n = n.parentElement) {
        if (parseFloat(getComputedStyle(n).opacity) < 1) return true;
        if ((n as HTMLButtonElement).disabled || n.getAttribute("aria-disabled") === "true") return true;
      }
      return false;
    };
    const ownText = (el: Element) =>
      Array.from(el.childNodes)
        .filter((n) => n.nodeType === Node.TEXT_NODE)
        .map((n) => n.textContent ?? "")
        .join("")
        .trim();
    const measurable = (el: Element, text: string) => {
      const box = el.getBoundingClientRect();
      return text !== "" && box.width > 0 && box.height > 0 && !faded(el) && !el.closest("svg");
    };
    /** `el`'s text as a failure line when it reads under 4.5:1, else null. */
    const check = (el: Element): string | null => {
      const text = ownText(el);
      if (!measurable(el, text)) return null;
      const bg = background(el);
      const [hi, lo] = [lum(over(parse(getComputedStyle(el).color), bg)), lum(bg)].sort((a, b) => b - a);
      const ratio = (hi + 0.05) / (lo + 0.05);
      const id = el.closest("[data-testid]")?.getAttribute("data-testid") ?? el.tagName;
      return ratio < 4.5 ? `${id} "${text.slice(0, 30)}" ${ratio.toFixed(2)}:1` : null;
    };
    const roots = ["editor-header", "editor-shell-library", "editor-shell-inspector", "editor-timeline", "editor-statusbar", "preview-header", "guide-invitation", "guide-coach", "guide-mini"]
      .map((id) => document.querySelector(`[data-testid="${id}"]`))
      .concat(Array.from(document.querySelectorAll('[role="menu"]')))
      .concat(Array.from(document.querySelectorAll('[role="dialog"]')))
      .filter((el): el is Element => el !== null);
    return roots
      .flatMap((root) => Array.from(root.querySelectorAll("*")).concat(root))
      .map(check)
      .filter((line): line is string => line !== null);
  });
}

/** WCAG 1.4.11 (non-text contrast): the ratio between `testid`'s own
 * (possibly translucent) background, composited over whatever sits behind
 * it, and that backdrop alone — i.e., whether the component's own edge is
 * visible against what it sits on. The threshold is 3:1, not text's 4.5:1
 * (`lowContrast` above), and there is no text to skip for disabled
 * controls — a UI-component boundary is measured whether or not it is
 * "text". */
async function boundaryContrast(page: Page, testid: string): Promise<number> {
  return surfaceContrast(page, `[data-testid="${testid}"]`);
}

/** `boundaryContrast`'s measurement for any CSS `selector`: the element's
 * own background, composited over what is behind it, against that backdrop
 * alone. Read while the pointer is really over the element, it measures a
 * `:hover` tint too. */
async function surfaceContrast(page: Page, selector: string): Promise<number> {
  return page.evaluate((sel) => {
    type Rgba = [number, number, number, number];
    const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true })!;
    const parse = (c: string): Rgba => {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = "rgba(0, 0, 0, 0)";
      ctx.fillStyle = c;
      ctx.fillRect(0, 0, 1, 1);
      const [r, g, b, a] = ctx.getImageData(0, 0, 1, 1).data;
      return [r, g, b, a / 255];
    };
    const over = (top: Rgba, bottom: Rgba): Rgba => {
      const a = top[3] + bottom[3] * (1 - top[3]);
      if (a === 0) return [0, 0, 0, 0];
      const mix = (i: number) => (top[i] * top[3] + bottom[i] * bottom[3] * (1 - top[3])) / a;
      return [mix(0), mix(1), mix(2), a];
    };
    const backgroundBehind = (el: Element): Rgba => {
      const layers: Rgba[] = [];
      for (let n: Element | null = el.parentElement; n; n = n.parentElement) {
        layers.push(parse(getComputedStyle(n).backgroundColor));
      }
      return layers.reduceRight<Rgba>((under, layer) => over(layer, under), [255, 255, 255, 1]);
    };
    const lum = (c: Rgba) => {
      const f = (v: number) => {
        const s = v / 255;
        return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
      };
      return 0.2126 * f(c[0]) + 0.7152 * f(c[1]) + 0.0722 * f(c[2]);
    };
    const el = document.querySelector(sel);
    if (!el) throw new Error(`no element matches ${sel}`);
    const behind = backgroundBehind(el);
    const own = parse(getComputedStyle(el).backgroundColor);
    const composited = over(own, behind);
    const [hi, lo] = [lum(composited), lum(behind)].sort((a, b) => b - a);
    return (hi + 0.05) / (lo + 0.05);
  }, selector);
}

// Fix round 1 (review Important, two sites): the trim handles are a static
// overlay, not text — `lowContrast` above never saw them. Only light is
// asserted here: the dark theme's own `bg-white/10` measures ~1.36:1 too
// (unchanged, byte-identical, and out of this fix's scope — the review
// flagged the LIGHT theme as invisible, not dark).
test("light theme trim handles meet 3:1 against the clip (WCAG 1.4.11)", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await openEditor(page, "light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await tabTo(page, "clip-body");
  await page.keyboard.press("Enter");
  // The grip's visible part is its ink bar (§6.5); the 9px grip around it
  // is a transparent hit area.
  const ratio = await boundaryContrast(page, "clip-body-trim-start-bar");
  expect(ratio, "the trim handle is not distinguishable from the clip body in light theme").toBeGreaterThanOrEqual(3);
});

// GAP-206's recorded residual: SaveProjectDialog's format rows (hover AND
// the selected row) and the learning center's chapter/lesson rows carried
// `bg-white/5` — white over the light theme's white `bg-panel`, 1.00:1,
// i.e. no tint at all. `--color-hover-subtle` keeps the dark theme's
// literal (pinned below at its pre-fix 1.16:1 on `panel`) and gives light
// a slate tint designed at 1.11:1 — a hover affordance, so no WCAG floor;
// the bar is "visibly there", lighter than `--color-hover`'s 1.13:1.
const SUBTLE_ROWS = [
  'label:has([data-testid="save-project-format-portable"])',
  'label:has([data-testid="save-project-format-lightweight"])',
  '[data-testid="learning-chapter-orient"]',
  '[data-testid^="learning-lesson-"]',
] as const;

for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme subtle row tints are discernible`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    const expectTint = async (selector: string, pinned = true) => {
      const ratio = await surfaceContrast(page, selector);
      if (theme === "light" || !pinned) expect(ratio, `${selector} tint in ${theme}`).toBeGreaterThanOrEqual(1.08);
      // The pre-fix `bg-white/5` measurement, to the canvas's byte rounding.
      else expect(ratio, `${selector} tint in dark (unchanged)`).toBeCloseTo(1.1583538594, 6);
    };

    // The portable row is selected (the dialog opens on the format the
    // menu item named) and, since visual-parity Task 21 (concept §9.5
    // `.save-option:has(input:checked)`), wears the accent tint rather than
    // the hover one; the lightweight row is only tinted under the pointer.
    await page.getByTestId("editor-header-project-menu").click();
    await page.getByTestId("editor-project-menu-item-saveCopy").click();
    await expect(page.getByTestId("save-project-originals-warning")).toBeVisible();
    await page.mouse.move(0, 0);
    expect(await surfaceContrast(page, SUBTLE_ROWS[0]), "the selected card's accent tint").toBeGreaterThanOrEqual(1.08);
    await page.hover(SUBTLE_ROWS[1]);
    await expectTint(SUBTLE_ROWS[1]);
    await page.keyboard.press("Escape");

    await page.getByTestId("editor-header-help").click();
    await page.getByTestId("editor-help-menu-item-learningCenter").click();
    await expect(page.getByTestId("learning-center")).toBeVisible();
    // Visual-parity Task 23 (concept §9.3): a chapter's lessons sit behind
    // its "See n steps" disclosure.
    await page.getByTestId("learning-card-orient").locator("summary").click();
    // The chapter cards sit on `--bg` since visual-parity Task 23 (concept
    // §9.3 `.guide-chapter{background:var(--bg)}`), not on `panel`, so the
    // same token composites to a different (still visible) dark tint: the
    // pinned `panel` figure no longer describes them.
    for (const selector of SUBTLE_ROWS.slice(2)) {
      await page.hover(selector);
      await expectTint(selector, false);
    }
  });
}

// Both themes (GAP-206 fixed the light one, GAP-209 the dark one): every
// visible text on the editor's surfaces, and in an open menu, reads at
// 4.5:1 or more against what is composited behind it.
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme text meets 4.5:1 on every surface`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    await tabTo(page, "clip-body");
    await page.keyboard.press("Enter");
    // Render video is enabled here, so its white label on the primary fill
    // is measured with the rest (ruling T3-1).
    await expect(page.getByTestId("editor-header-render")).toBeEnabled();
    expect(await lowContrast(page)).toEqual([]);

    // An open menu is a surface too.
    await tabTo(page, "editor-header-help");
    await page.keyboard.press("Enter");
    await expect(page.getByRole("menu", { name: "Help" })).toBeVisible();
    expect(await lowContrast(page)).toEqual([]);
    await page.keyboard.press("Escape");

    // A dialog is a surface too (review I-2/F-M4): the Project menu's
    // "Save a copy as project file…" opens SaveProjectDialog, whose "includes
    // your original recordings" warning was the one literal palette text
    // class left under src/components/editor/** — it measured ~1.05:1 in
    // the light theme before the fix (11.7:1 in dark, which is why only
    // light theme caught it).
    await tabTo(page, "editor-header-project-menu");
    await page.keyboard.press("Enter");
    await arrowTo(page, "editor-project-menu-item-saveCopy");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("save-project-originals-warning")).toBeVisible();
    expect(await lowContrast(page)).toEqual([]);
  });
}

// visual-parity Task 10: the library's Titles, Captions and Chapters tabs
// and its Project section (with a product card, so its "Earlier edit" pill
// is measured — only a render this session finished can match) are
// surfaces too.
const PRODUCT = {
  id: "prod-keys",
  projectId: "project-keys",
  name: "Walkthrough",
  filename: "prod-keys.mp4",
  mime: "video/mp4",
  revision: 1,
  durationMs: 6000,
  createdAt: "2026-09-25T10:00:00.000Z",
  editFingerprint: "fingerprint",
  renderRange: null,
  available: true,
};
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme text meets 4.5:1 in the library's other views`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme, undefined, { editor_get_products: [PRODUCT] });
    for (const tab of ["titles", "captions", "chapters"]) {
      await page.getByTestId(`library-tab-${tab}`).click();
      expect(await lowContrast(page), tab).toEqual([]);
    }
    await page.getByTestId("editor-statusbar-products").click();
    await expect(page.getByTestId("product-match-prod-keys")).toHaveText("Earlier edit");
    expect(await lowContrast(page)).toEqual([]);
  });
}

// Ruling T3-2: the Checks count is the concept's gold chip (§2
// `#issueCount`), mono 9px gold on gold-bg — it replaced a shared badge
// that read 2.2:1 — and it only renders with findings, so this run has one.
const WARNING = {
  id: "chk-gap-c1", severity: "warning", code: "gap", message: "A gap", target: { kind: "clip", id: "intro" }, action: null,
};
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme Checks count chip reads 4.5:1`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme, undefined, { editor_get_checks: [WARNING] });
    await expect(page.getByTestId("editor-header-checks-count")).toHaveText("1");
    expect(await lowContrast(page)).toEqual([]);
  });
}

// Visual-parity Task 21 (concept §9.4, §9.6; D16): the Checks dialog's
// FIX / REVIEW / NOTE chips, its rows and footer, and the Render dialog's
// cards and callout read 4.5:1 in both themes.
const SHARE_FINDINGS = [
  { id: "chk-missingMedia-intro", severity: "blocking", code: "missingMedia", message: "A file is missing.", target: { kind: "clip", id: "intro" }, action: "reconnect" },
  WARNING,
  { id: "chk-excludedCaptions-project", severity: "info", code: "excludedCaptions", message: "Captions are off.", target: { kind: "project", id: null }, action: "openCaptions" },
];
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme text meets 4.5:1 in the Checks and Render dialogs`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme, undefined, {
      editor_get_checks: SHARE_FINDINGS,
      list_vaults: [{ id: "vault-e2e", name: "Knowledge vault", path: "C:/v", open: false }],
    });
    await page.getByTestId("editor-header-checks").click();
    await expect(page.getByTestId("check-kind-chk-missingMedia-intro")).toHaveText("FIX");
    expect(await lowContrast(page), "Checks").toEqual([]);
    await page.getByTestId("checks-back").click();

    await page.getByTestId("editor-header-render").click();
    await expect(page.getByTestId("render-dialog-profile")).toBeVisible();
    expect(await lowContrast(page), "Render").toEqual([]);
  });
}

// Visual-parity Task 22 (concept spec §9.7, §9.8; D16): the webcam dialog
// (camera off — nothing asks for it) and the audio mixer read 4.5:1 in both
// themes. The camera view keeps its own dark picture in light too.
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme text meets 4.5:1 in the webcam dialog and the mixer`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme);
    await page.getByTestId("library-webcam").click();
    await expect(page.getByTestId("webcam-empty")).toBeVisible();
    expect(await lowContrast(page), "Webcam").toEqual([]);
    await page.getByTestId("webcam-close").click();

    await page.getByTestId("mixer-toggle").click();
    await expect(page.getByTestId("mixer-popover")).toBeVisible();
    expect(await lowContrast(page), "Mixer").toEqual([]);
  });
}

// Visual-parity Task 23 (D16): the guide's surfaces — the invitation, the
// coach card in each of its task voices, the minimized bar and the
// learning center — read at 4.5:1 in both themes.
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme: the guide's surfaces meet 4.5:1`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: theme });
    await openEditor(page, theme, { width: 1600, height: 1000 }, {
      editor_get_guide_progress: { ...FRESH_PROGRESS, invitationDismissed: false },
    });
    await expect(page.getByTestId("guide-invitation")).toBeVisible();
    expect(await lowContrast(page), "invitation").toEqual([]);

    await page.getByTestId("guide-invitation-start").click();
    await expect(page.getByTestId("guide-coach")).toBeVisible();
    expect(await lowContrast(page), "coach, prompt").toEqual([]);
    await page.getByTestId("guide-collapse").click();
    await expect(page.getByTestId("guide-mini")).toBeVisible();
    expect(await lowContrast(page), "minimized").toEqual([]);

    await page.getByTestId("editor-header-help").click();
    await page.getByTestId("editor-help-menu-item-learningCenter").click();
    await expect(page.getByTestId("learning-center")).toBeVisible();
    await page.getByTestId("learning-card-orient").locator("summary").click();
    await page.getByTestId("learning-preferences").locator("summary").click();
    expect(await lowContrast(page), "learning center").toEqual([]);

    // The gold "optional edit" voice (fades), from a chapter jump.
    await page.getByTestId("learning-chapter-polish").click();
    await expect(page.getByTestId("guide-coach-task")).toHaveAttribute("data-voice", "edit");
    expect(await lowContrast(page), "coach, optional edit").toEqual([]);
  });
}

// Visual-parity Task 25 (review focus 1): the populated sample project the
// parity gate measures — every track kind, the Teaching layers and Captions
// rows, cue chips, a selected clip's inspector, the timeline footer — read
// 4.5:1 in both themes after the concept palette port. The project above
// ("Keys") has none of that chrome to measure.
for (const theme of ["light", "dark"] as const) {
  test(`${theme} theme text meets 4.5:1 across the populated workspace`, async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { theme, invitation: false });
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    expect(await lowContrast(page), "workspace").toEqual([]);
    await page.getByTestId("clip-c5").click();
    await page.getByTestId("inspector-tab-layout").click();
    expect(await lowContrast(page), "clip inspector").toEqual([]);
    await page.getByTestId("timeline-cue-fx4").click();
    await expect(page.getByTestId("inspector-title")).toHaveText("Teaching properties");
    expect(await lowContrast(page), "cue inspector").toEqual([]);
    await page.getByTestId("timeline-cue-fx4").click({ button: "right" });
    await expect(page.getByRole("menu").first()).toBeVisible();
    expect(await lowContrast(page), "cue menu").toEqual([]);
  });
}

test("reduced motion keeps the guide ring still", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await openEditor(page, "dark");
  await tabTo(page, "editor-header-help");
  await page.keyboard.press("F1");
  const ring = page.getByTestId("guide-ring");
  await expect(ring).toBeVisible();
  const transition = await ring.evaluate((el) => getComputedStyle(el).transitionDuration);
  expect(transition.split(",").every((d) => parseFloat(d) < 0.001)).toBe(true);
});

function files(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    return statSync(path).isDirectory() ? files(path) : [path];
  });
}

test("production dist ships no editor test hook", () => {
  const built = files(DIST).filter((p) => /\.(js|html|css)$/.test(p));
  expect(built.length, "dist/ holds the built bundle").toBeGreaterThan(0);
  const hooked = built.filter((p) => readFileSync(p, "utf8").includes("__VB_EDITOR_PORT__"));
  expect(hooked).toEqual([]);
});
