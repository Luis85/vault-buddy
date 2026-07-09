---
type: UseCase
status: shipped
domain: platform
shipped_in: v0.3.0 (approximate — first version with the updater plugin wired up)
source_prd: "docs/prds/platform-and-cross-cutting.md"
tags: [use-case, platform]
---

# Software Auto-Update

> Vault Buddy checks GitHub Releases for a newer signed build, downloads it, and relaunches itself into the new version — entirely from the Settings panel.

## Source

[Platform & Cross-Cutting Capabilities PRD](../prds/platform-and-cross-cutting.md) § Capability: Software Auto-Update. This PRD was written specifically to give this shipped-but-previously-undocumented capability a home — see that PRD's Vision section for why. Before this PRD existed, this note flagged the gap directly; it existed only as an implementation detail inside `AGENTS.md` (§ Updater flow, § Releases).

## Status: Shipped

## Implementation

- Frontend: `src/stores/updates.ts` (phase machine: `idle → checking → upToDate → available → installing → error`), `UpdateSettings.vue`.
- Flow: check → download (panel stays open so progress/errors are visible) → `close_panel` → `prepare_update_install` (Rust saves the buddy's window position and stamps a clean-shutdown marker) → `install()` → `relaunch()`. On failure the panel reopens on the settings view via `toggle_panel`, and `rearm_crash_detection` re-arms the crash marker the prepare step had latched off.
- Rust: `prepare_update_install` (`src-tauri/src/commands.rs`) — must stay a synchronous, main-thread command for the same reason all window-state saves are main-thread-only (see AGENTS.md § Window-state saves).
- Distribution: `tauri-plugin-updater`, polling `latest.json` from GitHub Releases (`src-tauri/tauri.conf.json`); the `Release` GitHub Actions workflow (`tauri-action`) signs updater artifacts with `TAURI_SIGNING_PRIVATE_KEY` and attaches `latest.json`. CI builds without updater artifacts when signing secrets are absent (forked PRs).
- Version bump tooling: `scripts/bump-version.mjs`, the `Bump version` workflow — see [`docs/superpowers/specs/2026-07-06-version-bump-automation-design.md`](../superpowers/specs/2026-07-06-version-bump-automation-design.md).

## Related use-cases

- [Diagnostics, Crash Reporting & Recovery](diagnostics-and-crash-reporting.md) — shares the clean-shutdown marker mechanism.
