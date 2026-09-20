# Screen Capture Phase 5 — Export & Save Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A staged screen capture can be exported and saved into an Obsidian vault as a playable `.mp4` plus a companion note — the **ninth sanctioned vault write**, and the first time this feature touches a vault at all. An unedited capture takes a no-re-encode remux fast path; an edited one is decoded, restamped span-by-span and re-encoded, with progress and cancellation. Interrupted work is recovered, and a staged capture can be resumed or discarded.

**Architecture:** Media Foundation reads the staged fragmented MP4 back through an `IMFSourceReader` and writes a **standard** MP4 through `MFCreateMPEG4MediaSink`. Two modes share one reader: **passthrough** (no output media type set, so MF hands back the compressed H.264/AAC samples untouched) drives `Timeline::is_untouched`'s fast path; **decoded** (NV12 + PCM16 output types set, so MF inserts its own decoders) drives the edited path, whose span order and output timestamps come from the already-written, already-tested pure `screen::select::plan`. Everything a Linux runner can hold — the span arithmetic, the restamping, the vault naming and reservation, the note rendering, the disk-space estimate, the user-facing wording — lives in pure modules; the `cfg(windows)` arms only move bytes between them.

**Tech Stack:** Rust (`vault_buddy_screen`, `vault_buddy_core`, the Tauri shell), Windows Media Foundation (`IMFSourceReader`, `MFCreateMPEG4MediaSink`, `IMFSinkWriter`), Vue 3 + Pinia + Tailwind 4, Vitest + happy-dom.

## Global Constraints

Every task's requirements implicitly include this section.

- **Branch:** `claude/screen-capture-intake-g0j49q`. PR #79 is already open for it. **Never open a second PR, and never one against `main`.** `main` is red for an unrelated reason (a concept drop broke ESLint there); this branch carries the fix and it rides along at merge.
- **This phase breaks the vault seal.** Before writing any of it, read AGENTS.md's "The vault domain" section and the eight existing sanctioned writes. Every one of them is collision-safe, atomic (exclusive-create temp → fsync → `rename_noreplace`, **never** `std::fs::rename` for a first write), and never clobbers. The ninth must be indistinguishable in discipline. **No other code may touch vault contents** — a write outside the paths this plan names is a design change, not a patch.
- **Key the export fast path on `Timeline::is_untouched(source_duration_ms)`, NEVER on `Option::is_none()`.** The editor writes the timeline on every edit and never writes `null`; the sidecar always carries one after the first edit. It used to clear the field whenever the timeline matched the one the editor opened with — true only for a capture opened unedited, and on a *resumed* edit that told the exporter a previously-edited recording had never been touched, resurrecting deleted footage with every test green. Fixed in `6944ac0`; four tests now forbid the old rule.
- **`core::timeline` and `screen::select` are already complete and unit-tested.** `Timeline::{whole, split_at, delete, reorder, output_duration_ms, to_source_ms, is_untouched, is_empty}` and `select::plan(&Timeline) -> Vec<PlanSpan>`. **Consume them; do not reimplement, and do not re-derive an export plan by hand.** Phase 5 is where both finally get a production caller — the phase-4 plan's claim that `core::timeline` already had one was **false and was struck**; do not re-tick it without checking.
- **The segment algebra exists twice, in two languages.** `core::timeline` (Rust — what export plans on) and `src/utils/timelineGeometry.ts` + `useEditorTimeline.ts` (TypeScript — what the user watched). They measurably diverged before being reconciled. `tests/fixtures/timeline-cases.json` is read by `tests/timelineFixtures.test.ts` **and** by `core/src/timeline.rs` through `include_str!`, so moving the file breaks a Rust build. **Add a row to that table for anything export depends on** rather than trusting the two to agree; never write a Rust-only fixture for a mapping the frontend also implements (docs/Gaps.md GAP-136).
- **Verified environment limit:** `cargo clippy -p vault-buddy --target x86_64-pc-windows-msvc` is **impossible** in this container at any scope (`ring v0.17.14` fails in cc-rs with "failed to find tool lib.exe"). `cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc` and `-p vault_buddy_capture --lib` with that target **do** work and are the only local check of `cfg(windows)` code. Never list the shell one as a gate; never report it as skipped or passing.
- **Baselines are shrink-only.** Current floors: `deadCodeIssues=2`, `complexFunctions=13`, `criticalComplexity=4`, `averageMaintainability=91.5`, frontend LOC cap 500, Rust cap 800, `llvm-cov --fail-under-lines 94`, frontend coverage 95/91/93/96. **No counter metric has been loosened anywhere on this branch** — keep it that way. If a metric moves the wrong way, **extract, do not loosen.** The fallow ratchet has almost no slack: its maintainability index tracks complexity density and comment ratio, not line count, so pay pressure back with a genuine extraction or a well-commented low-complexity module.
- **NEVER run `npm run check:loc -- --update`** (it rewrites every escaped em dash across all 11 entries) **nor `npm run check:quality -- --update`** (it REPLACES the whole `description` field, destroying every recorded justification). Hand-edit the single value.
- **Gate discipline:** never pipe a gate through `grep`/`tail` in a way that swallows its exit code. Redirect to a file and tail the file, or put `; echo "exit=$?"` on the command itself. This has bitten four separate agents on this branch.
- **Commits:** Conventional Commits, `git commit -F <file>` with a heredoc. **Never put a backtick in a commit message** — they shell-expand and have silently deleted words from commit messages on this branch. Trailer exactly:
  ```
  Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
  Claude-Session: https://claude.ai/code/session_01Ud2Yq3jSXMKVdZaMSTeRco
  ```
- **Manual Windows verification is DEFERRED** until after the final phase, by the user's standing decision. Tasks write checklist rows; nobody runs them and nobody is asked to.
- **Every spawned thread is named** (`std::thread::Builder`). No swallowed errors — anything caught-and-hidden goes through `log::warn!`/`log::error!` or `src/logging.ts`.
- **`sink.rs` carries two structural self-scans** over its own pre-`#[cfg(test)]` prefix: it must never contain `.Flush(` and never contain `metadata(`. Task 4 adds code to that file and must not trip either.
- **`screen_commands.rs` carries three more structural scans**: exactly one `release(CaptureKind::Screen)` in its production prefix and exactly zero in `screen_capture_worker.rs`; the monitor clears before it emits; the exclusion clear lives inside `clear_active_screen`. New export code must not add a `CaptureKind::Screen` release anywhere (see Task 7 — export deliberately does **not** claim `CaptureGuard`).

## THE RECURRING FIXTURE FLAW — thirteen occurrences across four phases

A fixture that trips **more than one guard**, or that asserts a value captured **before** the action, proves nothing about the thing it names. Every phase of this increment has shipped at least one. The sharpest: the drag seam at `EditorRoot.vue` was explicitly briefed as the thing that fails silently **and** explicitly tested — yet replacing its conversion with an unconditional decrement left **all 1378 tests green**, breaking every leftward drag, because the test pinned one direction only.

**Ask of every fixture in this plan: would it still pass if the thing it names ran backwards, or did nothing at all?** Mutate one thing at a time, run, paste the real output, and restore byte-identically — back up with `cp` + `md5sum` first, and **never** use `git checkout --`, which restores to HEAD rather than your pre-mutation state and has destroyed work on this branch.

Every task below carries a **Mutation table**. Run every row. A row that stays green is a finding, not a formality — report it rather than deleting the row.

## The honest limit of this phase

`reader.rs`, `sink::StandardSink` and `export.rs`'s `cfg(windows)` arms will execute in **no automated test on any platform** — not on Linux (no Media Foundation), not on Windows CI (no way to assert a decoded frame is the right frame). That is GAP-117's class, and this phase adds the largest instance of it yet. Two consequences the task boundaries are drawn around:

1. **Push every decision that is not a COM call into a pure module.** The span order, the restamping arithmetic, the progress fraction, the vault path, the reserved name, the note bytes, the disk estimate, and every user-facing message are all Linux-testable and all live outside the `cfg(windows)` arms. What remains inside is "call `ReadSample`, hand the bytes to a function that was tested".
2. **The manual checklist is this phase's real gate.** Task 12 adds rows 29–36. They are the only place "the exported file actually plays in Obsidian, and shows the cuts the editor showed" is ever checked.

## File Structure

| File | Responsibility |
| --- | --- |
| `src-tauri/screen/src/select.rs` | **Modify.** Gains `PlanSpan::restamp`, `plan_output_duration_ms`, `progress_fraction`. Pure; the export's whole ordering and timestamp story. |
| `src-tauri/core/src/timeline.rs` | **Modify (tests only).** Two new shared-fixture assertions covering `is_untouched`, the rule the fast path keys on. |
| `tests/fixtures/timeline-cases.json` | **Modify.** Gains an `isUntouched` field per case plus two cases that separate "untouched" from "one segment". |
| `src/utils/timelineGeometry.ts` | **Modify.** Gains `isUntouched(timeline, durationMs)` so the shared table can run the rule through both languages. |
| `src-tauri/core/src/screen_capture_paths.rs` | **New.** The ninth vault write's naming: pairwise `.mp4` + `.md` reservation, and the commit-with-suffix-retry loop. In `core` because the one collision-suffix minter (`capture_paths::candidate`) is `pub(crate)` to that crate. |
| `src-tauri/core/src/screen_capture_config.rs` | **Modify.** Gains `export_size_estimate_bytes` — the pure half of the disk-pressure check. |
| `src-tauri/screen/src/disk.rs` | **New.** `free_bytes(&Path)`: `GetDiskFreeSpaceExW` on Windows, `Unsupported` elsewhere. |
| `src-tauri/screen/src/sink.rs` | **Modify.** `StandardSink` beside `FragmentedSink`: `MFCreateMPEG4MediaSink`, plus a passthrough constructor that adopts media types handed to it rather than declaring its own. |
| `src-tauri/screen/src/reader.rs` | **New.** `SourceReader` over a staged `.mp4`: open, describe, seek, read one sample. Two modes — decoded (NV12/PCM16) and passthrough (native compressed types). |
| `src-tauri/screen/src/export.rs` | **New.** The two export paths, the cancel poll, the progress callback. Its outcome type and every user-facing string are pure. |
| `src-tauri/src/export_commands.rs` | **New.** `export_and_save_capture`, `cancel_export`, `discard_staged_capture`, `list_staged_captures`, `open_screen_capture`; `ExportState` and the one `clear_active_export` chokepoint. |
| `src-tauri/src/export_worker.rs` | **New.** The `screen-export` named thread: export to a staging temp, commit into the vault, write the note, and only then delete the staged capture. |
| `src-tauri/src/screen_recovery.rs` | **New.** `run_screen_recovery`, wired into `setup` after `run_import_recovery`. |
| `src-tauri/src/lib.rs` | **Modify.** Three `mod` lines, `.manage(ExportState)`, five `generate_handler!` entries, one `setup` line. |
| `src/roots/EditorRoot.vue` | **Modify.** Save / Discard, the export progress bar, the four `screen:export*` listeners, and the `preventDefault` target guard the source already asks for. |
| `src/components/editor/ExportBar.vue` | **New.** Presentational: idle → Save / Discard, exporting → progress + Cancel, done → a message. Keeps `EditorRoot` under its cap. |
| `src/components/StagedCaptureList.vue` | **New.** Presentational: the resume-or-discard rows, shown first in Record Screen. |
| `src/components/ScreenSourcePicker.vue` | **Modify.** Renders `StagedCaptureList` above the tabs. |
| `src/stores/screenCapture.ts` | **Modify.** Clears `lastStaged` when the base it names is exported or discarded. |
| `src/types.ts` | **Modify.** `StagedCaptureSummary`, `ExportProgress`, `ExportResult`. |

---

### Task 1: The export plan's arithmetic, and the rule the fast path keys on

The whole of the edited export's correctness is "which source spans, in what order, and what output timestamp does each sample land on". `screen::select::plan` already answers the first two and is unit-tested. This task adds the third — the **source → output** restamp, which is the inverse of `Timeline::to_source_ms` and the one direction the phase-4 review found easiest to get subtly wrong (`<=` instead of `<` on `sourceEndMs` stayed fully green against seven fixtures, because the cut-out probes fell outside every segment under both rules and the round-trip test only walked output → source → output). It also pins `is_untouched` in the shared fixture table, because that single predicate is what decides between "remux, no quality loss" and "re-encode the whole recording".

**Files:**
- Modify: `src-tauri/screen/src/select.rs`
- Modify: `src-tauri/core/src/timeline.rs` (tests only)
- Modify: `tests/fixtures/timeline-cases.json`
- Modify: `tests/timelineFixtures.test.ts`

**Interfaces:**
- Consumes: `vault_buddy_core::timeline::{Timeline, Segment}`; `screen::select::{PlanSpan, plan}` (both already present).
- Produces, for Tasks 5–7:
  ```rust
  impl PlanSpan {
      pub fn restamp(&self, source_ms: u64) -> Option<u64>;
  }
  pub fn plan_output_duration_ms(spans: &[PlanSpan]) -> u64;
  pub fn progress_percent(written_output_ms: u64, total_output_ms: u64) -> u64;
  pub fn progress_fraction(written_output_ms: u64, total_output_ms: u64) -> f64;
  ```

**Why `progress_percent` and `progress_fraction` are both here, and one is derived from the other:** `core::throttle::EmitThrottle::should_emit` takes a `u64`, and spec §8.3 says the event carries a `fraction`. If the throttle gated on an integer percent while the event carried an independently-computed float, the value that passes the gate and the value the user sees could disagree at a boundary. `progress_fraction` is defined as `progress_percent(..) as f64 / 100.0` so there is exactly one computation and the emitted number is always the one that was gated.

- [ ] **Step 1: Write the failing tests in `src-tauri/screen/src/select.rs`**

Append inside the existing `#[cfg(test)] mod tests`:

```rust
    // The inverse of Timeline::to_source_ms, and the direction the phase-4
    // review found easiest to get subtly wrong: a `<=` on source_end_ms
    // stayed green against every fixture that probed only cut-out points,
    // because those fall outside every span under BOTH rules. These probe a
    // span's OWN far edge, which is the only place the two rules differ.
    #[test]
    fn a_span_restamps_its_own_source_range_and_refuses_its_far_edge() {
        let span = PlanSpan {
            source_start_ms: 4_000,
            source_end_ms: 6_000,
            output_start_ms: 1_000,
        };
        assert_eq!(span.restamp(4_000), Some(1_000));
        assert_eq!(span.restamp(5_999), Some(2_999));
        // Half-open, exactly like Timeline::to_source_ms: the end instant
        // belongs to whatever span comes next, never to this one.
        assert_eq!(span.restamp(6_000), None);
        assert_eq!(span.restamp(3_999), None);
    }

    #[test]
    fn restamping_a_reordered_plan_puts_later_source_earlier_in_the_output() {
        let t = Timeline::whole(6_000)
            .split_at(2_000)
            .split_at(4_000)
            .reorder(0, 2);
        let spans = plan(&t);
        // Source 4_500 lives in the span that now plays SECOND, so it lands
        // after the first span's 2_000 ms but before the third's.
        let hit: Vec<u64> = spans.iter().filter_map(|s| s.restamp(4_500)).collect();
        assert_eq!(hit, vec![2_500]);
        // Source 500 is the block moved to the END, so it lands last.
        let hit: Vec<u64> = spans.iter().filter_map(|s| s.restamp(500)).collect();
        assert_eq!(hit, vec![4_500]);
    }

    #[test]
    fn a_source_instant_belongs_to_exactly_one_span_of_a_disjoint_plan() {
        let t = Timeline::whole(9_000).split_at(3_000).split_at(6_000);
        let spans = plan(&t);
        for source_ms in [0u64, 2_999, 3_000, 5_999, 6_000, 8_999] {
            let hits = spans.iter().filter(|s| s.restamp(source_ms).is_some()).count();
            assert_eq!(hits, 1, "source {source_ms} matched {hits} spans");
        }
        assert!(spans.iter().all(|s| s.restamp(9_000).is_none()));
    }

    #[test]
    fn the_plans_output_duration_is_the_sum_of_its_spans() {
        let t = Timeline::whole(9_000).split_at(2_000).split_at(5_000).delete(1);
        let spans = plan(&t);
        assert_eq!(plan_output_duration_ms(&spans), 6_000);
        assert_eq!(plan_output_duration_ms(&[]), 0);
    }

    #[test]
    fn progress_is_an_integer_percent_that_clamps_at_both_ends() {
        assert_eq!(progress_percent(0, 8_000), 0);
        assert_eq!(progress_percent(2_000, 8_000), 25);
        assert_eq!(progress_percent(8_000, 8_000), 100);
        // A restamped sample can sit a hair past the planned end (the last
        // frame's duration runs off the edge); it must read 100, not 101.
        assert_eq!(progress_percent(8_400, 8_000), 100);
    }

    #[test]
    fn progress_of_an_empty_export_is_complete_rather_than_a_division_by_zero() {
        assert_eq!(progress_percent(0, 0), 100);
        assert_eq!(progress_fraction(0, 0), 1.0);
    }

    // ONE computation, two shapes: the number the throttle gates on and the
    // number the user sees must never disagree at a boundary.
    #[test]
    fn the_emitted_fraction_is_exactly_the_gated_percent() {
        for (written, total) in [(0u64, 7_000u64), (1_000, 7_000), (3_500, 7_000), (7_000, 7_000)] {
            let pct = progress_percent(written, total);
            assert_eq!(progress_fraction(written, total), pct as f64 / 100.0);
        }
    }
```

- [ ] **Step 2: Run the tests and watch them fail**

```
cd src-tauri && cargo test -p vault_buddy_screen select 2>&1 | tail -20
```

Expected: compile errors — `no method named restamp`, `cannot find function plan_output_duration_ms`, `progress_percent`, `progress_fraction`.

- [ ] **Step 3: Implement, in `src-tauri/screen/src/select.rs`**

Add to `impl PlanSpan`, directly under `duration_ms`:

```rust
    /// Map a SOURCE timestamp onto the OUTPUT timeline, or `None` when it
    /// does not fall inside this span.
    ///
    /// HALF-OPEN, exactly like `Timeline::to_source_ms`: `source_end_ms`
    /// itself is not in the span. The two rules must agree or the exported
    /// file does not match the preview the user approved — and a `<=` here
    /// would put one source instant in two spans at every cut, so the
    /// exporter would write the same frame at two different output times
    /// and the muxer would see a backwards timestamp.
    pub fn restamp(&self, source_ms: u64) -> Option<u64> {
        if source_ms < self.source_start_ms || source_ms >= self.source_end_ms {
            return None;
        }
        Some(self.output_start_ms + (source_ms - self.source_start_ms))
    }
```

Add as free functions after `plan`:

```rust
/// Total length of the exported file, in output milliseconds.
pub fn plan_output_duration_ms(spans: &[PlanSpan]) -> u64 {
    spans
        .iter()
        .fold(0u64, |acc, s| acc.saturating_add(s.duration_ms()))
}

/// Export progress as a whole percent, 0..=100.
///
/// Integer arithmetic on purpose: this is what `core::throttle` gates on,
/// and `progress_fraction` derives the emitted float from it, so the number
/// that passes the gate and the number the user sees are the same number.
/// A zero-length plan is COMPLETE rather than a division by zero — there is
/// nothing left to write.
pub fn progress_percent(written_output_ms: u64, total_output_ms: u64) -> u64 {
    if total_output_ms == 0 {
        return 100;
    }
    let scaled = written_output_ms.saturating_mul(100) / total_output_ms;
    scaled.min(100)
}

/// The same progress as the fraction spec 8.3's event carries.
pub fn progress_fraction(written_output_ms: u64, total_output_ms: u64) -> f64 {
    progress_percent(written_output_ms, total_output_ms) as f64 / 100.0
}
```

- [ ] **Step 4: Run the tests and watch them pass**

```
cd src-tauri && cargo test -p vault_buddy_screen select 2>&1 | tail -5
```

Expected: `test result: ok.` with 7 more tests than before.

- [ ] **Step 5: Add `isUntouched` rows to the shared fixture table**

In `tests/fixtures/timeline-cases.json`, add an `"isUntouched"` array to **every** object in `cases`. Each entry is `[sourceDurationMs, expected]`. For the existing `"one whole segment"` case (segments `[{0, 6000}]`):

```json
      "isUntouched": [
        [6000, true],
        [5999, false],
        [12000, false]
      ],
```

Every other case in the table has either more than one segment or a non-zero start, so its rows are all `false` — give each at least `[[6000, false]]`, using whatever source duration that case was built around.

Then add two NEW cases whose only job is to separate "untouched" from "has one segment", because that is the distinction the fast path turns on:

```json
    {
      "name": "one segment that does not start at zero is NOT untouched",
      "segments": [{ "sourceStartMs": 1000, "sourceEndMs": 6000 }],
      "outputDurationMs": 5000,
      "toSourceMs": [[0, 1000], [4999, 5999], [5000, null]],
      "splitAt": {
        "outputMs": 2000,
        "segments": [
          { "sourceStartMs": 1000, "sourceEndMs": 3000 },
          { "sourceStartMs": 3000, "sourceEndMs": 6000 }
        ]
      },
      "isUntouched": [[6000, false], [5000, false]]
    },
    {
      "name": "one segment covering the whole source is untouched at that duration only",
      "segments": [{ "sourceStartMs": 0, "sourceEndMs": 10000 }],
      "outputDurationMs": 10000,
      "toSourceMs": [[0, 0], [9999, 9999], [10000, null]],
      "splitAt": {
        "outputMs": 4000,
        "segments": [
          { "sourceStartMs": 0, "sourceEndMs": 4000 },
          { "sourceStartMs": 4000, "sourceEndMs": 10000 }
        ]
      },
      "isUntouched": [[10000, true], [9999, false], [10001, false]]
    }
```

Also add the field to the `"$comment"` block so the next reader knows the rule:

```json
    "isUntouched rows are [sourceDurationMs, expected] for Timeline::is_untouched",
    "-- the predicate phase 5's export fast path keys on. RUST ASSERTS THESE;",
    "the Vitest side deliberately does NOT reimplement the rule (6944ac0 chose",
    "not to put a second copy of it one IPC hop from the original) and asserts",
    "only that every case declares the field, so a new case cannot skip it."
```

- [ ] **Step 6: Assert those rows on the Rust side**

In `src-tauri/core/src/timeline.rs`'s test module, beside the existing `shared_fixture_table_*` tests, add:

