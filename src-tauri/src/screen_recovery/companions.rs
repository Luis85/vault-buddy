//! A capture's COMPANION parts in the recovery sweep: its webcam track
//! (F-22) and its per-input stems (Task 53, F24) -- promoting a stale one
//! beside the capture it belongs to, and listing a promoted stem in that
//! capture's sidecar. Split from `screen_recovery/mod.rs` near its 800-line
//! cap; the sweep itself (`sweep_staging_dir`) decides what each entry is,
//! decides every capture's own part first, then promotes the companions and
//! lists the stems it promoted once the pass is done.

use std::collections::HashMap;
use std::path::Path;

use vault_buddy_screen::staging;

use super::{sniff_part, Entry, Found, RecoveryAction, Sniff, Sweep};
use crate::editor::redact::{redact_name, redact_path};

/// What this pass made of a capture's own part, which is where its
/// companions go (GAP-198 item 4, GAP-201 item 2). A part that is gone --
/// deleted as empty, or never there -- has no entry, and its companions keep
/// the name they were written under.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PartFate {
    /// Promoted under this base: `<base>` itself, or `<base> (N)` when
    /// `<base>.mp4` was taken.
    Landed(String),
    /// Still on disk and undecided -- not yet stale, unreadable, or its
    /// promotion failed. Its companions wait for the pass that decides it,
    /// rather than landing under a name the capture may never take.
    Waiting,
}

/// The sweep's decision order: every capture's own part before anything
/// else, so its fate is known when its companions are reached. `read_dir`
/// promises no order (NTFS happens to list `.<base>.mp4.part` before its
/// webcam and stem parts; ext4 lists by hash), so this is not left to it.
pub(super) fn parts_first(found: &[Found]) -> Vec<&Found> {
    let (mut parts, rest): (Vec<&Found>, Vec<&Found>) = found
        .iter()
        .partition(|f| matches!(f.entry, Entry::Part(_)));
    parts.extend(rest);
    parts
}

/// A webcam or stem part (F-22, F24) is promoted to its OWN published name
/// under the base its capture's part LANDED on: `.<base>.webcam.mp4.part`
/// beside a capture promoted to `<base> (2)` becomes
/// `<base> (2).webcam.mp4`, which that capture owns
/// (`staging_files::capture_file_names`; a stem once `list_recovered_stem`
/// has added it to the sidecar). It is never given a free name of its own:
/// the name IS its link to the capture. A taken name is left alone
/// (`rename_noreplace`) and not counted pending: no later pass would answer
/// differently. A companion whose capture is `Waiting` is left and counted
/// pending. The base it landed under when promoted.
pub(super) fn promote_or_delete_companion_part(
    f: &Found,
    base: &str,
    fates: &HashMap<&str, PartFate>,
    sweep: &mut Sweep,
) -> Option<String> {
    let owner = match fates.get(base) {
        Some(PartFate::Waiting) => {
            sweep.pending += 1;
            return None;
        }
        Some(PartFate::Landed(landed)) => landed.as_str(),
        None => base,
    };
    // The part's published name, `<base><suffix>`, with the suffix (the
    // webcam infix, or the stem's `.stem-<n>.m4a` exactly as written) kept
    // and the base swapped for the owner's.
    let suffix = f
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix('.'))
        .and_then(|n| n.strip_suffix(".part"))
        .and_then(|n| n.strip_prefix(base))?;
    let to = f.path.with_file_name(format!("{owner}{suffix}"));
    if sniff_part(f, sweep) != Sniff::Footage {
        return None;
    }
    match vault_buddy_core::capture_paths::rename_noreplace(&f.path, &to) {
        Ok(()) => {
            log::info!("screen-recovery: recovered {}", redact_path(&to));
            sweep.actions.push(RecoveryAction::Promoted(to));
            Some(owner.to_string())
        }
        Err(e) => {
            log::warn!(
                "screen-recovery: could not promote {} to {}: {e}",
                redact_path(&f.path),
                redact_path(&to)
            );
            None
        }
    }
}

