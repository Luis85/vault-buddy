//! Turning an application-supplied window title into a safe file-name
//! FRAGMENT.
//!
//! Its own module rather than more of `staging.rs` because that file sits
//! against this repo's 800-line Rust cap and has already been hand-trimmed
//! back under it once — but the seam stands on its own terms, and GAP-108 is
//! the proof that it needed stating. Everything here takes a `&str` and
//! returns a `String`; nothing here touches a `Path`. That is the whole
//! distinction: **this produces a fragment, not a name.**
//!
//! A capture's file name is `capture_paths::base_name(date, h, m, fragment)`
//! — `YYYY-MM-DD HHmm <fragment>` — and only then `staging::reserve_base`.
//! So the fragment is never a path component by itself, and a rule that
//! belongs to a NAME does not belong here. GAP-108 filed the opposite: it
//! read `sanitize_title("CON") == "CON"` as a capture that could not create
//! its own file, when the name actually minted is
//! `2026-09-21 1430 CON.mp4`, which Windows opens perfectly well. Enforcing
//! the reserved-device rule here would have renamed that user's capture for
//! no safety at all, so it lives in `staging::reserve_base`, where the name
//! is minted, instead.
//!
//! What DOES belong here is anything about the title's own characters: the
//! reserved-character mapping (a security boundary — a window title is
//! whatever the recorded application put in its title bar, and an
//! unsanitized separator in it escapes the staging directory), the length
//! bound, and the owned-suffix (export, webcam, stem) disambiguation.

use crate::staging::{ends_with_stem_marker, EXPORT_PART_INFIX, WEBCAM_INFIX};

/// Longest title we will put in a file name. MAX_PATH is 260 by default on
/// Windows; the staging path, the `YYYY-MM-DD HHmm ` prefix, a possible
/// ` (12)` collision suffix and `.mp4.part` all have to fit alongside it.
pub const MAX_TITLE_CHARS: usize = 64;

/// Fallback when a title sanitizes to nothing. Never an empty string and
/// never ".": both are directory references, and `dir.join(".")` is the
/// staging directory itself, not a file in it.
const FALLBACK_TITLE: &str = "Screen Capture";

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
    let trimmed = disambiguate_owned_suffix(trimmed);

    if trimmed.is_empty() {
        FALLBACK_TITLE.to_string()
    } else {
        trimmed
    }
}

/// Keep a sanitized title from ENDING in an owned-file marker:
/// `EXPORT_PART_INFIX`, and since F-22 (F25) `WEBCAM_INFIX` and a stem
/// marker `.stem-<digits>` too — one rule for all three.
///
/// The webcam and stem cases are the export case again: a base ending in
/// `.webcam` mints a VIDEO named exactly like the webcam file of the base
/// without it, and its `.part` exactly like that capture's webcam part (the
/// sweep would attribute it to the wrong capture). A base ending in
/// `.stem-3` collides with no file today (a stem is `.m4a`), but it would
/// END in an owned-file marker, which the rule exists to make unmintable.
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
fn disambiguate_owned_suffix(title: String) -> String {
    let owned = title.ends_with(EXPORT_PART_INFIX)
        || title.ends_with(WEBCAM_INFIX)
        || ends_with_stem_marker(&title);
    if !owned {
        return title;
    }
    let mut out = title;
    if out.chars().count() >= MAX_TITLE_CHARS {
        // Room for the `_` comes out of the title's own tail, so this can
        // never breach the length budget. The character dropped is the
        // marker's last, and the `_` then ends the title — never a dot or
        // space Windows would strip.
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

#[cfg(test)]
mod tests {
    use super::*;
    // The round-trip assertion below deliberately crosses the seam: a
    // fragment is only safe if the NAME built from it survives the
    // part-file round trip, and that name is `staging`'s to mint.
    use crate::staging::{base_from_part, part_file_name};

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

    // F25: the webcam and stem suffixes are owned-file markers exactly as
    // `.export` is. Capture B's webcam file is `<B>.webcam.mp4` -- byte-
    // identical to the VIDEO of a capture recorded a minute-mate later from
    // a window titled "Demo.webcam" (and its part to B's webcam part). The
    // stem marker has no such byte collision today (a stem is `.m4a`, a
    // capture's video `.mp4`); it is disambiguated by the same rule so no
    // base ever ENDS in an owned-file marker -- the `sanitize_title` lines
    // below are what pin that arm.
    #[test]
    fn staging_title_disambiguates_webcam_and_stem_suffixed_titles() {
        use crate::staging::{
            reserve_base, stem_file_name, stem_part_file_name, webcam_part_file_name,
        };
        use crate::staging_files::capture_file_names;
        // Every REAL file name a capture can own: its capture files, its
        // webcam file, a stem, and each of their in-progress parts.
        let every_name = |base: &str, stem: u32| -> Vec<String> {
            let mut names = capture_file_names(base, &[stem_file_name(base, stem)]);
            names.push(part_file_name(base));
            names.push(webcam_part_file_name(base));
            names.push(stem_part_file_name(base, stem));
            names
        };
        let owner = "2026-09-21 1430 Demo";
        let owned = every_name(owner, 3);
        let dir = tempfile::tempdir().unwrap();
        for title in [
            "Demo.webcam",
            "Demo.stem-3",
            "Demo.webcam.. ",
            "Demo.stem-3 -",
        ] {
            let base = reserve_base(
                dir.path(),
                &format!("2026-09-21 1430 {}", sanitize_title(title)),
            );
            for name in every_name(&base, 1) {
                assert!(
                    !owned.contains(&name),
                    "{title:?}: the new capture's {name:?} IS one of {owner:?}'s files"
                );
            }
        }
        assert_eq!(sanitize_title("Demo.webcam"), "Demo.webcam_");
        assert_eq!(sanitize_title("Demo.stem-3"), "Demo.stem-3_");
        // Merely containing either marker is not ending in it.
        for unchanged in [
            "Demo.webcams",
            "Demo.webcam.notes",
            "Demo.stem-3a",
            "stem-3",
        ] {
            assert_eq!(sanitize_title(unchanged), unchanged, "{unchanged:?}");
        }
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
}
