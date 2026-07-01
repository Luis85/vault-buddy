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

This repository is configured to use the
[obra/superpowers](https://github.com/obra/superpowers) agentic skills framework
for Claude Code. The marketplace and plugin are declared at project scope in
[`.claude/settings.json`](.claude/settings.json), so collaborators are prompted to
install them after trusting the repository folder.

If the plugin does not load automatically, install it manually:

```shell
/plugin marketplace add obra/superpowers-marketplace
/plugin install superpowers@superpowers-marketplace
```

Then run `/reload-plugins` to activate it.
