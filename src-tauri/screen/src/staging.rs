//! Staging: where a capture lives between "stopped" and "saved into a
//! vault".
//!
//! Staging is deliberately OUTSIDE every vault (spec 10). An unedited,
//! unapproved capture is not knowledge; putting it in a vault would make
//! discard leave litter in the user's notes and would mean the app writes
//! to a vault the user never asked it to touch.
//!
//! Everything here is pure path and string logic, so it compiles and is
//! tested on Linux — which is the point: no CI runner can record a screen,
//! so every rule that CAN be checked without a screen is checked here.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The staging directory's name under the app's local data dir.
pub const STAGING_DIR_NAME: &str = "screen-captures";

/// Longest title we will put in a file name. MAX_PATH is 260 by default on
/// Windows; the staging path, the `YYYY-MM-DD HHmm ` prefix, a possible
/// ` (12)` collision suffix and `.mp4.part` all have to fit alongside it.
pub const MAX_TITLE_CHARS: usize = 64;

/// Fallback when a title sanitizes to nothing. Never an empty string and
/// never ".": both are directory references, and `dir.join(".")` is the
/// staging directory itself, not a file in it.
const FALLBACK_TITLE: &str = "Screen Capture";

const PART_SUFFIX: &str = ".mp4.part";

pub fn staging_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join(STAGING_DIR_NAME)
}

/// Turn a window or monitor title into something safe to put in a file
/// name.
///
/// This is a security boundary, not cosmetics: a window title is whatever
/// the recorded application chose to put in its title bar, and an
/// unsanitized separator in it escapes the staging directory.
pub fn sanitize_title(raw: &str) -> String {
    let mut mapped = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            // Windows-reserved, plus the separators.
            ':' | '\\' | '/' | '?' | '*' | '"' | '<' | '>' | '|' => mapped.push('-'),
            // Control characters are dropped outright rather than replaced:
            // replacing them would turn an invisible character into a
            // visible dash the user never typed.
            c if c.is_control() => {}
            c => mapped.push(c),
        }
    }

    // Collapse each run of separator characters (space or '-') into ONE
    // normalized separator. A run mixing whitespace AND a dash becomes
    // " - " (so "A ::  B" reads as "A - B", not "A --  B"); a run of dashes
    // with NO whitespace stays a single "-" (so a reserved character
    // between two letters, e.g. "a:b", becomes "a-b" with no spaces
    // invented around it); a run of pure whitespace collapses to one space.
    let mut collapsed = String::with_capacity(mapped.len());
    let mut run_has_dash = false;
    let mut run_has_space = false;
    let mut in_run = false;
    for ch in mapped.chars() {
        if ch == '-' || ch.is_whitespace() {
            in_run = true;
            run_has_dash |= ch == '-';
            run_has_space |= ch.is_whitespace();
        } else {
            if in_run {
                push_separator(&mut collapsed, run_has_dash, run_has_space);
                run_has_dash = false;
                run_has_space = false;
                in_run = false;
            }
            collapsed.push(ch);
        }
    }
    if in_run {
        push_separator(&mut collapsed, run_has_dash, run_has_space);
    }

    // Windows silently strips a trailing dot or space from a file name, so a
    // name ending in either stops matching the name we reserved.
    let trimmed: String = collapsed
        .trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '-')
        .chars()
        .take(MAX_TITLE_CHARS)
        .collect();
    let trimmed = trimmed
        .trim_matches(|c: char| c.is_whitespace() || c == '.' || c == '-')
        .to_string();

    if trimmed.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        trimmed
    }
}

fn push_separator(out: &mut String, has_dash: bool, has_space: bool) {
    match (has_dash, has_space) {
        (true, true) => out.push_str(" - "),
        (true, false) => out.push('-'),
        (false, _) => out.push(' '),
    }
}

/// The hidden in-progress file, mirroring the audio domain's `.mp3.part`.
pub fn part_file_name(base: &str) -> String {
    format!(".{base}{PART_SUFFIX}")
}

/// Recover the base from a part file name, or `None` when the name is not
/// one of ours. Phase 5's recovery deletes what this recognizes, so it is
/// deliberately strict — a loose match would let recovery delete a user
/// file that happens to sit in the staging directory.
pub fn base_from_part(file_name: &str) -> Option<String> {
    let rest = file_name.strip_prefix('.')?;
    let base = rest.strip_suffix(PART_SUFFIX)?;
    if base.is_empty() {
        return None;
    }
    Some(base.to_string())
}

pub fn mp4_file_name(base: &str) -> String {
    format!("{base}.mp4")
}

pub fn sidecar_file_name(base: &str) -> String {
    format!("{base}.json")
}

