//! The tool-agnostic half of "detect and run a user-installed external
//! binary": a registry-fresh PATH, the ordered candidate list, a headless
//! `Command` builder, and a bounded, capped, kill-on-timeout runner.
//!
//! Extracted verbatim from `pandoc.rs`, which had grown two concerns: what is
//! true of Pandoc, and what is true of *any* tool we shell out to. ffmpeg
//! (screen-capture export, spec phase 5) is the second consumer and needed
//! every one of these behaviours — a stale override shadowing a newer install,
//! a PATH that cannot see a just-installed binary until the app restarts, a
//! probe that wedges forever, a child that pops a console window and steals
//! focus. Each was paid for once; the tests moved here with the code so a
//! second copy never has to re-earn them.

use std::process::Command;
use std::time::Duration;

/// Merge the freshly-read registry PATH (`extra`) with the process PATH
/// (`base`), **registry entries FIRST** — deduped case-insensitively. The
/// registry is preferred because `base` is the process's launch-time PATH
/// snapshot, which is STALE: when a Windows user upgrades or relocates the
/// tool while Vault Buddy is running, the new location lands in the registry
/// but not in the process env. Searching the stale process PATH first would
/// keep resolving the OLD executable, so Recheck would report the
/// stale/unsupported version until restart (Codex review). Registry-first
/// fixes that; any process-only entries (session additions not in the
/// registry) still follow.
pub(crate) fn merged_path(base: &str, extra: &[String]) -> String {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut out: Vec<String> = extra.to_vec();
    for p in base.split(sep) {
        if !p.is_empty() && !out.iter().any(|e| e.eq_ignore_ascii_case(p)) {
            out.push(p.to_string());
        }
    }
    out.join(&sep.to_string())
}

/// Windows: read user + machine PATH from the registry so a just-installed
/// tool is visible without restarting (a running process keeps its launch
/// PATH snapshot). Non-Windows: nothing extra (the compile gate + tests).
#[cfg(windows)]
fn registry_path_entries() -> Vec<String> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;
    let mut entries = Vec::new();
    let reads = [
        (HKEY_CURRENT_USER, "Environment"),
        (
            HKEY_LOCAL_MACHINE,
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
        ),
    ];
    for (hive, sub) in reads {
        if let Ok(key) = RegKey::predef(hive).open_subkey(sub) {
            if let Ok(path) = key.get_value::<String, _>("Path") {
                entries.extend(path.split(';').map(str::to_string));
            }
        }
    }
    entries
}

#[cfg(not(windows))]
fn registry_path_entries() -> Vec<String> {
    Vec::new()
}

/// The process PATH augmented with the fresh registry entries, or `None` when
/// there are no registry extras (non-Windows, or nothing to add) so the caller
/// leaves the inherited PATH untouched. Single source for the base+registry
/// merge so `path_executables` (enumeration) and `tool_command` (spawn) don't
/// each re-read the registry and re-run `merged_path`.
pub(crate) fn augmented_path() -> Option<String> {
    let extra = registry_path_entries();
    if extra.is_empty() {
        return None;
    }
    Some(merged_path(
        &std::env::var("PATH").unwrap_or_default(),
        &extra,
    ))
}

/// Ordered candidates to try for `stem`: the configured override FIRST (if
/// non-empty), then each concrete executable found across PATH, then a bare
/// `<stem>` final fallback — deduped, preserving order. Pure so it's testable
/// without touching the real config or filesystem — `candidates_for` below is
/// the impure wrapper that feeds it the real PATH executables.
///
/// The concrete PATH executables matter (Codex review): a bare `pandoc` alone
/// resolves only the FIRST PATH match, so a multi-install state (old pre-2.15
/// pandoc earlier in PATH, a supported one later) would let detection report
/// "too old" even though a valid Pandoc exists. Listing every PATH match lets
/// the resolver probe PAST an unsupported hit to a capable one. The bare stem
/// is kept as a final fallback so we never resolve WORSE than the plain lookup
/// (e.g. if PATH enumeration misses a shell-resolved one).
///
/// `stem` is a parameter (the plan's sketch omitted it) because the bare
/// fallback has to be deduped against the override and the PATH hits INSIDE
/// this function — appending it afterwards in the impure wrapper would
/// re-admit the duplicate this function's own test forbids.
pub(crate) fn candidate_order(
    stem: &str,
    override_path: Option<&str>,
    path_execs: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !s.trim().is_empty() && !out.iter().any(|c| c.eq_ignore_ascii_case(&s)) {
            out.push(s);
        }
    };
    if let Some(p) = override_path {
        push(p.to_string());
    }
    for e in path_execs {
        push(e.clone());
    }
    push(stem.to_string());
    out
}

