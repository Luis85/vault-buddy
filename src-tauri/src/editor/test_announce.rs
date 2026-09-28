//! How a shell test says it SKIPPED (hardening Task 21, M-V11/T-6;
//! test-only).
//!
//! A test that cannot run here -- no ffmpeg on PATH, an account without the
//! symlink privilege, a filesystem that writes through a read-only folder --
//! returns early and reports `ok`. A skip is not a pass, so it must SAY so,
//! and `eprintln!` does not: libtest captures `print!`/`eprint!` for every
//! test that passes, so the line was only ever shown under `--nocapture`,
//! and a green run on a host without ffmpeg looked exactly like a run that
//! proved the ffmpeg round trips. `writeln!` to the process's own stderr is
//! not captured, so this line reaches the terminal (and the CI log) every
//! time -- the screen crate's `tests/render_support::announce` shape.

use std::io::Write as _;

/// Print `SKIP: <what>` to the real stderr, past the test harness's
/// output capture. `what` says why the test could not run; when it does
/// not start with the test's own name, the name libtest gave the test's
/// thread is put in front, so every line says WHICH test proved nothing.
pub(crate) fn announce_skip(what: &str) {
    let thread = std::thread::current();
    let test = thread.name().unwrap_or("");
    let own = test.rsplit("::").next().unwrap_or(test);
    let line = if own.is_empty() || what.starts_with(own) {
        format!("SKIP: {what}")
    } else {
        format!("SKIP: {test}: {what}")
    };
    let _ = writeln!(std::io::stderr(), "{line}");
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("readable").flatten() {
            let path = entry.path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    // T-6: every skip in the shell crate goes through `announce_skip`. An
    // `eprintln!` whose text (on its line or the two after it, where
    // rustfmt puts a long literal) says SKIP is captured by libtest and so
    // invisible in an ordinary run.
    #[test]
    fn no_shell_test_announces_a_skip_through_captured_output() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        assert!(files.len() > 40, "the walk found {} files", files.len());
        let mut offenders = Vec::new();
        for path in files {
            let text = std::fs::read_to_string(&path).expect("readable");
            let lines: Vec<&str> = text.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let printed = ["eprintln!(", "println!(", "eprint!(", "print!("]
                    .iter()
                    .any(|m| line.contains(m));
                let says_skip = lines[i..(i + 3).min(lines.len())]
                    .iter()
                    .any(|l| l.contains("SKIP"));
                if printed && says_skip && !line.trim_start().starts_with("//") {
                    offenders.push(format!("{}:{}", path.to_string_lossy(), i + 1));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a skip printed through captured output is invisible in a passing run; use \
             test_announce::announce_skip:\n{}",
            offenders.join("\n")
        );
    }
}
