//! Which files one staged capture owns, and what they weigh on disk.
//!
//! Its own module rather than more of `staging.rs` for a measured reason:
//! that file sat at 790 nonblank against this repo's 800-line Rust cap and
//! has already been hand-trimmed back under it once, so the next feature to
//! land there breaches a baseline that is shrink-only. The seam is real
//! enough to stand on its own terms — `staging.rs` decides what a capture is
//! CALLED, and this decides which of those names exist together and how much
//! room they take.
//!
//! **Everything here is no-follow.** A symlink wearing one of our names is
//! not ours: its target lives outside staging, so counting its length would
//! report disk this app cannot free, and following it is how a delete escapes
//! the directory. `symlink_metadata` + an explicit `is_file()` is the same
//! discipline `staged_commands::discard_staged_files` applies before it
//! unlinks anything, and the two must not disagree about what a capture is.

use std::path::Path;

use crate::staging;

/// Everything staging can hold for ONE capture: the published video, its
/// sidecar, and an export temp abandoned by a killed or crashed save.
///
/// The export temp is the one a two-file view forgets, and it is routinely
/// the LARGEST of the three — a re-encode of the whole recording. Leaving it
/// out of the accounting would under-report the directory by more than the
/// captures themselves.
///
/// Plus the synchronized webcam file (F-22) — ALWAYS, since its name
/// derives from `base` alone — and each of the sidecar's `stems` (F24; Task
/// 53 supplies the list, every caller passes `&[]` until then). A stem name
/// is owned only when it is literally `staging::stem_file_name(base, n)`:
/// the list comes from a hand-editable sidecar, and whatever this returns a
/// discard deletes and `resolve_source` serves.
///
/// Deliberately NOT the in-progress `.<base>.mp4.part` (nor the webcam or
/// stem `.part`): those belong to a capture that is still being written,
/// which is not a staged capture and is not something a bulk clear may
/// touch. An abandoned one is the recovery sweep's.
pub fn capture_file_names(base: &str, stems: &[String]) -> Vec<String> {
    let mut names = vec![
        staging::mp4_file_name(base),
        staging::sidecar_file_name(base),
        staging::export_part_file_name(base),
    ];
    names.extend(companion_file_names(base, stems));
    names
}

/// The capture's COMPANION media only — its webcam file and its stems —
/// never its own video, sidecar or export temp. What a project's
/// `StagingFile` source may name (`project_store::resolve_source`): a
/// `StagingFile` naming `<base>.mp4` would be a second locator for the
/// capture that every `Staging`-only matcher misses, and one naming the
/// sidecar or a live export temp would be packaged as "media".
pub fn companion_file_names(base: &str, stems: &[String]) -> Vec<String> {
    let mut names = vec![staging::webcam_file_name(base)];
    names.extend(stems.iter().filter(|s| is_stem_of(base, s)).cloned());
    names
}

/// Is `name` exactly `staging::stem_file_name(base, n)` for some index?
fn is_stem_of(base: &str, name: &str) -> bool {
    name.strip_prefix(base)
        .and_then(|rest| rest.strip_prefix(staging::STEM_INFIX))
        .and_then(|rest| rest.strip_suffix(".m4a"))
        .and_then(|index| index.parse::<u32>().ok())
        .is_some_and(|n| staging::stem_file_name(base, n) == name)
}

/// What staging is holding, as a settings card reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagingUsage {
    pub captures: usize,
    pub bytes: u64,
}

/// The bytes one staged capture occupies, across every file it owns.
///
/// A missing file contributes nothing rather than failing the measurement: a
/// capture that never had an export temp is the ordinary case, and a size
/// readout that errors because a file it did not need is absent would be
/// worse than useless.
pub fn capture_bytes(dir: &Path, base: &str) -> u64 {
    capture_file_names(base, &[])
        .iter()
        .filter_map(|name| std::fs::symlink_metadata(dir.join(name)).ok())
        .filter(|meta| meta.file_type().is_file())
        .map(|meta| meta.len())
        .sum()
}

