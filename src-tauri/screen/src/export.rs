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
//! 2. **Cancellation kills the child and deletes the destination.** There is
//!    no decode loop to poll any more. The flag is read on every wake of the
//!    progress loop, and the answer is `Child::kill()` followed by removing
//!    `dest` — a killed ffmpeg leaves a truncated file behind, and leaving
//!    it there would offer the user a broken export as if it were a saved
//!    one.
//! 3. **There is NO export timeout, deliberately. Do not add one.** A
//!    two-hour 4K recording legitimately takes a long time to re-encode, and
//!    a child that is still writing into a temp is indistinguishable from
//!    one that has hung. Killing it on a timer would destroy real work and
//!    look exactly like a crash. The user's Cancel is the only bound.
//! 4. **A refusal is a refusal, not a failed run.** `export_refusal` answers
//!    "can this export run at all" BEFORE a child exists, so the message
//!    names the missing capability instead of surfacing ffmpeg's own text.
//!
//! ## Why stdout and stderr are each drained on their own named thread
//!
//! ffmpeg writes `-progress` to stdout and its diagnostics to stderr. Read
//! one inline and the other fills its OS pipe buffer and blocks the child
//! forever — the classic two-pipe deadlock. Draining both, plus a timed
//! `recv_timeout` on the progress channel, also gives cancellation a bounded
//! response time on an export that emits no progress at all (a stalled
//! source), which a blocking `read_line` would not.

use crate::ffmpeg_args::{
    parse_progress_line, reencode_args, remux_args, EncodeSettings, ProgressTick,
};
use crate::select::{plan, plan_output_duration_ms, progress_percent};
use crate::ScreenError;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use vault_buddy_core::throttle::EmitThrottle;
use vault_buddy_core::timeline::Timeline;

/// How often the progress loop wakes when ffmpeg says nothing. This is the
/// cancel latency, NOT a timeout: nothing is killed because the timer
/// elapsed, only because the user asked (decision 3).
const CANCEL_POLL: Duration = Duration::from_millis(200);

/// Emit progress every 2 percentage points (spec §8.3's throttle).
const PROGRESS_MIN_DELTA: u64 = 2;

/// How much of ffmpeg's stderr a failure message may carry. `-loglevel
/// error` keeps it to a line or two; the cap exists so a pathological build
/// cannot grow an unbounded String in a failure path.
const STDERR_CAP: usize = 8 * 1024;

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

