//! Running ONE ffmpeg child: spawn it, stream its `-progress`, drain its
//! stderr, honour a cancel by killing it, and remove a truncated output.
//!
//! Extracted from the phase-5 export (tutorial-editor Task 42) so the
//! export and the editor's render (`render/`) drove the SAME runner rather
//! than two copies of the two-pipe discipline below. Task 59 retired the
//! export; the render is the runner's one caller, and its end-to-end proof
//! is `tests/render_roundtrip.rs`.
//!
//! ## What a caller must know
//!
//! 1. **Cancellation kills the child and deletes the destination.** There
//!    is no decode loop to poll. The flag is read on every wake of the
//!    progress loop, and the answer is `Child::kill()` followed by removing
//!    `dest` -- a killed ffmpeg leaves a truncated file behind, and leaving
//!    it there would offer the user a broken file as if it were a saved one.
//! 2. **There is NO timeout, deliberately. Do not add one.** A two-hour 4K
//!    recording legitimately takes a long time to re-encode, and a child
//!    that is still writing into a temp is indistinguishable from one that
//!    has hung. Killing it on a timer would destroy real work and look
//!    exactly like a crash. The user's Cancel is the only bound.
//!
//! ## Why stdout and stderr are each drained on their own named thread
//!
//! ffmpeg writes `-progress` to stdout and its diagnostics to stderr. Read
//! one inline and the other fills its OS pipe buffer and blocks the child
//! forever -- the classic two-pipe deadlock. Draining both, plus a timed
//! `recv_timeout` on the progress channel, also gives cancellation a bounded
//! response time on a run that emits no progress at all (a stalled source),
//! which a blocking `read_line` would not.

use crate::ffmpeg_args::{parse_progress_line, ProgressTick};
use crate::ScreenError;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;
use vault_buddy_core::throttle::EmitThrottle;

/// How often the progress loop wakes when ffmpeg says nothing. This is the
/// cancel latency, NOT a timeout: nothing is killed because the timer
/// elapsed, only because the user asked (point 2 above).
const CANCEL_POLL: Duration = Duration::from_millis(200);

/// Emit progress every 2 percentage points (spec §8.3's throttle).
const PROGRESS_MIN_DELTA: u64 = 2;

/// How much of ffmpeg's stderr a failure message may carry. `-loglevel
/// error` keeps it to a line or two; the cap exists so a pathological build
/// cannot grow an unbounded String in a failure path.
const STDERR_CAP: usize = 8 * 1024;

/// Windows process-creation flags for the ffmpeg child.
///
/// `CREATE_NO_WINDOW`, for the reason `external_tool.rs` records about
/// Pandoc: this app is `windows_subsystem = "windows"` in release and owns
/// no console, so a child spawned with default flags allocates a NEW console
/// window that flashes up and grabs foreground focus. Pandoc's probe made
/// that visible as the settings panel closing itself; a render runs for
/// minutes, so the console would simply sit there on top of the editor.
///
/// Takes the platform as a parameter so BOTH arms are asserted on Linux --
/// the shell's own version of this was once hard-wired to `0` with the whole
/// suite green.
pub(crate) const fn creation_flags_for(windows: bool) -> u32 {
    if windows {
        0x0800_0000 // CREATE_NO_WINDOW
    } else {
        0
    }
}

/// Progress as a whole percent, 0..=100 — moved here from the retired
/// export's `select` module (Task 59), whose only other resident this was.
///
/// Integer arithmetic on purpose: this is what `core::throttle` gates on,
/// so the number that passes the gate and the number the user sees are the
/// same number. A zero-length output is COMPLETE rather than a division by
/// zero — there is nothing left to write.
fn progress_percent(written_output_ms: u64, total_output_ms: u64) -> u64 {
    if total_output_ms == 0 {
        return 100;
    }
    let scaled = written_output_ms.saturating_mul(100) / total_output_ms;
    scaled.min(100)
}

/// ffmpeg's `out_time_us` is MICROseconds; `progress_percent` takes output
/// MILLIseconds. Nothing in `ffmpeg_args` enforces the divide, and getting
/// it wrong is invisible in every argument test: a bar that runs 1000x fast
/// pins at 100% within a frame and then sits there, which looks like a hang
/// on exactly the long renders progress exists for.
fn percent_from_out_time_us(out_time_us: u64, total_output_ms: u64) -> u64 {
    progress_percent(out_time_us / 1_000, total_output_ms)
}

/// What one run's two reader threads and its log lines are called, so a
/// crash record or a log line names the JOB that spawned the child (Task
/// 46: a render that died on a thread named after another feature would
/// send a reader to the wrong one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerNames {
    pub progress_thread: &'static str,
    pub stderr_thread: &'static str,
    pub log_prefix: &'static str,
}

/// The editor render's names: its job thread is `editor-render`, and the
/// readers are named after it.
pub const RENDER_RUNNER: RunnerNames = RunnerNames {
    progress_thread: "editor-render-progress",
    stderr_thread: "editor-render-stderr",
    log_prefix: "editor render",
};

