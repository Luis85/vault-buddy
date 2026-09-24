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

const PART_SUFFIX: &str = ".mp4.part";

pub fn staging_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join(STAGING_DIR_NAME)
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

/// The infix a capture's synchronized WEBCAM file carries (F-22), in both
/// its published `<base>.webcam.mp4` and its in-progress
/// `.<base>.webcam.mp4.part` shapes. Shared for the `EXPORT_PART_INFIX`
/// reason: `screen_recovery::classify` strips exactly this to find the
/// capture a webcam part belongs to, and `staging_title` refuses a title
/// ending in it.
pub const WEBCAM_INFIX: &str = ".webcam";

/// The published webcam file, `<base>.webcam.mp4` — derivable from `base`
/// alone, since a capture has at most one webcam track.
pub fn webcam_file_name(base: &str) -> String {
    mp4_file_name(&format!("{base}{WEBCAM_INFIX}"))
}

/// The hidden in-progress webcam file, `.<base>.webcam.mp4.part`: the
/// capture `.part` shape with an infix, exactly as the export temp is, so it
/// round-trips through `base_from_part` and the recovery sweep sees it.
pub fn webcam_part_file_name(base: &str) -> String {
    part_file_name(&format!("{base}{WEBCAM_INFIX}"))
}

/// The infix an audio STEM carries between its base and its index (Task
/// 53 mints them; `capture_file_names` and the recovery sweep must already
/// recognise the shape).
pub const STEM_INFIX: &str = ".stem-";
const STEM_SUFFIX: &str = ".m4a";

/// A published stem, `<base>.stem-<index>.m4a`.
pub fn stem_file_name(base: &str, index: u32) -> String {
    format!("{base}{STEM_INFIX}{index}{STEM_SUFFIX}")
}

/// A stem being written, `.<base>.stem-<index>.m4a.part`.
pub fn stem_part_file_name(base: &str, index: u32) -> String {
    format!(".{}.part", stem_file_name(base, index))
}

/// Does `text` END in `.stem-<digits>`? The pattern (`\.stem-\d+$`), not a
/// list: a sweep has no sidecar to read the real stem count from.
pub fn ends_with_stem_marker(text: &str) -> bool {
    text.rsplit_once(STEM_INFIX)
        .is_some_and(|(_, digits)| is_digit_run(digits))
}

