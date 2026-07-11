# Polish Sub-pass D — Security & Release Engineering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the security/release gap cluster: SHA-pinned actions (GAP-35), CI permissions + PR-unsigned builds (GAP-36), injection-safe bump workflow (GAP-37), validated + CI-gated releases (GAP-41/42), Windows `cargo test` (GAP-43), monotonic version bumps (GAP-44 item), and a restrictive CSP (GAP-34).

**Architecture:** Workflow edits are config-only and cannot execute locally — each task's verification is YAML validity + line-by-line review against the exact blocks in this plan, with CI's own run on the pushed branch as the live gate for `ci.yml` (release/bump workflows only execute on dispatch — say so honestly, never claim they ran). The two code changes (bump-version.mjs, CSP) are TDD'd/verified normally.

**Tech Stack:** GitHub Actions YAML, Node (bump script + Vitest), Tauri v2 config.

## Global Constraints

- **Branch:** `claude/task-management-vertical-slice-ikeuly`. Never push elsewhere; never amend/rebase existing commits.
- **Bookkeeping:** each task's Gaps.md entry tombstoned **in the same commit**, GAP-40 format, ENTRY KEPT (never deleted). Partial entries (GAP-43's remaining half, one GAP-44 bullet) get inline `(FIXED 2026-07-10 — …)` annotations instead of a struck heading — except GAP-43, whose heading already reads "clippy half FIXED"; when D-T5 lands, restructure its heading to fully FIXED with the body noting both halves.
- **SHA pinning rule (D2):** every third-party action in all three workflows is pinned to a FULL commit SHA with the tag as a trailing comment (`uses: actions/checkout@<sha> # v4`). Resolve SHAs live with `git ls-remote https://github.com/<owner>/<repo> '<ref>^{}'` (the peeled line is the commit for annotated tags; if no peeled line, the plain ref line IS the commit). Never copy a SHA from memory or from this plan.
- **Workflow verification floor:** after each workflow edit run `python3 -c "import yaml,sys; yaml.safe_load(open(sys.argv[1]))" <file>` (fall back to careful indentation review if PyYAML is absent — say which you did). Never claim a workflow "passed" unless CI actually ran it.
- **The CSP change (D1) ships behind a manual Windows verification note** — the Linux compile gate cannot prove the runtime policy; the plan's task says exactly what to write in Gaps.md/the report. One-line revert if the packaged app breaks.
- **Gates for code tasks:** Vitest for bump-version tests; `npx tauri build --no-bundle` for the CSP config change (Tauri validates the config at build); `npm run lint`/`check:loc` as usual.
- **Commits:** Conventional Commits (`ci(security)`, `ci(release)`, `fix(release)`, `feat(security)` — match repo history's `ci(release)` precedent), one per task, ending with the two trailers:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU`

---

### Task 1: Pin every third-party action to a commit SHA (D2 · GAP-35)

**Files:**
- Modify: `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `.github/workflows/bump-version.yml`
- Modify: `docs/Gaps.md` (GAP-35 tombstone)

The action set (verify by grepping `uses:` across the three files — this list must match what you find, flag any extra): `actions/checkout@v4`, `actions/setup-node@v4`, `dtolnay/rust-toolchain@stable`, `Swatinem/rust-cache@v2`, `taiki-e/install-action@v2`, `actions/upload-artifact@v4`, `tauri-apps/tauri-action@v0`.

- [ ] **Step 1: Resolve each SHA live**

For each action, run (example):

```bash
git ls-remote https://github.com/actions/checkout 'v4^{}' 'v4'
```

Take the `^{}`-peeled SHA when present, else the plain one. For `dtolnay/rust-toolchain@stable` the ref is a BRANCH: `git ls-remote https://github.com/dtolnay/rust-toolchain stable` — pin its head and comment it `# stable branch, pinned 2026-07-10`. For `tauri-apps/tauri-action@v0` resolve `v0^{}`/`v0` the same way (floating major tag). Record every resolved pair in your report.

- [ ] **Step 2: Rewrite every `uses:` line**

Format: `- uses: actions/checkout@<full-sha> # v4` (keep any existing `with:` blocks byte-identical). The `taiki-e/install-action` in ci.yml and `tauri-apps/tauri-action` in release.yml included.

- [ ] **Step 3: Validate YAML + self-check**

`python3 -c "import yaml,sys; yaml.safe_load(open(sys.argv[1]))" .github/workflows/ci.yml` (×3). `grep -n "uses:" .github/workflows/*.yml` must show zero un-pinned third-party refs (a bare `@vN` or `@stable` anywhere = incomplete).

- [ ] **Step 4: Tombstone GAP-35 + commit**

```bash
git add .github/workflows/ci.yml .github/workflows/release.yml .github/workflows/bump-version.yml docs/Gaps.md
git commit -m "ci(security): pin all third-party actions to commit SHAs" -m "GAP-35: every action was pinned by mutable tag — including tauri-apps/tauri-action, which receives TAURI_SIGNING_PRIVATE_KEY; a compromised tag exfiltrates the key that can ship updates to every installed app. All seven actions across the three workflows now pin full commit SHAs with the tag as a comment (dtolnay/rust-toolchain pins the stable branch head, dated)." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 2: CI permissions + unsigned PR builds (D3 · GAP-36)

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `docs/Gaps.md` (GAP-36 tombstone)

- [ ] **Step 1: Add the top-level permissions block**

Directly under `name: CI`:

```yaml
# Least privilege: every job only reads the repo. Nothing in CI pushes,
# comments, or releases (GAP-36).
permissions:
  contents: read
```

- [ ] **Step 2: Sign only on push to main**

In the `windows-app` job's "Build Tauri app and installers" step, the two env lines become conditional so same-repo PR builds run unsigned (the step's existing empty-key bash fallback already builds without updater artifacts):

```yaml
        env:
          # Signing material only on push builds (main): a same-repo PR
          # branch must not have the updater key in its environment while
          # running the branch's own npm ci / build.rs (GAP-36). Forked PRs
          # were already keyless; this makes ALL PR builds keyless. The
          # empty-key fallback below then skips updater artifacts.
          TAURI_SIGNING_PRIVATE_KEY: ${{ github.event_name == 'push' && secrets.TAURI_SIGNING_PRIVATE_KEY || '' }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ github.event_name == 'push' && secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD || '' }}
