# Polish Sub-pass E — Tech-Debt Splits & Docs Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Retire the LOC-allowlist split obligations (`tasks.rs`, `Tasks.vue`), extract the ActionPanel/VaultList button clone the fallow ratchet counts, fix the stale human-facing doc references (GAP-49/50 + the D-review's two doc nits), and run the pass-wide close-out sweep (Gaps.md, PR body, baselines, gates).

**Architecture:** E1/E2/E3 are **move-only refactors** — no behavior change is permitted in the same commit; the full test suites must pass unmodified apart from import paths. E4/E5 are docs. Baselines (LOC allowlist, fallow) re-ratchet DOWNWARD in the same commit as the split that earns it.

**Tech Stack:** Rust module system (re-exports), Vue 3 SFC components (props down / events up), markdown.

## Global Constraints

- **Branch:** `claude/task-management-vertical-slice-ikeuly`. Never push elsewhere; never amend/rebase existing commits.
- **Move-only discipline (E1–E3):** the diff for a split commit must consist of moved code, module/import wiring, and re-exports. If you find a bug while moving, STOP and report it — it gets its own commit after the move, never folded in.
- **Public-API freeze (E1):** every path currently used by callers (`tasks::TaskItem`, `tasks::list_tasks`, `tasks::set_fields`, `tasks::set_status`, `tasks::update_task_fields`, `tasks::set_task_status`, `tasks::create_task`, `tasks::render_task`, `tasks::task_basename`, `tasks::is_task`, `tasks::is_valid_due`, `tasks::is_valid_tag`, `tasks::note_tags`, `tasks::priority_rank`) must keep compiling verbatim via `pub use` re-exports in `tasks/mod.rs`. Verify with `grep -rn 'tasks::' src-tauri/src src-tauri/core/src/services.rs` before and after.
- **LOC caps:** every new file must land under its surface cap (Rust 800, frontend 500) — that is the point. Delete the allowlist entry in the same commit (`npm run check:loc` must pass with the entry GONE). If `Tasks.vue`'s container legitimately cannot reach 500, re-record it at the reduced size with an updated reason (spec E2 allows this fallback — say so in the commit body).
- **Gates per task:** `npm test`, `npm run lint`, `npm run check:loc`, `npm run check:quality` (frontend tasks); `cargo test` + `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check` in `src-tauri/core` (E1); the full sweep in E5.
- **Commits:** Conventional Commits (`refactor(core)`, `refactor(ui)`, `docs`, `chore(quality)`), one per task, ending with the two trailers:
  `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` and
  `Claude-Session: https://claude.ai/code/session_01EEPQK9Ns3ULMrVuwU3UeuU`

---

### Task E1: Split `core/src/tasks.rs` into focused modules

