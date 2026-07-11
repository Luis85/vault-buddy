# Quality & Polish Pass — Design

**Date:** 2026-07-10
**Status:** Approved
**Branch:** `claude/task-management-vertical-slice-ikeuly` (stacked onto PR #46)
**Source of truth:** [docs/Gaps.md](../../Gaps.md) — the audited backlog. This
pass works it down at the agreed depth: **all High and Medium entries, plus
the Low entries a user can see or type into, plus the tech-debt split
obligations**. Invisible Low edges, the untested-paths section as its own
workstream, CHANGELOG/SECURITY.md process docs, and accepted debt (GAP-48)
stay catalogued and out of scope.

## Shape

One umbrella spec (this document), **five themed sub-passes executed in
order**, each with its own implementation plan
(`docs/superpowers/plans/2026-07-10-polish-<letter>-….md`) run
subagent-driven with per-task adversarial review and a final whole-sub-pass
review — the same machinery as the three task increments on this branch.
Fixes land before refactors: sub-pass E's file splits come last so they
never churn code the earlier sub-passes' reviews just examined.

**Bookkeeping rules (apply to every sub-pass):**

- Every fixed Gaps.md entry is marked `FIXED 2026-07-10` (strikethrough
  severity, one-line what-changed, keep the entry as a tombstone — the
  GAP-40 precedent) **in the same commit as its fix**.
- Every behavioral fix is TDD'd: failing test first, named for the failure
  mode. Windows-only behavior that cannot execute on Linux gets its logic
  extracted into a testable pure function where feasible; what remains is
  covered by the Windows checklist and called out in the plan.
- All gates stay green at every task boundary: Vitest, vue-tsc, ESLint,
  LOC guard, fallow ratchet, `cargo fmt/clippy/test` (core, capture,
  transcribe, mcp), and the Linux shell compile gate for shell changes.
- When a change *improves* a ratcheted metric (LOC, fallow), re-run the
  gate with `--update` in the same commit so the baseline ratchets down.

## Sub-pass A — Rust correctness & data safety

Gaps: **GAP-01 (High)**, GAP-02, GAP-03, GAP-04, GAP-05, GAP-06, GAP-07,
GAP-08, GAP-23, GAP-24 (all Medium unless noted).

- **A1 · GAP-01.** `transcription.rs::owning_vault_id` (used by
  `transcribe_recording_now` and `retranscribe`) canonicalizes the incoming
  mp3 path (rejecting it when canonicalization fails) before the
  vault-prefix match, and both commands additionally require an
  `is_capture_base` basename — the same ownership filter `rename_capture`
  enforces. The read-only lexical matcher in `open_recording_note` gets the
  same canonicalization (lower stakes, same helper). `retranscribe` keeps
  bypassing the vault's `transcribe` *setting* (that is its documented
  purpose) but must not bypass containment.
- **A2 · GAP-02.** `capture_config::update_vault_config_at` defaults to
  `AppConfig::default()` **only** on `ErrorKind::NotFound`; any other read
  error is logged and propagated so the save fails loudly instead of
  wiping vaults B..N.
- **A3 · GAP-03.** `transcript.rs`'s `is_regenerable`,
  `needs_transcription`, and `transcript_status` read the ownership marker
  via a frontmatter-scoped check (`note_field(content,
  "vault-buddy-transcript")`) instead of whole-content `contains`.
- **A4 · GAP-04.** `rename_plan`/`execute` move `<old>.transcript.md` on
  the same reservation + `rename_noreplace` + suffix-retry rails as the
  audio, and retarget the note's `.transcript` embed line alongside the
  mp3 embed. A transcript-side failure after a successful audio move
  degrades to a warning (audio first — the existing rename posture).
- **A5 · GAP-05.** `session.rs`'s tick loop clamps `next_tick` to
  `now + TICK` whenever it has fallen more than 5 ticks (500 ms) behind —
  real encode backpressure never accumulates that much; a suspend gap
  always does — and skips the sample take on a wake that arrived before
  schedule (pause→resume spurious silence). The clamp policy lives in a
  pure, unit-tested function; the loop calls it.
- **A6 · GAP-06.** On Windows, `rename_noreplace`'s non-decisive-error
  fallback uses `MoveFileExW` **without** `MOVEFILE_REPLACE_EXISTING`
  (natively non-replacing) instead of the TOCTOU `exists()` + replacing
  rename. Non-Windows keeps the current fallback (compile-gate only).
