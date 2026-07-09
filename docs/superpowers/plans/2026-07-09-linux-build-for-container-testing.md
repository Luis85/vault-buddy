# Linux Build for Container Testing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Tauri shell crate compile on Linux so agents in cloud containers and CI can verify a shell edit without pushing to the Windows job.

**Architecture:** One idempotent setup script installs the Tauri v2 Linux system libraries (the single source of truth for the package list); a new parallel `linux-app` CI job runs `npx tauri build --no-bundle` as a pure compile gate; docs point agents at the script. Any latent Linux-portability gap the build surfaces is closed with a `cfg` gate mirroring the existing patterns. Nothing is bundled, signed, or released.

**Tech Stack:** Bash, GitHub Actions, Tauri v2 CLI (`npx tauri`), Rust (cfg gates only if needed), Debian/Ubuntu apt.

## Global Constraints

- **Compile gate only** — no xvfb/headless run, no E2E, no `.deb`/AppImage bundle, no updater artifacts, no signing, no release wiring. Use `npx tauri build --no-bundle`.
- **The Linux build is never released.** The Windows CI job stays the release + desktop-behavior gate; do not touch it.
- **`cfg`-gate, don't rewrite.** Any Linux fix mirrors existing patterns (`#[cfg(windows)]` / `#[cfg(not(windows))]`), keeps Windows behavior byte-identical, and carries a regression comment naming the failure mode.
- **Invoke the Tauri CLI as `npx tauri …`**, never `npm run tauri` (the `tauri dev build` expansion gotcha).
- **Package list lives in exactly one place** — `scripts/setup-linux-deps.sh`. CI and docs reference the script, never re-list packages.
- **Commits:** Conventional Commits. Committer identity `Claude <noreply@anthropic.com>`. `cargo fmt --check` stays green for any Rust touched.

---

### Task 1: Linux deps setup script + npm script

**Files:**
- Create: `scripts/setup-linux-deps.sh`
- Modify: `package.json` (add `"setup:linux"` to `scripts`)

**Interfaces:**
- Produces: an executable `scripts/setup-linux-deps.sh` that, on success, leaves `pkg-config --exists webkit2gtk-4.1` returning 0. Reused verbatim by Task 3 (CI) and referenced by Task 4 (docs).

- [ ] **Step 1: Write the setup script**

Create `scripts/setup-linux-deps.sh`:

```bash
#!/usr/bin/env bash
# Install the system libraries the Tauri shell crate needs to COMPILE on
# Linux. The core/capture/transcribe crates already build on Linux; only the
# shell (src-tauri/src/*.rs) needs the WebView + GTK stack. This is the single
# source of truth for that package list — CI and humans/agents both call it,
# so the list never drifts. See
# docs/superpowers/specs/2026-07-09-linux-build-for-container-testing-design.md
set -euo pipefail

# Fast no-op on a warm container: if the WebView headers are already present,
# everything else in the list came with them.
if pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
  echo "setup-linux-deps: webkit2gtk-4.1 already present — nothing to do"
  exit 0
fi

# Use sudo only when not already root (CI images run as root; a container
# agent may not).
SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  SUDO="sudo"
fi

$SUDO apt-get update
$SUDO apt-get install -y \
  libwebkit2gtk-4.1-dev `# the WebView — the actual blocker` \
  libgtk-3-dev `# GTK windowing/toolkit layer Tauri links on Linux` \
  libayatana-appindicator3-dev `# system tray (tray-icon on Linux)` \
  librsvg2-dev `# SVG icon rendering` \
  libxdo-dev `# input synthesis Tauri links on Linux` \
  libsoup-3.0-dev `# HTTP stack behind webkit2gtk-4.1` \
  libssl-dev `# TLS for updater/network crates` \
  libasound2-dev `# ALSA headers for cpal (capture)` \
  build-essential pkg-config `# C toolchain + lib discovery` \
  cmake clang `# whisper-rs-sys: bindgen + whisper.cpp`

echo "setup-linux-deps: done"
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/setup-linux-deps.sh`

- [ ] **Step 3: Run the script and verify it installs the WebView**

Run: `bash scripts/setup-linux-deps.sh && pkg-config --exists webkit2gtk-4.1 && echo WEBKIT_OK`
Expected: ends with `WEBKIT_OK` (first run installs; the guard makes reruns print the "already present" line and exit 0).

- [ ] **Step 4: Add the npm script**

In `package.json`, inside `"scripts"`, add after `"bump-version"`:

```json
    "bump-version": "node scripts/bump-version.mjs",
    "setup:linux": "bash scripts/setup-linux-deps.sh"
```

(Add a comma after the `bump-version` line; the `setup:linux` line is the last entry.)

- [ ] **Step 5: Verify the npm script resolves**

Run: `npm run setup:linux`
Expected: prints `setup-linux-deps: webkit2gtk-4.1 already present — nothing to do` (deps installed in Step 3), exit 0.

- [ ] **Step 6: Commit**

```bash
git add scripts/setup-linux-deps.sh package.json
git commit -m "build(linux): add setup-linux-deps.sh + npm run setup:linux