/// Measure the staged captures named in `bases`.
///
/// It takes the bases rather than reading the directory itself, and that is
/// the safety property: the ONLY things counted are captures some caller has
/// already established are ours. A `read_dir` here would have to re-derive
/// ownership, and a second copy of that rule is how a foreign file ends up
/// inside a number labelled "what Clear will free".
pub fn usage(dir: &Path, bases: &[String]) -> StagingUsage {
    StagingUsage {
        captures: bases.len(),
        bytes: bases.iter().map(|b| capture_bytes(dir, b)).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "2026-09-20 1432 Demo";
    const OTHER: &str = "2026-09-20 1500 Other";

    fn write(dir: &Path, name: &str, bytes: usize) {
        std::fs::write(dir.join(name), vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn a_capture_owns_its_video_its_sidecar_and_its_export_temp() {
        let names = capture_file_names(BASE, &[]);
        assert!(names.contains(&staging::mp4_file_name(BASE)));
        assert!(names.contains(&staging::sidecar_file_name(BASE)));
        assert!(
            names.contains(&staging::export_part_file_name(BASE)),
            "the export temp is often the largest file a capture owns: {names:?}"
        );
    }

    // F24: the webcam file is derivable from the base and so ALWAYS owned;
    // stems come from the sidecar's list (empty until Task 53), and only a
    // name in this capture's own stem shape may join the set -- the list is
    // hand-editable, and whatever this returns, a discard deletes.
    #[test]
    fn capture_file_names_includes_webcam_and_accepts_a_stem_list() {
        let names = capture_file_names(BASE, &[]);
        assert!(
            names.contains(&staging::webcam_file_name(BASE)),
            "the webcam file must be discarded, cleared and measured with its capture: {names:?}"
        );
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(!names.contains(&staging::webcam_part_file_name(BASE)));

        let stems = vec![
            staging::stem_file_name(BASE, 1),
            staging::stem_file_name(BASE, 2),
            // Not this capture's stem shape: never owned, whatever the
            // sidecar claims.
            "../escape.m4a".to_string(),
            staging::stem_file_name(OTHER, 1),
            staging::mp4_file_name(OTHER),
            format!("{BASE}.stem-.m4a"),
        ];
        let names = capture_file_names(BASE, &stems);
        assert_eq!(
            names[4..],
            [
                staging::stem_file_name(BASE, 1),
                staging::stem_file_name(BASE, 2)
            ],
            "{names:?}"
        );
    }

    #[test]
    fn the_in_progress_part_is_not_one_of_a_staged_captures_files() {
        // A live capture's `.part` belongs to a recording still being
        // written. Counting it would bill the user for a capture they have
        // not finished; clearing it would delete one mid-write.
        let names = capture_file_names(BASE, &[]);
        assert!(
            !names.contains(&staging::part_file_name(BASE)),
            "a bulk clear must never reach a live capture's .part: {names:?}"
        );
    }

    #[test]
    fn bytes_sum_every_file_a_capture_owns() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &staging::mp4_file_name(BASE), 100);
        write(d.path(), &staging::sidecar_file_name(BASE), 20);
        write(d.path(), &staging::export_part_file_name(BASE), 300);
        write(d.path(), &staging::webcam_file_name(BASE), 4_000);
        assert_eq!(capture_bytes(d.path(), BASE), 4_420);
    }

    #[test]
    fn a_capture_with_no_export_temp_still_measures() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &staging::mp4_file_name(BASE), 100);
        write(d.path(), &staging::sidecar_file_name(BASE), 20);
        assert_eq!(capture_bytes(d.path(), BASE), 120);
    }

    #[test]
    fn usage_counts_only_the_bases_it_was_given() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), &staging::mp4_file_name(BASE), 100);
        write(d.path(), &staging::mp4_file_name(OTHER), 500);
        // A foreign file, and a name-shaped one this caller did not vouch
        // for: neither may enter a number the UI labels as ours to free.
        write(d.path(), "holiday.mp4", 9_000);
        write(d.path(), "notes.txt", 7_000);

        let u = usage(d.path(), &[BASE.to_string()]);
        assert_eq!(u.captures, 1);
        assert_eq!(
            u.bytes, 100,
            "only the vouched-for base may be counted, got {u:?}"
        );
    }

    #[test]
    fn usage_over_no_captures_is_zero() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(usage(d.path(), &[]), StagingUsage::default());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_wearing_our_name_is_never_counted() {
        // Its target is outside staging, so its length is disk this app
        // cannot free — and `discard_staged_files` refuses to unlink through
        // it, so counting it would promise space no Clear could deliver.
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("elsewhere.bin");
        std::fs::write(&outside, vec![b'x'; 4_096]).unwrap();
        std::os::unix::fs::symlink(&outside, d.path().join(staging::mp4_file_name(BASE))).unwrap();
        write(d.path(), &staging::sidecar_file_name(BASE), 20);

        assert_eq!(
            capture_bytes(d.path(), BASE),
            20,
            "a symlinked leaf must contribute nothing"
        );
    }
}
