# Linux build for container testing — design

_2026-07-09_

## Problem

The Tauri shell crate (`src-tauri/src/*.rs`) is the one part of the codebase
that cannot be compiled outside Windows today. The `core`, `capture`, and
`transcribe` crates already build and test on Linux; the shell needs the
platform WebView libraries, which Linux containers lack. As a result, any
change to `src-tauri/src/*.rs` — IPC command signatures, window/tray logic,
plugin wiring — can only be compiled by CI's Windows job. An agent working in
a cloud container edits shell code blind and must push-and-wait to discover a
type error or a missing `cfg` gate.

The Windows job stays the release-and-behavior gate. What is missing is a
**fast compile gate** that agents (and CI) can run on Linux to catch the class
of errors that is purely about the code compiling — the bulk of what breaks
when editing the shell.

## Goal

Make the Tauri shell crate compile and link on Linux, both:

- **in containers** — so an agent can verify a shell edit locally before
  pushing, and
- **in the CI pipeline** — a Linux app-build job that fails fast on compile
  errors, in parallel with the Windows job.

Get as close as possible to Windows-level app-correctness verification for the
things that _can_ be verified by compilation. The Linux build is **never
released** — it exists to help agents and CI, not end users.

### Non-goals

- No headless run (xvfb), smoke test, or E2E (tauri-driver/WebdriverIO). The
  chosen rung on the verification ladder is **compile gate only**.
- No `.deb`/AppImage/bundle output. `--no-bundle` is used deliberately.
- No updater artifacts, no signing, no release wiring for Linux.
- No change to the Windows job's role as the release + desktop-behavior gate.

## Why this is feasible with (expected) no app-code changes

Tauri v2 treats Linux as a first-class target, and this codebase is already
portable:

- Every platform-specific path in the shell and capture crates is already
  `cfg`-gated. WASAPI desktop-audio loopback is `#[cfg(windows)]` and degrades
  to "mic only" on non-Windows (`capture/src/devices.rs`); the `GetKeyState`
  drag-stale guard is `#[cfg(windows)]` with a `#[cfg(not(windows))]` no-op
  fallback (`commands.rs`); the upkeep-tick button check is `#[cfg(windows)]`
  (`lib.rs`); diagnostics already has Linux/macOS signal handling
  (`diagnostics.rs`).
- Every plugin in use — `single-instance`, `window-state`, `updater`,
  `process`, `notification`, and `tray-icon` — supports Linux.
