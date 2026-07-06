# Version Bump Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Automate the mechanical version-bump step of the release process: one script updates all 5 version locations safely, a GitHub Actions workflow runs it on dispatch and opens a review PR, and the docs point at both instead of the old by-hand instructions.

**Architecture:** A dependency-free Node script (`scripts/bump-version.mjs`) does anchored text substitution across the 5 files (never a full JSON/TOML re-serialize, which would reformat the whole file). It's verified end-to-end by a Vitest test that runs it as a real child process against temp fixture files. A `workflow_dispatch` GitHub Actions workflow wraps the same script and opens a PR with the result. `AGENTS.md` / `docs/DEVELOPMENT.md` are updated to point at both instead of the manual by-hand instructions.

**Tech Stack:** Node 22 (plain `.mjs`, zero dependencies), Vitest (`node:child_process` + `node:fs` against temp fixtures, no DOM needed), GitHub Actions (`workflow_dispatch`, `gh` CLI).

## Global Constraints

- The script is a plain, dependency-free Node `.mjs` file, matching the existing `scripts/make-icon.mjs` convention (no shebang, no npm dependencies).
- Only strict `X.Y.Z` semver is accepted for an explicit version — no prerelease/build metadata (every past release is plain `X.Y.Z`).
- Only the **app** version is in scope: `package.json`, `package-lock.json` (2 occurrences), `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (the `vault-buddy` package entry). The three workspace crates (`vault_buddy_core`, `vault_buddy_capture`, `vault_buddy_transcribe`) keep their own independent `0.1.0` and must never be touched.
- Before writing anything, all 5 files must already agree on the *current* version (read from `package.json`) — if any file has drifted, abort with no writes, naming the offending file(s).
- No automatic git tag, push, or triggering of `release.yml`. The new workflow's job stops at opening a PR; tagging/publishing stays the existing manual step documented in `AGENTS.md`/`docs/DEVELOPMENT.md`.
- Conventional Commits with scopes per `AGENTS.md` (`feat`, `fix`, `chore`, `docs`, `ci`, ...); this repo practices TDD — write the failing test before the implementation.
- The new GitHub Actions workflow triggers only on `workflow_dispatch`, must refuse to run off any ref but `main`, needs `permissions: contents: write` + `pull-requests: write`, and uses the `gh` CLI (preinstalled on GitHub-hosted runners) rather than adding a new third-party marketplace action.

---

### Task 1: `scripts/bump-version.mjs` — the bump script, tested end-to-end

**Files:**
- Create: `scripts/bump-version.mjs`
- Create: `tests/bump-version.test.ts`
- Modify: `package.json` (add an npm script alias)

**Interfaces:**
- Produces: a CLI, `node scripts/bump-version.mjs <arg>` where `arg` is one of an explicit `X.Y.Z` version, a bump keyword (`patch`/`minor`/`major`), or `--check`.
  - Exit code `0` on success, non-zero on any failure.
  - On a successful bump, stdout contains the line `Bumped version: <old> -> <new>` followed by one `  - <file>` line per changed file.
  - On a successful `--check`, stdout contains `OK: all files agree on version <version>`.
  - On failure, stderr contains a human-readable reason: `Usage: bump-version.mjs ...` (wrong arg count), `Invalid version "<arg>": expected X.Y.Z or one of patch/minor/major` (bad explicit version), `Refusing to bump: files disagree on the current version (<version>):` or `Version drift detected:` (drift), each followed by `  - <file>: ...` lines describing which file(s) disagree.
- Produces: an npm alias, `npm run bump-version -- <arg>` (equivalent to the CLI above).
- Consumed by: Task 2 (the workflow runs `node scripts/bump-version.mjs "${{ inputs.version }}"` directly) and Task 3 (the docs describe both the CLI and the npm alias).

- [ ] **Step 1: Write the failing test file**

Create `tests/bump-version.test.ts`:

```ts
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { spawnSync } from "node:child_process";

