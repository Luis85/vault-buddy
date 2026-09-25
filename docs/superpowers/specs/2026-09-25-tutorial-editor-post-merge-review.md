# Tutorial Editor — post-merge review (2026-09-25)

Range reviewed: `9a8069c..930eadc` (528 files, +125 547 / −10 948), the merged
Tutorial Editor increment on branch `claude/vault-buddy-improvement-polish-95f5bc`
(= PR 79's head). Read-only review; nothing was changed by it.

**Method.** Six independent reviewers on the most capable model, one per
dimension (security boundaries, concurrency and lifecycle, data integrity,
frontend correctness and UX, tests, docs versus code), each starting from
`git diff --stat` and reading selectively against AGENTS.md, the ADR
(`2026-09-21-tutorial-editor-integration-design.md`) and docs/Gaps.md
(GAP-170–216 were treated as known). Then ONE adversarial verification pass
(three verifiers split by domain) re-checked every Important finding against
the code and tried to disprove it. The previous run's 101 rulings
(`tutorial-editor-rulings.md`) were treated as decided; no finding below
re-opens one.

**Verdict.** No Critical finding. Five Important findings survived
verification; eleven reviewer-Important findings were confirmed but
downgraded to Minor, and one turned out to be a catalogued gap (GAP-208)
whose entry is wrong. The authorization partition, the asset scope, the
package parser, the publish vault write, the never-clobber rails, the store
read bounds, the ledger ordering, the Channel job stream and the TS↔Rust
fixtures were each traced and hold (see "Verified as correct").

## Measured counts (each file's own one-liner, never incremented)

| What | Measured |
| --- | --- |
| IPC commands (`generate_handler!`) / `build.rs` `ALL_COMMANDS` | 131 / 131 |
| `editor_*` commands / `capabilities/editor.json` grants / `default.json` grants | 36 / 36 / 95 |
| Editor checklist rows (T1–T68) / with a result | 68 / 0 |
| Screen checklist rows / with a result / unrun | 57 / 23 / 34 |
| `CheckCode` variants / guide lessons / `EditorErrorCode` / `JobKind` | 14 / 22 / 15 / 4 |
| `screen:*` events emitted | 8 |

## Important (verified)

**I-1 · A session discard unpins and removes the project outside `EditorState::open`.**
`src-tauri/src/editor/session_commands.rs:630-706` (`close_in` / `close_locked`)
takes the closing mark and the session save lock but never `open`, while every
other pin writer or project remover holds it (`open_staged_session :479`,
`save_commands.rs:388`, `package_import.rs:405`, `project_discard.rs:92`,
`staged_commands.rs:334`, `staging_commands.rs`, `recovery.rs:565`,
`screen_recovery/mod.rs:433`). `mod.rs:68-73` even documents "close never
takes it", which predates `discardProject` becoming a pin writer. Reachable
with two ordinary clicks: Discard project… in the editor, then **Edit** on the
same capture in the panel (`editorProject.close` clears `sessionId` before
awaiting, so the re-drain is not short-circuited). Outcomes: (a) the open's
orphan scan re-pins the capture to the project being deleted → a pin to
nothing, the capture's Discard refused; (b) the open reuses the live session
the discard then drops → a dead editor; (c) an unlocked sidecar RMW can lose a
sweep's stems block or resurrect a pin. Found independently by the
concurrency and data-integrity reviewers.
*Fix:* take `open` in `close_in` for `DiscardProject` before `mark_closing`
(lock order `open → jobs → closing → save → maps`, verified: no `open` holder
takes a save lock and nothing the quiesce waits for takes `open`); refuse an
open of a project whose live session is closing ("This project is being
discarded."); correct `mod.rs`; two tests (discard waits for a held `open`;
an open of a closing project is refused).

**I-2 · The portable-file privacy warning is unreadable in the light theme.**
`src/components/editor/dialogs/SaveProjectDialog.vue:142`
(`border-amber-400/30 bg-amber-400/10 text-amber-100`) over `bg-panel` (#fff
in light) measures 1.05:1 — the sentence that tells the user their trimmed
and covered originals ship in the file. The only literal palette text class
left in `src/components/editor/**`; the e2e contrast sweep never opens this
dialog. *Fix:* `border-gold/30 bg-gold-bg text-gold` (4.93:1 light, 6.46:1
dark); open the dialog in `tests/e2e/editorKeyboard.spec.ts`'s sweep.

**I-3 · GAP-208's premise is wrong: F5 / Ctrl+R reload the editor in RELEASE builds.**
wry 0.55.1 defaults `browser_accelerator_keys: true`; tauri 2.11.5 never
switches it off and the shell never touches `ICoreWebView2Settings`. F5 /
Ctrl+R reload the editor webview in every build (also Ctrl+F / Ctrl+P open
browser chrome); `EditorRoot.vue:138-171` then drains an empty stash and shows
"No capture open" while Rust keeps the session, its render and its journal.
GAP-208 says "Ctrl+R in a debug build" and rates it Low. *Fix:* correct and
re-rate GAP-208 (Medium); disable the accelerators on the editor webview
(`with_webview` → `SetAreBrowserAcceleratorKeysEnabled(false)`, release only)
or at least `preventDefault` F5/Ctrl+R in the editor — a behaviour decision —
and/or GAP-208's re-attach path.

**I-4 · The shutdown gate's abandon latches and AppHandle wrappers run in no test.**
`render_jobs.rs:778-797` (`blocks_shutdown`, `cancel_all_bounded`,
`RENDERS_ABANDONED`) and `publish.rs:605-624` (`PUBLISHES_ABANDONED`). Tests
call only the inner state functions; `shutdown_gate.rs:242` checks a call
substring. Mutations no test catches: drop the `!` at `render_jobs.rs:779`
(a quit kills ffmpeg mid-write); `store(false)` at `:795`/`publish.rs:622`
(GAP-190's Alt+F4 loop returns); swap the publish term for the render term.
*Fix:* `*_in(state, &AtomicBool)` seams the wrappers delegate to, tested on a
fresh latch (expiry sets it, a set latch silences the term, a clean cancel
leaves it clear, a publish never flips the render latch).

**I-5 · `editor_webcam_begin`'s `CaptureGuard` read is unpinned (F35).**
`webcam_commands.rs:222` is the only wiring; `begin_is_refused_during_a_screen_capture`
injects the capture kind into `begin_in` itself. `let capture = None;`
compiles and passes every test — the native refusal silently becomes UI-only.
*Fix:* a structural pin that `editor_webcam_begin`'s body reads
`state::<CaptureGuard>().active()` and passes it to `begin_in(`.

## Confirmed, downgraded to Minor by verification

- **M-V2 · A crafted compact lightweight file just under 8 MiB installs an unremovable project.**
  `package_import.rs:117-120, 373-381, 455` re-serialises pretty-printed with no
  size check; nothing bounds the flattened `extra` maps, so a file with a large
  `extra` crosses `MAX_PROJECT_JSON_BYTES`, after which save, list and both
  discards (`remove_project`'s ownership read, `store_io.rs:447`) refuse it.
  The app's own export cannot produce one. *Fix:* size-check the final pretty
  envelope before `ImportDir::create`; let `remove_project`'s ownership read
  use a larger bound (softens GAP-214 item 1 too).
- **M-V3 · The interrupted-publish report logs vault-relative names and the product title unredacted on every start.**
  `recovery.rs:444-461, 566-568`; the scan misses it (`video`/`note` names).
  *Fix:* `redact_name` both; record in GAP-210.
- **M-V5 · Alt / Meta are ignored by the shortcut matcher.** `shortcuts.ts:94-103`:
  Alt+S splits, Alt+Delete deletes, Ctrl+Alt+{Z,C,X,V,D,G,S,E} run the Ctrl
  bindings (AltGr only on keys the layout leaves unmapped). *Fix:* bail on
  `altKey || metaKey`; two Vitest cases.
- **M-V6 · A focused `<select>` passes `shouldHandle`.** `shortcuts.ts:125`:
  Delete/S on the rate select, the ratio select and the inspector's selects
  (`FadesSection.vue:146`, `LayoutSection.vue:185/198/229`,
  `CaptionSettingsPanel.vue:107`) deletes/splits the selected clip — the clip
  being edited. *Fix:* add `SELECT`; one test.
- **M-V9 · TS error-code / job kind / job phase lists are unpinned to Rust.**
  `decode.ts:59-85, 349` vs `core/src/editor/error.rs:12-28`,
  `media_jobs.rs:58-118`. An unknown phase/kind fails `editor_get_jobs`; an
  unknown error code downgrades silently to `internal`. *Fix:* the
  `rustVariants` source-reading pin used for the checks enums.
- **M-V10 · `tests/editorEvidence.test.ts:57` accepts a substring.** Seven rows
  (F-04, F-05, F-14, F-21, F-25, F-28 — prefixes; F-45 — a `describe` title)
  rely on it; a comment or `it.skip` would satisfy it. *Fix:* match declarations,
  reject skips, correct the seven names.
- **M-V11 · Shell ffmpeg-dependent tests skip invisibly.** `eprintln!` is captured
  for passing tests (`media_derive_tests.rs:1-3,88-93`, `webcam_commands_tests.rs:4,660,686`,
  `store_io_tests.rs:200,245,453`); the screen crate writes raw stderr and is
  right. *Fix:* a shared raw-stderr `announce`; correct the module docs.
- **M-V13..16 · AGENTS.md drifts:** `:169` says 35 editor commands (36);
  `:1935` says 66 checklist rows (68); `:565` says save writes an always-empty
  `products` (it assembles them from the ledger); `:573` cites the retired
  `export_worker::vault_dir` (now `editor/vault_dir.rs`).

## Minor (reviewer-reported, not adversarially verified)

Concurrency: **C-2** `close(keep)` can drop an edit acknowledged during the
close (`session_commands.rs:524-544, 677, 704`; move the session out and
snapshot in one `sessions` section); **C-3** `editor_cancel_job` is sync (main
thread) but waits on `sessions`, held across `validate_project`
(`media_commands.rs:426-437`; make it async or drop the pre-check); **C-4**
`editorJobs.track()` installs a row for a session the store may have left
(`editorJobs.ts:174-178`).

Data integrity: **D-2** `discard_conflict` refuses on a pin whose project no
longer exists (`staged_commands.rs:147-154, 333-339`); **D-3**
`migrate_stems::stem_parts` logs a false "track or clip limit" warning on every
no-audio capture (`migrate_stems.rs:40-52`); **D-4** a crash inside
`store_io::create_project` leaves an unlistable, unremovable directory
(`store_io.rs:104-133`); **D-5** `media_import::rollback` rewrites
`sources.json` unlocked after a `keep` close (`media_import.rs:472-497`);
**D-6** an unreadable `products.json` blocks every save/render/export/publish
with no in-app remedy (record beside GAP-214).

Security: **S-2** `remove_dir_no_follow` lacks the top-level junction check
`remove_project` has (`store_io.rs:487-489`); **S-3** the zip is parsed twice,
the second parse without the raw pre-checks (`package_extract.rs:128`);
**S-4** two reserved-device lists diverged, the editor's door uses the weaker
(`screen/src/staging.rs:118-145` vs `core/src/device_names.rs`); **S-5/S-15**
raw capture names in logs and messages outside the scan's scope
(`editor_commands.rs:183`, `staged_commands.rs:216/304/319/385/389`,
`staging_commands.rs:80`, `screen_recovery/mod.rs:302-400`,
`project_store.rs:233`, `session_commands.rs:690`; the webview re-logs editor
error messages — `EditorRoot.vue:160/199`, `useEditorCloseGuard.ts`,
`PublishDialog.vue`, `editorOnboarding.ts`); **S-7** core log lines reachable
from Publish print absolute paths, one with stale advice
(`capture_paths.rs:407-413`, `screen_capture_paths.rs:205-213, 295-297`);
**S-8** a control character in a render name breaks the note's frontmatter
(`render_jobs.rs:363-364`, `yaml_scalar.rs:8-14`); **S-9** webcam `.part` is
check-then-open (`webcam_commands.rs:330-335`); **S-10** the asset scope's
`screen-captures/*` also serves sidecars and `.part` files
(`tauri.conf.json:100`); **S-11** package refusals echo untrusted entry names
incl. bidi controls; **S-12** the portable 200 MiB limit is measured before
the copy (`package_commands.rs:190-199`); **S-13** a manifest media entry for
a `card` builtin is extracted but never recorded (`package.rs:474-479`);
**S-14** `redact_paths_in` misses ffmpeg's filter-escaped ASS path
(`render_jobs.rs:573-577`); subtitle/diagnostics write failures say "Could not
save the project". Addenda: GAP-215 — carried width/height/mediaKind also
drive the R1 identity fast path (a lying package yields a product whose ledger
row misstates its size); GAP-210 — add S-5, S-7, S-14, S-15.

Frontend: **F-M1** a re-drain of the capture already showing records a stale
edit error as the open's outcome (`EditorRoot.vue:150-162`); **F-M2** Escape
that cancels a clip drag also pauses the guide (`ClipItem.vue:345-347`);
**F-M3** F1 is dropped inside text fields, unlike F6 (`EditorShell.vue:236-240`);
**F-M4** `hover:bg-white/10` and white-glass toggles are invisible in the
light theme (30 editor components; `SaveProjectMenu.vue:75`); **F-M5** the clip
context menu is not clamped to the viewport (`TimelineView.vue:213-224`);
**F-M6** `reconcile` can transiently regress a running job's phase
(`editorJobs.ts:249-256`); **F-M7** the shortcut table omits Space and the
clip nudge keys; **F-M8** the mixer closes on Escape only with focus inside,
and never on an outside click (`MixerPopover.vue:76-104`).

Tests: **T-6** `a_closing_session_cancels_a_running_decode` depends on a 700 ms
sleep to exercise the kill arm (`media_derive_tests.rs:512-552`); **T-7** five
`sleep(200)` negative assertions (`save_commands_tests.rs:643,862`,
`recovery_tests.rs:635,664`, `discard_tests.rs:376`); **T-8**
`cfg_windows_guard.rs:50` marks every token of a `use` line reachable; **T-9**
`no_quality_score_is_ever_emitted` checks only `score`.

Docs: package row says an empty `products` list (`AGENTS.md:570`); "nine
`screen:*` events" (eight, `:3485`); the webcam sidecar block omits
`durationMs` (`:657`); the editor window's other invokes are listed
incompletely (`:1871-1874`); GAP-170's body still says eight/91/96; GAP-198
and GAP-168 cite retired `export_worker` paths.

## Verified as correct (coverage)

- **Authorization:** `editor.json` grants exactly the 36 `editor_*` to
  `["editor"]`, `default.json` none of them; `ALL_COMMANDS` exhaustive (131);
  `authz_guard` requires `window: WebviewWindow` and a first-statement
  `require_editor_window(&window)?` (the raw-body append included);
  `capability_guard` replicates Tauri's resolution over `gen/schemas`
  (residual GAP-170). The panel-callable openers validate what they pass on.
- **Asset scope and paths:** R7's five entries pinned by `tray.rs`; Tauri
  canonicalizes and requires literal separators; every id becoming a path is
  `is_valid_id`-checked; `join_contained` refuses `:`/`\`/multi-component;
  store reads `read_bounded` (no-follow, bounded while reading).
- **Package:** raw end-record/central-directory walk, entry-name rules
  (device stems, ADS, case-folded dupes), per-entry ratio and total bounds,
  local header re-verified, no overlaps, SHA-256 before the next entry,
  `sources.json` never taken from the file, one rename last.
- **Publish (tenth write):** containment before and after `create_dir_all`,
  structurally pinned; owned temp, `rename_noreplace`, pairwise ` (N)`;
  video first, note second, note names the landed file; job registered before
  any byte.
- **Webcam raw body:** headers before naming the take; canonical `seq`; 1 MiB
  checked on the borrowed body before `to_vec`; session-bound registry.
- **Never-clobber and ledger:** every store JSON through
  `write_atomic_replacing`; first writes via `create_new` + `rename_noreplace`;
  product moved then recorded, an unrecorded file taken back out; save
  `mark_saved` only on `Ok`; `remove_project` proves ownership, `project.json`
  last.
- **Locks and threads:** `open → by_project → sessions`; `jobs` a true leaf;
  take entry → save → sessions; poisoning handled; every spawn in the range
  named with its failure handled; stems/webcam producers `try_send` and never
  block the mixer; bounded quit cancels with abandon latches.
- **Frontend:** generation/session/forward-revision guards on every reply;
  the Channel's four guards and pre-reply replay; `errorCopy` at all three
  entry points; one shell dispatcher; `SHORTCUT_TABLE` is an exact projection
  of `SHORTCUTS` and each listed key does what it says; guide sends zero
  `editor_execute`.
- **Tests:** the time, audibility, command-wire, check-enum, `steps.json` and
  presenter fixtures are read by both languages; screen-crate ffmpeg skips are
  visible; no fixed temp names; fake timers restored; Playwright `retries: 0`.
- **Docs:** every identifier and file AGENTS.md's editor rows cite exists;
  ADR §8/§9 match the code; README, DEVELOPMENT and CONTEXT agree with the UI.