Single source of truth for the Tauri v2 Linux system libraries the shell
crate needs to compile. Idempotent; used by CI and by agents in
containers so the shell can be verified without the Windows CI job."
```

---

### Task 2: Make the shell crate compile on Linux

**Files:**
- Modify: `src-tauri/src/*.rs` — **only if** the build surfaces a Linux-unsupported call or a missing `cfg` gate. The expectation (per the spec) is **zero** app-code changes; every platform path is already gated.
- Reference: `src-tauri/src/commands.rs` (`start_buddy_drag` / `primary_button_down` `cfg` pattern), `src-tauri/capture/src/devices.rs` (loopback `#[cfg(windows)]` / `#[cfg(not(windows))]` pattern).

**Interfaces:**
- Consumes: the installed deps from Task 1.
- Produces: a shell that links on Linux via `npx tauri build --no-bundle`. Task 3's CI job runs the same command.

- [ ] **Step 1: Run the compile gate**

Run: `npx tauri build --no-bundle --config '{"bundle":{"createUpdaterArtifacts":false}}'`
Expected: `beforeBuildCommand` (`npm run build`) succeeds, then the Rust shell + whisper.cpp compile and link to completion. First run is slow (whisper.cpp + ~450 crates).

- [ ] **Step 2: If it fails to compile, diagnose and gate the offending call**

If (and only if) a compile error appears, it will name a symbol that is Windows-only or otherwise unavailable on Linux. Wrap the offending path exactly like the existing gates — e.g. a Windows-only branch gets `#[cfg(windows)]` with a `#[cfg(not(windows))]` fallback that preserves behavior (log-and-degrade, or a no-op), and add a one-line comment naming the failure mode. Do not restructure surrounding code. Re-run Step 1 until it links.

- [ ] **Step 3: Keep rustfmt green (only if Rust was touched)**

Run: `cd src-tauri && cargo fmt --check`
Expected: exit 0. If Step 2 changed any `.rs`, run `cargo fmt` first, then re-check.

- [ ] **Step 4: Confirm existing suites still pass (only if Rust was touched)**

Run: `cd src-tauri && cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe`
Expected: PASS. Skip if no Rust changed in Step 2.

- [ ] **Step 5: Commit (only if Step 2 changed code)**

If no code changed, there is nothing to commit here — the deliverable is "the build links," proven by Step 1, and this task carries no diff. Otherwise:

```bash
git add src-tauri/src
git commit -m "fix(shell): gate <symbol> for Linux compilation

<symbol> is Windows-only; the Linux compile gate (tauri build --no-bundle)
surfaced it. Gated to preserve identical Windows behavior."
```

---

### Task 3: Add the `linux-app` CI job

**Files:**
- Modify: `.github/workflows/ci.yml` (add a `linux-app` job after `windows-app`)

**Interfaces:**
- Consumes: `scripts/setup-linux-deps.sh` (Task 1), the `npx tauri build --no-bundle` command proven in Task 2.

- [ ] **Step 1: Add the job**

Append to `.github/workflows/ci.yml`, as a sibling of `windows-app` (same indentation under `jobs:`):

```yaml
  linux-app:
    name: Linux app (compile gate)
    runs-on: ubuntu-latest
    needs: [frontend, rust-core]
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
          shared-key: linux-app
      - name: Install Linux GUI/build system deps
        run: bash scripts/setup-linux-deps.sh
      - run: npm ci
      # Compile gate only: --no-bundle links the whole app and runs Tauri's
      # codegen + capability checks but produces no installer, and
      # createUpdaterArtifacts:false means no signing secret is referenced
      # (runs identically on forks). This closes the loop that previously only
      # the windows-app job could — a shell compile error now fails here, fast
      # and in parallel with Windows. The Linux build is never released.
      - name: Compile the Tauri shell (no bundle)
        run: npx tauri build --no-bundle --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

- [ ] **Step 2: Validate the workflow YAML parses**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('YAML_OK')"`
Expected: `YAML_OK`. (If PyYAML is unavailable, instead confirm the job block's indentation matches `windows-app` by eye.)

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add linux-app compile-gate job

Runs npx tauri build --no-bundle on ubuntu-latest, in parallel with
windows-app after the fast jobs pass. Fails fast on shell compile errors
so agents get Linux feedback without waiting on the Windows build. No
bundle, no artifacts, no signing — never released."
```

---

### Task 4: Documentation

**Files:**
- Modify: `AGENTS.md` (the "What compiles where" table + the "you cannot compile it in a Linux container" paragraph)
- Modify: `docs/DEVELOPMENT.md` (a Linux compile-gate subsection + the CI table)

**Interfaces:**
- Consumes: `npm run setup:linux` (Task 1), the compile command (Task 2), the `linux-app` job (Task 3).

- [ ] **Step 1: Update the AGENTS.md compile matrix**

In `AGENTS.md`, in the "What compiles where" table, change the `src-tauri/` (root crate) row's "Compiles on" cell from:

```
| **Windows only** (Linux lacks webkit2gtk); CI's Windows job is the compile gate |
```

to:

```
| **Windows** (release + behavior gate) — **also compiles on Linux** as a compile gate once GUI deps are installed (`npm run setup:linux`, then `npx tauri build --no-bundle`); CI runs both |
```

- [ ] **Step 2: Update the AGENTS.md "cannot compile" paragraph**

In `AGENTS.md`, replace the paragraph that begins "When you change the shell crate (`src-tauri/src/*.rs`), you cannot compile it in a Linux container." with:

```markdown
When you change the shell crate (`src-tauri/src/*.rs`), you *can* now compile
it in a Linux container as a compile gate: run `npm run setup:linux` once (it
installs the WebView/GTK/tray system libs — the single source of truth is
`scripts/setup-linux-deps.sh`), then `npx tauri build --no-bundle`. This
catches type errors, IPC signature drift, and missing `cfg` gates locally
instead of push-and-wait. It is a **compile gate only** — the Windows job
remains the release + desktop-behavior gate (transparency, tray, drag, the
Obsidian round-trip). Mirror existing `cfg`-gate patterns for any
platform-specific code, run `cargo fmt --check`, and let CI's `windows-app`
and `linux-app` jobs verify the build.
```

- [ ] **Step 3: Add the Linux build subsection to DEVELOPMENT.md**

In `docs/DEVELOPMENT.md`, immediately after the paragraph ending "...which is why CI builds the app on a Windows runner." (end of the "Tests and checks" area), add:

```markdown
### Build the shell on Linux (compile gate for agents/CI)

The shell no longer builds *only* on Windows. Linux can now compile it as a
fast **compile gate** — enough to catch type errors, IPC signature drift, and
missing `cfg` gates, though not desktop behavior (transparency, tray, drag).
This exists mainly so coding agents in cloud containers can verify a shell
edit without pushing to the Windows CI job.

```bash
npm run setup:linux    # once per container: installs WebView/GTK/tray system
                       # libs (scripts/setup-linux-deps.sh) — needs sudo + apt
npx tauri build --no-bundle   # compile + link the app; no installer produced
```

A Claude-Code-on-web environment can point its environment setup script at
`scripts/setup-linux-deps.sh` to pre-provision the container, so agents skip
the install step. The Linux build is **never released**; the Windows job stays
the release and desktop-behavior gate.
```

- [ ] **Step 4: Add the Linux row to the DEVELOPMENT.md CI table**

In `docs/DEVELOPMENT.md`, in the "Quality pipeline" table, add a row after the "Rust core" row and before "Windows app":

```markdown
| Linux app | Linux | `tauri build --no-bundle` compile gate (no installer, never released) |
```

And update the sentence "The Windows job only runs after the two fast jobs pass." to:

```markdown
The Windows and Linux app jobs both run after the two fast jobs pass (in
parallel with each other).
```

- [ ] **Step 5: Commit**

```bash
git add AGENTS.md docs/DEVELOPMENT.md
git commit -m "docs: document the Linux compile-gate build

Update the AGENTS.md compile matrix and DEVELOPMENT.md so agents know the
shell now builds on Linux via npm run setup:linux + tauri build --no-bundle,
while the Windows job stays the release/behavior gate."
```

---

## Self-Review

**Spec coverage:**
- Setup script (single source of truth) → Task 1 ✓
- `npm run setup:linux` discoverability → Task 1 ✓
- Shell compiles on Linux / cfg-gate any gaps → Task 2 ✓
- Parallel `linux-app` CI job, `--no-bundle`, `createUpdaterArtifacts:false`, `needs:[frontend,rust-core]`, own `shared-key` → Task 3 ✓
- AGENTS.md matrix + paragraph, DEVELOPMENT.md subsection + CI table + env-setup-script note → Task 4 ✓
- Non-goals (no xvfb/E2E/bundle/signing/release) → encoded in Global Constraints and the `--no-bundle` command ✓

**Placeholder scan:** No TBD/TODO. Task 2 is intentionally conditional (fix *if* a gap surfaces) with the exact pattern to follow and the fallback ("no diff, deliverable is the build links") — not a placeholder.

**Type consistency:** The command `npx tauri build --no-bundle --config '{"bundle":{"createUpdaterArtifacts":false}}'` is identical in Task 2 (local), Task 3 (CI), and the docs. Script path `scripts/setup-linux-deps.sh` and npm script `setup:linux` are consistent across all tasks. Guard `pkg-config --exists webkit2gtk-4.1` matches the package `libwebkit2gtk-4.1-dev`.
