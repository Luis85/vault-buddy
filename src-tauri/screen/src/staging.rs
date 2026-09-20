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
    let trimmed = disambiguate_export_marker(trimmed);

    if trimmed.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        trimmed
    }
}

/// Keep a sanitized title from ENDING in exactly `EXPORT_PART_INFIX`.
///
/// A capture's base is `capture_paths::base_name(..., sanitize_title(title))`,
/// so the title's tail is the base's tail — and a base ending in `.export`
/// mints a `.<base>.mp4.part` byte-identical to the export temp
/// `export_part_file_name` mints for the base WITHOUT it.
/// `screen_recovery::classify` must check the export shape first (or every
/// abandoned transcode would be promoted as footage), so it read that capture
/// part as an ExportTemp and DELETED it, skipping `part_holds_footage`
/// entirely: a crash while recording a window titled "Build.export" destroyed
/// exactly what the sweep exists to rescue. The two shapes were only
/// "indistinguishable by name" because this function permitted the ending.
///
/// **This prevents NEW collisions only** — a capture already staged under such
/// a base keeps it, and its orphaned `.part` is still swept as an export temp.
///
/// Runs AFTER the truncation and both trims, because either can CREATE the
/// ending: `.take(MAX_TITLE_CHARS)` can cut a longer title off right at the
/// marker, and the trailing-separator trim turns `"Build.export..."` into it.
fn disambiguate_export_marker(title: String) -> String {
    if !title.ends_with(EXPORT_PART_INFIX) {
        return title;
    }
    let mut out = title;
    if out.chars().count() >= MAX_TITLE_CHARS {
        // Room for the `_` comes out of the title's own tail, so this can
        // never breach the length budget. The character dropped is the
        // marker's last, leaving a letter — never a dot or space Windows
        // would strip.
        out = out.chars().take(MAX_TITLE_CHARS - 1).collect();
    }
    out.push('_');
    out
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

/// The infix an EXPORT temp carries between its base and `.mp4.part`.
///
/// Shared, not spelled twice: `export_part_file_name` below mints the name
/// the export worker writes, and `screen_recovery::classify` recognises an
/// abandoned one by stripping exactly this. Two independent literals is how
/// a temp stops being swept and becomes permanent litter in the user's
/// staging directory, with nothing red anywhere.
pub const EXPORT_PART_INFIX: &str = ".export";

/// The hidden in-progress file an export writes into, `.<base>.export.mp4.part`.
///
/// Deliberately the capture `.part` shape with an infix rather than a new
/// suffix: `screen_recovery` classifies every name in the staging directory
/// through `base_from_part`, and a shape that does not round-trip through it
/// is a shape the sweep never sees.
pub fn export_part_file_name(base: &str) -> String {
    part_file_name(&format!("{base}{EXPORT_PART_INFIX}"))
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

/// Is `name`'s STEM (the text before the first `.`) one of Windows' reserved
/// device names, case-insensitively?
///
/// Windows resolves a path component whose stem matches one of these to the
/// PHYSICAL device it names, regardless of the surrounding directory or any
/// extension — `dir.join("COM1.json")` opens the COM1 serial port, not a
/// file called that (GAP-108). Public so every caller that turns untrusted
/// text into a path component can share one list instead of drifting apart:
/// `editor_commands::is_safe_base` (phase 4) is the first. `sanitize_title`
/// does not consume this yet — closing GAP-108 there means deciding how a
/// title that collapses onto a reserved name should be renamed rather than
/// merely refused, which belongs with the capture-session write-site
/// hardening GAP-108 is filed against, not here.
pub fn is_reserved_device_stem(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name);
    matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Every key this build does not declare, carried through verbatim.
    ///
    /// The same forward-compatibility goal `source_kind`'s own doc states,
    /// applied to the whole file rather than one field. Phase 4 made the
    /// sidecar a READ-MODIFY-WRITE surface (`save_capture_timeline` rewrites
    /// it on every editor operation), and a plain struct round-trip drops
    /// whatever it does not declare — so a downgrade, a rollback, or a
    /// mixed-version sync folder would have this build silently erase a
    /// newer one's fields on the user's next keystroke. Flattening them into
    /// a catch-all makes the rewrite a genuine patch instead.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Write `sidecar` as `<base>.json` inside `dir`, atomically.
///
/// **`base` is a separate argument on purpose (GAP-108).** The path is
/// derived from the CALLER's base, never from `sidecar.base`, because a
/// sidecar is a file on disk that `read_sidecar`'s own doc says may be
/// hand-edited — so in a read-modify-write (phase 4's
/// `save_capture_timeline`) that field is untrusted input that would
/// otherwise become a path. `dir.join("../../obsidian/obsidian.json")` lands
/// outside staging entirely, and on Windows `"C:Windows"` reaches
/// drive-C-relative and `"COM1"` reaches the serial port. The caller
/// validates its base (`sanitize_title` + `reserve_base` at capture time,
/// `editor_commands::is_safe_base` at edit time); the two assertions below
/// are the backstop that makes that a property of this function rather than
/// of every present and future caller remembering.
///
/// A disagreement between `base` and `sidecar.base` is REFUSED rather than
/// silently corrected: the two naming the same capture is the invariant the
/// read side already enforces (`load_from_staging_dir`'s mismatch refusal),
/// and writing the struct under the caller's name would leave a file whose
/// own `base` no longer matches it — un-loadable, i.e. the user's edit lost
/// anyway, but quietly.
///
/// The write itself is temp + fsync + replacing rename
/// (`capture_note::write_atomic_replacing`, the same writer
/// `core::transcript`'s sidecar uses). A plain `fs::write` truncates the
/// file before the first new byte lands, so a crash inside that window
/// leaves an empty sidecar, `read_sidecar` returns `None`, and the staged
/// `.mp4` is orphaned with no recovery sweep to find it (GAP-115) — a crash
/// mid-edit losing the WHOLE capture instead of spec 10's "at most the last
/// operation".
pub fn write_sidecar(dir: &Path, base: &str, sidecar: &StagedSidecar) -> std::io::Result<PathBuf> {
    if sidecar.base != base {
        log::warn!(
            "screen staging: refusing to write a sidecar carrying base {:?} as {:?}",
            sidecar.base,
            base
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "sidecar base {:?} does not match the name it would be written under ({base:?})",
                sidecar.base
            ),
        ));
    }
    let path = dir.join(sidecar_file_name(base));
    if path.parent() != Some(dir) {
        // Containment, structurally: a base carrying a separator, a `..`, or
        // a Windows drive prefix moves the joined path out of `dir`, and
        // `join` REPLACES `self` outright for the drive-prefix case. Comparing
        // the parent catches all three without re-spelling the caller's
        // validation rules here.
        log::warn!(
            "screen staging: refusing a sidecar base that escapes the staging directory: {base:?}"
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("sidecar base {base:?} does not name a file inside the staging directory"),
        ));
    }
    let json = serde_json::to_string_pretty(sidecar)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    vault_buddy_core::capture_note::write_atomic_replacing(&path, &json)?;
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

    // REGRESSION: the SECOND trim -- the one after `.take(64)` -- exists
    // solely for a truncation landing on a `.`, `-` or space, and no fixture
    // reached it (the length test uses "x".repeat(300), which truncates onto
    // an 'x'), so weakening it to whitespace-only left the module green.
    // It is load-bearing because `capture_paths::base_name` puts the label
    // LAST: a trailing dot in the title is a trailing dot in the BASE, and
    // `editor_commands::is_safe_base` refuses `base.ends_with('.')`. Every
    // base-taking command gates on it, and `screen_recovery`'s `owned` runs
    // it too -- so such a capture is staged on disk yet cannot be edited,
    // saved or discarded, and is never swept. Silently.
    //
    // The fixture returns FEWER than MAX_TITLE_CHARS characters, which is
    // exactly why the length test could not double as this one.
    #[test]
    fn a_title_truncated_onto_a_separator_is_trimmed_again() {
        let raw = format!("{}.b", "a".repeat(MAX_TITLE_CHARS - 1));
        let title = sanitize_title(&raw);

        assert_eq!(
            title,
            "a".repeat(MAX_TITLE_CHARS - 1),
            "the 64-character cut lands on the '.', which must then be trimmed"
        );
        assert!(
            !title.ends_with('.') && !title.ends_with('-') && !title.ends_with(' '),
            "Windows silently strips a trailing dot or space, so the name on \
             disk would stop matching the name we reserved: {title:?}"
        );

        // And the base it becomes has to survive the gate every base-taking
        // command runs. `base_name`'s output is spelled out rather than
        // called, because this crate deliberately carries no chrono
        // dependency (see Cargo.toml); `is_safe_base` lives in the shell
        // crate, so assert the property it keys on plus the round trip.
        let base = format!("2026-09-20 1432 {title}");
        assert!(vault_buddy_core::capture_paths::is_capture_base(&base));
        assert!(!base.ends_with('.') && !base.ends_with(' '));
        assert_eq!(
            base_from_part(&part_file_name(&base)).as_deref(),
            Some(&*base)
        );
    }

    // REGRESSION (silent footage loss): `screen_recovery::classify` must
    // check the export shape BEFORE the plain part shape, or every abandoned
    // transcode is promoted as a capture. The cost used to be paid by the
    // user: a window titled "Build.export" produced a base ending in the
    // marker, so its orphaned `.part` classified as an ExportTemp and was
    // DELETED outright, skipping `part_holds_footage` entirely. Refusing
    // that one ending here -- where identity is decided -- is what makes the
    // collision unconstructible.
    #[test]
    fn sanitize_never_returns_a_title_ending_in_the_export_marker() {
        assert_eq!(sanitize_title("Build.export"), "Build.export_");
        // The trailing-separator trim can CREATE the ending, so the check
        // has to run after it.
        assert_eq!(sanitize_title("Build.export. "), "Build.export_");
        // ...and so can the 64-character truncation, which is why it cannot
        // run before that either.
        let truncating = format!("{}.exportable notes", "a".repeat(MAX_TITLE_CHARS - 7));
        let title = sanitize_title(&truncating);
        assert!(
            !title.ends_with(EXPORT_PART_INFIX),
            "the cut landed exactly on the marker: {title:?}"
        );
        assert!(
            title.chars().count() <= MAX_TITLE_CHARS,
            "disambiguating must not breach the length budget: {title:?}"
        );
    }

    // The other half: the disambiguation must touch ONLY a title ending in
    // exactly the marker. `sanitize_title` decides base IDENTITY --
    // `is_capture_base`, the sidecar's own `base` round trip and
    // `screen_recovery::classify` all key on it -- so mangling anything else
    // renames people's captures for nothing.
    #[test]
    fn a_title_merely_containing_the_export_marker_is_untouched() {
        for unchanged in [
            "Build.exporter",
            "Build.export.log",
            "My.export.notes and more",
            "export",
            "Figma \u{2014} Design System",
            "report.json",
            "a.exports",
        ] {
            assert_eq!(
                sanitize_title(unchanged),
                unchanged,
                "{unchanged:?} was rewritten"
            );
        }
        // A LEADING marker is already handled by the edge trim, which strips
        // the dot -- and must keep behaving exactly as it did.
        assert_eq!(sanitize_title(".export"), "export");
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

    // The export temp's name and the sweep that deletes it must agree.
    // `screen_recovery::classify` strips EXPORT_PART_INFIX off the base
    // `base_from_part` recovers; if this round trip ever breaks, an
    // abandoned export temp stops being recognized and becomes permanent
    // litter -- silently, because nothing else in the app ever reads it.
    #[test]
    fn an_export_temp_name_round_trips_back_to_its_base() {
        let base = "2026-09-20 1432 Demo";
        let name = export_part_file_name(base);
        assert_eq!(name, ".2026-09-20 1432 Demo.export.mp4.part");
        let recovered = base_from_part(&name).expect("an export temp is a part file");
        assert_eq!(
            recovered.strip_suffix(EXPORT_PART_INFIX),
            Some(base),
            "the sweep recovers a different base than the worker wrote"
        );
    }

    // And it must NOT be mistaken for an ordinary capture part: a capture
    // part promotes to a staged recording, so an export temp classified as
    // one would offer the user a half-written transcode as footage.
    #[test]
    fn an_export_temp_is_not_an_ordinary_capture_part() {
        let base = "2026-09-20 1432 Demo";
        assert_ne!(export_part_file_name(base), part_file_name(base));
        assert_ne!(
            base_from_part(&export_part_file_name(base)).as_deref(),
            Some(base)
        );
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
    fn is_reserved_device_stem_matches_case_insensitively_and_with_any_extension() {
        assert!(is_reserved_device_stem("CON"));
        assert!(is_reserved_device_stem("con"));
        assert!(is_reserved_device_stem("NUL"));
        assert!(is_reserved_device_stem("COM1"));
        assert!(is_reserved_device_stem("com1.foo"));
        assert!(is_reserved_device_stem("LPT1"));
        assert!(
            !is_reserved_device_stem("COM10"),
            "only COM1-COM9 are reserved"
        );
        assert!(
            !is_reserved_device_stem("console"),
            "a longer name sharing a prefix is not reserved"
        );
        assert!(!is_reserved_device_stem("cap"));
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
            extra: Default::default(),
        };
        let dir = tempfile::tempdir().unwrap();
        let path = write_sidecar(dir.path(), &s.base, &s).unwrap();
        let back = read_sidecar(&path).expect("sidecar reads back");
        assert_eq!(back.base, s.base);
        assert_eq!(back.duration_ms, 197_000);
        assert_eq!(back.source_title, "Figma \u{2014} Design System");
    }

    fn sidecar_named(base: &str) -> StagedSidecar {
        StagedSidecar {
            base: base.to_string(),
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
            extra: Default::default(),
        }
    }

    // C-1: a sidecar is a file on disk that `read_sidecar`'s own doc says
    // may be hand-edited, so in phase 4's read-modify-write `sidecar.base`
    // is untrusted input. Deriving the WRITE path from it (what this
    // function used to do) turns a hand-edited `"base": "../../evil"` into a
    // write outside staging altogether -- and the caller, which validated
    // only the base it was ASKED for, sees `Ok`. The path comes from the
    // caller's base now, and a disagreement is refused rather than silently
    // corrected: writing it under the caller's name would leave a file whose
    // own `base` no longer matches it, which the read side then refuses,
    // losing the edit anyway.
    //
    // Single-guard fixture on purpose: "cap" is a perfectly safe base, so
    // the containment assertion below cannot be what fails this.
    #[test]
    fn a_sidecar_whose_base_disagrees_with_the_requested_name_is_refused() {
        // The escape target is a sibling INSIDE this test's own tempdir,
        // never the shared system temp directory: two tests that both
        // reached `dir.parent()` would collide on one `evil.json` and each
        // would then be asserting against the other's leftovers.
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("screen-captures");
        std::fs::create_dir(&dir).unwrap();
        let escaping = root.path().join("evil.json");
        let mut s = sidecar_named("cap");
        s.base = "../evil".to_string();

        assert!(write_sidecar(&dir, "cap", &s).is_err());
        assert!(
            !escaping.exists(),
            "the write must not land outside the staging directory"
        );
        assert!(
            !dir.join("cap.json").exists(),
            "a refused write must leave nothing behind at all"
        );
    }

    // The backstop that makes containment a property of this function
    // rather than of every present and future caller remembering to
    // validate (GAP-108). Single-guard fixture: `base` and `sidecar.base`
    // AGREE here, so the mismatch refusal above cannot be what fails it.
    #[test]
    fn a_base_that_escapes_the_staging_directory_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("screen-captures");
        std::fs::create_dir(&dir).unwrap();
        let escaping = root.path().join("evil.json");
        let s = sidecar_named("../evil");

        assert!(write_sidecar(&dir, "../evil", &s).is_err());
        assert!(!escaping.exists(), "nothing may be written outside staging");
    }

    // I-1: `fs::write` is create-truncate -- it empties the file before the
    // first new byte lands, so a crash inside that window leaves an empty
    // sidecar, `read_sidecar` then returns `None`, and the staged `.mp4` is
    // orphaned with no recovery sweep to find it (GAP-115). Spec 10 promises
    // a crash mid-edit loses at most the last OPERATION; that writer loses
    // the whole CAPTURE, and phase 4 is what put the rewrite on a
    // once-per-editor-operation path.
    //
    // A hard link is a second name for the SAME bytes, so it witnesses
    // which of the two happened: a temp + rename writer builds a new file
    // and swaps it in, leaving the original bytes intact behind the link,
    // while a create-truncate writer destroys them in place.
    #[test]
    fn a_rewrite_builds_a_new_file_rather_than_truncating_the_old_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_sidecar(dir.path(), "cap", &sidecar_named("cap")).unwrap();
        let witness = dir.path().join("witness");
        std::fs::hard_link(&path, &witness).unwrap();

        let mut second = sidecar_named("cap");
        second.duration_ms = 999;
        write_sidecar(dir.path(), "cap", &second).unwrap();

        assert_eq!(read_sidecar(&path).unwrap().duration_ms, 999);
        assert_eq!(
            read_sidecar(&witness).unwrap().duration_ms,
            1,
            "the previous sidecar's bytes must never be rewritten in place -- \
             the new content has to be built elsewhere and renamed over"
        );
        // And the temp it was built in must not be left behind: phase 5's
        // staging recovery will sweep this directory.
        let mut names: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec!["cap.json".to_string(), "witness".to_string()]);
    }

    // I-3: `source_kind`'s own doc states forward compatibility as a design
    // goal of this file -- "a sidecar written by a future version ... still
    // deserializes here instead of failing the whole file". Phase 4 made
    // this file a read-modify-write surface, and a plain struct round-trip
    // drops every key this build does not declare, so a downgrade, a
    // rollback or a mixed-version sync folder would have an older build
    // erase a newer one's fields on the user's next editor keystroke.
    #[test]
    fn a_rewrite_preserves_keys_this_build_does_not_declare() {
        let dir = tempfile::tempdir().unwrap();
        let mut written = serde_json::to_value(sidecar_named("cap")).unwrap();
        written["exportedTo"] = serde_json::json!("Work/Screen Captures/cap.md");
        written["cropRect"] = serde_json::json!({"x": 1, "y": 2});
        std::fs::write(
            dir.path().join("cap.json"),
            serde_json::to_vec_pretty(&written).unwrap(),
        )
        .unwrap();

        let mut back = read_sidecar(&dir.path().join("cap.json")).expect("reads back");
        back.timeline = Some(serde_json::json!({"segments": []}));
        let path = write_sidecar(dir.path(), "cap", &back).unwrap();

        let reread: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(reread["exportedTo"], "Work/Screen Captures/cap.md");
        assert_eq!(reread["cropRect"]["y"], 2);
        assert_eq!(reread["timeline"]["segments"], serde_json::json!([]));
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
            extra: Default::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"vaultId\""), "got {json}");
        assert!(json.contains("\"durationMs\""), "got {json}");
        assert!(!json.contains("vault_id"), "got {json}");
    }
}