```rust
    // The predicate phase 5's export fast path keys on. It lives in the
    // SHARED table rather than a Rust-only fixture so a later frontend
    // change that reshapes a case cannot quietly stop exercising it.
    #[test]
    fn shared_fixture_table_pins_is_untouched() {
        let table: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/timeline-cases.json"))
                .expect("fixture table parses");
        let cases = table["cases"].as_array().expect("cases is an array");
        assert!(!cases.is_empty(), "the fixture table is empty");
        let mut checked = 0usize;
        for case in cases {
            let name = case["name"].as_str().unwrap_or("<unnamed>");
            let timeline = Timeline {
                segments: case["segments"]
                    .as_array()
                    .expect("segments is an array")
                    .iter()
                    .map(|s| Segment {
                        source_start_ms: s["sourceStartMs"].as_u64().expect("sourceStartMs"),
                        source_end_ms: s["sourceEndMs"].as_u64().expect("sourceEndMs"),
                    })
                    .collect(),
            };
            let rows = case["isUntouched"]
                .as_array()
                .unwrap_or_else(|| panic!("case {name:?} declares no isUntouched rows"));
            assert!(!rows.is_empty(), "case {name:?} has an empty isUntouched list");
            for row in rows {
                let duration = row[0].as_u64().expect("sourceDurationMs");
                let expected = row[1].as_bool().expect("expected");
                assert_eq!(
                    timeline.is_untouched(duration),
                    expected,
                    "case {name:?} at source duration {duration}"
                );
                checked += 1;
            }
        }
        // Vacuity guard: if a refactor ever makes `rows` empty everywhere,
        // the loop above passes while asserting nothing.
        assert!(checked >= 10, "only {checked} isUntouched rows were checked");
    }
```

- [ ] **Step 7: Assert the field's presence — and only that — on the TypeScript side**

In `tests/timelineFixtures.test.ts`, extend the case interface with `isUntouched: [number, boolean][];` and add:

```ts
  // DELIBERATELY NOT a behaviour assertion. `is_untouched` has ONE
  // implementation and it is Rust's (core/src/timeline.rs); 6944ac0 rejected
  // putting a second copy in TypeScript one IPC hop from the original, and
  // this file must not smuggle one back in. What it CAN do without owning
  // the rule is stop a new case from silently skipping it.
  it("every shared case declares isUntouched rows for the Rust side to assert", () => {
    expect(table.cases.length).toBeGreaterThan(0);
    for (const c of table.cases) {
      expect(Array.isArray(c.isUntouched), `${c.name} declares isUntouched`).toBe(true);
      expect(c.isUntouched.length, `${c.name} has rows`).toBeGreaterThan(0);
      for (const [duration, expected] of c.isUntouched) {
        expect(typeof duration, `${c.name} duration is a number`).toBe("number");
        expect(typeof expected, `${c.name} expectation is a boolean`).toBe("boolean");
      }
    }
  });
```

- [ ] **Step 8: Run both sides**

```
cd src-tauri && cargo test -p vault_buddy_core timeline 2>&1 | tail -5
cd /home/user/vault-buddy && npx vitest run tests/timelineFixtures.test.ts 2>&1 | tail -8
```

Expected: both green. The Rust test prints nothing on success; confirm the count rose.

- [ ] **Step 9: Mutation table — run EVERY row, paste the real output**

Back up first: `cp src-tauri/screen/src/select.rs /tmp/select.bak && md5sum src-tauri/screen/src/select.rs`. Restore with `cp` and re-check the md5. **Never `git checkout --`.**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `restamp`: `source_ms >= self.source_end_ms` → `source_ms > self.source_end_ms` | `a_span_restamps_its_own_source_range_and_refuses_its_far_edge` AND `a_source_instant_belongs_to_exactly_one_span_of_a_disjoint_plan` |
| M2 | `restamp`: `source_ms < self.source_start_ms` → `source_ms <= self.source_start_ms` | the same far-edge test (`restamp(4_000)` becomes `None`) |
| M3 | `restamp`: return `Some(self.output_start_ms + source_ms)` (drop the span-relative offset) | `restamping_a_reordered_plan_…` |
| M4 | `restamp`: swap to `self.source_start_ms + (source_ms - self.output_start_ms)` (the inverse, i.e. output→source) | `restamping_a_reordered_plan_…` |
| M5 | `plan_output_duration_ms`: fold over `source_end_ms` instead of `duration_ms()` | `the_plans_output_duration_is_the_sum_of_its_spans` |
| M6 | `progress_percent`: drop the `.min(100)` | `progress_is_an_integer_percent_that_clamps_at_both_ends` |
| M7 | `progress_percent`: `total_output_ms == 0` returns `0` instead of `100` | `progress_of_an_empty_export_is_complete_…` |
| M8 | `progress_fraction`: compute `written as f64 / total as f64` directly instead of from `progress_percent` | `the_emitted_fraction_is_exactly_the_gated_percent` (at 1_000/7_000: 0.142857… vs 0.14) |
| M9 | `Timeline::is_untouched`: relax the `source_start_ms: 0` pattern to any start | `shared_fixture_table_pins_is_untouched` (the new "does not start at zero" case) |
| M10 | Delete the `isUntouched` field from one case in the JSON | BOTH the Rust test (`declares no isUntouched rows`) and the Vitest presence test |

M4 is the row that matters most: it is the exact confusion — inverse instead of forward — that the phase-4 review caught in the TypeScript twin. If M4 stays green, the test is measuring something other than direction; fix the test before moving on.

- [ ] **Step 10: Gates and commit**

```
cd src-tauri && cargo fmt --check && cargo clippy -p vault_buddy_screen -p vault_buddy_core --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

```bash
git add src-tauri/screen/src/select.rs src-tauri/core/src/timeline.rs tests/fixtures/timeline-cases.json tests/timelineFixtures.test.ts
git commit -F /tmp/msg.txt
```

Message body must say: the restamp is the inverse of `to_source_ms` and is half-open for the same reason; `progress_fraction` is derived from `progress_percent` so the gated and displayed numbers cannot disagree; and `is_untouched` now rides the shared fixture table with the Rust side owning the rule.

---

### Task 2: The ninth sanctioned vault write — naming, reservation and commit

This is the machinery, not the caller. It goes in `vault_buddy_core` for a concrete reason: the **one** collision-suffix minter, `capture_paths::candidate`, is `pub(crate)` to that crate, and its own doc says reservation, the stop-time recheck and the note writer "must all mint names from here so they can never diverge". A screen-side copy of `format!("{base} ({attempt})")` would be a second scheme by definition.

Everything here is Linux-testable against a `tempfile::tempdir()`, and it is the highest-value testable surface in the phase — the part that can lose or clobber a user's file.

**Files:**
- Create: `src-tauri/core/src/screen_capture_paths.rs`
- Modify: `src-tauri/core/src/lib.rs` (one `pub mod` line, alphabetical: after `screen_capture_config`)

**Interfaces:**
- Consumes: `capture_paths::{candidate, rename_noreplace, safe_recording_root, assert_path_inside_vault}`; `capture_note::write_note_collision_safe`.
- Produces, for Task 7:
  ```rust
  pub struct ScreenCaptureNames { pub base: String, pub final_mp4: PathBuf, pub note_md: PathBuf }
  pub fn reserve_screen_names(dir: &Path, base: &str) -> ScreenCaptureNames;
  pub fn reserve_final_screen(dir: &Path, base: &str) -> (PathBuf, PathBuf);
  pub fn commit_screen_capture(from: &Path, dir: &Path, base: &str) -> Result<(PathBuf, PathBuf), String>;
  pub fn mp4_file_name(base: &str) -> String;
  ```

**The four properties this module exists to guarantee**, each with its own test below:

1. **Pairwise.** A base is usable only when BOTH `<base>.mp4` and `<base>.md` are free. Reserving them independently lets a capture land its video on `Demo.mp4` and its note on `Demo (2).md`, so the note embeds a file it does not sit beside.
2. **The move is the arbiter, not the check.** `commit_screen_capture` re-reserves and retries inside the loop, so a file created by a sync client between the check and the move fails with `AlreadyExists`, advances the suffix and retries. An `exists()`-then-`rename` would clobber.
3. **Never `std::fs::rename`.** It replaces on every platform. `rename_noreplace` is hard-link-based with a `MoveFileExW(.., 0)` fallback.
4. **A linked-but-not-unlinked source is SUCCESS.** `rename_noreplace` returns `Ok(())` when the hard link landed but removing the source failed. Treating that as an error sends this retry loop into an endless suffix-minting spin, because `to` now exists and every retry reads as a fresh collision. `capture_paths` documents this; a caller that "improves" it re-breaks it.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/screen_capture_paths.rs` containing ONLY the module doc and this test module, so Step 2's failure is real:

```rust
//! The ninth sanctioned vault write: where an exported screen capture and
//! its companion note land, and how they get there without ever clobbering.
//!
//! Lives in `core`, not in the screen crate, because the ONE collision
//! suffix scheme (`capture_paths::candidate`) is `pub(crate)` here and its
//! own doc requires every name-minting site to go through it.
//!
//! Mirrors the audio domain exactly in discipline — pairwise reservation,
//! `rename_noreplace` with suffix retry, the note written AFTER the video
//! commits and named from where the video actually landed.

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(dir: &std::path::Path, name: &str) {
        fs::write(dir.join(name), b"x").unwrap();
    }

    #[test]
    fn a_free_base_reserves_itself_unsuffixed() {
        let dir = tempfile::tempdir().unwrap();
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo");
        assert_eq!(names.final_mp4, dir.path().join("2026-09-20 1432 Demo.mp4"));
        assert_eq!(names.note_md, dir.path().join("2026-09-20 1432 Demo.md"));
    }

    // PAIRWISE. A taken .md alone must still push the .mp4 onto a suffix, or
    // the note ends up beside a video it does not name.
    #[test]
    fn a_taken_note_alone_pushes_the_video_onto_the_next_suffix() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "2026-09-20 1432 Demo.md");
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo (2)");
        assert_eq!(names.final_mp4, dir.path().join("2026-09-20 1432 Demo (2).mp4"));
        assert_eq!(names.note_md, dir.path().join("2026-09-20 1432 Demo (2).md"));
    }

    #[test]
    fn a_taken_video_alone_pushes_the_note_onto_the_next_suffix() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "2026-09-20 1432 Demo.mp4");
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo (2)");
    }

    #[test]
    fn reservation_walks_past_a_run_of_taken_suffixes() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "2026-09-20 1432 Demo.mp4");
        touch(dir.path(), "2026-09-20 1432 Demo (2).md");
        touch(dir.path(), "2026-09-20 1432 Demo (3).mp4");
        let names = reserve_screen_names(dir.path(), "2026-09-20 1432 Demo");
        assert_eq!(names.base, "2026-09-20 1432 Demo (4)");
    }

    #[test]
    fn committing_into_a_free_directory_lands_the_plain_base_and_moves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("staged.mp4");
        fs::write(&src, b"video-bytes").unwrap();
        let out = tempfile::tempdir().unwrap();

        let (mp4, note) = commit_screen_capture(&src, out.path(), "2026-09-20 1432 Demo").unwrap();

        assert_eq!(mp4, out.path().join("2026-09-20 1432 Demo.mp4"));
        assert_eq!(note, out.path().join("2026-09-20 1432 Demo.md"));
        assert_eq!(fs::read(&mp4).unwrap(), b"video-bytes");
        assert!(!src.exists(), "the staged temp was left behind");
    }

    // THE property this whole module exists for: an existing user file is
    // never overwritten, whatever its name collides with.
    #[test]
    fn committing_never_overwrites_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("staged.mp4");
        fs::write(&src, b"new-video").unwrap();
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("2026-09-20 1432 Demo.mp4"), b"PRECIOUS").unwrap();

        let (mp4, note) = commit_screen_capture(&src, out.path(), "2026-09-20 1432 Demo").unwrap();

        assert_eq!(mp4, out.path().join("2026-09-20 1432 Demo (2).mp4"));
        assert_eq!(note, out.path().join("2026-09-20 1432 Demo (2).md"));
        assert_eq!(
            fs::read(out.path().join("2026-09-20 1432 Demo.mp4")).unwrap(),
            b"PRECIOUS",
            "the pre-existing file was overwritten"
        );
        assert_eq!(fs::read(&mp4).unwrap(), b"new-video");
    }

    // The note name comes from WHERE THE VIDEO LANDED, not from the base the
    // caller asked for. A note named from the request would embed a file
    // that is not beside it as soon as the video took a suffix.
    #[test]
    fn the_note_path_is_derived_from_the_landed_video_not_the_requested_base() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("staged.mp4");
        fs::write(&src, b"v").unwrap();
        let out = tempfile::tempdir().unwrap();
        fs::write(out.path().join("Clip.mp4"), b"taken").unwrap();
        fs::write(out.path().join("Clip (2).mp4"), b"taken too").unwrap();

        let (mp4, note) = commit_screen_capture(&src, out.path(), "Clip").unwrap();

        assert_eq!(mp4.file_stem().unwrap(), "Clip (3)");
        assert_eq!(note.file_stem().unwrap(), "Clip (3)");
        assert_eq!(mp4.parent(), note.parent());
    }

    #[test]
    fn committing_a_missing_source_is_an_error_not_a_panic_or_an_empty_file() {
        let out = tempfile::tempdir().unwrap();
        let err = commit_screen_capture(
            std::path::Path::new("/definitely/not/here.mp4"),
            out.path(),
            "Clip",
        )
        .unwrap_err();
        assert!(err.contains("could not be moved"), "unexpected message: {err}");
        assert!(!out.path().join("Clip.mp4").exists());
    }

    #[test]
    fn the_stop_time_recheck_ignores_nothing_but_still_pairs_both_extensions() {
        let dir = tempfile::tempdir().unwrap();
        touch(dir.path(), "Clip.md");
        let (mp4, note) = reserve_final_screen(dir.path(), "Clip");
        assert_eq!(mp4, dir.path().join("Clip (2).mp4"));
        assert_eq!(note, dir.path().join("Clip (2).md"));
    }

    #[test]
    fn the_video_file_name_is_the_base_plus_mp4() {
        assert_eq!(mp4_file_name("2026-09-20 1432 Demo"), "2026-09-20 1432 Demo.mp4");
    }
}
```

- [ ] **Step 2: Add the module and run the tests to watch them fail**

Add `pub mod screen_capture_paths;` to `src-tauri/core/src/lib.rs` between `pub mod screen_capture_config;` and `pub mod screen_geometry;`.

```
cd src-tauri && cargo test -p vault_buddy_core screen_capture_paths 2>&1 | tail -20
```

Expected: compile errors — `cannot find function reserve_screen_names`, `commit_screen_capture`, `reserve_final_screen`, `mp4_file_name`, `cannot find type ScreenCaptureNames`.

- [ ] **Step 3: Implement, above the test module**

```rust
use crate::capture_paths::{candidate, rename_noreplace};
use std::path::{Path, PathBuf};

/// The pair of names an export reserves before it writes anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenCaptureNames {
    pub base: String,
    pub final_mp4: PathBuf,
    pub note_md: PathBuf,
}

/// The exported video's file name for a base — the string the companion
/// note embeds, so it has exactly one definition.
pub fn mp4_file_name(base: &str) -> String {
    format!("{base}.mp4")
}

/// PAIRWISE reservation: a base is usable only when BOTH the `.mp4` and the
/// `.md` are free.
///
/// Reserving the two independently would let the video land on `Demo.mp4`
/// while the note took `Demo (2).md`, so the note would embed a file that is
/// not the one beside it. The audio domain reserves four names for the same
/// reason; a screen capture has only two.
pub fn reserve_screen_names(dir: &Path, base: &str) -> ScreenCaptureNames {
    for attempt in 1u32.. {
        let candidate_base = candidate(base, attempt);
        let final_mp4 = dir.join(mp4_file_name(&candidate_base));
        let note_md = dir.join(format!("{candidate_base}.md"));
        if !final_mp4.exists() && !note_md.exists() {
            return ScreenCaptureNames {
                base: candidate_base,
                final_mp4,
                note_md,
            };
        }
    }
    unreachable!("suffix search always terminates")
}

/// The commit-time recheck. Identical to `reserve_screen_names` today — a
/// screen capture stages OUTSIDE the vault, so unlike the audio domain there
/// is no in-progress `.part` sitting in this directory to exclude. It is a
/// separate function anyway so `commit_screen_capture` reads the same as its
/// audio twin and so the two can diverge later without a caller change.
pub fn reserve_final_screen(dir: &Path, base: &str) -> (PathBuf, PathBuf) {
    let names = reserve_screen_names(dir, base);
    (names.final_mp4, names.note_md)
}

/// Move the exported temp into the vault under `base`, never replacing.
///
/// THE MOVE IS THE ARBITER, not the `exists()` check: a destination created
/// by a sync client between the reservation and the rename fails with
/// `AlreadyExists`, advances the suffix and retries. The returned note path
/// is derived from where the VIDEO actually landed, so the note can never
/// name a suffix the video does not have.
///
/// A `rename_noreplace` that linked the destination but could not remove the
/// source returns `Ok(())` by design — do NOT "fix" that into an error here,
/// or this loop spins forever: the destination now exists, so every retry
/// reads as a fresh collision.
pub fn commit_screen_capture(
    from: &Path,
    dir: &Path,
    base: &str,
) -> Result<(PathBuf, PathBuf), String> {
    loop {
        let (mp4, note) = reserve_final_screen(dir, base);
        match rename_noreplace(from, &mp4) {
            Ok(()) => return Ok((mp4, note)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            // Some Windows API paths report a taken destination as
            // PermissionDenied rather than AlreadyExists.
            Err(_) if mp4.exists() => continue,
            Err(e) => return Err(format!("the exported video could not be moved into the vault: {e}")),
        }
    }
}
```

- [ ] **Step 4: Run the tests and watch them pass**

```
cd src-tauri && cargo test -p vault_buddy_core screen_capture_paths 2>&1 | tail -5
```

Expected: `test result: ok. 10 passed`.

- [ ] **Step 5: Mutation table — run EVERY row**

`cp src-tauri/core/src/screen_capture_paths.rs /tmp/scp.bak && md5sum src-tauri/core/src/screen_capture_paths.rs`

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `reserve_screen_names`: drop `&& !note_md.exists()` | `a_taken_note_alone_pushes_the_video_onto_the_next_suffix` |
| M2 | `reserve_screen_names`: drop `!final_mp4.exists() &&` | `a_taken_video_alone_pushes_the_note_onto_the_next_suffix` AND `reservation_walks_past_a_run_of_taken_suffixes` |
| M3 | `commit_screen_capture`: `rename_noreplace` → `std::fs::rename` | `committing_never_overwrites_an_existing_file` (PRECIOUS is gone) |
| M4 | `commit_screen_capture`: hoist `reserve_final_screen` OUT of the loop | not caught by the suite as written — **this is expected**; note it in the report. It needs a concurrent creator, which a single-threaded test cannot stage. The `continue` arms are what make the loop meaningful; M5 covers them. |
| M5 | `commit_screen_capture`: turn the `AlreadyExists` arm into `Err(...)` | `committing_never_overwrites_an_existing_file` — but check WHICH way: on Linux `rename_noreplace` hard-links first, so confirm it goes red rather than assuming. If it stays green, say so. |
| M6 | `commit_screen_capture`: return `(mp4, dir.join(format!("{base}.md")))` — the note named from the REQUESTED base | `the_note_path_is_derived_from_the_landed_video_not_the_requested_base` |
| M7 | `candidate(base, attempt)` → `format!("{base} ({attempt})")` inline | nothing fails — **expected.** Record it: the suffix scheme's single-sourcing is a convention this module cannot self-enforce. Task 12 adds a note to AGENTS.md instead. |
| M8 | `mp4_file_name`: return `format!("{base}.MP4")` | `the_video_file_name_is_the_base_plus_mp4` |

- [ ] **Step 6: Gates and commit**

```
cd src-tauri && cargo fmt --check; echo "exit=$?"
cd src-tauri && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo test -p vault_buddy_core 2>&1 | tail -3
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

Note `cargo clippy --workspace` rather than `-p`: this task adds a public module to `core`, and a public item widening a shared crate is exactly what broke a downstream exhaustive match in phase 3 with every named gate green.

```bash
git add src-tauri/core/src/screen_capture_paths.rs src-tauri/core/src/lib.rs
git commit -F /tmp/msg.txt
```

Message body: the ninth sanctioned vault write's naming; why it lives in `core` (the `pub(crate)` suffix minter); the pairwise rule and the failure it prevents; and that the move is the arbiter.

---

### Task 3: The disk-pressure check — a pure estimate and an honest probe

Spec §10: "At save time the app checks free space before export and refuses with a clear message rather than filling the disk." Video is large, and the failure mode being avoided is a user losing a recording AND filling their disk on the same click.

The split matters. **The estimate is pure** and lives in `core` beside the bitrate maths it reuses. **The probe is a Windows API call** and lives in the screen crate, and it returns `Option<u64>` rather than `Result`, because the one thing this check must never do is refuse a save it cannot measure — a `Result` invites a caller to treat `Err` as "no space".

**Files:**
- Modify: `src-tauri/core/src/screen_capture_config.rs`
- Create: `src-tauri/screen/src/disk.rs`
- Modify: `src-tauri/screen/src/lib.rs` (one `pub mod disk;` line)

**Interfaces:**
- Consumes: `screen_capture_config::{ScreenQuality, bitrate_bps}` (already present).
- Produces, for Task 7:
  ```rust
  // core::screen_capture_config
  pub fn export_size_estimate_bytes(duration_ms: u64, width: u32, height: u32, fps: u32, quality: ScreenQuality) -> u64;
  pub fn export_space_shortfall(needed: u64, free: Option<u64>) -> Option<u64>;
  // screen::disk
  pub fn free_bytes(path: &std::path::Path) -> Option<u64>;
  ```

- [ ] **Step 1: Write the failing tests in `src-tauri/core/src/screen_capture_config.rs`**

```rust
    #[test]
    fn the_export_estimate_scales_with_duration_and_with_pixels() {
        let short = export_size_estimate_bytes(10_000, 1920, 1080, 30, ScreenQuality::Balanced);
        let long = export_size_estimate_bytes(20_000, 1920, 1080, 30, ScreenQuality::Balanced);
        let big = export_size_estimate_bytes(10_000, 3840, 2160, 30, ScreenQuality::Balanced);
        assert!(long > short, "doubling the duration must raise the estimate");
        assert!(big > short, "quadrupling the pixels must raise the estimate");
    }

    #[test]
    fn a_higher_quality_preset_estimates_a_bigger_file() {
        let low = export_size_estimate_bytes(10_000, 1280, 720, 30, ScreenQuality::Low);
        let high = export_size_estimate_bytes(10_000, 1280, 720, 30, ScreenQuality::High);
        assert!(high > low);
    }

    // A ten-second 1080p balanced capture is a couple of megabytes, not two
    // kilobytes and not two gigabytes. A unit slip (bits vs bytes, ms vs s)
    // is the whole risk in this function and only an absolute range catches it.
    #[test]
    fn the_estimate_is_in_a_plausible_absolute_range() {
        let bytes = export_size_estimate_bytes(10_000, 1920, 1080, 30, ScreenQuality::Balanced);
        assert!(
            (1_000_000..40_000_000).contains(&bytes),
            "10 s of 1080p estimated at {bytes} bytes"
        );
    }

    #[test]
    fn a_zero_length_export_still_reserves_the_headroom() {
        // Never zero: a container has overhead, and a 0 estimate would make
        // the shortfall check pass on a completely full disk.
        assert!(export_size_estimate_bytes(0, 1920, 1080, 30, ScreenQuality::Balanced) > 0);
    }

    #[test]
    fn a_shortfall_is_reported_only_when_free_space_is_known_and_insufficient() {
        assert_eq!(export_space_shortfall(100, Some(300)), None);
        assert_eq!(export_space_shortfall(100, Some(100)), None);
        assert_eq!(export_space_shortfall(300, Some(100)), Some(200));
    }

    // THE most important row here. An unmeasurable disk must never block a
    // save: the user would have no way to proceed, and the recording is the
    // irreplaceable artifact.
    #[test]
    fn an_unknown_free_space_never_refuses_the_save() {
        assert_eq!(export_space_shortfall(u64::MAX, None), None);
    }