/// The platform file name an executable `stem` has: `<stem>.exe` on Windows,
/// `<stem>` elsewhere.
fn exe_file_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

/// Concrete `<stem>`/`<stem>.exe` executables across `search_path` (a PATH
/// string), in PATH order — so a resolver can probe past an old install to a
/// supported one later in PATH (Codex review). Best-effort: a non-existent or
/// unreadable dir is skipped.
///
/// The lookup joins the EXACT platform file name rather than scanning a
/// directory's entries, which is what keeps a neighbouring `ffmpeg-gui.exe`
/// or `pandoc-citeproc` from ever being offered as the tool.
pub(crate) fn executables_in(search_path: &str, stem: &str) -> Vec<String> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let exe = exe_file_name(stem);
    let mut out = Vec::new();
    for dir in search_path.split(sep) {
        if dir.is_empty() {
            continue;
        }
        let cand = std::path::Path::new(dir).join(&exe);
        // is_file() follows a symlink to a real file — a symlinked tool counts.
        if cand.is_file() {
            out.push(cand.to_string_lossy().to_string());
        }
    }
    out
}

/// `executables_in` over the registry-augmented PATH.
pub(crate) fn path_executables(stem: &str) -> Vec<String> {
    let merged = augmented_path().unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    executables_in(&merged, stem)
}

/// Ordered candidates for `stem` (see `candidate_order`), fed the caller's
/// config override and the concrete PATH executables.
pub(crate) fn candidates_for(stem: &str, override_path: Option<&str>) -> Vec<String> {
    candidate_order(stem, override_path, &path_executables(stem))
}

/// Windows process-creation flags for a spawned child: `CREATE_NO_WINDOW`
/// (0x0800_0000) on Windows, 0 elsewhere.
///
/// Vault Buddy is a GUI-subsystem app in release (`windows_subsystem =
/// "windows"` in main.rs), so it owns no console. Spawning a console program
/// like `pandoc.exe` or `ffmpeg.exe` with the default flags makes Windows
/// allocate a NEW console window that flashes on screen AND grabs foreground
/// focus. That focus theft blurs the panel and trips its focus-out auto-hide
/// (`schedule_focus_out_check` in lib.rs) — so the Pandoc `--version` probe,
/// which runs the moment `DocumentImportSettings` mounts (opening Buddy
/// settings, the record chooser, or the import picker), flashed a terminal and
/// slammed the settings panel shut. Spawning headless removes the window
/// entirely; the piped stdout/stderr `run_capturing` relies on are unaffected.
/// ffmpeg is spawned far more often than Pandoc and for far longer, so the
/// flag matters more here, not less.
///
/// `cfg!(windows)` (not `#[cfg]`) so both arms compile everywhere and the flag
/// value stays unit-testable on Linux, where the shell crate's tests run.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) const fn child_creation_flags() -> u32 {
    creation_flags_for(cfg!(windows))
}

/// The flag value as a pure function of "are we on Windows".
///
/// This split exists because the `cfg!(windows)` version was **not actually
/// pinned**: the test asserted `0x0800_0000` inside its own `if cfg!(windows)`
/// arm, so on Linux — the only platform the shell crate's tests run on — both
/// the production branch and the assertion took the `else` path, and a
/// `child_creation_flags()` hard-wired to `0` passed the whole suite. The
/// mutation table's M8 row found exactly that. Taking the platform as a
/// PARAMETER lets one Linux test assert BOTH arms, so removing the Windows
/// flag now turns the suite red on the platform CI actually runs.
const fn creation_flags_for(windows: bool) -> u32 {
    if windows {
        0x0800_0000 // CREATE_NO_WINDOW
    } else {
        0
    }
}

