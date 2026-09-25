//! Structural pin for the editor's content-free logs (Task 58; F-50;
//! test-only): every `log::` call in every `.rs` file under `src/editor/`
//! — subdirectories included, so a module moved in later (Task 59's
//! `export_worker/vault_dir.rs`) is covered the day it lands — formats a
//! path, a file or capture name, a title, a caption or cue text only
//! through `redact::redact_path`/`redact_name`.
//!
//! Three rules, each naming the file and line of the offending call:
//!
//! 1. **By name.** A formatted argument whose expression has a word
//!    `path`, `file`, `name`, `title`, `text`, `caption` or `base` in it
//!    (`webcam.file`, `entry.file_name()`, an inline `{base:?}`) and no
//!    `redact` call. `base` is not in the brief's list; it is here because
//!    a staged capture's base is `<date> <recorded window title>`.
//! 2. **`.display()`, whatever the name** (F38). `dir.display()` matches no
//!    word above and prints the whole path — exactly what
//!    `export_worker/vault_dir.rs` does today. There is no content-free use
//!    of `.display()` in a log line, so none is allowed.
//! 3. **`{:?}` of a `Path`/`PathBuf`, whatever the name** (F38). A scan has
//!    no types, so "Path-typed" is what this file SAYS: an identifier
//!    declared `x: &Path`/`&PathBuf`/`PathBuf` (a parameter, a field or a
//!    `let`), bound by `let x = <…Path::new/PathBuf::from/.join(/
//!    .to_path_buf()/.with_file_name(/.with_extension(…>`, or an argument
//!    that is itself such a call. A path reached some other way (a
//!    function returning `PathBuf`, a tuple field) is not seen — rules 1
//!    and 2 still catch the common spellings of it.
//!
//! Known limits: a value formatted into a `String` first and then logged
//! (`log::warn!("{msg}")`) is not followed, and a `Display` impl that
//! prints a path (a third-party error) is not visible to a scan.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The words rule 1 looks for, as whole identifier segments.
const WORDS: &[&str] = &["path", "file", "name", "title", "text", "caption", "base"];
const LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

/// `src` with every `//` comment blanked (string literals and line
/// numbers kept), so a log call quoted in a doc comment is not a call.
fn without_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    let (mut in_str, mut in_comment) = (false, false);
    while let Some(c) = chars.next() {
        if in_comment {
            if c == '\n' {
                in_comment = false;
                out.push('\n');
            } else {
                out.push(' ');
            }
            continue;
        }
        match c {
            '\\' if in_str => {
                out.push(c);
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '"' => {
                in_str = !in_str;
                out.push(c);
            }
            '/' if !in_str && chars.peek() == Some(&'/') => {
                in_comment = true;
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Byte offsets of every `log::<level>!(` / bare `<level>!(` call and the
/// offset just past its `(`.
fn macro_starts(code: &str) -> Vec<(usize, usize)> {
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    for level in LEVELS {
        let needle = format!("{level}!(");
        let mut from = 0;
        while let Some(i) = code[from..].find(&needle) {
            let at = from + i;
            from = at + needle.len();
            let before = if at == 0 { b' ' } else { bytes[at - 1] };
            let start = if code[..at].ends_with("log::") {
                at - "log::".len()
            } else if before.is_ascii_alphanumeric() || before == b'_' || before == b':' {
                continue;
            } else {
                at
            };
            out.push((start, from));
        }
    }
    out.sort_unstable();
    out
}

/// The text between `open` (just past a `(`) and its matching `)`.
fn balanced(code: &str, open: usize) -> &str {
    let mut depth = 1;
    let mut in_str = false;
    let mut chars = code[open..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' if in_str => {
                chars.next();
            }
            '"' => in_str = !in_str,
            '(' | '[' | '{' if !in_str => depth += 1,
            ')' | ']' | '}' if !in_str => {
                depth -= 1;
                if depth == 0 {
                    return &code[open..open + i];
                }
            }
            _ => {}
        }
    }
    &code[open..]
}

/// Top-level comma-separated arguments.
fn split_args(args: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut depth, mut in_str, mut cur) = (0i32, false, String::new());
    let mut chars = args.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' if in_str => {
                cur.push(c);
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
                continue;
            }
            '"' => in_str = !in_str,
            '(' | '[' | '{' if !in_str => depth += 1,
            ')' | ']' | '}' if !in_str => depth -= 1,
            ',' if !in_str && depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// A format string's placeholders as `(selector, is_debug)`: `""` for the
/// next positional argument, digits for an index, an identifier for a
/// named argument or an inline capture.
fn placeholders(fmt: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut chars = fmt.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            if chars.peek() == Some(&'{') {
                chars.next();
                continue;
            }
            let mut inner = String::new();
            for n in chars.by_ref() {
                if n == '}' {
                    break;
                }
                inner.push(n);
            }
            let (selector, spec) = inner.split_once(':').unwrap_or((&inner, ""));
            out.push((selector.trim().to_string(), spec.contains('?')));
        } else if c == '}' && chars.peek() == Some(&'}') {
            chars.next();
        }
    }
    out
}