```

- [ ] **Step 2: Run and watch it fail**

```
cd src-tauri && cargo test -p vault_buddy_core screen_capture_config 2>&1 | tail -15
```

Expected: `cannot find function export_size_estimate_bytes` / `export_space_shortfall`.

- [ ] **Step 3: Implement in `src-tauri/core/src/screen_capture_config.rs`**

```rust
/// Container and index overhead we reserve regardless of length, so a very
/// short export never estimates zero and passes the check on a full disk.
const EXPORT_OVERHEAD_BYTES: u64 = 1_048_576;

/// Headroom multiplier, as a percentage of the encoded estimate. The encoder
/// tracks its target bitrate loosely and the exported temp coexists with the
/// staged capture until the vault write lands, so "just enough" is not enough.
const EXPORT_HEADROOM_PERCENT: u64 = 150;

/// Roughly how many bytes an export of this length at this size and preset
/// will occupy, including headroom. Deliberately generous — refusing a save
/// that would have fitted is a smaller harm than filling the user's disk.
pub fn export_size_estimate_bytes(
    duration_ms: u64,
    width: u32,
    height: u32,
    fps: u32,
    quality: ScreenQuality,
) -> u64 {
    let video_bps = u64::from(bitrate_bps(quality, width, height, fps));
    // Audio is at most AAC stereo at the sink's ceiling (24 kB/s per the
    // AAC encoder's legal table); folding a fixed allowance in beats
    // threading the real device list down here for a number this rough.
    let audio_bps = 192_000u64;
    let bits = (video_bps + audio_bps).saturating_mul(duration_ms) / 1_000;
    let bytes = bits / 8;
    bytes
        .saturating_mul(EXPORT_HEADROOM_PERCENT)
        .saturating_div(100)
        .saturating_add(EXPORT_OVERHEAD_BYTES)
}

/// How many bytes short the destination is, or `None` when there is enough
/// room — or when free space could not be measured at all.
///
/// `None` for an UNKNOWN reading is the whole reason this takes an `Option`
/// rather than a `Result`: a probe that failed must never be read as "no
/// space left" and refuse a save the user cannot otherwise complete. The
/// recording is the irreplaceable artifact; a full disk surfaces as the
/// write's own error, which the export already handles.
pub fn export_space_shortfall(needed: u64, free: Option<u64>) -> Option<u64> {
    let free = free?;
    if free >= needed {
        return None;
    }
    Some(needed - free)
}
```

- [ ] **Step 4: Write `src-tauri/screen/src/disk.rs`**

```rust
//! Free space on the volume holding a path.
//!
//! Returns `Option<u64>`, never `Result`, and that is a deliberate API
//! choice rather than laziness: the ONE thing this probe must never do is
//! cause a save to be refused because the measurement failed. A `Result`
//! invites a caller to write `free.unwrap_or(0)`, which turns every
//! unmeasurable volume — and every non-Windows build — into a permanently
//! full disk. `None` means "unknown"; `core::screen_capture_config::
//! export_space_shortfall` consumes it and lets the save through.

use std::path::Path;

#[cfg(windows)]
pub fn free_bytes(path: &Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // GetDiskFreeSpaceExW wants a directory (or any path on the volume).
    let wide = HSTRING::from(path.as_os_str().to_string_lossy().as_ref());
    let mut available: u64 = 0;
    // SAFETY: `wide` outlives the call; the out-params are plain u64s.
    let ok = unsafe { GetDiskFreeSpaceExW(&wide, Some(&mut available), None, None) };
    match ok {
        Ok(()) => Some(available),
        Err(e) => {
            log::warn!(
                "screen export: could not measure free space at {}: {e}",
                path.display()
            );
            None
        }
    }
}

/// The Linux compile gate and the test suite. "Unknown", which by the rule
/// above means the save proceeds.
#[cfg(not(windows))]
pub fn free_bytes(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Off Windows this must be None, and `export_space_shortfall(_, None)`
    // must therefore let every save through. Pinning both halves here stops
    // a later "helpful" `unwrap_or(0)` from silently refusing every export
    // on the compile-gate build.
    #[cfg(not(windows))]
    #[test]
    fn an_unsupported_platform_reports_unknown_rather_than_zero() {
        assert_eq!(free_bytes(std::path::Path::new("/tmp")), None);
        assert_eq!(
            vault_buddy_core::screen_capture_config::export_space_shortfall(u64::MAX, free_bytes(std::path::Path::new("/tmp"))),
            None
        );
    }
}
```

Add `pub mod disk;` to `src-tauri/screen/src/lib.rs`, alphabetically before `pub mod exclusion;`.

**Add the `windows` feature this call needs.** Verified against the current file: `src-tauri/screen/Cargo.toml`'s `[target.'cfg(windows)'.dependencies] windows` block lists `Win32_Foundation`, `Win32_Graphics_Dwm`, `Win32_Media_MediaFoundation`, `Win32_System_Com` and `Win32_UI_WindowsAndMessaging` — and **not** `Win32_Storage_FileSystem`, which is where `GetDiskFreeSpaceExW` lives. Add it to that array, with a comment in the style of the ones already there naming the call and why. This is a feature on a crate the workspace already depends on at the same version, so it is **no new crate**, `cargo deny` is unaffected, and `Cargo.lock` does not gain an entry — confirm all three by running `cargo deny check` and `git diff Cargo.lock` after building.

- [ ] **Step 5: Run both crates' tests**

```
cd src-tauri && cargo test -p vault_buddy_core screen_capture_config 2>&1 | tail -5
cd src-tauri && cargo test -p vault_buddy_screen disk 2>&1 | tail -5
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "exit=$?"
```

The Windows-target clippy is the ONLY check of `free_bytes`'s Windows arm anywhere. If `GetDiskFreeSpaceExW`'s signature differs from the above in `windows` 0.62.2, fix it against the error and record the real signature in the commit body.

- [ ] **Step 6: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `export_space_shortfall`: `let free = free.unwrap_or(0);` | `an_unknown_free_space_never_refuses_the_save` |
| M2 | `export_space_shortfall`: `free > needed` instead of `free >= needed` | `a_shortfall_is_reported_only_when_…` (the exactly-equal row) |
| M3 | `export_size_estimate_bytes`: drop `/ 8` (bits reported as bytes) | `the_estimate_is_in_a_plausible_absolute_range` |
| M4 | `export_size_estimate_bytes`: drop `/ 1_000` (ms treated as s) | `the_estimate_is_in_a_plausible_absolute_range` |
| M5 | `export_size_estimate_bytes`: drop `saturating_add(EXPORT_OVERHEAD_BYTES)` | `a_zero_length_export_still_reserves_the_headroom` |
| M6 | `export_size_estimate_bytes`: ignore `width`/`height`, use a constant bitrate | `the_export_estimate_scales_with_duration_and_with_pixels` (the pixels half) |
| M7 | `disk::free_bytes` non-Windows arm: `Some(0)` | `an_unsupported_platform_reports_unknown_rather_than_zero` |

M6 is the row that makes the test earn its name: an estimate that scales with duration only would pass a test that doubles just the duration. Confirm the pixels assertion is what goes red.

- [ ] **Step 7: Gates and commit**

```
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo machete .; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

```bash
git add src-tauri/core/src/screen_capture_config.rs src-tauri/screen/src/disk.rs src-tauri/screen/src/lib.rs
git commit -F /tmp/msg.txt
```

Message body: why the probe returns `Option` and not `Result` — an unmeasurable volume must not refuse a save — and that the estimate is deliberately generous because refusing a save that would have fitted is the smaller harm.

---

### Task 4: `StandardSink` — the non-fragmented MP4 writer, in two flavours

The staged capture is a **fragmented** MP4 because a crash mid-recording must still leave a playable file (spec §6.4, measured). The exported file has the opposite requirement: it is finished before anyone sees it, and it should be the ordinary, maximally-compatible thing a player and Obsidian's embedded Chromium expect. So export writes through `MFCreateMPEG4MediaSink` — the same construction chain as `FragmentedSink`, one function name different, which is exactly the shape the existing sink's own comment at `sink.rs:383-385` points out.

Two constructors, because the two export paths need different input types:

- **`create`** declares NV12 video and PCM audio inputs, exactly like `FragmentedSink::create`, and MF inserts its encoders. This is the **edited** path's sink.
- **`create_passthrough`** is handed the media types the source reader reported and declares them as BOTH the output and the input type. When the input subtype equals the output subtype, MF inserts no transform: the compressed samples go from the demuxer to the muxer untouched. This is the **fast path's** sink, and it is what makes "no quality loss, near instant" true rather than aspirational.

**Files:**
- Modify: `src-tauri/screen/src/sink.rs`

**Interfaces:**
- Consumes: the private helpers already in `sink.rs` — `MfRuntime`, `video_output_type`, `video_input_type`, `audio_output_type`, `audio_input_type`, `to_hns`, `sink_err`, `VIDEO_STREAM`, `AUDIO_STREAM`, `VideoFormat`, `AudioFormat`.
- Produces, for Tasks 5–6:
  ```rust
  pub use imp::StandardSink;
  impl StandardSink {
      pub fn create(path: &Path, video: VideoFormat, audio: Option<AudioFormat>) -> Result<StandardSink, ScreenError>;
      #[cfg(windows)]
      pub fn create_passthrough(path: &Path, video: &IMFMediaType, audio: Option<&IMFMediaType>) -> Result<StandardSink, ScreenError>;
      pub fn write_video(&mut self, bytes: &[u8], ts: Duration, duration: Duration) -> Result<(), ScreenError>;
      pub fn write_audio(&mut self, bytes: &[u8], ts: Duration, duration: Duration) -> Result<(), ScreenError>;
      pub fn finalize(self) -> Result<(), ScreenError>;
  }
  ```

**Note the signature difference from `FragmentedSink`:** `write_audio` here takes `&[u8]`, not `&[i16]`. The capture path always has PCM16 in hand; export's passthrough mode has an opaque AAC frame. One byte-oriented method serves both, and it removes the `from_raw_parts` reinterpretation from this path entirely.

**Two rules this file enforces on itself — do not trip them.** `sink.rs`'s own `production_src()` scans assert its pre-`#[cfg(test)]` prefix contains neither `.Flush(` nor `metadata(`. Both apply to everything added here. `Flush` is documented to **drop** pending samples; only `Finalize` drains. Extend both scans' doc comments to say they now cover two sinks.

- [ ] **Step 1: Write the failing tests**

Append to `sink.rs`'s existing `#[cfg(test)] mod tests`:

```rust
    // The Linux compile gate must DEGRADE, never panic — the same contract
    // FragmentedSink's stub carries, and the reason the stub exists at all.
    #[cfg(not(windows))]
    #[test]
    fn the_non_windows_standard_sink_reports_unsupported_rather_than_panicking() {
        let path = std::path::Path::new("unused.mp4");
        let video = VideoFormat { width: 1280, height: 720, fps: 30, bitrate_bps: 2_000_000 };
        assert_eq!(StandardSink::create(path, video, None).unwrap_err(), ScreenError::Unsupported);
    }

    // The standard sink is the EXPORT target and the fragmented one is the
    // CAPTURE target. Getting these the wrong way round produces a staged
    // file that cannot survive a crash and an exported file that is
    // needlessly fragmented -- and neither shows a symptom until hardware.
    #[test]
    fn each_sink_declares_the_media_sink_its_purpose_requires() {
        let src = production_src();
        let frag_at = src
            .find("pub fn create(path: &Path, video: VideoFormat, audio: Option<AudioFormat>)")
            .expect("FragmentedSink::create is present");
        let std_at = src
            .find("impl StandardSink")
            .expect("StandardSink is present");
        let frag_call = src.find("MFCreateFMPEG4MediaSink").expect("fragmented sink call");
        let std_call = src.find("MFCreateMPEG4MediaSink(").expect("standard sink call");
        assert!(
            frag_call < std_at,
            "MFCreateFMPEG4MediaSink must sit in the FragmentedSink impl, before StandardSink"
        );
        assert!(
            std_call > std_at,
            "MFCreateMPEG4MediaSink must sit inside the StandardSink impl"
        );
        assert!(frag_at < std_at);
        // And exactly one of each, so neither sink can quietly grow the
        // other's constructor.
        assert_eq!(src.matches("MFCreateFMPEG4MediaSink(").count(), 1);
        assert_eq!(src.matches("MFCreateMPEG4MediaSink(").count(), 1);
    }

    // Both scans now cover two sinks; a new Flush anywhere in this file is
    // the bug that destroyed two spike runs' footage.
    #[test]
    fn neither_sink_calls_flush() {
        assert!(!production_src().contains(".Flush("));
    }
```

**`assert_eq!(…count(), 1)` on `MFCreateMPEG4MediaSink(` is deliberate and will be wrong unless you check:** `fmp4_spike.rs` also calls it, but that is a different file and `production_src()` reads only `sink.rs`. Confirm with `grep -c "MFCreateMPEG4MediaSink(" src-tauri/screen/src/sink.rs` after implementing; expect exactly 1.

- [ ] **Step 2: Run and watch them fail**

```
cd src-tauri && cargo test -p vault_buddy_screen sink 2>&1 | tail -20
```

Expected: `cannot find type StandardSink`, and `each_sink_declares_…` panicking on `StandardSink is present`.

- [ ] **Step 3: Implement the non-Windows arm**

Inside `#[cfg(not(windows))] mod imp`, beside `FragmentedSink`:

```rust
    /// The export target's stub. Same contract as `FragmentedSink`'s: it
    /// must DEGRADE rather than panic, because Linux is this crate's
    /// compile gate and every one of these methods is reachable there from
    /// a test that does not know it is on the wrong platform.
    pub struct StandardSink;

    impl StandardSink {
        pub fn create(
            _path: &Path,
            _video: VideoFormat,
            _audio: Option<AudioFormat>,
        ) -> Result<StandardSink, ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn write_video(
            &mut self,
            _bytes: &[u8],
            _ts: Duration,
            _duration: Duration,
        ) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn write_audio(
            &mut self,
            _bytes: &[u8],
            _ts: Duration,
            _duration: Duration,
        ) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }

        pub fn finalize(self) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }
    }
```

- [ ] **Step 4: Implement the Windows arm**

Inside `#[cfg(windows)] mod imp`, after `FragmentedSink`'s impl. Add `IMFMediaType` to that module's imports if it is not already there.

```rust
    /// The EXPORT target: a standard, non-fragmented MP4.
    ///
    /// Why the opposite container choice from `FragmentedSink`, ten lines
    /// away: a CAPTURE must survive being killed mid-write, so it pays for
    /// fragmentation (spec 6.4 measured 280/300 frames recovered from a
    /// crashed fMP4 against 0/300 from a crashed standard MP4). An EXPORT is
    /// finished before anyone sees it and then lives in the user's vault, so
    /// it should be the ordinary thing every player and Obsidian's embedded
    /// Chromium handle best. One function name is the whole difference.
    pub struct StandardSink {
        writer: IMFSinkWriter,
        has_audio: bool,
        /// Dropped last, after the writer: MFShutdown must not run while a
        /// Media Foundation object is still alive.
        _mf: MfRuntime,
    }

    impl StandardSink {
        /// The EDITED path's sink: declares NV12 video and PCM audio inputs,
        /// so MF inserts its encoders, exactly like the capture sink.
        pub fn create(
            path: &Path,
            video: VideoFormat,
            audio: Option<AudioFormat>,
        ) -> Result<StandardSink, ScreenError> {
            video.validate()?;
            if let Some(a) = audio {
                a.validate()?;
            }
            let mf = MfRuntime::start().map_err(|e| sink_err("MFStartup", e))?;
            let video_out = video_output_type(video).map_err(|e| sink_err("video output type", e))?;
            let audio_out = match audio {
                Some(a) => Some(audio_output_type(a).map_err(|e| sink_err("audio output type", e))?),
                None => None,
            };
            let writer = Self::writer_for(path, &video_out, audio_out.as_ref())?;
            let video_in = video_input_type(video).map_err(|e| sink_err("video input type", e))?;
            Self::set_input(&writer, VIDEO_STREAM, &video_in)?;
            if let Some(a) = audio {
                let audio_in = audio_input_type(a).map_err(|e| sink_err("audio input type", e))?;
                Self::set_input(&writer, AUDIO_STREAM, &audio_in)?;
            }
            unsafe { writer.BeginWriting() }.map_err(|e| sink_err("BeginWriting", e))?;
            Ok(StandardSink {
                writer,
                has_audio: audio.is_some(),
                _mf: mf,
            })
        }

        /// The FAST path's sink: adopts the reader's own media types as BOTH
        /// the output and the input type.
        ///
        /// That equality is the whole mechanism. When a sink writer's input
        /// subtype matches its output subtype, MF inserts no transform, so
        /// the demuxer's compressed H.264 and AAC samples reach the muxer
        /// untouched — no decode, no re-encode, no quality loss, and a
        /// remux that runs at disk speed. Passing a DIFFERENT input type
        /// here silently turns the fast path into a transcode that still
        /// produces a correct-looking file, which is why Task 6's caller
        /// hands over the reader's types rather than rebuilding them.
        pub fn create_passthrough(
            path: &Path,
            video: &IMFMediaType,
            audio: Option<&IMFMediaType>,
        ) -> Result<StandardSink, ScreenError> {
            let mf = MfRuntime::start().map_err(|e| sink_err("MFStartup", e))?;
            let writer = Self::writer_for(path, video, audio)?;
            Self::set_input(&writer, VIDEO_STREAM, video)?;
            if let Some(a) = audio {
                Self::set_input(&writer, AUDIO_STREAM, a)?;
            }
            unsafe { writer.BeginWriting() }.map_err(|e| sink_err("BeginWriting", e))?;
            Ok(StandardSink {
                writer,
                has_audio: audio.is_some(),
                _mf: mf,
            })
        }

        fn writer_for(
            path: &Path,
            video_out: &IMFMediaType,
            audio_out: Option<&IMFMediaType>,
        ) -> Result<IMFSinkWriter, ScreenError> {
            let byte_stream = unsafe {
                MFCreateFile(
                    MF_ACCESSMODE_WRITE,
                    MF_OPENMODE_DELETE_IF_EXIST,
                    MF_FILEFLAGS_NONE,
                    &HSTRING::from(path.to_string_lossy().as_ref()),
                )
            }
            .map_err(|e| sink_err("MFCreateFile", e))?;
            // STANDARD, not fragmented. See this type's doc comment.
            let sink: IMFMediaSink =
                unsafe { MFCreateMPEG4MediaSink(&byte_stream, Some(video_out), audio_out) }
                    .map_err(|e| sink_err("MFCreateMPEG4MediaSink", e))?;
            unsafe { MFCreateSinkWriterFromMediaSink(&sink, None) }
                .map_err(|e| sink_err("MFCreateSinkWriterFromMediaSink", e))
        }

        fn set_input(
            writer: &IMFSinkWriter,
            stream: u32,
            ty: &IMFMediaType,
        ) -> Result<(), ScreenError> {
            unsafe { writer.SetInputMediaType(stream, ty, None) }.map_err(|e| {
                if e.code() == MF_E_TOPO_CODEC_NOT_FOUND {
                    ScreenError::EncoderUnavailable
                } else {
                    sink_err("SetInputMediaType", e)
                }
            })
        }

        pub fn write_video(
            &mut self,
            bytes: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            self.write(VIDEO_STREAM, bytes, ts, duration)
        }

        /// Byte-oriented, unlike `FragmentedSink::write_audio`'s `&[i16]`:
        /// the edited path has PCM16 in hand and the fast path has an opaque
        /// AAC frame, and one method serves both without reinterpreting a
        /// slice.
        pub fn write_audio(
            &mut self,
            bytes: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            if !self.has_audio {
                return Ok(());
            }
            self.write(AUDIO_STREAM, bytes, ts, duration)
        }

        fn write(
            &mut self,
            stream: u32,
            bytes: &[u8],
            ts: Duration,
            duration: Duration,
        ) -> Result<(), ScreenError> {
            // Mirrors FragmentedSink::write exactly, including the empty
            // early-out: MFCreateMemoryBuffer(0) followed by a copy from a
            // null pointer is undefined behaviour.
            if bytes.is_empty() {
                return Ok(());
            }
            let len = u32::try_from(bytes.len())
                .map_err(|_| ScreenError::Sink("sample larger than 4 GiB".into()))?;
            unsafe {
                let buffer =
                    MFCreateMemoryBuffer(len).map_err(|e| sink_err("MFCreateMemoryBuffer", e))?;
                let mut dst: *mut u8 = std::ptr::null_mut();
                buffer
                    .Lock(&mut dst, None, None)
                    .map_err(|e| sink_err("IMFMediaBuffer::Lock", e))?;
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                buffer
                    .Unlock()
                    .map_err(|e| sink_err("IMFMediaBuffer::Unlock", e))?;
                buffer
                    .SetCurrentLength(len)
                    .map_err(|e| sink_err("SetCurrentLength", e))?;
                let sample = MFCreateSample().map_err(|e| sink_err("MFCreateSample", e))?;
                sample
                    .AddBuffer(&buffer)
                    .map_err(|e| sink_err("AddBuffer", e))?;
                sample
                    .SetSampleTime(to_hns(ts))
                    .map_err(|e| sink_err("SetSampleTime", e))?;
                sample
                    .SetSampleDuration(to_hns(duration))
                    .map_err(|e| sink_err("SetSampleDuration", e))?;
                self.writer
                    .WriteSample(stream, &sample)
                    .map_err(|e| sink_err("WriteSample", e))?;
            }
            Ok(())
        }

        /// There is NO Flush here and there must never be one — see this
        /// module's rules. Only Finalize drains.
        pub fn finalize(self) -> Result<(), ScreenError> {
            unsafe { self.writer.Finalize() }.map_err(|e| sink_err("Finalize", e))
        }
    }
```

