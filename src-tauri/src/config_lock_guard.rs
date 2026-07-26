//! Structural pin for the config-lock collapse (`fix(shell): serialize every
//! config write behind one lock`): the shell crate must never grow a SECOND
//! `Mutex<()>`-backed config-write guard. Before that fix, a shell-only
//! `struct ConfigWriteLock(pub Mutex<()>)` (managed as Tauri state, taken by
//! the IPC settings commands) and core's `capture_config::config_write_lock()`
//! both serialized `config.json` read-modify-writes without excluding each
//! other — see `core/src/services/tasks/parent.rs`'s module doc for the
//! desync that caused. The fix retired the shell lock so every config-write
//! site takes the one core lock; the guarantee it shipped — "there is no
//! second lock left to pick" — landed only as prose and a doc comment. This
//! file is what turns that guarantee into an artifact: it reads every shell
//! source file and fails if any of them declares a config-flavored
//! `Mutex<()>` gate again.

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// Recursively collect every `.rs` file under `dir`, skipping this test's
    /// OWN file — which necessarily talks ABOUT the pattern being searched
    /// for (`Mutex<()>`, `ConfigWriteLock`) in its own doc comments above and
    /// would otherwise flag itself.
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

    fn is_ident_char(c: char) -> bool {
        c.is_ascii_alphanumeric() || c == '_'
    }

    /// Collapse every run of whitespace (spaces, tabs, newlines) to a SINGLE
    /// space, rather than deleting it outright. This still lets a
    /// rustfmt-wrapped, multi-line declaration read as one contiguous
    /// string, but — unlike deleting whitespace — it preserves the mandatory
    /// separator between two adjacent keywords/identifiers. An earlier
    /// version of this scan deleted whitespace entirely, which fused
    /// `pub struct Foo` into `pubstructFoo` and made the real `struct`
    /// keyword indistinguishable from being embedded inside a longer
    /// identifier — the boundary check below rejected every legitimate
    /// `pub struct ...` declaration as a false non-match (caught by the
    /// `predicate_requires_both_the_name_and_the_shape` test's verbatim
    /// `ConfigWriteLock` fixture going unexpectedly green before this fix).
    fn normalize_whitespace(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut last_was_space = false;
        for c in text.chars() {
            if c.is_whitespace() {
                if !last_was_space {
                    out.push(' ');
                }
                last_was_space = true;
            } else {
                out.push(c);
                last_was_space = false;
            }
        }
        out
    }

    /// Every violation `text` contains: a `struct <Name>(..)` / `static
    /// <NAME>: ..` declaration whose body (from the name to the next `;`,
    /// capped) contains the literal `Mutex<()>` AND whose declared name
    /// contains "config" (case-insensitive) — plus, independently, the exact
    /// retired identifier `ConfigWriteLock` anywhere in the file, in case a
    /// future reintroduction keeps that name but changes shape entirely.
    ///
    /// WHAT THIS CATCHES: a struct field or `static` item typed `Mutex<()>`
    /// — directly, `std::sync::`-qualified, or `Arc<..Mutex<()>>`-wrapped
    /// (Rust requires the full type to be written out at both a struct
    /// field and a `static` item, so qualifying/wrapping can't hide the
    /// substring) — whose declared name mentions "config". That is the
    /// shape both the old `ConfigWriteLock` (a struct) and a plausible
    /// reintroduction would take: a fresh `struct ConfigGuard(Mutex<()>)`
    /// registered as Tauri state (the old pattern), or a shell-local
    /// `static CONFIG_LOCK: Mutex<()> = Mutex::new(())` mirroring core's
    /// OWN `config_write_lock()` internals one crate up.
    ///
    /// WHAT IT DOES NOT CATCH: a `Mutex<()>` gate for an unrelated purpose —
    /// `document_commands::ImportLock` serializes Pandoc conversions, not
    /// config.json, and carries no "config" in its name, so it must NOT trip
    /// this (pinned below alongside the capture-state mutex, which isn't
    /// even `Mutex<()>`); a named-field struct (`struct X { .. }`, no `;`
    /// terminator — every `Mutex<()>` gate in this codebase has always been
    /// a tuple struct, so that shape has no precedent here); or a determined
    /// rename that avoids "config" in its identifier entirely. Comments and
    /// string literals are not excluded from the scan, so a prose sentence
    /// shaped exactly like `struct <SomeConfigWord> ... Mutex<()> ...`
    /// within 400 characters could in principle manufacture a hit — but that
    /// requires "config" to be the very next word after a bare `struct`/
    /// `static` keyword, which ordinary prose about the topic does not do
    /// (English puts articles/adjectives there, not a Rust identifier).
    /// This is a text scan, not a parser or a borrow-checker — a tripwire
    /// for the likely accidental reintroduction, not a proof of absence.
    fn config_mutex_violations(text: &str) -> Vec<String> {
        let normalized = normalize_whitespace(text);
        let bytes = normalized.as_bytes();
        let mut hits = Vec::new();

        for kw in ["struct", "static"] {
            let mut from = 0;
            while let Some(rel) = normalized[from..].find(kw) {
                let kw_start = from + rel;
                let kw_end = kw_start + kw.len();
                from = kw_end; // always advance, whether or not this hit counts

                // A real keyword boundary on both sides. Reading a single
                // byte and comparing it as ASCII is safe even mid multi-byte
                // UTF-8 (common in this codebase's em-dash-heavy comments):
                // every continuation/leading byte is >= 0x80, which
                // `is_ascii_alphanumeric` always rejects, so this can only
                // ever be too PERMISSIVE there — correctly so, since a
                // multi-byte character is never part of an ASCII identifier
                // either. The right side must be exactly one normalized
                // space: `struct` is always followed by whitespace then the
                // name in valid Rust.
                let left_ok = kw_start == 0 || !is_ident_char(bytes[kw_start - 1] as char);
                let right_ok = kw_end < bytes.len() && bytes[kw_end] == b' ';
                if !left_ok || !right_ok {
                    continue;
                }

                // Exactly one normalized space separates the keyword from
                // the name (runs were already collapsed above).
                let name_start = kw_end + 1;
                let name: String = normalized[name_start..]
                    .chars()
                    .take_while(|c| is_ident_char(*c))
                    .collect();
                if name.is_empty() {
                    continue;
                }

                // The declaration body: name to the next `;`, capped at ~400
                // chars so one missing `;` (a named-field struct — see "what
                // this does not catch" above) can't drag the scan across the
                // rest of the file. Bounded by CHAR count via `.chars()`,
                // never a raw byte offset, so this can't panic slicing into
                // the middle of a multi-byte character. Re-stripped of its
                // (now single) spaces before the substring check, so a
                // pathologically spaced `Mutex < ( ) >` still matches.
                let after_name = name_start + name.len();
                let rest = &normalized[after_name..];
                let body: String = match rest.find(';') {
                    Some(i) if i <= 400 => rest[..i].to_string(),
                    _ => rest.chars().take(400).collect(),
                };
                let body_tight: String = body.chars().filter(|c| !c.is_whitespace()).collect();
                if body_tight.contains("Mutex<()>") && name.to_ascii_lowercase().contains("config")
                {
                    hits.push(format!("{kw} {name}"));
                }
            }
        }

        // Defense in depth: the exact deleted identifier, in ANY shape at
        // all — not just the struct/static forms scanned above — matched as
        // a whole word (not a substring of a longer identifier). Identifiers
        // can't contain whitespace, so scanning the space-normalized text is
        // equivalent to scanning the original here.
        let needle = "ConfigWriteLock";
        let mut idx = 0;
        while let Some(rel) = normalized[idx..].find(needle) {
            let start = idx + rel;
            let end = start + needle.len();
            let before_ok = start == 0 || !is_ident_char(bytes[start - 1] as char);
            let after_ok = end == bytes.len() || !is_ident_char(bytes[end] as char);
            if before_ok && after_ok {
                hits.push(format!("literal identifier `{needle}`"));
            }
            idx = end;
        }

        hits
    }

    #[test]
    fn shell_declares_no_second_config_write_mutex() {
        let shell_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let self_name = Path::new(file!())
            .file_name()
            .expect("file!() always has a file name")
            .to_owned();
        let mut files = Vec::new();
        rust_files(&shell_src, &self_name, &mut files);
        // A self-check on the walk, not the invariant: if this ever comes
        // back empty (or suspiciously small) the test would vacuously
        // "pass" having checked nothing.
        assert!(
            files.len() > 5,
            "scan under {shell_src:?} found only {} shell source file(s) — the walk is \
             broken, not the invariant it's meant to check",
            files.len()
        );

        let mut violations = Vec::new();
        for path in &files {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("could not read {path:?}: {e}"));
            for hit in config_mutex_violations(&text) {
                violations.push(format!("{}: {hit}", path.display()));
            }
        }

        assert!(
            violations.is_empty(),
            "found a second Mutex<()>-backed config guard in the shell crate: {violations:?}\n\n\
             config.json read-modify-writes must serialize through exactly ONE lock, \
             vault_buddy_core::capture_config::config_write_lock() — see \
             core/src/services/tasks/parent.rs's module doc for the desync a second lock \
             caused. Take that lock instead of adding a new one."
        );
    }

    /// Pins the predicate itself against the two ways it could go wrong: a
    /// config-NAMED type that doesn't wrap `Mutex<()>` must not trip it
    /// (the shape is required), and a `Mutex<()>`-wrapping type with no
    /// "config" in its name — the REAL `ImportLock` and `CaptureState`
    /// shapes — must not trip it either (the name is required). Only the
    /// conjunction, or the literal retired name, is a violation.
    #[test]
    fn predicate_requires_both_the_name_and_the_shape() {
        // Config-named, but the wrong shape (not Mutex<()>) — a hypothetical
        // config-related flag, not a lock.
        assert!(
            config_mutex_violations("pub struct ConfigDirty(pub std::sync::Mutex<bool>);")
                .is_empty()
        );
        // Mutex<()>-shaped, but no "config" in the name — document_commands's
        // REAL ImportLock declaration, verbatim. Must never trip this.
        assert!(config_mutex_violations(
            "pub struct ImportLock(pub std::sync::Arc<std::sync::Mutex<()>>);"
        )
        .is_empty());
        // Not Mutex<()> at all — capture_commands's REAL CaptureState
        // declaration, verbatim (the "capture state mutex" the task calls
        // out by name). Must never trip this.
        assert!(config_mutex_violations(
            "pub struct CaptureState(pub std::sync::Mutex<Option<ActiveCapture>>, pub std::sync::Condvar);"
        )
        .is_empty());
        // The actual retired declaration (capture_commands.rs pre-fix,
        // verbatim, including the "pub struct" two-keyword prefix that once
        // tripped this predicate up — see normalize_whitespace's doc
        // comment): both the shape match ("struct ConfigWriteLock") AND the
        // literal-name defense-in-depth check fire — two independent hits.
        assert_eq!(
            config_mutex_violations(
                "#[derive(Default)]\npub struct ConfigWriteLock(pub Mutex<()>);"
            )
            .len(),
            2
        );
        // A multi-line, rustfmt-wrapped variant of the same declaration —
        // pins that the scan isn't fooled by reformatting alone.
        assert_eq!(
            config_mutex_violations(
                "pub struct ConfigWriteLock(\n    pub std::sync::Mutex<()>,\n);"
            )
            .len(),
            2
        );
        // A plausible reintroduction that isn't a struct at all — mirroring
        // core's OWN config_write_lock() internals, but placed in the shell.
        assert_eq!(
            config_mutex_violations(
                "static SETTINGS_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());"
            )
            .len(),
            1
        );
        // A prose mention of "struct"/"config" near an UNRELATED (non-Mutex)
        // declaration must not, by itself, manufacture a hit.
        assert!(config_mutex_violations(
            "// the struct below replaces the old config lock\npub struct Foo(pub bool);"
        )
        .is_empty());
    }
}
