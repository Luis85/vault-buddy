# Vault Buddy

An AI-native desktop companion for knowledge work — starting as the best desktop
companion for Obsidian and evolving into an extensible, local-first desktop
operating layer.

- **Platform:** Windows (MVP)
- **Stack:** Tauri 2 · Vue 3 · TypeScript · Rust
- **Status:** Product Discovery

See the full [Product Requirements Document](docs/PRD.md) for vision, principles,
capabilities, architecture, and roadmap.

## Development with Superpowers

This repository vendors the [obra/superpowers](https://github.com/obra/superpowers)
agentic skills framework directly into [`.claude/skills/`](.claude/skills), rather
than depending on the plugin marketplace. The skills are checked into version
control, so every collaborator gets them automatically — no marketplace, install,
or trust step required.

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
Model-invoked skills trigger automatically from their descriptions; you can also
invoke one explicitly, e.g. `/brainstorming`.

A `SessionStart` hook ([`.claude/hooks/session-start`](.claude/hooks), wired in
[`.claude/settings.json`](.claude/settings.json)) injects the `using-superpowers`
meta-skill at the start of every session — so Claude consults the skills library
proactively rather than only when a description happens to match. The hook is a
cross-platform polyglot wrapper (`run-hook.cmd`) that runs under both Windows
(Git Bash) and Unix shells.

To update the vendored copies, re-pull the `skills/` directory from the upstream
[obra/superpowers](https://github.com/obra/superpowers) repository.
