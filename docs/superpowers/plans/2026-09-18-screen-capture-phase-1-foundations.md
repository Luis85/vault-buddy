# Screen Capture — Phase 1 (Foundations) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build every pure, platform-independent foundation the Screen Capture increment needs — the timeline algebra, the DPI/crop geometry, the per-vault config fields, the companion-note renderer, an N-source audio mixer, and the `screen` crate skeleton with its pause clock and frame-selection planner — all unit-tested on Linux, with no user-visible change.

**Architecture:** Correctness for this feature lives in pure modules precisely because no CI runner can record a screen. This phase builds all of them and nothing else: five additions to `vault_buddy_core`, one generalization in `vault_buddy_capture`, and a new `vault_buddy_screen` crate whose Windows-only surface is a compile-time stub at this stage. Phase 2 fills that stub in.

**Tech Stack:** Rust 2021 (`vault_buddy_core`, `vault_buddy_capture`, new `vault_buddy_screen`), `serde_json` for config parsing, `serde_yaml_ng` via the existing `core::template`. No new third-party dependency in this phase.

**Spec:** `docs/superpowers/specs/2026-09-18-screen-capture-intake-design.md` (§4.1, §4.2, §5.2, §6.2, §6.5, §8.1, §9.1, §12, §13 phase 1, §15)

## Global Constraints

- **This phase adds no user-visible behavior.** No IPC command, no Vue component, no window. A reviewer should be able to run the app and see nothing new.
- **Nothing Windows-only compiles in this phase.** `vault_buddy_screen`'s engine modules are stubs returning `ScreenError::Unsupported`. `windows-capture` is NOT added as a dependency yet — that is Phase 2, after the fMP4 spike.
- **Every new module must compile and test on Linux.** This is the whole point of the phase.
- **Never widen the vault write surface.** This phase writes nothing to any vault; `render_screen_note` returns a `String` and nothing calls it yet.
- **Additive config only.** The seven new `VaultCaptureConfig` fields must default such that an existing `config.json` parses byte-identically to today, and `serialize_vault_entry` must not emit a key for a default value where the existing code omits defaults.
- **One staleness rule, one mixer, one clock.** Do not introduce a second copy of a rule that already exists.
- Exact default values, copied from the spec §12: `screen_capture_folder` → `None` (resolves to `Screen Captures`), `screen_capture_date_folders` → `false`, `screen_quality` → `Balanced`, `screen_fps` → `30` (only `30` or `60` legal), `screen_create_note` → `true`, `screen_extra_frontmatter` / `screen_body_template` → `None`.
- Bits-per-pixel-per-frame for the quality presets, spec §12: Low `0.08`, Balanced `0.15`, High `0.25`.
- Managed (reserved) screen-note frontmatter keys, spec §9.1: `type`, `recorded`, `duration`, `source`, `inputs`, `resolution`, `vault`, `created-by`.
- Every regression test names its failure mode in a comment (repo TDD convention).
- Commit style: Conventional Commits, imperative subject, body explains the *why* and the failure mode being prevented.
- Rust gates, run from `src-tauri/`: `cargo fmt --check`; from `src-tauri/core/`: `cargo clippy --all-targets -- -D warnings && cargo test`.
- Coverage: the `rust-core` job runs `cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --fail-under-lines 94`. Every new `core` module is pure and must be thoroughly tested so this floor does not drop.

## File Structure

| File | Responsibility | Change |
| --- | --- | --- |
| `src-tauri/core/src/timeline.rs` | Segment algebra: split / delete / reorder / duration / mapping | Create |
| `src-tauri/core/src/screen_geometry.rs` | Logical→physical DPI scaling and crop clamping | Create |
| `src-tauri/core/src/screen_capture_config.rs` | `ScreenQuality` enum, bitrate maths, fps validation, folder default | Create |
| `src-tauri/core/src/screen_note.rs` | The Screen Capture companion-note renderer | Create |
| `src-tauri/core/src/vault_config.rs` | Per-vault config struct + parse/serialize | Modify: add 7 fields |
| `src-tauri/core/src/config_merge.rs` | Cross-surface field preservation | Modify: preserve screen fields |
| `src-tauri/core/src/lib.rs` | Core module list | Modify: 4 new `pub mod` lines |
| `src-tauri/capture/src/mixer.rs` | Pure sample maths | Modify: add `mix_n_to_stereo_i16` |
| `src-tauri/screen/Cargo.toml` | New crate manifest | Create |
| `src-tauri/screen/src/lib.rs` | Crate root, `ScreenError`, re-exports | Create |
| `src-tauri/screen/src/clock.rs` | Pause-aware monotonic capture clock | Create |
| `src-tauri/screen/src/select.rs` | Timeline → ordered frame plan | Create |
| `src-tauri/screen/src/engine.rs` | Windows-only surface; `Unsupported` stub this phase | Create |
| `src-tauri/Cargo.toml` | Workspace members | Modify: add `"screen"` |
| `.github/workflows/ci.yml` | CI gates | Modify: add `-p vault_buddy_screen` to clippy + test |
| `AGENTS.md` | Agent operating guide | Modify: "what compiles where" row |

**Task order rationale:** Tasks 1–2 and 5 are fully independent. Task 3 must precede Task 4 (the note renderer reads config-derived values). Task 6 depends on Task 1 (`select` consumes `Timeline`). Task 7 is docs/CI and comes last.

---

### Task 1: The timeline algebra (`core::timeline`)

