//! Pandoc toolchain: resolving a working `pandoc` executable (config override
//! → registry-augmented PATH → bare fallback), building its sandboxed
//! argument vector, and running it with a wall-clock kill. Split out of
//! `document_commands.rs` (which keeps the IPC surface — detection status,
//! conversion command, settings, import recovery) purely to stay under the
//! per-file LOC cap; no behavior changes.

use std::process::Command;
use std::time::Duration;
use vault_buddy_core::capture_config;

/// First line of `pandoc --version` is `pandoc <x.y.z>`; return (major, minor).
fn parse_pandoc_version(stdout: &str) -> Option<(u32, u32)> {
    let first = stdout.lines().next()?;
    let ver = first.split_whitespace().nth(1)?;
    let mut parts = ver.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor))
}

/// `--sandbox` landed in Pandoc 2.15.
pub(crate) fn sandbox_supported(major: u32, minor: u32) -> bool {
    (major, minor) >= (2, 15)
}

/// Merge the freshly-read registry PATH (`extra`) with the process PATH
/// (`base`), **registry entries FIRST** — deduped case-insensitively. The
/// registry is preferred because `base` is the process's launch-time PATH
/// snapshot, which is STALE: when a Windows user upgrades or relocates Pandoc
/// while Vault Buddy is running, the new location lands in the registry but not
/// in the process env. Searching the stale process PATH first would keep
/// resolving the OLD `pandoc.exe`, so Recheck would report the stale/unsupported
/// version until restart (Codex review). Registry-first fixes that; any
/// process-only entries (session additions not in the registry) still follow.
fn merged_path(base: &str, extra: &[String]) -> String {
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
/// Pandoc is visible without restarting (a running process keeps its launch
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
/// merge so `path_pandoc_executables` (enumeration) and `pandoc_command`
/// (spawn) don't each re-read the registry and re-run `merged_path`.
fn augmented_path() -> Option<String> {
    let extra = registry_path_entries();
    if extra.is_empty() {
        return None;
    }
    Some(merged_path(
        &std::env::var("PATH").unwrap_or_default(),
        &extra,
    ))
}

/// Ordered pandoc candidates to try: the configured override FIRST (if
/// non-empty), then each concrete pandoc executable found across PATH, then a
/// bare `pandoc` final fallback — deduped, preserving order. Pure so it's
/// testable without touching the real config or filesystem — `pandoc_candidates`
/// below is the impure wrapper that feeds it the real config + PATH executables.
///
/// The concrete PATH executables matter (Codex review): a bare `pandoc` alone
/// resolves only the FIRST PATH match, so a multi-install state (old pre-2.15
/// pandoc earlier in PATH, a supported one later) would let detection report
/// "too old" even though a valid Pandoc exists. Listing every PATH pandoc lets
/// `resolve_working_pandoc` probe PAST an unsupported hit to a sandbox-capable
/// one. Bare `pandoc` is kept as a final fallback so we never resolve WORSE
/// than the plain lookup (e.g. if PATH enumeration misses a shell-resolved one).
fn candidate_order(override_path: Option<&str>, path_execs: &[String]) -> Vec<String> {
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
    push("pandoc".to_string());
    out
}

/// Concrete `pandoc`/`pandoc.exe` executables across the registry-augmented
/// PATH, in PATH order — so `resolve_working_pandoc` can probe past an old
/// install to a supported one later in PATH (Codex review). Best-effort: a
/// non-existent or unreadable dir is skipped.
fn path_pandoc_executables() -> Vec<String> {
    let sep = if cfg!(windows) { ';' } else { ':' };
    let exe = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    let merged = augmented_path().unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let mut out = Vec::new();
    for dir in merged.split(sep) {
        if dir.is_empty() {
            continue;
        }
        let cand = std::path::Path::new(dir).join(exe);
        // is_file() follows a symlink to a real file — a symlinked pandoc counts.
        if cand.is_file() {
            out.push(cand.to_string_lossy().to_string());
        }
    }
    out
}

/// Ordered pandoc candidates (see `candidate_order`), fed the real config
/// override and the concrete PATH executables.
fn pandoc_candidates() -> Vec<String> {
    candidate_order(
        capture_config::load_config()
            .document_import
            .pandoc_path
            .as_deref(),
        &path_pandoc_executables(),
    )
}

/// Build a Command with the registry-augmented PATH so PATH lookup sees a
/// fresh install.
pub(crate) fn pandoc_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
    cmd
}

