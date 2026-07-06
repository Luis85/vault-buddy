# Version Bump Automation Design — "One script, one command, one PR"

- **Date:** 2026-07-06
- **Status:** Approved
- **Source:** Every release requires editing the version string in five
  places by hand (`package.json`, `package-lock.json` ×2, `src-tauri/tauri.conf.json`,
  `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock`) before tagging, as documented
  in `AGENTS.md` and `docs/DEVELOPMENT.md`. It's mechanical, easy to get
  inconsistent (see the exact diff of commit `9b62d28`, the v0.3.0 bump), and
  worth automating. This spec covers a small Node script that does the bump
  safely, plus wiring it into the docs and a dispatchable CI workflow so a
  release can be kicked off without a local checkout.

## Goal

`node scripts/bump-version.mjs <version>` updates all five version locations
in one atomic, verified step — never touching unrelated `"version"` fields
elsewhere in the lockfiles. A new `Bump version` GitHub Actions workflow runs
the same script on dispatch and opens a PR with the result, so bumping no
longer requires anyone to have Node/a checkout locally. Tagging and publishing
stay exactly as documented today (manual `git tag` + push, or dispatching
`release.yml`) — this only automates the file-edit step that currently
precedes it.

## Non-goals

- No automatic git tag, push, or triggering of `release.yml`. The bump
  workflow opens a PR; a human merges it and then tags, same two-step flow as
  today.
- No changes to the three workspace crates' own versions (`core`, `capture`,
  `transcribe` all stay at their independent `0.1.0`s) — only the app version
  (`vault-buddy` in `src-tauri/Cargo.toml`, `package.json`, `tauri.conf.json`)
  is in scope, matching what past release commits actually touched.
- No prerelease/build-metadata versions (`1.2.3-beta.1`); every past release
  is strict `X.Y.Z` and the script only supports that.

## `scripts/bump-version.mjs`

Plain Node `.mjs`, zero dependencies (matches `scripts/make-icon.mjs`'s
existing convention).

**CLI:**
```
node scripts/bump-version.mjs 0.4.0       # explicit version
node scripts/bump-version.mjs minor       # patch | minor | major, computed
node scripts/bump-version.mjs --check     # verify-only, no writes, no arg needed
```

**Source of truth:** `package.json`'s `"version"` is read first. For a bump
keyword, the next version is computed from it (patch/minor/major semver
increment, reset lower components to 0). For an explicit version, it must
match strict `X.Y.Z` (validated with a regex) or the script exits non-zero
with an error.

**Targets, and exactly how each is matched (text surgery, not
parse-and-reserialize, so formatting outside the version is untouched):**

| File | Match anchor |
| --- | --- |
| `package.json` | `"version": "<current>"` (single occurrence) |
| `package-lock.json` | `"name": "vault-buddy",\s*\n\s*"version": "<current>"` — matches **both** the root object and `packages[""]`, `replace_all`, so no other dependency's `"version"` field is ever touched regardless of whether it happens to share the same version string |
| `src-tauri/tauri.conf.json` | `"version": "<current>"` (single occurrence) |
| `src-tauri/Cargo.toml` | `name = "vault-buddy"\nversion = "<current>"` (anchored to the `[package]` block, not the `[workspace.dependencies]`-style version pins elsewhere in the file) |
| `src-tauri/Cargo.lock` | `name = "vault-buddy"\nversion = "<current>"` (the `vault-buddy` lockfile entry, distinct from `vault_buddy_core`/`_capture`/`_transcribe` entries which keep their own `0.1.0`) |

**Safety checks, in order:**
1. Read current version from `package.json`.
2. Before writing anything, confirm all five locations above currently
   contain that exact current version. If any file has drifted (doesn't
   contain the expected anchor+version), abort with no writes and an error
   naming the offending file — the script never "fixes" a pre-existing
   inconsistency silently.
3. Validate/compute the new version.
4. If `--check` was passed, stop here and report success/failure — this is
   the verify-only mode, usable standalone or later wired into CI.
5. Otherwise, apply all five replacements and print `old → new` plus the list
   of files changed.

**Exit codes:** `0` on success (including a successful `--check`), non-zero on
any validation failure, drift, or missing file — so it's safe to use in a
script/workflow with normal shell error propagation.

**`package.json` addition:** a `"bump-version": "node scripts/bump-version.mjs"`
npm script, so it's invoked as `npm run bump-version -- 0.4.0` or
`npm run bump-version -- --check`, consistent with the other `scripts` entries.

## Testing

`tests/bump-version.test.ts` (Vitest, runs the script as a child process
against temp copies of small fixture files, per the existing test
conventions):
- Bumping via explicit version updates all 5 fixture files correctly and
  leaves every other line byte-identical.
- Bumping via `patch`/`minor`/`major` computes the expected next version.
- An unrelated `"version"` field elsewhere in the lockfile fixtures (e.g. a
  dependency that happens to share the current app version string) is left
  untouched.
- A deliberately drifted fixture (one file with a different version) causes
  the script to abort with a non-zero exit and no writes anywhere.
- `--check` exits `0` when all fixtures agree and non-zero when they don't,
  without writing anything either way.
- An invalid explicit version (e.g. `1.2`, `v1.2.3`, `1.2.3-beta`) is
  rejected.

## Docs updates

- **`AGENTS.md`** "Releases" section: replace "version bump in `package.json`,
  `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml` (+ both lockfiles)"
  with a pointer to `npm run bump-version -- <version|patch|minor|major>`,
  and mention the `Bump version` workflow as the no-checkout alternative.
  Tagging/dispatch instructions after that are unchanged.
- **`docs/DEVELOPMENT.md`** "Releases" section: same replacement for the
  `# after bumping the version ...` comment above the `git tag` example, plus
  a short mention of the new workflow (link to the Actions tab).

## `.github/workflows/bump-version.yml`

- **Trigger:** `workflow_dispatch` with one required input, `version`
  (string) — same accepted forms as the script (`X.Y.Z` or
  `patch`/`minor`/`major`), described in the input's `description`.
- **Guard:** first step fails the run immediately if
  `github.ref_name != 'main'` (dispatch must target `main`, matching "on
  main" in the documented release flow).
- **Permissions:** `contents: write`, `pull-requests: write` (top-level, like
  `release.yml`'s `contents: write`).
- **Steps:**
  1. `actions/checkout@v4`
  2. `actions/setup-node@v4` (node 22) — no `npm ci`, the script has no
     dependencies
  3. `node scripts/bump-version.mjs "${{ inputs.version }}"`
  4. Resolve the concrete new version via
     `node -p "require('./package.json').version"` (works uniformly whether
     the input was explicit or a keyword) and expose it as a step output
  5. Create branch `chore/bump-version-v<version>`, commit the 5 changed
     files (`git add` the exact 5 paths, not `-A`) as
     `chore(release): v<version>`, push
  6. Open a PR via the `gh` CLI (preinstalled on GitHub-hosted runners — no
     new marketplace action to trust) with title `chore(release): v<version>`
     and a body that lists the changed files and reminds the merger of the
     next manual step (tag & push, or dispatch `release.yml` with the tag)
- If a PR/branch for that version already exists, the `gh pr create` /
  `git push` step fails loudly rather than silently overwriting — acceptable
  since this is a rare, human-triggered action.

## Open questions / risks

- None outstanding — scope, safety checks, and the workflow boundary were
  confirmed during brainstorming (script-only vs. also automating tag/publish
  was explicitly decided against, to keep the human checkpoint before a
  release actually ships).
