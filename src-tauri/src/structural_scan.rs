//! Shared machinery for the shell's structural pins.
//!
//! Several invariants in this crate are ORDERINGS or SINGLE-SITE claims that
//! no value assertion can reach: a `#[cfg(windows)]` apply site, a release
//! that must live in exactly one function, a rollback that must accompany
//! every early return. The established remedy is a source scan, and three
//! files had independently grown their own — `config_lock_guard.rs` first,
//! then `capture_exclusion.rs`, then `screen_commands.rs`.
//!
//! They did not grow the same one, and that is the reason this module
//! exists: `capture_exclusion.rs` walks the WHOLE shell tree, while
//! `screen_commands.rs`'s `CaptureGuard` pin `include_str!`s two named
//! files. A rogue `release(CaptureKind::Screen)` in any of the other twenty
//! shell modules left the entire suite green — proven by injecting one into
//! `staged_commands.rs`. A scan is only worth what its FILE SET covers, so
//! the file set belongs in one place that every pin reads.
//!
//! Test-only: nothing in production depends on it, so `lib.rs` gates the
//! whole module on `cfg(test)` — the `config_lock_guard` / `window_close_guard`
//! precedent.

use std::path::{Path, PathBuf};

/// The production half of a source file: everything before its inline
/// `#[cfg(test)]` module, so a needle spelled out in a test cannot satisfy
/// an assertion about production code.
pub(crate) fn production_half(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap_or(src)
}

/// `line` with its string literals and its trailing `//` comment removed.
///
/// Two jobs. For the brace walks it keeps `format!("...: {e}")` from
/// reading as a block that opens and closes. For the call-site counts it
/// keeps PROSE from counting: this crate documents its own invariants by
/// quoting them, so `release(CaptureKind::Screen)` appears in four module
/// docs that call nothing at all.
pub(crate) fn code_only(line: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_str => {
                chars.next();
            }
            '"' => in_str = !in_str,
            '/' if !in_str && chars.peek() == Some(&'/') => break,
            _ if !in_str => out.push(c),
            _ => {}
        }
    }
    out
}

/// `production_half` with every line run through `code_only`, line
/// structure preserved.
pub(crate) fn production_code(src: &str) -> String {
    production_half(src)
        .lines()
        .map(code_only)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Byte offset of `needle`, with a message that names it.
pub(crate) fn offset_of(src: &str, needle: &str) -> usize {
    src.find(needle)
        .unwrap_or_else(|| panic!("expected to find {needle:?} in the production source"))
}

/// Recursively collect every `.rs` file under `dir`, skipping THIS file —
/// which necessarily names the things the scans search for.
fn rust_files(dir: &Path, self_name: &std::ffi::OsStr, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, self_name, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
            && path.file_name() != Some(self_name)
        {
            out.push(path);
        }
    }
}

/// Every shell source file, paired with its production CODE (test modules,
/// comments and string literals removed).
///
/// Rooted at `CARGO_MANIFEST_DIR`, never the CWD, and self-checked for
/// vacuity: an empty walk would make every assertion built on it trivially
/// true, which is the failure mode a structural pin is least able to
/// notice about itself.
pub(crate) fn shell_sources() -> Vec<(PathBuf, String)> {
    let shell_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let self_name = Path::new(file!())
        .file_name()
        .expect("file!() always has a file name")
        .to_owned();
    let mut files = Vec::new();
    rust_files(&shell_src, &self_name, &mut files);
    assert!(
        files.len() > 20,
        "scan under {shell_src:?} found only {} file(s) -- the walk is broken, not the \
         invariant",
        files.len()
    );
    files
        .into_iter()
        .map(|p| {
            let text =
                std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("could not read {p:?}: {e}"));
            let code = production_code(&text);
            (p, code)
        })
        .collect()
}

/// One entry per occurrence of `needle` in the shell's production code,
/// naming the file it was found in — so a failure says WHERE the extra call
/// site is rather than only that the count is wrong.
///
/// Known limit (the `config_lock_guard.rs` precedent's): the scan matches a
/// literal, so an aliased re-export would add a site it cannot see.
/// Removing a site is still caught.
pub(crate) fn call_sites(needle: &str) -> Vec<String> {
    let mut hits = Vec::new();
    for (path, code) in shell_sources() {
        for _ in 0..code.matches(needle).count() {
            hits.push(path.display().to_string());
        }
    }
    hits
}

/// Every `needle` call in the shell's production code, as
/// `(file, argument text)`.
///
/// Counting a WHOLE call as a literal (`release(CaptureKind::Screen)`) sees
/// only one of its spellings: the same call written
/// `release(crate::capture_guard::CaptureKind::Screen)` is invisible to it,
/// and that is not hypothetical — it is how a module that does not already
/// import the type has to write it. So the needle names the METHOD and the
/// caller classifies the ARGUMENT, which survives any path qualifier.
///
/// The argument is the text from the needle to the next `)`, read across
/// line breaks; the calls this crate scans take one simple path argument,
/// so no paren matching is needed.
pub(crate) fn call_args(needle: &str) -> Vec<(String, String)> {
    let mut hits = Vec::new();
    for (path, code) in shell_sources() {
        for (at, _) in code.match_indices(needle) {
            let rest = &code[at + needle.len()..];
            let arg = rest.split(')').next().unwrap_or(rest);
            hits.push((path.display().to_string(), arg.trim().to_string()));
        }
    }
    hits
}

/// The production code of ONE shell source file, by file name — via
/// `shell_sources`, so comments, doc comments and string literals are gone
/// before anything is matched. This crate documents its invariants by
/// quoting them, and prose naming a predicate must never satisfy an
/// assertion that it is CALLED.
///
/// Promoted out of the (since retired) export-shutdown module's tests when
/// `shutdown_gate` needed the same two helpers: a scan helper that exists
/// twice is the exact drift this module was created to stop.
pub(crate) fn shell_file(name: &str) -> String {
    shell_sources()
        .into_iter()
        .find(|(p, _)| p.file_name().and_then(|n| n.to_str()) == Some(name))
        .unwrap_or_else(|| panic!("{name} must exist in the shell"))
        .1
}

/// The body of the function introduced by `sig`, up to its closing brace at
/// column 0.
pub(crate) fn fn_body<'a>(code: &'a str, sig: &str) -> &'a str {
    let start = offset_of(code, sig);
    let end = code[start..]
        .find("\n}")
        .map(|i| start + i)
        .unwrap_or_else(|| panic!("{sig} must have a closing brace at column 0"));
    &code[start..end]
}
