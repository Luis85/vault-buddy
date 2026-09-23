//! The STREAMING half of "run a user-installed external binary" (Task 28):
//! `external_tool::run_capturing` buffers a child's output up to a 64 KiB
//! cap, which is right for a version banner or an error slice and wrong for
//! a decoded audio stream — the editor's waveform decode pipes minutes of
//! PCM through stdout. `run_streaming` folds each read into caller-owned
//! state instead of keeping the bytes, and adds the one thing a long decode
//! needs that a probe never did: cancellation.
//!
//! Its own module rather than a fourth runner inside `external_tool.rs`
//! (which sits near the 800-line cap); it reuses that module's
//! `DRAIN_GRACE`, and its callers build their `Command` with
//! `external_tool::tool_command`, so the headless `CREATE_NO_WINDOW` flag
//! and the registry-fresh PATH still have exactly one chokepoint.

use std::process::Command;
use std::time::Duration;

use crate::external_tool::DRAIN_GRACE;

/// What `run_streaming` ended with.
#[derive(Debug)]
pub(crate) enum Streamed<S> {
    /// The child exited on its own: `success` is its exit status, `state`
    /// what the fold made of ALL of its stdout.
    Finished { success: bool, state: S },
    /// `cancel` was set: the child was killed and its partial output is
    /// discarded — half a waveform is not one (R20).
    Cancelled,
}

/// How often `run_streaming` looks at the child and the cancel flag — the
/// longest a Cancel waits before the child is killed.
const STREAM_POLL: Duration = Duration::from_millis(50);

/// The size of one stdout read `run_streaming` hands its fold.
const STREAM_CHUNK: usize = 64 * 1024;