- `whisper-rs-sys` builds anywhere with `cmake` + `clang` (both already used by
  the `rust-core` job's `whisper`-feature step); the shell pulls
  `vault_buddy_transcribe` with `features = ["whisper"]`, so the Linux shell
  build compiles whisper.cpp too.

The design therefore assumes **zero app-code changes**, but the implementation
must actually run the Linux build and close any latent gap that surfaces (a
missing `cfg` gate, a Linux-unsupported call) by mirroring the existing
`cfg`-gate patterns. That iteration is the real work and the main risk; it
cannot be fully predicted until the build runs.

## Design

### 1. `scripts/setup-linux-deps.sh` — single source of truth for system deps

An idempotent installer for the Tauri v2 Linux toolchain plus this repo's
extra native build dependencies. It is the **one** place the package list
lives; both CI and humans/agents invoke it, so the list never drifts between
CI and docs.

Packages:

| Package | Why |
| --- | --- |
| `libwebkit2gtk-4.1-dev` | the WebView — the actual blocker |
| `libgtk-3-dev` | GTK, the windowing/toolkit layer Tauri links on Linux |
| `libayatana-appindicator3-dev` | the system tray (`tray-icon` on Linux) |
| `librsvg2-dev` | SVG rendering for icons |
| `libxdo-dev` | input synthesis Tauri links on Linux |
| `libsoup-3.0-dev` | HTTP stack behind webkit2gtk-4.1 |
| `libssl-dev` | TLS for the updater/network crates |
| `libasound2-dev` | ALSA headers for `cpal` (capture) |
| `build-essential`, `pkg-config` | C toolchain + lib discovery |
| `cmake`, `clang` | `whisper-rs-sys` (bindgen + whisper.cpp) |

Behavior:

- Guard: if `pkg-config --exists webkit2gtk-4.1` already succeeds, print a
  "deps already present" line and exit `0` — fast no-op on a warm container.
- Otherwise `sudo apt-get update` and `sudo apt-get install -y <list>`. Use
  `sudo` only when not already root (CI runs as root in some images; a
  container agent may not) — detect via `$(id -u)`.
- Fail loudly (non-zero exit) if the install fails; the script is used as a CI
  step and must not mask a broken environment.
- Named/commented so the package list explains _why_ each entry is present
  (matches this repo's comment-the-invariant convention).

Exposed as an npm script for discoverability:

```json
"setup:linux": "bash scripts/setup-linux-deps.sh"
```

### 2. CI: a new `linux-app` job in `.github/workflows/ci.yml`

Mirrors `windows-app` but on Linux and without bundling:

- `runs-on: ubuntu-latest`
- `needs: [frontend, rust-core]` — same gating as `windows-app`, so it runs
  **in parallel** with the Windows job after the two fast jobs pass. Linux
  compile errors are cheaper and usually faster to surface than the Windows
  build, giving quick feedback without serializing behind Windows.
- Steps: `checkout` → `setup-node@v4` (Node 22, npm cache) →
  `dtolnay/rust-toolchain@stable` → `Swatinem/rust-cache@v2`
  (`workspaces: src-tauri`, `shared-key: linux-app`) → run
  `scripts/setup-linux-deps.sh` → `npm ci` →
  `npx tauri build --no-bundle --config '{"bundle":{"createUpdaterArtifacts":false}}'`.
- `--no-bundle` compiles and links the whole app and runs Tauri's codegen +
  capability checks, but produces no installer — the true compile gate with no
  release artifact. `createUpdaterArtifacts:false` means no signing secret is
  needed (and none is referenced), so the job runs identically on forks.
- No artifact upload — nothing is released.

Invoke the Tauri CLI as `npx tauri …` (never the `npm run tauri` indirection),
per the repo gotcha about `tauri dev build` expansion.

### 3. Documentation

- **`AGENTS.md`** — "What compiles where" table: the shell row changes from
  "Windows only" to note it _also_ compiles on Linux once the GUI deps are
  installed (`npm run setup:linux`), with the `npx tauri build --no-bundle`
  command. Keep the Windows job as the release/behavior gate in the wording.
  Update the "you cannot compile it in a Linux container" paragraph to point at
  the setup script instead.
- **`docs/DEVELOPMENT.md`** — a short "Build the shell on Linux (compile gate
  for agents/CI)" subsection under the run-from-source area, and a new row in
  the Quality-pipeline CI table: `Linux app | Linux | tauri build --no-bundle
  compile gate (no installer)`.
- Note that a Claude-Code-on-web environment can point its environment setup
  script at `scripts/setup-linux-deps.sh` to pre-provision the container, so
  agents skip the install step entirely.

## Testing / verification

This change is build-tooling, not runtime code, so verification is the build
itself:

1. Run `scripts/setup-linux-deps.sh` in the container, then
   `npx tauri build --no-bundle` and confirm it links to completion.
2. If any compile error surfaces, fix it with a `cfg` gate that mirrors the
   existing patterns, add a regression comment naming the failure mode, and
   re-run until green.
3. `cargo fmt --check` stays green for any Rust touched.
4. The existing Vitest and `cargo test` suites are unaffected and must stay
   green (no product code changes intended).

CI proves the gate holds going forward: the `linux-app` job must pass on the
PR.

## Rollout

Single PR on `claude/linux-build-container-testing-lpz95y`: the setup script,
the npm script, the CI job, the docs, and any `cfg`-gate fixes the build turns
up. The Windows job is untouched. Nothing about releases changes.