```

- [ ] **Step 3: Validate YAML; tombstone GAP-36; commit**

```bash
git add .github/workflows/ci.yml docs/Gaps.md
git commit -m "ci(security): least-privilege token; sign only push builds" -m "GAP-36: CI had no permissions block (default token scope) and the signing secrets were present during npm ci/build.rs for any same-repo PR branch. The token is now contents:read and the signing env is empty for every PR event — the existing keyless fallback builds those without updater artifacts, exactly like forks." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 3: Injection-safe bump workflow (D4 · GAP-37)

**Files:**
- Modify: `.github/workflows/bump-version.yml`
- Modify: `docs/Gaps.md` (GAP-37 tombstone)

- [ ] **Step 1: Route the dispatch input through env**

The "Bump version" step becomes:

```yaml
      - name: Bump version
        env:
          # Never interpolate a dispatch input into the script line — a
          # crafted input would run as shell with a contents:write token
          # (GAP-37). env + quoting keeps it data.
          REQUESTED_VERSION: ${{ inputs.version }}
        run: node scripts/bump-version.mjs "$REQUESTED_VERSION"
```

And the "Require dispatch from main" step's error line stops interpolating the ref into the shell for the same hygiene (ref names are user-influenced):

```yaml
      - name: Require dispatch from main
        if: github.ref_name != 'main'
        env:
          REF_NAME: ${{ github.ref_name }}
        run: |
          echo "::error::Dispatch this workflow from main (was: $REF_NAME)"
          exit 1
```