Add the re-export beside the existing one, and widen both rule doc comments:

```rust
pub use imp::{FragmentedSink, StandardSink};
```

- [ ] **Step 5: Run the tests and both clippy targets**

```
cd src-tauri && cargo test -p vault_buddy_screen sink 2>&1 | tail -5
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "exit=$?"
grep -c "MFCreateMPEG4MediaSink(" screen/src/sink.rs
```

The Windows-target clippy is the ONLY check that any of Step 4 compiles. Expect to iterate here: `MFCreateMPEG4MediaSink`'s third parameter's optionality and `IMFMediaType`'s reference form in `windows` 0.62.2 may differ from the sketch. **Fix against the compiler's message and record the real signature in the commit body** — do not guess and move on, and do not add a second `windows` version to this crate (that trap is documented in AGENTS.md).

- [ ] **Step 6: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `StandardSink::writer_for`: `MFCreateMPEG4MediaSink` → `MFCreateFMPEG4MediaSink` | `each_sink_declares_the_media_sink_its_purpose_requires` (the count and the position both) |
| M2 | Add `let _ = unsafe { self.writer.Flush(VIDEO_STREAM) };` before `Finalize` | `neither_sink_calls_flush` AND the pre-existing `the_sink_never_calls_flush` |
| M3 | Non-Windows `StandardSink::create`: `panic!("unsupported")` | `the_non_windows_standard_sink_reports_unsupported_rather_than_panicking` |
| M4 | Non-Windows `StandardSink::create`: `Ok(StandardSink)` | the same test (`unwrap_err` panics) |
| M5 | `create_passthrough`: pass `video_input_type(...)` instead of the reader's `video` as the input type | **not caught by any test on any platform** — expected, and the reason the doc comment spells the mechanism out. Record it; Task 12 adds a checklist row for the observable symptom (a fast-path export that takes as long as a re-encode). |
| M6 | `write`: drop the `bytes.is_empty()` early-out | not caught (no Windows test) — record it. |

M5 and M6 are the honest ones: this file has no executable test on any platform, and the mutation table's job here is to make that visible rather than to imply coverage that does not exist.

- [ ] **Step 7: Gates and commit**

```
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo test -p vault_buddy_screen 2>&1 | tail -3
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

`sink.rs` is 740 lines against the 800 cap and this task adds roughly 190. **It will breach.** Do NOT raise the baseline. Split first: move the Windows `imp` module's media-type builders (`video_output_type`, `video_input_type`, `audio_output_type`, `audio_input_type`, and the AAC bitrate helpers, which are already shared-arm and already tested) into a new `src-tauri/screen/src/sink_types.rs`, re-exported into `sink::imp` with `use crate::sink_types::*;`. That is a genuine extraction — one file describes the formats, one drives the writers — and it keeps both under cap with the tests following the code they test. Report the resulting line counts.

```bash
git add src-tauri/screen/src/sink.rs src-tauri/screen/src/sink_types.rs src-tauri/screen/src/lib.rs
git commit -F /tmp/msg.txt
```

Message body: why export writes a standard MP4 where capture writes a fragmented one (opposite requirements, one function name apart); what makes passthrough a passthrough (input subtype equal to output subtype, so MF inserts no transform); and that the media-type builders were extracted rather than the LOC ceiling raised.

---

### Task 5: `screen::reader` — reading the staged capture back

The one Media Foundation read path in this repo today is ~35 lines inside `fmp4_spike.rs`, gated behind `#![cfg(all(windows, feature = "fmp4-spike"))]` and absent from every default build. It counts samples and nothing else: it never sets an output media type, never reads audio, never reads a timestamp, and never seeks. **Phase 5 builds the read side from scratch**; the spike is a reference to copy from, not a dependency.

**Files:**
- Create: `src-tauri/screen/src/reader.rs`
- Modify: `src-tauri/screen/src/lib.rs` (one `pub mod reader;` line, alphabetically after `pub mod mp4_boxes;`)

**Interfaces:**
- Consumes: `ScreenError`; `mp4_boxes::scan` for the precondition check.
- Produces, for Task 6:
  ```rust
  pub struct SourceDescription {
      pub width: u32,
      pub height: u32,
      pub fps: u32,
      pub duration_ms: u64,
      pub has_audio: bool,
      pub audio_sample_rate: u32,
      pub audio_channels: u16,
  }

  pub enum Track { Video, Audio }

  pub struct Sample {
      pub track: Track,
      pub bytes: Vec<u8>,
      pub source_ms: u64,
      pub duration_ms: u64,
  }

  pub enum ReadOutcome { Sample(Sample), EndOfStream }

  pub struct SourceReader { /* private */ }

  impl SourceReader {
      pub fn open_decoded(path: &Path) -> Result<SourceReader, ScreenError>;
      pub fn open_passthrough(path: &Path) -> Result<SourceReader, ScreenError>;
      pub fn describe(&self) -> Result<SourceDescription, ScreenError>;
      pub fn seek_ms(&mut self, source_ms: u64) -> Result<(), ScreenError>;
      pub fn read_next(&mut self) -> Result<ReadOutcome, ScreenError>;
      #[cfg(windows)]
      pub fn native_types(&self) -> Result<(IMFMediaType, Option<IMFMediaType>), ScreenError>;
  }
  ```

**Why `read_next` returns one interleaved `Sample` rather than a per-track reader:** `IMFSourceReader::ReadSample` with `MF_SOURCE_READER_ANY_STREAM` hands back whichever track's next sample comes first in decode order, which is exactly the order a sink writer wants. Two separate readers over one file would mean two demuxers, two seek positions, and a manual interleave — and getting the interleave wrong produces a file that plays but stutters, with nothing to catch it.

**Why seeking is on the reader and not the plan:** `SetCurrentPosition` on a compressed stream snaps BACKWARD to the previous keyframe; the reader then delivers samples from there. The exporter must therefore **discard** samples whose timestamp falls before the span it asked for, and Task 6 does exactly that. Do not try to make the reader hide it — a reader that silently swallowed pre-roll would have to decode to know what to swallow, which the passthrough mode cannot do.

**Three pure things live here and are tested on Linux:**

```rust
pub fn hns_to_ms(hns: i64) -> u64;
pub fn ms_to_hns(ms: u64) -> i64;
pub fn unreadable_source_message(path: &Path, scan: &crate::mp4_boxes::Scan) -> Option<String>;
```

`unreadable_source_message` is the `diagnose.rs` pattern applied to this module: the wording a user sees when a staged file cannot be exported is decided and tested where Linux can reach it, not composed inside a `cfg(windows)` arm that nothing can ever run.

- [ ] **Step 1: Write the failing tests in `src-tauri/screen/src/reader.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::mp4_boxes;

    fn bx(kind: &str, payload: &[u8]) -> Vec<u8> {
        let size = (8 + payload.len()) as u32;
        let mut out = size.to_be_bytes().to_vec();
        out.extend_from_slice(kind.as_bytes());
        out.extend_from_slice(payload);
        out
    }

    #[test]
    fn media_foundation_time_round_trips_through_milliseconds() {
        assert_eq!(hns_to_ms(0), 0);
        assert_eq!(hns_to_ms(10_000), 1);
        assert_eq!(hns_to_ms(10_000_000), 1_000);
        assert_eq!(ms_to_hns(1_000), 10_000_000);
        for ms in [0u64, 1, 999, 1_000, 123_456] {
            assert_eq!(hns_to_ms(ms_to_hns(ms)), ms);
        }
    }

    // MF hands back a signed time and a sample can legitimately carry a
    // negative one (a decoder's pre-roll). Clamping to 0 keeps it out of the
    // u64 arithmetic every restamp downstream does; wrapping would make one
    // frame land 584 million years into the output.
    #[test]
    fn a_negative_media_foundation_time_clamps_to_zero_rather_than_wrapping() {
        assert_eq!(hns_to_ms(-1), 0);
        assert_eq!(hns_to_ms(i64::MIN), 0);
    }

    #[test]
    fn a_healthy_fragmented_file_has_no_complaint() {
        let mut bytes = bx("ftyp", b"isom");
        bytes.extend(bx("moov", b"...."));
        bytes.extend(bx("moof", b"...."));
        bytes.extend(bx("mdat", b"...."));
        let scan = mp4_boxes::scan(&bytes);
        assert_eq!(unreadable_source_message(std::path::Path::new("a.mp4"), &scan), None);
    }

    #[test]
    fn a_file_with_no_moov_names_the_problem_and_the_file() {
        let bytes = bx("ftyp", b"isom");
        let scan = mp4_boxes::scan(&bytes);
        let msg = unreadable_source_message(std::path::Path::new("Demo.mp4"), &scan)
            .expect("a moov-less file must be refused");
        assert!(msg.contains("Demo.mp4"), "message names no file: {msg}");
        assert!(msg.to_lowercase().contains("incomplete"), "unexpected wording: {msg}");
    }

    // A capture killed mid-write leaves a truncated tail. That file is
    // PLAYABLE by design (spec 6.4) and must still export, so a truncated
    // scan is NOT a refusal -- it is a warning the caller may surface.
    #[test]
    fn a_truncated_file_is_not_refused_because_a_crashed_capture_still_holds_footage() {
        let mut bytes = bx("ftyp", b"isom");
        bytes.extend(bx("moov", b"...."));
        bytes.extend(bx("moof", b"...."));
        bytes.extend_from_slice(&[0, 0, 1]); // a partial box header
        let scan = mp4_boxes::scan(&bytes);
        assert!(matches!(scan.end, mp4_boxes::ScanEnd::Truncated { .. }));
        assert_eq!(unreadable_source_message(std::path::Path::new("a.mp4"), &scan), None);
    }

    #[cfg(not(windows))]
    #[test]
    fn the_non_windows_reader_reports_unsupported_rather_than_panicking() {
        let p = std::path::Path::new("unused.mp4");
        assert_eq!(SourceReader::open_decoded(p).unwrap_err(), ScreenError::Unsupported);
        assert_eq!(SourceReader::open_passthrough(p).unwrap_err(), ScreenError::Unsupported);
    }
}
```

- [ ] **Step 2: Run and watch them fail**

Add `pub mod reader;` to `lib.rs`, then:

```
cd src-tauri && cargo test -p vault_buddy_screen reader 2>&1 | tail -20
```

Expected: `cannot find function hns_to_ms` / `unreadable_source_message`, `cannot find type SourceReader`.

- [ ] **Step 3: Implement the pure half**

```rust
//! Reading a staged capture back, so it can be exported (spec 8.3 step 1).
//!
//! Two modes over ONE `IMFSourceReader`:
//!
//! * `open_decoded` sets NV12 and PCM16 output types, so Media Foundation
//!   inserts its own decoders and hands back raw frames the edited path can
//!   restamp and re-encode.
//! * `open_passthrough` sets no output type at all, so MF hands back the
//!   compressed H.264 and AAC samples untouched. Paired with
//!   `sink::StandardSink::create_passthrough` this is the fast path: a
//!   remux at disk speed with no quality loss.
//!
//! SEEKING IS LOSSY AND THAT IS EXPOSED, NOT HIDDEN.
//! `IMFSourceReader::SetCurrentPosition` snaps BACKWARD to the keyframe at
//! or before the requested time, so the first samples after a seek belong
//! to the span before the one that was asked for. The exporter drops them
//! by timestamp (see `export.rs`). A reader that swallowed them itself
//! would have to decode to know which to swallow, which the passthrough
//! mode cannot do -- so the caller owns the rule and one implementation of
//! it serves both modes.

use std::path::Path;

/// Media Foundation counts in 100-nanosecond units.
const HNS_PER_MS: i64 = 10_000;

/// Milliseconds from an MF timestamp. A NEGATIVE time clamps to zero: MF
/// can hand back a decoder's pre-roll with a negative stamp, and a `as u64`
/// on that would land the frame 584 million years into the output.
pub fn hns_to_ms(hns: i64) -> u64 {
    if hns <= 0 {
        return 0;
    }
    (hns / HNS_PER_MS) as u64
}

/// An MF timestamp from milliseconds. Saturates rather than wrapping.
pub fn ms_to_hns(ms: u64) -> i64 {
    i64::try_from(ms).unwrap_or(i64::MAX).saturating_mul(HNS_PER_MS)
}

/// Why this staged file cannot be exported, or `None` when it can be tried.
///
/// The `diagnose.rs` pattern: the wording a user actually reads is decided
/// and tested HERE, on Linux, rather than composed inside a `cfg(windows)`
/// arm that no test on any platform executes.
///
/// A TRUNCATED file is deliberately NOT refused. Spec 6.4's whole point is
/// that a capture killed mid-write still holds playable footage; refusing
/// to export it would throw away the recording the crash-safe container
/// exists to preserve.
pub fn unreadable_source_message(path: &Path, scan: &crate::mp4_boxes::Scan) -> Option<String> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    if !scan.has_moov() {
        return Some(format!(
            "{name} is incomplete and cannot be exported: it has no index. \
             The recording may not have finished writing."
        ));
    }
    None
}
```

- [ ] **Step 4: Implement the non-Windows arm and the shared types**

```rust
/// Which track a sample came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Track {
    Video,
    Audio,
}

/// One sample, with its position on the SOURCE timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub track: Track,
    pub bytes: Vec<u8>,
    pub source_ms: u64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    Sample(Sample),
    EndOfStream,
}

/// What the staged file turned out to contain. The exporter builds its
/// output format from THIS rather than from the sidecar, because the
/// sidecar is hand-editable and the file is the truth about its own pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceDescription {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub has_audio: bool,
    pub audio_sample_rate: u32,
    pub audio_channels: u16,
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    use crate::ScreenError;

    /// The Linux compile gate. Statically constructible but never usable —
    /// it must DEGRADE rather than panic, the contract every `cfg(windows)`
    /// surface in this crate carries.
    pub struct SourceReader;

    impl SourceReader {
        pub fn open_decoded(_path: &Path) -> Result<SourceReader, ScreenError> {
            Err(ScreenError::Unsupported)
        }
        pub fn open_passthrough(_path: &Path) -> Result<SourceReader, ScreenError> {
            Err(ScreenError::Unsupported)
        }
        pub fn describe(&self) -> Result<SourceDescription, ScreenError> {
            Err(ScreenError::Unsupported)
        }
        pub fn seek_ms(&mut self, _source_ms: u64) -> Result<(), ScreenError> {
            Err(ScreenError::Unsupported)
        }
        pub fn read_next(&mut self) -> Result<ReadOutcome, ScreenError> {
            Err(ScreenError::Unsupported)
        }
    }
}

pub use imp::SourceReader;
```

- [ ] **Step 5: Implement the Windows arm**

Write `#[cfg(windows)] mod imp` with the same public shape. The construction chain, copied from `fmp4_spike.rs:254-290` and extended:

1. `MfRuntime::start()` — this crate's `sink.rs` has one but it is **private to `sink::imp`**. Give `reader.rs` its own; `fmp4_spike.rs` already sets that precedent (it carries a second copy too). Do NOT make `sink`'s public just to share it — `MFStartup`/`MFShutdown` are refcounted, and two independent pairs is the documented-correct usage.
2. `MFCreateSourceReaderFromURL(&HSTRING::from(path…), None)`.
3. **Decoded mode only:** `SetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM, None, &nv12_type)` and `SetCurrentMediaType(MF_SOURCE_READER_FIRST_AUDIO_STREAM, None, &pcm_type)` where each type carries only major type + subtype, letting MF fill the rest. **Passthrough mode sets neither** — that absence IS the passthrough.
4. `SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS, true)`.
5. `describe()` reads `GetCurrentMediaType` for each stream: `MF_MT_FRAME_SIZE` unpacked (high 32 bits width, low 32 height — `diagnose::unpack_dims` already does exactly this unpack and is tested, so use it), `MF_MT_FRAME_RATE` (numerator over denominator, rounded), `MF_MT_AUDIO_SAMPLES_PER_SECOND`, `MF_MT_AUDIO_NUM_CHANNELS`; duration from `GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE, &MF_PD_DURATION)` through `hns_to_ms`. An absent audio stream sets `has_audio: false` and leaves the two audio fields at 0 — it must not be an error, because a zero-device capture is legal (spec §6.5).
6. `seek_ms` builds a `PROPVARIANT` holding `ms_to_hns(source_ms)` and calls `SetCurrentPosition(&GUID_NULL, &pv)`.
7. `read_next` calls `ReadSample(MF_SOURCE_READER_ANY_STREAM, 0, Some(&mut stream_index), Some(&mut flags), Some(&mut ts), Some(&mut sample))`; `MF_SOURCE_READERF_ENDOFSTREAM` → `ReadOutcome::EndOfStream`; a `None` sample with non-zero flags → recurse/loop (a stream tick); otherwise `ConvertToContiguousBuffer` → `Lock` → copy → `Unlock`, and `GetSampleDuration` for the duration. Map `stream_index` to `Track` by comparing against the indices `MF_SOURCE_READER_FIRST_VIDEO_STREAM` / `FIRST_AUDIO_STREAM` resolved at open time.
8. `native_types()` returns the current media types for both streams — what `StandardSink::create_passthrough` adopts.

Every failure maps through a local `reader_err(context, e) -> ScreenError::Sink(...)` mirroring `sink_err`, except a file that cannot be opened at all, which is `ScreenError::Io`.

- [ ] **Step 6: Run the tests and both clippy targets**

```
cd src-tauri && cargo test -p vault_buddy_screen reader 2>&1 | tail -5
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "exit=$?"
```

Expect several iterations on the Windows target. `PROPVARIANT` construction and the `ReadSample` out-parameter shapes are the two most likely to differ from the sketch in `windows` 0.62.2. **Record the real signatures in the commit body** so the next reader does not re-derive them.

- [ ] **Step 7: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `hns_to_ms`: drop the `hns <= 0` guard, use `(hns / HNS_PER_MS) as u64` | `a_negative_media_foundation_time_clamps_to_zero_rather_than_wrapping` |
| M2 | `hns_to_ms`: `HNS_PER_MS` = 1_000 | `media_foundation_time_round_trips_through_milliseconds` |
| M3 | `ms_to_hns`: `.saturating_mul` → `*` | not caught (no overflow fixture) — **add one** rather than accepting it: `assert_eq!(ms_to_hns(u64::MAX), i64::MAX)`. Re-run and confirm it now goes red. |
| M4 | `unreadable_source_message`: refuse a `Truncated` scan too | `a_truncated_file_is_not_refused_because_a_crashed_capture_still_holds_footage` |
| M5 | `unreadable_source_message`: return `None` unconditionally | `a_file_with_no_moov_names_the_problem_and_the_file` |
| M6 | `unreadable_source_message`: drop the file name from the message | the same test's `msg.contains("Demo.mp4")` |
| M7 | Non-Windows `open_passthrough`: `Ok(SourceReader)` | `the_non_windows_reader_reports_unsupported_rather_than_panicking` |
| M8 | Passthrough mode: also call `SetCurrentMediaType` | not caught on any platform — record it. This is the same untestable seam as Task 4's M5, and the two together are what the checklist row measures. |

- [ ] **Step 8: Gates and commit**