**Files:**
- Create: `src-tauri/core/src/timeline.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod timeline;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `Segment { source_start_ms: u64, source_end_ms: u64 }`, `Timeline { segments: Vec<Segment> }`, and on `Timeline`: `whole(duration_ms: u64) -> Timeline`, `split_at(&self, output_ms: u64) -> Timeline`, `delete(&self, index: usize) -> Timeline`, `reorder(&self, from: usize, to: usize) -> Timeline`, `output_duration_ms(&self) -> u64`, `to_source_ms(&self, output_ms: u64) -> Option<u64>`, `is_untouched(&self, source_duration_ms: u64) -> bool`, `is_empty(&self) -> bool`. On `Segment`: `duration_ms(&self) -> u64`. Task 6's `select::plan` consumes `Timeline`; Phase 4's editor consumes all of it.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/timeline.rs` containing ONLY the test module for now:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: u64, end: u64) -> Segment {
        Segment { source_start_ms: start, source_end_ms: end }
    }

    #[test]
    fn whole_is_one_segment_spanning_the_source() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.segments, vec![seg(0, 5_000)]);
        assert_eq!(t.output_duration_ms(), 5_000);
    }

    #[test]
    fn split_divides_the_containing_segment_at_the_playhead() {
        let t = Timeline::whole(5_000).split_at(2_000);
        assert_eq!(t.segments, vec![seg(0, 2_000), seg(2_000, 5_000)]);
        assert_eq!(t.output_duration_ms(), 5_000, "a split never changes duration");
    }

    // Regression: a split exactly on a boundary must be a NO-OP, not a
    // zero-length segment. A zero-length segment reaches the exporter and
    // produces an unplayable file (spec §8.1).
    #[test]
    fn split_on_an_existing_boundary_is_a_noop() {
        let t = Timeline::whole(5_000).split_at(2_000);
        assert_eq!(t.clone().split_at(2_000), t, "boundary split changes nothing");
        assert_eq!(t.clone().split_at(0), t, "split at 0 changes nothing");
        assert_eq!(t.clone().split_at(5_000), t, "split at the end changes nothing");
    }

    #[test]
    fn split_past_the_end_is_a_noop() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.clone().split_at(9_999), t);
    }

    #[test]
    fn delete_removes_one_segment_and_shortens_the_output() {
        let t = Timeline::whole(5_000).split_at(2_000).delete(0);
        assert_eq!(t.segments, vec![seg(2_000, 5_000)]);
        assert_eq!(t.output_duration_ms(), 3_000);
    }

    #[test]
    fn delete_out_of_range_is_a_noop() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.clone().delete(7), t);
    }

    #[test]
    fn deleting_the_last_segment_yields_an_empty_timeline() {
        let t = Timeline::whole(5_000).delete(0);
        assert!(t.is_empty());
        assert_eq!(t.output_duration_ms(), 0);
    }

    #[test]
    fn reorder_moves_a_segment_without_changing_duration() {
        let t = Timeline::whole(6_000).split_at(2_000).split_at(4_000);
        assert_eq!(t.segments, vec![seg(0, 2_000), seg(2_000, 4_000), seg(4_000, 6_000)]);
        let r = t.reorder(0, 2);
        assert_eq!(r.segments, vec![seg(2_000, 4_000), seg(4_000, 6_000), seg(0, 2_000)]);
        assert_eq!(r.output_duration_ms(), 6_000);
    }

    #[test]
    fn reorder_out_of_range_is_a_noop() {
        let t = Timeline::whole(5_000).split_at(2_000);
        assert_eq!(t.clone().reorder(0, 9), t);
        assert_eq!(t.clone().reorder(9, 0), t);
    }

    #[test]
    fn to_source_ms_maps_output_time_through_reordered_segments() {
        let t = Timeline::whole(6_000).split_at(2_000).split_at(4_000).reorder(0, 2);
        // Output now plays [2000..4000), [4000..6000), [0..2000).
        assert_eq!(t.to_source_ms(0), Some(2_000));
        assert_eq!(t.to_source_ms(1_500), Some(3_500));
        assert_eq!(t.to_source_ms(2_000), Some(4_000), "first frame of segment 2");
        assert_eq!(t.to_source_ms(4_500), Some(500), "inside the moved-to-last segment");
    }

    #[test]
    fn to_source_ms_is_none_past_the_end() {
        let t = Timeline::whole(5_000);
        assert_eq!(t.to_source_ms(5_000), None, "end is exclusive");
        assert_eq!(t.to_source_ms(9_999), None);
    }

    #[test]
    fn is_untouched_only_for_a_single_full_span_segment() {
        assert!(Timeline::whole(5_000).is_untouched(5_000), "the fast-path case");
        assert!(!Timeline::whole(5_000).split_at(2_000).is_untouched(5_000));
        assert!(!Timeline::whole(5_000).delete(0).is_untouched(5_000));
        assert!(
            !Timeline { segments: vec![seg(0, 4_000)] }.is_untouched(5_000),
            "a trimmed tail is not untouched"
        );
    }

    // Invariant: no operation may ever produce an empty or inverted segment.
    // Anything violating this reaches the exporter as a corrupt frame plan.
    #[test]
    fn every_operation_preserves_non_empty_segments() {
        let cases = vec![
            Timeline::whole(5_000),
            Timeline::whole(5_000).split_at(2_500),
            Timeline::whole(5_000).split_at(2_500).delete(0),
            Timeline::whole(6_000).split_at(2_000).split_at(4_000).reorder(2, 0),
        ];
        for t in cases {
            for s in &t.segments {
                assert!(s.source_start_ms < s.source_end_ms, "segment {s:?} is empty or inverted");
            }
        }
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri/core && cargo test timeline 2>&1 | head -20`
Expected: FAIL to compile — `cannot find type Segment in this scope` (the module has tests but no implementation).

- [ ] **Step 3: Write the implementation**

Insert ABOVE the `#[cfg(test)]` block in `src-tauri/core/src/timeline.rs`:

```rust
//! The editor's segment algebra: which spans of a staged Screen Capture play,
//! and in what order. Pure arithmetic with no I/O, so the editor's correctness
//! is testable on Linux even though a screen can only be captured on Windows
//! (spec §4.2, §8.1).
//!
//! Every operation returns a NEW `Timeline`, which is what makes undo/redo a
//! stack of snapshots rather than a set of inverse operations.

/// A half-open span `[source_start_ms, source_end_ms)` of the staged capture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub source_start_ms: u64,
    pub source_end_ms: u64,
}

impl Segment {
    pub fn duration_ms(&self) -> u64 {
        self.source_end_ms.saturating_sub(self.source_start_ms)
    }
}

/// The ordered list of segments an editor session produces. Segments never
/// overlap, are never empty, and may appear in any order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Timeline {
    pub segments: Vec<Segment>,
}

impl Timeline {
    /// The untouched timeline: one segment spanning the whole source.
    pub fn whole(duration_ms: u64) -> Timeline {
        if duration_ms == 0 {
            return Timeline { segments: Vec::new() };
        }
        Timeline {
            segments: vec![Segment { source_start_ms: 0, source_end_ms: duration_ms }],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn output_duration_ms(&self) -> u64 {
        self.segments.iter().map(Segment::duration_ms).sum()
    }

    /// Split the segment containing `output_ms` into two at that point.
    ///
    /// A split landing exactly on a segment boundary (including 0 and the
    /// end) is a NO-OP. Allowing it would mint a zero-length segment, which
    /// the exporter cannot encode.
    pub fn split_at(&self, output_ms: u64) -> Timeline {
        let mut elapsed = 0u64;
        let mut out = Vec::with_capacity(self.segments.len() + 1);
        let mut done = false;
        for seg in &self.segments {
            let seg_end = elapsed + seg.duration_ms();
            if !done && output_ms > elapsed && output_ms < seg_end {
                let cut = seg.source_start_ms + (output_ms - elapsed);
                out.push(Segment { source_start_ms: seg.source_start_ms, source_end_ms: cut });
                out.push(Segment { source_start_ms: cut, source_end_ms: seg.source_end_ms });
                done = true;
            } else {
                out.push(*seg);
            }
            elapsed = seg_end;
        }
        Timeline { segments: out }
    }

    /// Remove one segment. Out of range is a no-op.
    pub fn delete(&self, index: usize) -> Timeline {
        if index >= self.segments.len() {
            return self.clone();
        }
        let mut segments = self.segments.clone();
        segments.remove(index);
        Timeline { segments }
    }

    /// Move the segment at `from` to position `to`. Either index out of range
    /// is a no-op.
    pub fn reorder(&self, from: usize, to: usize) -> Timeline {
        if from >= self.segments.len() || to >= self.segments.len() {
            return self.clone();
        }
        let mut segments = self.segments.clone();
        let seg = segments.remove(from);
        segments.insert(to, seg);
        Timeline { segments }
    }

    /// Map a point on the OUTPUT timeline back to a point in the source.
    /// `None` once `output_ms` reaches or passes the output duration — the
    /// span is half-open, so the end itself is not a playable instant.
    pub fn to_source_ms(&self, output_ms: u64) -> Option<u64> {
        let mut elapsed = 0u64;
        for seg in &self.segments {
            let seg_end = elapsed + seg.duration_ms();
            if output_ms < seg_end {
                return Some(seg.source_start_ms + (output_ms - elapsed));
            }
            elapsed = seg_end;
        }
        None
    }

    /// True when this timeline is exactly the whole source, unedited — the
    /// export fast path (spec §8.3), which remuxes instead of re-encoding.
    pub fn is_untouched(&self, source_duration_ms: u64) -> bool {
        matches!(
            self.segments.as_slice(),
            [Segment { source_start_ms: 0, source_end_ms }] if *source_end_ms == source_duration_ms
        )
    }
}
```

Then add the module to `src-tauri/core/src/lib.rs`. The list is alphabetical, so `timeline` goes immediately after `pub mod throttle;` and before `pub mod transcript;`:

```rust
pub mod throttle;
pub mod timeline;
pub mod transcript;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri/core && cargo test timeline`
Expected: PASS — 13 tests in `timeline::tests`.

- [ ] **Step 5: Run the lint gates**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
Expected: no output, exit 0.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/timeline.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): add the screen-capture timeline algebra

The editor's correctness lives here rather than in the Windows-only
engine: no CI runner can record a screen, so split/delete/reorder must
be testable as pure arithmetic on Linux.

Every operation returns a new Timeline, which makes undo/redo a stack of
snapshots instead of a set of inverse operations. A split landing exactly
on a segment boundary is a no-op: minting a zero-length segment there
would reach the exporter and produce an unplayable file."
```

---

### Task 2: DPI scaling and crop clamping (`core::screen_geometry`)

**Files:**
- Create: `src-tauri/core/src/screen_geometry.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod screen_geometry;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `LogicalRect { x: f64, y: f64, width: f64, height: f64 }`, `PhysicalRect { x: u32, y: u32, width: u32, height: u32 }`, `to_physical(rect: LogicalRect, scale: f64) -> PhysicalRect`, `clamp_to_frame(rect: PhysicalRect, frame_w: u32, frame_h: u32) -> Option<PhysicalRect>`. Phase 3's overlay produces `LogicalRect`; Phase 2's capture session consumes the clamped `PhysicalRect`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/screen_geometry.rs` containing ONLY:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn logical(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect { x, y, width, height }
    }

    #[test]
    fn scale_of_one_is_a_straight_cast() {
        let p = to_physical(logical(10.0, 20.0, 300.0, 200.0), 1.0);
        assert_eq!(p, PhysicalRect { x: 10, y: 20, width: 300, height: 200 });
    }

    #[test]
    fn common_dpi_scales_multiply_every_component() {
        assert_eq!(
            to_physical(logical(10.0, 20.0, 300.0, 200.0), 2.0),
            PhysicalRect { x: 20, y: 40, width: 600, height: 400 }
        );
        assert_eq!(
            to_physical(logical(100.0, 100.0, 400.0, 300.0), 1.5),
            PhysicalRect { x: 150, y: 150, width: 600, height: 450 }
        );
        assert_eq!(
            to_physical(logical(0.0, 0.0, 800.0, 600.0), 1.25),
            PhysicalRect { x: 0, y: 0, width: 1000, height: 750 }
        );
    }

    // A fractional scale must never round a dimension down to zero; a
    // zero-width crop is not encodable.
    #[test]
    fn a_tiny_rect_keeps_at_least_one_pixel() {
        let p = to_physical(logical(0.0, 0.0, 0.4, 0.4), 1.0);
        assert_eq!(p.width, 1);
        assert_eq!(p.height, 1);
    }

    #[test]
    fn a_negative_or_nonfinite_input_is_treated_as_zero() {
        let p = to_physical(logical(-5.0, -5.0, 100.0, 100.0), 1.0);
        assert_eq!((p.x, p.y), (0, 0), "negative origin clamps to 0");
        let n = to_physical(logical(f64::NAN, 0.0, f64::INFINITY, 100.0), 1.0);
        assert_eq!(n.x, 0, "NaN origin is 0");
        assert_eq!(n.height, 100);
    }

    #[test]
    fn a_nonpositive_scale_falls_back_to_one() {
        let p = to_physical(logical(10.0, 10.0, 100.0, 100.0), 0.0);
        assert_eq!(p, PhysicalRect { x: 10, y: 10, width: 100, height: 100 });
    }

    #[test]
    fn a_rect_inside_the_frame_is_unchanged() {
        let r = PhysicalRect { x: 10, y: 10, width: 100, height: 100 };
        assert_eq!(clamp_to_frame(r, 1920, 1080), Some(r));
    }

    // Regression: the monitor's resolution can change between selecting a
    // region and starting the capture. Without re-clamping at start, the
    // stale rectangle indexes outside the frame buffer (spec §5.2).
    #[test]
    fn a_rect_overflowing_the_frame_is_truncated() {
        let r = PhysicalRect { x: 1800, y: 1000, width: 400, height: 400 };
        assert_eq!(
            clamp_to_frame(r, 1920, 1080),
            Some(PhysicalRect { x: 1800, y: 1000, width: 120, height: 80 })
        );
    }

    #[test]
    fn a_rect_fully_outside_the_frame_is_none() {
        let r = PhysicalRect { x: 5000, y: 5000, width: 100, height: 100 };
        assert_eq!(clamp_to_frame(r, 1920, 1080), None, "empty intersection");
    }

    #[test]
    fn a_rect_starting_exactly_at_the_frame_edge_is_none() {
        let r = PhysicalRect { x: 1920, y: 0, width: 100, height: 100 };
        assert_eq!(clamp_to_frame(r, 1920, 1080), None);
    }

    #[test]
    fn a_zero_sized_rect_or_frame_is_none() {
        assert_eq!(clamp_to_frame(PhysicalRect { x: 0, y: 0, width: 0, height: 10 }, 1920, 1080), None);
        assert_eq!(clamp_to_frame(PhysicalRect { x: 0, y: 0, width: 10, height: 10 }, 0, 1080), None);
    }

    #[test]
    fn the_overlay_to_capture_round_trip_holds_at_150_percent() {
        // The overlay reports CSS pixels; the frame is physical. A 1280x720
        // selection on a 150%-scaled 2560x1440 monitor is 1920x1080 physical.
        let selected = logical(0.0, 0.0, 1280.0, 720.0);
        let physical = to_physical(selected, 1.5);
        assert_eq!(physical, PhysicalRect { x: 0, y: 0, width: 1920, height: 1080 });
        assert_eq!(clamp_to_frame(physical, 2560, 1440), Some(physical), "fits the real frame");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd src-tauri/core && cargo test screen_geometry 2>&1 | head -20`