const SCRIPT_SRC = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../scripts/bump-version.mjs",
);

// One fixture per file the real release process touches, parameterized by
// version so tests can set up a consistent starting point and then drift
// one file to exercise the safety check. Each fixture also carries an
// unrelated "version" field that happens to share the same value, to prove
// the script's anchors don't spill onto it.
const FIXTURES: Record<string, (version: string) => string> = {
  "package.json": (version) =>
    JSON.stringify({ name: "vault-buddy", private: true, version }, null, 2) + "\n",
  "package-lock.json": (version) =>
    JSON.stringify(
      {
        name: "vault-buddy",
        version,
        lockfileVersion: 3,
        requires: true,
        packages: {
          "": {
            name: "vault-buddy",
            version,
            dependencies: { "some-dep": `^${version}` },
          },
          "node_modules/some-dep": {
            version,
            resolved: `https://example.com/some-dep-${version}.tgz`,
          },
        },
      },
      null,
      2,
    ) + "\n",
  "src-tauri/tauri.conf.json": (version) =>
    JSON.stringify(
      {
        $schema: "https://schema.tauri.app/config/2",
        productName: "Vault Buddy",
        version,
        identifier: "com.vaultbuddy.desktop",
      },
      null,
      2,
    ) + "\n",
  "src-tauri/Cargo.toml": (version) =>
    [
      "[package]",
      'name = "vault-buddy"',
      `version = "${version}"`,
      'description = "test fixture"',
      'edition = "2021"',
      "",
      "[dependencies]",
      `some-crate = { version = "${version}" }`,
      "",
    ].join("\n"),
  "src-tauri/Cargo.lock": (version) =>
    [
      "[[package]]",
      'name = "some-crate"',
      `version = "${version}"`,
      "dependencies = [",
      "]",
      "",
      "[[package]]",
      'name = "vault-buddy"',
      `version = "${version}"`,
      "dependencies = [",
      ' "some-crate",',
      "]",
      "",
    ].join("\n"),
};

function writeFixtures(dir: string, version: string): void {
  for (const [relPath, render] of Object.entries(FIXTURES)) {
    const filePath = path.join(dir, relPath);
    mkdirSync(path.dirname(filePath), { recursive: true });
    writeFileSync(filePath, render(version));
  }
  mkdirSync(path.join(dir, "scripts"), { recursive: true });
  copyFileSync(SCRIPT_SRC, path.join(dir, "scripts", "bump-version.mjs"));
}

function readFixture(dir: string, relPath: string): string {
  return readFileSync(path.join(dir, relPath), "utf8");
}

function runScript(dir: string, args: string[]) {
  return spawnSync(process.execPath, [path.join(dir, "scripts", "bump-version.mjs"), ...args], {
    cwd: dir,
    encoding: "utf8" as const,
  });
}

