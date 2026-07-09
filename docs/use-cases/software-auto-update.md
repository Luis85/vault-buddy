---
type: UseCase
status: shipped
domain: platform
shipped_in: v0.3.0 (approximate — first version with the updater plugin wired up)
source_prd: none
tags: [use-case, platform, undocumented-in-prd]
---

# Software Auto-Update

> Vault Buddy checks GitHub Releases for a newer signed build, downloads it, and relaunches itself into the new version — entirely from the Settings panel.

## ⚠ Not mentioned in any PRD

This is a fully shipped, load-bearing capability (its own Pinia store, IPC command, signing pipeline, and release workflow) with **no corresponding entry in any PRD** — not in the main PRD's Core Capabilities/Functional Requirements, not in a `docs/prds/*.md` capability PRD, and not in the Knowledge Lifecycle or AI Platform vision docs. It exists only as an implementation detail inside `AGENTS.md` (§ Updater flow, § Releases). Flagged per the request to surface implemented use-cases missing dedicated PRD coverage.

## Status: Shipped

## Implementation

- Frontend: `src/stores/updates.ts` (phase machine: `idle → checking → upToDate → available → installing → error`), `UpdateSettings.vue`.
- Flow: check → download (panel stays open so progress/errors are visible) → `close_panel` → `prepare_update_install` (Rust saves the buddy's window position and stamps a clean-shutdown marker) → `install()` → `relaunch()`. On failure the panel reopens on the settings view via `toggle_panel`, and `rearm_crash_detection` re-arms the crash marker the prepare step had latched off.
- Rust: `prepare_update_install` (`src-tauri/src/commands.rs`) — must stay a synchronous, main-thread command for the same reason all window-state saves are main-thread-only (see AGENTS.md § Window-state saves).
- Distribution: `tauri-plugin-updater`, polling `latest.json` from GitHub Releases (`src-tauri/tauri.conf.json`); the `Release` GitHub Actions workflow (`tauri-action`) signs updater artifacts with `TAURI_SIGNING_PRIVATE_KEY` and attaches `latest.json`. CI builds without updater artifacts when signing secrets are absent (forked PRs).
- Version bump tooling: `scripts/bump-version.mjs`, the `Bump version` workflow — see [`docs/superpowers/specs/2026-07-06-version-bump-automation-design.md`](../superpowers/specs/2026-07-06-version-bump-automation-design.md).

## Recommendation

Consider adding a short "Software Updates" section to the main PRD's Functional/Non-Functional Requirements (it fits naturally under Reliability — "Automatic recovery" is adjacent but distinct) so this capability is no longer undocumented at the product level.