Expected: FAIL to compile — `cannot find type LogicalRect in this scope`.

- [ ] **Step 3: Write the implementation**

Insert ABOVE the `#[cfg(test)]` block in `src-tauri/core/src/screen_geometry.rs`:

```rust
//! Region-selection geometry: converting the overlay's LOGICAL (CSS pixel)
//! rectangle into the PHYSICAL pixels a captured frame is measured in, and
//! clamping it to the frame that actually arrived.
//!
//! These are two different coordinate spaces and confusing them is the
//! region feature's most likely bug, so the conversion is one pure function
//! tested across the DPI scales Windows actually ships (spec §5.2).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Non-finite or negative values read as 0 — the overlay is a webview and a
/// NaN from a pointer event must degrade, never panic or wrap.
fn non_negative(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        0.0
    }
}

/// Scale a logical rectangle into physical pixels.
///
/// Width and height are floored to at least 1: a sub-pixel selection that
/// rounded to zero would be an unencodable crop.
pub fn to_physical(rect: LogicalRect, scale: f64) -> PhysicalRect {
    // A non-positive or non-finite scale means we failed to read the monitor;
    // 1.0 (no scaling) is the safe reading, not 0.
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    PhysicalRect {
        x: (non_negative(rect.x) * scale) as u32,
        y: (non_negative(rect.y) * scale) as u32,
        width: ((non_negative(rect.width) * scale) as u32).max(1),
        height: ((non_negative(rect.height) * scale) as u32).max(1),
    }
}

/// Intersect a rectangle with the frame. `None` when the intersection is
/// empty — a region whose monitor changed resolution, or a zero-sized frame.
///
/// This runs at capture START, not only at selection time: the resolution can
/// change in between, and an unclamped stale rectangle indexes outside the
/// frame buffer.
pub fn clamp_to_frame(rect: PhysicalRect, frame_w: u32, frame_h: u32) -> Option<PhysicalRect> {
    if frame_w == 0 || frame_h == 0 || rect.width == 0 || rect.height == 0 {
        return None;
    }
    if rect.x >= frame_w || rect.y >= frame_h {
        return None;
    }
    let width = rect.width.min(frame_w - rect.x);
    let height = rect.height.min(frame_h - rect.y);
    if width == 0 || height == 0 {
        return None;
    }
    Some(PhysicalRect { x: rect.x, y: rect.y, width, height })
}
```

Add to `src-tauri/core/src/lib.rs`. Alphabetically `screen_*` sorts BEFORE `search` (`scr` < `sea`), so it goes between `pub mod recordings;` and `pub mod search;`:

```rust
pub mod recordings;
pub mod screen_geometry;
pub mod search;
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd src-tauri/core && cargo test screen_geometry`
Expected: PASS — 11 tests.

- [ ] **Step 5: Run the lint gates**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
Expected: no output, exit 0.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/screen_geometry.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): add region-selection DPI and crop geometry

The overlay reports logical CSS pixels; captured frames are physical
pixels. Confusing the two is the region feature's most likely bug, so
the conversion is one pure function tested at 1.0/1.25/1.5/2.0.

clamp_to_frame exists because a monitor can change resolution between
selecting a region and starting the capture; an unclamped stale
rectangle would index outside the frame buffer."
```

---

### Task 3: Per-vault screen-capture config (`core::screen_capture_config` + `vault_config`)

**Files:**
- Create: `src-tauri/core/src/screen_capture_config.rs`
- Modify: `src-tauri/core/src/vault_config.rs` (struct fields, `Default`, `vault_entry`, `serialize_vault_entry`, accessor)
- Modify: `src-tauri/core/src/config_merge.rs` (preserve the new fields)
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod screen_capture_config;`)

**Interfaces:**
- Consumes: nothing.
- Produces: `ScreenQuality` (`Low`/`Balanced`/`High`) with `as_key(&self) -> &'static str`, `from_key(&str) -> Option<ScreenQuality>`, `bits_per_pixel(&self) -> f64`; free functions `bitrate_bps(quality: ScreenQuality, width: u32, height: u32, fps: u32) -> u32` and `normalize_fps(fps: u32) -> u32`; `DEFAULT_SCREEN_FOLDER: &str`. On `VaultCaptureConfig`: the seven fields plus `screen_capture_root(&self) -> &str`. Task 4's note renderer and Phase 2's session consume these.

- [ ] **Step 1: Write the failing tests for the new module**

