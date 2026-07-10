# Developing Vault Buddy

Contributor documentation: building from source, tests, the CI/release
pipelines, and the agentic development setup. For what the product is and
where it's going, see the [PRD - Product Vision](PRD%20-%20Product%20Vision.md).

## Run it from source

### Prerequisites (Windows)

1. [Node.js 22+](https://nodejs.org)
2. [Rust stable](https://rustup.rs) — the default MSVC toolchain; rustup will
   prompt you to install the Visual Studio C++ Build Tools if missing
3. WebView2 runtime — preinstalled on Windows 11; on Windows 10 see the
   [Tauri prerequisites](https://tauri.app/start/prerequisites/)
4. **LLVM (libclang) and CMake** — the app statically links whisper.cpp for
   local speech-to-text, so the `whisper` feature is always on for the shell
   and *every* app build compiles `whisper-rs-sys`, whose build runs `bindgen`
   (needs `libclang`) and `cmake`. Install both and open a fresh terminal:

   ```powershell
   winget install LLVM.LLVM Kitware.CMake
   ```

   If `bindgen` still can't find libclang — the telltale error is
   `Unable to find libclang: … set the LIBCLANG_PATH environment variable` —
   point it at the install explicitly and reopen the terminal:

   ```powershell
   setx LIBCLANG_PATH "C:\Program Files\LLVM\bin"
   ```

   CI's Windows runner ships both tools, so this is a local-only setup step.
   (bindgen genuinely can't be skipped on Windows: `whisper-rs-sys` ships only
   Linux-generated committed bindings, and their glibc struct-layout assertions
   — e.g. `_IO_FILE` sized at 216 bytes — fail to compile on MSVC, so bindgen
   must regenerate them from the local headers.)

### Check out and run

```bash
git clone https://github.com/Luis85/vault-buddy.git
cd vault-buddy

# to try a branch that isn't merged yet (e.g. a PR branch):
#   git fetch origin <branch-name>
#   git checkout <branch-name>

npm install
npm run test-build   # `tauri dev` — compile the shell and run the app
```

The first `tauri dev` compiles the Rust shell and takes a few minutes; after
that it's incremental.

### Build an installer

```bash
npx tauri build
```

Installers land in `src-tauri/target/release/bundle/` (`msi/` and `nsis/`).
Alternatively, every push through CI builds Windows installers — download the
`vault-buddy-windows-<sha>` artifact from the
[Actions](https://github.com/Luis85/vault-buddy/actions) run.

### Tests and checks

```bash
npm run test                       # Vitest component/store tests
npm run build                      # vue-tsc typecheck + production build

# from src-tauri/ — mirrors the CI "Rust core" job (Linux needs ALSA's
# headers first: sudo apt-get install -y libasound2-dev)
cargo fmt --check
cargo clippy -p vault_buddy_core -p vault_buddy_capture --all-targets -- -D warnings
cargo test -p vault_buddy_core -p vault_buddy_capture
```

The Rust code is split into three crates: `src-tauri/core/` (`vault_buddy_core`)
is a pure crate with all Obsidian logic (config parsing, daily-note
resolution, URI building) and no GUI or audio dependencies — it tests on any
machine, including CI containers. `src-tauri/capture/` (`vault_buddy_capture`)
is the audio engine — device capture via `cpal`, MP3 encoding via LAME
(`mp3lame-encoder`) — and also tests anywhere, though on Linux it needs
ALSA's development headers to build: `sudo apt-get install -y
libasound2-dev`. `src-tauri/` itself is the thin Tauri shell (window, tray,
command wrappers) and needs platform WebView libraries to compile — on
Windows that works out of the box; on Linux it needs the WebView/GTK/tray
system libraries (see the compile-gate section below), which is why the
*release* build runs on a Windows runner.

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

The build needs a current stable Rust toolchain (`rustup update stable`) —
the same one CI uses; some dependencies set a recent minimum. A
Claude-Code-on-web environment can point its environment setup script at
`scripts/setup-linux-deps.sh` to pre-provision the container, so agents skip
the install step. The Linux build is **never released**; the Windows job stays
the release and desktop-behavior gate.

## Quality pipeline

CI runs on every push to `main` and every pull request
([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)):

| Job | Runner | What it gates |
| --- | --- | --- |
| Frontend | Linux | ESLint, LOC guard, fallow quality ratchet, `vue-tsc` typecheck + production build, Vitest with coverage floors |
| Rust core | Linux | `cargo fmt --check`, `clippy -D warnings`, core unit tests |
| Linux app | Linux | `tauri build --no-bundle` compile gate (no installer, never released) |
| Windows app | Windows | Full Tauri compile + MSI/NSIS installers, uploaded as artifacts (14-day retention) |

### Frontend quality gates

The frontend job runs four gates beyond the test suite, mirrored locally by:

```bash
npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage
```

Ordering matters: `check:quality` must run while no `coverage/` directory
exists (a stray coverage report flips fallow's complexity weighting from
static estimation to coverage-weighted CRAP), so `test:coverage` — which
creates `coverage/` — always runs last. Delete `coverage/` before re-running
the ratchet locally.

- **ESLint** (`npm run lint`, config in `eslint.config.mjs`) — flat config:
  JS + typescript-eslint recommended, `eslint-plugin-vue` flat/recommended
  for SFCs, import sorting, the vitest plugin for `tests/`, and a src-only
  safety gate (`no-console` funneling diagnostics through `src/logging.ts`,
  bans on `innerHTML`/`v-html` — vault-derived strings are an XSS vector).
  **Severity policy:** a rule with an existing backlog is staged at `warn`
  (tracked, non-blocking — CI passes no `--max-warnings`); burn the backlog
  down, then promote the rule to `error` and note the promotion in the
  config. Never blanket-disable to get green; a genuinely unavoidable case
  takes a narrow `// eslint-disable-next-line <rule>` with a justification.
- **LOC guard** (`npm run check:loc`, `scripts/check-loc.mjs`) — no
  `src/**` `.ts`/`.vue` file may exceed 500 nonblank lines. Existing
  hotspots are grandfathered in `scripts/loc-baseline.json` and may shrink
  but never grow (a shrink-only ratchet); new oversized files fail.
- **Quality ratchet** (`npm run check:quality`, `scripts/check-quality.mjs`)
  — runs [fallow](https://www.npmjs.com/package/fallow) (`.fallowrc.json`)
  and compares dead code, circular dependencies, duplication, and
  complexity against `scripts/quality-baseline.json`. Counters may shrink
  but not grow; average maintainability may rise but not drop. `npm run
  quality` prints the full advisory report; `npm run quality:audit` reviews
  changed files before a PR.
- **Coverage floors** (`npm run test:coverage`) — Vitest + Istanbul with
  rise-only thresholds in `vite.config.ts` (statements/branches/functions/
  lines, floored from the adoption run).

**Locking improvements in:** when a gated metric improves, re-baseline in
the same PR (`npm run check:loc -- --update`, `npm run check:quality --
--update`, raise the coverage floors) and commit the diff — an unlocked
gain can silently regress. Bumping a baseline in the *loosening* direction
is a reviewed, justified decision, never a side effect.

The Windows and Linux app jobs both run after the two fast jobs pass (in
parallel with each other). Desktop behavior that
can't be asserted in CI (transparency, tray, drag, the real Obsidian
round-trip) is covered by the manual verification checklist in
[`docs/superpowers/specs/`](superpowers/specs/).

## Releases

A separate [release workflow](../.github/workflows/release.yml) runs on `v*`
tags: it builds the Windows installers and publishes them as a GitHub
Release for end users.

```bash
npm run bump-version -- 0.1.0    # or: patch | minor | major
git tag v0.1.0 && git push origin v0.1.0
```

Prefer not to check out the repo locally? Dispatch the **Bump version**
workflow from the [Actions](https://github.com/Luis85/vault-buddy/actions)
tab (`version` input takes an explicit `X.Y.Z` or `patch`/`minor`/`major`) —
it runs `scripts/bump-version.mjs` on `main` and opens a PR with the version
bump for you to review and merge before tagging.

### In-app updates (updater signing)

Installed apps self-update from Settings → Updates. Updates are verified
against a dedicated updater keypair (independent of Windows code signing):

- the **public key** lives in `src-tauri/tauri.conf.json` under
  `plugins.updater.pubkey`
- the **private key** must exist as the repository secrets
  `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` —
  both CI and the release workflow need them to build
  (`bundle.createUpdaterArtifacts` signs at build time)

Generate a keypair once with `npx tauri signer generate -w <path>` and keep
the private key safe: whoever holds it can ship updates to every user. The
release workflow attaches a `latest.json` manifest to each GitHub release;
installed apps poll
`releases/latest/download/latest.json` and offer the update in the settings
panel (download, signature check, install, relaunch — always user-initiated,
per the PRD's Human in Control principle).

## Development with coding agents

Agent-facing guidance — commands, architecture invariants, conventions, and
the release flow in one place — lives in [`AGENTS.md`](../AGENTS.md) at the
repo root, where coding agents (Claude Code, Codex, Cursor, ...) pick it up
automatically. [`CLAUDE.md`](../CLAUDE.md) points Claude Code at it. Keep
`AGENTS.md` current when the repo changes.

### Superpowers skills

This repository vendors the [obra/superpowers](https://github.com/obra/superpowers)
agentic skills framework directly into [`.claude/skills/`](../.claude/skills),
rather than depending on the plugin marketplace. The skills are checked into
version control, so every collaborator gets them automatically — no
marketplace, install, or trust step required.

Included skills:

- `brainstorming` — turn ideas into designs before implementation
- `writing-plans` / `executing-plans` — plan authoring and execution
- `test-driven-development` — red/green/refactor discipline
- `systematic-debugging` — root-cause tracing and defense-in-depth
- `requesting-code-review` / `receiving-code-review` — review workflows
- `subagent-driven-development` / `dispatching-parallel-agents` — subagent orchestration
- `using-git-worktrees` / `finishing-a-development-branch` — branch workflows
- `verification-before-completion` — pre-completion checks
- `writing-skills` — authoring new skills
- `using-superpowers` — meta-skill that coordinates the rest

Claude Code discovers these on the next session (or after `/reload-plugins`).
Model-invoked skills trigger automatically from their descriptions; you can
also invoke one explicitly, e.g. `/brainstorming`.

A `SessionStart` hook ([`.claude/hooks/session-start`](../.claude/hooks),
wired in [`.claude/settings.json`](../.claude/settings.json)) injects the
`using-superpowers` meta-skill at the start of every session — so Claude
consults the skills library proactively rather than only when a description
happens to match. The hook is a cross-platform polyglot wrapper
(`run-hook.cmd`) that runs under both Windows (Git Bash) and Unix shells.

To update the vendored copies, re-pull the `skills/` directory from the
upstream [obra/superpowers](https://github.com/obra/superpowers) repository.

## Logs & crash reporting

Log folder: `%LOCALAPPDATA%\com.vaultbuddy.desktop\logs` (tray → Open logs
folder, or Settings).

- `vault-buddy.log` — the rotating app log (5 MB, one rotated file kept).
  Frontend diagnostics funnel into it too (`src/logging.ts`).
- `crash.log` — Rust panic records (thread, location, backtrace) written
  synchronously by the panic hook. Every record also carries the app
  version and OS/architecture. A panic in the first instants of startup
  lands in `%TEMP%\vault-buddy-crash.log` and is folded into `crash.log` on
  the next launch.
- Native faults (a SEH exception on Windows — WebView2, GPU, or
  audio-driver crashes — or a fatal signal on Unix) are now caught by an
  OS-level crash handler (the `crash-handler` crate) and also land in
  `crash.log`, as a `native crash …` record with the exception code
  (Windows) or signal number (Unix), version, and OS — but no stack, and
  no fault timestamp (see the record's timestamp field, which instead
  points at the tail of `vault-buddy.log`). The handler runs in an
  already-crashed process, possibly with the heap lock still held by
  whatever corrupted it, so it does zero allocation on the path that
  matters: the record text is preformatted once at startup, and at fault
  time the handler only writes those bytes plus the exception/signal code
  (rendered into a fixed stack buffer, no `format!`) through a `crash.log`
  handle opened in advance. Only a fault in the first few milliseconds of
  startup, before that handle is ready, falls back to opening a file at
  crash time (best-effort, and the one place this path may still
  allocate). A main-thread Rust panic on Windows can produce **both** a
  panic record and a native-crash record for the same event (the panic
  unwinds into an abort, which the native handler also observes) — read
  two records with matching timestamps as one crash, not two.
- `.vault-buddy.run` — the run marker. If a session ends without passing
  through a graceful exit path, the next launch logs a warning and shows a
  notification that the previous session ended uncleanly. The notification
  distinguishes two cases by checking whether `crash.log` holds a record at
  least as new as the stale marker: **a crash record is present** ("Vault
  Buddy crashed last time" — see crash.log) versus **no record** ("Vault
  Buddy didn't shut down cleanly" — see vault-buddy.log instead). The
  second case is not rare: a native WebView2/GPU/audio-driver fault that
  happens while interacting with another window (e.g. dragging the buddy
  over another app) commonly kills the process before any handler runs, so
  no crash.log record ever gets written — the previous notification wording
  ("see crash.log") was misleading in exactly this case. Crash detection
  also re-arms itself automatically if an update install fails after the
  updater's pre-install step already stamped the marker "clean" — the
  frontend tells Rust to turn detection back on since the process is
  clearly still running.
- `Vault Buddy.log` (if you still have one) — the pre-v0.2.2 default-named
  log; the app no longer writes it, safe to delete manually.

Honest limitation: a kill or power loss allows no in-process write at
all, native crash handler included — the run marker is the only signal,
detected at the next launch. For a full native crash dump (not just the
one-line record above), enable Windows Error Reporting LocalDumps for
`vault-buddy.exe` (see
[Collecting user-mode dumps](https://learn.microsoft.com/en-us/windows/win32/wer/collecting-user-mode-dumps)).

## Capture configuration

Per-vault capture settings live app-side in `%APPDATA%\vault-buddy\config.json`
(keyed by Obsidian vault ID — the key from `obsidian.json`). The file is
optional; missing files, entries, or fields fall back to defaults. Nothing is
ever written into your vaults except recordings and their notes.

```json
{
  "vaults": {
    "<vault-id>": {
      "mode": "meeting",          // "meeting" (mic + desktop audio) | "voice-note" (mic only)
      "recordingFolder": "Meetings", // optional — omit for the mode default ("Meetings" / "Voice Notes")
      "bitrateKbps": 128,          // 128 | 160 | 192
      "createNote": true,          // companion .md with metadata + embed
      "followUpTemplate": true,    // append a "## Follow-up" scaffold to the companion note (needs createNote)
      "inputDevice": "USB Mic",    // optional — cpal device name; omit for system default
      "outputDevice": "Speakers",  // optional — loopback source (Meeting mode); omit for system default
      "transcribe": false,         // opt in to local speech-to-text
      "transcriptionModel": "small", // "base" | "small" | "medium"
      "transcriptionLanguage": "es", // optional — omit to auto-detect per recording
      "transcriptTimestamps": true  // prefix each segment with [HH:MM:SS]
    }
  }
}
```

- `followUpTemplate` (bool, default `true`) — append a `## Follow-up`
  scaffold (action items, decisions, notes) to each recording's companion
  note. Only applies when `createNote` is on.
- `transcribe` (bool, default `false`) — opt in to local speech-to-text.
  Enabling it downloads a Whisper model on the next recording (or backfills
  existing recordings) and writes a `<name>.transcript.md` sidecar the note
  embeds.
- `transcriptionModel` (`"base"` | `"small"` | `"medium"`, default
  `"small"`) — accuracy/speed/size trade-off. Models download to
  `%APPDATA%\vault-buddy\models`.
- `transcriptionLanguage` (string or omit, default auto-detect) — e.g.
  `"es"`; omit to auto-detect per recording.
- `transcriptTimestamps` (bool, default `true`) — prefix each segment with
  `[HH:MM:SS]`.

The file is written by the panel's per-vault ⚙ form (atomic temp + rename); it stays hand-editable and malformed fields still degrade per-field to defaults; a configured device that is missing at record time falls back to the system default with a warning.

### Transcription dependencies

The local speech-to-text path pins `whisper-rs` 0.16 / `whisper-rs-sys` 0.15
deliberately: `src-tauri/transcribe/src/engine.rs` hand-wires the abort and
progress callbacks around upstream whisper-rs bugs (abort UB #277; the
progress/language closure leaks). A future whisper-rs that fixes these would
let us delete that wiring — tracked as a standalone upgrade, not done casually,
having just stabilized the engine. `sha2` verifies downloaded model integrity;
`symphonia` (MP3-only) decodes; `ureq` downloads with connect/idle timeouts.
Full review: `docs/superpowers/specs/2026-07-07-transcription-reliability-and-verification-design.md`.
