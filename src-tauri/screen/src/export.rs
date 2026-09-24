//! Running the export: spawn ffmpeg, stream its progress, cancel by killing
//! it — and the first executable end-to-end proof anywhere in this feature
//! that a cut lands where the editor said it would.
//!
//! Phases 2–4 shipped correctness claims whose only gate was a manual
//! Windows checklist nobody has run (GAP-117). The route change from Media
//! Foundation to a user-installed ffmpeg is what makes this module's tests
//! possible at all: the arguments are pure (`ffmpeg_args`), and here CI
//! synthesizes a clip, applies a real timeline, and reads the OUTPUT back.
//! The round-trip tests below are the deliverable, not decoration.
//!
//! ## The four decisions this module encodes
//!
//! 1. **`is_untouched` is measured against the SIDECAR's duration, never a
//!    probed one.** The editor built the timeline from the sidecar's
//!    `duration_ms`; ffprobe reports a slightly different number for the
//!    same file, because container duration and encoder timing disagree by a
//!    frame or two. Feeding a probed duration in would make an untouched
//!    capture read as edited and silently drop to a full re-encode — slow,
//!    lossy, and invisible. `ExportRequest::source_duration_ms` is the
//!    sidecar's, and a structural test below forbids this module from
//!    probing at all.
//! 2. **Cancellation kills the child and deletes the destination**, and
//! 3. **there is NO export timeout, deliberately** -- both now documented
//!    and implemented once in `ffmpeg_run`, the runner this module shares
//!    with the editor's render (tutorial-editor Task 42).
//! 4. **A refusal is a refusal, not a failed run.** `export_refusal` answers
//!    "can this export run at all" BEFORE a child exists, so the message
//!    names the missing capability instead of surfacing ffmpeg's own text.

use crate::ffmpeg_args::{reencode_args, remux_args, EncodeSettings};
use crate::ffmpeg_run::run;
use crate::select::{plan, plan_output_duration_ms};
use crate::ScreenError;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use vault_buddy_core::timeline::Timeline;

/// Everything the runner needs. Borrowed rather than owned because the
/// caller (the `screen-export` worker) already holds all of it.
pub struct ExportRequest<'a> {
    pub ffmpeg: &'a Path,
    pub source: &'a Path,
    pub dest: &'a Path,
    pub timeline: &'a Timeline,
    /// The SIDECAR's duration — see decision 1. Never a probed one.
    pub source_duration_ms: u64,
    pub settings: EncodeSettings,
}

/// What the export produced, for the note and the vault write.
///
/// `remuxed` is the fast path's only observable trace once the file is on
/// disk, so it is carried out rather than re-derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportOutcome {
    pub output_duration_ms: u64,
    pub remuxed: bool,
}

/// Why this export cannot run, or `None`.
///
/// Answered before any child exists, so the user is told what is missing
/// rather than handed ffmpeg's own error text (decision 4).
///
/// **`source_duration_ms` is a parameter and must stay one.** The plan
/// specified `export_refusal(timeline, settings)` with no duration, which
/// cannot tell an untouched capture from an edited one: the only rule
/// available without it is "one segment starting at zero", and a TRIMMED
/// TAIL satisfies that while `is_untouched` correctly rejects it. That
/// approximation would let a trimmed-tail export past the encoder check on a
/// build with no H.264 encoder, and `export` would then take the re-encode
/// path and die inside ffmpeg naming none of it — exactly the failure this
/// function exists to prevent. One rule, one place: both this and `export`
/// key on `Timeline::is_untouched(source_duration_ms)`.
pub fn export_refusal(
    timeline: &Timeline,
    source_duration_ms: u64,
    settings: &EncodeSettings,
) -> Option<String> {
    // Measured against the PLAN, not `Timeline::is_empty`: a timeline of
    // nothing but zero-length segments is not empty and still plans no
    // spans, and an empty filter graph makes ffmpeg fail on a message that
    // names none of this.
    if plan(timeline).is_empty() {
        return Some(
            "There is nothing to export — every part of this capture has been removed. \
             Undo a delete, or discard the capture."
                .into(),
        );
    }
    // The fast path copies streams and decodes nothing, so an untouched
    // capture needs no encoder at all. Refusing it too would tell a user
    // with a minimal ffmpeg build that they cannot save anything, which is
    // false.
    if !timeline.is_untouched(source_duration_ms) && settings.h264_encoder.is_empty() {
        return Some(
            "This capture has been edited, and the installed ffmpeg has no H.264 encoder \
             to re-encode it with. Install a build with libx264, or undo the edits and \
             save the recording unchanged."
                .into(),
        );
    }
    None
}