/// Add a promoted stem to its capture's sidecar `stems` list (Task 53, F24):
/// that list is what makes a stem the capture's to discard, Clear and serve,
/// so an unlisted stem would outlive every discard as litter. One that
/// already lists the stem is left as it is. `base` is the one the capture
/// LANDED on (see `PartFate`). A capture with NO sidecar under it (its own
/// part is gone, and so is its published capture) leaves the stem unlisted
/// — logged, docs/Gaps.md GAP-201.
pub(super) fn list_recovered_stem(dir: &Path, base: &str, index: &str, sweep: &mut Sweep) {
    let Ok(index) = index.parse::<u32>() else {
        log::warn!(
            "screen-recovery: a stem index of {} is out of range; not listed",
            redact_name(base)
        );
        return;
    };
    let Some(mut sidecar) = staging::read_sidecar(&dir.join(staging::sidecar_file_name(base)))
    else {
        log::warn!(
            "screen-recovery: {} has no sidecar to list its recovered stem {index} in",
            redact_name(base)
        );
        return;
    };
    let file = staging::stem_file_name(base, index);
    if sidecar.stems.iter().any(|s| s.file == file) {
        return;
    }
    let input = usize::try_from(index)
        .ok()
        .and_then(|i| sidecar.inputs.get(i.checked_sub(1)?))
        .cloned()
        .unwrap_or_else(|| format!("Audio input {index}"));
    let mut stems = sidecar.stems.clone();
    stems.push(staging::StemSidecar {
        index,
        input,
        file,
        extra: serde_json::Map::new(),
    });
    stems.sort_by_key(|s| s.index);
    sidecar.set_stems(stems);
    match staging::write_sidecar(dir, base, &sidecar) {
        Ok(path) => sweep.actions.push(RecoveryAction::WroteSidecar(path)),
        Err(e) => log::warn!(
            "screen-recovery: could not list a stem in {}'s sidecar: {e}",
            redact_name(base)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::super::decide::fixtures::*;
    use super::super::*;

    /// A capture's part and sidecar in the staging directory.
    fn names(d: &Path, b: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        (
            d.join(staging::part_file_name(b)),
            d.join(staging::sidecar_file_name(b)),
        )
    }

    fn stale(dir: &Path) -> Sweep {
        let later = SystemTime::now() + Duration::from_secs(3600);
        sweep_staging_dir(dir, later, Duration::from_secs(60))
    }

    /// The stem files a sidecar lists, `(index, file)`.
    fn listed(dir: &Path, base: &str) -> Vec<(u32, String)> {
        let read = staging::read_sidecar(&dir.join(staging::sidecar_file_name(base)))
            .unwrap_or_else(|| panic!("{base} has no readable sidecar"));
        read.stems
            .iter()
            .map(|s| (s.index, s.file.clone()))
            .collect()
    }

    // GAP-198 item 4, GAP-201 item 2: a crashed capture whose `<base>.mp4`
    // is already taken (a same-minute collision) lands on `<base> (2)`. Its
    // webcam and stem parts must land beside THAT name and the stem be
    // listed in THAT sidecar -- not beside the other capture, which would
    // then appear to own them and delete them with its own discard.
    #[test]
    fn companions_land_beside_the_name_their_capture_landed_on() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        // The OTHER capture, a real one: footage and a sidecar.
        std::fs::write(d.join(staging::mp4_file_name(BASE)), b"an earlier capture").unwrap();
        staging::write_sidecar(d, BASE, &minimal_sidecar(BASE, SystemTime::now())).unwrap();
        // The crashed one, under the same base: main, webcam and stem parts.
        std::fs::write(d.join(staging::part_file_name(BASE)), footage()).unwrap();
        std::fs::write(d.join(staging::webcam_part_file_name(BASE)), footage()).unwrap();
        std::fs::write(d.join(staging::stem_part_file_name(BASE, 1)), footage()).unwrap();

        let sweep = stale(d);

        let landed = format!("{BASE} (2)");
        for file in [
            staging::mp4_file_name(&landed),
            staging::webcam_file_name(&landed),
            staging::stem_file_name(&landed, 1),
        ] {
            assert!(d.join(&file).is_file(), "{file} is missing: {sweep:?}");
        }
        assert_eq!(
            listed(d, &landed),
            [(1, staging::stem_file_name(&landed, 1))]
        );
        // The other capture gained nothing and kept its own footage.
        assert!(!d.join(staging::webcam_file_name(BASE)).exists());
        assert!(!d.join(staging::stem_file_name(BASE, 1)).exists());
        assert!(
            listed(d, BASE).is_empty(),
            "the stem was listed in the wrong capture"
        );
        assert_eq!(
            std::fs::read(d.join(staging::mp4_file_name(BASE))).unwrap(),
            b"an earlier capture"
        );
        // Nothing was left behind, and a second pass has nothing to do.
        for part in [
            staging::part_file_name(BASE),
            staging::webcam_part_file_name(BASE),
            staging::stem_part_file_name(BASE, 1),
        ] {
            assert!(!d.join(&part).exists(), "{part} was left behind");
        }
        let again = stale(d);
        assert!(again.actions.is_empty(), "{again:?}");
    }

    // `read_dir` order is the filesystem's: a companion listed before its
    // capture's part would find no fate and keep the name it was written
    // under. The order is the sweep's own, and pinned here.
    #[test]
    fn a_captures_own_part_is_decided_before_its_companions() {
        let found = |entry: Entry| Found {
            path: std::path::PathBuf::new(),
            entry,
            modified: SystemTime::UNIX_EPOCH,
        };
        let listed = [
            found(Entry::WebcamPart(BASE.into())),
            found(Entry::StemPart(BASE.into(), "1".into())),
            found(Entry::Part(BASE.into())),
            found(Entry::Staged(KEPT.into())),
        ];
        let order: Vec<&Entry> = parts_first(&listed).into_iter().map(|f| &f.entry).collect();
        assert_eq!(
            order,
            [
                &Entry::Part(BASE.into()),
                &Entry::WebcamPart(BASE.into()),
                &Entry::StemPart(BASE.into(), "1".into()),
                &Entry::Staged(KEPT.into()),
            ]
        );
    }

    // The same rule, one pass earlier: a companion that is stale while its
    // capture's own part is not yet (it was written a moment later) waits
    // for the capture to be decided, rather than landing -- and being
    // listed -- under a name the capture may never take.
    #[test]
    fn a_companion_waits_for_its_captures_part_to_be_decided() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let now = SystemTime::now();
        std::fs::write(d.join(staging::part_file_name(BASE)), footage()).unwrap();
        for part in [
            staging::webcam_part_file_name(BASE),
            staging::stem_part_file_name(BASE, 1),
        ] {
            let path = d.join(part);
            std::fs::write(&path, footage()).unwrap();
            let file = std::fs::File::options().write(true).open(&path).unwrap();
            file.set_modified(now - Duration::from_secs(3600)).unwrap();
        }

        let first = sweep_staging_dir(d, now, Duration::from_secs(60));

        assert!(first.actions.is_empty(), "{first:?}");
        assert_eq!(first.pending, 3, "all three come back next pass");
        assert!(d.join(staging::webcam_part_file_name(BASE)).is_file());

        stale(d);

        assert!(d.join(staging::mp4_file_name(BASE)).is_file());
        assert!(d.join(staging::webcam_file_name(BASE)).is_file());
        assert_eq!(listed(d, BASE), [(1, staging::stem_file_name(BASE, 1))]);
    }

    // Task 53 (F24): ownership of a stem is its capture's sidecar LIST, so a
    // stem part a crash left behind is promoted AND listed -- or discard and
    // Clear, which read that list, would strand it as litter. The input name
    // comes from the sidecar's `inputs` when it has one ("Audio input N"
    // otherwise, a recovered capture's minimal sidecar knows none), and a
    // second pass lists nothing twice.
    #[test]
    fn a_stale_stem_part_is_promoted_and_listed_in_its_captures_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let (part, json) = names(dir.path(), BASE);
        std::fs::write(&part, footage()).unwrap();
        std::fs::write(
            dir.path().join(staging::stem_part_file_name(BASE, 2)),
            footage(),
        )
        .unwrap();
        let mut kept = minimal_sidecar(KEPT, SystemTime::now());
        kept.inputs = vec!["USB Mic".into(), "Speakers".into()];
        staging::write_sidecar(dir.path(), KEPT, &kept).unwrap();
        std::fs::write(dir.path().join(staging::mp4_file_name(KEPT)), footage()).unwrap();
        std::fs::write(
            dir.path().join(staging::stem_part_file_name(KEPT, 2)),
            footage(),
        )
        .unwrap();

        stale(dir.path());
        stale(dir.path());

        for (base, input) in [(BASE, "Audio input 2"), (KEPT, "Speakers")] {
            let read =
                staging::read_sidecar(&dir.path().join(staging::sidecar_file_name(base))).unwrap();
            let listed: Vec<(u32, &str, &str)> = read
                .stems
                .iter()
                .map(|s| (s.index, s.input.as_str(), s.file.as_str()))
                .collect();
            let file = staging::stem_file_name(base, 2);
            assert_eq!(listed, [(2, input, file.as_str())], "{base}");
            assert!(dir.path().join(&file).is_file());
            assert!(vault_buddy_screen::staging_files::capture_file_names(
                base,
                &read.stem_files()
            )
            .contains(&file));
        }
        assert!(json.is_file());
    }
}