/// Windows process-creation flags for the ffmpeg child.
///
/// `CREATE_NO_WINDOW`, for the reason `external_tool.rs` records about
/// Pandoc: this app is `windows_subsystem = "windows"` in release and owns
/// no console, so a child spawned with default flags allocates a NEW console
/// window that flashes up and grabs foreground focus. Pandoc's probe made
/// that visible as the settings panel closing itself; an export runs for
/// minutes, so the console would simply sit there on top of the editor.
///
/// Takes the platform as a parameter so BOTH arms are asserted on Linux —
/// the shell's own version of this was once hard-wired to `0` with the whole
/// suite green.
const fn creation_flags_for(windows: bool) -> u32 {
    if windows {
        0x0800_0000 // CREATE_NO_WINDOW
    } else {
        0
    }
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

/// ffmpeg's `out_time_us` is MICROseconds; `select::progress_percent` takes
/// output MILLIseconds. Nothing in `ffmpeg_args` enforces the divide, and
/// getting it wrong is invisible in every argument test: a bar that runs
/// 1000x fast pins at 100% within a frame and then sits there, which looks
/// like a hang on exactly the long exports progress exists for.
fn percent_from_out_time_us(out_time_us: u64, total_output_ms: u64) -> u64 {
    progress_percent(out_time_us / 1_000, total_output_ms)
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

fn run(
    ffmpeg: &Path,
    args: &[String],
    dest: &Path,
    total_output_ms: u64,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64),
) -> Result<(), ScreenError> {
    // A cancel that arrived while the worker was still preparing must not
    // start a child at all; otherwise the first thing the user's Cancel does
    // is launch an encoder.
    if cancel.load(Ordering::Relaxed) {
        return Err(ScreenError::Cancelled);
    }

    let mut command = Command::new(ffmpeg);
    command
        .args(args)
        // ffmpeg already gets `-nostdin`; a null stdin means a build that
        // ignores it still cannot stop to ask a question.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let creation_flags = creation_flags_for(cfg!(windows));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(creation_flags);
    }
    #[cfg(not(windows))]
    {
        // There is no flag to apply here, but the helper is still BOUND on
        // this platform on purpose: it keeps the Windows-only constant live
        // (and so compiled and warning-free) on the one platform whose
        // suite actually runs, which is where its two-arm test lives.
        debug_assert_eq!(creation_flags, 0);
    }
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScreenError::ToolMissing
        } else {
            ScreenError::Io(format!("could not start ffmpeg: {e}"))
        }
    })?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ScreenError::Io("ffmpeg stdout was not captured".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ScreenError::Io("ffmpeg stderr was not captured".into()))?;

    let (tx, rx) = mpsc::channel::<ProgressTick>();
    let progress_reader = std::thread::Builder::new()
        .name("screen-export-progress".into())
        .spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(tick) = parse_progress_line(&line) {
                    // A closed receiver means the export is being torn down;
                    // stop reading rather than spinning to EOF.
                    if tx.send(tick).is_err() {
                        return;
                    }
                }
            }
        })
        .map_err(|e| ScreenError::Io(format!("could not start the export progress reader: {e}")))?;
    let stderr_reader = std::thread::Builder::new()
        .name("screen-export-stderr".into())
        .spawn(move || drain_capped(stderr))
        .map_err(|e| ScreenError::Io(format!("could not start the export error reader: {e}")))?;

    let mut throttle = EmitThrottle::new(PROGRESS_MIN_DELTA);
    let mut last_emitted: Option<u64> = None;
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        match rx.recv_timeout(CANCEL_POLL) {
            Ok(ProgressTick::OutTimeUs(us)) => {
                let percent = percent_from_out_time_us(us, total_output_ms);
                if throttle.should_emit(percent, false) {
                    last_emitted = Some(percent);
                    on_progress(percent);
                }
            }
            // `progress=end` means the last packet was written, not that the
            // file is closed — faststart still has to move the index. Wait
            // for stdout to actually close.
            Ok(ProgressTick::Done) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    if cancelled {
        // Kill first, then reap, then join: the readers only finish once the
        // child's pipes close.
        let _ = child.kill();
        let _ = child.wait();
        let _ = progress_reader.join();
        let _ = stderr_reader.join();
        // The other half of a cancel. A killed ffmpeg leaves a truncated
        // file; leaving it on disk would offer the user a broken export as
        // though it were a saved one.
        remove_output(dest);
        return Err(ScreenError::Cancelled);
    }

    let status = child
        .wait()
        .map_err(|e| ScreenError::Io(format!("could not wait for ffmpeg: {e}")))?;
    let _ = progress_reader.join();
    let errors = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        remove_output(dest);
        return Err(ScreenError::Sink(failure_message(
            status.code(),
            errors.trim(),
        )));
    }
    // The terminal emit. ffmpeg's last tick is the last PACKET's position,
    // which lands short of the planned duration, so without this the bar
    // stops at 98-99% on a finished export — and a remux can emit as few as
    // two ticks in total.
    if last_emitted != Some(100) {
        on_progress(100);
    }
    Ok(())
}

fn failure_message(code: Option<i32>, errors: &str) -> String {
    let status = match code {
        Some(c) => format!("ffmpeg exited with status {c}"),
        None => "ffmpeg was terminated by a signal".to_string(),
    };
    if errors.is_empty() {
        status
    } else {
        format!("{status}: {errors}")
    }
}

/// Read a child pipe to EOF, keeping at most `STDERR_CAP` bytes.
///
/// Reading to EOF is not optional even when the text is discarded: stopping
/// early leaves the pipe full and the child blocked on its next write.
fn drain_capped<R: Read>(reader: R) -> String {
    let mut kept = String::new();
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        if kept.len() < STDERR_CAP {
            kept.push_str(&line);
            kept.push('\n');
        }
    }
    kept
}