/// Build a Command with the registry-augmented PATH so PATH lookup sees a
/// fresh install. On Windows the child is spawned headless (see
/// `child_creation_flags`) so no probe or conversion pops a console window.
pub(crate) fn tool_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
    // One chokepoint for the no-console-window flag: EVERY probe and every
    // conversion/export builds its Command here, so all of them inherit it.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(child_creation_flags());
    }
    cmd
}

/// Max wall-clock for a `--version`/`-version` probe. It returns near-instantly
/// for a real tool, so this is generous; it exists only to bound a
/// pathological candidate (a wrapper/network binary that hangs) so it can't
/// block the probe loop — critically in the import path, where the probe runs
/// while `ImportLock` is held (Codex review).
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Which child stream `run_capturing` pipes back: conversion wants stderr (the
/// failure reason), a version probe wants stdout (the version line).
#[derive(Clone, Copy)]
pub(crate) enum Capture {
    Stdout,
    Stderr,
}

/// How long `run_capturing` waits for the pipe drain AFTER the child exits.
/// A real child closes its pipe on exit, so the reader delivers at once and
/// this is never hit; it only bounds the pathological case where a surviving
/// descendant keeps the inherited pipe open (Codex review) so the call can't
/// block — critically while `ImportLock` is held.
pub(crate) const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Max bytes `run_capturing` STORES from the captured stream. We only ever use
/// the first version line or a 500-char stderr slice, so 64 KiB is ample;
/// it exists to bound memory against a child that floods the pipe for the whole
/// timeout window. Excess is drained but discarded.
pub(crate) const CAPTURE_CAP: usize = 64 * 1024;

/// Spawn `cmd`, wait with a wall-clock kill at `timeout`, and return
/// `(success, captured)`: `success` is true on a zero-exit, false on
/// non-zero/killed; `captured` is the chosen stream's output (empty when none).
/// The kill matters for EVERY caller — a hung version probe (a wrapper or
/// network binary that never returns) would otherwise block forever, and in the
/// import path that runs while `ImportLock` is held, wedging every later import
/// until restart (Codex review). The captured stream is drained on a named
/// worker so a chatty child can't fill the pipe buffer and deadlock the poll
/// loop; stdin is nulled so a child can't block reading it, and the other std
/// stream is nulled. `Err` only on spawn failure.
pub(crate) fn run_capturing(
    mut cmd: Command,
    timeout: Duration,
    capture: Capture,
) -> std::io::Result<(bool, String)> {
    use std::io::Read;
    use std::process::Stdio;
    use std::sync::mpsc;
    cmd.stdin(Stdio::null());
    match capture {
        Capture::Stdout => cmd.stdout(Stdio::piped()).stderr(Stdio::null()),
        Capture::Stderr => cmd.stdout(Stdio::null()).stderr(Stdio::piped()),
    };
    let mut child = cmd.spawn()?;
    let stream: Option<Box<dyn Read + Send>> = match capture {
        Capture::Stdout => child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        Capture::Stderr => child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    };
    // Drain the captured stream on a named worker that delivers its bytes over a
    // channel. Reading concurrently keeps a chatty child from filling the pipe
    // buffer and deadlocking the poll loop below; the channel lets us BOUND how
    // long we wait for it (see the recv_timeout at the end) instead of an
    // unbounded join. We STORE at most CAPTURE_CAP bytes but keep reading past
    // it (discarding) — the timeout bounds wall-clock, not bytes, so a noisy or
    // malicious executable streaming for the whole window could otherwise grow
    // this Vec until OOM (Codex review). Draining the excess (rather than
    // stopping) keeps the pipe from backing up and blocking the child.
    let rx = stream.map(|mut s| {
        let (tx, rx) = mpsc::channel();
        let _ = std::thread::Builder::new()
            .name("external-tool-io".into())
            .spawn(move || {
                let mut buf = Vec::new();
                let mut scratch = [0u8; 8192];
                loop {
                    match s.read(&mut scratch) {
                        Ok(0) | Err(_) => break, // EOF or read error
                        Ok(n) => {
                            if buf.len() < CAPTURE_CAP {
                                let take = (CAPTURE_CAP - buf.len()).min(n);
                                buf.extend_from_slice(&scratch[..take]);
                            }
                            // bytes beyond the cap are read (pipe stays drained)
                            // but not stored.
                        }
                    }
                }
                let _ = tx.send(buf); // no-op if we've already stopped waiting
            });
        rx
    });
    let start = std::time::Instant::now();
    let mut timed_out = false;
    let success = loop {
        if let Some(status) = child.try_wait()? {
            break status.success();
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break false;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    // NEVER block the caller (and, in the import path, `ImportLock`) on the
    // drain. `try_wait` returning does NOT guarantee the pipe is closed: a child
    // that EXITED — or a killed child's surviving grandchild — can still hold
    // the inherited write end open, so `read_to_end` may never see EOF. So we
    // BOUND the wait rather than join unconditionally: on a timeout we don't
    // need the output (it's a failure); otherwise we give the reader a short
    // grace to deliver, then give up with what we have. A reader that outlives
    // the grace finishes detached (its send no-ops once the receiver is gone).
    let captured = if timed_out {
        String::new()
    } else {
        rx.and_then(|rx| rx.recv_timeout(DRAIN_GRACE).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    Ok((success, captured))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The probe/convert runner must never wait on a hung child. Unix-only
    // because the tests drive `sh`; the shell crate's tests run on Linux.
    #[cfg(unix)]
    #[test]
    fn run_capturing_returns_the_chosen_stream_on_success() {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "printf 'pandoc 3.1.9\\n'"]);
        let (ok, out) = run_capturing(cmd, Duration::from_secs(5), Capture::Stdout).unwrap();
        assert!(ok);
        assert!(out.contains("pandoc 3.1.9"), "captured stdout: {out:?}");
    }

    #[cfg(unix)]
    #[test]
    fn run_capturing_kills_a_hung_child_at_the_timeout() {
        // A child that would otherwise run for 30s must be killed promptly and
        // reported as failure — the guarantee that a wedged version probe can't
        // hold ImportLock forever.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30"]);
        let start = std::time::Instant::now();
        let (ok, _) = run_capturing(cmd, Duration::from_millis(150), Capture::Stderr).unwrap();
        assert!(!ok);
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "should return at the timeout, not wait out the sleep"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_capturing_does_not_hang_when_a_descendant_holds_the_pipe() {
        // The shell exits 0 immediately but backgrounds a child that inherits
        // the pipe, so read_to_end never sees EOF even though try_wait returns.
        // The drain wait must be BOUNDED so the call still returns (the direct
        // child's success) instead of blocking on the held pipe — the wedge
        // that would otherwise hold ImportLock (Codex review).
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "sleep 30 & exit 0"]);
        let start = std::time::Instant::now();
        let (ok, _) = run_capturing(cmd, Duration::from_secs(60), Capture::Stderr).unwrap();
        assert!(ok, "the direct child exited 0");
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "must give up on the held pipe near DRAIN_GRACE, not wait out the descendant"
        );
    }

    #[cfg(unix)]
    #[test]
    fn run_capturing_caps_a_flood_of_output() {
        // A child that streams far more than the cap must not grow the buffer
        // without bound — we store at most CAPTURE_CAP and drain the rest.
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "head -c 200000 /dev/zero"]);
        let (ok, out) = run_capturing(cmd, Duration::from_secs(10), Capture::Stdout).unwrap();
        assert!(ok);
        assert_eq!(out.len(), CAPTURE_CAP, "stored output must be capped");
    }

    #[test]
    fn tool_children_are_spawned_headless_on_windows_only() {
        // Regression (buddy-settings crash): Vault Buddy is a GUI-subsystem app
        // in release (`windows_subsystem = "windows"`), so it owns no console.
        // Spawning `pandoc.exe` WITHOUT CREATE_NO_WINDOW makes Windows allocate
        // a NEW console window that flashes on screen AND grabs foreground
        // focus. That focus theft blurs the panel and trips its focus-out
        // auto-hide (schedule_focus_out_check in lib.rs) — so the `--version`
        // probe that runs the moment DocumentImportSettings mounts (opening
        // Buddy settings / the record chooser / the import picker) flashed a
        // terminal and slammed the settings panel shut. Lock the flag value per
        // platform: CREATE_NO_WINDOW on Windows, and never a stray flag on Unix.
        //
        // Asserted through `creation_flags_for`, which takes the platform as a
        // PARAMETER, so BOTH arms are pinned from the Linux job that actually
        // runs these tests. The host-conditional form this replaced asserted
        // only the arm it happened to be running on, and stayed green with the
        // Windows flag deleted entirely (mutation M8).
        //
        // 0x0800_0000 == winapi CREATE_NO_WINDOW.
        assert_eq!(creation_flags_for(true), 0x0800_0000);
        // No console-window concept off Windows — spawn with default flags.
        assert_eq!(creation_flags_for(false), 0);
        // And the shipped constant really is wired to the host platform,
        // rather than to a literal that drifted from the pair above.
        assert_eq!(child_creation_flags(), creation_flags_for(cfg!(windows)));
    }

    // The VALUE test above cannot reach the line that APPLIES the flag:
    // `cmd.creation_flags(..)` is `#[cfg(windows)]` and executes in no test on
    // any platform (the GAP-117 class). So this scans the module's own
    // production source — the `sink.rs` / `capture_exclusion` structural-pin
    // precedent — and fails if the apply is deleted, moved out of
    // `tool_command`, or rewired to a literal that could drift from
    // `child_creation_flags`.
    #[test]
    fn the_windows_apply_site_still_passes_the_flag_from_one_chokepoint() {
        let src = include_str!("external_tool.rs");
        // Only the PRODUCTION half: this test's own string literals would
        // otherwise satisfy every assertion below.
        let production = src
            .split_once("#[cfg(test)]")
            .expect("the test module marker must exist")
            .0;
        assert_eq!(
            // The leading dot is what distinguishes the APPLY from the two
            // definitions (`child_creation_flags` / `creation_flags_for`).
            production.matches(".creation_flags(").count(),
            1,
            "exactly one apply site — a second Command builder that skipped it \
             would pop a console window and steal focus"
        );
        assert!(
            production.contains("cmd.creation_flags(child_creation_flags())"),
            "the apply must read the shared constant, not a literal"
        );
        // And that one site lives inside tool_command, not somewhere a start
        // path could bypass.
        let body = production
            .split_once("pub(crate) fn tool_command(")
            .expect("tool_command must exist")
            .1;
        assert!(
            body.contains("cmd.creation_flags(child_creation_flags())"),
            "the apply must live inside tool_command"
        );
    }

    #[test]
    fn merged_path_prefers_registry_over_stale_process_path_without_dupes() {
        // Registry entries (extra) come FIRST so a just-upgraded tool wins
        // over the stale process-PATH snapshot; process-only entries follow;
        // no duplicates. Built with the PLATFORM path-list separator
        // (';' on Windows, ':' elsewhere), the same `cfg!(windows)` choice
        // `merged_path` itself makes: a hardcoded ':' here split nothing out
        // of the Windows-separated base string, so on a Windows host the
        // "process-only entries follow" and "no duplicates" guarantees were
        // never actually exercised — the assert failed instead, but for the
        // wrong reason (a literal separator mismatch, not a merge defect).
        let sep = if cfg!(windows) { ';' } else { ':' };
        let base = ["/usr/bin", "/bin"].join(&sep.to_string());
        let extra = vec!["/opt/pandoc".to_string(), "/usr/bin".to_string()];
        let merged = merged_path(&base, &extra);
        let expected = ["/opt/pandoc", "/usr/bin", "/bin"].join(&sep.to_string());
        assert_eq!(merged, expected);
        assert_eq!(merged.matches("/usr/bin").count(), 1);
    }

    #[test]
    fn candidates_try_override_then_path_execs_then_bare_deduped() {
        let no_execs: Vec<String> = vec![];
        // No override, no PATH execs: just the bare-stem fallback.
        assert_eq!(
            candidate_order("pandoc", None, &no_execs),
            vec!["pandoc".to_string()]
        );
        // Override first, then each concrete PATH exec, then bare fallback.
        assert_eq!(
            candidate_order(
                "pandoc",
                Some("/custom/pandoc"),
                &[
                    "/usr/bin/pandoc".to_string(),
                    "/opt/pandoc/pandoc".to_string()
                ]
            ),
            vec![
                "/custom/pandoc".to_string(),
                "/usr/bin/pandoc".to_string(),
                "/opt/pandoc/pandoc".to_string(),
                "pandoc".to_string(),
            ]
        );
        // Blank override is treated as unset.
        assert_eq!(
            candidate_order("pandoc", Some("   "), &no_execs),
            vec!["pandoc".to_string()]
        );
        // A PATH exec / override literally "pandoc" is deduped against the fallback.
        assert_eq!(
            candidate_order("pandoc", Some("pandoc"), &["pandoc".to_string()]),
            vec!["pandoc".to_string()]
        );
        // Duplicate PATH execs collapse (case-insensitive), order preserved.
        assert_eq!(
            candidate_order(
                "pandoc",
                None,
                &["/usr/bin/pandoc".to_string(), "/usr/bin/pandoc".to_string()]
            ),
            vec!["/usr/bin/pandoc".to_string(), "pandoc".to_string()]
        );
        // The bare fallback is the STEM, so the same function serves ffmpeg.
        assert_eq!(
            candidate_order("ffmpeg", None, &no_execs),
            vec!["ffmpeg".to_string()]
        );
    }

    // PATH enumeration is now generic over the stem. A stem must match the
    // executable name EXACTLY, or a directory holding `ffmpeg-gui` would be
    // offered as ffmpeg and spawned in its place.
    //
    // NOTE (plan deviation, recorded deliberately): the plan asked for a
    // `matches_stem(stem, file_name)` helper here. There is nothing to factor
    // out — the lookup joins the exact platform file name rather than scanning
    // a directory's entries, so no name comparison exists. Extracting one
    // would have been dead code with only a test to call it; this test pins
    // the real production path instead, and fails if the exact join is ever
    // replaced by a prefix or substring scan.
    #[test]
    fn an_executable_matches_its_stem_exactly_and_not_as_a_prefix() {
        let dir = std::env::temp_dir().join(format!(
            "vb-exe-stem-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let exact = dir.join(exe_file_name("ffmpeg"));
        for decoy in [
            exe_file_name("ffmpeg-gui"),
            exe_file_name("myffmpeg"),
            exe_file_name("ffmpeg2"),
            // A prefix scan that ignored the platform extension would take this.
            "ffmpeg.bak".to_string(),
        ] {
            std::fs::write(dir.join(decoy), b"x").unwrap();
        }
        std::fs::write(&exact, b"x").unwrap();

        let hits = executables_in(&dir.to_string_lossy(), "ffmpeg");
        assert_eq!(
            hits,
            vec![exact.to_string_lossy().to_string()],
            "only the exact <stem> executable may be offered"
        );
        // And a stem with no exact match finds nothing, even though names
        // starting with it are present.
        assert!(executables_in(&dir.to_string_lossy(), "ffmpe").is_empty());
        // The Pandoc case that motivated the rule: pandoc-citeproc is not pandoc.
        std::fs::write(dir.join(exe_file_name("pandoc-citeproc")), b"x").unwrap();
        assert!(executables_in(&dir.to_string_lossy(), "pandoc").is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