describe("bump-version.mjs", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(path.join(tmpdir(), "bump-version-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("errors with a usage message when called with the wrong number of arguments", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, []);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("Usage: bump-version.mjs");
  });

  it("--check reports OK and exits 0 when all files agree", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, ["--check"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("OK: all files agree on version 1.2.3");
  });

  it("--check exits non-zero and names the offending file when versions have drifted", () => {
    writeFixtures(dir, "1.2.3");
    writeFileSync(
      path.join(dir, "src-tauri/tauri.conf.json"),
      FIXTURES["src-tauri/tauri.conf.json"]("1.2.4"),
    );
    const result = runScript(dir, ["--check"]);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("src-tauri/tauri.conf.json");
  });

  it("bumps all 5 files to an explicit version, leaving unrelated version fields untouched", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, ["1.3.0"]);
    expect(result.status).toBe(0);
    expect(result.stdout).toContain("1.2.3 -> 1.3.0");

    expect(JSON.parse(readFixture(dir, "package.json")).version).toBe("1.3.0");

    const lock = JSON.parse(readFixture(dir, "package-lock.json"));
    expect(lock.version).toBe("1.3.0");
    expect(lock.packages[""].version).toBe("1.3.0");
    expect(lock.packages["node_modules/some-dep"].version).toBe("1.2.3");

    expect(JSON.parse(readFixture(dir, "src-tauri/tauri.conf.json")).version).toBe("1.3.0");

    const cargoToml = readFixture(dir, "src-tauri/Cargo.toml");
    expect(cargoToml).toContain('name = "vault-buddy"\nversion = "1.3.0"');
    expect(cargoToml).toContain('some-crate = { version = "1.2.3" }');

    const cargoLock = readFixture(dir, "src-tauri/Cargo.lock");
    expect(cargoLock).toContain('name = "vault-buddy"\nversion = "1.3.0"');
    expect(cargoLock).toContain('name = "some-crate"\nversion = "1.2.3"');
  });

  it("computes the next version for patch, minor, and major keywords", () => {
    writeFixtures(dir, "1.2.3");
    expect(runScript(dir, ["patch"]).stdout).toContain("1.2.3 -> 1.2.4");

    writeFixtures(dir, "1.2.3");
    expect(runScript(dir, ["minor"]).stdout).toContain("1.2.3 -> 1.3.0");

    writeFixtures(dir, "1.2.3");
    expect(runScript(dir, ["major"]).stdout).toContain("1.2.3 -> 2.0.0");
  });

  it("rejects an invalid explicit version and writes nothing", () => {
    writeFixtures(dir, "1.2.3");
    const result = runScript(dir, ["1.2"]);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain('Invalid version "1.2"');
    expect(JSON.parse(readFixture(dir, "package.json")).version).toBe("1.2.3");
  });

  it("refuses to bump when the files have already drifted, and writes nothing", () => {
    writeFixtures(dir, "1.2.3");
    const drifted = readFixture(dir, "src-tauri/Cargo.toml").replace(
      'version = "1.2.3"',
      'version = "9.9.9"',
    );
    writeFileSync(path.join(dir, "src-tauri/Cargo.toml"), drifted);

    const result = runScript(dir, ["1.3.0"]);
    expect(result.status).not.toBe(0);
    expect(result.stderr).toContain("Refusing to bump");
    expect(JSON.parse(readFixture(dir, "package.json")).version).toBe("1.2.3");
  });
});
```

- [ ] **Step 2: Run the test file and confirm it fails**

Run: `npx vitest run tests/bump-version.test.ts`
Expected: FAIL — every test errors, because `writeFixtures` tries to `copyFileSync` a `scripts/bump-version.mjs` that doesn't exist yet (`ENOENT: no such file or directory, copyfile '.../scripts/bump-version.mjs'`).

- [ ] **Step 3: Implement the script**

Create `scripts/bump-version.mjs`:

```js
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
```

- [ ] **Step 4: Run the test file and confirm it passes**

Run: `npx vitest run tests/bump-version.test.ts`
Expected: PASS — all 7 tests green.

- [ ] **Step 5: Add the npm script alias**

Modify `package.json` — add `"bump-version"` to the `"scripts"` object (after `"test-build"`):

```json
  "scripts": {
    "dev": "vite",
    "build": "vue-tsc --noEmit && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "tauri": "tauri",
    "test-build": "tauri dev",
    "bump-version": "node scripts/bump-version.mjs"
  },
```

- [ ] **Step 6: Typecheck the new test file**

Run: `npm run build`
Expected: PASS — `vue-tsc --noEmit` reports no errors (this also typechecks `tests/**/*.ts` per `tsconfig.json`'s `include`), then the production build completes.

- [ ] **Step 7: Smoke-test against the real repo (read-only)**

Run: `npm run bump-version -- --check`
Expected: `OK: all files agree on version 0.3.0` and exit code 0 — confirms the script correctly reads the real 5 files, not just fixtures. This is `--check`, so it writes nothing; no need to revert anything after.

- [ ] **Step 8: Run the full test suite**

Run: `npm test`
Expected: PASS — every existing test still passes alongside the new `tests/bump-version.test.ts`.

- [ ] **Step 9: Commit**

```bash
git add scripts/bump-version.mjs tests/bump-version.test.ts package.json
git commit -m "feat(release): add bump-version script for the app version bump

