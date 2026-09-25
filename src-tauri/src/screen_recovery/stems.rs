//! Listing a stem the recovery sweep promoted in its capture's sidecar
//! (Task 53, F24). Split from `screen_recovery/mod.rs` at its 800-line cap;
//! the sweep itself (`sweep_staging_dir`) collects the stems it promoted and
//! calls this once the pass is done.

use std::path::Path;

use vault_buddy_screen::staging;

use super::{RecoveryAction, Sweep};

/// Add a promoted stem to its capture's sidecar `stems` list (Task 53, F24):
/// that list is what makes a stem the capture's to discard, Clear and serve,
/// so an unlisted stem would outlive every discard as litter. One that
/// already lists the stem is left as it is. A capture with NO sidecar under
/// this base (it was promoted to a ` (N)` name, or it is gone) leaves the
/// stem unlisted — logged, docs/Gaps.md GAP-201.
pub(super) fn list_recovered_stem(dir: &Path, base: &str, index: &str, sweep: &mut Sweep) {
    let Ok(index) = index.parse::<u32>() else {
        log::warn!("screen-recovery: stem index {index:?} of {base:?} is out of range; not listed");
        return;
    };
    let Some(mut sidecar) = staging::read_sidecar(&dir.join(staging::sidecar_file_name(base)))
    else {
        log::warn!(
            "screen-recovery: {base:?} has no sidecar to list its recovered stem {index} in"
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
        Err(e) => log::warn!("screen-recovery: could not list a stem in {base:?}'s sidecar: {e}"),
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
