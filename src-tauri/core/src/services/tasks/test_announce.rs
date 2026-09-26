//! How a core test says it SKIPPED (Task 23 carried finding).
//!
//! Three tests in this module self-skip when the sandbox they run in
//! defeats the privilege probe they need (running as root, an unsupported
//! filesystem for `chattr +i`) — and reported that through `eprintln!`,
//! which libtest captures for every test that PASSES. A skip is not a
//! pass, so it must say so somewhere the harness cannot swallow; `writeln!`
//! to the process's own stderr is not captured, so this line reaches the
//! terminal (and the CI log) every time. Mirrors the shell's own
//! `editor::test_announce::announce_skip` (Task 21), copied rather than
//! shared because `core` and the shell crate do not share a dependency
//! edge that way.
use std::io::Write as _;

/// Print `SKIP: <what>` to the real stderr, past the test harness's output
/// capture. `what` says why the test could not run; when it does not start
/// with the test's own name, the name libtest gave the test's thread is put
/// in front, so every line says WHICH test proved nothing.
///
/// All three current call sites are `#[cfg(unix)]` privilege probes (chmod,
/// `chattr +i`) with no Windows equivalent, so this is genuinely unused on
/// that target — the `external_tool.rs` `cfg_attr(not(windows), ...)`
/// precedent, inverted.
#[cfg_attr(not(unix), allow(dead_code))]
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

    // The shell crate's own `editor::test_announce` regression (Task 21),
    // applied to this crate: a skip printed through `eprintln!`/`println!`
    // is captured by libtest for every test that PASSES, so it is invisible
    // in an ordinary run and a green CI log proves nothing about whether the
    // test actually ran.
    #[test]
    fn no_core_test_announces_a_skip_through_captured_output() {
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