Anchored text substitution across package.json, package-lock.json,
tauri.conf.json, Cargo.toml, and Cargo.lock; refuses to run if the 5
files have already drifted apart on version. Supports an explicit
X.Y.Z or a patch/minor/major keyword, plus a --check verify-only mode."
```

---

### Task 2: `.github/workflows/bump-version.yml` — dispatchable CI wrapper

**Files:**
- Create: `.github/workflows/bump-version.yml`

**Interfaces:**
- Consumes: Task 1's CLI contract — runs `node scripts/bump-version.mjs "${{ inputs.version }}"` directly (no `npm ci`, since the script has zero dependencies).
- Produces: a branch `chore/bump-version-v<version>` and a PR titled `chore(release): v<version>` against `main`, containing exactly the 5 files Task 1's `TARGETS` list touches.

- [ ] **Step 1: Create the workflow file**

Create `.github/workflows/bump-version.yml`:

```yaml
name: Bump version

# Manually dispatched: bumps the app version across package.json,
# package-lock.json, tauri.conf.json, Cargo.toml, and Cargo.lock (see
# scripts/bump-version.mjs) and opens a PR with the result. Tagging and
# publishing stay separate, manual steps (see AGENTS.md "Releases") — this
# only automates the file-edit step that precedes them.
on:
  workflow_dispatch:
    inputs:
      version:
        description: "New version (X.Y.Z) or a bump keyword: patch | minor | major"
        required: true

permissions:
  contents: write
  pull-requests: write

jobs:
  bump:
    name: Bump version and open a PR
    runs-on: ubuntu-latest
    steps:
      - name: Require dispatch from main
        if: github.ref_name != 'main'
        run: |
          echo "::error::Dispatch this workflow from main (was: ${{ github.ref_name }})"
          exit 1

      - uses: actions/checkout@v4

      - uses: actions/setup-node@v4
        with:
          node-version: 22

      - name: Bump version
        run: node scripts/bump-version.mjs "${{ inputs.version }}"

      - name: Resolve new version
        id: version
        run: echo "value=$(node -p "require('./package.json').version")" >> "$GITHUB_OUTPUT"

      - name: Commit and push
        env:
          VERSION: ${{ steps.version.outputs.value }}
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git checkout -b "chore/bump-version-v${VERSION}"
          git add package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
          git commit -m "chore(release): v${VERSION}"
          git push -u origin "chore/bump-version-v${VERSION}"

      - name: Open pull request
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          VERSION: ${{ steps.version.outputs.value }}
        run: |
          cat > /tmp/pr-body.md <<EOF
          Bumps the app version to v${VERSION} across package.json, package-lock.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml, and src-tauri/Cargo.lock.

          Next: merge this PR, then either push a \`v${VERSION}\` tag or dispatch the Release workflow with \`tag=v${VERSION}\`.
          EOF
          gh pr create \
            --base main \
            --head "chore/bump-version-v${VERSION}" \
            --title "chore(release): v${VERSION}" \
            --body-file /tmp/pr-body.md
```

- [ ] **Step 2: Validate the YAML parses**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/bump-version.yml')); print('valid YAML')"`
Expected: prints `valid YAML`, no exception.

- [ ] **Step 3: Cross-check against Task 1's file list**

Open `scripts/bump-version.mjs` and confirm the `git add` line in the workflow lists exactly the 5 `file` values from `TARGETS` (`package.json`, `package-lock.json`, `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`) — no more, no fewer. This is a manual read-through, not an automated check: workflow YAML has no test harness in this repo.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/bump-version.yml
git commit -m "ci(release): add a dispatchable workflow to bump the version and open a PR