Create `src-tauri/core/src/screen_capture_config.rs` containing ONLY:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quality_keys_round_trip() {
        for q in [ScreenQuality::Low, ScreenQuality::Balanced, ScreenQuality::High] {
            assert_eq!(ScreenQuality::from_key(q.as_key()), Some(q));
        }
    }

    #[test]
    fn an_unknown_quality_key_is_none_so_the_caller_can_default() {
        assert_eq!(ScreenQuality::from_key("ultra"), None);
        assert_eq!(ScreenQuality::from_key(""), None);
    }

    #[test]
    fn quality_is_balanced_by_default() {
        assert_eq!(ScreenQuality::default(), ScreenQuality::Balanced);
    }

    // The presets are bits per pixel per frame (spec §12) rather than a flat
    // bitrate, so a 4K capture is not starved at the same absolute rate as a
    // 720p one.
    #[test]
    fn bitrate_scales_with_area_and_frame_rate() {
        let hd_30 = bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 30);
        let hd_60 = bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 60);
        let uhd_30 = bitrate_bps(ScreenQuality::Balanced, 3840, 2160, 30);
        assert_eq!(hd_60, hd_30 * 2, "doubling fps doubles bitrate");
        assert_eq!(uhd_30, hd_30 * 4, "quadrupling area quadruples bitrate");
    }

    #[test]
    fn bitrate_uses_the_documented_bits_per_pixel_presets() {
        assert_eq!(ScreenQuality::Low.bits_per_pixel(), 0.08);
        assert_eq!(ScreenQuality::Balanced.bits_per_pixel(), 0.15);
        assert_eq!(ScreenQuality::High.bits_per_pixel(), 0.25);
        // 1920*1080*30*0.15 = 9_331_200
        assert_eq!(bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 30), 9_331_200);
    }

    #[test]
    fn higher_presets_produce_higher_bitrates() {
        let low = bitrate_bps(ScreenQuality::Low, 1920, 1080, 30);
        let bal = bitrate_bps(ScreenQuality::Balanced, 1920, 1080, 30);
        let high = bitrate_bps(ScreenQuality::High, 1920, 1080, 30);
        assert!(low < bal && bal < high);
    }

    // A degenerate size must never yield a zero bitrate: the encoder rejects
    // it outright.
    #[test]
    fn a_degenerate_size_still_yields_a_usable_floor() {
        assert!(bitrate_bps(ScreenQuality::Low, 0, 0, 0) >= MIN_BITRATE_BPS);
        assert!(bitrate_bps(ScreenQuality::Low, 2, 2, 1) >= MIN_BITRATE_BPS);
    }

    #[test]
    fn only_30_and_60_fps_are_legal_and_anything_else_is_30() {
        assert_eq!(normalize_fps(30), 30);
        assert_eq!(normalize_fps(60), 60);
        assert_eq!(normalize_fps(0), 30);
        assert_eq!(normalize_fps(24), 30, "a hand-edited config defaults, never errors");
        assert_eq!(normalize_fps(144), 30);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri/core && cargo test screen_capture_config 2>&1 | head -20`
Expected: FAIL to compile — `cannot find type ScreenQuality in this scope`.

- [ ] **Step 3: Implement the module**

Insert ABOVE the `#[cfg(test)]` block in `src-tauri/core/src/screen_capture_config.rs`:

```rust
//! Screen Capture's own per-vault settings vocabulary: the quality preset,
//! its bitrate maths, frame-rate validation, and the default folder name.
//!
//! Lives in its own module (the mcp_config / document_import_config
//! precedent) for LOC headroom; `vault_config` owns the fields themselves.

/// Default per-vault folder for saved screen captures (spec §9).
pub const DEFAULT_SCREEN_FOLDER: &str = "Screen Captures";

/// Floor so a degenerate resolution can never ask the encoder for 0 bps,
/// which it rejects outright.
pub const MIN_BITRATE_BPS: u32 = 200_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScreenQuality {
    Low,
    #[default]
    Balanced,
    High,
}

impl ScreenQuality {
    /// Stable key used in config.json and the IPC DTOs.
    pub fn as_key(&self) -> &'static str {
        match self {
            ScreenQuality::Low => "low",
            ScreenQuality::Balanced => "balanced",
            ScreenQuality::High => "high",
        }
    }

    pub fn from_key(key: &str) -> Option<ScreenQuality> {
        match key {
            "low" => Some(ScreenQuality::Low),
            "balanced" => Some(ScreenQuality::Balanced),
            "high" => Some(ScreenQuality::High),
            _ => None,
        }
    }

    /// Bits per pixel per frame. Exhaustive match: a new preset must decide
    /// this explicitly rather than inherit a default.
    pub fn bits_per_pixel(&self) -> f64 {
        match self {
            ScreenQuality::Low => 0.08,
            ScreenQuality::Balanced => 0.15,
            ScreenQuality::High => 0.25,
        }
    }
}

/// Target video bitrate for a given output size and frame rate.
///
/// Area-relative rather than absolute so a 4K capture is not starved at the
/// same bitrate as a 720p one (spec §12).
pub fn bitrate_bps(quality: ScreenQuality, width: u32, height: u32, fps: u32) -> u32 {
    let pixels = width as f64 * height as f64;
    let raw = pixels * fps as f64 * quality.bits_per_pixel();
    if !raw.is_finite() || raw <= 0.0 {
        return MIN_BITRATE_BPS;
    }
    (raw.min(u32::MAX as f64) as u32).max(MIN_BITRATE_BPS)
}

/// Only 30 and 60 are offered. Anything else — including a hand-edited
/// config.json — reads as 30 rather than erroring, matching the per-field
/// defensive-parse posture of the whole config module.
pub fn normalize_fps(fps: u32) -> u32 {
    match fps {
        60 => 60,
        _ => 30,
    }
}
```

Add to `src-tauri/core/src/lib.rs`, immediately before `pub mod screen_geometry;` (added in Task 2):

```rust
pub mod recordings;
pub mod screen_capture_config;
pub mod screen_geometry;
pub mod search;
```

- [ ] **Step 4: Run to verify the module tests pass**

Run: `cd src-tauri/core && cargo test screen_capture_config`
Expected: PASS — 8 tests.

- [ ] **Step 5: Write the failing config-plumbing tests**

Append these tests INSIDE the existing `#[cfg(test)] mod tests` block at the bottom of `src-tauri/core/src/vault_config.rs`:

```rust
    #[test]
    fn screen_capture_defaults_are_flat_balanced_30_with_a_note() {
        let v = VaultCaptureConfig::default();
        assert_eq!(v.screen_capture_folder, None);
        assert_eq!(v.screen_capture_root(), "Screen Captures");
        assert!(!v.screen_capture_date_folders, "flat is the default layout");
        assert_eq!(v.screen_quality, crate::screen_capture_config::ScreenQuality::Balanced);
        assert_eq!(v.screen_fps, 30);
        assert!(v.screen_create_note);
        assert_eq!(v.screen_extra_frontmatter, None);
        assert_eq!(v.screen_body_template, None);
    }

    #[test]
    fn screen_capture_fields_parse() {
        let entry = serde_json::json!({
            "screenCaptureFolder": "Demos",
            "screenCaptureDateFolders": true,
            "screenQuality": "high",
            "screenFps": 60,
            "screenCreateNote": false,
            "screenExtraFrontmatter": "project: acme",
            "screenBodyTemplate": "## Notes"
        });
        let v = vault_entry(&entry);
        assert_eq!(v.screen_capture_folder.as_deref(), Some("Demos"));
        assert_eq!(v.screen_capture_root(), "Demos");
        assert!(v.screen_capture_date_folders);
        assert_eq!(v.screen_quality, crate::screen_capture_config::ScreenQuality::High);
        assert_eq!(v.screen_fps, 60);
        assert!(!v.screen_create_note);
        assert_eq!(v.screen_extra_frontmatter.as_deref(), Some("project: acme"));
        assert_eq!(v.screen_body_template.as_deref(), Some("## Notes"));
    }

    // Per-field defensive parse: one malformed value defaults ONLY itself.
    // A derived deserializer would reject the whole entry and silently reset
    // every other setting in the vault.
    #[test]
    fn malformed_screen_fields_default_locally_not_globally() {
        let entry = serde_json::json!({
            "screenQuality": "ultra",
            "screenFps": 144,
            "screenCaptureDateFolders": "yes",
            "screenCaptureFolder": "Demos"
        });
        let v = vault_entry(&entry);
        assert_eq!(v.screen_quality, crate::screen_capture_config::ScreenQuality::Balanced);
        assert_eq!(v.screen_fps, 30);
        assert!(!v.screen_capture_date_folders);
        assert_eq!(v.screen_capture_folder.as_deref(), Some("Demos"), "the valid sibling survives");
    }

    #[test]
    fn screen_capture_fields_round_trip_through_serialize() {
        let entry = serde_json::json!({
            "screenCaptureFolder": "Demos",
            "screenCaptureDateFolders": true,
            "screenQuality": "low",
            "screenFps": 60,
            "screenCreateNote": false,
            "screenExtraFrontmatter": "project: acme",
            "screenBodyTemplate": "## Notes"
        });
        let v = vault_entry(&entry);
        let round_tripped = vault_entry(&serde_json::Value::Object(serialize_vault_entry(&v)));
        assert_eq!(round_tripped, v);
    }

    // Regression: an existing config.json must not gain keys just because the
    // app learned about screen capture. Defaults are omitted, matching how
    // every other optional field is serialized.
    #[test]
    fn default_screen_fields_emit_no_keys() {
        let entry = serialize_vault_entry(&VaultCaptureConfig::default());
        for key in [
            "screenCaptureFolder",
            "screenCaptureDateFolders",
            "screenQuality",
            "screenFps",
            "screenCreateNote",
            "screenExtraFrontmatter",
            "screenBodyTemplate",
        ] {
            assert!(!entry.contains_key(key), "{key} should be omitted at its default");
        }
    }
```

- [ ] **Step 6: Run to verify they fail**

Run: `cd src-tauri/core && cargo test vault_config 2>&1 | head -20`
Expected: FAIL to compile — `no field screen_capture_folder on type VaultCaptureConfig`.

- [ ] **Step 7: Add the fields to the struct and its Default**

In `src-tauri/core/src/vault_config.rs`, add to the END of the `pub struct VaultCaptureConfig { ... }` field list (before the closing brace):

```rust
    /// Screen Capture (spec §12). Additive: every field defaults so an
    /// existing config.json parses and re-serializes unchanged.
    pub screen_capture_folder: Option<String>,
    pub screen_capture_date_folders: bool,
    pub screen_quality: crate::screen_capture_config::ScreenQuality,
    /// Only 30 or 60; anything else normalizes to 30 at parse time.
    pub screen_fps: u32,
    pub screen_create_note: bool,
    pub screen_extra_frontmatter: Option<String>,
    pub screen_body_template: Option<String>,
```

In the `impl Default for VaultCaptureConfig` body, add to the end of the struct literal:

```rust
            screen_capture_folder: None,
            screen_capture_date_folders: false,
            screen_quality: crate::screen_capture_config::ScreenQuality::Balanced,
            screen_fps: 30,
            screen_create_note: true,
            screen_extra_frontmatter: None,
            screen_body_template: None,
```

Add the accessor next to the existing `documents_root` method, inside `impl VaultCaptureConfig`:

```rust
    /// The vault's screen-capture folder, defaulting to "Screen Captures".
    /// Mirrors `documents_root` / `tasks_root`.
    pub fn screen_capture_root(&self) -> &str {
        self.screen_capture_folder
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(crate::screen_capture_config::DEFAULT_SCREEN_FOLDER)
    }
```

- [ ] **Step 8: Add parsing and serialization**

In `vault_entry`, add to the end of the returned `VaultCaptureConfig { ... }` literal:

```rust
        screen_capture_folder: entry
            .get("screenCaptureFolder")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        screen_capture_date_folders: entry
            .get("screenCaptureDateFolders")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.screen_capture_date_folders),
        screen_quality: entry
            .get("screenQuality")
            .and_then(|v| v.as_str())
            .and_then(crate::screen_capture_config::ScreenQuality::from_key)
            .unwrap_or(defaults.screen_quality),
        screen_fps: entry
            .get("screenFps")
            .and_then(|v| v.as_u64())
            .map(|v| crate::screen_capture_config::normalize_fps(v as u32))
            .unwrap_or(defaults.screen_fps),
        screen_create_note: entry
            .get("screenCreateNote")
            .and_then(|v| v.as_bool())
            .unwrap_or(defaults.screen_create_note),
        screen_extra_frontmatter: template_field(entry, "screenExtraFrontmatter"),
        screen_body_template: template_field(entry, "screenBodyTemplate"),
```

In `serialize_vault_entry`, add before the final `entry` return (defaults omitted, matching the surrounding style):

```rust
    if let Some(folder) = &v.screen_capture_folder {
        entry.insert("screenCaptureFolder".to_string(), json!(folder));
    }
    if v.screen_capture_date_folders {
        entry.insert("screenCaptureDateFolders".to_string(), json!(true));
    }
    if v.screen_quality != crate::screen_capture_config::ScreenQuality::default() {
        entry.insert("screenQuality".to_string(), json!(v.screen_quality.as_key()));
    }
    if v.screen_fps != 30 {
        entry.insert("screenFps".to_string(), json!(v.screen_fps));
    }
    if !v.screen_create_note {
        entry.insert("screenCreateNote".to_string(), json!(false));
    }
    if let Some(t) = &v.screen_extra_frontmatter {
        entry.insert("screenExtraFrontmatter".to_string(), json!(t));
    }
    if let Some(t) = &v.screen_body_template {
        entry.insert("screenBodyTemplate".to_string(), json!(t));
    }
```

- [ ] **Step 9: Run to verify the config tests pass**

Run: `cd src-tauri/core && cargo test vault_config`
Expected: PASS — all existing tests plus the 5 new ones.

- [ ] **Step 10: Write the failing cross-surface preservation test**

Append INSIDE the existing `#[cfg(test)] mod tests` block of `src-tauri/core/src/config_merge.rs`:

```rust
    // Each settings surface owns its own fields. `merge_capture_owned` fills
    // unlisted fields from `..incoming`, so a field it does not name
    // explicitly is taken from whatever the capture settings card sent —
    // which for the screen fields means a capture save silently resets
    // them. They must be preserved by name.
    #[test]
    fn a_capture_save_preserves_the_screen_fields() {
        use crate::screen_capture_config::ScreenQuality;
        let existing = VaultCaptureConfig {
            screen_capture_folder: Some("Demos".into()),
            screen_capture_date_folders: true,
            screen_quality: ScreenQuality::High,
            screen_fps: 60,
            screen_create_note: false,
            screen_extra_frontmatter: Some("project: acme".into()),
            screen_body_template: Some("## Notes".into()),
            ..VaultCaptureConfig::default()
        };
        // The capture settings card knows nothing about screen capture, so it
        // sends defaults for those fields.
        let incoming = VaultCaptureConfig::default();

        let merged = merge_capture_owned(&existing, incoming);
        assert_eq!(merged.screen_capture_folder.as_deref(), Some("Demos"));
        assert!(merged.screen_capture_date_folders);
        assert_eq!(merged.screen_quality, ScreenQuality::High);
        assert_eq!(merged.screen_fps, 60);
        assert!(!merged.screen_create_note);
        assert_eq!(merged.screen_extra_frontmatter.as_deref(), Some("project: acme"));
        assert_eq!(merged.screen_body_template.as_deref(), Some("## Notes"));
    }

    // merge_documents_owned builds from `..existing.clone()`, so the screen
    // fields are preserved by construction. This pins that, so a future
    // refactor to `..incoming` cannot silently break it.
    #[test]
    fn a_documents_save_preserves_the_screen_fields() {
        use crate::screen_capture_config::ScreenQuality;
        let existing = VaultCaptureConfig {
            screen_capture_folder: Some("Demos".into()),
            screen_quality: ScreenQuality::High,
            screen_fps: 60,
            ..VaultCaptureConfig::default()
        };
        let merged = merge_documents_owned(&existing, Some("Docs".into()), true, true, None, None);
        assert_eq!(merged.screen_capture_folder.as_deref(), Some("Demos"));
        assert_eq!(merged.screen_quality, ScreenQuality::High);
        assert_eq!(merged.screen_fps, 60);
        assert_eq!(merged.documents_folder.as_deref(), Some("Docs"), "the owned field still writes");
    }
```

- [ ] **Step 11: Run to verify the capture test fails, then preserve the fields**

Run: `cd src-tauri/core && cargo test config_merge`
Expected: `a_documents_save_preserves_the_screen_fields` PASSES (it builds from `..existing.clone()`), and `a_capture_save_preserves_the_screen_fields` FAILS — `assertion failed: left: None, right: Some("Demos")`.

That failure is the real bug: `merge_capture_owned` ends in `..incoming`, so any field it does not name explicitly is taken from the incoming capture payload. Fix it by naming the seven screen fields alongside the other preserved groups. In `src-tauri/core/src/config_merge.rs`, inside `merge_capture_owned`'s struct literal, add immediately before `..incoming`:

```rust
        // Screen Capture is owned by set_screen_capture_config (phase 6).
        // These MUST be listed: `..incoming` would otherwise take them from
        // the capture settings payload, which knows nothing about them, and
        // every capture save would reset the vault's screen settings.
        screen_capture_folder: existing.screen_capture_folder.clone(),
        screen_capture_date_folders: existing.screen_capture_date_folders,
        screen_quality: existing.screen_quality,
        screen_fps: existing.screen_fps,
        screen_create_note: existing.screen_create_note,
        screen_extra_frontmatter: existing.screen_extra_frontmatter.clone(),
        screen_body_template: existing.screen_body_template.clone(),
```

Also extend that function's doc comment, which enumerates the preserved groups, by appending to the parenthetical list before the closing paren:

```
/// and `set_screen_capture_config`: screen_capture_folder/
/// screen_capture_date_folders/screen_quality/screen_fps/screen_create_note/
/// screen_extra_frontmatter/screen_body_template
```

Re-run: `cd src-tauri/core && cargo test config_merge`
Expected: PASS — both new tests plus the existing ones.

- [ ] **Step 12: Run the full gates**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test && cd .. && cargo fmt --check`
Expected: all tests pass, no clippy warnings, no fmt diff.

- [ ] **Step 13: Commit**

```bash
git add src-tauri/core/src/screen_capture_config.rs src-tauri/core/src/vault_config.rs src-tauri/core/src/config_merge.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): add the per-vault screen-capture settings