fn remove_output(dest: &Path) {
    match std::fs::remove_file(dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!(
            "screen export: could not remove the abandoned output {}: {e}",
            dest.display()
        ),
    }
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

    // The units hop nothing else owns: ffmpeg reports MICROseconds and
    // `progress_percent` takes output MILLIseconds. Reading the tick as
    // milliseconds makes the bar reach 100% a thousand times too early and
    // then sit there for the whole export -- and every argument-level test
    // stays green, because no argument is involved.
    //
    // The 1000x is asserted in BOTH directions: the correct conversion at
    // the halfway mark, and the fact that the mis-read value is a DIFFERENT
    // answer. A single assertion at 0 or at the end would pass under either
    // reading.
    #[test]
    fn out_time_us_is_divided_to_milliseconds_before_the_percent_is_computed() {
        // 2 s written of a 4 s output, reported as 2_000_000 us.
        assert_eq!(percent_from_out_time_us(2_000_000, 4_000), 50);
        // Read as milliseconds instead, 2_000_000 would clamp to 100.
        assert_eq!(progress_percent(2_000_000, 4_000), 100);
        assert_eq!(percent_from_out_time_us(0, 4_000), 0);
        assert_eq!(percent_from_out_time_us(4_000_000, 4_000), 100);
        // Sub-millisecond ticks truncate rather than wrapping or panicking.
        assert_eq!(percent_from_out_time_us(999, 4_000), 0);
    }

    // The Pandoc lesson, applied to a child that runs for minutes rather
    // than seconds: this app owns no console in a release build, so a child
    // spawned with default flags allocates a NEW one that flashes up and
    // takes foreground focus. Both arms are asserted because the shell's own
    // version of this constant was once hard-wired to 0 with every test
    // green -- a Linux-only suite can never execute the Windows arm, so it
    // has to be able to VALUE it.
    #[test]
    fn the_ffmpeg_child_is_given_create_no_window_on_windows_and_no_flag_elsewhere() {
        assert_eq!(creation_flags_for(true), 0x0800_0000);
        assert_eq!(creation_flags_for(false), 0);
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
        let src = production_src();
        assert!(
            !src.contains("\"ffprobe"),
            "the export must not shell out to ffprobe: decision 1"
        );
        assert_eq!(
            src.matches("Command::new(").count(),
            1,
            "the export spawns exactly one child, the export itself"
        );
        assert!(
            src.contains("is_untouched(req.source_duration_ms)"),
            "the fast path must key on the request's (sidecar) duration"
        );
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

    #[test]
    fn a_failed_run_names_the_exit_status_and_carries_ffmpegs_own_text() {
        assert_eq!(
            failure_message(Some(1), "Invalid argument"),
            "ffmpeg exited with status 1: Invalid argument"
        );
        assert_eq!(failure_message(Some(1), ""), "ffmpeg exited with status 1");
        assert_eq!(
            failure_message(None, ""),
            "ffmpeg was terminated by a signal"
        );
    }

    // The cap is checked per LINE, so a many-line flood is bounded to about
    // STDERR_CAP while a single pathological line is kept whole. That limit
    // is deliberate and stated here rather than left for a reader to
    // discover: the alternative, truncating mid-line, would cut an ffmpeg
    // error in half in the one place the user needs to read it.
    #[test]
    fn a_drained_pipe_is_read_to_the_end_and_a_flood_is_bounded_to_about_the_cap() {
        let flood: String = std::iter::repeat_n("noise\n", STDERR_CAP).collect();
        let kept = drain_capped(flood.as_bytes());
        assert!(
            kept.len() < STDERR_CAP + 16,
            "a flood is bounded: {}",
            kept.len()
        );
        assert!(!kept.is_empty(), "and it is not simply dropped");
        assert_eq!(drain_capped("one\ntwo\n".as_bytes()), "one\ntwo\n");
    }
}