**Files:**
- Create: `src-tauri/core/src/tasks/mod.rs`, `tasks/doc.rs`, `tasks/parse.rs`, `tasks/writer.rs`, `tasks/list.rs`, `tasks/disk.rs`
- Delete: `src-tauri/core/src/tasks.rs` (contents move; `lib.rs`'s `pub mod tasks;` line is unchanged — a directory module replaces the file)
- Modify: `scripts/loc-baseline.json` (DELETE the tasks.rs entry)

**Interfaces:** the module map (line numbers from the current file, `grep -n` to re-verify before moving):

| New module | Items (with their `#[cfg(test)]` tests) |
| --- | --- |
| `doc.rs` | `has_closed_frontmatter`, `is_task` — the two document-identity primitives BOTH the writer and the list depend on (`pub(crate)` for the former, re-exported `pub` for the latter) |
| `parse.rs` | `is_valid_due`, `is_valid_tag`, `normalize_tag`, `dedupe_tags`, `strip_inline_comment`, `strip_scalar_tags_comment`, `scalar_field`, `parse_tags_key`, `note_tags` |
| `writer.rs` | `set_fields`, `set_status` |
| `list.rs` | `TaskItem`, `priority_rank`, `due_key`, `list_tasks`, `collect_task_file` |
| `disk.rs` | `slugify`, `task_basename`, `render_task`, `create_task`, `update_task_fields`, `set_task_status` |
| `mod.rs` | `mod` declarations + `pub use` re-exports reproducing today's `tasks::*` surface exactly; the module-level doc comment moves here |

Cross-module visibility: items used across modules but not public today (`scalar_field`, `strip_inline_comment`, `due_key`, …) become `pub(super)`; do NOT widen anything to `pub` that isn't already.

- [ ] **Step 1:** `grep -n` the current file to confirm the item list above still matches HEAD (mid-run Codex fixes have been landing in this file — flag any new item and place it with its siblings).
- [ ] **Step 2:** Create the six files, moving each item + its doc comment + its tests verbatim. The big `mod tests` block splits by what each test exercises; a test touching two modules goes with the module whose behavior it pins (e.g. reader/writer agreement tests live in `writer.rs`'s tests since they assert `set_fields`).
- [ ] **Step 3:** `cargo test` in `src-tauri/core` — identical pass count to before the move (currently `cargo test 2>&1 | grep 'test result'`; record before/after).
- [ ] **Step 4:** `cargo clippy --all-targets -- -D warnings && cargo fmt --check`; then `grep -cve '^[[:space:]]*$' src-tauri/core/src/tasks/*.rs` — every file under 800.
- [ ] **Step 5:** Delete the `src-tauri/core/src/tasks.rs` allowlist entry from `scripts/loc-baseline.json`; `npm run check:loc` passes.
- [ ] **Step 6:** Verify callers: `cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings` is NOT runnable here without GUI deps unless already installed — if installed run it; otherwise `cargo check -p vault_buddy_core -p vault_buddy_mcp` plus grep-verify `services.rs`/`task_commands.rs` import paths unchanged.
- [ ] **Step 7:** Commit `refactor(core): split tasks.rs into doc/parse/writer/list/disk modules` with the move-only statement + before/after test counts in the body.

### Task E2: Extract `Tasks.vue`'s row and editor into child components

**Files:**
- Create: `src/components/TaskRow.vue` (checkbox, title button, vault chip, tag chips, due chip, priority dot, archive + pencil buttons), `src/components/TaskEditor.vue` (the inline editor row: title/due/priority/tags inputs, Save/Cancel)
- Modify: `src/components/Tasks.vue` (renders the children; keeps ALL state — list, filters, buckets, busy set, editingKey, optimistic mutations), `tests/tasks.test.ts` (only if selectors must change — prefer keeping `data-testid`s identical so tests stay untouched)
- Modify: `scripts/loc-baseline.json` (delete or re-record the Tasks.vue entry per the Global Constraint)

**Interfaces:** props down / events up, no store access in the children:
- `TaskRow` props `{ task: AggTask, busy: boolean, isAggregate: boolean, editing: boolean }`, emits `toggle`, `archive`, `edit`, `open`, `tagClick(tag)`.
- `TaskEditor` props `{ task: AggTask, busy: boolean }`, emits `save(patch: TaskPatch)`, `cancel`. The IME-guarded Enter/Escape handlers move WITH the editor.
- All `data-testid` attributes keep their exact current values — the Vitest suite must pass unmodified. If a test reaches into component internals that moved, adapt the test minimally and say so in the report.

- [ ] **Step 1:** Move the row template + its script bindings into `TaskRow.vue`; run `npx vitest run tests/tasks.test.ts tests/action-panel.test.ts` — green, unmodified.
- [ ] **Step 2:** Move the editor into `TaskEditor.vue`; rerun — green.
- [ ] **Step 3:** Full gates: `npm test && npm run lint && npm run build`; `grep -cve '^[[:space:]]*$'` on all three files — children under 500; update/delete the allowlist entry accordingly; `npm run check:loc && npm run check:quality` (fallow complexity should DROP — if the ratchet now beats the baseline, `npm run check:quality -- --update` in the same commit and note the improvement).
- [ ] **Step 4:** Commit `refactor(ui): extract TaskRow and TaskEditor from Tasks.vue`.

### Task E3: Extract the shared icon-button-bar clone (fallow groups a0359856 / 920f14c5)

**Files:**
- Create: `src/components/PanelActionButton.vue` (or fold into an existing pattern if inspection shows one — the clone is the icon + label + optional count-badge button markup shared by ActionPanel's All-tasks bar/header buttons and VaultList's row action buttons)
- Modify: `src/components/ActionPanel.vue`, `src/components/VaultList.vue`
- Modify: `scripts/quality-baseline.json` (`npm run check:quality -- --update` — cloneGroups/duplicatedLines must DROP)

- [ ] **Step 1:** Run `npm run quality` and read the two clone groups' exact line ranges; design the smallest component that dissolves BOTH (props: icon slot, label, badge count, disabled, testid passthrough).
- [ ] **Step 2:** Replace both sites; `data-testid`s unchanged; `npm test` green unmodified.
- [ ] **Step 3:** `npm run check:quality -- --update` — verify cloneGroups ≤ 2 and duplicatedLines < 100 in the new baseline (an INCREASE is a task failure); lint/loc/build green.
- [ ] **Step 4:** Commit `refactor(ui): shared panel action button dissolves the ActionPanel/VaultList clones`.

### Task E4: Human-facing doc fixes (GAP-49 + GAP-50 rename + D-review nit)

**Files:**
- Rename: `docs/PRD - Product Vision.md` → `docs/PRD.md` (`git mv`)
- Modify: `README.md`, `docs/DEVELOPMENT.md`, `AGENTS.md`, `docs/use-cases/README.md` (+ any other referrer `grep -rl 'PRD%20-%20Product%20Vision\|PRD - Product Vision' --include='*.md'` finds), `.github/pull_request_template.md`, `.github/workflows/release.yml` (stale comment), `src-tauri/transcribe/Cargo.toml` (stale comment), `docs/Gaps.md` (GAP-49 tombstone; GAP-50 partial annotation)

Item list (each is one focused edit — the GAP-49 entry in Gaps.md has the details):
1. README PRD link → `docs/PRD.md` (fixes the front-page 404).
2. DEVELOPMENT.md: crate list "three crates" → the real five (core, capture, transcribe, mcp + shell); test-command list gains the transcribe + mcp crates CI actually runs; the signing sentence ("needed to build") → CI builds unsigned without them, and per GAP-36 ALL PR builds are now unsigned by design.
3. AGENTS.md:~1072/~1090: same signing-wording fix ("forks" → all PR events, main-push + release only).
4. PR template: drop "can't compile in this container", name all four CI jobs without implying sequence.
5. `docs/PRD.md` status line → shipped reality (0.5.x, Search + Tasks shipped; MCP v0.6.0 per its own section).
6. release.yml: delete the stale "npm script aliases tauri dev" comment; transcribe/Cargo.toml: comment now points at the Linux rust-core whisper gate.
- [ ] **Step 1:** Make all edits; `grep -rn 'PRD%20-%20Product%20Vision\|PRD - Product Vision' --include='*.md' .` returns ONLY the Gaps.md tombstone text (which describes the old name historically).
- [ ] **Step 2:** `npm run check:loc` (doc rename must not confuse it — it only scans src/), yaml-validate release.yml, `cargo fmt --check` untouched.
- [ ] **Step 3:** Commit `docs: fix the stale references GAP-49 catalogued; rename the PRD (GAP-50)` tombstoning GAP-49 and annotating GAP-50's rename bullet.

### Task E5: Pass close-out sweep

**Files:**
- Modify: `docs/Gaps.md` (final consistency pass), `.superpowers/sdd/progress.md` (close E + the pass), PR #46 body (via the controller — the subagent DRAFTS the body section in the report)

- [ ] **Step 1:** Gaps.md sweep: §7's devices.rs bullet rewritten (Windows CI compiles + smoke-runs, device-less runners still never exercise real devices); §8's GAP-45/46/47 entries annotated for what E1–E3 paid down; verify every FIXED tombstone this pass claims matches HEAD reality (`grep -c 'FIXED 2026-07-10' docs/Gaps.md` and spot-check three).
- [ ] **Step 2:** Full verification sweep: `npm test && npm run lint && npm run check:loc && npm run check:quality && npm run build`, then in `src-tauri`: `cargo fmt --check`, `cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp`, `cargo clippy` on the same four, and `npx tauri build --no-bundle` if GUI deps are present (say if skipped).
- [ ] **Step 3:** DRAFT the PR-body polish section covering: the five sub-passes one line each; the LOC justifications (tasks.rs 1431→1481 pre-split, Tasks.vue 792→809, capture.ts 602→621 — now largely paid down by E1/E2); the GAP-34/D1 Windows runtime CSP verification note (three windows render, updater/settings work, one-line revert); the release-time note (validate job proves itself on the next real tag/dispatch — expect fail-closed if CI hasn't completed).
- [ ] **Step 4:** Commit `docs(gaps): polish-pass close-out sweep`.

## Self-Review

- Spec coverage: E1→spec E1, E2→spec E2, E3→spec E3, E4→spec E4 (+D-review AGENTS.md nit), E5→spec E5 (+D-review devices.rs nit + PR-body obligations). ✓
- No placeholders; the module map and interface contracts are concrete. ✓
- Type consistency: `AggTask`/`TaskPatch` names match `Tasks.vue`'s existing exports; `tasks::` re-export list matches the current public surface (verified by grep in E1 Step 1). ✓