Seven additive fields plus their own vocabulary module (the mcp_config /
document_import_config precedent, for LOC headroom).

Two properties the tests pin. Parsing is per-field defensive, so one
hand-edited bad value defaults only itself rather than resetting the
whole vault entry. And serialization omits defaults, so an existing
config.json does not gain seven keys merely because the app learned
about screen capture.

Bitrate is bits-per-pixel-per-frame rather than absolute, so a 4K
capture is not starved at the same rate as a 720p one.

merge_capture_owned must name the new fields explicitly: it ends in
..incoming, so anything it does not list is taken from the capture
settings payload — which knows nothing about screen capture — and every
capture save would silently reset the vault's screen settings."
```

---

### Task 4: The companion-note renderer (`core::screen_note`)

**Files:**
- Create: `src-tauri/core/src/screen_note.rs`
- Modify: `src-tauri/core/src/lib.rs` (add `pub mod screen_note;`)

**Interfaces:**
- Consumes: `core::yaml_scalar::yaml_quote`, `core::capture_note::format_duration`, `core::template::{substitute, render_extra_frontmatter}` (all existing).
- Produces: `ScreenNoteMeta { recorded_at: String, duration_secs: u64, vault_name: String, source: String, input_devices: Vec<String>, width: u32, height: u32, extra_frontmatter: Option<String>, body_template: Option<String> }`, `RESERVED_SCREEN_NOTE_KEYS: &[&str]`, `render_screen_note(meta: &ScreenNoteMeta, mp4_file_name: &str) -> String`. Phase 5's vault write consumes it.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/core/src/screen_note.rs` containing ONLY:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn meta() -> ScreenNoteMeta {
        ScreenNoteMeta {
            recorded_at: "2026-09-18 14:32".into(),
            duration_secs: 197,
            vault_name: "Engineering".into(),
            source: "Figma — Design System".into(),
            input_devices: vec!["Microphone (Yeti)".into()],
            width: 1920,
            height: 1080,
            extra_frontmatter: None,
            body_template: None,
        }
    }

    #[test]
    fn renders_the_managed_frontmatter_and_the_embed() {
        let out = render_screen_note(&meta(), "2026-09-18 1432 Figma walkthrough.mp4");
        assert_eq!(
            out,
            concat!(
                "---\n",
                "type: \"Screen Capture\"\n",
                "recorded: \"2026-09-18 14:32\"\n",
                "duration: \"3:17\"\n",
                "source: \"Figma — Design System\"\n",
                "inputs:\n",
                "  - \"Microphone (Yeti)\"\n",
                "resolution: \"1920x1080\"\n",
                "vault: \"Engineering\"\n",
                "created-by: Vault Buddy\n",
                "---\n",
                "\n",
                "![[2026-09-18 1432 Figma walkthrough.mp4]]\n",
            )
        );
    }

    #[test]
    fn a_silent_capture_emits_an_empty_inputs_list() {
        let mut m = meta();
        m.input_devices = vec![];
        let out = render_screen_note(&m, "x.mp4");
        assert!(out.contains("inputs: []\n"), "flow-style empty list, never a dangling key");
    }

    // A window title is user data and can contain YAML metacharacters. An
    // unquoted colon or backslash produces a malformed frontmatter block that
    // Obsidian refuses to parse.
    #[test]
    fn a_hostile_source_title_is_quoted_and_escaped() {
        let mut m = meta();
        m.source = "C:\\Users\\a: \"weird\" title".into();
        let out = render_screen_note(&m, "x.mp4");
        assert!(
            out.contains(r#"source: "C:\\Users\\a: \"weird\" title""#),
            "got: {out}"
        );
    }

    #[test]
    fn extra_frontmatter_is_injected_after_created_by() {
        let mut m = meta();
        m.extra_frontmatter = Some("project: acme".into());
        let out = render_screen_note(&m, "x.mp4");
        let created = out.find("created-by: Vault Buddy").expect("created-by present");
        let project = out.find("project: acme").expect("extra frontmatter present");
        assert!(created < project, "extra frontmatter follows the managed keys");
        assert!(project < out.find("---\n\n![[").expect("closing fence"), "and precedes the fence");
    }

    // A user template must never be able to redefine a managed key: the
    // writer and every reader would then disagree about the note's identity.
    #[test]
    fn a_template_cannot_override_a_managed_key() {
        let mut m = meta();
        m.extra_frontmatter = Some("type: Note\nvault: Other\nproject: acme".into());
        let out = render_screen_note(&m, "x.mp4");
        assert!(out.contains("type: \"Screen Capture\""));
        assert!(!out.contains("type: Note"), "reserved key dropped");
        assert!(!out.contains("vault: Other"), "reserved key dropped");
        assert!(out.contains("project: acme"), "non-reserved key survives");
        assert_eq!(out.matches("vault:").count(), 1, "exactly one vault key");
    }

    #[test]
    fn template_placeholders_resolve() {
        let mut m = meta();
        m.extra_frontmatter = Some("title: \"{{source}}\"\nlen: \"{{duration}}\"".into());
        m.body_template = Some("Recorded {{date}} from {{source}} ({{duration}}).".into());
        let out = render_screen_note(&m, "x.mp4");
        assert!(out.contains("Figma — Design System"), "source placeholder resolved");
        assert!(out.contains("Recorded 2026-09-18 from Figma — Design System (3:17)."));
    }

    #[test]
    fn a_body_template_follows_the_embed() {
        let mut m = meta();
        m.body_template = Some("## Notes".into());
        let out = render_screen_note(&m, "x.mp4");
        let embed = out.find("![[x.mp4]]").expect("embed present");
        assert!(embed < out.find("## Notes").expect("body present"));
        assert!(out.ends_with('\n'), "file ends with a newline");
    }

    // Regression: an empty or unset template must reproduce the template-free
    // output byte-for-byte, so a vault that never opts in sees no change.
    #[test]
    fn blank_templates_are_byte_identical_to_none() {
        let baseline = render_screen_note(&meta(), "x.mp4");
        let mut m = meta();
        m.extra_frontmatter = Some("   ".into());
        m.body_template = Some("".into());
        assert_eq!(render_screen_note(&m, "x.mp4"), baseline);
    }

    #[test]
    fn malformed_template_yaml_degrades_to_nothing_and_never_breaks_the_fence() {
        let mut m = meta();
        m.extra_frontmatter = Some("this: is: not: valid: yaml: [".into());
        let out = render_screen_note(&m, "x.mp4");
        assert_eq!(out.matches("---\n").count(), 2, "exactly two fence lines");
        assert!(out.contains("![[x.mp4]]"));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd src-tauri/core && cargo test screen_note 2>&1 | head -20`
Expected: FAIL to compile — `cannot find type ScreenNoteMeta in this scope`.

- [ ] **Step 3: Write the implementation**

Insert ABOVE the `#[cfg(test)]` block in `src-tauri/core/src/screen_note.rs`:

```rust
//! The Screen Capture companion note: managed frontmatter, the video embed,
//! and the vault's additive template content around them.
//!
//! Mirrors `capture_note::render_note` exactly in discipline — the managed
//! keys are always emitted and are never user-removable, and the vault's
//! extra frontmatter is filtered against them before injection, so a template
//! can neither break the fence nor shadow a key the writer depends on
//! (spec §9.1).

use crate::capture_note::format_duration;
use crate::template::{render_extra_frontmatter, substitute};
use crate::yaml_scalar::yaml_quote;

/// Frontmatter keys this renderer owns. A user template naming one of these
/// has that key dropped rather than honoured.
pub const RESERVED_SCREEN_NOTE_KEYS: &[&str] = &[
    "type",
    "recorded",
    "duration",
    "source",
    "inputs",
    "resolution",
    "vault",
    "created-by",
];

#[derive(Debug, Clone)]
pub struct ScreenNoteMeta {
    pub recorded_at: String,
    pub duration_secs: u64,
    pub vault_name: String,
    /// The window title or screen label that was captured.
    pub source: String,
    pub input_devices: Vec<String>,
    pub width: u32,
    pub height: u32,
    /// Additive per-vault template content. None → the template-free output.
    pub extra_frontmatter: Option<String>,
    pub body_template: Option<String>,
}

pub fn render_screen_note(meta: &ScreenNoteMeta, mp4_file_name: &str) -> String {
    let duration = format_duration(meta.duration_secs);
    let resolution = format!("{}x{}", meta.width, meta.height);

    let mut out = String::from("---\n");
    out.push_str("type: \"Screen Capture\"\n");
    out.push_str(&format!("recorded: {}\n", yaml_quote(&meta.recorded_at)));
    out.push_str(&format!("duration: {}\n", yaml_quote(&duration)));
    out.push_str(&format!("source: {}\n", yaml_quote(&meta.source)));
    if meta.input_devices.is_empty() {
        // A silent capture is legal (spec §7.2). Flow style, so the key is
        // never left dangling with no list under it.
        out.push_str("inputs: []\n");
    } else {
        out.push_str("inputs:\n");
        for device in &meta.input_devices {
            out.push_str(&format!("  - {}\n", yaml_quote(device)));
        }
    }
    out.push_str(&format!("resolution: {}\n", yaml_quote(&resolution)));
    out.push_str(&format!("vault: {}\n", yaml_quote(&meta.vault_name)));
    out.push_str("created-by: Vault Buddy\n");

    let date = meta.recorded_at.split(['T', ' ']).next().unwrap_or("");
    let vars = [
        ("recordedAt", meta.recorded_at.as_str()),
        ("date", date),
        ("duration", duration.as_str()),
        ("source", meta.source.as_str()),
        ("resolution", resolution.as_str()),
        ("vault", meta.vault_name.as_str()),
    ];

    if let Some(template) = &meta.extra_frontmatter {
        let extra = render_extra_frontmatter(template, &vars, RESERVED_SCREEN_NOTE_KEYS);
        if !extra.is_empty() {
            out.push_str(&extra);
            if !out.ends_with('\n') {
                out.push('\n');
            }
        }
    }

    out.push_str("---\n\n");
    out.push_str(&format!("![[{mp4_file_name}]]\n"));

    if let Some(template) = &meta.body_template {
        let body = substitute(template, &vars);
        if !body.trim().is_empty() {
            out.push('\n');
            out.push_str(body.trim_end());
            out.push('\n');
        }
    }

    out
}
```

Add to `src-tauri/core/src/lib.rs`, immediately after `pub mod screen_geometry;`. The finished `screen_*` group reads:

```rust
pub mod screen_capture_config;
pub mod screen_geometry;
pub mod screen_note;
```

- [ ] **Step 4: Run to verify the tests pass**

Run: `cd src-tauri/core && cargo test screen_note`
Expected: PASS — 9 tests.

If `a_hostile_source_title_is_quoted_and_escaped` fails, read `yaml_scalar::yaml_quote`'s actual escaping and correct the test's expected string to match the real implementation — `yaml_quote` is the authority here, not the test.

- [ ] **Step 5: Run the gates**

Run: `cd src-tauri/core && cargo clippy --all-targets -- -D warnings && cargo test && cd .. && cargo fmt --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/core/src/screen_note.rs src-tauri/core/src/lib.rs
git commit -m "feat(core): add the screen-capture companion-note renderer

Same discipline as the capture note: managed keys are always emitted and
never user-removable, and the vault's extra frontmatter is filtered
against them before injection, so a template can neither break the fence
nor shadow a key the writer depends on.

A window title is user data that can carry colons, quotes and
backslashes, so every value goes through yaml_quote; an unquoted one
produces frontmatter Obsidian refuses to parse. A silent capture emits
inputs: [] rather than a dangling key."
```

---

### Task 5: N-source audio mixing (`capture::mixer`)

**Files:**
- Modify: `src-tauri/capture/src/mixer.rs`

**Interfaces:**
- Consumes: the existing `soft_clip`.
- Produces: `mix_n_to_stereo_i16(sources: &[&[f32]]) -> Vec<i16>`. `mix_to_stereo_i16` keeps its signature and becomes a two-source wrapper. Phase 2's capture session consumes the N-source form.

- [ ] **Step 1: Write the failing tests**

Append INSIDE the existing `#[cfg(test)] mod tests` block in `src-tauri/capture/src/mixer.rs`:

```rust
    #[test]
    fn mix_n_with_no_sources_is_silence() {
        // Zero selected audio devices is legal — a silent UI demo is a real
        // use case (spec §7.2), so this must be empty output, not a panic.
        assert!(mix_n_to_stereo_i16(&[]).is_empty());
    }

    #[test]
    fn mix_n_with_one_source_duplicates_it_across_both_channels() {
        let out = mix_n_to_stereo_i16(&[&[0.5, -0.5]]);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], out[1], "L == R");
        assert_eq!(out[2], out[3]);
        assert!((out[0] as f32 / i16::MAX as f32 - (0.5f32).tanh()).abs() < 0.001);
    }

    // Regression: the two-source path is the existing meeting-recording
    // behaviour and must stay byte-identical, or every meeting recording's
    // levels change the day multi-select lands.
    #[test]
    fn mix_n_matches_the_two_source_mixer_exactly() {
        let cases: Vec<(Vec<f32>, Vec<f32>)> = vec![
            (vec![0.5, 0.5], vec![0.25]),
            (vec![], vec![0.1, 0.2, 0.3]),
            (vec![0.9, -0.9, 0.4], vec![0.9, -0.9, 0.4]),
            (vec![], vec![]),
        ];
        for (a, b) in cases {
            assert_eq!(
                mix_n_to_stereo_i16(&[&a, &b]),
                mix_to_stereo_i16(&a, &b),
                "a={a:?} b={b:?}"
            );
        }
    }

    #[test]
    fn mix_n_pads_every_shorter_source_with_silence() {
        let out = mix_n_to_stereo_i16(&[&[0.1, 0.1, 0.1], &[0.1], &[0.1, 0.1]]);
        assert_eq!(out.len(), 6, "3 frames * 2 channels — the longest source wins");
        let third = out[4] as f32 / i16::MAX as f32;
        assert!((third - (0.1f32).tanh()).abs() < 0.001, "only the long source contributes");
    }

    #[test]
    fn mix_n_soft_clips_a_summed_overload_instead_of_wrapping() {
        // Five hot sources sum to 4.0; without soft_clip this wraps to a
        // negative i16 and the audio is destroyed.
        let hot = [0.8f32];
        let sources: Vec<&[f32]> = vec![&hot; 5];
        let out = mix_n_to_stereo_i16(&sources);
        assert_eq!(out.len(), 2);
        assert!(out[0] > 0, "stays positive");
        assert!(out[0] <= i16::MAX);
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `cd src-tauri/capture && cargo test mixer 2>&1 | head -20`
Expected: FAIL to compile — `cannot find function mix_n_to_stereo_i16 in this scope`.

Note for Linux: the `capture` crate needs ALSA headers. If the build fails with `alsa` not found, run `npm run setup:linux` from the repo root first.

- [ ] **Step 3: Write the implementation**

In `src-tauri/capture/src/mixer.rs`, REPLACE the existing `mix_to_stereo_i16` function with:

```rust
/// Sum N mono sources into one interleaved stereo i16 buffer.
///
/// Shorter sources are silence-padded to the longest. No per-source gain
/// normalisation is applied: dividing by N would change the levels of every
/// existing two-source meeting recording. `soft_clip` already bounds a
/// summed overload, which is the behaviour the audio domain shipped with.
///
/// Zero sources yields an empty buffer — a silent capture is legal.
pub fn mix_n_to_stereo_i16(sources: &[&[f32]]) -> Vec<i16> {
    let frames = sources.iter().map(|s| s.len()).max().unwrap_or(0);
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let sum: f32 = sources.iter().map(|s| s.get(i).copied().unwrap_or(0.0)).sum();
        let sample = (soft_clip(sum) * i16::MAX as f32) as i16;
        out.push(sample);
        out.push(sample);
    }
    out
}

/// The two-source case, kept as the name the existing audio-capture session
/// calls. A thin wrapper so both paths can never drift apart.
pub fn mix_to_stereo_i16(a: &[f32], b: &[f32]) -> Vec<i16> {
    mix_n_to_stereo_i16(&[a, b])
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cd src-tauri/capture && cargo test mixer`
Expected: PASS — the existing `mix_pads_shorter_side_with_silence_and_interleaves_stereo` plus the 5 new tests.

- [ ] **Step 5: Run the gates**

Run: `cd src-tauri/capture && cargo clippy --all-targets -- -D warnings && cargo test && cd .. && cargo fmt --check`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/capture/src/mixer.rs
git commit -m "feat(capture): generalize the mixer to N sources

Screen capture lets the user multi-select audio devices, but the mixer
took exactly two. mix_n_to_stereo_i16 sums an arbitrary number and
mix_to_stereo_i16 becomes a two-source wrapper over it, so the two paths
cannot drift apart.

No per-source gain normalisation: dividing by N would change the levels
of every existing two-source meeting recording. A regression test pins
the two-source output byte-identical to the previous implementation.
Zero sources is legal and yields silence — a silent UI demo is a real
use case."
```

---

### Task 6: The `screen` crate skeleton, pause clock, and frame planner

**Files:**
- Create: `src-tauri/screen/Cargo.toml`
- Create: `src-tauri/screen/src/lib.rs`
- Create: `src-tauri/screen/src/clock.rs`
- Create: `src-tauri/screen/src/select.rs`
- Create: `src-tauri/screen/src/engine.rs`
- Modify: `src-tauri/Cargo.toml` (workspace members)

**Interfaces:**
- Consumes: `vault_buddy_core::timeline::{Timeline, Segment}` (Task 1).
- Produces: `ScreenError` (`Unsupported`, `SourceGone`, `EncoderUnavailable`, `Io(String)`), `clock::CaptureClock` with `new(started: Instant)`, `pause(&mut self, now: Instant)`, `resume(&mut self, now: Instant)`, `is_paused(&self) -> bool`, `output_ts(&self, now: Instant) -> Option<Duration>`, `elapsed(&self, now: Instant) -> Duration`; `select::{PlanSpan, plan}` where `plan(timeline: &Timeline) -> Vec<PlanSpan>` and `PlanSpan { source_start_ms, source_end_ms, output_start_ms }`; `engine::start_capture(...) -> Result<(), ScreenError>` (stub). Phase 2 fills `engine`.

- [ ] **Step 1: Create the crate manifest and register it in the workspace**

Create `src-tauri/screen/Cargo.toml`:

```toml
[package]
name = "vault_buddy_screen"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
log = "0.4"
vault_buddy_core = { path = "../core" }

# NOTE: windows-capture and the `windows` crate are deliberately NOT here
# yet. They arrive in phase 2, after the fragmented-MP4 spike decides the
# staged container format. Everything in this crate today is pure and
# compiles on any platform.
```

In `src-tauri/Cargo.toml`, change the workspace members line to:

```toml
members = ["core", "capture", "transcribe", "mcp", "screen"]
```

- [ ] **Step 2: Write the failing clock tests**

Create `src-tauri/screen/src/clock.rs` containing ONLY:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    #[test]
    fn output_time_tracks_wall_clock_while_running() {
        let base = Instant::now();
        let c = CaptureClock::new(base);
        assert_eq!(c.output_ts(at(base, 0)), Some(Duration::from_millis(0)));
        assert_eq!(c.output_ts(at(base, 1_500)), Some(Duration::from_millis(1_500)));
        assert!(!c.is_paused());
    }

    #[test]
    fn output_time_is_none_while_paused() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        assert!(c.is_paused());
        assert_eq!(c.output_ts(at(base, 2_000)), None, "nothing is encoded while paused");
    }

    // The invariant the whole pause design rests on: paused wall-clock time
    // never appears in the output timeline. Without this, audio and video
    // drift apart by exactly the pause duration.
    #[test]
    fn paused_time_is_excluded_from_the_output_timeline() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        c.resume(at(base, 6_000)); // paused for 5s
        assert_eq!(c.output_ts(at(base, 7_000)), Some(Duration::from_millis(2_000)));
        assert_eq!(c.elapsed(at(base, 7_000)), Duration::from_millis(2_000));
    }

    #[test]
    fn multiple_pause_cycles_accumulate() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        c.resume(at(base, 2_000)); // +1s paused
        c.pause(at(base, 3_000));
        c.resume(at(base, 5_000)); // +2s paused, 3s total
        assert_eq!(c.output_ts(at(base, 6_000)), Some(Duration::from_millis(3_000)));
    }

    #[test]
    fn a_second_pause_while_already_paused_is_ignored() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.pause(at(base, 1_000));
        c.pause(at(base, 3_000)); // must not reset the pause start
        c.resume(at(base, 5_000));
        assert_eq!(
            c.output_ts(at(base, 6_000)),
            Some(Duration::from_millis(2_000)),
            "4s paused, so 6s wall = 2s output"
        );
    }

    #[test]
    fn a_resume_without_a_pause_is_ignored() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        c.resume(at(base, 1_000));
        assert_eq!(c.output_ts(at(base, 2_000)), Some(Duration::from_millis(2_000)));
    }

    // Timestamps handed to the encoder must never go backwards; a
    // non-monotonic stream is rejected outright by the muxer.
    #[test]
    fn output_time_never_goes_backwards_across_a_pause() {
        let base = Instant::now();
        let mut c = CaptureClock::new(base);
        let before = c.output_ts(at(base, 1_000)).unwrap();
        c.pause(at(base, 1_000));
        c.resume(at(base, 9_000));
        let after = c.output_ts(at(base, 9_001)).unwrap();
        assert!(after >= before, "{after:?} must not precede {before:?}");
    }

    #[test]
    fn a_clock_reading_before_the_start_is_zero_not_a_panic() {
        let base = Instant::now() + Duration::from_secs(10);
        let c = CaptureClock::new(base);
        assert_eq!(c.output_ts(Instant::now()), Some(Duration::ZERO));
    }
}
```

- [ ] **Step 3: Write the failing select tests**

Create `src-tauri/screen/src/select.rs` containing ONLY:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use vault_buddy_core::timeline::{Segment, Timeline};

    #[test]
    fn an_untouched_timeline_plans_one_span_at_output_zero() {
        let spans = plan(&Timeline::whole(5_000));
        assert_eq!(
            spans,
            vec![PlanSpan { source_start_ms: 0, source_end_ms: 5_000, output_start_ms: 0 }]
        );
    }

    #[test]
    fn an_empty_timeline_plans_nothing() {
        assert!(plan(&Timeline::default()).is_empty());
    }

    #[test]
    fn a_deleted_segment_is_absent_and_later_output_shifts_earlier() {
        let t = Timeline::whole(6_000).split_at(2_000).delete(0);
        assert_eq!(
            plan(&t),
            vec![PlanSpan { source_start_ms: 2_000, source_end_ms: 6_000, output_start_ms: 0 }]
        );
    }

    #[test]
    fn reordered_segments_are_planned_in_output_order_with_rewritten_timestamps() {
        let t = Timeline::whole(6_000).split_at(2_000).split_at(4_000).reorder(0, 2);
        assert_eq!(
            plan(&t),
            vec![
                PlanSpan { source_start_ms: 2_000, source_end_ms: 4_000, output_start_ms: 0 },
                PlanSpan { source_start_ms: 4_000, source_end_ms: 6_000, output_start_ms: 2_000 },
                PlanSpan { source_start_ms: 0, source_end_ms: 2_000, output_start_ms: 4_000 },
            ]
        );
    }

    // The exporter writes these timestamps straight into the muxer. A gap or
    // an overlap in output time produces a file that stutters or is rejected.
    #[test]
    fn output_time_is_contiguous_and_monotonic() {
        let t = Timeline::whole(9_000)
            .split_at(2_000)
            .split_at(5_000)
            .reorder(2, 0)
            .reorder(1, 2);
        let spans = plan(&t);
        let mut expected_next = 0u64;
        for s in &spans {
            assert_eq!(s.output_start_ms, expected_next, "contiguous: {spans:?}");
            expected_next += s.duration_ms();
        }
        assert_eq!(expected_next, t.output_duration_ms(), "plan covers the whole output");
    }

    #[test]
    fn every_planned_span_is_non_empty() {
        let t = Timeline::whole(6_000).split_at(3_000);
        for s in plan(&t) {
            assert!(s.source_start_ms < s.source_end_ms, "empty span {s:?} would break the encoder");
        }
    }

    #[test]
    fn plan_skips_a_hand_constructed_empty_segment() {
        // Timeline's own operations never produce one, but plan() is the last
        // gate before the encoder and must not forward it.
        let t = Timeline {
            segments: vec![
                Segment { source_start_ms: 0, source_end_ms: 1_000 },
                Segment { source_start_ms: 2_000, source_end_ms: 2_000 },
                Segment { source_start_ms: 3_000, source_end_ms: 4_000 },
            ],
        };
        let spans = plan(&t);
        assert_eq!(spans.len(), 2, "the zero-length segment is dropped");
        assert_eq!(spans[1].output_start_ms, 1_000, "output time stays contiguous");
    }
}
```

