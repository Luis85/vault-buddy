#!/usr/bin/env node
/**
 * Quality ratchet: fail when a fallow codebase metric regresses past the
 * committed baseline.
 *
 * Why this exists: dead code, duplication, and complexity regress silently
 * between reviews without an objective gate (docs/Gaps.md § Tech debt names
 * the current backlog). fallow's own gate flags do not reliably drive the
 * exit code, so this wrapper parses `fallow --format json` and applies the
 * same ratchet policy as scripts/check-loc.mjs.
 *
 * Policy (a ratchet, not a freeze):
 *   - Counter metrics (dead-code issues, clone groups, duplicated lines,
 *     complexity findings) may shrink freely but may NOT grow past the
 *     baseline — existing debt can only get better.
 *   - Floor metrics (average maintainability) may rise freely but may NOT
 *     drop below the baseline.
 *   - When a metric improves, lock the gain in: run `--update` in the same PR
 *     so the next regression is caught at the new level. Improvements print a
 *     reminder but do not fail.
 *
 * Gotcha: run with no `coverage/` directory present (CI orders this gate
 * before `test:coverage`) — a stray coverage report flips fallow's complexity
 * weighting from static estimation to coverage-weighted CRAP and shifts the
 * numbers.
 *
 * Usage:
 *   node scripts/check-quality.mjs            # verify (CI + local)
 *   node scripts/check-quality.mjs --update   # rewrite the baseline from current metrics
 *   node scripts/check-quality.mjs --json     # machine-readable report on failure
 */

import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, "..");
const BASELINE_PATH = join(__dirname, "quality-baseline.json");

const require = createRequire(import.meta.url);

const args = process.argv.slice(2);
const update = args.includes("--update");
const asJson = args.includes("--json");

// direction 'max': counter that may only shrink. direction 'min': floor that
// may only rise. `read` pulls the value out of fallow's combined JSON report.
const METRICS = {
  deadCodeIssues: {
    direction: "max",
    read: (r) => r.check.summary.total_issues,
    label: "dead-code issues (fallow dead-code)",
  },
  // Structural gates. All three are 0 today and must stay 0: the ratchet
  // direction allows it, but treat any bump of these baselines as an
  // architecture decision, not a metric trade-off.
  circularDependencies: {
    direction: "max",
    read: (r) => r.check.summary.circular_dependencies,
    label: "circular dependencies (fallow dead-code)",
  },
  reExportCycles: {
    direction: "max",
    read: (r) => r.check.summary.re_export_cycles,
    label: "re-export cycles (fallow dead-code)",
  },
  boundaryViolations: {
    direction: "max",
    read: (r) => r.check.summary.boundary_violations,
    label: "architecture boundary violations (fallow dead-code)",
  },
  cloneGroups: {
    direction: "max",
    read: (r) => r.dupes.stats.clone_groups,
    label: "clone groups (fallow dupes)",
  },
  duplicatedLines: {
    direction: "max",
    read: (r) => r.dupes.stats.duplicated_lines,
    label: "duplicated lines (fallow dupes)",
  },
  complexFunctions: {
    direction: "max",
    read: (r) => r.health.summary.functions_above_threshold,
    label: "functions above complexity threshold (fallow health)",
  },
  criticalComplexity: {
    direction: "max",
    read: (r) => r.health.summary.severity_critical_count,
    label: "critical-severity complexity findings (fallow health)",
  },
  averageMaintainability: {
    direction: "min",
    read: (r) => r.health.summary.average_maintainability,
    label: "average maintainability score (fallow health)",
    // Reported with one decimal; tolerate float noise below display precision.
    epsilon: 0.05,
  },
};

function toPosix(path) {
  return path.split(sep).join("/");
}

function runFallow() {
  const bin = require.resolve("fallow/bin/fallow");
  const stdout = execFileSync(
    process.execPath,
    [bin, "--quiet", "--format", "json"],
    {
      cwd: ROOT,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
      // fallow exits non-zero when findings exist; the report is still complete.
      stdio: ["ignore", "pipe", "inherit"],
    },
  );
  return JSON.parse(stdout);
}