/// Find a free base in `dir`, suffixing ` (N)` on collision.
///
/// A base is free only when ALL THREE names it owns are free — the staged
/// `.mp4`, the `.json` sidecar and the hidden `.mp4.part`. This is the
/// pairwise reservation the audio domain uses, widened to three: checking
/// only the `.mp4` would let a second capture adopt a base whose
/// in-progress `.part` still exists, and the two captures would then write
/// the same file.
pub fn reserve_base(dir: &Path, base: &str) -> String {
    let free = |candidate: &str| {
        !dir.join(mp4_file_name(candidate)).exists()
            && !dir.join(sidecar_file_name(candidate)).exists()
            && !dir.join(part_file_name(candidate)).exists()
    };
    if free(base) {
        return base.to_string();
    }
    for n in 2..10_000 {
        let candidate = format!("{base} ({n})");
        if free(&candidate) {
            return candidate;
        }
    }
    // Astronomically unreachable; a timestamped fallback beats a panic in a
    // capture-start path.
    format!("{base} ({})", std::process::id())
}

/// What the editor needs to resume a staged capture (spec 10).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedSidecar {
    pub base: String,
    pub vault_id: String,
    pub source_title: String,
    /// `"screen"` or `"window"` — a string, not an enum, so a sidecar
    /// written by a future version that adds a source kind still
    /// deserializes here instead of failing the whole file.
    pub source_kind: String,
    pub inputs: Vec<String>,
    pub duration_ms: u64,
    pub paused_ms: u64,
    pub width: u32,
    pub height: u32,
    pub recorded_at: String,
    /// The in-progress edit, saved on each editor operation (phase 4).
    /// `None` until the editor touches it.
    #[serde(default)]
    pub timeline: Option<serde_json::Value>,
}