/// Run `ffmpeg args`, reporting whole-percent progress of `total_output_ms`
/// and honouring `cancel`.
///
/// `on_progress` is called with 0..=100 and always ends on 100 for a
/// successful run -- ffmpeg's own last tick lands a little short of the end
/// (its final `out_time_us` is the last packet's, not the file's), and a
/// remux is over so fast that it may emit only two ticks in total.
pub fn run_named(
    names: RunnerNames,
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
        .name(names.progress_thread.into())
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
        .map_err(|e| {
            ScreenError::Io(format!(
                "{}: could not start the progress reader: {e}",
                names.log_prefix
            ))
        })?;
    let stderr_reader = std::thread::Builder::new()
        .name(names.stderr_thread.into())
        .spawn(move || drain_capped(stderr))
        .map_err(|e| {
            ScreenError::Io(format!(
                "{}: could not start the error reader: {e}",
                names.log_prefix
            ))
        })?;

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
        remove_output(dest, names.log_prefix);
        return Err(ScreenError::Cancelled);
    }

    let status = child
        .wait()
        .map_err(|e| ScreenError::Io(format!("could not wait for ffmpeg: {e}")))?;
    let _ = progress_reader.join();
    let errors = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        remove_output(dest, names.log_prefix);
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

fn remove_output(dest: &Path, log_prefix: &str) {
    match std::fs::remove_file(dest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!(
            "{log_prefix}: could not remove the abandoned output {}: {e}",
            dest.display()
        ),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

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

    // The VALUE test above cannot reach the line that APPLIES the flag:
    // `command.creation_flags(..)` is `#[cfg(windows)]` and so executes in
    // no test on any platform (the GAP-117 class). Deleting it left the
    // whole suite green on Linux AND on Windows while shipping an ffmpeg
    // console that flashes up and steals foreground focus for the MINUTES
    // a render runs. So this scans the module's own production source --
    // `external_tool.rs`'s `the_windows_apply_site_still_passes_the_flag_\
    // from_one_chokepoint`, the sibling this file's flag helper was lifted
    // from, and the same `sink.rs` / `capture_exclusion` structural-pin
    // precedent -- and fails if the apply is deleted, duplicated, moved out
    // of `run_named`, or rewired to a literal that could drift from
    // `creation_flags_for`. (Task 42 moved the runner here from the export;
    // the scan followed it with its assertions unchanged.)
    #[test]
    fn the_windows_apply_site_still_passes_the_flag_from_one_chokepoint() {
        let production = production_src();
        assert_eq!(
            // The leading dot is what distinguishes the APPLY from the
            // definition (`const fn creation_flags_for`) and from the
            // `debug_assert_eq!(creation_flags, 0)` belt on the other arm.
            production.matches(".creation_flags(").count(),
            1,
            "exactly one apply site -- a second Command builder that skipped it \
             would pop a console window and steal focus for the length of a render"
        );
        assert!(
            production.contains("command.creation_flags(creation_flags)"),
            "the apply must read the binding fed by `creation_flags_for`, not a literal"
        );
        assert!(
            production.contains("let creation_flags = creation_flags_for(cfg!(windows));"),
            "the applied value must come from the two-arm helper the VALUE test pins, \
             so a literal cannot drift away from it"
        );
        // And that one site lives inside `run_named`, the single place a
        // child is spawned -- not somewhere a future second spawn path could
        // bypass.
        let body = production
            .split_once("\npub fn run_named(")
            .expect("the spawn chokepoint `run_named` must exist")
            .1;
        let apply = body
            .find("command.creation_flags(creation_flags)")
            .expect("the apply must live inside `run_named`");
        let spawn = body
            .find("command.spawn()")
            .expect("`run_named` must be where the child is spawned");
        assert!(
            apply < spawn,
            "the flag must be applied BEFORE the child is spawned; applying it \
             afterwards changes nothing about the console that already appeared"
        );
    }

    /// This file up to (not including) its test module. The scans necessarily
    /// name the thing they forbid, so scanning the whole file would make them
    /// self-match -- the same fix `sink.rs` applies.
    pub(crate) fn production_src() -> &'static str {
        let src = include_str!("ffmpeg_run.rs");
        src.split("#[cfg(test)]").next().unwrap_or(src)
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
    // Task 46: the runner's threads and log lines carry the JOB's name, so a
    // crash record names the feature that spawned the child. The spawn
    // sites read the names they were given (no name hard-wired into the
    // runner's body), and the render hands in its own.
    #[test]
    fn each_job_runs_its_child_under_its_own_thread_names() {
        let body = production_src()
            .split_once(
                "
pub fn run_named(",
            )
            .expect("the shared runner")
            .1;
        assert!(body.contains(".name(names.progress_thread.into())"));
        assert!(body.contains(".name(names.stderr_thread.into())"));
        assert!(
            !body.contains("\"screen-export-") && !body.contains("\"screen export"),
            "the runner must not hard-wire a job's names"
        );
        for name in [RENDER_RUNNER.progress_thread, RENDER_RUNNER.stderr_thread] {
            assert!(name.starts_with("editor-render-"), "{name}");
        }
        let render = include_str!("render/run.rs");
        let render = render.split("#[cfg(test)]").next().unwrap_or(render);
        assert!(
            render.contains(
                "run_named(
        RENDER_RUNNER,"
            ),
            "the render must run the child under RENDER_RUNNER"
        );
    }

    #[test]
    fn progress_is_an_integer_percent_that_clamps_at_both_ends() {
        assert_eq!(progress_percent(0, 8_000), 0);
        assert_eq!(progress_percent(2_000, 8_000), 25);
        assert_eq!(progress_percent(8_000, 8_000), 100);
        // The last frame's duration can run a hair past the planned end; it
        // must read 100, not 101.
        assert_eq!(progress_percent(8_400, 8_000), 100);
    }

    #[test]
    fn progress_of_an_empty_output_is_complete_rather_than_a_division_by_zero() {
        assert_eq!(progress_percent(0, 0), 100);
    }
}
