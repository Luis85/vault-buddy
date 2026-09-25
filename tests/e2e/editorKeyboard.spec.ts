import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, type Page, test } from "@playwright/test";

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

async function openEditor(page: Page, theme: "dark" | "light", size = { width: 1280, height: 820 }) {
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

  // Add a text cue from the preview toolbar with Enter, then type its text.
  await tabTo(page, "preview-toolbar-addText");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("effect-section")).toBeVisible();
  await tabTo(page, "effect-field-text");
  await page.keyboard.press("Control+A");
  await page.keyboard.type("Press Save");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("editor-header-status")).toHaveText("Unsaved changes");

  // Ctrl+S from outside a text field (a field keeps its own keys, so the
  // user Tabs on to the next button): the header's Save, and the header
  // says so once Rust's receipt lands.
  await tabToButton(page);
  await page.keyboard.press("Control+s");
  await expect(page.getByTestId("editor-header-status")).toHaveText("Saved");

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
  for (const testid of ["timeline-playhead", "clip-body-trim-start", "clip-body-fade-in-handle"]) {
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
    const roots = ["editor-header", "editor-shell-library", "editor-shell-inspector", "editor-shell-timeline", "preview-toolbar"]
      .map((id) => document.querySelector(`[data-testid="${id}"]`))
      .concat(Array.from(document.querySelectorAll('[role="menu"]')))
      .filter((el): el is Element => el !== null);
    return roots
      .flatMap((root) => Array.from(root.querySelectorAll("*")).concat(root))
      .map(check)
      .filter((line): line is string => line !== null);
  });
}

test("light theme text meets 4.5:1 on every surface", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await openEditor(page, "light");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await tabTo(page, "clip-body");
  await page.keyboard.press("Enter");
  expect(await lowContrast(page)).toEqual([]);

  // An open menu is a surface too.
  await tabTo(page, "editor-header-help");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menu", { name: "Help" })).toBeVisible();
  expect(await lowContrast(page)).toEqual([]);
});

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