Wraps scripts/bump-version.mjs: takes an X.Y.Z or patch/minor/major
input, commits the 5-file bump to a new branch, and opens a PR against
main. Tagging/publishing stay manual, unchanged."
```

---

### Task 3: Docs — point AGENTS.md and docs/DEVELOPMENT.md at the script and workflow

**Files:**
- Modify: `AGENTS.md:449-461` (the `## Releases` section)
- Modify: `docs/DEVELOPMENT.md:108-117` (the `## Releases` section)

**Interfaces:**
- Consumes: Task 1's npm alias name (`npm run bump-version --`) and Task 2's workflow name (`Bump version`) and its `version` input contract — purely descriptive, no code dependency.

- [ ] **Step 1: Update `AGENTS.md`**

In `AGENTS.md`, replace:

```markdown
Release = version bump in `package.json`, `src-tauri/tauri.conf.json`, and
`src-tauri/Cargo.toml` (+ both lockfiles) on `main`, then either push a
`v*` tag **or** dispatch the Release workflow with the tag as input
```

with:

```markdown
Release = version bump in `package.json`, `src-tauri/tauri.conf.json`, and
`src-tauri/Cargo.toml` (+ both lockfiles) on `main` — run
`npm run bump-version -- <version|patch|minor|major>`
(`scripts/bump-version.mjs`) rather than editing the five files by hand; it
refuses to run if they've already drifted apart. The `Bump version` GitHub
Actions workflow (`workflow_dispatch`) runs the same script from `main` and
opens a PR with the result, for bumping without a local checkout. Once the
bump lands on `main`, either push a
`v*` tag **or** dispatch the Release workflow with the tag as input
```

(The rest of the paragraph — `gh workflow run release.yml -f tag=vX.Y.Z` onward — is unchanged.)

- [ ] **Step 2: Update `docs/DEVELOPMENT.md`**

In `docs/DEVELOPMENT.md`, replace:

```markdown
```bash
# after bumping the version in tauri.conf.json / package.json / Cargo.toml
git tag v0.1.0 && git push origin v0.1.0
```
```

with:

```markdown
```bash
npm run bump-version -- 0.1.0    # or: patch | minor | major
git tag v0.1.0 && git push origin v0.1.0
```

Prefer not to check out the repo locally? Dispatch the **Bump version**
workflow from the [Actions](https://github.com/Luis85/vault-buddy/actions)
tab (`version` input takes an explicit `X.Y.Z` or `patch`/`minor`/`major`) —
it runs `scripts/bump-version.mjs` on `main` and opens a PR with the version
bump for you to review and merge before tagging.
```

- [ ] **Step 3: Verify no stale instructions remain**

Run: `grep -rn "after bumping the version" AGENTS.md docs/DEVELOPMENT.md`
Expected: no output (both occurrences replaced).

Run: `grep -n "bump-version" AGENTS.md docs/DEVELOPMENT.md`
Expected: at least one match in each file.

- [ ] **Step 4: Commit**

```bash
git add AGENTS.md docs/DEVELOPMENT.md
git commit -m "docs(release): point Releases sections at bump-version script and workflow

Replaces the by-hand version-bump instructions with npm run
bump-version and the new dispatchable Bump version workflow; tagging
and publishing instructions are unchanged."
```

---

### Task 4: Full verification pass

**Files:** none (verification only, no changes expected)

- [ ] **Step 1: Run the whole frontend test suite**

Run: `npm test`
Expected: PASS — all tests, including `tests/bump-version.test.ts`, green.

- [ ] **Step 2: Run the typecheck + production build**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 3: Re-run the real-repo smoke check**

Run: `npm run bump-version -- --check`
Expected: `OK: all files agree on version 0.3.0` (or whatever `package.json` currently holds) — confirms Task 1–3's changes haven't touched the actual version files.

- [ ] **Step 4: Confirm the working tree is clean apart from the intended commits**

Run: `git status --short`
Expected: empty (everything from Tasks 1–3 already committed; Step 3 above is read-only via `--check`).

No commit for this task — it's a verification gate, not a change.