pub fn write_sidecar(dir: &Path, sidecar: &StagedSidecar) -> std::io::Result<PathBuf> {
    let path = dir.join(sidecar_file_name(&sidecar.base));
    let json = serde_json::to_vec_pretty(sidecar)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Read a sidecar, degrading to `None` on anything unreadable.
///
/// Same defensive-read posture as the rest of the vault domain: a
/// hand-edited or truncated sidecar must make ONE capture un-resumable,
/// never fail the whole staging scan.
pub fn read_sidecar(path: &Path) -> Option<StagedSidecar> {
    let bytes = std::fs::read(path)
        .map_err(|e| {
            log::warn!(
                "screen staging: cannot read sidecar {}: {e}",
                path.display()
            )
        })
        .ok()?;
    serde_json::from_slice(&bytes)
        .map_err(|e| log::warn!("screen staging: malformed sidecar {}: {e}", path.display()))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn staging_lives_beside_the_logs_not_inside_any_vault() {
        // Spec 10: an unedited, unapproved capture is not knowledge. If this
        // ever resolved under a vault path, discard would leave litter in the
        // user's notes and the app would be writing to a vault nobody asked
        // it to touch.
        let dir = staging_dir(&PathBuf::from("/lad/com.vaultbuddy.desktop"));
        assert_eq!(
            dir,
            PathBuf::from("/lad/com.vaultbuddy.desktop/screen-captures")
        );
    }

    #[test]
    fn sanitize_strips_every_windows_reserved_filename_character() {
        // A window title is attacker-adjacent input: it is whatever the app
        // being recorded chose to put in its title bar. Unsanitized, a title
        // containing a separator escapes the staging directory outright.
        assert_eq!(
            sanitize_title(r#"a:b\c/d?e*f"g<h>i|j"#),
            "a-b-c-d-e-f-g-h-i-j"
        );
    }

    #[test]
    fn sanitize_drops_control_characters_rather_than_replacing_them() {
        assert_eq!(sanitize_title("Ti\u{0}tle\u{1f}"), "Title");
    }

    #[test]
    fn sanitize_collapses_runs_and_trims_the_edges() {
        // Without collapsing, "A :: B" becomes "A -- B"; without trimming,
        // Windows silently strips a trailing dot or space from a filename,
        // so the name on disk stops matching the name we reserved.
        assert_eq!(sanitize_title("  A ::  B . "), "A - B");
    }

    #[test]
    fn sanitize_never_returns_an_empty_or_dot_only_name() {
        // A title of "..." or "" must not produce "." or "" — both are
        // directory references, not file names, and `dir.join(".")` is the
        // staging directory itself.
        assert_eq!(sanitize_title(""), "Screen Capture");
        assert_eq!(sanitize_title("..."), "Screen Capture");
        assert_eq!(sanitize_title("   "), "Screen Capture");
    }

    #[test]
    fn sanitize_bounds_the_length_so_the_full_path_stays_writable() {
        // MAX_PATH is 260 by default; a 300-character window title plus the
        // staging path plus " (12).mp4.part" overruns it and the create
        // fails with a bewildering OS error at capture start.
        let long = "x".repeat(300);
        assert_eq!(sanitize_title(&long).chars().count(), MAX_TITLE_CHARS);
    }

    #[test]
    fn the_part_file_is_hidden_and_round_trips_back_to_its_base() {
        // Mirrors the audio domain's .mp3.part convention: dot-prefixed so
        // Obsidian and Explorer ignore an in-progress capture.
        let base = "2026-09-18 1432 Figma walkthrough";
        let part = part_file_name(base);
        assert_eq!(part, ".2026-09-18 1432 Figma walkthrough.mp4.part");
        assert_eq!(base_from_part(&part).as_deref(), Some(base));
    }

    #[test]
    fn base_from_part_rejects_anything_that_is_not_our_own_part_file() {
        // Recovery (phase 5) deletes what this recognizes. A loose match
        // would let it delete a user file that happens to live in the
        // staging directory.
        assert_eq!(base_from_part("notes.mp4"), None);
        assert_eq!(base_from_part(".notes.mp3.part"), None);
        assert_eq!(
            base_from_part("notes.mp4.part"),
            None,
            "must be dot-prefixed"
        );
        assert_eq!(base_from_part(".mp4.part"), None, "empty base");
    }

    #[test]
    fn the_staged_and_sidecar_names_derive_from_the_same_base() {
        let base = "2026-09-18 1432 Figma walkthrough";
        assert_eq!(mp4_file_name(base), "2026-09-18 1432 Figma walkthrough.mp4");
        assert_eq!(
            sidecar_file_name(base),
            "2026-09-18 1432 Figma walkthrough.json"
        );
    }

    #[test]
    fn reserve_base_returns_the_plain_base_when_nothing_collides() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap");
    }

    #[test]
    fn reserve_base_suffixes_past_any_of_the_three_names_it_owns() {
        // The pairwise reservation the audio domain uses: a base is free only
        // when the .mp4, the .json AND the .mp4.part are all free. Checking
        // only the .mp4 would let a second capture reuse a base whose
        // in-progress .part still exists, and the two would fight over one
        // file.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("cap.mp4"), b"x").unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap (2)");

        std::fs::write(dir.path().join("cap (2).json"), b"x").unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap (3)");

        std::fs::write(dir.path().join(part_file_name("cap (3)")), b"x").unwrap();
        assert_eq!(reserve_base(dir.path(), "cap"), "cap (4)");
    }

    #[test]
    fn a_sidecar_round_trips_through_json() {
        let s = StagedSidecar {
            base: "2026-09-18 1432 Demo".into(),
            vault_id: "abc123".into(),
            source_title: "Figma \u{2014} Design System".into(),
            source_kind: "window".into(),
            inputs: vec!["Microphone (Yeti)".into()],
            duration_ms: 197_000,
            paused_ms: 12_000,
            width: 1920,
            height: 1080,
            recorded_at: "2026-09-18T14:32:00+02:00".into(),
            timeline: None,
        };
        let dir = tempfile::tempdir().unwrap();
        let path = write_sidecar(dir.path(), &s).unwrap();
        let back = read_sidecar(&path).expect("sidecar reads back");
        assert_eq!(back.base, s.base);
        assert_eq!(back.duration_ms, 197_000);
        assert_eq!(back.source_title, "Figma \u{2014} Design System");
    }

    #[test]
    fn read_sidecar_degrades_to_none_rather_than_erroring() {
        // Same defensive-read posture as the rest of the vault domain: a
        // hand-edited or truncated sidecar must make ONE capture
        // un-resumable, never fail the whole staging scan.
        let dir = tempfile::tempdir().unwrap();
        let bad = dir.path().join("bad.json");
        std::fs::write(&bad, b"{ not json").unwrap();
        assert!(read_sidecar(&bad).is_none());
        assert!(read_sidecar(&dir.path().join("missing.json")).is_none());
    }

    #[test]
    fn the_sidecar_serializes_camel_case_for_the_webview() {
        // Phase 4's editor reads this through the IPC boundary, where every
        // other DTO in this app is camelCase. A snake_case key here would
        // deserialize to undefined in the editor with no error.
        let s = StagedSidecar {
            base: "b".into(),
            vault_id: "v".into(),
            source_title: "t".into(),
            source_kind: "screen".into(),
            inputs: vec![],
            duration_ms: 1,
            paused_ms: 0,
            width: 2,
            height: 2,
            recorded_at: "r".into(),
            timeline: None,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"vaultId\""), "got {json}");
        assert!(json.contains("\"durationMs\""), "got {json}");
        assert!(!json.contains("vault_id"), "got {json}");
    }
}