(The later steps already use `VERSION` via env from the script's own resolved output — leave them; note in the report that the branch name derives from the RESOLVED version, not the raw input.)

- [ ] **Step 2: Validate YAML; tombstone GAP-37; commit**

```bash
git add .github/workflows/bump-version.yml docs/Gaps.md
git commit -m "ci(security): pass the bump dispatch input via env, not interpolation" -m "GAP-37: \${{ inputs.version }} landed directly in a run: line — a workflow-command/shell injection vector for write-access users under a contents:write + pull-requests:write token. The input (and the ref-name error path) now travel via env and are quoted; downstream steps already used the script's resolved version." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 4: Release validation + CI-green gate (D5 · GAP-41, D6 · GAP-42)

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `docs/Gaps.md` (GAP-41 + GAP-42 tombstones)
- Modify: `AGENTS.md` § Releases (one sentence: the release validates tag↔version agreement and requires green CI for the SHA)

- [ ] **Step 1: Add the validate job**

After the `permissions:` block (which gains `actions: read` — needed to query workflow runs):

```yaml
permissions:
  contents: write
  # validate reads CI run conclusions for the released SHA (GAP-42)
  actions: read

jobs:
  validate:
    name: Validate tag, branch, and CI status
    runs-on: ubuntu-latest
    steps:
      - name: Require dispatch from main
        # Tag pushes carry refs/tags/*; the branch guard applies to the
        # dispatch path only (GAP-41 — dispatching from any branch used to
        # release that branch's code under an arbitrary tag).
        if: github.event_name == 'workflow_dispatch' && github.ref != 'refs/heads/main'
        env:
          REF: ${{ github.ref }}
        run: |
          echo "::error::Dispatch the release from main (was: $REF)"
          exit 1
      - uses: actions/checkout@<same pinned SHA as Task 1> # v4
      - name: Tag must match tauri.conf.json's version
        env:
          TAG: ${{ inputs.tag || github.ref_name }}
        run: |
          EXPECTED="v$(node -p "require('./src-tauri/tauri.conf.json').version")"
          if [ "$TAG" != "$EXPECTED" ]; then
            echo "::error::Tag $TAG does not match tauri.conf.json ($EXPECTED) — a mismatch ships a latest.json whose version disagrees with the tag (GAP-41)"
            exit 1
          fi
      - name: Require green CI for this commit
        env:
          GH_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          SHA: ${{ github.sha }}
        run: |
          # A tag on a red or unvalidated commit must not publish (GAP-42):
          # installed apps would be offered the broken build via the updater.
          # Works identically for tag-push and dispatch — github.sha is the
          # released commit either way.
          CONCLUSION=$(gh run list --repo "$GITHUB_REPOSITORY" --workflow CI --commit "$SHA" --json conclusion,status --jq '[.[] | select(.status == "completed")] | first | .conclusion' 2>/dev/null || echo "")
          if [ "$CONCLUSION" != "success" ]; then
            echo "::error::No successful CI run found for $SHA (got: '${CONCLUSION:-none}') — run CI to green before releasing"
            exit 1
          fi
```

And the existing job gains the dependency:

```yaml
  windows-installer:
    name: Build and publish Windows installers
    needs: validate
    runs-on: windows-latest
```

- [ ] **Step 2: Validate YAML; verify the jq expression** with a local dry parse: `echo '[{"status":"completed","conclusion":"success"}]' | jq '[.[] | select(.status == "completed")] | first | .conclusion'` → `"success"`.

- [ ] **Step 3: Tombstones (GAP-41 High struck, GAP-42) + AGENTS.md sentence + commit**

```bash
git add .github/workflows/release.yml docs/Gaps.md AGENTS.md
git commit -m "ci(release): validate tag, branch, and CI status before publishing" -m "GAP-41: the dispatch path had no ref guard and nothing enforced the tag==version comment, so any branch could be released under an arbitrary tag with a disagreeing latest.json. GAP-42: no dependency on CI success meant a tag on a red commit published straight to the updater feed. A validate job now gates the build on all three." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 5: Windows `cargo test` (D7 · GAP-43)

**Files:**
- Modify: `.github/workflows/ci.yml` (windows-app job)
- Modify: `docs/Gaps.md` (GAP-43 fully FIXED), `AGENTS.md` (CI table row + the "Not covered by CI: any cargo test on Windows" line in Known gaps/§CI)

- [ ] **Step 1: Add the test steps** to `windows-app` AFTER the build step (the build warms the target dir; tests reuse it):

```yaml
      # The most platform-sensitive code (process detection, GetKeyState,
      # WASAPI loopback gates, MoveFileExW's non-replacing fallback, whisper
      # on MSVC) is Windows-only, yet this job was build-only — the
      # cfg(windows) tests never executed anywhere (GAP-43).
      - name: tests (core + capture + transcribe crates, Windows)
        run: cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe
        working-directory: src-tauri
      - name: tests (transcribe crate, whisper feature, Windows)
        run: cargo test -p vault_buddy_transcribe --features whisper
        working-directory: src-tauri
```

- [ ] **Step 2: Docs** — GAP-43 heading becomes `### GAP-43 · ~~Medium~~ FIXED 2026-07-10 · No Rust tests run on Windows` with a body noting both halves (workspace clippy in linux-app; core/capture/transcribe + whisper tests now in windows-app — including the GAP-06 `cfg(windows)` MoveFileExW contract test, which executes for the first time). AGENTS.md: the CI table's `windows-app` row adds "+ `cargo test` for core/capture/transcribe (incl. `--features whisper`)"; delete the "Not covered by CI (see docs/Gaps.md): any `cargo test` on Windows." line.

- [ ] **Step 3: Validate YAML; commit**

```bash
git add .github/workflows/ci.yml docs/Gaps.md AGENTS.md
git commit -m "ci(release): run the platform-sensitive Rust tests on Windows" -m "GAP-43: windows-app was build-only, so cfg(windows) code (GetKeyState re-check, process detection, WASAPI gates, the GAP-06 MoveFileExW non-replacing fallback and the GAP-08 predicate) never executed in CI. Core, capture, and transcribe (with the whisper feature) now cargo-test on the Windows runner after the build warms the target dir." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

Honest note for the report: this task cannot observe the Windows run locally — the proof arrives when CI runs on the pushed branch; check it at close-out.

---

### Task 6: Reject non-increasing versions in bump-version.mjs (D8 · GAP-44 item)

**Files:**
- Modify: `scripts/bump-version.mjs` (`resolveNewVersion` area)
- Modify: `docs/Gaps.md` (annotate ONLY the `<=` bullet in GAP-44 — the entry stays open)
- Test: `tests/bump-version.test.ts`

- [ ] **Step 1: Write the failing tests** (mirror the file's existing idiom — it tests the script's pure functions or via child_process; check first):

```ts
it("rejects a version equal to the current one (GAP-44)", () => {
  // Equal input used to fail later at `git commit` with a confusing
  // "nothing to commit".
  expect(() => resolveAgainstCurrent("0.5.1", "0.5.1")).toThrow(/must be greater/i);
});

it("rejects a version lower than the current one (GAP-44)", () => {
  expect(() => resolveAgainstCurrent("0.5.1", "0.4.9")).toThrow(/must be greater/i);
});

it("accepts a higher version", () => {
  expect(resolveAgainstCurrent("0.5.1", "0.6.0")).toBe("0.6.0");
});
```

(If the test file exercises the script differently — e.g. exported helpers with other names — adapt: the CONTRACT is that an explicit `X.Y.Z` argument `<=` the current version throws with a message naming both versions, while `patch|minor|major` keywords are inherently increasing and stay untouched.)

- [ ] **Step 2: RED** — `npx vitest run tests/bump-version.test.ts`.

- [ ] **Step 3: Implement** — in `bump-version.mjs`, add a semver comparison and enforce it where the explicit form is accepted (adapt to the file's real export structure; the plan's sketch):

```js
function compareSemver(a, b) {
  const pa = a.split(".").map(Number);
  const pb = b.split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pa[i] - pb[i];
  }
  return 0;
}