/// The identifiers `code` declares or binds as a `Path`/`PathBuf` (rule 3).
fn path_typed_idents(code: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let ident_before = |text: &str| -> Option<String> {
        let name: String = text
            .trim_end()
            .chars()
            .rev()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        (!name.is_empty()).then_some(name)
    };
    for ty in [
        ": &Path",
        ": &PathBuf",
        ": PathBuf",
        ": &std::path::Path",
        ": &'_ Path",
    ] {
        let mut from = 0;
        while let Some(i) = code[from..].find(ty) {
            let at = from + i;
            from = at + ty.len();
            let next = code[from..].chars().next().unwrap_or(' ');
            if next.is_ascii_alphanumeric() || next == '_' {
                continue; // `: PathBufExt` and the like
            }
            if let Some(name) = ident_before(&code[..at]) {
                out.insert(name);
            }
        }
    }
    for line in code.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("let ") else {
            continue;
        };
        let Some((lhs, rhs)) = rest.split_once('=') else {
            continue;
        };
        if is_path_expr(rhs) {
            if let Some(name) = ident_before(lhs.trim_start_matches("mut ")) {
                out.insert(name);
            }
        }
    }
    out
}

fn is_path_expr(expr: &str) -> bool {
    [
        "Path::new(",
        "PathBuf::from(",
        ".join(",
        ".to_path_buf()",
        ".with_file_name(",
        ".with_extension(",
    ]
    .iter()
    .any(|n| expr.contains(n))
}

fn segments(expr: &str) -> impl Iterator<Item = &str> {
    expr.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .flat_map(|s| s.split('_'))
        .filter(|s| !s.is_empty())
}

/// Why `expr`, formatted by a log call, breaks a rule — or `None`.
fn problem(expr: &str, debug: bool, paths: &HashSet<String>) -> Option<&'static str> {
    if expr.contains("redact") {
        return None;
    }
    if expr.contains(".display()") {
        return Some(".display() prints the whole path");
    }
    if segments(expr).any(|s| WORDS.contains(&s)) {
        return Some("names a path, file, name, title, text, caption or base");
    }
    let bare = expr.trim_start_matches('&').trim();
    if debug && (paths.contains(bare) || is_path_expr(expr)) {
        return Some("{:?} of a Path/PathBuf prints the whole path");
    }
    None
}

