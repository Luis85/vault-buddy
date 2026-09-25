//! Content-free log values (Task 58; F-50; PRODUCT-SPEC NFR "logs carry no
//! content"). The editor's log lines may say THAT a file, a capture or a
//! caption was involved, and may let two lines about the same one be
//! matched up, but may never say WHICH one: a path names the user and their
//! folders, a staged capture's base carries the recorded window's title,
//! and a caption or cue text is the tutorial itself.
//!
//! So every `log::` call under `src/editor/` formats such a value through
//! one of these two functions, never `{}`/`{:?}`/`.display()` directly —
//! `redact_guard.rs` (test-only) scans the whole directory and names every
//! line that does not.
//!
//! The value becomes `<path:#1a2b3c4d>` / `<name:#1a2b3c4d>`: the first 8
//! hex digits of the SHA-256 of its text. Stable across runs and builds
//! (not `DefaultHasher`, whose output Rust does not promise), so a support
//! reader can see that two lines, or two logs, are about the same file
//! without learning what it is. 32 bits is enough to tell a handful of
//! files in one log apart and too few to confirm a guess about a long path
//! by brute force in any useful way — it is a correlation handle, not a
//! secret.

use std::io::Read;
use std::path::Path;

use vault_buddy_core::editor::import_io::hashing_reader;

/// `<path:#hash8>` for a filesystem path.
pub fn redact_path(path: &Path) -> String {
    format!("<path:#{}>", hash8(path.to_string_lossy().as_bytes()))
}

/// `<name:#hash8>` for a file name, a capture base, a title or any other
/// user text.
pub fn redact_name(name: &str) -> String {
    format!("<name:#{}>", hash8(name.as_bytes()))
}

/// `text` with every occurrence of each of `paths` replaced by its
/// `redact_path` handle (final review M8) — for a tool's output, ffmpeg's
/// stderr above all, which names the files it was handed. Longest first, so
/// a folder never cuts a file path inside it in half; the forward-slash
/// spelling too, which ffmpeg sometimes echoes. A path the text names that
/// is not in `paths` is NOT caught: pass every path the tool was given.
pub fn redact_paths_in(text: &str, paths: &[&Path]) -> String {
    let mut known: Vec<(String, String)> = Vec::new();
    for path in paths {
        let plain = path.to_string_lossy().into_owned();
        if plain.is_empty() {
            continue;
        }
        let handle = redact_path(path);
        known.push((plain.replace('\\', "/"), handle.clone()));
        known.push((plain, handle));
    }
    known.sort_by_key(|(plain, _)| std::cmp::Reverse(plain.len()));
    let mut out = text.to_string();
    for (plain, handle) in known {
        out = out.replace(&plain, &handle);
    }
    out
}

fn hash8(bytes: &[u8]) -> String {
    let (mut reader, digest) = hashing_reader(bytes);
    let mut sink = Vec::new();
    // Reading from a byte slice cannot fail.
    let _ = reader.read_to_end(&mut sink);
    let (_, hex) = digest.finish();
    hex[..8].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_becomes_a_stable_hash_and_nothing_else() {
        let path = Path::new(r"C:\Users\x\Secret plan\a.mp4");
        let shown = redact_path(path);
        // The SHA-256 of the path's own text, first 8 hex digits: pinned
        // so a change of hash (or of the text hashed) is a decision.
        assert_eq!(shown, "<path:#bbe240b5>");
        assert_eq!(redact_path(path), shown, "the same path, the same handle");
        assert_ne!(redact_path(Path::new(r"C:\Users\x\b.mp4")), shown);
        for leak in ["Users", "Secret", "a.mp4", "C:"] {
            assert!(!shown.contains(leak), "{shown} leaks {leak}");
        }
    }

    // Final review M8: ffmpeg's stderr, logged on a failed render or take,
    // names every file it was handed.
    #[test]
    fn known_paths_in_tool_output_become_handles() {
        let folder = Path::new(r"C:\Users\x\Secret plan\jobs\job-1");
        let part = Path::new(r"C:\Users\x\Secret plan\jobs\job-1\out.mp4.part");
        let stderr = [
            part.to_string_lossy().as_ref(),
            ": Invalid data; also C:/Users/x/Secret plan/jobs/job-1/out.mp4.part and ",
            folder.to_string_lossy().as_ref(),
        ]
        .concat();
        let shown = redact_paths_in(&stderr, &[folder, part]);
        assert_eq!(
            shown,
            format!(
                "{}: Invalid data; also {} and {}",
                redact_path(part),
                redact_path(part),
                redact_path(folder)
            )
        );
        assert!(!shown.contains("Secret"), "{shown}");
    }

    #[test]
    fn a_name_becomes_a_stable_hash_and_nothing_else() {
        let shown = redact_name("2026-09-21 1430 Secret plan");
        assert!(shown.starts_with("<name:#") && shown.ends_with('>'));
        assert_eq!(shown.len(), "<name:#".len() + 8 + 1);
        assert!(!shown.contains("Secret"));
        assert_ne!(redact_name("a"), redact_name("b"));
    }
}