fn is_digit_run(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// `(base, index)` of a stem PART by pattern (`^\.(.+)\.stem-\d+\.m4a\.part$`),
/// or `None`. The index stays text: `\d+` admits a run no integer holds,
/// and a name the sweep cannot parse must still be recognised as ours.
pub fn stem_part_base(file_name: &str) -> Option<(String, String)> {
    let stem = file_name
        .strip_prefix('.')?
        .strip_suffix(".part")?
        .strip_suffix(STEM_SUFFIX)?;
    let (base, index) = stem.rsplit_once(STEM_INFIX)?;
    (!base.is_empty() && is_digit_run(index)).then(|| (base.to_string(), index.to_string()))
}

/// Is `name`'s STEM (the text before the first `.`) one of Windows' reserved
/// device names, case-insensitively?
///
/// Windows resolves a path component whose stem matches one of these to the
/// PHYSICAL device it names, regardless of the surrounding directory or any
/// extension — `dir.join("COM1.json")` opens the COM1 serial port, not a
/// file called that (GAP-108). Public so every caller that turns untrusted
/// text into a path component can share one list instead of drifting apart:
/// `editor_commands::is_safe_base` (phase 4) refuses such a name from the
/// frontend, and `reserve_base` renames past one when minting a base.
///
/// `staging_title::sanitize_title` deliberately does NOT consume it. That
/// function yields a FRAGMENT, which `capture_paths::base_name` prefixes
/// with `YYYY-MM-DD HHmm ` before it is ever a path component — so a window
/// titled "CON" already produces the perfectly writable
/// `2026-09-21 1430 CON.mp4`, and checking there would only cost that user
/// an underscore. The rule belongs where a NAME is minted, which is here.
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

/// Keep a base from naming a Windows reserved DEVICE rather than a file.
///
/// Win32 resolves a path component whose stem matches `CON`/`PRN`/`AUX`/
/// `NUL`/`COM1`-`COM9`/`LPT1`-`LPT9` to the physical device, with or without
/// an extension — `dir.join("CON.mp4")` opens the console, so the capture
/// fails at `File::create` after the user has already started recording.
///
/// **In practice this never fires, and that is the point.** Every base
/// reaching `reserve_base` is `capture_paths::base_name`'s
/// `YYYY-MM-DD HHmm <title>`, so its stem begins with the date and cannot
/// equal a reserved name however the recorded window is titled. GAP-108
/// proposed putting this in `sanitize_title` instead, which would have cost
/// a window honestly titled "CON" a file called
/// `2026-09-21 1430 CON_.mp4` in exchange for no safety at all. This is the
/// backstop that makes the property hold for any FUTURE caller that mints a
/// base without that prefix, rather than renaming today's captures.
///
/// The `_` goes on the STEM, never the tail: `is_reserved_device_stem` reads
/// the text before the first dot, so `"con.mp4_"` still names the device.
fn disambiguate_reserved_device(base: &str) -> String {
    if !is_reserved_device_stem(base) {
        return base.to_string();
    }
    match base.find('.') {
        Some(dot) => format!("{}_{}", &base[..dot], &base[dot..]),
        None => format!("{base}_"),
    }
}

/// Find a free base in `dir`, suffixing ` (N)` on collision.
///
/// A base is free only when ALL THREE names it owns are free — the staged
/// `.mp4`, the `.json` sidecar and the hidden `.mp4.part`. This is the
/// pairwise reservation the audio domain uses, widened to three: checking
/// only the `.mp4` would let a second capture adopt a base whose
/// in-progress `.part` still exists, and the two captures would then write
/// the same file.
///
/// The capture's WEBCAM file and part (F-22) are two more names it owns:
/// a leftover `<base>.webcam.mp4` would otherwise be adopted as the new
/// capture's own webcam track.
///
/// It also refuses to hand back a base whose stem is a Windows reserved
/// device name (GAP-108), because this is where a base becomes the name
/// THREE files are created under — putting the check at any one of those
/// writes would leave the other two.
pub fn reserve_base(dir: &Path, base: &str) -> String {
    let base = &disambiguate_reserved_device(base);
    let free = |candidate: &str| {
        !dir.join(mp4_file_name(candidate)).exists()
            && !dir.join(sidecar_file_name(candidate)).exists()
            && !dir.join(part_file_name(candidate)).exists()
            && !dir.join(webcam_file_name(candidate)).exists()
            && !dir.join(webcam_part_file_name(candidate)).exists()
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
    /// The synchronized webcam track recorded beside the screen (F-22), or
    /// `None` — every sidecar before this build, and every capture made
    /// without a webcam. Skipped when absent so those sidecars stay
    /// byte-identical on a rewrite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webcam: Option<WebcamSidecar>,
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

/// A staged capture's synchronized webcam track (F-22): which staging file
/// holds it, its pixel size, the device it came from, and where its first
/// frame sits on the capture's shared clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebcamSidecar {
    /// A staging file NAME (`webcam_file_name(base)`), never a path.
    pub file: String,
    pub width: u32,
    pub height: u32,
    pub device_label: String,
    /// Milliseconds on the capture's `CaptureClock` at which the webcam's
    /// first frame arrived — where migration starts its clip. Signed: a
    /// device that delivered before the screen's first frame is negative.
    pub offset_ms: i64,
    /// Keys a newer build adds to this block, carried through verbatim for
    /// the same reason `StagedSidecar::extra` exists.
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
#[path = "staging_webcam_tests.rs"]
mod webcam_tests;

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
    fn reserve_base_never_hands_back_a_reserved_windows_device_name() {
        // GAP-108. Win32 resolves a path component whose stem is one of the
        // reserved device names to the PHYSICAL device, so `dir.join("CON.mp4")`
        // opens the console rather than creating a file. `reserve_base` is
        // where a base becomes the name three files are created under, so the
        // refusal belongs here rather than at any one of the three writes.
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(reserve_base(dir.path(), "CON"), "CON_");
        assert_eq!(reserve_base(dir.path(), "nul"), "nul_");
        assert_eq!(reserve_base(dir.path(), "com9"), "com9_");
    }

    #[test]
    fn a_reserved_base_is_disambiguated_on_its_stem_not_its_tail() {
        // The mutation this test exists to catch: appending the `_` to the
        // whole string instead of to the stem. `is_reserved_device_stem`
        // reads the text before the FIRST dot, so "con.mp4_" still names the
        // console device and a tail-append would leave the bug in place while
        // looking like a fix.
        let dir = tempfile::tempdir().unwrap();
        let out = reserve_base(dir.path(), "con.mp4");
        assert_eq!(out, "con_.mp4");
        assert!(!is_reserved_device_stem(&out));
    }

    #[test]
    fn a_real_capture_base_is_never_renamed_by_the_reserved_name_guard() {
        // The guard is a BACKSTOP, not a renamer, and this pins why. Every
        // base minted for a capture is `capture_paths::base_name`'s
        // `YYYY-MM-DD HHmm <title>`, whose stem therefore begins with the
        // date — so it can never equal a reserved device name however the
        // recorded window is titled. A guard placed in `sanitize_title`
        // instead would fire here and cost the user a file called
        // "2026-09-21 1430 CON_.mp4" for a name that was always writable.
        let dir = tempfile::tempdir().unwrap();
        for title in ["CON", "nul", "COM1", "LPT9", "aux"] {
            let base = format!("2026-09-21 1430 {title}");
            assert!(
                !is_reserved_device_stem(&base),
                "the timestamp prefix makes {base:?} an ordinary name"
            );
            assert_eq!(reserve_base(dir.path(), &base), base);
        }
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
            webcam: None,
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
            webcam: None,
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
            webcam: None,
            extra: Default::default(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"vaultId\""), "got {json}");
        assert!(json.contains("\"durationMs\""), "got {json}");
        assert!(!json.contains("vault_id"), "got {json}");
    }
}
