# Tutorial Editor Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the verified findings of the 2026-09-25 post-merge review and the triaged open Tutorial Editor gaps, without changing any shipped behaviour the user has not approved.

**Architecture:** Each task is a small, independently reviewable hardening change to an existing module (no new subsystem). Rust changes follow the existing seams (`*_in(state, …)` testable cores behind thin `AppHandle` wrappers, `ImportIo`/`TakeIo`-style injectable I/O). Frontend changes stay inside the existing stores, the one shortcut dispatcher and the design tokens. Every task updates its own docs (AGENTS.md, docs/Gaps.md, ADR §9, the evidence file, the checklist) in the same commit.

**Tech Stack:** Tauri 2.11.5 (Rust 2021, workspace crates `core`, `screen`, shell), Vue 3 + Pinia + Tailwind 4, Vitest (happy-dom + mockIPC), Playwright (built `dist/`).

**Spec:** `docs/superpowers/specs/2026-09-25-tutorial-editor-post-merge-review.md` (the findings, IDs I-*, M-*, C-*, D-*, S-*, F-M*, T-*), `docs/Gaps.md` (GAP-170–216), the ADR `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md` (governs; §8 invariants, §9 drifts), and the previous run's rulings (`tutorial-editor-rulings.md`, 101 lines — decided; re-open only with evidence and the user's approval). Full reviewer reports with extra line detail were saved alongside this plan's execution ledger.

## Global Constraints

Copied from the previous plan's constraints as amended by the user; they bind every task and every reviewer.

- TDD red-first: see each new test fail for the stated reason before implementing; mutation-check each fix (revert the fix, the new test must go red).
- Every gate per task (Windows host): `npm run lint && npm run check:loc && npm run check:quality && npm run build && npm run test:e2e && npm run test:coverage`; `cargo fmt --check`; `cargo clippy --workspace --all-targets -- -D warnings`; `cargo clippy -p vault-buddy --all-targets -- -D warnings`; `cargo test -p vault_buddy_core -p vault_buddy_screen`; `cargo test -p vault-buddy --lib`; `npx tauri build --no-bundle` when the shell changes. `cargo machete` / `cargo deny` / `cargo llvm-cov` are not installed: report them NOT RUN (CI gates them). Known flakes: GAP-169 `core::tasks::disk` "Access denied", and the wall-clock `the_remux_is_faster_than_re_encoding_the_same_fixture` — rerun alone and report honestly. IDE TS diagnostics are stale; judge by `npx vue-tsc --noEmit`.
- LOC caps: 800 nonblank per Rust file incl. inline tests; 500 per `.ts`/`.vue`. No new allowlist entry. Split BEFORE growing (measure with `grep -cv '^\s*$' <file>`).
- Quality ratchet (`scripts/quality-baseline.json`): deadCode 0 with NO fallow suppressions, complexFunctions 13, criticalComplexity 3, cloneGroups 0, circularDependencies 0. averageMaintainability (90.6) may drop only with measured justification folded into the ONE "Tutorial Editor" sentence of its description (edit in place), never below 90.0. Hand-edit the file (keep literal em dashes); never `--update`.
- A new command needs five edits: `lib.rs` `generate_handler!`, `build.rs` `ALL_COMMANDS`, exactly one capability grant (`editor_*` → `capabilities/editor.json` with `require_editor_window` first; anything else → `default.json`), and the AGENTS.md IPC row plus the MEASURED count. (No task in this plan adds a command.)
- Never put a path, file name, caption or title into a log or an error message (`redact::redact_path` / `redact_name`). The user sees role wording, never a `<path:#…>` handle. Watch for lost `\` string continuations; assert whole strings.
- Never force-push, never bare `git stash`, never `git clean -fdx`, never `remove_dir_all`. Commits are Conventional Commits ending with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` (a subagent may name the model that actually wrote it — previous ruling).
- Keep AGENTS.md, docs/Gaps.md (close or amend entries; new ones after the highest number, currently GAP-216), ADR §9, the evidence file (`tests/editorEvidence.test.ts` stays green) and the checklist (append rows at the next free T-number, currently T69; never delete; re-measure its header count with its own one-liner) in step with the code.
- Do not push.

## Review Focus

