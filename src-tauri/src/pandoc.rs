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

/// Windows process-creation flags for a spawned Pandoc child: `CREATE_NO_WINDOW`
/// (0x0800_0000) on Windows, 0 elsewhere.
///
/// Vault Buddy is a GUI-subsystem app in release (`windows_subsystem =
/// "windows"` in main.rs), so it owns no console. Spawning a console program
/// like `pandoc.exe` with the default flags makes Windows allocate a NEW
/// console window that flashes on screen AND grabs foreground focus. That focus
/// theft blurs the panel and trips its focus-out auto-hide
/// (`schedule_focus_out_check` in lib.rs) — so the Pandoc `--version` probe,
/// which runs the moment `DocumentImportSettings` mounts (opening Buddy
/// settings, the record chooser, or the import picker), flashed a terminal and
/// slammed the settings panel shut. Spawning headless removes the window
/// entirely; the piped stdout/stderr `run_capturing` relies on are unaffected.
///
/// `cfg!(windows)` (not `#[cfg]`) so both arms compile everywhere and the flag
/// value stays unit-testable on Linux, where the shell crate's tests run.
#[cfg_attr(not(windows), allow(dead_code))]
const fn child_creation_flags() -> u32 {
    if cfg!(windows) {
        0x0800_0000 // CREATE_NO_WINDOW
    } else {
        0
    }
}

/// Build a Command with the registry-augmented PATH so PATH lookup sees a
/// fresh install. On Windows the child is spawned headless (see
/// `child_creation_flags`) so no probe or conversion pops a console window.
pub(crate) fn pandoc_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    if let Some(path) = augmented_path() {
        cmd.env("PATH", path);
    }
    // One chokepoint for the no-console-window flag: BOTH the `--version` probe
    // and the conversion build their Command here, so both inherit it.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(child_creation_flags());
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

/// Filename of the "text only" image-strip Lua filter. `convert_blocking`
/// writes it into the per-import staging dir and passes it to Pandoc relative
/// to Pandoc's cwd (= that staging dir). A plain (non-dot) name is fine: it
/// lives inside the already-hidden, already-cleaned staging dir.
pub(crate) const STRIP_IMAGES_FILTER: &str = "strip-images.lua";

/// The image-strip Lua filter body. App-authored and I/O-free: it only
/// deletes Image/Figure nodes from the parsed document, so it does NOT weaken
/// `--sandbox`'s protection of the untrusted document read. Handles both older
/// Pandoc (an implicit figure is a Para holding one Image — its inline Image is
/// dropped) and Pandoc 3.x (an explicit Figure block); a handler for an element
/// a given Pandoc version never produces simply never fires.
pub(crate) const STRIP_IMAGES_LUA: &str = "\
-- Vault Buddy: \"text only\" document import — drop all images so the
-- imported note carries only text (no image links, no media folder).
function Image() return {} end
function Figure() return {} end
";

/// Pandoc argument vector (program excluded). Source is added by the caller as
/// an absolute path; every OUTPUT here is relative (Pandoc runs with cwd =
/// work dir) so rewritten image links stay valid after publish.
///
/// `extract_images` picks the media behavior: true extracts embedded/linked
/// media into the reserved sibling folder (`--extract-media`, the default);
/// false strips all images via the app-authored `--lua-filter` and creates NO
/// media folder — the per-vault "text only" mode. `--sandbox` and the heap cap
/// are present either way.
pub(crate) fn pandoc_args(
    reader: &str,
    media_name: &str,
    note_name: &str,
    extract_images: bool,
) -> Vec<String> {
    let mut args = vec![
        "-f".into(),
        reader.into(),
        "-t".into(),
        "gfm".into(),
        "--sandbox".into(),
    ];
    if extract_images {
        args.push(format!("--extract-media={media_name}"));
    } else {
        // Text only: strip images instead of extracting them. Without
        // --extract-media no media folder is created; the filter drops the
        // links so the note has no dangling image references.
        args.push(format!("--lua-filter={STRIP_IMAGES_FILTER}"));
    }
    args.extend([
        "-o".into(),
        note_name.into(),
        // GHC RTS heap cap: a timeout bounds time, not memory; a crafted doc
        // could OOM before it fires. Pandoc dies with a memory error instead.
        "+RTS".into(),
        "-M512M".into(),
        "-RTS".into(),
    ]);
    args
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

/// How long `run_capturing` waits for the pipe drain AFTER the child exits.
/// A real child closes its pipe on exit, so the reader delivers at once and
/// this is never hit; it only bounds the pathological case where a surviving
/// descendant keeps the inherited pipe open (Codex review) so the call can't
/// block — critically while `ImportLock` is held.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Max bytes `run_capturing` STORES from the captured stream. We only ever use
/// the first `--version` line or a 500-char stderr slice, so 64 KiB is ample;
/// it exists to bound memory against a child that floods the pipe for the whole
/// timeout window. Excess is drained but discarded.
const CAPTURE_CAP: usize = 64 * 1024;

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
            .name("pandoc-io".into())
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
    fn sandbox_requires_2_15_or_newer() {
        assert!(!sandbox_supported(2, 14));
        assert!(sandbox_supported(2, 15));
        assert!(sandbox_supported(3, 1));
        assert!(sandbox_supported(2, 20));
    }

    #[test]
    fn pandoc_children_are_spawned_headless_on_windows_only() {
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
        if cfg!(windows) {
            // 0x0800_0000 == winapi CREATE_NO_WINDOW.
            assert_eq!(child_creation_flags(), 0x0800_0000);
        } else {
            // No console-window concept off Windows — spawn with default flags.
            assert_eq!(child_creation_flags(), 0);
        }
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
        let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md", true);
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
        // images mode does NOT add the strip filter
        assert!(!args.iter().any(|a| a.starts_with("--lua-filter")));
    }

    #[test]
    fn text_only_args_strip_images_and_skip_extract_media() {
        // extract_images = false: the strip filter replaces --extract-media, so
        // Pandoc never writes a media folder and the note ends up text-only.
        let args = pandoc_args("docx", "2026-07-10 Report", "2026-07-10 Report.md", false);
        assert!(args
            .iter()
            .any(|a| a == &format!("--lua-filter={STRIP_IMAGES_FILTER}")));
        assert!(!args.iter().any(|a| a.starts_with("--extract-media")));
        // Still sandboxed, GFM, and heap-capped in text-only mode.
        assert!(args.iter().any(|a| a == "--sandbox"));
        assert!(args.windows(2).any(|w| w == ["-t", "gfm"]));
        assert!(args.join(" ").contains("+RTS -M512M -RTS"));
        // The filter body actually removes images.
        assert!(STRIP_IMAGES_LUA.contains("function Image()"));
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