/// Max wall-clock for a `--version` probe. `--version` returns near-instantly
/// for a real Pandoc, so this is generous; it exists only to bound a
/// pathological candidate (a wrapper/network binary that hangs) so it can't
/// block the probe loop — critically in the import path, where the probe runs
/// while `ImportLock` is held (Codex review).
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Probe one candidate: run `<program> --version` (bounded — see `PROBE_TIMEOUT`).
/// On success, return the program string, its parsed (major, minor), AND the
/// first `--version` line for display — all from the SAME spawn. None if it
/// can't run, times out, or exits non-zero (so the caller falls through to the
/// next candidate).
fn probe_pandoc(program: &str) -> Option<(String, u32, u32, String)> {
    let mut cmd = pandoc_command(program);
    cmd.arg("--version");
    let (ok, stdout) = run_capturing(cmd, PROBE_TIMEOUT, Capture::Stdout).ok()?;
    if !ok {
        return None;
    }
    let (major, minor) = parse_pandoc_version(&stdout)?;
    // Reuse the version output we already have for the display line — a second
    // spawn (and, on Windows, a second registry PATH read) just to re-read the
    // first line would be wasteful.
    let line = stdout
        .lines()
        .next()
        .map(|l| l.trim().to_string())
        .unwrap_or_default();
    Some((program.to_string(), major, minor, line))
}

/// Resolve a pandoc to use across the ordered candidates (override → PATH),
/// with its full first `--version` line for display. None if no candidate
/// runs.
///
/// **Prefer a candidate that meets the sandbox minimum** (Codex review): a
/// working-but-old (< 2.15) override must not shadow a supported Pandoc on
/// PATH. Returning the old override here would make `convert_document` reject
/// it at the sandbox gate and never probe PATH, so imports stay broken until
/// the user clears the override. So we keep probing past a too-old runnable
/// candidate and return the first sandbox-capable one; only if NONE is
/// sandbox-capable do we return the first runnable (old) one, so
/// `detect_pandoc` can still report an accurate "installed but too old"
/// status (and `convert_document` still rejects it — nothing usable exists).
pub(crate) fn resolve_working_pandoc() -> Option<(String, u32, u32, String)> {
    let mut too_old: Option<(String, u32, u32, String)> = None;
    for program in pandoc_candidates() {
        if let Some(hit) = probe_pandoc(&program) {
            if sandbox_supported(hit.1, hit.2) {
                return Some(hit);
            }
            // Runnable but too old — remember the FIRST such one and keep
            // looking for a newer candidate.
            too_old.get_or_insert(hit);
        }
    }
    too_old
}

/// Pandoc argument vector (program excluded). Source is added by the caller as
/// an absolute path; every OUTPUT here is relative (Pandoc runs with cwd =
/// work dir) so rewritten image links stay valid after publish.
pub(crate) fn pandoc_args(reader: &str, media_name: &str, note_name: &str) -> Vec<String> {
    vec![
        "-f".into(),
        reader.into(),
        "-t".into(),
        "gfm".into(),
        "--sandbox".into(),
        format!("--extract-media={media_name}"),
        "-o".into(),
        note_name.into(),
        // GHC RTS heap cap: a timeout bounds time, not memory; a crafted doc
        // could OOM before it fires. Pandoc dies with a memory error instead.
        "+RTS".into(),
        "-M512M".into(),
        "-RTS".into(),
    ]
}

/// Max wall-clock for a single conversion before the child is killed.
pub(crate) const CONVERT_TIMEOUT: Duration = Duration::from_secs(120);

/// Which child stream `run_capturing` pipes back: conversion wants stderr (the
/// failure reason), the `--version` probe wants stdout (the version line).
#[derive(Clone, Copy)]
pub(crate) enum Capture {
    Stdout,
    Stderr,
}