```
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo test -p vault_buddy_screen 2>&1 | tail -3
cd src-tauri && cargo machete .; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

```bash
git add src-tauri/screen/src/reader.rs src-tauri/screen/src/lib.rs
git commit -F /tmp/msg.txt
```

Message body: the two modes and what distinguishes them; that a backward seek's pre-roll is the CALLER's to drop and why a reader cannot hide it; that a truncated staged file still exports because that is what the fragmented container was chosen for; and the real MF signatures if they differed from the plan.

---

### Task 6: `screen::export` — the two paths, the cancel poll, the progress

This is where `select::plan` and `Timeline::is_untouched` finally get their production caller. The module is deliberately thin around a pure core: **`sample_action` is where "the exported file matches the preview the user approved" actually lives**, and it is a pure function of a span and a timestamp, tested on Linux.

**Files:**
- Create: `src-tauri/screen/src/export.rs`
- Modify: `src-tauri/screen/src/lib.rs` (`pub mod export;`, and one new `ScreenError` variant)

**Interfaces:**
- Consumes: `select::{plan, PlanSpan, plan_output_duration_ms, progress_percent}`; `reader::{SourceReader, Sample, Track, ReadOutcome, SourceDescription}`; `sink::{StandardSink, VideoFormat, AudioFormat}`; `vault_buddy_core::timeline::Timeline`; `vault_buddy_core::screen_capture_config::{ScreenQuality, bitrate_bps}`; `vault_buddy_core::throttle::EmitThrottle`.
- Produces, for Task 7:
  ```rust
  pub enum SampleAction { Skip, Write { output_ms: u64 }, SpanComplete }
  pub fn sample_action(span: &PlanSpan, source_ms: u64) -> SampleAction;
  pub fn export_refusal(timeline: &Timeline) -> Option<String>;

  pub struct ExportRequest<'a> {
      pub source: &'a Path,
      pub dest: &'a Path,
      pub timeline: &'a Timeline,
      pub quality: ScreenQuality,
  }
  pub struct ExportOutcome {
      pub output_duration_ms: u64,
      pub width: u32,
      pub height: u32,
      pub remuxed: bool,
  }
  pub fn export(
      req: ExportRequest<'_>,
      cancel: &AtomicBool,
      on_progress: &mut dyn FnMut(u64),
  ) -> Result<ExportOutcome, ScreenError>;
  ```

**THE SPEC IS WRONG ABOUT CANCELLATION AND THIS TASK CORRECTS IT.** Spec §8.3 step 5 says "cancellation is a polled atomic checked **per span**". For an *edited* export that is fine — spans are short. But the **fast path's plan is one span covering the whole recording**, so a per-span poll makes the one export a user is most likely to fire off unattended — "record an hour, glance at it, save" — completely uncancellable. Poll **per sample** instead. A `Relaxed` atomic load costs nothing beside the memcpy that accompanies every sample, and it makes Cancel mean the same thing on both paths. Record this deviation in the commit body and in Task 12's spec reconciliation note.

**`ScreenError` gains one variant.** `Cancelled`, a FIXED variant carrying no caller data — so it must be added to `lib.rs`'s `fixed_variants_render_a_constant_message_with_no_caller_data` test, which asserts exactly that property of every fixed variant. Display: `"the export was cancelled"`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/screen/src/export.rs` with the module doc and this test module only:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::plan;
    use vault_buddy_core::timeline::{Segment, Timeline};

    fn span(start: u64, end: u64, out: u64) -> PlanSpan {
        PlanSpan { source_start_ms: start, source_end_ms: end, output_start_ms: out }
    }

    // A backward seek lands on the keyframe BEFORE the span, so the reader
    // delivers samples that belong to footage the user cut. Writing them
    // would put deleted content back into the export -- the same class of
    // failure the null-timeline rule was removed to prevent, arriving by a
    // different door.
    #[test]
    fn a_sample_before_the_span_is_seek_preroll_and_is_skipped() {
        let s = span(4_000, 6_000, 1_000);
        assert!(matches!(sample_action(&s, 0), SampleAction::Skip));
        assert!(matches!(sample_action(&s, 3_999), SampleAction::Skip));
    }

    #[test]
    fn a_sample_inside_the_span_is_written_at_its_restamped_time() {
        let s = span(4_000, 6_000, 1_000);
        assert!(matches!(sample_action(&s, 4_000), SampleAction::Write { output_ms: 1_000 }));
        assert!(matches!(sample_action(&s, 5_999), SampleAction::Write { output_ms: 2_999 }));
    }

    // The span's own end is NOT in it (half-open, matching to_source_ms and
    // restamp). Reaching it means this span is finished, which is a
    // different instruction from "skip this sample": one advances the plan,
    // the other reads another sample from the same span. Conflating them
    // either drops a whole span or loops forever on one.
    #[test]
    fn the_spans_far_edge_completes_it_rather_than_skipping_the_sample() {
        let s = span(4_000, 6_000, 1_000);
        assert!(matches!(sample_action(&s, 6_000), SampleAction::SpanComplete));
        assert!(matches!(sample_action(&s, 9_999), SampleAction::SpanComplete));
    }

    #[test]
    fn the_three_actions_are_exhaustive_and_disjoint_across_a_source() {
        let s = span(4_000, 6_000, 1_000);
        let mut skips = 0;
        let mut writes = 0;
        let mut completes = 0;
        for source_ms in 0u64..8_000 {
            match sample_action(&s, source_ms) {
                SampleAction::Skip => skips += 1,
                SampleAction::Write { .. } => writes += 1,
                SampleAction::SpanComplete => completes += 1,
            }
        }
        assert_eq!(skips, 4_000);
        assert_eq!(writes, 2_000);
        assert_eq!(completes, 2_000);
    }

    // Walking a real reordered plan the way `export` does: every span's
    // output range must be contiguous with the previous one's, or the file
    // stutters (a gap) or the muxer rejects it (an overlap).
    #[test]
    fn walking_a_reordered_plan_produces_contiguous_output_times() {
        let t = Timeline::whole(9_000).split_at(3_000).split_at(6_000).reorder(0, 2);
        let spans = plan(&t);
        let mut written: Vec<u64> = Vec::new();
        for s in &spans {
            for source_ms in (s.source_start_ms..s.source_end_ms).step_by(1_000) {
                match sample_action(s, source_ms) {
                    SampleAction::Write { output_ms } => written.push(output_ms),
                    other => panic!("expected Write inside the span, got {other:?}"),
                }
            }
        }
        assert_eq!(written, vec![0, 1_000, 2_000, 3_000, 4_000, 5_000, 6_000, 7_000, 8_000]);
    }

    #[test]
    fn an_empty_timeline_is_refused_with_a_message_naming_what_to_do() {
        let msg = export_refusal(&Timeline::default()).expect("an empty timeline must be refused");
        assert!(msg.to_lowercase().contains("nothing"), "unexpected wording: {msg}");
    }

    #[test]
    fn a_timeline_with_footage_is_not_refused() {
        assert_eq!(export_refusal(&Timeline::whole(5_000)), None);
        let trimmed = Timeline::whole(9_000).split_at(3_000).delete(0);
        assert_eq!(export_refusal(&trimmed), None);
    }

    // A timeline whose only segments are zero-length holds no footage even
    // though it is not `is_empty()`. select::plan drops those spans, so the
    // export would produce a zero-byte file with no error -- refuse it
    // against the PLAN, not against the segment count.
    #[test]
    fn a_timeline_of_only_zero_length_segments_is_refused_too() {
        let t = Timeline { segments: vec![Segment { source_start_ms: 500, source_end_ms: 500 }] };
        assert!(!t.is_empty(), "the fixture must not trip is_empty as well");
        assert!(export_refusal(&t).is_some());
    }

    #[cfg(not(windows))]
    #[test]
    fn exporting_on_an_unsupported_platform_degrades_rather_than_panicking() {
        use std::sync::atomic::AtomicBool;
        let t = Timeline::whole(1_000);
        let cancel = AtomicBool::new(false);
        let mut seen: Vec<u64> = Vec::new();
        let err = export(
            ExportRequest {
                source: std::path::Path::new("in.mp4"),
                dest: std::path::Path::new("out.mp4"),
                timeline: &t,
                quality: ScreenQuality::Balanced,
            },
            &cancel,
            &mut |pct| seen.push(pct),
        )
        .unwrap_err();
        assert_eq!(err, ScreenError::Unsupported);
    }

    #[test]
    fn the_cancelled_error_renders_a_constant_message() {
        assert_eq!(ScreenError::Cancelled.to_string(), "the export was cancelled");
    }
}
```

`SampleAction` needs `#[derive(Debug)]` for the `{other:?}` in `walking_a_reordered_plan_…`.

- [ ] **Step 2: Run and watch them fail**

Add `pub mod export;` to `lib.rs`.

```
cd src-tauri && cargo test -p vault_buddy_screen export 2>&1 | tail -20
```

Expected: `cannot find function sample_action` / `export_refusal` / `export`, `no variant Cancelled`.

- [ ] **Step 3: Add the `Cancelled` variant**

In `src-tauri/screen/src/lib.rs`, add to `ScreenError` after `AlreadyCapturing`:

```rust
    /// The user cancelled an in-progress export. A distinct variant rather
    /// than an `Io`/`Sink` string because the caller must NOT treat it as a
    /// failure: spec 14 says a cancelled export keeps the staged capture and
    /// its timeline, and the editor stays open with no error banner.
    Cancelled,
```

Display arm: `ScreenError::Cancelled => write!(f, "the export was cancelled"),`

Add `ScreenError::Cancelled` to the list in `fixed_variants_render_a_constant_message_with_no_caller_data` — the test asserts every fixed variant renders a constant, and a variant missing from it silently weakens that invariant.

- [ ] **Step 4: Implement the pure core**

```rust
//! Exporting a staged capture to a finished MP4 (spec 8.3).
//!
//! TWO PATHS, one predicate.
//!
//! * `Timeline::is_untouched(source_duration_ms)` -> **remux**. The reader
//!   runs in passthrough mode and the sink adopts its media types, so the
//!   compressed samples are copied from demuxer to muxer with no decode and
//!   no re-encode. "Record, glance, save" never pays for a transcode.
//! * Anything else -> **re-encode**. `select::plan` gives the ordered spans
//!   and each one's output start; the reader seeks to each span in turn and
//!   `sample_action` decides, per sample, whether it is seek pre-roll to
//!   discard, footage to restamp and write, or the signal that the span is
//!   finished.
//!
//! NEVER key the fast path on the sidecar's timeline field being absent.
//! The editor writes a timeline on every edit and never writes null, so
//! "absent" means "never edited in this build", not "unedited" -- a
//! resumed edit would take the fast path and put the user's cut footage
//! back into the file, silently. `is_untouched` is the single authority.
//!
//! CANCELLATION IS POLLED PER SAMPLE, not per span as spec 8.3 says. The
//! fast path's plan is ONE span covering the whole recording, so a
//! per-span poll would make the longest, most unattended export the one
//! that cannot be cancelled at all. A Relaxed load per sample costs
//! nothing beside the memcpy that accompanies it.

use crate::select::{plan, plan_output_duration_ms, progress_percent, PlanSpan};
use crate::ScreenError;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use vault_buddy_core::screen_capture_config::ScreenQuality;
use vault_buddy_core::timeline::Timeline;

/// What to do with one sample the reader handed back while a span is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleAction {
    /// Before the span: `SetCurrentPosition` snapped back to the previous
    /// keyframe, so this is footage the user cut. Discard it and read on.
    Skip,
    /// Inside the span: write it at this OUTPUT timestamp.
    Write { output_ms: u64 },
    /// At or past the span's (half-open) end: advance to the next span.
    SpanComplete,
}

/// The single rule the edited export's correctness rests on.
///
/// Half-open at the far edge, matching `PlanSpan::restamp` and
/// `Timeline::to_source_ms`. `Skip` and `SpanComplete` are deliberately
/// different instructions even though both mean "do not write this sample":
/// one reads another sample from the SAME span, the other moves to the
/// next. Conflating them either loops forever on one span or drops a span
/// whole.
pub fn sample_action(span: &PlanSpan, source_ms: u64) -> SampleAction {
    if source_ms < span.source_start_ms {
        return SampleAction::Skip;
    }
    match span.restamp(source_ms) {
        Some(output_ms) => SampleAction::Write { output_ms },
        None => SampleAction::SpanComplete,
    }
}

/// Why this timeline cannot be exported, or `None` when it can.
///
/// Measured against the PLAN, not against `Timeline::is_empty`: a timeline
/// whose only segments are zero-length is not empty, but `select::plan`
/// drops every one of them, so the export would write a valid container
/// holding no footage and report success.
pub fn export_refusal(timeline: &Timeline) -> Option<String> {
    if plan(timeline).is_empty() {
        return Some(
            "There is nothing left to save — every part of this recording has been deleted. \
             Undo a delete, or discard the capture."
                .to_string(),
        );
    }
    None
}

/// What the exporter was asked to do.
pub struct ExportRequest<'a> {
    /// The staged `.mp4`.
    pub source: &'a Path,
    /// A temp inside the staging directory. NEVER a path in a vault — the
    /// vault write is the shell's, and it moves this file in only once the
    /// export has finished (spec 8.3's never-lose rule).
    pub dest: &'a Path,
    pub timeline: &'a Timeline,
    pub quality: ScreenQuality,
}

/// What the exported file turned out to be. `width`/`height` come from the
/// SOURCE FILE, not the sidecar: the sidecar is hand-editable and the file
/// is the truth about its own pixels, and these numbers go into the
/// companion note's `resolution` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOutcome {
    pub output_duration_ms: u64,
    pub width: u32,
    pub height: u32,
    /// True when the fast path ran. Surfaced so the caller can log which
    /// path a capture took without inferring it from a duration.
    pub remuxed: bool,
}
```

- [ ] **Step 5: Implement `export` — the platform split**

The non-Windows arm is one line, and it must come before the `cfg(windows)` body so the Linux test in Step 1 compiles:

```rust
#[cfg(not(windows))]
pub fn export(
    _req: ExportRequest<'_>,
    _cancel: &AtomicBool,
    _on_progress: &mut dyn FnMut(u64),
) -> Result<ExportOutcome, ScreenError> {
    Err(ScreenError::Unsupported)
}
```

The Windows arm:

```rust
#[cfg(windows)]
pub fn export(
    req: ExportRequest<'_>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64),
) -> Result<ExportOutcome, ScreenError> {
    use crate::reader::SourceReader;
    // The precondition check the wording lives in reader.rs for.
    let bytes = std::fs::read(req.source).map_err(|e| ScreenError::Io(e.to_string()))?;
    let scan = crate::mp4_boxes::scan(&bytes);
    if let Some(message) = crate::reader::unreadable_source_message(req.source, &scan) {
        return Err(ScreenError::Io(message));
    }
    drop(bytes);

    let probe = SourceReader::open_passthrough(req.source)?;
    let desc = probe.describe()?;
    drop(probe);

    if req.timeline.is_untouched(desc.duration_ms) {
        remux(req, desc, cancel, on_progress)
    } else {
        reencode(req, desc, cancel, on_progress)
    }
}
```

`remux` opens `SourceReader::open_passthrough`, takes `native_types()`, builds `StandardSink::create_passthrough`, and copies every sample through unchanged with its own timestamp — there is no restamping, because an untouched timeline IS the identity mapping. It polls `cancel` per sample and reports `progress_percent(sample.source_ms, desc.duration_ms)`.

`reencode` builds `VideoFormat { width: desc.width, height: desc.height, fps: desc.fps, bitrate_bps: bitrate_bps(req.quality, desc.width, desc.height, desc.fps) }` and, when `desc.has_audio`, an `AudioFormat` from the description; opens `SourceReader::open_decoded` and `StandardSink::create`; then for each `PlanSpan` in `plan(req.timeline)`:

```
reader.seek_ms(span.source_start_ms)?
loop {
    if cancel.load(Ordering::Relaxed) { return Err(ScreenError::Cancelled); }
    match reader.read_next()? {
        ReadOutcome::EndOfStream => break,
        ReadOutcome::Sample(s) => match sample_action(span, s.source_ms) {
            SampleAction::Skip => continue,
            SampleAction::SpanComplete => break,
            SampleAction::Write { output_ms } => {
                let ts = Duration::from_millis(output_ms);
                let dur = Duration::from_millis(s.duration_ms.max(1));
                match s.track {
                    Track::Video => sink.write_video(&s.bytes, ts, dur)?,
                    Track::Audio => sink.write_audio(&s.bytes, ts, dur)?,
                }
                written_output_ms = written_output_ms.max(output_ms);
                if throttle.should_emit(progress_percent(written_output_ms, total), false) {
                    on_progress(progress_percent(written_output_ms, total));
                }
            }
        },
    }
}
```

Three details a reviewer will look for, so get them right the first time:

- **`s.duration_ms.max(1)`.** Every MF input sample needs a valid time AND a **non-zero duration**: a zero duration makes `ProcessOutput` throw a divide-by-zero. This is a documented MF bug, it is already recorded in this increment's ledger as a landmine, and the clamp is what avoids it.
- **EVERY error path deletes `dest`.** `export` owns the temp it was told to write; on any `Err` — `Cancelled`, a sink failure, a reader failure — it removes `dest` before returning, best-effort with a `log::warn!`. A half-written temp left behind is staging litter, and worse, a later reader could mistake it for a finished export. Task 7 keeps the same rule for a failure that happens AFTER the export returns (a failed vault commit), and the reasoning is written up there.
- **Emit a terminal progress tick.** After the span loop, `on_progress(progress_percent(total, total))` — i.e. 100 — via `should_emit(.., true)`, so the UI never freezes at 98%.

`finalize()` the sink LAST, and return `ExportOutcome { output_duration_ms: total, width: desc.width, height: desc.height, remuxed: false }`.

- [ ] **Step 6: Run the tests and both clippy targets**

```
cd src-tauri && cargo test -p vault_buddy_screen export 2>&1 | tail -5
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "exit=$?"
```

- [ ] **Step 7: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `sample_action`: drop the `source_ms < span.source_start_ms` arm (fall through to `restamp`) | `a_sample_before_the_span_is_seek_preroll_and_is_skipped` — **and check WHICH way it fails**: `restamp` returns `None` for a pre-span sample, so the mutant reports `SpanComplete`, which silently drops the whole span. If this row stays green the three-way split is not being measured. |
| M2 | `sample_action`: swap `Skip` and `SpanComplete` | `a_sample_before_the_span…` AND `the_spans_far_edge_completes_it…` |
| M3 | `sample_action`: `Write { output_ms: source_ms }` (forget the restamp) | `a_sample_inside_the_span_is_written_at_its_restamped_time` AND `walking_a_reordered_plan_produces_contiguous_output_times` |
| M4 | `export_refusal`: test `timeline.is_empty()` instead of `plan(..).is_empty()` | `a_timeline_of_only_zero_length_segments_is_refused_too` |
| M5 | `export_refusal`: return `None` always | `an_empty_timeline_is_refused_with_a_message_naming_what_to_do` |
| M6 | `export`: `if !req.timeline.is_untouched(..)` (paths swapped) | not caught on Linux — record it. The observable symptom is an edited export that comes back uncut; Task 12 adds the checklist row. |
| M7 | `export`: key the fast path on the sidecar timeline being `None` instead | **not expressible here** — `export` takes a `&Timeline`, never an `Option`. That is deliberate: the trap is designed out of the signature rather than guarded against. Note it; Task 7's caller is where the `Option` is resolved, and its test covers it. |
| M8 | `reencode`: drop `.max(1)` from the sample duration | not caught — record it, and confirm the comment naming the MF divide-by-zero is present. |
| M9 | `ScreenError::Cancelled` display → `format!("cancelled: {}", ...)` with caller data | `fixed_variants_render_a_constant_message_with_no_caller_data` |

M7 is the row worth reading twice: the strongest guard in this task is a type, not a test.

- [ ] **Step 8: Gates and commit**