- [ ] **Step 4: Run to verify both fail**

Run: `cd src-tauri/screen && cargo test 2>&1 | head -20`
Expected: FAIL to compile — `cannot find type CaptureClock` / `cannot find function plan`.

- [ ] **Step 5: Implement the clock**

Insert ABOVE the `#[cfg(test)]` block in `src-tauri/screen/src/clock.rs`:

```rust
//! The single source of output timestamps for BOTH the video and audio
//! streams of a screen capture.
//!
//! Sharing one clock is what makes A/V sync across a pause structural rather
//! than incidental: there is no second time base to drift from. Pure and
//! `Instant`-injected, so every pause edge is unit-testable (spec §6.2).

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct CaptureClock {
    started: Instant,
    paused_total: Duration,
    paused_since: Option<Instant>,
}

impl CaptureClock {
    pub fn new(started: Instant) -> CaptureClock {
        CaptureClock { started, paused_total: Duration::ZERO, paused_since: None }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    /// Idempotent: a second pause while already paused must not reset the
    /// pause start, or the first pause's elapsed time is lost.
    pub fn pause(&mut self, now: Instant) {
        if self.paused_since.is_none() {
            self.paused_since = Some(now);
        }
    }

    /// Idempotent: a resume with no matching pause is ignored.
    pub fn resume(&mut self, now: Instant) {
        if let Some(since) = self.paused_since.take() {
            self.paused_total += now.saturating_duration_since(since);
        }
    }

    /// Wall-clock time since start, minus all paused time.
    pub fn elapsed(&self, now: Instant) -> Duration {
        let raw = now.saturating_duration_since(self.started);
        let paused = match self.paused_since {
            Some(since) => self.paused_total + now.saturating_duration_since(since),
            None => self.paused_total,
        };
        raw.saturating_sub(paused)
    }

    /// The timestamp to stamp on a frame or audio buffer arriving `now`.
    /// `None` while paused — arriving data is drained and discarded, so
    /// paused wall-clock time never reaches the output timeline.
    pub fn output_ts(&self, now: Instant) -> Option<Duration> {
        if self.is_paused() {
            return None;
        }
        Some(self.elapsed(now))
    }
}
```