function resolveNewVersion(current, arg) {
  if (BUMP_KEYWORDS.includes(arg)) return nextVersion(current, arg);
  if (SEMVER_RE.test(arg)) {
    // GAP-44: an equal version used to die later at `git commit` with a
    // confusing "nothing to commit"; a lower one silently downgraded.
    if (compareSemver(arg, current) <= 0) {
      throw new Error(`New version ${arg} must be greater than the current ${current}`);
    }
    return arg;
  }
  throw new Error(`Invalid version "${arg}": expected X.Y.Z or one of patch/minor/major`);
}
```

Export whatever the tests need, matching the file's existing export style.

- [ ] **Step 4: GREEN + gates** — `npx vitest run tests/bump-version.test.ts && npm run lint && node scripts/bump-version.mjs --check` (the check mode must be unaffected).

- [ ] **Step 5: Annotate the GAP-44 bullet + commit**

The `<=` bullet gains `(FIXED 2026-07-10 — resolveNewVersion rejects X.Y.Z <= current with a message naming both)`. Entry heading untouched (npm-audit/SECURITY.md/CHANGELOG bullets stay open).

```bash
git add scripts/bump-version.mjs tests/bump-version.test.ts docs/Gaps.md
git commit -m "fix(release): reject non-increasing versions in the bump script" -m "GAP-44 (one bullet): an explicit version equal to the current one failed later at git commit with a confusing 'nothing to commit', and a lower one silently downgraded the five version files. The explicit form now requires strictly-greater semver; the patch/minor/major keywords are inherently increasing and unchanged." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 7: Restrictive CSP (D1 · GAP-34)

