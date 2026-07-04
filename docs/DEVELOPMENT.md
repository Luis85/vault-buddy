# Developing Vault Buddy

Contributor documentation: building from source, tests, the CI/release
pipelines, and the agentic development setup. For what the product is and
where it's going, see the [PRD](PRD.md).

## Run it from source

### Prerequisites (Windows)

1. [Node.js 22+](https://nodejs.org)
2. [Rust stable](https://rustup.rs) — the default MSVC toolchain; rustup will
   prompt you to install the Visual Studio C++ Build Tools if missing
3. WebView2 runtime — preinstalled on Windows 11; on Windows 10 see the
   [Tauri prerequisites](https://tauri.app/start/prerequisites/)

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
cd src-tauri && cargo fmt --check  # Rust formatting (whole workspace)
cd core && cargo clippy --all-targets -- -D warnings
cargo test                         # pure Rust core tests (run anywhere)
```

The Rust code is split in two: `src-tauri/core/` is a pure crate with all
Obsidian logic (config parsing, daily-note resolution, URI building) and no
GUI dependencies — it tests on any machine, including CI containers.
`src-tauri/` is the thin Tauri shell (window, tray, command wrappers) and
needs platform WebView libraries to compile — on Windows that works out of
the box; Linux containers can't compile it (no webkit2gtk), which is why CI
builds the app on a Windows runner.

## Quality pipeline

CI runs on every push to `main` and every pull request
([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)):

| Job | Runner | What it gates |
| --- | --- | --- |
| Frontend | Linux | Vitest tests, `vue-tsc` typecheck, production build |
| Rust core | Linux | `cargo fmt --check`, `clippy -D warnings`, core unit tests |
| Windows app | Windows | Full Tauri compile + MSI/NSIS installers, uploaded as artifacts (14-day retention) |

The Windows job only runs after the two fast jobs pass. Desktop behavior that
can't be asserted in CI (transparency, tray, drag, the real Obsidian
round-trip) is covered by the manual verification checklist in
[`docs/superpowers/specs/`](superpowers/specs/).

## Releases

A separate [release workflow](../.github/workflows/release.yml) runs on `v*`
tags: it builds the Windows installers and publishes them as a GitHub
Release for end users.

```bash
# after bumping the version in tauri.conf.json / package.json / Cargo.toml
git tag v0.1.0 && git push origin v0.1.0
```

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
