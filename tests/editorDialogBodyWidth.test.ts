/**
 * Task 6 fix round 1 (visual-parity, concept-spec §9): the dialog's WIDTH is
 * `DialogHost`'s own `width` prop now, not something a dialog's body content
 * sizes itself — but three migrations (`CloseGuardDialog`, `DiscardProjectDialog`,
 * `RecoveryDialog`) kept a leftover `w-96 max-w-full` (384px) on their own
 * body root, leaving dead space inside the wider 560/560/660px frame, and no
 * gate caught it (a human review did).
 *
 * This finds each `DialogHost` consumer's own body root — the element
 * carrying the dialog's `data-testid`, one level inside `DialogHost`'s
 * default slot — and asserts its OPENING TAG never reintroduces a fixed
 * `w-*` width utility. It reads the opening tag as the lines from the
 * `data-testid` attribute to that same element's closing `>` (this repo's
 * Prettier formatting puts one attribute per line and a lone `>` — never an
 * unencoded `>` inside an attribute value on these elements), rather than a
 * same-line regex, which would miss every one of these dialogs: none of
 * them puts `data-testid` and `class` on the same line.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DIALOGS_DIR = path.resolve(HERE, "../src/components/editor/dialogs");
const GUIDE_DIR = path.resolve(HERE, "../src/components/editor/guide");

// Every DialogHost consumer, and the data-testid on its own body root — the
// element directly inside DialogHost's default (unnamed) slot that used to
// carry the dialog's own width before Task 6 gave that job to DialogHost's
// `width` prop.
const BODY_ROOTS: readonly { dir: string; file: string; testid: string }[] = [
  { dir: DIALOGS_DIR, file: "ChecksDialog.vue", testid: "checks-dialog" },
  { dir: DIALOGS_DIR, file: "CloseGuardDialog.vue", testid: "close-guard" },
  { dir: DIALOGS_DIR, file: "DiscardProjectDialog.vue", testid: "discard-project-dialog" },
  { dir: DIALOGS_DIR, file: "PublishDialog.vue", testid: "publish-dialog" },
  { dir: DIALOGS_DIR, file: "ReconnectDialog.vue", testid: "reconnect-dialog" },
  { dir: DIALOGS_DIR, file: "RecoveryDialog.vue", testid: "recovery-dialog" },
  { dir: DIALOGS_DIR, file: "RenderDialog.vue", testid: "render-dialog" },
  { dir: DIALOGS_DIR, file: "ReviewDialog.vue", testid: "review-dialog" },
  { dir: DIALOGS_DIR, file: "SaveProjectDialog.vue", testid: "save-project-dialog" },
  { dir: DIALOGS_DIR, file: "WebcamDialog.vue", testid: "webcam-dialog" },
  { dir: GUIDE_DIR, file: "LearningCenter.vue", testid: "learning-center" },
];

// `w-96`, `w-40`, `w-[30rem]`, `w-[480px]`, … — a fixed-width utility, never
// `max-w-*`/`min-w-*`/anything else starting with `w-`'s own letter run.
const FIXED_WIDTH = /(?:^|[\s"])w-(?:\d+|\[[^\]]+\])(?=[\s"]|$)/;

/** The element's own opening tag, as the source text from the line naming
 * `data-testid="testid"` up to (and including) that tag's closing `>` —
 * never past it, so a later sibling's own `class` can't be mistaken for
 * this element's. */
function openingTagOf(source: string, testid: string): string {
  const lines = source.split("\n");
  const startIndex = lines.findIndex((line) => line.includes(`data-testid="${testid}"`));
  if (startIndex === -1) throw new Error(`no data-testid="${testid}" found`);
  const tagLines: string[] = [];
  for (let i = startIndex; i < lines.length; i += 1) {
    tagLines.push(lines[i]);
    if (/(^|[^=])>\s*$/.test(lines[i])) break; // a lone `>` (never `="`…`>` inside a value)
  }
  return tagLines.join("\n");
}

describe("dialog body roots never reintroduce a fixed width", () => {
  for (const { dir, file, testid } of BODY_ROOTS) {
    it(`${file}'s "${testid}" root fills DialogHost's own width`, () => {
      const source = readFileSync(path.join(dir, file), "utf8");
      const tag = openingTagOf(source, testid);
      expect(tag, `${file}'s "${testid}" opening tag`).not.toMatch(FIXED_WIDTH);
    });
  }
});