```
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd src-tauri && cargo test -p vault_buddy_screen 2>&1 | tail -3
cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94 2>&1 | tail -5; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

Coverage is the one at risk here: `export.rs` adds a large `cfg(windows)` body that no test reaches. It does not count on Linux (it is not compiled), so the floor should hold — **verify, do not assume**, and report the measured percentage.

```bash
git add src-tauri/screen/src/export.rs src-tauri/screen/src/lib.rs
git commit -F /tmp/msg.txt
```

Message body: the two paths and the one predicate; that cancellation polls per sample rather than per span, and that this deliberately departs from spec 8.3 because the fast path is one span; that a cancelled export deletes its own temp; and that `export` takes a `&Timeline` so the absent-means-untouched trap cannot be expressed.

---

### Task 7: The export command, the `screen-export` worker, and the ninth vault write

Everything before this task was machinery. This is the task that writes into a user's vault for the ninth time in the app's life.

**Files:**
- Create: `src-tauri/src/export_commands.rs`
- Create: `src-tauri/src/export_worker.rs`
- Modify: `src-tauri/src/lib.rs` (two `mod` lines, `.manage(ExportState::default())`, one `generate_handler!` entry)
- Modify: `src-tauri/src/editor_commands.rs` (make `is_safe_base` `pub(crate)`)

**Interfaces:**
- Consumes: `screen::export::{export, export_refusal, ExportRequest}`; `screen::staging::{staging_dir, read_sidecar, mp4_file_name as staged_mp4_file_name, sidecar_file_name}`; `screen::disk::free_bytes`; `core::screen_capture_paths::{commit_screen_capture, mp4_file_name}`; `core::screen_note::{render_screen_note, ScreenNoteMeta}`; `core::capture_note::write_note_collision_safe`; `core::capture_paths::{safe_recording_root, assert_path_inside_vault, capture_dir}`; `core::screen_capture_config::{export_size_estimate_bytes, export_space_shortfall}`; `core::throttle::EmitThrottle`; `editor_commands::is_safe_base`; `capture_guard::CaptureGuard`.
- Produces, for Task 8 and the frontend:
  ```rust
  pub struct ActiveExport { pub base: String, pub cancel: Arc<AtomicBool> }
  #[derive(Default)]
  pub struct ExportState(pub Mutex<Option<ActiveExport>>);
  pub(crate) fn clear_active_export(app: &AppHandle);

  #[tauri::command]
  pub async fn export_and_save_capture(app: AppHandle, base: String) -> Result<(), String>;
  ```

#### Five decisions this task makes, each of which a reviewer will question

1. **Export does NOT claim `CaptureGuard`; it READS it and refuses while either capture kind is live.** Claiming would trip the structural test that pins exactly one `release(CaptureKind::Screen)` in `screen_commands.rs` and zero in the worker, and would mean adding a second release site to the one thing that file exists to keep singular. Reading `CaptureGuard::active()` is a lock-take-decide-drop, so the "never held while another lock is taken" rule is untouched. Refusing is the right behaviour on the merits too: both an export and a live capture drive a hardware H.264 encoder, and on a machine with one, the export would steal throughput from the recording — and the recording is the irreplaceable artifact. The refusal names the running kind via `CaptureKind::busy_message()`.

2. **The staged capture is deleted only after the vault write has landed** (spec §8.3), and "landed" means the video committed AND the note either written or explicitly degraded to a warning. Until then the staged `.mp4` and its sidecar are untouched. This is the never-lose invariant; get it wrong and a failed note write costs the user their recording.

3. **Video first, note second, and a note failure is a WARNING.** Exactly `capture::session::finalize`'s order and posture. The video is irreplaceable; the note is regenerable prose. A note-write failure appends to the outcome's warning rather than failing the export or rolling the video back.

4. **The note's `duration` is the OUTPUT duration, not the source's.** A capture trimmed from 10 minutes to 2 must not carry `duration: "10:00"`. `ExportOutcome::output_duration_ms` is what `render_screen_note` gets. Likewise `resolution` comes from `ExportOutcome`'s width/height (the file's own pixels), not from the sidecar's hand-editable fields.

5. **Every failure deletes the export temp and keeps the staged capture — a deliberate departure from spec §14.** §14 says a failed vault write keeps "the exported temp … with retry". Do not do that. A kept temp is a promise this codebase cannot keep: Task 9's sweep deletes a stale `.export.mp4.part`, so the temp survives an immediate retry and silently vanishes before a next-session one, and the user gets two different behaviours from the same button. Delete it on every failure path instead. What a retry actually needs is the **staged capture**, which is untouched until step 12 — and re-exporting from it is the honest cost of a failed save. Record the deviation in the commit body, in Task 12's spec reconciliation note, and as a gaps entry.

6. **The sidecar's `timeline` is PARSED into `core::timeline::Timeline` here, defensively.** Today `load_staged_capture` returns it as an opaque `serde_json::Value` it never inspects (docs/Gaps.md GAP-134: a `{}` timeline throws a raw `TypeError` out of the frontend). The export cannot pass an unvalidated value to `select::plan`, so this is where it gets parsed — and a malformed or absent one degrades to `Timeline::whole(sidecar.duration_ms)`, the same defensive-read posture as the rest of the vault domain. **That default is safe precisely because `is_untouched` is the authority**: a whole-capture timeline answers it exactly as an absent field would, so the fallback takes the fast path rather than silently re-encoding.

- [ ] **Step 1: Write the failing tests in `export_commands.rs`**

The shell crate's tests run on Linux and cannot drive Media Foundation, so they pin the parts that are decidable without it: the structural invariants, the sidecar parse, and the refusal ordering.

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn production_src() -> &'static str {
        include_str!("export_commands.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    fn worker_src() -> &'static str {
        include_str!("export_worker.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("the production prefix")
    }

    // Export must not claim the cross-domain capture guard: the one
    // `release(CaptureKind::Screen)` in the whole shell lives in
    // screen_commands::clear_active_screen and a structural test there pins
    // it at exactly one. A claim here would need a second release site.
    #[test]
    fn export_reads_the_capture_guard_but_never_claims_or_releases_it() {
        for src in [production_src(), worker_src()] {
            assert!(!src.contains("try_claim("), "export must not claim CaptureGuard");
            assert!(!src.contains(".release("), "export must not release CaptureGuard");
        }
        assert!(
            production_src().contains("CaptureGuard>().active()"),
            "export must refuse while a capture of either kind is live"
        );
    }

    // THE never-lose invariant, structurally: the staged capture's removal
    // must sit AFTER the vault commit in the worker's source. A reviewer
    // cannot eyeball a 200-line function for this, and getting it backwards
    // costs the user a recording.
    #[test]
    fn the_staged_capture_is_removed_only_after_the_vault_commit() {
        let src = worker_src();
        let commit = src
            .find("commit_screen_capture(")
            .expect("the vault commit is present");
        let remove = src
            .find("remove_staged_capture(")
            .expect("the staged removal is present");
        assert!(
            commit < remove,
            "the staged capture is removed at byte {remove}, before the vault commit at {commit}"
        );
        assert_eq!(
            src.matches("remove_staged_capture(").count(),
            1,
            "the staged capture must be removed from exactly one place"
        );
    }

    // Video first, note second -- capture::session::finalize's order. A note
    // written first and a video commit that then failed would leave a note
    // in the vault embedding a file that is not there.
    #[test]
    fn the_video_commits_before_the_note_is_written() {
        let src = worker_src();
        let video = src.find("commit_screen_capture(").expect("video commit");
        let note = src
            .find("write_note_collision_safe(")
            .expect("note write");
        assert!(video < note, "the note is written before the video commits");
    }

    // The dated directory is asserted inside the vault BEFORE create_dir_all
    // (so a pre-existing symlink is not followed) and AFTER it (closing the
    // swap-in race) -- the document-import discipline, which is stronger
    // than the audio path's post-only check.
    #[test]
    fn the_export_directory_is_asserted_inside_the_vault_before_and_after_creation() {
        let src = worker_src();
        let create = src.find("create_dir_all(").expect("create_dir_all");
        let asserts: Vec<usize> = src.match_indices("assert_path_inside_vault(").map(|(i, _)| i).collect();
        assert!(
            asserts.iter().any(|&i| i < create),
            "no containment assertion before create_dir_all"
        );
        assert!(
            asserts.iter().any(|&i| i > create),
            "no containment assertion after create_dir_all"
        );
    }

    #[test]
    fn a_malformed_sidecar_timeline_degrades_to_the_whole_capture() {
        // An absent timeline is an unedited capture.
        assert_eq!(timeline_from_sidecar(None, 5_000), Timeline::whole(5_000));
        // So is a null, an empty object, a wrong-typed field, and a segment
        // list with a non-numeric bound -- every one is a hand-edit or a
        // version skew, and none of them may become a partial export.
        for bad in [
            serde_json::json!(null),
            serde_json::json!({}),
            serde_json::json!({"segments": "nope"}),
            serde_json::json!({"segments": [{"sourceStartMs": "x", "sourceEndMs": 3}]}),
            serde_json::json!([]),
        ] {
            assert_eq!(
                timeline_from_sidecar(Some(bad.clone()), 5_000),
                Timeline::whole(5_000),
                "not defaulted: {bad}"
            );
        }
    }

    #[test]
    fn a_well_formed_sidecar_timeline_is_honoured_exactly() {
        let value = serde_json::json!({
            "segments": [
                {"sourceStartMs": 4_000, "sourceEndMs": 9_000},
                {"sourceStartMs": 0, "sourceEndMs": 1_000}
            ]
        });
        let t = timeline_from_sidecar(Some(value), 9_000);
        assert_eq!(t.segments.len(), 2);
        assert_eq!(t.segments[0].source_start_ms, 4_000);
        assert_eq!(t.segments[1].source_end_ms, 1_000);
        // And it must NOT read as untouched -- this is the edited path.
        assert!(!t.is_untouched(9_000));
    }

    // An empty segment list is NOT the same as an absent field: the user
    // deleted everything, and Task 6's refusal must see it rather than have
    // the whole capture silently restored underneath them.
    #[test]
    fn an_explicitly_empty_segment_list_is_kept_empty_not_restored() {
        let t = timeline_from_sidecar(Some(serde_json::json!({"segments": []})), 5_000);
        assert!(t.is_empty(), "an explicit empty edit was replaced by the whole capture");
    }
}
```

- [ ] **Step 2: Run and watch them fail**

```
cd src-tauri && cargo test -p vault-buddy --lib export 2>&1 | tail -20
```

Expected: the module does not exist.

- [ ] **Step 3: Implement `timeline_from_sidecar` and the state**

In `export_commands.rs`:

```rust
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use vault_buddy_core::timeline::{Segment, Timeline};

/// The in-flight export. One at a time, process-wide.
pub struct ActiveExport {
    pub base: String,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct ExportState(pub Mutex<Option<ActiveExport>>);

/// THE chokepoint: drop the reservation. One place, so no path can finish
/// an export without freeing the slot -- the `clear_active_screen`
/// discipline applied to a much smaller piece of state.
pub(crate) fn clear_active_export(app: &AppHandle) {
    let state = app.state::<ExportState>();
    *crate::lock_ignoring_poison(&state.0) = None;
}

/// Turn the sidecar's hand-editable `timeline` field into a real timeline.
///
/// This is the ONLY place that value is interpreted (docs/Gaps.md GAP-134:
/// `load_staged_capture` passes it through as an opaque `serde_json::Value`
/// it never inspects). Anything malformed -- absent, null, wrong-typed, a
/// segment with a non-numeric bound -- degrades to the WHOLE capture, the
/// same defensive-read posture as the rest of the vault domain.
///
/// That default is safe only because `Timeline::is_untouched` is the
/// authority on the fast path: a whole-capture timeline answers it exactly
/// as an absent field would, so a degraded read remuxes rather than
/// re-encoding. An EXPLICITLY empty segment list is NOT degraded -- the
/// user deleted everything, and `export_refusal` must see that rather than
/// have their recording silently restored underneath them.
pub(crate) fn timeline_from_sidecar(
    value: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> Timeline {
    let whole = || Timeline::whole(source_duration_ms);
    let Some(value) = value else { return whole() };
    let Some(segments) = value.get("segments").and_then(|s| s.as_array()) else {
        return whole();
    };
    let mut parsed = Vec::with_capacity(segments.len());
    for seg in segments {
        let (Some(start), Some(end)) = (
            seg.get("sourceStartMs").and_then(serde_json::Value::as_u64),
            seg.get("sourceEndMs").and_then(serde_json::Value::as_u64),
        ) else {
            return whole();
        };
        parsed.push(Segment {
            source_start_ms: start,
            source_end_ms: end,
        });
    }
    Timeline { segments: parsed }
}
```

- [ ] **Step 4: Implement the command**

```rust
/// Export the staged capture `base` and save it into its vault.
///
/// Async: it reads the sidecar, probes the disk, runs a full re-encode on a
/// worker thread and waits for it. None of that may sit on the main thread.
#[tauri::command]
pub async fn export_and_save_capture(app: AppHandle, base: String) -> Result<(), String> {
    if !crate::editor_commands::is_safe_base(&base) {
        return Err("That capture name is not one Vault Buddy can open.".into());
    }
    // Read, never claim. See this module's doc: an export and a live
    // capture both drive the H.264 encoder, and the recording is the
    // irreplaceable one.
    if let Some(kind) = app.state::<CaptureGuard>().active() {
        return Err(kind.busy_message());
    }
    {
        let state = app.state::<ExportState>();
        let mut guard = crate::lock_ignoring_poison(&state.0);
        if let Some(active) = guard.as_ref() {
            return Err(format!("An export of {} is already running.", active.base));
        }
        *guard = Some(ActiveExport {
            base: base.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
        });
    }
    // A NAMED thread, not `spawn_blocking`. Spec 8.3 says "on a named
    // `screen-export` worker" and the diagnostics invariant says every
    // spawned thread is named, because a crash record must identify the
    // dying thread -- and this is the longest-running, most COM-heavy piece
    // of work the app does, so it is exactly the thread a crash is most
    // likely to be in. `spawn_blocking` would put it on a tokio pool thread
    // carrying tokio's name.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let app2 = app.clone();
    let spawned = std::thread::Builder::new()
        .name("screen-export".into())
        .spawn(move || {
            let outcome = crate::export_worker::export_blocking(&app2, base);
            // A dropped receiver is not a reason to panic across the FFI
            // boundary; the caller has already gone away.
            let _ = done_tx.send(outcome);
        });
    if let Err(e) = spawned {
        clear_active_export(&app);
        return Err(format!("The export could not be started: {e}"));
    }
    // `recv` blocks, so it runs on the async runtime's blocking side rather
    // than the main thread -- this command is async for exactly that reason.
    // Unbounded on purpose: an export of a long recording can legitimately
    // take many minutes, and a deadline here would abandon a worker that is
    // still writing into the user's vault.
    let result = tauri::async_runtime::spawn_blocking(move || done_rx.recv())
        .await
        .map_err(|e| format!("The export could not be awaited: {e}"))?;
    clear_active_export(&app);
    match result {
        Ok(inner) => inner,
        Err(_) => Err("The export stopped unexpectedly.".into()),
    }
}
```

- [ ] **Step 5: Implement `export_worker::export_blocking`**

In order, with each step's reason in a comment:

```
1.  staging = staging::staging_dir(&app.path().app_local_data_dir()?)
2.  sidecar = staging::read_sidecar(&staging.join(staging::sidecar_file_name(&base)))
        .ok_or("That capture's details could not be read…")?
    if sidecar.base != base { return Err(…) }      // the load path's own mismatch refusal
3.  vault = crate::commands::find_vault(&sidecar.vault_id)
        .map_err(|_| format!("The vault this capture was recorded for is no longer \
                              in Obsidian. Open it in Obsidian and try again."))?
    let vault_path = PathBuf::from(&vault.path);
    if !vault_path.is_dir() { return Err("Vault folder not found — was it moved or deleted?") }
4.  cfg = capture_config::vault_config(&capture_config::load_config(), &sidecar.vault_id)
5.  root = capture_paths::safe_recording_root(&vault_path, cfg.screen_capture_root())?
    capture_paths::assert_path_inside_vault(&vault_path, &root)?
6.  date  = the sidecar's recorded_at date, else today
    dir   = capture_paths::capture_dir(&root, date, cfg.screen_capture_date_folders)
    capture_paths::assert_path_inside_vault(&vault_path, &dir)?    // PRE
    std::fs::create_dir_all(&dir)…
    capture_paths::assert_path_inside_vault(&vault_path, &dir)?    // POST
7.  timeline = timeline_from_sidecar(sidecar.timeline.clone(), sidecar.duration_ms)
    if let Some(msg) = screen::export::export_refusal(&timeline) { return Err(msg) }
8.  needed = screen_capture_config::export_size_estimate_bytes(
        timeline.output_duration_ms(), sidecar.width, sidecar.height,
        cfg.screen_fps, cfg.screen_quality)
    if let Some(short) = export_space_shortfall(needed, screen::disk::free_bytes(&dir)) {
        return Err(format!("There is not enough free space to save this capture — \
                            about {} more is needed.", human_bytes(short)))
    }
9.  temp = staging.join(format!(".{base}.export.mp4.part"))
    let cancel = the Arc cloned out of ExportState under its lock
    let mut throttle = EmitThrottle::new(2);
    outcome = screen::export::export(ExportRequest { source: &staged_mp4, dest: &temp,
                  timeline: &timeline, quality: cfg.screen_quality },
                  &cancel,
                  &mut |pct| emit_export_progress(app, &base, pct))?
10. (mp4, note_path) = core::screen_capture_paths::commit_screen_capture(&temp, &dir, &base)?
11. if cfg.screen_create_note { … render_screen_note … write_note_collision_safe … }
       a failure here is a WARNING, never a rollback
12. remove_staged_capture(&staging, &base)      // ONLY NOW
13. emit screen:exported
```

The note's meta:

```rust
let meta = ScreenNoteMeta {
    recorded_at: sidecar.recorded_at.clone(),
    // The OUTPUT duration. A capture trimmed from 10 minutes to 2 must not
    // claim ten in its frontmatter.
    duration_secs: outcome.output_duration_ms / 1_000,
    vault_name: vault.name.clone(),
    source: sidecar.source_title.clone(),
    input_devices: sidecar.inputs.clone(),
    // The FILE's own pixels, from the reader -- the sidecar's copies are
    // hand-editable.
    width: outcome.width,
    height: outcome.height,
    extra_frontmatter: cfg.screen_extra_frontmatter.clone(),
    body_template: cfg.screen_body_template.clone(),
};
let file_name = mp4.file_name().unwrap_or_default().to_string_lossy();
let content = render_screen_note(&meta, &file_name);
match write_note_collision_safe(&note_path, &content) { … }
```

and `remove_staged_capture`:

```rust
/// Delete the staged `.mp4` and its sidecar, AFTER the vault write landed.
///
/// The one removal site (a structural test pins that). Both failures are
/// warnings: the user's capture is safely in their vault by this point, and
/// a leftover staged file is litter rather than loss.
fn remove_staged_capture(staging: &Path, base: &str) {
    for path in [
        staging.join(vault_buddy_screen::staging::mp4_file_name(base)),
        staging.join(vault_buddy_screen::staging::sidecar_file_name(base)),
    ] {
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!("screen export: could not remove {} after saving: {e}", path.display());
        }
    }
}
```

- [ ] **Step 6: Wire it up**

In `src-tauri/src/lib.rs`: `mod export_commands;` and `mod export_worker;` (alphabetical among the existing `mod` lines), `.manage(export_commands::ExportState::default())` beside the other `.manage` calls, and `export_commands::export_and_save_capture,` in `generate_handler!`.

In `src-tauri/src/editor_commands.rs`, change `fn is_safe_base` to `pub(crate) fn is_safe_base`. **Do not copy it** — its rules (no separator, no leading/trailing dot, no trailing space, no `:`, no control character, no Windows reserved device stem, and an interior `..` deliberately ALLOWED) are the product of two Windows-only path escapes found by review, and a second copy would drift.

**Check the structural guard in `editor_commands.rs` still holds.** `both_commands_guard_the_base_with_is_safe_base` scans that file generically over every `#[tauri::command]` and asserts the SET of base-taking commands it finds. Making the function `pub(crate)` does not change that set, but re-run `cargo test -p vault-buddy --lib` and confirm rather than assuming — this exact test has already been broken once by an unrelated append.

- [ ] **Step 7: Run everything**

```
cd src-tauri && cargo test -p vault-buddy --lib 2>&1 | tail -5
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

- [ ] **Step 8: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | Move `remove_staged_capture(&staging, &base)` above `commit_screen_capture` | `the_staged_capture_is_removed_only_after_the_vault_commit` |
| M2 | Add a second `remove_staged_capture(..)` call in an error arm | the same test's `count() == 1` |
| M3 | Write the note before committing the video | `the_video_commits_before_the_note_is_written` |
| M4 | Delete the PRE-`create_dir_all` `assert_path_inside_vault` | `the_export_directory_is_asserted_inside_the_vault_before_and_after_creation` |
| M5 | Delete the POST one instead | the same test, the other half — **run both separately**; a single mutation that trips both halves would prove neither |
| M6 | Replace `active()` with `try_claim(CaptureKind::Screen)` | `export_reads_the_capture_guard_but_never_claims_or_releases_it` |
| M7 | `timeline_from_sidecar`: return the parsed timeline for a non-array `segments` instead of `whole()` | `a_malformed_sidecar_timeline_degrades_to_the_whole_capture` |
| M8 | `timeline_from_sidecar`: treat an empty `segments` array as `whole()` | `an_explicitly_empty_segment_list_is_kept_empty_not_restored` |
| M9 | `timeline_from_sidecar`: `as_u64` → `as_i64().unwrap_or(0) as u64` | `a_malformed_sidecar_timeline…` (the `"x"` row) — **check it actually goes red**; if `as_i64` also rejects a string, this mutation is equivalent and should be replaced with one that accepts a float |
| M10 | Note meta: `duration_secs: sidecar.duration_ms / 1_000` (source, not output) | **not caught** — the shell tests cannot run an export. Record it and add the checklist row: save a trimmed capture and read the note's `duration`. |
| M11 | Note meta: width/height from the sidecar instead of the outcome | not caught — same row. |

M5 and M10 are the two to be honest about in the report. M5 is a real test-design trap: one mutation that reddens a test proves the test is alive, not that both halves are.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/export_commands.rs src-tauri/src/export_worker.rs src-tauri/src/lib.rs src-tauri/src/editor_commands.rs
git commit -F /tmp/msg.txt
```

Message body: this is the ninth sanctioned vault write; the staged capture is deleted only after the vault write lands and a structural test pins the ordering; video first and note second with a note failure degrading to a warning; why export reads `CaptureGuard` rather than claiming it; that the note records the OUTPUT duration; and that the sidecar's timeline is parsed here for the first time, defensively, with the degrade-to-whole default safe because `is_untouched` is the authority.

---

### Task 8: The other four commands, and the five export events

**Files:**
- Modify: `src-tauri/src/export_commands.rs`
- Modify: `src-tauri/src/lib.rs` (four more `generate_handler!` entries)

**Interfaces — produces, for Tasks 10–11:**
```rust
#[tauri::command] pub fn cancel_export(app: AppHandle) -> Result<(), String>;
#[tauri::command] pub async fn discard_staged_capture(app: AppHandle, base: String) -> Result<(), String>;
#[tauri::command] pub async fn list_staged_captures(app: AppHandle) -> Vec<StagedCaptureSummaryDto>;
#[tauri::command] pub fn open_screen_capture(app: AppHandle, id: String, path: String) -> Result<(), String>;

#[derive(Serialize)] #[serde(rename_all = "camelCase")]
pub struct StagedCaptureSummaryDto {
    pub base: String,
    pub vault_id: String,
    pub source_title: String,
    pub duration_ms: u64,
    pub output_duration_ms: u64,
    pub recorded_at: String,
    pub width: u32,
    pub height: u32,
    pub edited: bool,
}
```

**Events (all `app.emit`, i.e. app-wide — none of these is a single-window latch like `region:begin` or `editor:open`):**

| Event | Payload | Listened to by |
| --- | --- | --- |
| `screen:exportProgress` | `{ base, fraction }` | EditorRoot |
| `screen:exported` | `{ base, videoPath, notePath, vaultId, warning }` | EditorRoot; `screenCapture` store (clears `lastStaged` when the base matches) |
| `screen:exportFailed` | `{ base, message }` | EditorRoot |
| `screen:exportCancelled` | `{ base }` | EditorRoot |
| `screen:discarded` | `{ base }` | `screenCapture` store (same clear); `ScreenSourcePicker` (refresh) |

**`screen:discarded` is not in the spec and is added deliberately.** Without it, discarding a capture leaves `lastStaged` pointing at a base that no longer exists on disk, so the panel keeps offering **Edit** on it and `open_capture_editor` → `load_staged_capture` fails with a banner the user cannot act on. `lastStaged` is set in exactly one place and cleared in exactly one place today; this adds a second clear, keyed on the base, and the store's existing `seq` staleness guard is untouched because this is not a lifecycle transition.

**Sync vs async, per the documented rule** (sync only where a window API is touched or the work is O(1) and lock-free):
- `cancel_export` — **sync.** It takes the `ExportState` mutex, sets one `AtomicBool`, and drops it. No I/O.
- `open_screen_capture` — **sync**, exactly like `open_recording`/`open_task`: a `uri::launch` handoff, read-only, logged.
- `discard_staged_capture` and `list_staged_captures` — **async.** Both do filesystem work.

- [ ] **Step 1: Write the failing tests**