- **A7 · GAP-07.** `rename_capture` resolves the owning vault
  (canonicalized, per A1's helper) and refuses paths outside every
  registered vault before planning the rename.
- **A8 · GAP-08.** Shutdown paths (`quit`, close-requested,
  `hide_buddy`'s recording check) may bypass a wedged **startup**
  reservation when `active.part.is_none()` — nothing has reached disk, so
  the never-lose-audio invariant is not in play. Once a `.part` exists the
  current wait-forever posture stands. The reservation carries an explicit
  startup-wedged state so the bypass cannot trigger mid-recording.
- **A9 · GAP-23.** The five silent `Ok`-with-empty single-file reads
  (`discovery.rs`, `capture_config.rs`, `daily_notes.rs`,
  `app_diagnostics.rs`, `transcript.rs` ×2) `log::warn!` on any error
  other than NotFound. Return values are unchanged (still degrade).
- **A10 · GAP-24.** Every `.expect` on thread spawn inside main-thread
  native callbacks (`lib.rs` close handler, `tray.rs` ×2,
  `capture_commands.rs` ×5) becomes the log-and-degrade pattern
  `schedule_focus_out_check` already uses.

## Sub-pass B — Main-thread responsiveness

Gaps: **GAP-20 (High), GAP-21 (High)**, GAP-22 (Medium).

- **B1 · GAP-20.** `stop_capture` becomes an async command (its wait is on
  `CaptureState`'s condvar, not window APIs). On the finalize timeout it
  returns a distinct "still saving" result — a typed variant the frontend
  maps to the existing saving UI — never a bare `Ok`.
- **B2 · GAP-21.** `start_capture` becomes async; the 10 s device-ready
  wait moves off the main thread with it. Reservation semantics (names
  reserved under the mutex before the worker spawns) are unchanged.
- **B3 · GAP-22.** `list_recordings`, `list_tasks`, `count_open_tasks`,
  and `list_audio_devices` become async, each wrapping its filesystem/COM
  work in `spawn_blocking` — the `search_vaults` precedent. None touches a
  window API or window-state lock (verified per command in the plan).
- **B4.** The frontend's `invoke` call sites need no changes (`invoke` is
  already promise-based), but the capture store's start/stop error paths
  are re-verified against the new async rejection timing, and AGENTS.md's
  IPC table gains sync/async annotations for the moved commands with the
  one-line reason (the window-thread invariant only pins *window-touching*
  commands to sync).

## Sub-pass C — Frontend defects, UX, accessibility, copy

Gaps: GAP-26, GAP-27, GAP-28, GAP-29, GAP-30, GAP-31 (Medium), the
user-visible slice of GAP-32, GAP-33 (Low), plus the aggregation review's
deferred debt.

- **C1 · GAP-27.** `SelectMenu`'s Escape branch adds `stopPropagation()` so
  dismissing a dropdown no longer closes the whole panel. Regression test:
  Escape in an open menu leaves the panel-close handler uncalled.
- **C2 · GAP-28.** `checkForUpdatesQuietly` re-checks `phase === "idle"`
  after its `await check()` before assigning, so a slow quiet check can
  never stomp a manual check or an in-flight install.
- **C3 · GAP-29.** The `shownNonce` watcher stops unconditionally killing
  the rename prompt: `dismissRename` is skipped when the prompt is fresh
  (younger than the existing 30 s rename window), so a tray-stopped
  recording can still be named on the next panel open. A genuinely stale
  prompt still clears.
- **C4 · GAP-30.** `RecordMode.vue`'s transcription toggle persists only
  after a *successful* config load (`loaded` no longer set in `finally` on
  failure — the `tasksFolderLoaded` gate pattern), so a failed read can't
  rewrite the vault's capture config to defaults.
- **C5 · GAP-31.** The add-task Enter handler and the panel filter's
  Escape ignore `event.isComposing` (the Search precedent) — an IME
  candidate commit can no longer create a half-composed task document.
- **C6 · GAP-32 (selected).** (a) `taskCounts` refresh after task
  mutations — `Tasks.vue` triggers a store refresh on toggle/archive/add
  success and on `back()`, so vault-row badges don't go stale until the
  next panel open. (b) The failed-toggle revert restores the task's
  *original* status (captured before the optimistic flip) and recomputes
  the re-insert index at revert time. (c) `notifications.ts` restarts the
  TTL timer when dedupe reuses a toast.
- **C7 · GAP-33.** `SelectMenu` gets the full listbox keyboard/a11y story:
  option `id`s + `aria-activedescendant`, `scrollIntoView` on highlight
  move, Home/End, and tests covering the keyboard path. `Search.vue` binds
  `aria-expanded` to `visibleHits.length > 0` and adds
  `aria-autocomplete="list"`.
- **C8 · GAP-26.** One shared vault-lookup helper (wraps
  `services::find_vault`) replaces the six duplicated
  `discover_vaults().find(..)` sites; every user-facing error uses the
  user-worded "Vault not found — was it removed from Obsidian?" form, and
  user-facing error strings stop embedding absolute local paths (paths go
  to the log line instead).
- **C9 · Deferred debt.** The one-line cross-vault path-uniqueness comment
  at `Tasks.vue`'s `busy` set; the teleport test's cleanup wrapped
  try/finally; the `vaults.ts` import comma-space.

## Sub-pass D — Security & release engineering

Gaps: GAP-34, GAP-35, GAP-36, GAP-37 (Medium), **GAP-41 (High)**, GAP-42,
GAP-43 (Medium), GAP-44 (one item).

- **D1 · GAP-34.** `tauri.conf.json` sets a restrictive CSP
  (`default-src 'self'; style-src 'self' 'unsafe-inline'` as the starting
  point; Vite/Tauri asset serving must be verified in the Windows build —
  a Linux compile gate cannot prove the runtime policy, so the plan calls
  out a manual verification step).
- **D2 · GAP-35.** All third-party actions in all three workflows are
  pinned to full commit SHAs (with the tag as a trailing comment), most
  critically `tauri-apps/tauri-action` in the workflow holding
  `TAURI_SIGNING_PRIVATE_KEY`.
- **D3 · GAP-36.** `ci.yml` gets a top-level `permissions: contents:
  read`; the signing env moves so PR builds (same-repo included) build
  unsigned — signing only on push to `main` and in the release workflow.
- **D4 · GAP-37.** `bump-version.yml` passes the dispatch input via `env:`
  and quotes it (`"$VERSION"`) everywhere, including the branch name.
- **D5 · GAP-41.** `release.yml` validates before building: the run must
  be on `main` (`github.ref_name` guard) and `inputs.tag` must equal
  `"v" + tauri.conf.json's version` (a step that fails the job otherwise).
- **D6 · GAP-42.** The release job gains an explicit gate step that
  queries the released SHA's check runs via the GitHub API and fails
  unless the CI workflow succeeded for that commit (works identically for
  the tag-push and dispatch paths) — a tag on a red commit refuses to
  publish.
- **D7 · GAP-43.** The `windows-app` job runs `cargo test` for core,
  capture, and transcribe (`--features whisper`) so the platform-sensitive
  code finally executes on Windows.
- **D8 · GAP-44 (one item).** `bump-version.mjs` rejects a new version
  `<=` the current one with a clear message. (npm audit/Dependabot,
  SECURITY.md, CHANGELOG stay out of scope.)

## Sub-pass E — Tech-debt splits & docs

Gaps: GAP-45/46/47 (the split obligations recorded in the LOC allowlist),
GAP-49 (Medium docs), GAP-50 (rename only).

- **E1.** `core/src/tasks.rs` (~1430 nonblank lines) splits into focused
  modules under `core/src/tasks/`: parsing/normalization (tags, scalars,
  validity), the `set_fields` writer, list/sort/collect, and disk
  operations (create/update/status) — public API preserved via
  `pub use` re-exports so `task_commands.rs`, `services.rs`, and tests
  keep compiling with at most import-path edits. Inline tests move with
  their units. The LOC allowlist entry for `tasks.rs` is **deleted** (each
  new module must land under the 800 cap).
- **E2.** `Tasks.vue` (~760 nonblank lines) extracts self-contained child
  components — the row (checkbox/chips/actions) and the inline editor, and
  the add-composer if still needed to clear the cap — props-down/events-up,
  no store coupling changes. The allowlist entry is deleted or, if the
  container legitimately stays above 500 after extraction, re-recorded at
  the reduced size with an updated reason.
- **E3.** The ActionPanel/VaultList duplicated button markup (fallow clone
  groups a0359856/920f14c5 + the All-tasks bar clone) extracts into one
  shared presentational component; the fallow baseline re-ratchets down.
- **E4 · GAP-49 + GAP-50.** `docs/PRD - Product Vision.md` renames to
  `docs/PRD.md`; the README front-page link, DEVELOPMENT.md, AGENTS.md doc
  map, and use-case catalog referrers update. DEVELOPMENT.md's crate list
  ("three crates" → five, incl. mcp), test-command list, and signing-claims
  fix; the PR template drops the stale "can't compile in this container"
  claim and names all four CI jobs; the PRD status line updates to the
  shipped reality; the stale `release.yml` comment goes.
- **E5.** Final sweep: Gaps.md section 8/9 entries updated, PR #46 body
  gains the polish-pass summary, full gate run.

## Testing & verification strategy

- Pure-logic fixes (A2, A3, A5's clamp policy, A9, C-all, D8, E1/E2) are
  unit/component-TDD'd on Linux.
- Path/containment fixes (A1, A7) get positive + escape-attempt tests with
  real temp dirs and symlinks where the platform allows.
- Windows-only mechanics (A6's `MoveFileExW`, D1's runtime CSP, B's
  perceived responsiveness) are exercised by D7's new Windows `cargo
  test` step where testable, and otherwise carry an explicit
  manual-verification note in the plan — no silent "verified" claims.
- Sub-pass B adds regression tests asserting the moved commands are
  `async` (compile-level) and that timeout paths return the typed
  still-saving variant.

## Risks & mitigations

- **A8 (wedged-device quit)** touches the never-lose-audio invariant: the
  bypass is scoped to reservations with no `.part` on disk, and the plan
  requires a test proving stop-while-recording still waits.
- **B (sync→async)** risks subtle frontend timing changes: the capture
  store's event-driven state (not command return values) already drives
  the UI, and the existing Vitest suite pins those flows; the plan adds
  cases for the new rejection timing.
- **D1 (CSP)** can break asset loading in the packaged app: it ships
  behind the manual Windows verification step and is a one-line revert.
- **E1/E2 (splits)** risk semantic drift: they are move-only tasks — the
  plan forbids behavior changes in the same commit, and the full test
  suites must pass unmodified (import paths aside) before and after.