/// Spawn `cmd`, wait with a wall-clock kill at `timeout`, and return
/// `(success, captured)`: `success` is true on a zero-exit, false on
/// non-zero/killed; `captured` is the chosen stream's output (empty when none).
/// The kill matters for BOTH callers — a hung `--version` probe (a wrapper or
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
    let drain = stream.and_then(|mut s| {
        std::thread::Builder::new()
            .name("pandoc-io".into())
            .spawn(move || {
                let mut buf = Vec::new();
                let _ = s.read_to_end(&mut buf);
                buf
            })
            .ok()
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
    // On a NORMAL exit the child closed its pipe, so joining the drain returns
    // its output at once. On a TIMEOUT we must NOT join: killing the child does
    // not reap a grandchild it may have forked, and that grandchild can keep the
    // pipe's write end open — so `read_to_end` (and thus the join) would block
    // as long as it lives, recreating the very hang this timeout bounds. We
    // don't need the output on a failure, so drop the handle and let the named
    // reader finish detached.
    let captured = if timed_out {
        String::new()
    } else {
        drain
            .and_then(|h| h.join().ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default()
    };
    Ok((success, captured))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pandoc_version_line() {
        assert_eq!(
            parse_pandoc_version("pandoc 3.1.9\nCompiled with..."),
            Some((3, 1))
        );
        assert_eq!(parse_pandoc_version("pandoc.exe 2.14.2"), Some((2, 14)));
        assert_eq!(parse_pandoc_version("not pandoc"), None);
    }

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
        // reported as failure — the guarantee that a wedged `--version` can't
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

    #[test]
    fn sandbox_requires_2_15_or_newer() {
        assert!(!sandbox_supported(2, 14));
        assert!(sandbox_supported(2, 15));
        assert!(sandbox_supported(3, 1));
        assert!(sandbox_supported(2, 20));
    }

    #[test]
    fn merged_path_prefers_registry_over_stale_process_path_without_dupes() {
        // Registry entries (extra) come FIRST so a just-upgraded Pandoc wins
        // over the stale process-PATH snapshot; process-only entries follow;
        // no duplicates.
        let merged = merged_path("/usr/bin:/bin", &["/opt/pandoc".into(), "/usr/bin".into()]);
        assert_eq!(merged, "/opt/pandoc:/usr/bin:/bin");
        assert_eq!(merged.matches("/usr/bin").count(), 1);
    }

    #[test]
    fn pandoc_args_are_sandboxed_relative_and_heap_capped() {
        let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md");
        // reader
        assert!(args.windows(2).any(|w| w == ["-f", "docx"]));
        assert!(args.windows(2).any(|w| w == ["-t", "gfm"]));
        // sandbox always present
        assert!(args.iter().any(|a| a == "--sandbox"));
        // relative extract-media + output (no temp path baked in)
        assert!(args
            .iter()
            .any(|a| a == "--extract-media=2026-07-10 Report"));
        assert!(args.windows(2).any(|w| w == ["-o", "2026-07-10 Report.md"]));
        // heap cap
        let joined = args.join(" ");
        assert!(joined.contains("+RTS -M512M -RTS"));
    }

    #[test]
    fn candidates_try_override_then_path_execs_then_bare_deduped() {
        let no_execs: Vec<String> = vec![];
        // No override, no PATH execs: just the bare-pandoc fallback.
        assert_eq!(candidate_order(None, &no_execs), vec!["pandoc".to_string()]);
        // Override first, then each concrete PATH exec, then bare fallback.
        assert_eq!(
            candidate_order(
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
            candidate_order(Some("   "), &no_execs),
            vec!["pandoc".to_string()]
        );
        // A PATH exec / override literally "pandoc" is deduped against the fallback.
        assert_eq!(
            candidate_order(Some("pandoc"), &["pandoc".to_string()]),
            vec!["pandoc".to_string()]
        );
        // Duplicate PATH execs collapse (case-insensitive), order preserved.
        assert_eq!(
            candidate_order(
                None,
                &["/usr/bin/pandoc".to_string(), "/usr/bin/pandoc".to_string()]
            ),
            vec!["/usr/bin/pandoc".to_string(), "pandoc".to_string()]
        );
    }
}
