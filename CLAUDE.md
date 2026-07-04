# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

The single source of truth for agent guidance is @AGENTS.md — commands,
architecture, invariants, testing and commit conventions, CI/release flow.
Read it before making changes; keep it (not this file) up to date when the
repo changes.

Also injected automatically: the superpowers skills framework vendored in
`.claude/skills/` (via the SessionStart hook in `.claude/settings.json`) —
brainstorm before building, TDD for every change, systematic debugging for
every bug.