- [ ] **Step 6: Implement the planner**

Insert ABOVE the `#[cfg(test)]` block in `src-tauri/screen/src/select.rs`:

```rust
//! Turn an edited `Timeline` into the ordered list of source spans the
//! exporter reads, each carrying the output timestamp its first frame lands
//! on.
//!
//! Pure, so the export ordering is testable on Linux even though decoding
//! and encoding are Windows-only (spec §8.3).

use vault_buddy_core::timeline::Timeline;

/// One source span plus where it starts on the OUTPUT timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSpan {
    pub source_start_ms: u64,
    pub source_end_ms: u64,
    pub output_start_ms: u64,
}

impl PlanSpan {
    pub fn duration_ms(&self) -> u64 {
        self.source_end_ms.saturating_sub(self.source_start_ms)
    }
}

/// Build the export plan. Output timestamps are contiguous and monotonic by
/// construction — a gap or overlap produces a file that stutters or that the
/// muxer rejects.
///
/// Zero-length segments are dropped. `Timeline`'s own operations never make
/// one, but this is the last gate before the encoder, which cannot encode an
/// empty span.
pub fn plan(timeline: &Timeline) -> Vec<PlanSpan> {
    let mut output_start_ms = 0u64;
    let mut spans = Vec::with_capacity(timeline.segments.len());
    for seg in &timeline.segments {
        if seg.source_start_ms >= seg.source_end_ms {
            continue;
        }
        spans.push(PlanSpan {
            source_start_ms: seg.source_start_ms,
            source_end_ms: seg.source_end_ms,
            output_start_ms,
        });
        output_start_ms += seg.duration_ms();
    }
    spans
}
```

