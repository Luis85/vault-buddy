//! App-side log-directory housekeeping: the unclean-shutdown run marker
//! and adoption of crash records written to the temp-dir fallback before
//! the log dir was known.
//! Pure functions over a directory so every branch is testable anywhere;
//! the shell wires them to the real app log dir.

use std::io::Write;
use std::path::Path;

/// Marker file recording whether the current/previous run ended cleanly.
/// Lives beside the logs so "Open logs folder" shows it too.
pub const RUN_MARKER: &str = ".vault-buddy.run";

pub enum PreviousRun {
    /// No marker (first run) or the previous run wrote "clean".
    CleanOrFirst,
    /// The previous run's marker still says running — crash, kill, power
    /// loss, or logoff. Carries the marker content for the log line.
    Unclean(String),
}

pub fn check_previous_run(dir: &Path) -> PreviousRun {
    match std::fs::read_to_string(dir.join(RUN_MARKER)) {
        Ok(content) if !content.starts_with("clean") => {
            PreviousRun::Unclean(content.trim().to_string())
        }
        _ => PreviousRun::CleanOrFirst,
    }
}

/// Stamp the marker as running. Called at startup, and again explicitly by
/// the shell crate's `rearm_running_marker` when an update install fails
/// after already stamping "clean" — the gate that stamp latches would
/// otherwise keep the heartbeat from ever writing "running" again, so the
/// frontend calls back in on that specific failure. Once re-armed, the
/// checkpoint heartbeat re-stamps this periodically as a backstop while the
/// app keeps running.
pub fn write_running_marker(dir: &Path, version: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join(RUN_MARKER),
        format!(
            "running v{version} since {}",
            chrono::Local::now().to_rfc3339()
        ),
    )
}

pub fn write_clean_marker(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(RUN_MARKER), "clean")
}

/// Fold a crash log written to the temp-dir fallback (panic before the log
/// dir was known) into the real crash.log, so it surfaces where "Open logs
/// folder" points. Appends, then removes the stray; Ok(false) = nothing to
/// adopt.
pub fn adopt_stray_crash_log(stray: &Path, dir: &Path) -> std::io::Result<bool> {
    if !stray.is_file() {
        return Ok(false);
    }
    let bytes = std::fs::read(stray)?;
    std::fs::create_dir_all(dir)?;
    let mut out = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("crash.log"))?;
    out.write_all(&bytes)?;
    out.sync_all()?;
    std::fs::remove_file(stray)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_and_clean_marker_are_not_unclean() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            check_previous_run(dir.path()),
            PreviousRun::CleanOrFirst
        ));
        write_running_marker(dir.path(), "9.9.9").unwrap();
        write_clean_marker(dir.path()).unwrap();
        assert!(matches!(
            check_previous_run(dir.path()),
            PreviousRun::CleanOrFirst
        ));
    }

    #[test]
    fn stale_running_marker_reports_unclean_with_content() {
        let dir = tempfile::tempdir().unwrap();
        write_running_marker(dir.path(), "9.9.9").unwrap();
        match check_previous_run(dir.path()) {
            PreviousRun::Unclean(content) => {
                assert!(content.contains("running v9.9.9"), "{content}");
            }
            PreviousRun::CleanOrFirst => panic!("stale running marker must report unclean"),
        }
    }

    #[test]
    fn running_marker_overwrites_a_stale_clean_stamp() {
        // The heartbeat re-arms after a premature "clean" (failed update
        // install keeps the app alive) — running must win over clean.
        let dir = tempfile::tempdir().unwrap();
        write_clean_marker(dir.path()).unwrap();
        write_running_marker(dir.path(), "9.9.9").unwrap();
        assert!(matches!(
            check_previous_run(dir.path()),
            PreviousRun::Unclean(_)
        ));
    }

    #[test]
    fn stray_crash_log_is_appended_and_removed() {
        let temp = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let stray = temp.path().join("vault-buddy-crash.log");
        std::fs::write(&stray, "early panic\n").unwrap();
        std::fs::write(logs.path().join("crash.log"), "existing\n").unwrap();
        assert!(adopt_stray_crash_log(&stray, logs.path()).unwrap());
        assert!(!stray.exists(), "stray removed after adoption");
        let merged = std::fs::read_to_string(logs.path().join("crash.log")).unwrap();
        assert_eq!(merged, "existing\nearly panic\n");
        // idempotent: nothing left to adopt
        assert!(!adopt_stray_crash_log(&stray, logs.path()).unwrap());
    }
}