**Files:**
- Modify: `src-tauri/tauri.conf.json` (`app.security`)
- Modify: `docs/Gaps.md` (GAP-34 tombstone WITH the Windows-verification caveat)

- [ ] **Step 1: Set the policy** — `"security": { "csp": null }` becomes:

```json
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:"
    }
```

(The spec's starting point plus `img-src data:` — the buddy sprites/tray previews are bundled, but Vite inlines small assets as data: URIs below its assetsInlineLimit; without the img-src clause those break silently. Tauri v2 auto-appends its IPC/asset origins to a configured CSP unless `dangerousDisableAssetCspModification` is set — it is not.)

- [ ] **Step 2: Verify what can be verified here**

- `npx tauri build --no-bundle` from the repo root — Tauri validates and embeds the config; a malformed policy fails here.
- `npm test` — full Vitest suite (happy-dom ignores CSP; this only proves no config-shape regressions).
- `grep -rn "assetsInlineLimit\|data:image" src/ vite.config.ts` — report whether any asset actually relies on data: URIs, so the reviewer can judge the img-src clause.

- [ ] **Step 3: Tombstone GAP-34 with the honest caveat + commit**

The tombstone body must state: policy set (quote it), Linux compile gate green, **runtime behavior in the packaged WebView2 app is NOT yet verified — the next Windows-checklist run must confirm all three windows render (buddy sprites, panel styles, bubble) and the updater/settings views work; a breakage is a one-line revert of this commit.**

```bash
git add src-tauri/tauri.conf.json docs/Gaps.md
git commit -m "feat(security): enable a restrictive CSP for all three webviews" -m "GAP-34: csp was null, so every window ran uninhibited while rendering strings derived from vault contents — cheap defense-in-depth for exactly the injection class that would weaponize a containment bug. Policy: default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: (Tauri appends its IPC/asset origins automatically). Runtime verification on Windows is called out in the tombstone; revert is one line." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>" -m "Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU"
```

---

### Task 8: Sub-pass close-out — gates, CI observation, final review

- [ ] **Step 1: Full local gate run** (same CI-order list as the B/C close-outs: lint → check:loc → check:quality → test:coverage → fmt --check → workspace clippy → all crate suites → shell lib tests → deny → `npx tauri build --no-bundle`).
- [ ] **Step 2: Push and OBSERVE CI** — this sub-pass changed ci.yml itself; the pushed run is the only real test of Tasks 1/2/5. Wait for the run on the new head: frontend + rust-core + linux-app green, and **windows-app now runs the new cargo-test steps** — confirm they executed and passed (this is also GAP-06's cfg(windows) test running for the first time). If the Windows tests fail on a real platform issue, that is a FINDING to fix, not to skip.
- [ ] **Step 3: Ledger** — GAP-34/35/36/37/41/42/43 tombstones + the GAP-44 bullet annotation present; one commit per task.
- [ ] **Step 4:** The controller dispatches the final whole-sub-pass review (most capable model), including the workflow diffs (reviewers can read YAML), then closes.

---

## Self-review record

- **Spec coverage:** D1→T7, D2→T1, D3→T2, D4→T3, D5→T4, D6→T4, D7→T5, D8→T6; close-out→T8. The spec's manual-Windows-verification requirement for D1 is the tombstone caveat + Windows checklist note in T7; D7's "where testable" is T8's CI observation step.
- **Placeholder scan:** T4's `<same pinned SHA as Task 1>` is a deliberate cross-task reference resolved at execution time (Task 1 records the SHA table in its report; T4's implementer copies from the live ci.yml, which Task 1 already rewrote — instruct: grep ci.yml for the pinned checkout line). No other placeholders.
- **Type consistency:** `resolveNewVersion(current, arg)`/`compareSemver(a, b)` names consistent; workflow job name `validate` matches the `needs: validate` reference; env var names (`REQUESTED_VERSION`, `TAG`, `SHA`, `REF`) used consistently within their steps.
- **Ordering rationale:** workflows (T1–T5) land before the CSP (T7) so the close-out's single CI observation covers both the pinned actions and the Windows tests; T6 is independent.