1. **Two windows acting on one capture at once** (panel Edit / Discard / Clear while the editor discards or opens the same project): a reasonable person expects one of the two to win cleanly — never a pin to nothing, never a dead editor. Pinned by Task 2's race tests and Task 4's dangling-pin test.
2. **Non-US keyboards and focus on form controls** (AltGr layouts; Delete/S while a `<select>` or the rate control has focus): a reasonable person expects typing in a control never to edit the timeline. Pinned by Task 13's Alt/AltGr/select cases.
3. **Hand-edited, foreign or crafted files** (a minified lightweight project, an unreadable journal or ledger, a package with bidi names): the app refuses with role wording and leaves nothing it cannot later remove. Pinned by Tasks 5, 10 and the D-6 Gaps entry.
4. **Leftovers after a crash** (a `.part`, a `jobs\` folder, an unrecorded product, a review render, a webcam take part, a half-created project): startup sweeps remove only OUR stale plain files, never a symlink, never a fresh file, never anything the ledger or a live session owns. Pinned by Task 7's keep-cases (symlink, fresh file, recorded product, unreadable ledger).
5. **The light theme and Windows contrast themes on every surface a task touches**: text ≥ 4.5:1, hover affordances visible. Pinned by Task 14's e2e sweep extension.

## Decision gates (the user decides before execution; see "Decisions" at the end)

Tasks marked **[GATED Dn]** are executed only if the user approves option D*n*; otherwise the task is replaced by its "if declined" docs-only step.

---

## Batch 0 — Docs that currently mislead

### Task 1: Reconcile the measured doc drifts

**Model:** Sonnet (mechanical). **Files:** Modify `AGENTS.md`, `docs/Gaps.md`.

Findings: M-V13..16 and the docs minors of the review; triage GAP-170 / GAP-174 item 1 / GAP-198 / GAP-168 text; the stale `staged_commands` line count.

- [ ] **Step 1: Re-measure** (record the outputs in the commit body):
  ```bash
  awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -cE '^\s+[a-z_]+::[a-z_]+,$'   # expect 131
  grep -c '"allow-editor-' src-tauri/capabilities/editor.json                                  # expect 36
  grep -cv '^\s*$' src-tauri/src/staged_commands.rs                                           # expect 376
  ```
  and the checklist's own header one-liner in `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (expect 68).
- [ ] **Step 2: Edit AGENTS.md** — exactly these corrections:
  - repo map (`capabilities/` line, ~:169): "every command but the 35 editor_* ones) + editor.json (those 35," → "…the 36 editor_* ones) + editor.json (those 36,".
  - tutorial-editor "Verification status" (~:1935): "66 rows, none run" → "68 rows, none run".
  - `save_commands.rs` IPC row (~:565): replace "plus an always-empty `products` array (Task 46 populates it)" with "plus `record.products` assembled from the product ledger (`products.json`, Task 46; a damaged or duplicate-id ledger refuses the save)".
  - `publish.rs` row (~:573): `export_worker::vault_dir::prepare_export_dir` / `rollback_export_dir` → `editor::vault_dir::prepare_export_dir` / `rollback_export_dir` (`src-tauri/src/editor/vault_dir.rs`, moved from the retired `export_worker/` by Task 59).
  - `package_commands.rs` row (~:570): "an empty `products` list until Task 46's ledger exists" → "`record.products` from the product ledger (Task 46)".
  - Frontend state (~:3485): "driven by the nine `screen:*` events" → "eight".
  - Where state lives on disk (~:657): webcam block `{file, width, height, deviceLabel, offsetMs}` → `{file, width, height, deviceLabel, offsetMs, durationMs}` ("`durationMs` optional, absent before Task 52").
  - tutorial editor domain (~:1871): the sentence listing the window's non-editor `invoke`s gains `get_screen_capture_config` (Publish defaults), `list_vaults` and `open_screen_capture` (Publish dialog) — all through `src/editor/port.ts`.
  - `staging_commands.rs` row: "`staged_commands` sits at 677 of the 800-line Rust cap" → "`staged_commands` sits at 376 nonblank lines (its tests live in `staged_commands_tests.rs`)".
- [ ] **Step 3: Edit docs/Gaps.md**:
  - GAP-170 body: present-tense "the eight `editor_*` commands"/"those eight" → "the thirty-six `editor_*` commands (eight at Task 12; measured in `editor.json`)"/"those thirty-six"; "the other 91"/"the other 96" → "the other 95"; "all eight `editor_*` commands resolve as allowed" → "every `editor_*` command (36) resolves as allowed"; the Task-11 near-miss paragraph's "5 of 101 / 96 / all 101" gets the prefix "(at Task 11: 101 commands, 5 in the manifest; 131 / 36 / 95 today)".
  - GAP-174 item 1's recorded fix: replace "records whose asset id the project graph does not hold" with "records whose asset id neither the graph nor any retained product snapshot uses (`package_plan::assets_needing_media`) — a snapshot-only asset is what Restore needs".
  - GAP-198: lead with `screen_recovery/mod.rs` (paths 2–4); mark path 1's `export_worker/mod.rs` as "retired by Task 59".
  - GAP-168: `export_worker::vault_dir::a_symlinked_capture_folder_is_refused_before_anything_is_created` → `editor::vault_dir::…` (`src-tauri/src/editor/vault_dir.rs`).
  - GAP-208: replace the premise "a reload (Ctrl+R in a debug build, …)" with "a reload — F5 or Ctrl+R in ANY build: WebView2's browser accelerator keys are on by default (wry 0.55.1 `browser_accelerator_keys: true`, never switched off by tauri 2.11.5 or this app), so a release build reloads the editor too" and re-rate it **Medium**. (Task 15 may close it; this step only makes the entry true.)
- [ ] **Step 4: Run** `npx vitest run tests/editorEvidence.test.ts` → PASS.
- [ ] **Step 5: Commit** `docs: reconcile measured counts and stale paths after the tutorial editor merge`.

---

## Batch 1 — Discard and pin lifecycle

### Task 2: Serialize the session discard under `EditorState::open` (I-1) and unpin by sidecar scan (GAP-214 item 7)

**Model:** Opus (concurrency). **Files:**
- Create: `src-tauri/src/editor/session_close.rs` (split FIRST: move `close_in`, `close_locked` and their private helpers out of `session_commands.rs`, which is at 767/800; `session_commands.rs` re-exports nothing new — `editor_close_session` stays in `session_commands.rs` and calls `session_close::close_in`).
- Modify: `src-tauri/src/editor/session_commands.rs` (open paths: refuse a closing project), `src-tauri/src/editor/save_commands.rs` (`open_project_session`: same refusal), `src-tauri/src/editor/mod.rs` (doc of `open`, ~:68-73; `mod session_close;`), `src-tauri/src/editor/project_discard.rs` (make `unpin_everywhere` `pub(super)`).
- Test: `src-tauri/src/editor/session_commands_tests.rs` (or a new `session_close_tests.rs` if that file would pass 800), `discard_tests.rs`.

**Interfaces:**
- Produces: `pub(super) fn close_in(state: &EditorState, root: &Path, staging: &Path, session_id: &str, disposition: CloseDisposition) -> Result<(), EditorError>` (same signature as today, moved); `pub(super) fn refuse_if_project_closing(state: &EditorState, project_id: &str) -> Result<(), EditorError>` returning `invalidRequest` "This project is being discarded." when the project's live session is in `state.closing`.
- Consumes: `project_discard::unpin_everywhere(staging: &Path, project_id: &str)` (sidecar scan; never reads `sources.json`).

- [ ] **Step 1: Split without behaviour change.** Move `close_in`/`close_locked` into `session_close.rs`; run `cargo test -p vault-buddy --lib editor::` → all green; commit `refactor(editor): move the session close into its own module before it grows`.
- [ ] **Step 2: Write the failing race test** (shape: `staging_commands::tests::clear_waits_for_an_open_in_progress_and_keeps_what_it_pinned`, `staging_commands.rs:~200`):
  ```rust
  #[test]
  fn a_session_discard_waits_for_an_open_in_progress() {
      let fx = open_staged_fixture(); // existing helper: one pinned capture + live session
      let open_guard = lock_ignoring_poison(&fx.state.open);
      let state = fx.state.clone();
      let (root, staging, sid) = (fx.root.clone(), fx.staging.clone(), fx.session_id.clone());
      let discard = std::thread::Builder::new().name("t-discard".into()).spawn(move || {
          close_in(&state, &root, &staging, &sid, CloseDisposition::DiscardProject)
      }).unwrap();
      // Handshake, not a sleep: a #[cfg(test)] hook the discard sets immediately
      // BEFORE it locks `open`. Once it is set, the discard cannot unpin until we
      // release the guard, so the pin must still be there.
      wait_until(|| fx.state.test_hooks.discard_waiting_for_open.load(SeqCst), Duration::from_secs(5));
      assert_eq!(read_pin(&fx.staging, &fx.base), Some(fx.project_id.clone()));
      assert!(!fx.state.is_closing(&fx.session_id)); // the mark is taken UNDER `open`
      drop(open_guard);
      discard.join().unwrap().unwrap();
      assert_eq!(read_pin(&fx.staging, &fx.base), None);
      assert!(!fx.root.join(&fx.project_id).exists());
  }
  ```
  Add, test-only, `EditorState::test_hooks.discard_waiting_for_open: AtomicBool` (set in `close_in` right before `lock_ignoring_poison(&state.open)`) and `#[cfg(test)] fn is_closing(&self, session_id: &str) -> bool` (reads `closing` under the jobs lock, the order `mark_closing` uses). Use the fixture helpers the neighbouring tests already have; never a bare `sleep` to prove a negative (review T-7).
- [ ] **Step 3: Write the failing refusal test:**
  ```rust
  #[test]
  fn an_open_of_a_project_being_discarded_is_refused() {
      let fx = open_staged_fixture();
      let _mark = mark_closing(&fx.state, &fx.session_id).unwrap(); // discard.rs helper
      let err = open_staged_in(&fx.state, &fx.root, &fx.staging, &fx.base).unwrap_err();
      assert_eq!(err.code, EditorErrorCode::InvalidRequest);
      assert_eq!(err.message, "This project is being discarded.");
  }
  ```
  plus the same for `editor_open_project`'s core (`open_project_session`'s testable inner fn).
- [ ] **Step 4: Write the failing GAP-214 item 7 test:** a pinned capture whose project has NO `sources.json` (delete it in the fixture) → `close_in(DiscardProject)` still unpins the sidecar and removes the directory.
- [ ] **Step 5: Run** `cargo test -p vault-buddy --lib session_close` (and the discard filter) → the three tests FAIL for the stated reasons (discard does not wait; open reuses the closing session; unpin reads `sources.json`).
- [ ] **Step 6: Implement:** in `close_in`, for `DiscardProject`, `let _open = lock_ignoring_poison(&state.open);` BEFORE `mark_closing`, held through `close_locked`; in `close_locked` replace the `load_sources` → `unpin_staged` loop with `project_discard::unpin_everywhere(staging, project_id)`; call `refuse_if_project_closing` in `open_staged_in` (beside the `by_project` lookup) and in `open_project_session`; rewrite `mod.rs`'s doc: "`open` … taken by every open, every project removal and every pin writer — including a session's `discardProject`, which holds it from before its closing mark until the project is gone. Lock order: `open` → jobs → closing → save lock → `by_project` → `sessions`."
- [ ] **Step 7: Run** the three tests → PASS; mutation-check each (remove the `open` lock; remove the refusal; restore the `sources.json` loop) → each goes red.
- [ ] **Step 8: Docs:** AGENTS.md `session_commands.rs` row (the discard now takes `open` and unpins by sidecar scan; opens refuse a closing project), GAP-214 item 7 → CLOSED (this task), ADR §9 one line ("the session discard holds `open`; R6's pin rule now has one writer discipline").
- [ ] **Step 9: Gates + commit** `fix(editor): serialize the session discard with opens and unpin by sidecar scan`.

