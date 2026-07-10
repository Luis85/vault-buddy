#!/usr/bin/env node
/**
 * LOC guard: fail when a source file grows past the agreed ceiling.
 *
 * Why this exists: agentic contributors optimize for finishing the requested
 * change, which quietly grows already-large modules and creates new oversized
 * ones. The 2026-07-10 audit flagged the current hotspots (docs/Gaps.md
 * GAP-45/GAP-47) and CI had no objective gate for file-size drift.
 *
 * Policy (a ratchet, not a freeze):
 *   - Frontend: any .ts/.vue file under src/ <= 500 nonblank lines is fine.
 *   - Rust: any .rs file under an src-tauri crate's src/ <= 800 nonblank
 *     lines is fine — the higher cap exists because the repo convention
 *     keeps unit tests inline (#[cfg(test)] modules) in the same file.
 *   - New files above their cap fail. Split them or earn an allowlist entry.
 *   - Known hotspots are grandfathered in scripts/loc-baseline.json with the
 *     LOC measured at baseline time. They may shrink freely but may NOT grow
 *     past their recorded ceiling — existing debt can only get better.
 *   - A baselined file that drops to <= its cap (or is deleted) makes its
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
const BASELINE_PATH = join(__dirname, "loc-baseline.json");

// One entry per language surface. `filter` sees the repo-relative posix path.
const SURFACES = [
  {
    name: "frontend",
    dir: "src",
    defaultCap: 500,
    filter: (p) => p.endsWith(".ts") || p.endsWith(".vue"),
  },
  {
    name: "rust",
    dir: "src-tauri",
    defaultCap: 800,
    // Only crate sources; never build output.
    filter: (p) => p.endsWith(".rs") && p.includes("/src/"),
    skipDirs: new Set(["target", "gen", "icons", "capabilities"]),
  },
];

const DEFAULT_REASON =
  "Grandfathered hotspot (docs/Gaps.md GAP-45/GAP-47). Shrink only; split " +
  "when next touched.";

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

function collectFiles(surface, dir, acc = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (surface.skipDirs?.has(entry.name)) continue;
      collectFiles(surface, abs, acc);
    } else if (entry.isFile()) {
      const rel = toPosix(relative(ROOT, abs));
      if (surface.filter(rel)) acc.push({ path: rel, loc: countLoc(abs) });
    }
  }
  return acc;
}

function readBaseline() {
  try {
    return JSON.parse(readFileSync(BASELINE_PATH, "utf8"));
  } catch {
    return { caps: {}, description: "", allowlist: {} };
  }
}

const baseline = readBaseline();
const capFor = (surface) =>
  baseline.caps?.[surface.name] ?? surface.defaultCap;

const overCap = [];
let totalFiles = 0;
for (const surface of SURFACES) {
  const cap = capFor(surface);
  const files = collectFiles(surface, join(ROOT, surface.dir));
  totalFiles += files.length;
  for (const f of files) {
    if (f.loc > cap) overCap.push({ ...f, cap });
  }
}
overCap.sort((a, b) => b.loc - a.loc);

if (update) {
  const allowlist = {};
  for (const { path, loc } of overCap) {
    allowlist[path] = {
      loc,
      reason: baseline.allowlist?.[path]?.reason ?? DEFAULT_REASON,
    };
  }
  const next = {
    caps: Object.fromEntries(
      SURFACES.map((s) => [s.name, capFor(s)]),
    ),
    description:
      "Grandfathered source files above their surface's nonblank-LOC cap " +
      "(frontend: src/**.{ts,vue}; rust: src-tauri/**/src/**.rs — higher " +
      "cap because unit tests live inline). Entries may shrink but not " +
      "grow; new files above the cap are rejected. Regenerate with " +
      "`npm run check:loc -- --update`.",
    allowlist,
  };
  writeFileSync(BASELINE_PATH, JSON.stringify(next, null, 2) + "\n");
  console.log(
    `Updated ${toPosix(relative(ROOT, BASELINE_PATH))}: ` +
      `${overCap.length} file(s) above their cap.`,
  );
  process.exit(0);
}

const allowlist = baseline.allowlist ?? {};
const newOverCap = [];
const grown = [];
const seen = new Set();

for (const { path, loc, cap } of overCap) {
  const entry = allowlist[path];
  if (!entry) {
    newOverCap.push({ path, loc, cap });
    continue;
  }
  seen.add(path);
  if (loc > entry.loc) {
    grown.push({ path, loc, ceiling: entry.loc });
  }
}

// Stale entries: allowlisted files that no longer exist or no longer exceed
// their cap. Keeping them would let the file silently regrow up to the stale
// ceiling, so we force a baseline refresh instead.
const overCapPaths = new Set(overCap.map((f) => f.path));
const stale = Object.keys(allowlist).filter((p) => !overCapPaths.has(p));

const problems = [];
if (newOverCap.length > 0) {
  problems.push(
    "New source file(s) above their LOC cap (split them, or allowlist " +
      "with a reason in scripts/loc-baseline.json):",
  );
  for (const { path, loc, cap } of newOverCap) {
    problems.push(`  ${loc} (cap ${cap})  ${path}`);
  }
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
      "under its cap or gone — run `npm run check:loc -- --update`):",
  );
  for (const path of stale) problems.push(`  ${path}`);
}

if (problems.length === 0) {
  const caps = SURFACES.map((s) => `${s.name} ${capFor(s)}`).join(", ");
  console.log(
    `LOC guard OK: ${totalFiles} files (caps: ${caps}), ` +
      `${seen.size} grandfathered hotspot(s).`,
  );
  process.exit(0);
}

if (asJson) {
  console.error(JSON.stringify({ newOverCap, grown, stale }, null, 2));
} else {
  console.error("LOC guard FAILED:\n" + problems.join("\n"));
}
process.exit(1);
