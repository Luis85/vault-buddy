import { mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import type { Page } from "@playwright/test";

import { PARITY_OPEN_RESULT, PARITY_REPLIES } from "./fixtures/parityProject";
import { FIXTURE_IMAGE_URL, FIXTURE_VIDEO_URL, installTauriStub } from "./tauriStub";

/**
 * The concept-parity e2e harness (Task 1 of the tutorial editor visual-
 * parity effort). Every later task's `editorParity.spec.ts` region test
 * opens the shell through `openParity`, reads a real box through `box`, and
 * (once its region is in place) renders a side-by-side comparison against
 * the matching concept screenshot through `composite` — the same "measure
 * the real, built bundle in Chromium" discipline as `editorShell.spec.ts`
 * (AGENTS.md's Testing conventions), aimed at concept parity rather than a
 * fixed-pixel contract.
 */

const VIDEO_FIXTURE_PATH = fixturePath("capture-1920x1080.webm");
const IMAGE_FIXTURE_PATH = fixturePath("thumb-160x90.jpg");

function fixturePath(name: string): string {
  return fileURLToPath(new URL(`./fixtures/${name}`, import.meta.url));
}

const CONCEPT_SCREENS_DIR = resolve(process.cwd(), "docs/concepts/vault-buddy-editor/screens");
const PARITY_OUT_DIR = resolve(process.cwd(), "test-results/parity");

export interface OpenParityOptions {
  /** The theme the project's saved workspace carries (default `"dark"`);
   * `null` saves none, so the editor shows its own default. */
  theme?: "dark" | "light" | null;
  /** The OS colour scheme to emulate (default: the saved theme, else
   * dark). */
  osScheme?: "dark" | "light";
  /** `false` dismisses the guide invitation (its "Not now" button) once the
   * shell has settled — the geometry/parity checks that follow want a clean
   * workspace, not the invitation card sitting on top of it. Omitted (or
   * `true`) leaves it showing, exactly as a fresh, never-dismissed vault
   * would (`PARITY_REPLIES`'s `invitationDismissed: false`). */
  invitation?: boolean;
  /** Fields merged into the saved workspace — a selection to open on (a
   * teaching cue, which the parity project cannot select by a click). */
  workspace?: Record<string, unknown>;
}

/** Opens the tutorial editor against the populated `PARITY_OPEN_RESULT`
 * sample project (`fixtures/parityProject.ts`), at `size`, and waits for the
 * shell to settle. Every parity spec starts here. */
export async function openParity(
  page: Page,
  size: { width: number; height: number },
  opts: OpenParityOptions = {},
): Promise<void> {
  // The editor opens dark whatever the OS prefers (design D1) and takes its
  // theme from the SAVED workspace, so the theme travels through the stub's
  // workspace replies; `emulateMedia` below only keeps native controls in
  // step with it.
  const theme = opts.theme === undefined ? "dark" : opts.theme;
  const saved = { ...PARITY_OPEN_RESULT.workspace, ...opts.workspace };
  const workspace = theme === null ? saved : { ...saved, theme };
  await installTauriStub(page, {
    openResult: { ...PARITY_OPEN_RESULT, workspace },
    replies: { ...PARITY_REPLIES, editor_get_workspace: workspace },
  });
  await page.route(`**${FIXTURE_VIDEO_URL}`, (route) =>
    route.fulfill({ contentType: "video/webm", body: readFileSync(VIDEO_FIXTURE_PATH) }),
  );
  await page.route(`**${FIXTURE_IMAGE_URL}`, (route) =>
    route.fulfill({ contentType: "image/jpeg", body: readFileSync(IMAGE_FIXTURE_PATH) }),
  );
  await page.emulateMedia({ colorScheme: opts.osScheme ?? theme ?? "dark" });
  await page.setViewportSize(size);
  await page.goto("/");
  await page.getByTestId("editor-shell").waitFor();
  // The shell mounts before the drawer collapse / grid columns / preview
  // canvas have all settled onto their final layout -- a fixed settle
  // rather than polling any one element, the `capture.spec.ts` throwaway
  // reference's own posture (Task 1's brief cites it as precedent).
  await page.waitForTimeout(800);
  if (opts.invitation === false) {
    await page.getByTestId("guide-invitation-dismiss").click();
  }
}

/** Drives one of the preview header's panel controls (visual-parity Task
 * 11): the library and properties toggles — which open the drawers at a
 * narrow width, since the header's own toggles left (Task 8) — and Focus
 * preview, which lives in the View menu. */
export async function previewTool(
  page: Page,
  id: "toggleLibrary" | "toggleInspector" | "focusPreview",
): Promise<void> {
  if (id === "toggleLibrary") {
    await page.getByTestId("preview-library-toggle").click();
    return;
  }
  if (id === "toggleInspector") {
    await page.getByTestId("preview-properties-toggle").click();
    return;
  }
  await page.getByTestId("preview-view-menu").click();
  await page.getByTestId("preview-view-panel-item-focusPreview").click();
}

/** A real bounding box for `data-testid="<testId>"`, or a named failure —
 * never a bare `null` a caller could forward into a `toBeCloseTo` and get a
 * confusing "NaN" failure three lines away. */
export async function box(
  page: Page,
  testId: string,
): Promise<{ x: number; y: number; width: number; height: number }> {
  const b = await page.getByTestId(testId).boundingBox();
  if (b === null) {
    throw new Error(`parity: data-testid="${testId}" has no bounding box (not rendered, or display:none)`);
  }
  return b;
}

/** Renders a side-by-side comparison of a concept screenshot
 * (`docs/concepts/vault-buddy-editor/screens/<conceptScreen>`) and the
 * built app's own screenshot (`builtPath`, already written by the caller —
 * typically `page.screenshot()` against the SAME page `composite` is
 * called with) to `test-results/parity/<outName>.png`. Each image renders
 * at its own natural size, captioned, so a size mismatch between the two is
 * itself visible in the composite rather than hidden by a shared scale.
 *
 * Deliberately opens a SIBLING page (`page.context().newPage()`) for the
 * composite rather than reusing `page` — `page.setContent` replaces
 * whatever the page was showing, and callers keep making assertions
 * against the live editor on `page` after calling this. `page.setContent`
 * needs no server: both images are inlined as base64 data URIs, so this
 * never touches the `vite preview` origin at all. */
export async function composite(
  page: Page,
  conceptScreen: string,
  builtPath: string,
  outName: string,
): Promise<void> {
  const conceptDataUrl = toDataUrl(join(CONCEPT_SCREENS_DIR, conceptScreen), "image/png");
  const builtDataUrl = toDataUrl(resolve(process.cwd(), builtPath), "image/png");

  const html = `<!doctype html>
<html>
<head><meta charset="utf-8"></head>
<body style="margin:0;background:#ffffff;font-family:sans-serif;color:#111111;">
  <div style="display:flex;align-items:flex-start;gap:16px;padding:16px;">
    <figure style="margin:0;">
      <figcaption style="font-size:14px;font-weight:600;margin-bottom:8px;">Concept — ${escapeHtml(conceptScreen)}</figcaption>
      <img src="${conceptDataUrl}" style="display:block;border:1px solid #cccccc;">
    </figure>
    <figure style="margin:0;">
      <figcaption style="font-size:14px;font-weight:600;margin-bottom:8px;">Built — ${escapeHtml(outName)}</figcaption>
      <img src="${builtDataUrl}" style="display:block;border:1px solid #cccccc;">
    </figure>
  </div>
</body>
</html>`;

  const compositePage = await page.context().newPage();
  try {
    await compositePage.setContent(html);
    const outPath = join(PARITY_OUT_DIR, `${outName}.png`);
    mkdirSync(dirname(outPath), { recursive: true });
    await compositePage.screenshot({ path: outPath, fullPage: true });
  } finally {
    await compositePage.close();
  }
}

function toDataUrl(path: string, mime: string): string {
  return `data:${mime};base64,${readFileSync(path).toString("base64")}`;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