### Task 3: Quiesce before cancelling, and keep a refused chunk from failing a take (GAP-214 items 5, 6)

**Model:** Opus. **Files:** Modify `src-tauri/src/editor/discard.rs` (~:118-146), `src-tauri/src/editor/media_derive.rs` (~:602-608, the overrun log), `src-tauri/src/editor/webcam_commands.rs` (~:308-322). Test: `discard_tests.rs`, `webcam_commands_tests.rs`.

- [ ] **Step 1: Failing test (item 5):** a session with a running fake render job and a take mid-finish (entry lock held by the test) → `quiesce` returns the take refusal ("A webcam take is still being saved. Wait for it to finish, then discard the project.") AND the render job is still `running` (not cancelled).
- [ ] **Step 2: Failing test (item 5, overrun):** a derivation that does not stop within its bound → the discard is REFUSED (`invalidRequest`, "Media is still being prepared. Try discarding again in a moment.") and the project files are untouched.
- [ ] **Step 3: Failing test (item 6):** with the session closing, `append` of the next chunk is refused; after `unmark`, the SAME `seq` is accepted and the take finishes.
- [ ] **Step 4: Run** → all FAIL for the stated reasons.
- [ ] **Step 5: Implement:** in `quiesce`, run the non-cancelling checks first (take `open_takes`/`wait_idle`, relink in flight, import in flight) and refuse before any cancel; only then `stop_session_derivations` + render/publish/import cancels; turn `stop_session_derivations`' overrun into `Err` propagated as the refusal above. In `webcam_commands.rs`, hoist `refuse_if_closing` out of the `and_then` closure so its `Err` returns before `entry.failed` is set (still under the entry lock).
- [ ] **Step 6: Run** → PASS; mutation-check (restore the old order; move the check back into the closure) → red.
- [ ] **Step 7: Docs:** GAP-214 items 5 and 6 → CLOSED; AGENTS.md `session_commands.rs` row's quiesce sentence ("work cancelled before the refusal stays cancelled") → "a refused discard cancels nothing: every wait runs before any cancel".
- [ ] **Step 8: Gates + commit** `fix(editor): refuse a discard before cancelling anything and keep closing refusals from failing a take`.

### Task 4: Prove ownership before unpinning, and stop dangling pins blocking a capture (GAP-214 item 8, D-2)

**Model:** Opus. **Files:** Modify `src-tauri/src/editor/store_io.rs` (extract `prove_ownership`), `project_discard.rs` (~:105-106), `src-tauri/src/staged_commands.rs` (`discard_conflict` ~:147-154, `discard_unpinned` ~:333-339), `src-tauri/src/staging_commands.rs` (~:67). Test: `project_discard_tests.rs`, `staged_commands_tests.rs`, `staging_commands.rs` tests.

**Interfaces:** Produces `pub(crate) fn prove_ownership(root: &Path, project_id: &str) -> Result<(), EditorError>` (the check `remove_project` does today, `store_io.rs:~446-471`; `remove_project` calls it). Produces `pub(crate) fn pin_is_live(root: &Path, project_id: &str) -> bool` (`project_dir(root, id).is_some_and(|d| d.join(PROJECT_FILE).is_file())`).

- [ ] **Step 1: Failing test (item 8):** a folder whose `project.json` names ANOTHER id + a sidecar pinned to the folder's id → `discard_project_in` refused; the pin is untouched; `err.message` contains neither `<path:#` nor `expected` (serde's text) — assert it equals the fixed copy "This project could not be discarded because its files do not belong to it."
- [ ] **Step 2: Failing test (D-2):** a capture pinned to a project id whose directory does not exist → `discard_staged_capture`'s core succeeds (unpins under `open`, deletes the files); `clear_staged` removes it and does NOT count it in `skippedPinned`.
- [ ] **Step 3: Run** → FAIL.
- [ ] **Step 4: Implement** `prove_ownership`; call it first in `discard_project_in`, mapping its `InvalidProject` to the fixed copy; in `discard_unpinned`/`clear_staged`, treat a pin as live only when `pin_is_live`, otherwise `unpin_staged` it (already under `open`) and proceed.
- [ ] **Step 5: Run** → PASS; mutation-check.
- [ ] **Step 6: Docs:** GAP-214 item 8 → CLOSED; AGENTS.md `staged_commands.rs` row ("a pin to a project that no longer exists is cleared, not honoured").
- [ ] **Step 7: Gates + commit** `fix(editor): prove ownership before unpinning and clear pins to missing projects`.

### Task 5: Package import hardening (M-V2, S-2, S-3, S-11, S-12, S-13)

