#!/usr/bin/env node
/**
 * LOC guard: fail when a source file grows past the agreed ceiling.
 *
 * Why this exists: agentic contributors optimize for finishing the requested
 * change, which quietly grows already-large modules and creates new oversized
 * ones. The 2026-07-10 audit flagged the two current hotspots (Search.vue,
 * stores/capture.ts — docs/Gaps.md GAP-47) and CI had no objective gate for
 * file-size drift.
 *
 * Policy (a ratchet, not a freeze):
 *   - Any `src/**` `.ts` or `.vue` file <= MAX_LOC nonblank lines is always fine.
 *   - New files above MAX_LOC fail. Split them or earn an allowlist entry.
 *   - Known hotspots are grandfathered in scripts/loc-baseline.json with the
 *     LOC measured at baseline time. They may shrink freely but may NOT grow
 *     past their recorded ceiling — existing debt can only get better.
 *   - A baselined file that drops to <= MAX_LOC (or is deleted) makes its
 *     entry stale; the guard fails so the baseline stays honest and minimal.
 *
 * Usage:
 *   node scripts/check-loc.mjs            # verify (CI + local)
 *   node scripts/check-loc.mjs --update   # rewrite the baseline from current LOC
 *   node scripts/check-loc.mjs --json     # machine-readable report on failure
 *
 * Output is intentionally short so an agent can act on it without opening CI
 * logs.
 */

import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");
const SRC_DIR = join(ROOT, "src");
const BASELINE_PATH = join(__dirname, "loc-baseline.json");

const DEFAULT_REASON =
  "Grandfathered hotspot (docs/Gaps.md GAP-47). Shrink only; split when " +
  "next touched.";

const args = process.argv.slice(2);
const update = args.includes("--update");
const asJson = args.includes("--json");

function toPosix(path) {
  return path.split(sep).join("/");
}

/** Nonblank LOC, matching `grep -cve '^[[:space:]]*$'`. */
function countLoc(absPath) {
  const text = readFileSync(absPath, "utf8");
  let count = 0;
  for (const line of text.split(/\r?\n/)) {
    if (line.trim() !== "") count++;
  }
  return count;
}

function collectSourceFiles(dir, acc = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) {
      collectSourceFiles(abs, acc);
    } else if (
      entry.isFile() &&
      (entry.name.endsWith(".ts") || entry.name.endsWith(".vue"))
    ) {
      acc.push(abs);
    }
  }
  return acc;
}

function readBaseline() {
  try {
    return JSON.parse(readFileSync(BASELINE_PATH, "utf8"));
  } catch {
    return { maxLoc: 500, description: "", allowlist: {} };
  }
}

const baseline = readBaseline();
const MAX_LOC = baseline.maxLoc ?? 500;

const files = collectSourceFiles(SRC_DIR)
  .map((abs) => ({ path: toPosix(relative(ROOT, abs)), loc: countLoc(abs) }))
  .sort((a, b) => b.loc - a.loc);

const overCap = files.filter((f) => f.loc > MAX_LOC);

if (update) {
  const allowlist = {};
  for (const { path, loc } of overCap) {
    allowlist[path] = {
      loc,
      reason: baseline.allowlist?.[path]?.reason ?? DEFAULT_REASON,
    };
  }
  const next = {
    maxLoc: MAX_LOC,
    description:
      "Grandfathered source files above maxLoc nonblank LOC. Entries may " +
      "shrink but not grow; new files above maxLoc are rejected. " +
      "Regenerate with `npm run check:loc -- --update`.",
    allowlist,
  };
  writeFileSync(BASELINE_PATH, JSON.stringify(next, null, 2) + "\n");
  console.log(
    `Updated ${toPosix(relative(ROOT, BASELINE_PATH))}: ` +
      `${overCap.length} file(s) above ${MAX_LOC} LOC.`,
  );
  process.exit(0);
}

const allowlist = baseline.allowlist ?? {};
const newOverCap = [];
const grown = [];
const seen = new Set();

for (const { path, loc } of overCap) {
  const entry = allowlist[path];
  if (!entry) {
    newOverCap.push({ path, loc });
    continue;
  }
  seen.add(path);
  if (loc > entry.loc) {
    grown.push({ path, loc, ceiling: entry.loc });
  }
}

// Stale entries: allowlisted files that no longer exist or no longer exceed
// the cap. Keeping them would let the file silently regrow up to the stale
// ceiling, so we force a baseline refresh instead.
const overCapPaths = new Set(overCap.map((f) => f.path));
const stale = Object.keys(allowlist).filter((p) => !overCapPaths.has(p));

const problems = [];
if (newOverCap.length > 0) {
  problems.push(
    `New source file(s) above the ${MAX_LOC} LOC cap (split them, or ` +
      `allowlist with a reason in scripts/loc-baseline.json):`,
  );
  for (const { path, loc } of newOverCap) problems.push(`  ${loc}  ${path}`);
}
if (grown.length > 0) {
  problems.push(
    "Allowlisted hotspot(s) grew past their recorded ceiling (shrink only):",
  );
  for (const { path, loc, ceiling } of grown) {
    problems.push(`  ${loc} (was ${ceiling})  ${path}`);
  }
}
if (stale.length > 0) {
  problems.push(
    `Stale baseline entr${stale.length === 1 ? "y" : "ies"} (file is now ` +
      `<= ${MAX_LOC} LOC or gone — run \`npm run check:loc -- --update\`):`,
  );
  for (const path of stale) problems.push(`  ${path}`);
}

if (problems.length === 0) {
  console.log(
    `LOC guard OK: ${files.length} files, cap ${MAX_LOC}, ` +
      `${seen.size} grandfathered hotspot(s).`,
  );
  process.exit(0);
}

if (asJson) {
  console.error(
    JSON.stringify({ maxLoc: MAX_LOC, newOverCap, grown, stale }, null, 2),
  );
} else {
  console.error("LOC guard FAILED:\n" + problems.join("\n"));
}
process.exit(1);