let report;
try {
  report = runFallow();
} catch (err) {
  if (err.stdout) {
    try {
      report = JSON.parse(err.stdout);
    } catch {
      report = null;
    }
  }
  if (!report) {
    console.error(
      "Quality ratchet ERROR: fallow did not produce a JSON report.",
    );
    console.error(String(err.message ?? err));
    process.exit(2);
  }
}

const current = {};
for (const [name, metric] of Object.entries(METRICS)) {
  const value = metric.read(report);
  if (typeof value !== "number" || Number.isNaN(value)) {
    console.error(
      `Quality ratchet ERROR: could not read "${name}" from the fallow report ` +
        "(fallow JSON schema changed?).",
    );
    process.exit(2);
  }
  current[name] = value;
}

if (update) {
  const next = {
    description:
      "Fallow quality baseline. Counters may shrink but not grow; " +
      "averageMaintainability may rise but not drop. Regenerate with " +
      "`npm run check:quality -- --update` (commit the diff in the same PR " +
      "that moves the metric).",
    fallowVersion: report.version ?? "unknown",
    metrics: current,
  };
  writeFileSync(BASELINE_PATH, JSON.stringify(next, null, 2) + "\n");
  console.log(
    `Updated ${toPosix(relative(ROOT, BASELINE_PATH))}: ` +
      Object.entries(current)
        .map(([k, v]) => `${k}=${v}`)
        .join(", "),
  );
  process.exit(0);
}

let baseline;
try {
  baseline = JSON.parse(readFileSync(BASELINE_PATH, "utf8"));
} catch {
  console.error(
    "Quality ratchet ERROR: missing or unreadable scripts/quality-baseline.json. " +
      "Generate it with `npm run check:quality -- --update`.",
  );
  process.exit(2);
}

const regressions = [];
const improvements = [];

for (const [name, metric] of Object.entries(METRICS)) {
  const base = baseline.metrics?.[name];
  if (typeof base !== "number") {
    regressions.push({
      name,
      message: `  ${name}: missing from baseline — run \`npm run check:quality -- --update\``,
    });
    continue;
  }
  const value = current[name];
  const epsilon = metric.epsilon ?? 0;
  if (metric.direction === "max") {
    if (value > base) {
      regressions.push({
        name,
        value,
        base,
        message: `  ${name}: ${value} (baseline ${base}) — ${metric.label}`,
      });
    } else if (value < base) {
      improvements.push(`  ${name}: ${value} (baseline ${base})`);
    }
  } else {
    if (value < base - epsilon) {
      regressions.push({
        name,
        value,
        base,
        message: `  ${name}: ${value} (baseline floor ${base}) — ${metric.label}`,
      });
    } else if (value > base + epsilon) {
      improvements.push(`  ${name}: ${value} (baseline ${base})`);
    }
  }
}

if (regressions.length > 0) {
  if (asJson) {
    console.error(
      JSON.stringify(
        { regressions, current, baseline: baseline.metrics },
        null,
        2,
      ),
    );
  } else {
    console.error(
      "Quality ratchet FAILED — metric(s) regressed past the baseline:",
    );
    for (const r of regressions) console.error(r.message);
    console.error(
      "\nFix the regression (run `npm run quality` for details), or — only for a " +
        "deliberate, reviewed trade-off — bump the baseline with " +
        "`npm run check:quality -- --update` and justify it in the PR.",
    );
  }
  process.exit(1);
}

const summary = Object.entries(current)
  .map(([k, v]) => `${k}=${v}`)
  .join(", ");
console.log(`Quality ratchet OK: ${summary}.`);
if (improvements.length > 0) {
  console.log(
    "Improvement(s) not yet locked in — run `npm run check:quality -- --update` " +
      "and commit the baseline so the gain cannot regress:",
  );
  for (const line of improvements) console.log(line);
}
process.exit(0);
