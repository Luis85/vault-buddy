// Bumps the Vault Buddy app version across every file the release process
// touches (see AGENTS.md "Releases"). Refuses to run if those files have
// already drifted apart on version, so it never "fixes" a pre-existing
// inconsistency silently.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const SEMVER_RE = /^\d+\.\d+\.\d+$/;
const BUMP_KEYWORDS = ["patch", "minor", "major"];

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// One entry per file the release process touches. Each `regex` anchors on
// this project's own package name next to "version", so it can never match
// an unrelated dependency's version field that happens to share the same
// version string (package-lock.json/Cargo.lock list dozens of those).
const TARGETS = [
  {
    file: "package.json",
    expectedCount: 1,
    regex: (v) => new RegExp(`"version": "${escapeRegExp(v)}"`, "g"),
    replacement: (v) => `"version": "${v}"`,
  },
  {
    file: "package-lock.json",
    expectedCount: 2,
    regex: (v) =>
      new RegExp(`("name": "vault-buddy",\\n\\s*"version": ")${escapeRegExp(v)}(")`, "g"),
    replacement: (v) => `$1${v}$2`,
  },
  {
    file: "src-tauri/tauri.conf.json",
    expectedCount: 1,
    regex: (v) => new RegExp(`"version": "${escapeRegExp(v)}"`, "g"),
    replacement: (v) => `"version": "${v}"`,
  },
  {
    file: "src-tauri/Cargo.toml",
    expectedCount: 1,
    regex: (v) => new RegExp(`(name = "vault-buddy"\\nversion = ")${escapeRegExp(v)}(")`, "g"),
    replacement: (v) => `$1${v}$2`,
  },
  {
    file: "src-tauri/Cargo.lock",
    expectedCount: 1,
    regex: (v) => new RegExp(`(name = "vault-buddy"\\nversion = ")${escapeRegExp(v)}(")`, "g"),
    replacement: (v) => `$1${v}$2`,
  },
];

function countMatches(content, regex) {
  const matches = content.match(regex);
  return matches ? matches.length : 0;
}

function currentVersion() {
  const pkg = JSON.parse(readFileSync(path.join(root, "package.json"), "utf8"));
  return pkg.version;
}

// Empty array means all 5 files agree on `version`.
function checkDrift(version) {
  const problems = [];
  for (const target of TARGETS) {
    const content = readFileSync(path.join(root, target.file), "utf8");
    const count = countMatches(content, target.regex(version));
    if (count !== target.expectedCount) {
      problems.push(
        `${target.file}: expected ${target.expectedCount} occurrence(s) of version ${version}, found ${count}`,
      );
    }
  }
  return problems;
}

function applyBump(oldVersion, newVersion) {
  const changed = [];
  for (const target of TARGETS) {
    const filePath = path.join(root, target.file);
    const content = readFileSync(filePath, "utf8");
    const updated = content.replace(target.regex(oldVersion), target.replacement(newVersion));
    writeFileSync(filePath, updated);
    changed.push(target.file);
  }
  return changed;
}

function nextVersion(current, keyword) {
  const [major, minor, patch] = current.split(".").map(Number);
  if (keyword === "major") return `${major + 1}.0.0`;
  if (keyword === "minor") return `${major}.${minor + 1}.0`;
  return `${major}.${minor}.${patch + 1}`;
}

function parseArg(argv) {
  if (argv.length !== 1) {
    throw new Error("Usage: bump-version.mjs <X.Y.Z|patch|minor|major> | --check");
  }
  return argv[0];
}

function resolveNewVersion(current, arg) {
  if (BUMP_KEYWORDS.includes(arg)) return nextVersion(current, arg);
  if (SEMVER_RE.test(arg)) return arg;
  throw new Error(`Invalid version "${arg}": expected X.Y.Z or one of patch/minor/major`);
}

function main() {
  let arg;
  try {
    arg = parseArg(process.argv.slice(2));
  } catch (err) {
    console.error(err.message);
    process.exitCode = 1;
    return;
  }

  const current = currentVersion();

  if (arg === "--check") {
    const problems = checkDrift(current);
    if (problems.length > 0) {
      console.error("Version drift detected:");
      problems.forEach((p) => console.error(`  - ${p}`));
      process.exitCode = 1;
      return;
    }
    console.log(`OK: all files agree on version ${current}`);
    return;
  }

  const driftProblems = checkDrift(current);
  if (driftProblems.length > 0) {
    console.error(`Refusing to bump: files disagree on the current version (${current}):`);
    driftProblems.forEach((p) => console.error(`  - ${p}`));
    process.exitCode = 1;
    return;
  }

  let newVersion;
  try {
    newVersion = resolveNewVersion(current, arg);
  } catch (err) {
    console.error(err.message);
    process.exitCode = 1;
    return;
  }

  const changed = applyBump(current, newVersion);
  console.log(`Bumped version: ${current} -> ${newVersion}`);
  changed.forEach((f) => console.log(`  - ${f}`));
}

main();