/// Every offending log call in `src`, as `"<file>:<line>: <why>: <call>"`.
pub(crate) fn findings(file: &str, src: &str) -> Vec<String> {
    let code = without_comments(src);
    let paths = path_typed_idents(&code);
    let mut out = Vec::new();
    for (start, open) in macro_starts(&code) {
        let body = balanced(&code, open);
        let mut args = split_args(body);
        if args.first().is_some_and(|a| a.starts_with("target:")) {
            args.remove(0);
        }
        let Some(fmt) = args.first().cloned() else {
            continue;
        };
        let rest = &args[1..];
        let named: Vec<(&str, &str)> = rest
            .iter()
            .filter_map(|a| a.split_once('='))
            .filter(|(k, v)| {
                !v.starts_with('=')
                    && k.trim()
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_')
            })
            .map(|(k, v)| (k.trim(), v.trim()))
            .collect();
        let mut exprs: Vec<(String, bool)> = Vec::new();
        let mut next = 0;
        for (selector, debug) in placeholders(&fmt) {
            let expr = if selector.is_empty() {
                next += 1;
                rest.get(next - 1).cloned()
            } else if let Ok(i) = selector.parse::<usize>() {
                rest.get(i).cloned()
            } else if let Some((_, v)) = named.iter().find(|(k, _)| *k == selector) {
                Some((*v).to_string())
            } else {
                Some(selector)
            };
            if let Some(expr) = expr {
                exprs.push((expr, debug));
            }
        }
        // `.display()` anywhere in the call, formatted or not.
        exprs.extend(rest.iter().map(|a| (a.clone(), false)));
        if let Some(why) = exprs.iter().find_map(|(e, d)| problem(e, *d, &paths)) {
            let line = code[..start].matches('\n').count() + 1;
            let call: String = code[start..open + body.len() + 1]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            out.push(format!("{file}:{line}: {why}: {call}"));
        }
    }
    out
}

fn editor_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir)
        .expect("src/editor is readable")
        .flatten()
    {
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            editor_files(&path, out);
        } else if name.ends_with(".rs")
            && !name.ends_with("_tests.rs")
            && !name.ends_with("_guard.rs")
        {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_logs_redact_paths() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("editor");
        let mut files = Vec::new();
        editor_files(&dir, &mut files);
        assert!(
            files.len() > 20,
            "the walk found only {} files",
            files.len()
        );
        let mut all = Vec::new();
        for path in files {
            let src = std::fs::read_to_string(&path).expect("readable");
            let name = path
                .strip_prefix(&dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            all.extend(findings(&name, &src));
        }
        assert!(
            all.is_empty(),
            "log lines under src/editor/ print content; format it with redact_path/redact_name:\n{}",
            all.join("\n")
        );
    }

    #[test]
    fn editor_logs_redact_display_calls_regardless_of_argument_name() {
        // `export_worker/vault_dir.rs`'s own shape: `dir` is none of the
        // name rule's words.
        let fixture =
            "fn f(dir: &Path) {\n    log::warn!(\"could not roll back {}\", dir.display());\n}\n";
        let found = findings("fixture.rs", fixture);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].starts_with("fixture.rs:2: .display()"),
            "{found:?}"
        );
    }

    #[test]
    fn a_named_argument_is_flagged_and_a_redacted_one_is_not() {
        let fixture = concat!(
            "fn f(base: &str, path: &Path, e: std::io::Error) {\n",
            "    log::warn!(\"refused {base:?}\");\n",
            "    log::warn!(\n        \"could not read {}: {e}\",\n        webcam.file\n    );\n",
            "    log::warn!(\"refused {}\", redact_name(base));\n",
            "    log::warn!(\"could not remove {}: {e}\", redact_path(path));\n",
            "    log::warn!(\"a thread panicked: {e} ({:?})\", e.kind());\n",
            "}\n",
        );
        let found = findings("fixture.rs", fixture);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(found[0].starts_with("fixture.rs:2: names"), "{found:?}");
        assert!(found[1].starts_with("fixture.rs:3: names"), "{found:?}");
    }

    #[test]
    fn debug_of_a_path_typed_value_is_flagged_whatever_its_name() {
        let fixture = concat!(
            "fn f(src: &Path) {\n",
            "    let out = dir.join(\"x\");\n",
            "    log::warn!(\"ffmpeg failed on {src:?}\");\n",
            "    log::warn!(\"wrote {:?}\", out);\n",
            "    log::warn!(\"{:?} took long\", limit);\n",
            "}\n",
        );
        let found = findings("fixture.rs", fixture);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!(
            found[0].starts_with("fixture.rs:3: {:?} of a Path"),
            "{found:?}"
        );
        assert!(
            found[1].starts_with("fixture.rs:4: {:?} of a Path"),
            "{found:?}"
        );
    }

    #[test]
    fn a_call_quoted_in_a_comment_is_not_a_call() {
        let fixture = "//! `log::warn!(\"{}\", path.display())` is what this forbids.\nfn f() {}\n";
        assert!(findings("fixture.rs", fixture).is_empty());
    }
}
