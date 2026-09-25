//! A structural pin over the ONE class of defect the Linux compile gate
//! cannot see: a name error inside a `#[cfg(windows)]` body.
//!
//! AGENTS.md sells the Linux shell build as catching "type errors, IPC
//! signature drift, and missing `cfg` gates locally instead of
//! push-and-wait". That is true of everything it COMPILES -- and a
//! `cfg(windows)` body is precisely what it does not compile. Nor can the
//! Windows-target clippy cover it: `cargo clippy -p vault-buddy --target
//! x86_64-pc-windows-msvc` cannot run in this container at any scope
//! (`ring` fails in cc-rs for want of MSVC's `lib.exe`), which is why
//! `set_display_affinity` lives in `vault_buddy_screen` instead.
//!
//! So between the two Linux gates there is a hole, and a real commit fell
//! through it. Extracting `window_upkeep.rs` out of `lib.rs` moved this
//! line into a sibling module:
//!
//! ```text
//! #[cfg(windows)]
//! if commands::primary_button_down() { return; }
//! ```
//!
//! In `lib.rs` that resolved, because `mod commands;` puts `commands` at the
//! crate root. From a sibling it needs `crate::commands::`. Every Linux gate
//! stayed green -- fmt, workspace clippy, the shell's own 205 tests, the
//! whole `tauri build --no-bundle` -- and the Windows job failed the build
//! with E0433 eleven minutes later.
//!
//! This test compiles nothing. It reads the source and requires every
//! sibling-module path inside a `cfg(windows)` region to be reachable from
//! that file: `crate::`/`super::`/`self::`-qualified, or brought in by a
//! `use`. That is a WEAKER claim than compiling the arm, and it is the
//! strongest one available on Linux -- it catches the unresolvable-path
//! class and nothing else, which is the class that actually bit.

#[cfg(test)]
mod tests {
    use super::super::structural_scan::shell_sources;
    use std::collections::HashSet;

    /// The shell's own module names, read from `lib.rs`'s `mod x;` lines.
    fn shell_modules(lib: &str) -> HashSet<String> {
        lib.lines()
            .filter_map(|l| l.trim().strip_prefix("mod "))
            .filter_map(|l| l.strip_suffix(';'))
            .map(str::to_string)
            .collect()
    }

    /// Every name this file can already reach unqualified via a `use`.
    fn imported(src: &str) -> HashSet<String> {
        let mut names = HashSet::new();
        for line in src.lines() {
            let Some(rest) = line.trim().strip_prefix("use ") else {
                continue;
            };
            // `use crate::{a, b};` / `use crate::a;` / `use super::a::…`
            for token in rest
                .trim_end_matches(';')
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            {
                if !token.is_empty() {
                    names.insert(token.to_string());
                }
            }
        }
        names
    }

    /// The identifier in a leading `ident::` path on this line, if any.
    fn bare_paths(line: &str) -> Vec<String> {
        let bytes: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if !(bytes[i].is_alphabetic() || bytes[i] == '_') {
                i += 1;
                continue;
            }
            let start = i;
            while i < bytes.len() && (bytes[i].is_alphanumeric() || bytes[i] == '_') {
                i += 1;
            }
            // Only a path HEAD counts: `a::b` yes, the `b` of `a::b` no.
            let preceded_by_sep = start >= 2 && bytes[start - 1] == ':' && bytes[start - 2] == ':';
            if !preceded_by_sep && i + 1 < bytes.len() && bytes[i] == ':' && bytes[i + 1] == ':' {
                out.push(bytes[start..i].iter().collect());
            }
        }
        out
    }

    // REGRESSION: `window_upkeep.rs`, split out of `lib.rs` at the 800-line
    // cap, carried `commands::primary_button_down()` into a sibling module
    // where that path does not resolve. Every Linux gate passed; the Windows
    // build failed with E0433. Reverting the `crate::` qualifier reddens
    // this, naming the file, the line and the path.
    #[test]
    fn a_cfg_windows_body_never_names_a_sibling_module_it_cannot_reach() {
        let lib = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"))
            .expect("lib.rs");
        let modules = shell_modules(&lib);
        assert!(
            modules.contains("commands") && modules.len() > 20,
            "the module list looks wrong: {} found",
            modules.len()
        );

        let mut offenders = Vec::new();
        for (path, src) in shell_sources() {
            // lib.rs is the crate root: a bare `commands::` is correct there.
            if path.file_name().is_some_and(|n| n == "lib.rs") {
                continue;
            }
            let reachable = imported(&src);
            let lines: Vec<&str> = src.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if !line.contains("cfg(windows)") {
                    continue;
                }
                // The gated item: an attribute applies to what follows, and
                // 30 lines covers the block/fn forms this crate uses.
                for (n, gated) in lines.iter().enumerate().skip(i + 1).take(30) {
                    for name in bare_paths(gated) {
                        if modules.contains(&name) && !reachable.contains(&name) {
                            offenders.push(format!(
                                "{}:{} — `{name}::` is a sibling module and is neither \
                                 crate-qualified nor imported here: {}",
                                path.display(),
                                n + 1,
                                gated.trim()
                            ));
                        }
                    }
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a cfg(windows) body names a module it cannot reach — Linux compiles \
             this block out, so only the Windows job would catch it:\n{}",
            offenders.join("\n")
        );
    }

    // The scan is only worth its FILE SET and its path parser, so both are
    // pinned: a parser that found nothing would pass the test above forever.
    #[test]
    fn the_path_parser_finds_a_head_and_ignores_a_tail() {
        assert_eq!(
            bare_paths("if commands::primary_button_down() {"),
            ["commands"]
        );
        assert_eq!(bare_paths("if crate::commands::x() {"), ["crate"]);
        assert!(bare_paths("let x = 1;").is_empty());
        // `use` brings it into scope, so a bare head there is legitimate.
        assert!(imported("use crate::{commands, tray};").contains("commands"));
        assert!(imported("use crate::tray;").contains("tray"));
        assert!(!imported("use crate::tray;").contains("commands"));
    }
}
