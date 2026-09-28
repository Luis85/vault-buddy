#!/usr/bin/env node
/**
 * One-shot generator: turns the tutorial editor's concept icon data into a
 * typed TS module the app imports at build time.
 *
 * Source of truth: docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json
 * (74 icon entries + one `_meta` key, which is skipped). Each entry's
 * `svgInnerMarkup` is the trusted inner-SVG markup `EditorIcon.vue` renders
 * verbatim — never user- or vault-derived content.
 *
 * Idempotent: run it again any time the source JSON changes and commit the
 * regenerated file — re-running against an unchanged source produces a
 * byte-identical file (icons are emitted in sorted key order, independent of
 * the JSON's own key order, and every string goes through JSON.stringify so
 * escaping never drifts between runs).
 *
 * Usage: node scripts/gen-editor-icons.mjs
 */

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");
const SOURCE_JSON = join(
  ROOT,
  "docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json",
);
const OUT_FILE = join(
  ROOT,
  "src/components/editor/icons/conceptIcons.ts",
);

function loadIconNames(data) {
  return Object.keys(data)
    .filter((key) => key !== "_meta")
    .sort();
}

function renderModule(data, names) {
  const lines = names.map(
    (name) => `  ${name}: ${JSON.stringify(data[name].svgInnerMarkup)},`,
  );
  return [
    "// GENERATED FILE — do not edit by hand.",
    "// Regenerate with: node scripts/gen-editor-icons.mjs",
    "// Source: docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json",
    "//",
    "// Each value is the trusted inner-SVG markup for one concept icon,",
    "// carried verbatim from the source JSON's svgInnerMarkup field.",
    "// EditorIcon.vue renders it inside a shared <svg> wrapper.",
    "",
    "export const ICONS = {",
    ...lines,
    "} as const;",
    "",
    "export type EditorIconName = keyof typeof ICONS;",
    "",
  ].join("\n");
}

function main() {
  const data = JSON.parse(readFileSync(SOURCE_JSON, "utf8"));
  const names = loadIconNames(data);
  const module = renderModule(data, names);
  writeFileSync(OUT_FILE, module, { encoding: "utf8" });
  console.log(`Wrote ${names.length} icons to ${OUT_FILE}`);
}

main();
