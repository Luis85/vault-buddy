//! Pandoc toolchain: resolving a working `pandoc` executable (config override
//! → registry-augmented PATH → bare fallback) and building its sandboxed
//! argument vector. Split out of `document_commands.rs` (which keeps the IPC
//! surface — detection status, conversion command, settings, import recovery)
//! purely to stay under the per-file LOC cap; no behavior changes.
//!
//! Everything here that was NOT about Pandoc — the registry-fresh PATH, the
//! candidate ordering, the headless `Command` builder and the bounded runner —
//! now lives in `external_tool.rs`, shared with the ffmpeg integration.

use std::time::Duration;
use vault_buddy_core::capture_config;

use crate::external_tool::{candidates_for, run_capturing, tool_command, Capture, PROBE_TIMEOUT};

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

/// Ordered pandoc candidates (see `external_tool::candidate_order`), fed the
/// real config override and the concrete PATH executables.
fn pandoc_candidates() -> Vec<String> {
    candidates_for(
        "pandoc",
        capture_config::load_config()
            .document_import
            .pandoc_path
            .as_deref(),
    )
}

/// Probe one candidate: run `<program> --version` (bounded — see `PROBE_TIMEOUT`).
/// On success, return the program string, its parsed (major, minor), AND the
/// first `--version` line for display — all from the SAME spawn. None if it
/// can't run, times out, or exits non-zero (so the caller falls through to the
/// next candidate).
fn probe_pandoc(program: &str) -> Option<(String, u32, u32, String)> {
    let mut cmd = tool_command(program);
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

    #[test]
    fn sandbox_requires_2_15_or_newer() {
        assert!(!sandbox_supported(2, 14));
        assert!(sandbox_supported(2, 15));
        assert!(sandbox_supported(3, 1));
        assert!(sandbox_supported(2, 20));
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
}