- [ ] **Step 7: Implement the crate root and the platform stub**

Create `src-tauri/screen/src/engine.rs`:

```rust
//! The Windows-only capture engine surface.
//!
//! Phase 1 ships the shape and a stub only: the real implementation
//! (windows-capture frame acquisition + a Media Foundation SinkWriter)
//! arrives in phase 2, after the fragmented-MP4 spike settles the staged
//! container format. The non-Windows arm exists so the Linux compile gate
//! and every pure test in this crate keep running (spec §4.1).

use crate::ScreenError;

/// Begin capturing. Phase 2 replaces this signature with the real parameter
/// set; until then it exists so callers and the stub agree on the shape.
#[cfg(windows)]
pub fn start_capture() -> Result<(), ScreenError> {
    Err(ScreenError::Unsupported)
}

#[cfg(not(windows))]
pub fn start_capture() -> Result<(), ScreenError> {
    Err(ScreenError::Unsupported)
}
```

Create `src-tauri/screen/src/lib.rs`:

```rust
//! Screen Capture engine.
//!
//! What compiles where: `clock` and `select` are pure and build and test on
//! ANY platform — they carry this feature's correctness precisely because no
//! CI runner can record a screen. `engine` is the Windows-only surface, a
//! stub until phase 2.

pub mod clock;
pub mod engine;
pub mod select;

/// Every way a screen capture can fail, as a typed value the shell renders.
/// Stringly-typed errors would let a user-supplied window title reach a UI
/// that expects a fixed set of outcomes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenError {
    /// This platform has no screen-capture engine (non-Windows), or the
    /// engine is not implemented yet.
    Unsupported,
    /// The chosen window or monitor disappeared before capture began.
    SourceGone,
    /// No usable H.264 encoder, or Media Foundation is unavailable.
    EncoderUnavailable,
    Io(String),
}

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScreenError::Unsupported => write!(f, "screen capture is not supported here"),
            ScreenError::SourceGone => write!(f, "the capture source is no longer available"),
            ScreenError::EncoderUnavailable => write!(f, "no usable video encoder was found"),
            ScreenError::Io(e) => write!(f, "screen capture I/O error: {e}"),
        }
    }
}

impl std::error::Error for ScreenError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_engine_stub_reports_unsupported_rather_than_panicking() {
        // The Linux compile gate builds this crate; the stub must degrade,
        // never abort, or CI dies instead of reporting.
        assert_eq!(engine::start_capture(), Err(ScreenError::Unsupported));
    }

    #[test]
    fn errors_render_without_interpolating_caller_data_into_the_variant() {
        assert_eq!(ScreenError::SourceGone.to_string(), "the capture source is no longer available");
        assert!(ScreenError::Io("disk full".into()).to_string().contains("disk full"));
    }
}
```

- [ ] **Step 8: Run to verify everything passes**

Run: `cd src-tauri/screen && cargo test`
Expected: PASS — 8 clock tests, 7 select tests, 2 lib tests.

- [ ] **Step 9: Run the gates**

Run: `cd src-tauri/screen && cargo clippy --all-targets -- -D warnings && cd .. && cargo fmt --check`
Expected: no output, exit 0.

- [ ] **Step 10: Verify the whole workspace still builds**

Run: `cd src-tauri && cargo check -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_screen --all-targets`
Expected: `Finished` with no errors.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/screen src-tauri/Cargo.toml
git commit -m "feat(screen): add the screen crate with its pause clock and frame planner

A fifth workspace crate rather than growing capture: capture compiles and
tests on any platform, and bolting Windows-only D3D11/Media Foundation
into it would make the what-compiles-where table false and take the audio
engine's cross-platform tests hostage.

clock and select are pure and test on Linux — they carry this feature's
correctness because no CI runner can record a screen. engine is the
Windows-only surface, a stub returning Unsupported until phase 2 settles
the staged container format.

CaptureClock is the single time base for BOTH streams, which makes A/V
sync across a pause structural rather than incidental: there is no second
clock to drift from. Pause and resume are idempotent — a double pause
that reset the pause start would silently lose the first interval.

windows-capture is deliberately not a dependency yet; it arrives in
phase 2 after the fragmented-MP4 spike."
```

---

### Task 7: Wire the new crate into CI and document what compiles where

**Files:**
- Modify: `.github/workflows/ci.yml:72,88`
- Modify: `AGENTS.md` (the "what compiles where" table)

**Interfaces:**
- Consumes: the `vault_buddy_screen` crate from Task 6.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Add the crate to the clippy gate**

In `.github/workflows/ci.yml`, line 72, change:

```yaml
        run: cargo clippy -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp --all-targets -- -D warnings
```

to:

```yaml
        run: cargo clippy -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen --all-targets -- -D warnings
```

- [ ] **Step 2: Add the crate to the test gate**

In `.github/workflows/ci.yml`, line 88, change:

```yaml
        run: cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp
```

to:

```yaml
        run: cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen
```

Do NOT add `vault_buddy_screen` to the `cargo llvm-cov` line (106). Its pure modules are well covered, but `engine.rs` is an uncovered stub this phase and would drag the 94% floor down for no signal. It joins the coverage set in phase 2 when the stub becomes real code.

- [ ] **Step 3: Document what compiles where**

In `AGENTS.md`, in the "What compiles where (read this first)" table, add a row after the `src-tauri/mcp/` row:

```markdown
| `src-tauri/screen/` | Screen capture engine: frame acquisition, Media Foundation encode/decode, export. The pure submodules (`clock`, `select`) compile and test anywhere; `engine` is `cfg(windows)` with an `Unsupported` stub. | Anywhere — pure modules test on Linux and CI gates them (`-p vault_buddy_screen`); the engine is Windows-only. |
```

In the same file's Repository map code block, add after the `├── mcp/src/` entry:

```
│   ├── screen/src/             # SCREEN CAPTURE: clock (pause-aware time base),
│   │                           #   select (timeline → frame plan), engine (Windows)
```

- [ ] **Step 4: Verify the CI commands run locally**

Run: `cd src-tauri && cargo clippy -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_screen --all-targets -- -D warnings && cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_screen`
Expected: no clippy warnings; all tests pass.

(The full CI line also includes `transcribe` and `mcp`, which are unchanged by this phase and slower to build. Running the three touched crates is the proportionate local check.)

- [ ] **Step 5: Run the complete gate set one final time**

Run: `cd src-tauri && cargo fmt --check && cargo deny check`
Expected: no fmt diff; `advisories ok, bans ok, licenses ok, sources ok`.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/ci.yml AGENTS.md
git commit -m "ci: gate the screen crate and document what compiles where

The rust-core job now runs clippy and tests for vault_buddy_screen, so
the Linux stub arm and every pure module are exercised on every PR —
which is the whole reason the correctness lives in pure modules.

Deliberately NOT added to the llvm-cov set yet: engine.rs is an
uncovered stub this phase and would drag the 94% floor down for no
signal. It joins in phase 2 when the stub becomes real code."
```

---

## Phase exit criteria

Phase 1 is complete when all of the following hold:

1. `cd src-tauri && cargo fmt --check` — clean.
2. `cd src-tauri && cargo clippy -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen --all-targets -- -D warnings` — clean.
3. `cd src-tauri && cargo test -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe -p vault_buddy_mcp -p vault_buddy_screen` — green.
4. `cd src-tauri && cargo llvm-cov -p vault_buddy_core -p vault_buddy_capture -p vault_buddy_transcribe --fail-under-lines 94` — still passing.
5. `cd src-tauri && cargo deny check` — `advisories ok, bans ok, licenses ok, sources ok`.
6. `npm test && npm run build` — unchanged and green (this phase touches no frontend file).
7. Launching the app shows **no new behaviour**. That is the intended result.

## What phase 2 needs from this phase

Phase 2 opens with the **fragmented-MP4 spike** (spec §6.4) and must not
proceed past it on an assumption. It will consume, unchanged:

- `core::timeline::Timeline` — the staged capture's initial timeline is `Timeline::whole(duration)`.
- `core::screen_geometry::{to_physical, clamp_to_frame}` — re-clamping at capture start.
- `core::screen_capture_config::{ScreenQuality, bitrate_bps, normalize_fps}` — encoder configuration.
- `capture::mixer::mix_n_to_stereo_i16` — the audio path.
- `screen::clock::CaptureClock` — timestamps for both streams.
- `screen::ScreenError` — the typed failure surface.

`screen::engine` is the module phase 2 replaces; its stub signature is a
placeholder and phase 2 is expected to change it.