**Model:** Opus (security). **Files:** Modify `src-tauri/src/editor/package_import.rs` (~:117-120, :373-381, :418-423, :451-455), `store_io.rs` (`remove_dir_no_follow`/`remove_tree` ~:487-489; `remove_project`'s ownership read bound ~:447), `core/src/editor/package.rs` (~:232-265, :349-363, :474-479), `core/src/editor/package_archive.rs` (~:136-149), `core/src/editor/package_extract.rs` (~:128), `src-tauri/src/editor/package_commands.rs` (~:190-199, :323-335). Tests: `package_import_tests.rs`, `package_commands_tests.rs`, `store_io_tests.rs`, core `package` tests.

- [ ] **Step 1: Failing tests:**
  - (M-V2) a lightweight `.vbproject.json` whose compact bytes are < 8 MiB but whose pretty envelope (a large `extra` map on the project) is > 8 MiB → `import_file` refuses `invalidProject` "This project file is too large to install." and `editor-projects\` holds no new directory (no `.importing` left either).
  - (store_io) `remove_project` succeeds on a directory whose `project.json` is 9 MiB but whose `/project/id` matches (ownership read uses `LEDGER_MAX_BYTES`-style wider bound, `store_io::OWNERSHIP_READ_MAX = 64 * 1024 * 1024`).
  - (S-2) `remove_dir_no_follow` on a junction/symlink AT the top refuses and removes nothing behind it (use the symlink helper the existing no-follow tests use; `#[cfg(windows)]` junction variant if that helper exists, else the portable symlink variant).
  - (S-13) a package whose manifest carries a media entry for a `card` builtin asset → refused `invalidProject` ("The project file lists media no clip can use.").
  - (S-11) an entry name containing U+202E → refused, and the refusal message contains no part of the entry name (assert the whole message string).
  - (S-12) a portable export whose source grows between the size check and the copy → refused (use the export's injectable reader/`PathChooser` seam; if no size seam exists, feed a source the test appends to from the hashing callback).
  - (S-3) the extractor is built from the archive `inspect_archive` validated (unit: `inspect_archive` returns the `ZipArchive` and `PackageExtractor::from_validated` takes it; a test swaps the file after inspection and asserts extraction reads the validated handle — i.e. the extracted bytes are the ORIGINAL ones).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement:** compute the final stripped envelope and serialise it once with `to_string_pretty` BEFORE `ImportDir::create`; refuse over `MAX_PROJECT_JSON_BYTES`; write that exact string. Move the top-level `is_symlink()` refusal into `remove_tree`. Ownership read via a new bounded JSON-pointer read at `OWNERSHIP_READ_MAX`. Refusal messages name entries by position/kind ("entry 3", "a media file") never by name; refuse bidi controls (U+202A–202E, U+2066–2069) in `validate_entry_name`. Re-sum copied bytes against the 200 MiB limit. Require manifest media ids ⊆ `package_plan::assets_needing_media`. Keep one parse of the archive.
- [ ] **Step 4: Run** → PASS; mutation-check each.
- [ ] **Step 5: Docs:** GAP-214 item 1 amended ("an oversized `project.json` is still discardable: the ownership read allows 64 MiB"); AGENTS.md `package_commands.rs`/`package_import.rs` row (the size check before install, bidi refusal, manifest ⊆ needed media); new Gaps entries only for anything not fixed.
- [ ] **Step 6: Gates + commit** `fix(editor): refuse oversized, misleading or drifting project files before anything is installed`.

### Task 6: Re-probe media a project file carries (GAP-215)

**Model:** Opus. **Files:** Modify `src-tauri/src/editor/package_import.rs` (~:329-334, :357), `media_probe.rs`. Test: `package_import_tests.rs`.

**Interfaces:** Produces `pub(super) trait Prober { fn probe(&self, path: &Path, kind: MediaKind) -> Option<ProbeFacts>; }` with a production impl over `ffmpeg::resolve` + `media_probe::probe_media` and a test fake. `import_file` takes `&dyn Prober` (the `ImportIo`/`TakeIo` pattern).

- [ ] **Step 1: Failing test:** a portable package whose carried facts say `hasAudio: true` for an extracted video; the fake prober says silent → the installed `sources.json` record has `has_audio: false` (and `width`/`height`/`mediaKind` from the probe). Second test: the prober returns `None` (no ffmpeg) → carried facts kept.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** (override facts for EXTRACTED media only). **Step 4: Run** → PASS; mutation-check.
- [ ] **Step 5: Docs:** GAP-215 → CLOSED (and its R1-identity addendum: a lying package can no longer choose the fast path for extracted media); GAP-182 (a) → CLOSED; AGENTS.md package row ("carried facts are re-probed for media the file carries; a placeholder keeps its carried facts").
- [ ] **Step 7: Gates + commit** `fix(editor): re-probe the media a project file carries instead of trusting its facts`.

### Task 7: Startup sweep of the project store's crash leftovers (GAP-174 item 2, GAP-189, GAP-191, D-4)

**Model:** Opus. **Files:** Create `src-tauri/src/editor/store_sweep.rs` (+ `store_sweep_tests.rs`); modify `recovery.rs` (call it from the `editor-recovery-sweep` thread beside `sweep_stale_imports`, under the `open` lock it already holds — startup, before any session can exist), `store_io.rs` (`create_project` builds in `.<id>.creating` and renames last, the `ImportDir` pattern).

**Interfaces:** Produces `pub(super) fn sweep_project_leftovers(root: &Path, now: SystemTime) -> SweepReport { removed: usize, kept: usize }`.

- [ ] **Step 1: Failing tests** (one per rule, each with its KEEP twin):
  - stale (> 1 h) plain `media\.<valid id>.<ext>.part` removed; fresh one kept; a symlink with that name kept.
  - stale `jobs\<valid id>\` removed via `remove_dir_no_follow`; fresh kept; symlinked kept.
  - stale plain `products\<valid id>.mp4` NOT in the ledger removed; a recorded one kept; with an unreadable `products.json` NOTHING in `products\` removed.
  - `cache\review-<valid id>.mp4` removed; `cache\review-notanid.mp4` and a symlink kept.
  - `.<valid id>.creating` older than 1 h removed; a crash between `create_dir` and `project.json` (simulate: create `.<id>.creating\sources.json` only) leaves nothing `list_projects` shows and the sweep clears it.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement.** **Step 4: Run** → PASS; mutation-check.
- [ ] **Step 5: Docs:** GAP-174 item 2, GAP-189, GAP-191 → CLOSED (GAP-191: a review is removed at the next start); D-4 recorded as fixed in the store row; AGENTS.md "Where state lives on disk" (the startup sweep's rules).
- [ ] **Step 6: Gates + commit** `fix(editor): sweep the project store's own crash leftovers at startup`.

### Task 8: Promote a capture's companion parts beside the name the capture actually landed on (GAP-198 item 4, GAP-201 item 2 first half)

**Model:** Opus. **Files:** Modify `src-tauri/src/screen_recovery/mod.rs` (743/800 — split first: move the companion-promotion helpers `promote_or_delete_companion_part`, `list_recovered_stem` into `screen_recovery/companions.rs`), `screen_recovery/decide.rs` if the entry order changes. Test: the existing tests beside `mod.rs:~528`.

- [ ] **Step 1: Split without behaviour change; commit.**
- [ ] **Step 2: Failing test:** `<base>.mp4` already taken + stale main part + stale webcam part + stale stem part → `<base> (2).mp4`, `<base> (2).webcam.mp4`, `<base> (2).stem-1.m4a`, and the stem listed in `<base> (2).json`.
- [ ] **Step 3: Run** → FAIL. **Step 4: Implement:** process `Entry::Part` first, record `original base → landed base`, give the companion promoter the landed base. **Step 5: Run** → PASS; mutation-check.
- [ ] **Step 6: Docs:** GAP-198 item 4 and GAP-201 item 2 (first half) → CLOSED.
- [ ] **Step 7: Gates (incl. `npx tauri build --no-bundle`) + commit** `fix(screen): promote a recovered capture's companions beside the name it landed on`.

### Task 9: Recover a webcam take a crash interrupted [GATED D4] (GAP-197)

**Model:** Opus. **Files:** Modify `src-tauri/src/editor/webcam_commands.rs` (699 — split first: move the finish/land/register half into `webcam_finish.rs`), `webcam_registry.rs` (`TakeState::Recovered { bytes }` in `core/src/editor/take.rs`), `session_commands.rs`/`save_commands.rs` open paths (call the recovery on a NEW session only). Test: `webcam_commands_tests.rs` (fake `TakeIo`).

- [ ] **Step 1: Split; commit.**
- [ ] **Step 2: Failing tests:** a stale (> 60 s) plain `takes\.<valid take id>.webm.part` on a new session's open → one asset "Webcam take N (recovered)", part gone, `.remux.webm` leftover removed; a fresh part untouched; a symlink untouched; without ffmpeg the raw bytes land (A09) with length 0.
- [ ] **Step 3: Run** → FAIL. **Step 4: Implement** through the existing `land` + `register` (`EditorOpenResult` NOT widened — ruling l.87). **Step 5: Run** → PASS; mutation-check.
- [ ] **Step 6: Docs:** GAP-197 → CLOSED; AGENTS.md webcam row; checklist row T69 "a take interrupted by killing the app is in the library as '(recovered)' after reopening, playable".
- [ ] **Step 7: Gates + commit** `feat(editor): recover a webcam take a crash interrupted`.
- **If D4 declined:** docs-only — GAP-197 keeps its entry; nothing else.

### Task 10: Set an unreadable recovery journal aside (GAP-180) and record the unreadable ledger (D-6)

**Model:** Sonnet. **Files:** Modify `src-tauri/src/editor/save_commands.rs` (~:392), `recovery.rs` (`load_journal` caller), `src/components/editor/dialogs/RecoveryDialog.vue` (~:59). Test: `recovery_tests.rs`, `tests/editorRecovery*.test.ts` (the suite that covers `RecoveryDialog`).

- [ ] **Step 1: Failing test:** a present but unparseable `recovery.json` + `openProject(id, false)` → the file is renamed (`rename_noreplace`) to `recovery.unreadable-<secs>.json`, byte-identical; `recovery.json` is gone; the session opens clean. A READABLE journal with `useRecovery: false` is left untouched.
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement**; the dialog's unreadable-journal copy says "The unsaved changes could not be read. They were kept in a separate file in the project folder." **Step 4: Run** → PASS; mutation-check.
- [ ] **Step 5: Docs:** GAP-180 → CLOSED. Add **GAP-217** "An unreadable `products.json` blocks every save, render, export and publish of its project" (D-6; the recorded remedy: rename it aside behind a confirm; products files stay) — recorded, not fixed.
- [ ] **Step 6: Gates + commit** `fix(editor): keep an unreadable recovery journal instead of overwriting it`.

---

## Batch 2 — Content-free logs and messages

### Task 11: Redact the log lines and messages the scan cannot see (M-V3, S-5, S-7, S-14, S-15 Rust half, GAP-210 residual)

**Model:** Opus. **Files:**
- Create: `src-tauri/core/src/editor/redact.rs` (move `redact_path`/`redact_name`/`redact_paths_in` here; `src-tauri/src/editor/redact.rs` re-exports them so no editor call site changes).
- Modify: `recovery.rs` (~:444-461 `publish_report`), `render_jobs.rs` (~:566-580: add the filter-escaped spelling `plain.replace('\\', "\\\\").replace(':', "\\:")` of each known path, or pass the ASS docs by bare name with `current_dir(job_dir)` — pick the bare-name route if `screen::render::run` already sets `current_dir`), `core/src/capture_paths.rs` (~:407-413), `core/src/screen_capture_paths.rs` (~:205-213 message rewritten for Publish's temp inside the vault; ~:295-297 no `{base:?}`), `src-tauri/screen/src/ffmpeg_run.rs` (~:299-305 log `dest.file_name()`), `editor_commands.rs` (~:183), `staged_commands.rs` (~:216, 304, 319, 385, 389), `staging_commands.rs` (~:80), `screen_recovery/mod.rs` (~:302-400), `project_store.rs` (~:233), `session_close.rs` (the "Could not unlink the capture {base:?}" message), `redact_guard.rs` (widen the walk).
- Test: `redact_guard.rs` (the widened scan must be RED before the fixes), `recovery_tests.rs`, `render_jobs_tests.rs`.

- [ ] **Step 1: Widen `redact_guard`** to also scan `src-tauri/src/editor_commands.rs`, `staged_commands.rs`, `staging_commands.rs`, `screen_recovery/**`, `src-tauri/screen/src/ffmpeg_run.rs`, `core/src/capture_paths.rs`, `core/src/screen_capture_paths.rs`, and add `video`, `note`, `report`, `dest` to its argument words. Run `cargo test -p vault-buddy --lib redact_guard` → FAIL naming every site above (paste the failure list into the commit body).
- [ ] **Step 2: Failing tests:** `publish_report(journal)` output contains no `/` outside a `<name:#…>` handle; `render_error` redacts `C\:\\Users\\x\\…\\jobs\\<id>\\cues.ass` (the escaped spelling) given that path as known.
- [ ] **Step 3: Implement** the redactions (every message keeps role wording: "Could not unlink the capture." — no name).
- [ ] **Step 4: Run** → PASS; mutation-check two sites (restore one raw `{base:?}` → guard red).
- [ ] **Step 5: Docs:** GAP-210: residual `ffmpeg_run.rs` → CLOSED, the scan's new scope listed, items S-5/S-7/S-14/S-15/M-V3 recorded as fixed; AGENTS.md `redact.rs` row (core home, widened scan).
- [ ] **Step 6: Gates (incl. `npx tauri build --no-bundle`) + commit** `fix(editor): keep names and paths out of the log lines the redaction scan could not see`.

### Task 12: The webview logs codes, not messages; honest write-failure wording; safe render names (S-15 frontend half, S-8, cosmetic)

**Model:** Sonnet. **Files:** Modify `src/roots/EditorRoot.vue` (~:160, :199), `src/composables/useEditorCloseGuard.ts` (~:59, 69, 149), `src/components/editor/dialogs/PublishDialog.vue` (~:91, 166), `src/stores/editorOnboarding.ts` (~:117, 240) — log `${cmd} failed: ${err.code} (${err.operationId})`; `src-tauri/src/editor/subtitle_commands.rs` (~:141), `diagnostics.rs` (~:185) — their own "Could not write the file." instead of `save_commands::map_write_error`'s "Could not save the project"; `render_jobs.rs` (~:363-364) refuse `char::is_control` in a render name ("A video name cannot contain control characters."). Tests: the suites covering each file; `render_jobs_tests.rs`; `subtitle_commands_tests.rs`.

- [ ] **Step 1: Failing tests:** each logging site, given an error whose message is "no staged capture named Secret Window", writes a log line that does not contain "Secret"; a render named "a\u{1}b" is refused with the exact message; a failed subtitle write says "Could not write the file.".
- [ ] **Step 2–4:** run red → implement → green; mutation-check one site.
- [ ] **Step 5: Gates + commit** `fix(editor): log error codes rather than messages from the editor window`.

---

## Batch 3 — Frontend

### Task 13: Keyboard dispatch holes (M-V5, M-V6, F-M2, F-M3, F-M7)

**Model:** Sonnet. **Files:** Modify `src/editor/shortcuts.ts` (~:94-103, :121-128, :141-144), `src/components/editor/shell/EditorShell.vue` (~:236-240), `src/components/editor/timeline/ClipItem.vue` (~:345-347), `src/editor/guide/answers.ts` (shortcut table rows). Tests: `tests/editorShortcuts.test.ts` (or the file that unit-tests `shortcuts.ts`), `tests/editorShortcutsWired.test.ts`.

- [ ] **Step 1: Failing tests:**
  ```ts
  it.each([
    { key: "s", altKey: true },
    { key: "Delete", altKey: true },
    { key: "z", ctrlKey: true, altKey: true }, // AltGr+Z on a layout that maps nothing there
    { key: "s", metaKey: true },
  ])("does not match %o", (init) => {
    expect(matchShortcut(new KeyboardEvent("keydown", init))).toBeNull();
  });
  it("leaves keys typed into a <select> alone", () => {
    const select = document.createElement("select");
    document.body.append(select);
    expect(shouldHandle(new KeyboardEvent("keydown", { key: "Delete" }), select)).toBe(false);
  });
  ```
  (match `shouldHandle`'s real signature). Plus: F1 inside a text input starts the guide (`onboarding.start` called); Escape that cancels a clip drag does not pause the guide (`defaultPrevented` true); the learning center's shortcut table lists "Space — Play or pause" and "← / → — Nudge the selected clip (Shift: 1 s)".
- [ ] **Step 2: Run** → FAIL. **Step 3: Implement** (bail on `altKey || metaKey`; add `SELECT` to the exclusion; `preventDefault()` in `ClipItem`'s Escape branch; F1 skips the text-field rule like F6; two table rows — `SHORTCUT_TABLE` stays a projection: add the rows to the source list the table reads, and keep `editorShortcutsWired`'s walk pressing every row, including the two new ones).
- [ ] **Step 4: Run** → PASS; mutation-check.
- [ ] **Step 5: Docs:** checklist row T70 "on a German layout, AltGr+Z/C/X/V in the editor edits nothing; F5 behaviour per Task 15".
- [ ] **Step 6: Gates + commit** `fix(editor): keep Alt chords and form controls from firing editing shortcuts`.

### Task 14: Light-theme legibility (I-2, F-M4)

**Model:** Sonnet. **Files:** Modify `src/components/editor/dialogs/SaveProjectDialog.vue` (~:142), `src/style.css` (add an editor `--color-hover` token in both themes, e.g. dark `rgb(255 255 255 / 0.1)`, light `rgb(15 23 42 / 0.06)`), the editor components using `hover:bg-white/10` (30; list with `grep -rl "hover:bg-white/10" src/components/editor`) → `hover:bg-hover`, `SaveProjectMenu.vue` (~:75) → `border-line bg-raised`, `tests/e2e/editorKeyboard.spec.ts` (open the Save dialog's portable option in the contrast sweep, both themes).

- [ ] **Step 1: Failing e2e:** the sweep opens Save → "Save a portable copy…" and measures the warning in light → FAIL at 1.05:1.
- [ ] **Step 2: Implement** `border-gold/30 bg-gold-bg text-gold`; the hover token. **Step 3:** `npm run build && npm run test:e2e` → PASS both themes; mutation-check (restore amber-100 → red).
- [ ] **Step 4: Docs:** AGENTS.md design-token paragraph (the editor's `--color-hover`; white-glass stays literal in the panel window only). GAP-206 note: "the portable warning fixed 2026-09-25".
- [ ] **Step 5: Gates + commit** `fix(editor): make the portable-file warning and hover states legible in the light theme`.

### Task 15: A reload must not orphan the open project [GATED D1] (I-3, GAP-208)

**Model:** Opus. **Files (option D1-a, recommended):** Modify `src/roots/EditorRoot.vue` (reattach), `src-tauri/src/editor_commands.rs` (`take_editor_request` keeps the LAST request as a non-consuming "current" read — new sibling behaviour inside the existing command: a second drain after a reload returns the current request once per mount; no new command), `src/editor/shortcuts.ts` or `EditorShell.vue` (`preventDefault` on F5, Ctrl+R, Ctrl+Shift+R, Ctrl+F, Ctrl+P while the editor window has focus), `src/components/editor/dialogs/RenderDialog.vue` + `src/stores/editorJobs.ts` (after reattach, `reconcile()` lists the running render; the dialog shows its progress by polling `editor_get_jobs` at 1 s until a terminal — the Channel is gone after a reload).
- [ ] **Step 1: Failing tests:** (Rust) after `take_editor_request` drained a project request, a `peek_current` (private, called by the same command when the stash is empty and `current` is set) returns it; `editor_close_session` clears `current`. (Vitest) `EditorRoot` remounted with an empty stash and a `current` project → opens it (reuses the live session) instead of "No capture open"; F5 keydown in the editor has `defaultPrevented`; a render running at remount shows its progress from `editor_get_jobs`.
- [ ] **Step 2–4:** red → implement → green; mutation-check.
- [ ] **Step 5: Docs:** GAP-208 → CLOSED (or amended to what remains); AGENTS.md `take_editor_request` row; checklist row T71 "F5 and Ctrl+R in the editor do nothing; a reload (devtools build) reopens the project and a running render's progress".
- [ ] **Step 6: Gates + commit** `fix(editor): keep a reloaded editor on its project and its running render`.
- **Option D1-b (native accelerators off):** instead of the `preventDefault` step, `editor_window.with_webview(...)` → `ICoreWebView2Settings3::SetAreBrowserAcceleratorKeysEnabled(false)` under `cfg(windows)` in release — requires adding `webview2-com` as a direct dependency (already in `Cargo.lock` via wry). **If D1 declined:** Task 1's GAP-208 correction stands; nothing else.

### Task 16: Editor store and job-row correctness (F-M1, F-M6, C-4)

**Model:** Sonnet. **Files:** Modify `src/roots/EditorRoot.vue` (~:150-162: compare `editorProject.generation` before/after the open; unchanged → clear `openError`, skip the log), `src/stores/editorJobs.ts` (~:174-178 guard `install` with the current session; ~:249-256 skip a registry row whose `fraction` is below a held row's with `sequence > 0`; new `forgetSession(sessionId)` called from `editorProject.close`/`beginOpen`). Tests: `tests/editorRoot.test.ts`, `tests/editorJobsStore.test.ts`.

- [ ] **Step 1: Failing tests:** a refused edit then a re-drain of the same capture → `openError` stays null and no "editor_open_staged failed" log; `track()` whose `{jobId}` arrives after the session closed installs nothing; `reconcile` never shows `preparing 0.0` over a held `rendering 0.4`.
- [ ] **Step 2–4:** red → implement → green; mutation-check.
- [ ] **Step 5: Gates + commit** `fix(editor): keep stale outcomes and replies out of the editor's stores`.

### Task 17: Menus, mixer and learning-center tabs (F-M5, F-M8, GAP-207 item 5)

**Model:** Sonnet. **Files:** `src/components/editor/timeline/TimelineView.vue` (~:213-224) + `src/components/editor/ContextMenu.vue` (clamp after `nextTick`, the `TaskScheduleMenu` precedent), `src/components/editor/shell/MixerPopover.vue` (~:76-104: outside `pointerdown` closes; window-level Escape while open, `TrackHeader.vue:180` pattern), `src/components/editor/guide/LearningCenter.vue` (~:122-131: ids, `aria-controls`, `role="tabpanel"` + `aria-labelledby`, ArrowLeft/Right/Home/End via `useRovingTablist`). Tests: the three components' suites.

- [ ] **Step 1: Failing tests:** a context menu opened at (viewport width − 10, height − 10) is fully inside the viewport; Escape with focus on the timeline closes an open mixer; a pointerdown outside closes it; ArrowRight on the learning-center tablist activates the next tab and the panel is labelled by it.
- [ ] **Step 2–4:** red → implement → green; mutation-check.
- [ ] **Step 5: Docs:** GAP-207 item 5 → CLOSED.
- [ ] **Step 6: Gates + commit** `fix(editor): clamp the clip menu, close the mixer from outside and make the learning center's tabs a real tablist`.

---

## Batch 4 — Lifecycle minors

### Task 18: Keep-close edit race, off-main cancel, locked rollback (C-2, C-3, D-5)

**Model:** Opus. **Files:** `src-tauri/src/editor/session_close.rs` (`close_locked(Keep)`: move the `EditorSession` out of `sessions` and take its journal snapshot in ONE `sessions` critical section, write the journal from the moved value), `media_commands.rs` (756 — `editor_cancel_job` becomes `async` on the blocking pool OR drops its `require_session` pre-check; pick the pre-check drop, it removes the lock without growing the file), `media_import.rs` (~:472-497: skip the `sources.json` revert when the session is gone; an orphan record is GAP-174's invisible kind). Tests: `session_commands_tests.rs`/`session_close_tests.rs`, `media_import_tests.rs`, a `media_commands` test that `editor_cancel_job`'s core takes no `sessions` lock (hold `sessions` on another thread; cancel returns within 100 ms via a channel handshake).

- [ ] **Step 1: Failing tests** for each; **Step 2–4:** red → implement → green; mutation-check.
- [ ] **Step 5: Docs:** AGENTS.md `media_commands.rs` row (`editor_cancel_job` reads only the `jobs` leaf).
- [ ] **Step 6: Gates + commit** `fix(editor): never lose an edit acknowledged during Keep, and keep cancel off the session lock`.

---

## Batch 5 — Tests that pin what they claim

### Task 19: Shutdown-gate latches and the webcam capture refusal (I-4, I-5)

**Model:** Opus. **Files:** `src-tauri/src/editor/render_jobs.rs` (757 — move `blocks_shutdown`/`cancel_all_bounded` and the latch into `render_shutdown.rs` first), `publish.rs` (same shape in place, 587), `webcam_commands.rs` (structural pin), tests `render_jobs_tests.rs`, `publish_tests.rs`, `webcam_commands_tests.rs`, `shutdown_gate.rs` (pins read the new module).

**Interfaces:** `pub(crate) fn blocks_shutdown_in(state: &EditorState, latch: &AtomicBool) -> bool`; `pub(crate) fn cancel_all_bounded_in(state: &EditorState, latch: &AtomicBool, limit: Duration, poll: Duration)`; the `AppHandle` wrappers become one-liners passing `&RENDERS_ABANDONED` / `&PUBLISHES_ABANDONED`.

- [ ] **Step 1: Split render's shutdown half; commit.**
- [ ] **Step 2: Failing tests** (each on a fresh `AtomicBool`): a never-ending fake render → `cancel_all_bounded_in` with a 50 ms limit sets the latch and `blocks_shutdown_in` then returns false; a render that ends inside the limit leaves the latch clear; a set latch silences a running render; a publish never flips the render latch (and vice versa). A structural pin: the wrappers' bodies pass the module statics. For I-5: `fn_body(src, "pub async fn editor_webcam_begin(")` contains `state::<CaptureGuard>().active()` before `begin_in(` — write it first against a copy of the body with the line replaced by `let capture = None;` to see it fail.
- [ ] **Step 3–4:** red (the seams do not exist) → implement → green; mutation-check the three mutations the review lists.
- [ ] **Step 5: Gates + commit** `test(editor): pin the shutdown gate's abandon latches and the webcam capture refusal`.

### Task 20: Wire-enum drift pins, declaration-exact evidence, one clip minimum (M-V9, M-V10, GAP-216 `MIN_CLIP_MS`)

**Model:** Sonnet. **Files:** Create `tests/helpers/rustSource.ts` (extract `rustVariants` from `tests/editorCheckWireSpellings.test.ts` and `rustLimit` from `tests/editorCaptions.test.ts:~256`); create `tests/editorWireEnums.test.ts`; modify `src/editor/decode.ts` (export `ERROR_CODES`, `JOB_PHASES`, a new `JOB_KINDS` replacing the inline list ~:349), `tests/editorEvidence.test.ts` (~:57), `docs/superpowers/specs/2026-09-21-tutorial-editor-acceptance-evidence.md` (seven rows' full names: F-04, F-05, F-14, F-21, F-25, F-28, F-45), `tests/useTimelineDrag.test.ts` (`MIN_CLIP_MS === rustLimit("MIN_CLIP_MS")`).

- [ ] **Step 1: Failing tests:** `rustVariants("EditorErrorCode", "src-tauri/core/src/editor/error.rs")` (camelCase per serde) equals `ERROR_CODES` as a set; same for `JobKind`/`JobPhase` in `src-tauri/src/editor/media_jobs.rs`. The evidence guard matches `fn <name>(` for `.rs` and `(it|test)(\.each\(.*?\))?\(\s*["'\`]<name>` for `.ts`, rejects `it.skip`/`#[ignore]` — red on the seven prefix/describe rows until their names are completed.
- [ ] **Step 2–4:** red → implement → green; mutation-check (add a fake Rust variant in a temp copy → red).
- [ ] **Step 5: Gates + commit** `test(editor): pin the job and error wire enums and require exact evidence declarations`.

### Task 21: Rust test hygiene (M-V11, T-6, T-7, T-8, T-9)

**Model:** Sonnet. **Files:** Create `src-tauri/src/editor/test_announce.rs` (`#[cfg(test)] pub(crate) fn announce_skip(what: &str)` → `writeln!(std::io::stderr(), "SKIP: {what}")`, the screen crate's `render_support::announce` shape); modify `media_derive_tests.rs` (1-3, 88-93, 512-552), `webcam_commands_tests.rs` (4, 660, 686), `store_io_tests.rs` (200, 245, 453), `save_commands_tests.rs` (643, 862), `recovery_tests.rs` (635, 664), `discard_tests.rs` (376), `src-tauri/src/cfg_windows_guard.rs` (~:50), `core/src/editor/checks_tests.rs` (~:735).

- [ ] **Step 1:** replace each skip `eprintln!` with `announce_skip`; correct the two module docs ("SKIP VISIBLY" now true).
- [ ] **Step 2:** replace the 700 ms sleep with a marker file `slow_tool` writes once spawned, asserted before cancel; replace each `sleep(200)` negative with an `Arc::strong_count`/channel handshake (`recovery_tests.rs:~596` shape) that proves the waiter is blocked on the lock.
- [ ] **Step 3:** `cfg_windows_guard::imported` records only the final imported item/module of a `use` line — failing test first: `use crate::commands::primary_button_down;` must NOT make a bare `commands::x` reachable.
- [ ] **Step 4:** `no_quality_score_is_ever_emitted` also fails on keys containing `rating`, `grade`, `percent` — failing test first on a crafted JSON.
- [ ] **Step 5: Gates** (run `cargo test -p vault-buddy --lib` twice) **+ commit** `test(editor): make skips visible and replace sleeps with handshakes`.

---

## Batch 6 — Consolidation and small fixes

### Task 22: One home per shell helper (GAP-216)

**Model:** Sonnet. **Files:** Create `src-tauri/src/editor/errors.rs` (`err`, `internal`, `invalid`, `write_error`); create `core/src/editor/io_errors.rs` (`pub fn is_disk_full(e: &io::Error) -> bool` = `StorageFull` or raw OS 112); delete the 15 `err`, 8 `internal`, 3 private `local_data` copies (keep `prefs_commands::local_data`), the 5 shell `cfg` disk-full pairs, and fix `core/src/editor/package.rs:~146` (`write_failed` forgets 112).

- [ ] **Step 1: Failing tests:** `is_disk_full` true for `ErrorKind::StorageFull` and `io::Error::from_raw_os_error(112)` (on Windows) and false otherwise; `package::write_failed` maps raw 112 to `DiskFull`.
- [ ] **Step 2–4:** red → implement (mechanical call-site replacement) → green; `cargo clippy` clean; measure quality (cloneGroups must not rise).
- [ ] **Step 5: Docs:** GAP-216 → CLOSED.
- [ ] **Step 6: Gates + commit** `refactor(editor): one home for the shell's error helpers and the disk-full rule`.

### Task 23: Core odds and ends (GAP-213 [GATED D2], D-3, S-4)

**Model:** Sonnet. **Files:** `core/src/editor/note.rs` (~:114, :199), `core/src/capture_note.rs` (~:112), `core/src/transcript.rs` (66/79/91/119), `core/src/document_import.rs` — one `pub const CREATED_BY: &str = "Vault Buddy";` in core used by all four [D2]; `core/src/editor/migrate_stems.rs` (~:40-52: return `None` silently when `input_count == 0 || stems.is_empty()`); `src-tauri/screen/src/staging.rs` (`is_reserved_device_stem` delegates to `core::device_names::is_reserved_device_name` — staging.rs is 793/800: FIRST move its inline `mod tests` (~345 nonblank) to `staging_tests.rs` via `#[path]`, and move `read_sidecar`'s doc (~:387-391) from above `MAX_SIDECAR_BYTES` to above `pub fn read_sidecar`).

- [ ] **Step 1: Split staging.rs tests + move the doc; commit.**
- [ ] **Step 2: Failing tests:** tutorial note contains `created-by: Vault Buddy\n` and a cross-writer pin (all four writers use `CREATED_BY`) [D2]; a no-audio capture's migration logs nothing at warn (use the crate's log-capture test helper if one exists; else assert the function returns `None` without touching the warn path by making the warning a returned `Option<StemSkip>` the caller logs); `is_safe_base("COM0")` false and an agreement test over both lists.
- [ ] **Step 3–4:** red → implement → green; mutation-check.
- [ ] **Step 5: Docs:** GAP-213 → CLOSED [D2]; ADR §9 (h)'s `created-by: vault-buddy` note updated [D2].
- [ ] **Step 6: Gates (incl. `npx tauri build --no-bundle`) + commit** `fix: one created-by value, one device-name list and no false stem warning`.
- **If D2 declined:** skip the `CREATED_BY` part; GAP-213 stays open.

### Task 24: Shell odds and ends (S-9, S-10, GAP-177, dead parameter)

**Model:** Opus (S-10 touches a pinned security boundary). **Files:** `src-tauri/tauri.conf.json` (~:100 `screen-captures/*` → `screen-captures/*.mp4` + `screen-captures/*.m4a`) + `tray.rs`'s scope pin; `webcam_commands.rs` (or `webcam_finish.rs`) (~:330-335: open the `.part` with `custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)` under `cfg(windows)` and refuse if the opened handle is a reparse point); `src-tauri/src/external_tool.rs` (~:55-56: pure `expand_env_tokens`); `session_commands.rs` (~:365-386: drop `ensure_sources_name`'s dead `project_id` parameter and `let _`).

- [ ] **Step 1: Failing tests:** the scope pin asserts the new list (and that `<base>.json` and `.x.mp4.part` do not match the scope patterns — use tauri's `glob` matching as the pin already does, or a pure matcher test); `expand_env_tokens("%VB_TEST_ROOT%\\bin;%NOPE%")` with `VB_TEST_ROOT` set → `"<root>\\bin;%NOPE%"`; (Windows) appending to a `.part` that is a symlink is refused.
- [ ] **Step 2–4:** red → implement → green; mutation-check.
- [ ] **Step 5: Docs:** GAP-177 expansion half → CLOSED (the `ping` cause stays KEEP-HARDWARE); AGENTS.md asset-scope sentences (staging entry now `*.mp4`/`*.m4a`) and ADR §9 one line for the R7 narrowing; checklist row T72 "a staged capture, its webcam file and a stem still play in the editor preview after the scope narrowing".
- [ ] **Step 6: Gates (incl. `npx tauri build --no-bundle`) + commit** `fix: narrow the staging asset scope, open webcam parts no-follow and expand PATH tokens`.

### Task 25: Carry a crossfade through duplicate and paste [GATED D3] (GAP-178)

**Model:** Opus. **Files:** `core/src/editor/commands/groups.rs` (`check_no_overlap`), `commands/payloads.rs` (`ClipboardFragment.transitions`), `src/editorCommandTypes.ts`, `src/editor/clipboard.ts`, `tests/fixtures/editorCommandWire.ts` + core `wire_tests`. Tests: core `groups` tests, `tests/editorClipboard.test.ts`.
- [ ] **Step 1: Failing tests:** duplicating a crossfaded pair → `Ok`, one transition between the new ids; pasting a copied pair → same; a fragment whose transition names a clip outside the fragment → refused.
- [ ] **Step 2–4:** red → implement (additive wire field, `#[serde(default)]`) → green; mutation-check.
- [ ] **Step 5: Docs:** GAP-178 → CLOSED; ADR §9 (additive `pasteFragment.fragment.transitions`).
- [ ] **Step 6: Gates + commit** `feat(editor): duplicate and paste a crossfaded pair with its transition`.
- **If D3 declined:** skip; GAP-178 stays open.

### Task 26: Final bookkeeping

**Model:** Sonnet. **Files:** AGENTS.md, docs/Gaps.md, ADR §9, the evidence file, the checklist, `scripts/quality-baseline.json` (only if a task dropped averageMaintainability — one sentence edited in place).
- [ ] **Step 1:** re-measure every count with its own one-liner (IPC 131, editor grants 36, checklist rows — expect 68 + the rows Tasks 9/13/15/24 appended); fix any drift.
- [ ] **Step 2:** every Gaps entry this plan closed says CLOSED with the date and task; every finding this plan left unfixed (review "Minor" list items not covered by a task) has a Gaps entry after the highest number.
- [ ] **Step 3:** `npx vitest run tests/editorEvidence.test.ts` → PASS; full gates.
- [ ] **Step 4: Commit** `docs: close the hardening pass's gaps and re-measure the counts`.

---

## Findings → tasks

| Finding | Task |
| --- | --- |
| I-1 discard/open race | 2 |
| I-2 portable warning contrast | 14 |
| I-3 / GAP-208 reload | 1 (docs), 15 [D1] |
| I-4 latches | 19 |
| I-5 webcam capture refusal pin | 19 |
| M-V2, S-2, S-3, S-11, S-12, S-13 | 5 |
| M-V3, S-5, S-7, S-14, S-15, GAP-210 residual | 11, 12 |
| M-V5, M-V6, F-M2, F-M3, F-M7 | 13 |
| M-V9, M-V10 | 20 |
| M-V11, T-6..T-9 | 21 |
| M-V13..16, docs minors, GAP-170/174/198/168 text | 1 |
| C-2, C-3, D-5 | 18 |
| C-4, F-M1, F-M6 | 16 |
| D-2 | 4 |
| D-3, S-4 | 23 |
| D-4, GAP-174.2, GAP-189, GAP-191 | 7 |
| D-6 | 10 (recorded as GAP-217) |
| S-8 | 12 |
| S-9, S-10, GAP-177 | 24 |
| F-M4 | 14 |
| F-M5, F-M8, GAP-207(5) | 17 |
| GAP-178 | 25 [D3] |
| GAP-180 | 10 |
| GAP-197 | 9 [D4] |
| GAP-198(4), GAP-201(2a) | 8 |
| GAP-213 | 23 [D2] |
| GAP-214 (5)(6) / (7) / (8) | 3 / 2 / 4 |
| GAP-215, GAP-182(a) | 6 |
| GAP-216 | 20 (`MIN_CLIP_MS`), 22 |

**Left recorded (KEEP), with reason in the triage:** GAP-170 live ACL (T4/T5, hardware), 173, 176, 181, 182(b–d), 183, 184, 185, 186, 187, 188, 192 (ruling l.87), 193 (R10), 194 (hardware, R-H1), 195 (ruling l.88), 196 (ruling l.89), 199/200/201 (hardware), 202, 204, 205, 207 (other items; (1) ruling l.98), 212, 214 (1)(2)(3)(4)(9).

## Decisions (the user's, before execution)

**Taken 2026-09-25:** D1 = (a) suppress + reattach; D2 = yes; D3 = yes; D4 = register as recovered; D5 = keep (recommendation, not objected). Tasks 9, 15, 23 and 25 run in full.

- **D1 — GAP-208 / F5 reload (re-opens ruling l.100's deferral).** (a) *Recommended:* suppress F5 / Ctrl+R / Ctrl+Shift+R / Ctrl+F / Ctrl+P in the editor window from the webview (no dependency) AND implement the reattach so a reload that still happens (devtools builds) reopens the project and a running render's progress (Task 15); (b) the same but switch WebView2's accelerators off natively — adds `webview2-com` as a direct dependency; (c) docs only (Task 1's correction).
- **D2 — GAP-213:** new tutorial notes say `created-by: Vault Buddy` like every other note (existing notes untouched). *Recommended: yes.*
- **D3 — GAP-178:** duplicate/paste a crossfaded pair with its transition (additive wire field). *Recommended: yes, it removes a refusal.*
- **D4 — GAP-197:** on the next open, an interrupted webcam take is registered as "Webcam take N (recovered)" (the screen sweep's promote-never-delete precedent). *Recommended: yes.* Alternative: report-only in the log.
- **D5 — keep as recorded (no work):** GAP-187 + 212 (product removal UI), GAP-188 and 182(c) (products/unplaced media in portable files — ruling l.7's 200 MiB cap), GAP-181, GAP-183, GAP-214(2) "Damaged projects" row. *Recommended: keep all.*
- **Behaviour notes (no decision needed unless you object):** Task 2 refuses an open of a project mid-discard ("This project is being discarded."); Task 3 refuses a discard more often (instead of cancelling work first); Task 10 renames an unreadable journal aside; Task 12 refuses control characters in a video name; Task 24 narrows the staging asset scope to `*.mp4`/`*.m4a`.