```rust
    #[test]
    fn a_staged_capture_is_summarised_as_edited_only_when_its_timeline_differs_from_the_whole() {
        // The SAME predicate the export fast path uses, surfaced to the UI.
        // Deriving "edited" from the field's presence would mark every
        // capture the editor has ever been opened on as edited, because the
        // editor writes a timeline on every operation and never writes null.
        assert!(!summary_is_edited(None, 5_000));
        assert!(!summary_is_edited(
            Some(serde_json::json!({"segments": [{"sourceStartMs": 0, "sourceEndMs": 5_000}]})),
            5_000
        ));
        assert!(summary_is_edited(
            Some(serde_json::json!({"segments": [{"sourceStartMs": 1_000, "sourceEndMs": 5_000}]})),
            5_000
        ));
    }

    #[test]
    fn the_summary_reports_the_output_duration_the_export_will_produce() {
        let trimmed = serde_json::json!({"segments": [{"sourceStartMs": 1_000, "sourceEndMs": 4_000}]});
        assert_eq!(summary_output_duration_ms(Some(trimmed), 9_000), 3_000);
        assert_eq!(summary_output_duration_ms(None, 9_000), 9_000);
    }

    // Discard is the phase's DESTRUCTIVE path. It must refuse a base that
    // does not name a file directly in the staging directory, by the same
    // rule every other base-taking command uses -- and it must refuse while
    // that base is being exported, or it deletes the file out from under a
    // running re-encode.
    #[test]
    fn discard_refuses_an_unsafe_base() {
        for bad in ["../obsidian", "a/b", ".hidden", "trailing.", "C:Windows", "COM1", "x "] {
            assert!(
                !crate::editor_commands::is_safe_base(bad),
                "{bad:?} must be refused before it becomes a path"
            );
        }
    }

    #[test]
    fn every_new_command_is_registered_and_the_base_takers_are_guarded() {
        let src = production_src();
        for name in [
            "export_and_save_capture",
            "cancel_export",
            "discard_staged_capture",
            "list_staged_captures",
            "open_screen_capture",
        ] {
            assert!(src.contains(&format!("pub fn {name}")) || src.contains(&format!("pub async fn {name}")),
                "{name} is missing");
        }
        // Generic scan: every command in this file that takes a `base`
        // must reject an unsafe one in its own body, bounded by that
        // body's own closing brace -- an open-ended slice is the latent
        // form of the bug that let one command's guard satisfy another's
        // assertion in phase 4.
        let mut checked = 0usize;
        for (start, _) in src.match_indices("#[tauri::command]") {
            let body_start = src[start..].find('{').map(|i| start + i).expect("a body");
            let body_end = src[body_start..]
                .find("\n}")
                .map(|i| body_start + i)
                .unwrap_or(src.len());
            let body = &src[body_start..body_end];
            let head = &src[start..body_start];
            if head.contains("base: String") {
                assert!(
                    body.contains("is_safe_base(&base)"),
                    "a base-taking command does not guard its base: {head}"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 2, "expected export_and_save_capture and discard_staged_capture");
    }

    #[test]
    fn discard_refuses_while_that_capture_is_being_exported() {
        assert!(discard_conflict(Some("2026-09-20 1432 Demo"), "2026-09-20 1432 Demo").is_some());
        assert!(discard_conflict(Some("2026-09-20 1432 Other"), "2026-09-20 1432 Demo").is_none());
        assert!(discard_conflict(None, "2026-09-20 1432 Demo").is_none());
    }
```

- [ ] **Step 2: Run and watch them fail**

```
cd src-tauri && cargo test -p vault-buddy --lib export 2>&1 | tail -20
```

- [ ] **Step 3: Implement the pure helpers**

```rust
/// Does this staged capture carry an edit? The SAME predicate the export
/// fast path uses, surfaced so the resume-or-discard list can say so.
///
/// Deriving this from "the sidecar has a timeline field" would mark every
/// capture the editor has ever been OPENED on as edited: the editor writes
/// a timeline on every operation and never writes null.
pub(crate) fn summary_is_edited(
    timeline: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> bool {
    !timeline_from_sidecar(timeline, source_duration_ms).is_untouched(source_duration_ms)
}

/// How long the exported file will be, so the list can show it beside the
/// recording's own length.
pub(crate) fn summary_output_duration_ms(
    timeline: Option<serde_json::Value>,
    source_duration_ms: u64,
) -> u64 {
    timeline_from_sidecar(timeline, source_duration_ms).output_duration_ms()
}

/// Why this discard must be refused, or `None`.
///
/// Deleting the `.mp4` out from under a running export would leave the
/// re-encode reading a handle to an unlinked file on Windows and produce a
/// truncated export with no error.
pub(crate) fn discard_conflict(exporting: Option<&str>, base: &str) -> Option<String> {
    match exporting {
        Some(active) if active == base => Some(format!(
            "{base} is being saved right now. Cancel the save first, or wait for it to finish."
        )),
        _ => None,
    }
}
```

- [ ] **Step 4: Implement the four commands**

`cancel_export`: take the `ExportState` lock, `active.cancel.store(true, Ordering::Relaxed)`, drop. Return `Ok(())` even when nothing is running — a Cancel click racing the export's own completion is not an error the user should see.

`discard_staged_capture`: guard the base; read `ExportState` for the active base and apply `discard_conflict`; then `spawn_blocking` to remove the `.mp4` and the `.json` from the staging dir (`NotFound` counts as success — "the path is clear", the `delete_transcription_model` precedent); emit `screen:discarded { base }`. **Do not** follow a symlink: check `symlink_metadata` and refuse a leaf that is one, the `delete_task` no-follow discipline.

`list_staged_captures`: `spawn_blocking` a read of the staging dir; for each `*.json` whose stem is a safe base, `read_sidecar`, skip any whose own `base` disagrees with the file name, skip any whose `.mp4` is absent, and build a summary. Sort newest-first by `recorded_at` descending, then by base, so the order is deterministic. Degrade to an empty list on any directory error — this feeds a UI list, not a guard.

`open_screen_capture`: mirror `open_recording` exactly — `find_vault`, canonicalise, require containment inside the vault, compute the vault-relative path against the CANONICAL vault path, `uri::launch(uri::open_file_uri(..))`.

- [ ] **Step 5: Implement the five emitters**

One function per event in `export_commands.rs`, each `log::warn!`-ing a failed emit rather than `let _ =`:

```rust
pub(crate) fn emit_export_progress(app: &AppHandle, base: &str, percent: u64) {
    if let Err(e) = app.emit(
        "screen:exportProgress",
        serde_json::json!({ "base": base, "fraction": percent as f64 / 100.0 }),
    ) {
        log::warn!("screen export: could not emit progress: {e}");
    }
}
```

The fraction is `percent / 100.0` and nothing else, because `select::progress_fraction` defines it that way and the throttle gated on the percent — one number, two shapes.

- [ ] **Step 6: Register and run**

Add all four to `generate_handler!`. The IPC surface goes from **85 to 90**; AGENTS.md's table says to **COUNT** the list rather than add to the previous number, and that sentence exists because the count has been wrong three times. Measure it:

```
awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -cE '^\s+[a-z_]+::[a-z_]+,$'
```

```
cd src-tauri && cargo test -p vault-buddy --lib 2>&1 | tail -5
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

- [ ] **Step 7: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `summary_is_edited`: return `timeline.is_some()` | `a_staged_capture_is_summarised_as_edited_only_when…` (the whole-timeline row) |
| M2 | `summary_output_duration_ms`: return `source_duration_ms` always | `the_summary_reports_the_output_duration…` |
| M3 | `discard_conflict`: always `None` | `discard_refuses_while_that_capture_is_being_exported` |
| M4 | `discard_conflict`: refuse whenever ANY export is running | the same test's `"Other"` row |
| M5 | Remove `is_safe_base` from `discard_staged_capture` | `every_new_command_is_registered_and_the_base_takers_are_guarded` |
| M6 | Rename `discard_staged_capture`'s parameter to `name: String` | the same test's `assert_eq!(checked, 2)` — this is the evasion that beat an earlier version of this scan; confirm it goes red |
| M7 | Emit `fraction: percent as f64` (0..100 instead of 0..1) | not caught by a Rust test — the frontend test in Task 10 covers it. Note the cross-task dependency. |

- [ ] **Step 8: Commit**

Message body: the four commands and why each is sync or async; `screen:discarded` is not in the spec and exists so a discarded capture stops being offered for editing; "edited" is the export's own `is_untouched` predicate rather than the field's presence; discard refuses while that base is exporting.

---

### Task 9: `run_screen_recovery`

Spec §10. Today there is **no screen recovery at all** — `capture/src/recovery.rs` sweeps vault roots, not the staging directory, and a `.part` orphaned by a crash or by the ready-timeout path (docs/Gaps.md GAP-110) sits there forever (GAP-115).

**Files:**
- Create: `src-tauri/src/screen_recovery.rs`
- Modify: `src-tauri/src/lib.rs` (`mod` line + one `setup` line)

**Interfaces — produces:** `pub fn run_screen_recovery(app: &AppHandle)`.

**What it does, per file in the staging directory and nothing else:**

| Found | Action |
| --- | --- |
| `.<base>.mp4.part`, stale, with a `moov` and at least one `moof` | Rename to `<base>.mp4` and write a minimal sidecar, so it appears in the resume-or-discard list. |
| `.<base>.mp4.part`, stale, with no `moov` | Delete. It holds no playable footage — `mp4_boxes::scan` says so, and that is the same "a read failure must not look like no-audio" care the audio side takes. |
| `.<base>.mp4.part`, **not** stale | Leave it and reschedule. It may be a live capture's own file. |
| `.<base>.export.mp4.part` | Delete when stale. An abandoned export temp is litter by definition — the staged capture it came from is still there. |
| `<base>.mp4` with no sidecar | Write a minimal sidecar so the capture is reachable, rather than deleting footage. |
| `<base>.json` with no `.mp4` | Delete the orphan sidecar. |
| Anything else | **Leave it alone.** |

**Four rules copied from the audio and import recoveries, each because it prevented a real failure:**

1. **Ownership filter first.** Only names that round-trip through `staging::base_from_part` / `mp4_file_name` / `sidecar_file_name` **and** pass `is_safe_base` are ever touched. The staging directory is ours, but a user who has opened it and dropped a file there must not lose it.
2. **Never follow a symlink.** Use `entry.file_type()` (which does not follow) and `symlink_metadata` on the leaf.
3. **Postponed while a capture is active**, and rescheduled while work is pending — the `run_recovery` shape. Read `CaptureGuard::active()`; do not claim.
4. **Named constants, not inlined literals.** The audio side's staleness window is a bare `Duration::from_secs(60)` at a call site, which the ledger flags as the thing to fix rather than copy. Declare `const STALE_AFTER: Duration = Duration::from_secs(60);` and `const RETRY_EVERY: Duration = Duration::from_secs(90);` and `const MAX_RETRIES: u32 = 960;` with a comment giving the ~24 h figure. **The same 60 s as capture recovery on purpose** — one staleness rule for the whole app, as spec §10 requires.

Spawn failure **logs and continues** (`run_import_recovery`'s posture), never `.expect`-panics (`run_recovery`'s, which the ledger names as the worse precedent).

- [ ] **Step 1: Write the failing tests**

Everything decidable without a filesystem goes in a pure classifier, which is the whole point:

```rust
    #[test]
    fn only_our_own_names_are_recognised() {
        assert_eq!(classify(".2026-09-20 1432 Demo.mp4.part"), Entry::Part("2026-09-20 1432 Demo".into()));
        assert_eq!(classify("2026-09-20 1432 Demo.mp4"), Entry::Staged("2026-09-20 1432 Demo".into()));
        assert_eq!(classify("2026-09-20 1432 Demo.json"), Entry::Sidecar("2026-09-20 1432 Demo".into()));
        assert_eq!(classify(".2026-09-20 1432 Demo.export.mp4.part"), Entry::ExportTemp("2026-09-20 1432 Demo".into()));
        // Not ours. Every one of these has been a real file in somebody's
        // temp directory; none may be deleted.
        for foreign in ["notes.txt", "Demo.mkv", "thumbs.db", ".DS_Store", "Demo.mp4.bak", "report.json.bak"] {
            assert_eq!(classify(foreign), Entry::Foreign, "{foreign} was claimed");
        }
    }

    // An unsafe base must never be recognised as ours, or recovery becomes
    // the one path that deletes through a name the guarded commands refuse.
    #[test]
    fn an_unsafe_base_is_foreign_even_in_our_own_name_shape() {
        assert_eq!(classify("../obsidian/obsidian.json"), Entry::Foreign);
        assert_eq!(classify("COM1.mp4"), Entry::Foreign);
        assert_eq!(classify(".mp4"), Entry::Foreign);
    }

    #[test]
    fn a_part_with_no_index_holds_no_footage_and_a_fragmented_one_does() {
        let mut good = bx("ftyp", b"isom");
        good.extend(bx("moov", b"...."));
        good.extend(bx("moof", b"...."));
        assert!(part_holds_footage(&good));
        assert!(!part_holds_footage(&bx("ftyp", b"isom")));
        assert!(!part_holds_footage(&[]));
    }

    // A moov with NO moof is a standard-MP4 header and no fragments: there
    // is nothing recoverable in it, and promoting it would offer the user a
    // zero-length "recording".
    #[test]
    fn a_part_with_an_index_but_no_fragment_holds_no_footage() {
        let mut header_only = bx("ftyp", b"isom");
        header_only.extend(bx("moov", b"...."));
        assert!(!part_holds_footage(&header_only));
    }

    #[test]
    fn recovery_is_postponed_while_a_capture_is_running() {
        assert!(should_postpone(Some(crate::capture_guard::CaptureKind::Screen)));
        assert!(should_postpone(Some(crate::capture_guard::CaptureKind::Audio)));
        assert!(!should_postpone(None));
    }
```

- [ ] **Step 2–4: Implement `classify`, `part_holds_footage`, `should_postpone`, then the sweep and the thread**

`classify` returns an `Entry` enum and is pure. `part_holds_footage(&[u8]) -> bool` is `mp4_boxes::scan(bytes)` with `has_moov() && fragment_count() > 0` — **read only a prefix**, the audio side's `FRAME_SNIFF_LEN` trick, since a staged capture can be gigabytes and `scan` takes a `&[u8]`. Use a 1 MiB prefix and say why in a comment: `moov` and the first `moof` are both near the head of an MF-written fMP4.

The sweep walks `read_dir` once, classifies each name, and acts per the table. The thread is `screen-recovery`, `std::thread::Builder::new().name(...)`, logging a spawn failure.

Wire into `setup` immediately after `document_commands::run_import_recovery(app.handle());`:

```rust
            screen_recovery::run_screen_recovery(app.handle());
```

- [ ] **Step 5: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `classify`: drop the `is_safe_base` check | `an_unsafe_base_is_foreign_even_in_our_own_name_shape` |
| M2 | `classify`: match any `.mp4` as `Staged` regardless of stem | `only_our_own_names_are_recognised` (`Demo.mkv` stays Foreign, but `Demo.mp4.bak` — check which row actually catches it, and if none does, add one) |
| M3 | `classify`: return `Part` for `.export.mp4.part` | `only_our_own_names_are_recognised` — the ExportTemp row. A misclassified export temp would be PROMOTED to a staged capture, offering the user a half-written file as a recording |
| M4 | `part_holds_footage`: `has_moov()` only, drop `fragment_count() > 0` | `a_part_with_an_index_but_no_fragment_holds_no_footage` |
| M5 | `part_holds_footage`: `fragment_count() > 0` only, drop `has_moov()` | `a_part_with_no_index_holds_no_footage…` — **run both separately**; one mutation reddening the pair proves neither half |
| M6 | `should_postpone`: `matches!(active, Some(CaptureKind::Screen))` | `recovery_is_postponed_while_a_capture_is_running` (the Audio row) |
| M7 | Read the whole file instead of a 1 MiB prefix | not caught; note the memory cost it reintroduces |

- [ ] **Step 6: Gates and commit**

```
cd src-tauri && cargo test -p vault-buddy --lib 2>&1 | tail -5
cd src-tauri && cargo fmt --check && cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cd /home/user/vault-buddy && npm run check:loc; echo "exit=$?"
```

Message body: closes GAP-115; the ownership filter and the no-follow rule; named constants rather than the audio side's inlined literals, sharing the one 60 s staleness window; and that a spawn failure logs rather than panicking.

---

### Task 10: The editor grows Save, Discard and a progress bar

**Files:**
- Create: `src/components/editor/ExportBar.vue`
- Create: `tests/exportBar.test.ts`
- Modify: `src/roots/EditorRoot.vue`
- Modify: `src/types.ts`
- Modify: `tests/editorRoot.test.ts`, `tests/helpers/editorMount.ts`

**Interfaces:**
- Consumes: `export_and_save_capture { base }`, `cancel_export`, `discard_staged_capture { base }`, `open_screen_capture { id, path }`; the four `screen:export*` events.
- Produces: `ExportBar`'s props/emits, below.

**`ExportBar.vue` is presentational — no `invoke`, no store.** `EditorRoot` is 300/500 nonblank lines and this task adds roughly 120; extracting the bar keeps both comfortably under cap and follows how `ScreenRegionPicker`/`ScreenAudioPicker` were already split out of `ScreenSourcePicker`.

```ts
defineProps<{
  phase: "idle" | "exporting" | "done" | "failed";
  fraction: number;          // 0..1
  message: string | null;    // the failure message, or the success line
  canSave: boolean;          // false when the timeline holds no footage
  busy: boolean;             // a discard is in flight
}>();
defineEmits<{ save: []; discard: []; cancel: []; open: [] }>();
```

Testids: `export-save`, `export-discard`, `export-cancel`, `export-open`, `export-progress`, `export-message`.

#### Four things this task must get right

1. **The `preventDefault` target guard the source already asks for.** `EditorRoot`'s `onKeydown` calls `preventDefault()` unconditionally on Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y, and `editor_commands`'s own comment at `EditorRoot.vue:168-173` says in as many words that "the first `<input>` this window grows loses its native text undo to the timeline. That needs a target check (`closest("input, textarea")`), which is deliberately not written ahead of the field it would guard." **This task adds the first focusable control to that window.** Add the guard now:

```ts
function onKeydown(e: KeyboardEvent) {
  if ((!e.ctrlKey && !e.metaKey) || e.altKey) return;
  // The editor binds undo/redo on `window` because it fills its own
  // window, so every control in it is in scope. A text field's own undo
  // must still win inside that field -- otherwise Ctrl+Z in an input
  // silently edits the timeline instead of the text.
  const target = e.target as HTMLElement | null;
  if (target?.closest("input, textarea, [contenteditable='true']")) return;
  ...
}
```

Even though this phase's controls are buttons rather than inputs, write the guard with the phase that adds a field in mind — and test it, because a test is what stops the next author deleting it as unreachable.

2. **Discard is confirm-gated and Save is not.** Discard is irreversible and destroys the only copy of a recording; spec §10 says "Discard is confirm-gated, being irreversible. Nothing is ever deleted silently." Use a two-step in-component confirm (the `TaskSectionMenu` delete precedent), not a native dialog — a native dialog would steal focus, and `DIALOG_ACTIVE` is a process-wide bool with two drivers already (docs/Gaps.md GAP-128).

3. **A cancelled export is not a failure.** `screen:exportCancelled` returns the bar to `idle` with no message and no banner. The staged capture and its timeline are kept (spec §14), the editor stays open, and the user can press Save again.

4. **The success state offers Open, and `lastStaged` is already gone.** On `screen:exported` the bar shows "Saved to <vault>" with an **Open** button invoking `open_screen_capture`. The capture is no longer in staging, so Save and Discard are both hidden — leaving Save clickable would fire `export_and_save_capture` against a base whose sidecar was just deleted.

- [ ] **Step 1: Write the failing tests in `tests/exportBar.test.ts`**

```ts
  it("offers Save and Discard while idle and neither while exporting", async () => {
    const w = mount(ExportBar, { props: { phase: "idle", fraction: 0, message: null, canSave: true, busy: false } });
    expect(w.find('[data-testid="export-save"]').exists()).toBe(true);
    expect(w.find('[data-testid="export-cancel"]').exists()).toBe(false);
    await w.setProps({ phase: "exporting", fraction: 0.4 });
    expect(w.find('[data-testid="export-save"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-cancel"]').exists()).toBe(true);
  });

  // The progress element must reflect the fraction, not merely exist. A bar
  // that renders at a fixed width looks identical in a screenshot and tells
  // the user nothing.
  it("renders the progress value it is given, and moves when it changes", async () => {
    const w = mount(ExportBar, { props: { phase: "exporting", fraction: 0.25, message: null, canSave: true, busy: false } });
    const bar = w.find('[data-testid="export-progress"]');
    expect(bar.attributes("aria-valuenow")).toBe("25");
    await w.setProps({ fraction: 0.75 });
    expect(bar.attributes("aria-valuenow")).toBe("75");
  });

  it("disables Save when there is nothing left to save", () => {
    const w = mount(ExportBar, { props: { phase: "idle", fraction: 0, message: null, canSave: false, busy: false } });
    expect(w.find('[data-testid="export-save"]').attributes("disabled")).toBeDefined();
  });

  // Discard is irreversible: one click must not delete anything.
  it("requires a second click to discard", async () => {
    const w = mount(ExportBar, { props: { phase: "idle", fraction: 0, message: null, canSave: true, busy: false } });
    await w.find('[data-testid="export-discard"]').trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
    await w.find('[data-testid="export-discard"]').trigger("click");
    expect(w.emitted("discard")).toHaveLength(1);
  });

  it("hides Save and Discard once the capture has been saved", () => {
    const w = mount(ExportBar, { props: { phase: "done", fraction: 1, message: "Saved to Engineering", canSave: true, busy: false } });
    expect(w.find('[data-testid="export-save"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-discard"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-open"]').exists()).toBe(true);
    expect(w.text()).toContain("Saved to Engineering");
  });
```

And in `tests/editorRoot.test.ts`:

```ts
  it("sends the open base to export_and_save_capture, not a stale one", async () => {
    // Open capture A, then B, then Save. The command must carry B.
    const calls: string[] = [];
    ...
    expect(calls).toEqual(["cap-two"]);
  });

  // The Rust side emits a fraction in 0..1 and the bar renders a percent.
  // Emitting 0..100 by mistake renders 4000% with nothing to catch it on
  // the Rust side, which has no frontend to assert against.
  it("reads screen:exportProgress as a fraction between zero and one", async () => {
    ...
    emit("screen:exportProgress", { base: "cap-one", fraction: 0.4 });
    await nextTick();
    expect(w.find('[data-testid="export-progress"]').attributes("aria-valuenow")).toBe("40");
  });

  // A progress event for a DIFFERENT capture must not drive this window's
  // bar: the events are app-wide, and an editor reopened on capture B while
  // A is still exporting would otherwise show A's progress.
  it("ignores an export event addressed to another capture", async () => { ... });

  it("returns to idle without an error banner when an export is cancelled", async () => { ... });

  it("shows the failure message and keeps the edit on screen when an export fails", async () => { ... });

  // The guard the source comment asks for, tested so the next author
  // cannot delete it as unreachable.
  it("lets a text field keep its own undo", async () => {
    const w = await mountEditor();
    const input = document.createElement("input");
    document.body.appendChild(input);
    const saves = countSaves();
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, bubbles: true }));
    await nextTick();
    expect(countSaves()).toBe(saves);   // no timeline undo ran
  });