/// Spawn `cmd` and FOLD its stdout as it arrives, instead of buffering it
/// the way `run_capturing` does (whose 64 KiB cap would truncate a decoded
/// waveform at under five seconds of audio). Built for the editor's peaks
/// decode (Task 28): memory is whatever `state` is — the bytes themselves
/// are never kept.
///
/// - stdout is read on a thread named `thread_name`, one `STREAM_CHUNK`
///   read at a time; a read may end anywhere, including mid-sample, so the
///   fold owns carrying a partial record across reads.
/// - `cancel` is polled every `STREAM_POLL` by THIS thread, which owns the
///   child: setting it kills the child, whose closed pipe ends the reader.
/// - `timeout` bounds the whole run the way `run_capturing`'s does; a child
///   still running at it is killed and reported as `TimedOut`.
/// - stdin and stderr are nulled — a child that blocks reading stdin or
///   fills an unread stderr pipe would otherwise wedge the fold forever.
///
/// Build `cmd` with `tool_command`, so the child inherits the headless
/// `CREATE_NO_WINDOW` flag and the registry-fresh PATH. `Err` on a spawn
/// failure, a read error, a timeout, or a reader that never delivered.
pub(crate) fn run_streaming<S: Send + 'static>(
    mut cmd: Command,
    thread_name: &str,
    cancel: &std::sync::atomic::AtomicBool,
    timeout: Duration,
    state: S,
    fold: fn(&[u8], &mut S),
) -> std::io::Result<Streamed<S>> {
    use std::io::{Error, ErrorKind, Read};
    use std::process::Stdio;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    if cancel.load(Ordering::SeqCst) {
        return Ok(Streamed::Cancelled);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    let Some(mut stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Error::other("the child's stdout was not piped"));
    };
    let (tx, rx) = mpsc::channel::<std::io::Result<S>>();
    let reader = std::thread::Builder::new()
        .name(thread_name.to_string())
        .spawn(move || {
            let mut state = state;
            let mut buf = vec![0u8; STREAM_CHUNK];
            let outcome = loop {
                match stdout.read(&mut buf) {
                    Ok(0) => break Ok(state),
                    Ok(n) => fold(&buf[..n], &mut state),
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(e) => break Err(e),
                }
            };
            // No-op when the caller already stopped waiting (cancel/timeout).
            let _ = tx.send(outcome);
        });
    if let Err(e) = reader {
        let _ = child.kill();
        let _ = child.wait();
        return Err(e);
    }
    let start = std::time::Instant::now();
    let success = loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Streamed::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) => {}
            Err(e) => {
                // Never return with the child still running unreaped.
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::new(
                ErrorKind::TimedOut,
                "the tool did not finish in time and was stopped",
            ));
        }
        std::thread::sleep(STREAM_POLL);
    };
    // The same bounded drain as `run_capturing`: an exited child normally
    // closed its pipe, so the reader delivers at once.
    match rx.recv_timeout(DRAIN_GRACE) {
        Ok(Ok(state)) => Ok(Streamed::Finished { success, state }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(Error::other("the tool's output was never fully read")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The platform's own way to print a known string and to run long.
    fn printing(text: &str) -> Command {
        if cfg!(windows) {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", &format!("echo {text}")]);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", &format!("printf '{text}'")]);
            cmd
        }
    }

    fn sleeping() -> Command {
        if cfg!(windows) {
            let mut cmd = Command::new("ping");
            cmd.args(["-n", "30", "127.0.0.1"]);
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "sleep 30"]);
            cmd
        }
    }

    fn collect(chunk: &[u8], state: &mut Vec<u8>) {
        state.extend_from_slice(chunk);
    }

    // Task 28: the streaming runner hands EVERY stdout byte to the fold
    // (not a capped prefix) and reports the child's own exit status.
    #[test]
    fn run_streaming_folds_all_of_stdout() {
        use std::sync::atomic::AtomicBool;
        let cancel = AtomicBool::new(false);
        let out = run_streaming(
            printing("streamed-9f3"),
            "test-stream",
            &cancel,
            Duration::from_secs(20),
            Vec::new(),
            collect,
        )
        .unwrap();
        let Streamed::Finished { success, state } = out else {
            panic!("not cancelled: {out:?}");
        };
        assert!(success);
        assert!(
            String::from_utf8_lossy(&state).contains("streamed-9f3"),
            "{state:?}"
        );
    }

    // Cancel is the point of streaming: a set flag kills a child that
    // would otherwise run for 30 s, well before it finishes.
    #[test]
    fn run_streaming_kills_the_child_when_cancelled() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancel);
        let setter = std::thread::Builder::new()
            .name("test-cancel".into())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                flag.store(true, Ordering::SeqCst);
            })
            .unwrap();
        let start = std::time::Instant::now();
        let out = run_streaming(
            sleeping(),
            "test-stream",
            &cancel,
            Duration::from_secs(60),
            Vec::new(),
            collect,
        )
        .unwrap();
        setter.join().unwrap();
        assert!(matches!(out, Streamed::Cancelled), "{out:?}");
        assert!(start.elapsed() < Duration::from_secs(10), "killed promptly");
        // A flag already set never spawns anything.
        let out = run_streaming(
            printing("x"),
            "test-stream",
            &cancel,
            Duration::from_secs(5),
            Vec::new(),
            collect,
        )
        .unwrap();
        assert!(matches!(out, Streamed::Cancelled));
    }

    #[test]
    fn run_streaming_stops_a_child_at_the_timeout() {
        use std::sync::atomic::AtomicBool;
        let cancel = AtomicBool::new(false);
        let start = std::time::Instant::now();
        let e = run_streaming(
            sleeping(),
            "test-stream",
            &cancel,
            Duration::from_millis(300),
            Vec::new(),
            collect,
        )
        .unwrap_err();
        assert_eq!(e.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    // Fix round 1 (review Minor 3): once the child is spawned, NO exit may
    // leave it running unreaped. `try_wait()?` returned straight out of the
    // poll loop on an error, leaving ffmpeg writing and the reader folding.
    // (A real `GetExitCodeProcess` failure cannot be provoked here, so the
    // rule is pinned in the source.)
    #[test]
    fn no_exit_after_the_spawn_leaves_the_child_unreaped() {
        let src = include_str!("external_stream.rs");
        let body = src.split("#[cfg(test)]").next().unwrap();
        assert!(
            !body.contains("try_wait()?"),
            "a try_wait error must kill and reap the child before returning"
        );
    }
}