/// Export `source` to `dest` through ffmpeg, reporting whole-percent
/// progress and honouring `cancel`.
///
/// `on_progress` is called with 0..=100 and always ends on 100 for a
/// successful export — ffmpeg's own last tick lands a little short of the
/// end (its final `out_time_us` is the last packet's, not the file's), and a
/// remux is over so fast that it may emit only two ticks in total.
pub fn export(
    req: ExportRequest<'_>,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64),
) -> Result<ExportOutcome, ScreenError> {
    if let Some(message) = export_refusal(req.timeline, req.source_duration_ms, &req.settings) {
        return Err(ScreenError::Refused(message));
    }
    let spans = plan(req.timeline);
    let output_duration_ms = plan_output_duration_ms(&spans);
    // Decision 1: the sidecar's duration, never a probed one.
    let remuxed = req.timeline.is_untouched(req.source_duration_ms);
    let args = if remuxed {
        remux_args(req.source, req.dest)
    } else {
        reencode_args(req.source, req.dest, &spans, &req.settings)
    };
    run(
        req.ffmpeg,
        &args,
        req.dest,
        output_duration_ms,
        cancel,
        on_progress,
    )?;
    Ok(ExportOutcome {
        output_duration_ms,
        remuxed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use vault_buddy_core::screen_capture_config::ScreenQuality;
    use vault_buddy_core::timeline::Segment;

    /// The REAL round trips -- a synthesized clip driven through a real
    /// ffmpeg and read back -- live in `screen/tests/export_roundtrip.rs`,
    /// as an integration test, the `mcp/tests/roundtrip.rs` precedent. They
    /// drive an external binary and only the crate's public surface, so they
    /// belong beside the crate rather than inside it. What stays here is
    /// what must: the pure decisions, and the structural scans that can only
    /// be written where `include_str!("export.rs")` resolves.
    fn settings(has_audio: bool) -> EncodeSettings {
        EncodeSettings {
            width: 1920,
            height: 1080,
            fps: 30,
            quality: ScreenQuality::Balanced,
            h264_encoder: "libx264".into(),
            has_audio,
        }
    }

    // ---------------------------------------------------------------- pure

    #[test]
    fn an_empty_timeline_is_refused_with_a_message_naming_what_to_do() {
        let msg = export_refusal(&Timeline::default(), 9_000, &settings(true)).expect("refused");
        assert!(msg.to_lowercase().contains("nothing"), "{msg}");
    }

    #[test]
    fn a_timeline_of_only_zero_length_segments_is_refused_too() {
        let t = Timeline {
            segments: vec![Segment {
                source_start_ms: 500,
                source_end_ms: 500,
            }],
        };
        assert!(!t.is_empty(), "the fixture must not trip is_empty as well");
        assert!(export_refusal(&t, 9_000, &settings(true)).is_some());
    }

    // The fast path needs NO encoder, so a build without one must still be
    // able to save an untouched capture. Refusing both would tell a user
    // with a minimal ffmpeg that they cannot save at all, which is false.
    #[test]
    fn a_build_with_no_encoder_refuses_an_edited_export_but_not_an_untouched_one() {
        let mut s = settings(true);
        s.h264_encoder = String::new();
        let edited = Timeline::whole(9_000).split_at(4_000).delete(0);
        let msg = export_refusal(&edited, 9_000, &s).expect("an edited export needs an encoder");
        let lower = msg.to_lowercase();
        assert!(
            lower.contains("h.264") || lower.contains("encoder"),
            "{msg}"
        );
        assert_eq!(export_refusal(&Timeline::whole(9_000), 9_000, &s), None);
    }

    // PLAN DEFECT, closed here. The plan specified
    // `export_refusal(timeline, settings)` with no source duration -- and
    // without one the only "is this untouched" rule available is "a single
    // segment starting at zero", which a TRIMMED TAIL satisfies. Under that
    // approximation this fixture would be waved through as needing no
    // encoder, and `export` would then take the re-encode path and die
    // inside ffmpeg on a message naming none of it.
    //
    // Note the fixture deliberately trips the approximation and NOT
    // `is_empty`, and is one segment starting at 0: it fails only if the
    // refusal keys on something other than is_untouched(source_duration_ms).
    #[test]
    fn a_trimmed_tail_needs_an_encoder_even_though_it_is_one_segment_starting_at_zero() {
        let mut s = settings(true);
        s.h264_encoder = String::new();
        let trimmed_tail = Timeline {
            segments: vec![Segment {
                source_start_ms: 0,
                source_end_ms: 4_000,
            }],
        };
        assert!(
            !trimmed_tail.is_untouched(9_000),
            "the fixture must be edited against the sidecar's 9 s duration"
        );
        let msg = export_refusal(&trimmed_tail, 9_000, &s)
            .expect("a trimmed tail is re-encoded and so needs an encoder");
        assert!(msg.to_lowercase().contains("encoder"), "{msg}");
        // ...and the SAME timeline against a 4 s source really is untouched,
        // so the rule is the duration comparison rather than "always refuse".
        assert_eq!(export_refusal(&trimmed_tail, 4_000, &s), None);
    }

    #[test]
    fn the_cancelled_error_renders_a_constant_message() {
        assert_eq!(
            ScreenError::Cancelled.to_string(),
            "the export was cancelled"
        );
    }

    /// This file up to (not including) its test module. The scans below
    /// necessarily name the thing they forbid, so scanning the whole file
    /// would make them self-match -- the same fix `sink.rs` applies.
    fn production_src() -> &'static str {
        let src = include_str!("export.rs");
        src.split("#[cfg(test)]").next().unwrap_or(src)
    }

    // MUTATION M2, pinned structurally as well as end-to-end. Decision 1
    // says the fast path is keyed on the SIDECAR's duration; the way that
    // rule gets broken is somebody "improving" it by probing the file, which
    // reports a number a frame or two different and turns every untouched
    // capture into a silent full re-encode. This module must therefore never
    // run a probe at all: it spawns exactly ONE child, and that child is
    // ffmpeg doing the export.
    //
    // Scanned as a string LITERAL (with its opening quote) rather than as
    // the bare word, so the module's own prose may go on explaining why.
    #[test]
    fn the_export_never_probes_the_source_it_is_handed_the_sidecars_duration() {
        // Task 42 moved the spawn into `ffmpeg_run`, so the "one child"
        // half is measured across BOTH files: this one must still spawn
        // nothing itself, and together they spawn exactly the one.
        let src = production_src();
        let runner = crate::ffmpeg_run::tests::production_src();
        for (name, text) in [("export.rs", src), ("ffmpeg_run.rs", runner)] {
            assert!(
                !text.contains("\"ffprobe"),
                "{name}: the export must not shell out to ffprobe: decision 1"
            );
        }
        assert_eq!(
            src.matches("Command::new(").count() + runner.matches("Command::new(").count(),
            1,
            "the export spawns exactly one child, the export itself"
        );
        assert!(
            src.contains("is_untouched(req.source_duration_ms)"),
            "the fast path must key on the request's (sidecar) duration"
        );
    }

    // Task 42's extraction, pinned: the export keeps its decisions and
    // hands its child to the ONE runner the render shares. A spawn, a pipe
    // drain or a cancel poll growing back here would be a second copy of
    // the two-pipe discipline to keep in step -- the copy the extraction
    // exists to prevent. (The behaviour itself is pinned by this suite and
    // tests/export_roundtrip.rs, both green before and after the move.)
    #[test]
    fn extraction_keeps_export_behaviour() {
        let src = production_src();
        assert!(
            src.contains("use crate::ffmpeg_run::run;"),
            "the export must run its child through ffmpeg_run"
        );
        for forbidden in [
            "std::process",
            "thread::Builder",
            "recv_timeout",
            "fn drain_capped",
            "fn run(",
        ] {
            assert!(
                !src.contains(forbidden),
                "export.rs grew its own runner again: {forbidden}"
            );
        }
    }

    #[test]
    fn a_cancel_that_arrived_before_the_spawn_never_starts_a_child() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("out.mp4");
        let cancel = AtomicBool::new(true);
        let timeline = Timeline::whole(6_000);
        let err = export(
            ExportRequest {
                // Deliberately a path that could not run: reaching a spawn
                // at all is the failure this pins.
                ffmpeg: Path::new("/nonexistent/ffmpeg"),
                source: Path::new("/nonexistent/in.mp4"),
                dest: &dest,
                timeline: &timeline,
                source_duration_ms: 6_000,
                settings: settings(true),
            },
            &cancel,
            &mut |_| {},
        )
        .expect_err("a pre-set cancel flag must refuse");
        assert_eq!(err, ScreenError::Cancelled);
        assert!(!dest.exists());
    }

    #[test]
    fn a_missing_ffmpeg_is_a_typed_tool_missing_rather_than_a_raw_io_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let dest = dir.path().join("out.mp4");
        let timeline = Timeline::whole(6_000);
        let err = export(
            ExportRequest {
                ffmpeg: Path::new("/nonexistent/ffmpeg-that-is-not-installed"),
                source: Path::new("/nonexistent/in.mp4"),
                dest: &dest,
                timeline: &timeline,
                source_duration_ms: 6_000,
                settings: settings(true),
            },
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect_err("no such binary");
        assert_eq!(err, ScreenError::ToolMissing);
    }
}