```

- [ ] **Step 2: Run and watch them fail**

```
cd /home/user/vault-buddy && npx vitest run tests/exportBar.test.ts tests/editorRoot.test.ts 2>&1 | tail -20
```

- [ ] **Step 3: Implement `ExportBar.vue`, then wire `EditorRoot`**

`EditorRoot` gains: `exportPhase`, `exportFraction`, `exportMessage`, `savedPath`, `discardBusy` refs; four `listen()` subscriptions registered in `onMounted` **before** the first `openRequested()` and torn down in `onBeforeUnmount` beside the existing `editor:open` unlisten; handlers that **ignore any event whose `base` is not the open capture's**; `canSave` computed as `timeline.value.segments.some((s) => s.sourceEndMs > s.sourceStartMs)` — **a UI hint, not the rule.** `export::export_refusal` is the authority and refuses the same case server-side; this exists only so Save reads as disabled rather than failing on click. That is the established posture for exactly this shape (the task-hierarchy picker pre-disables self and descendants while core's cycle check remains the authority), and it is why the predicate is deliberately trivial: anything cleverer would be a second implementation of a Rust rule, which is the divergence GAP-136 already tracks. Add a comment saying so; and `load()` resetting all four export refs so opening a second capture does not inherit the first's success state.

Delete the phase-4 honesty line at `EditorRoot.vue:297-303` — "Saving into a vault arrives in a later update." — and its comment. It is now false.

Add to `src/types.ts`:

```ts
/** `screen:exportProgress`. `fraction` is 0..1, never 0..100. */
export interface ExportProgress { base: string; fraction: number }
/** `screen:exported`. `notePath` is null when the vault has notes turned off. */
export interface ExportResult {
  base: string;
  videoPath: string;
  notePath: string | null;
  vaultId: string;
  warning: string | null;
}
```

- [ ] **Step 4: Run, then the mutation table**

```
cd /home/user/vault-buddy && npx vitest run tests/exportBar.test.ts tests/editorRoot.test.ts 2>&1 | tail -8
```

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `ExportBar`: render `aria-valuenow="50"` fixed | `renders the progress value it is given, and moves when it changes` |
| M2 | `ExportBar`: emit `discard` on the first click | `requires a second click to discard` |
| M3 | `ExportBar`: keep Save visible in `done` | `hides Save and Discard once the capture has been saved` |
| M4 | `EditorRoot`: drop the base check in the progress handler | `ignores an export event addressed to another capture` |
| M5 | `EditorRoot`: treat `screen:exportCancelled` as a failure | `returns to idle without an error banner when an export is cancelled` |
| M6 | `EditorRoot`: `invoke("export_and_save_capture", { base: detail.value.base })` → a captured constant from the first load | `sends the open base to export_and_save_capture, not a stale one` |
| M7 | `EditorRoot`: delete the `closest("input, textarea…")` guard | `lets a text field keep its own undo` |
| M8 | `EditorRoot`: `fraction * 1` → render the raw fraction as the percent | `reads screen:exportProgress as a fraction between zero and one` |
| M9 | `EditorRoot`: don't reset `exportPhase` in `load()` | **add a test if none fails** — open A, save it, open B, and assert B's bar is `idle` with Save available. A stale `done` would leave B unsaveable. |

M9 is written as an instruction rather than an expectation on purpose: it is the phase-4 "the editor comes back showing capture A" bug in a new field, and the point is to find out whether the suite covers it.

- [ ] **Step 5: Gates and commit**

```
cd /home/user/vault-buddy && rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run build; echo "exit=$?"
cd /home/user/vault-buddy && npm run test:coverage 2>&1 | tail -12
```

Report `EditorRoot.vue` and `ExportBar.vue`'s nonblank line counts against the 500 cap, and every coverage axis against 95/91/93/96.

---

### Task 11: Resume or discard, shown first in Record Screen

Spec §10: "Opening Record Screen with staged captures present shows them first: each with its source, duration and age, offering *Resume editing* or *Discard*."

**Files:**
- Create: `src/components/StagedCaptureList.vue`, `tests/stagedCaptureList.test.ts`
- Modify: `src/components/ScreenSourcePicker.vue`, `src/stores/screenCapture.ts`, `src/types.ts`, `tests/screenSourcePicker.test.ts`, `tests/screenCaptureStore.test.ts`

**Where it attaches, and why not a new view.** A sibling block inside `ScreenSourcePicker`'s root, between the error `Banner` and the `<TabGroup>`. That is literally "shown first" while leaving the source picker reachable below it, and it needs **no** change to `vaults.ts`. A new panel view would need six edit sites (union member, `open…` action, id field, a `showList()` line, a `back()` branch, a `VIEW_TITLES` entry and a render block) and would fork the Record Screen entry point. A fourth tab would *hide* the list behind a tab, which is the opposite of what the spec asks.

`ScreenSourcePicker` is 225/500 nonblank; a presentational child keeps both well under cap and matches how `ScreenRegionPicker` and `ScreenAudioPicker` were already extracted from this same file.

Add to `src/types.ts`, mirroring Task 8's `StagedCaptureSummaryDto` field for field (camelCase on both sides, the `StagedCapture` precedent):

```ts
/** One row of `list_staged_captures`. `outputDurationMs` is what the export
 *  will produce; `durationMs` is what was recorded. They differ on an edit. */
export interface StagedCaptureSummary {
  base: string;
  vaultId: string;
  sourceTitle: string;
  durationMs: number;
  outputDurationMs: number;
  recordedAt: string;
  width: number;
  height: number;
  edited: boolean;
}
```

```ts
// StagedCaptureList.vue
defineProps<{ captures: StagedCaptureSummary[]; busyBase: string | null }>();
defineEmits<{ resume: [base: string]; discard: [base: string] }>();
```

Testids: `staged-list`, `staged-row-<base>`, `staged-resume-<base>`, `staged-discard-<base>`.

**The store change:** `screenCapture` gains listeners for `screen:exported` and `screen:discarded` that clear `lastStaged` **only when the base matches**. `lastStaged` has exactly one set site and one clear site today, and the clear site's comment records that skipping its staleness guard would discard the only handle on the footage. Adding a second clear is safe here because it is keyed on identity rather than on lifecycle: it fires only for the capture that genuinely no longer exists.

- [ ] **Step 1: Write the failing tests**

```ts
// tests/stagedCaptureList.test.ts
  it("names each capture's source, its length and how long ago it was recorded", () => { ... });

  // The list is how a user finds work they abandoned. A capture carrying an
  // edit must say so, or they cannot tell which one they were part-way
  // through.
  it("marks an edited capture and shows the length it will export to", () => {
    const w = mount(StagedCaptureList, { props: { captures: [edited({ durationMs: 60_000, outputDurationMs: 20_000 })], busyBase: null } });
    expect(w.text()).toContain("edited");
    expect(w.text()).toContain("0:20");
  });

  it("requires a second click to discard a staged capture", async () => { ... });

  it("disables both actions on the row whose write is in flight, and only that row", async () => { ... });

  it("renders nothing at all when there are no staged captures", () => {
    const w = mount(StagedCaptureList, { props: { captures: [], busyBase: null } });
    expect(w.find('[data-testid="staged-list"]').exists()).toBe(false);
  });
```

```ts
// tests/screenSourcePicker.test.ts
  it("shows staged captures above the source tabs, not below or behind one", async () => {
    // Positional, not merely present: the spec says shown FIRST.
    const html = w.html();
    expect(html.indexOf('data-testid="staged-list"')).toBeLessThan(html.indexOf('role="tablist"'));
  });

  it("still lets a new capture be started while staged captures are listed", async () => { ... });
```

```ts
// tests/screenCaptureStore.test.ts
  it("clears lastStaged when that capture is exported", async () => { ... });
  it("clears lastStaged when that capture is discarded", async () => { ... });

  // The events are app-wide. Clearing on a base we are not holding would
  // throw away the handle on a DIFFERENT capture's footage.
  it("keeps lastStaged when another capture is exported or discarded", async () => {
    store.lastStaged = { base: "keep-me", ... };
    emit("screen:discarded", { base: "something-else" });
    await nextTick();
    expect(store.lastStaged?.base).toBe("keep-me");
  });
```

- [ ] **Step 2–4: Run them failing, implement, run them passing**

`ScreenSourcePicker` loads the list in `onMounted` beside `loadSources()` and reloads it after a discard. **Resume** invokes `open_capture_editor { base }` — the same command the capture bar's Edit button uses, so there is one way into the editor rather than two.

- [ ] **Step 5: Mutation table**

| # | Mutation | Must fail |
| --- | --- | --- |
| M1 | `StagedCaptureList`: render the list below the tabs | `shows staged captures above the source tabs` |
| M2 | `StagedCaptureList`: emit `discard` on the first click | `requires a second click to discard a staged capture` |
| M3 | `StagedCaptureList`: disable every row when any is busy | `disables both actions on the row whose write is in flight, and only that row` |
| M4 | `StagedCaptureList`: show `outputDurationMs` as `durationMs` | `marks an edited capture and shows the length it will export to` |
| M5 | Store: clear `lastStaged` on every `screen:discarded` | `keeps lastStaged when another capture is exported or discarded` |
| M6 | Store: drop the `screen:exported` clear | `clears lastStaged when that capture is exported` |
| M7 | `StagedCaptureList`: render the container even when empty | `renders nothing at all when there are no staged captures` |

- [ ] **Step 6: Gates and commit**

```
cd /home/user/vault-buddy && rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run build; echo "exit=$?"
cd /home/user/vault-buddy && npm run test:coverage 2>&1 | tail -12
```

---

### Task 12: Docs, gaps, the checklist, and the baselines

A docs task's failure mode is a **confident false statement** in the one file the next agent treats as authoritative. **Measure every number; never increment a previous one.** The command count has been wrong three times on this branch.

**Files:** `AGENTS.md`, `docs/Gaps.md`, `docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`, `docs/superpowers/specs/2026-09-18-screen-capture-intake-design.md`, `CONTEXT.md`, `README.md`, `scripts/loc-baseline.json` / `scripts/quality-baseline.json` (only if a metric genuinely moved).

- [ ] **Step 1: AGENTS.md — eleven sites, each verified against the tree**

1. **The vault-domain list gains a NINTH sanctioned write.** It is currently headed "Eight sanctioned write paths exist"; the sentence, the count and the numbered list all change. Word it as: *the **screen-capture** domain — the exported `.mp4` and its companion note, committed through `core::screen_capture_paths` on the same never-clobber rails (pairwise `.mp4`/`.md` reservation, `rename_noreplace` with ` (N)` retry), video first and note second, with a note failure degrading to a warning.*
2. **The screen-capture section's opening claim is now FALSE and must be rewritten.** It currently says "**Phases 2–4 write NOTHING into a vault** … the ninth sanctioned vault write does not exist yet; it arrives in Phase 5 … Do not add one here," and instructs the reader to verify it structurally against `editor_commands.rs`. That structural check still holds for `editor_commands.rs` specifically — say so, and name `export_worker.rs` as the one file in this feature that does resolve a vault path.
3. **The IPC table gains a `export_commands.rs` row** with all five commands and their sync/async markers, and the count sentence is updated — **by counting**:
   ```
   awk '/generate_handler!\[/,/\]\)/' src-tauri/src/lib.rs | grep -cE '^\s+[a-z_]+::[a-z_]+,$'
   ```
4. **The Events table gains five rows.** All five are `app.emit`; the table's opener names `region:begin` and `editor:open` as the only two exceptions, and that stays true — check it rather than assuming.
5. **"What compiles where"** — the `screen` crate row gains `reader`, `export` and `disk` to its `cfg(windows)` list, and `select`'s pure list is unchanged but now has a production caller.
6. **The three-orphans sentence is now wrong.** It says `core::timeline`, `screen::select` and `core::screen_note` "STILL have no production caller after phase 4". All three now have one. Rewrite it; do not delete it — the remaining orphan (`screen::mp4_boxes`, reachable only from the feature-gated spike) still needs naming, and `mp4_boxes` in fact gains a caller here too, via `screen_recovery` and `export`'s precondition check. **Verify with grep before writing either claim.**
7. **The config section:** five of the seven `screen_*` fields are now read (`screen_capture_folder`, `screen_capture_date_folders`, `screen_create_note`, `screen_extra_frontmatter`, `screen_body_template`); `screen_quality` and `screen_fps` are read by export too. Check whether any remain write-only and say exactly which.
8. **`EditorRoot` installs no store** — still true, and now it listens to four Rust events directly. The per-window `init()` rule does not start applying, because there is still no store to initialise; say why, so the next reader does not "fix" it.
9. **A fourth process-wide lock**, `ExportState`, beside `config_write_lock`, the per-file task lock and `CaptureGuard`. It stands outside the ordering rule the same way `CaptureGuard` does: taken, decided, dropped. Add it to the concurrency note.
10. **`core::screen_capture_paths` in the repository map**, and the note that the one collision-suffix scheme lives in `capture_paths::candidate` and is `pub(crate)`, which is why the screen reservation lives in `core`.
11. **Where state lives on disk** — the staging directory now has a recovery sweep and an export temp shape (`.<base>.export.mp4.part`).

- [ ] **Step 2: docs/Gaps.md**

Close or rewrite, each verified at source first:
- **GAP-115** (no staging recovery, completed staged captures never swept or user-deletable) — **closed** by Tasks 9 and 11.
- **GAP-134** (a `{}` sidecar timeline throws a raw TypeError) — **closed** on the Rust side by `timeline_from_sidecar`; check whether the frontend path it names still exists before closing it outright, and if it does, narrow the entry rather than closing it.
- **GAP-135** (`core::timeline` has no serde derives, so the camelCase names are an unenforced convention) — Task 7 parses the field by hand rather than deriving `Deserialize`, which keeps the convention unenforced but adds a test that pins it. Rewrite the entry to say so.
- **GAP-136** (two implementations, two languages) — still open; note that `is_untouched` now rides the shared table with Rust owning the rule.
- **GAP-138** (the staged-capture row never expires) — check whether Task 11's discard/export clears close it.

New entries, numbered from **GAP-140** (139 is the current high — verify):
- The export's `cfg(windows)` bodies execute in no test on any platform (the largest instance of GAP-117's class yet).
- Export refuses while a capture is running, so a user cannot save an old capture while recording — a deliberate trade, recorded as a limitation.
- No rename-after-save, and no way to re-export a capture once it has been saved and the staged copy removed.
- Screen captures are absent from the Recordings browser and are never transcribed.
- The fast path's passthrough is unverified: a mis-set input media type would silently transcode and still produce a correct file.
- Preview seek latency at segment boundaries (spec §8.2 named it; it belongs in the backlog now that export makes the preview/export distinction user-visible).
- Export time on long recordings is unmeasured.
- Any mutation row that stayed green across Tasks 1–11, each as its own entry.

- [ ] **Step 3: The verification checklist — rows 29–36**

Keep every earlier row's text intact. Empty Result columns; report-don't-assert. **These rows are this phase's real gate.**

| Row | What it checks |
| --- | --- |
| 29 | **The untouched fast path.** Record ~60 s, open the editor, change nothing, Save. Record: how long the save took (seconds, not minutes — a re-encode of 60 s would be plainly slower), and whether the saved `.mp4` is bit-for-bit the same length and quality as the staged one. A fast path that quietly transcodes shows up here and nowhere else. |
| 30 | **An edited export matches the preview.** Record ~60 s with a visible clock. Trim the first 20 s, delete a middle block, reorder two blocks. Save. Open the result in a player and **write down the clock values at each cut**, then compare against what the editor's preview showed. This is the row that checks the two implementations of the segment algebra agree in the one place it matters. |
| 31 | **The note.** Open the saved note in Obsidian. Record: whether the video plays **inside Obsidian**; whether `duration` is the EXPORTED length (not the original); whether `resolution` matches the file; and whether `source` survived a window title containing a colon or a quote. |
| 32 | **Never clobber.** Save a capture, then record and save a second one in the same minute with the same window title. Record both file names — the second must carry ` (2)` and the first must be untouched. |
| 33 | **Cancel.** Start a save of a long edited capture, press Cancel at ~30%. Record: whether the editor returns to normal with no error, whether the staged capture is still listed, whether the vault gained any file, and whether a `.export.mp4.part` was left in the staging directory. |
| 34 | **Discard.** Discard a staged capture. Record: whether it takes two clicks, whether the `.mp4` and `.json` are both gone from staging, and whether the panel's capture bar stops offering **Edit** for it. |
| 35 | **Recovery.** Kill the app mid-capture (Task Manager → End task), relaunch, and open Record Screen. Record: whether the orphaned `.part` appears as a staged capture, whether it plays, and how much of the recording it holds. |
| 36 | **The disk check and a full disk.** If cheap to stage: fill the vault's volume to under the estimate and press Save. Record the message. Then restore the space and confirm the same capture saves. |

- [ ] **Step 4: The spec's own reconciliation note**

Add a short reconciled-after-phase-5 note in the style of the existing §11 one, covering all three deliberate departures:

1. **§8.3's per-span cancellation poll** is shipped as a **per-sample** poll, because the fast path's plan is one span and a per-span poll would make the longest, most unattended export the only uncancellable one.
2. **§14's "exported temp kept" on a failed vault write** is shipped as "temp deleted, staged capture kept". A kept temp is a promise the staging sweep breaks after 60 s, so the same button would behave differently depending on how long the user spent reading the error. A retry re-exports from the staged capture, which is the artifact that must never be lost.
3. **§11's command list gains `screen:discarded`**, an event it does not name, without which a discarded capture keeps being offered for editing through a `lastStaged` that points at a file no longer on disk.

- [ ] **Step 5: CONTEXT.md and README.md**

CONTEXT.md: the §2 terms this phase makes real — **Staged Capture**, **Export**, **Discard**. README: a Screen Capture feature entry describing record → edit → save.

- [ ] **Step 6: Baselines — measure, do not assume**

```
cd /home/user/vault-buddy && rm -rf coverage
npm run lint; echo "exit=$?"
npm run check:loc; echo "exit=$?"
npm run check:quality; echo "exit=$?"
npm run build; echo "exit=$?"
npm run test:coverage 2>&1 | tail -12
cd src-tauri && cargo fmt --check; echo "exit=$?"
cargo clippy --workspace --all-targets -- -D warnings; echo "exit=$?"
cargo test -p vault-buddy --lib 2>&1 | tail -3
for c in core capture transcribe screen mcp; do cargo test -p vault_buddy_$c 2>&1 | tail -2; done
cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_screen --fail-under-lines 94 2>&1 | tail -3; echo "exit=$?"
cargo machete .; echo "exit=$?"
cargo deny check; echo "exit=$?"
cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc -- -D warnings; echo "exit=$?"
```

If a baseline must move, **hand-edit the single value** and write the justification into the entry. Report the before/after and the reason. If a counter metric would rise, **extract instead** — no counter metric has been loosened anywhere on this branch.

- [ ] **Step 7: Commit**

Message body: the ninth sanctioned vault write is documented; which claims the phase's own code falsified (the "writes NOTHING into a vault" opener, the three-orphans sentence, GAP-115); the measured command count; and that the checklist gained rows 29–36 which nobody is asked to run yet.

---

## Phase exit criteria

Verified by running the command, not by reading a report:

- [ ] All twelve tasks committed on `claude/screen-capture-intake-g0j49q` and pushed; PR #79 shows them; no second PR exists.
- [ ] `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo clippy -p vault_buddy_screen --all-targets --target x86_64-pc-windows-msvc` clean. **The shell-crate equivalent is impossible here and is not a gate.**
- [ ] All five member crates' tests plus `cargo test -p vault-buddy --lib` pass.
- [ ] `cargo machete .` and `cargo deny check` clean. **No new crate** — the only dependency change permitted is the `Win32_Storage_FileSystem` feature on the `windows` crate the screen crate already uses. Confirm `git diff Cargo.lock` is empty.
- [ ] `cargo llvm-cov … --fail-under-lines 94` passes; report the measured figure.
- [ ] `rm -rf coverage && npm run lint && npm run check:loc && npm run check:quality && npm run test:coverage && npm run build` passes in that order, with exactly one pre-existing lint warning in `src/main.ts`.
- [ ] Baselines unchanged, or a moved one carries a written justification. **No counter metric loosened.**
- [ ] **`core::timeline` and `screen::select` have a production caller** — `grep` for it, and do not tick this the way phase 4's equivalent was ticked while false.
- [ ] **`core::screen_note::render_screen_note` has a production caller.**
- [ ] The staged capture is removed only after the vault write lands, pinned by a structural test.
- [ ] The export claims no `CaptureGuard`: `screen_commands.rs`'s production prefix still contains exactly one `release(CaptureKind::Screen)` and `screen_capture_worker.rs` exactly zero.
- [ ] `sink.rs` still contains no `.Flush(` and no `metadata(` in its production prefix.
- [ ] The asset-protocol scope is still exactly `["$APPLOCALDATA/screen-captures/*"]` — export must not widen it to reach a vault.
- [ ] The IPC command count in AGENTS.md was **measured**, not incremented, and matches the `generate_handler!` list.
- [ ] The export runs on a thread named `screen-export` (`std::thread::Builder`), not on a `spawn_blocking` pool thread — `grep` for the builder.
- [ ] No failure path leaves an export temp behind: `grep` the worker and `export.rs` and confirm every `Err` return is preceded by the removal.
- [ ] The three deliberate departures from the spec are each recorded in the spec's own reconciliation note AND in docs/Gaps.md: cancellation polls per sample rather than per span; a failed vault write deletes the export temp rather than keeping it; and `screen:discarded` exists although §11 does not list it.
- [ ] The verification checklist carries rows 29–36 AND every open earlier row. **Nobody is asked to run them yet.**

## What Phase 6 needs from this phase

- **The settings surface is the last thing standing between this feature and a user who can configure it.** `screen_quality`, `screen_fps`, the capture folder and the date-folder toggle are all `config.json` hand-edits today; `ScreenCaptureConfigTab` and `set_screen_capture_config` are Phase 6's.
- **The staging directory's total size and a "Clear staged captures" action** (spec §10's disk-pressure paragraph) are Phase 6's. Task 9 gives it the sweep to build on; Task 8 gives it `list_staged_captures`.
- **Checklist row 10 (4K @ 60 fps) is still deferred**, explicitly, until the fps knob is real. Carry it forward.
- **The two implementations of the segment algebra are still two** (GAP-136). This phase added `is_untouched` to the shared fixture table with Rust owning the rule; `toOutputMs` remains TypeScript-only, and `PlanSpan::restamp` is now its Rust counterpart without a shared row. If Phase 6 touches either, that is the row to add.
- **Nothing in Phases 3, 4 or 5 has been verified on hardware.** Rows 11–36 are written and unrun. The user's standing decision defers that to after the final phase — which is Phase 6, so Phase 6 owes the hand-off that makes the run possible.
