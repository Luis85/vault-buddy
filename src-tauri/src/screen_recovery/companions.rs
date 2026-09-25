//! A capture's COMPANION parts in the recovery sweep: its webcam track
//! (F-22) and its per-input stems (Task 53, F24) -- promoting a stale one
//! beside the capture it belongs to, and listing a promoted stem in that
//! capture's sidecar. Split from `screen_recovery/mod.rs` near its 800-line
//! cap; the sweep itself (`sweep_staging_dir`) decides what each entry is,
//! decides every capture's own part first, then promotes the companions and
//! lists the stems it promoted once the pass is done.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::{Duration, SystemTime};

use vault_buddy_screen::staging;

use super::{is_stale_at, sniff_part, Entry, Found, RecoveryAction, Sniff, Sweep};
use crate::editor::redact::{redact_name, redact_path};

/// The key a recovered capture's sidecar records its ORIGINAL base under
/// (fix round 1 of hardening Task 8), in `StagedSidecar::extra` beside
/// `recovered`. It is only ever COMPARED with a base the sweep classified,
/// never joined into a path, and only read from a sidecar that is itself
/// `recovered` -- so a hand-written one can neither steer a file nor claim
/// another capture's companions.
pub(super) const RECOVERED_FROM: &str = "recoveredFrom";

/// Stamp where a recovered capture came from, so a companion decided in a
/// LATER pass can still find the ` (N)` it landed on (`Owners::resolve`).
pub(super) fn stamp_recovered_from(sidecar: &mut staging::StagedSidecar, original: &str) {
    sidecar.extra.insert(
        RECOVERED_FROM.to_string(),
        serde_json::Value::String(original.to_string()),
    );
}

/// What this pass made of a capture's own part, which is where its
/// companions go (GAP-198 path 4, GAP-201 item 2).
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PartFate {
    /// Promoted under this base: `<base>` itself, or `<base> (N)` when
    /// `<base>.mp4` was taken.
    Landed(String),
    /// Still on disk and undecided -- not yet stale, waiting for its own
    /// fresh companions, unreadable, or its promotion failed. Its companions
    /// wait for the pass that decides it.
    Waiting,
    /// Held no footage while `<base>.mp4` or `<base>.json` belongs to another
    /// capture: KEPT as the marker that its companions are not that
    /// capture's, and they are left where they are (fix round 1, M-1).
    Held,
    /// Held no footage, and nothing else answers to `<base>`: deleted. Its
    /// companions keep the base they were written under (GAP-198 path 2).
    Gone,
}

/// Where a companion goes.
enum Resolve {
    Wait,
    Leave,
    To(String),
}

/// Every capture's own part and its companions, decided as ONE group per
/// base (GAP-198 path 4, GAP-201 item 2).
pub(super) struct Owners<'a> {
    dir: &'a Path,
    found: &'a [Found],
    fates: HashMap<&'a str, PartFate>,
    /// Bases with a companion part that is not yet stale: their own part
    /// waits too, or it would be promoted to a ` (N)` its companions might
    /// never learn of (fix round 1, I-1).
    held_back: HashSet<&'a str>,
    /// Bases with any companion part at all.
    with_companions: HashSet<&'a str>,
}

impl<'a> Owners<'a> {
    pub(super) fn new(
        dir: &'a Path,
        found: &'a [Found],
        now: SystemTime,
        stale_after: Duration,
    ) -> Self {
        let mut held_back = HashSet::new();
        let mut with_companions = HashSet::new();
        for f in found {
            if let Entry::WebcamPart(b) | Entry::StemPart(b, _) = &f.entry {
                with_companions.insert(b.as_str());
                if !is_stale_at(f.modified, now, stale_after) {
                    held_back.insert(b.as_str());
                }
            }
        }
        Self {
            dir,
            found,
            fates: HashMap::new(),
            held_back,
            with_companions,
        }
    }

    /// Does this capture's (stale) part wait for its own companions?
    pub(super) fn part_waits(&self, base: &str) -> bool {
        self.held_back.contains(base)
    }

    /// Is an EMPTY part under `base` the only thing keeping its companions
    /// off another capture's name? No-follow: a link wearing the name counts
    /// as taken.
    pub(super) fn keeps_empty_part(&self, base: &str) -> bool {
        let taken = |name: String| std::fs::symlink_metadata(self.dir.join(name)).is_ok();
        self.with_companions.contains(base)
            && (taken(staging::mp4_file_name(base)) || taken(staging::sidecar_file_name(base)))
    }

    pub(super) fn record(&mut self, base: &'a str, fate: PartFate) {
        self.fates.insert(base, fate);
    }

    fn resolve(&self, base: &str) -> Resolve {
        match self.fates.get(base) {
            Some(PartFate::Landed(landed)) => Resolve::To(landed.clone()),
            Some(PartFate::Waiting) => Resolve::Wait,
            Some(PartFate::Held) => Resolve::Leave,
            Some(PartFate::Gone) => Resolve::To(base.to_string()),
            None => self.landed_earlier(base),
        }
    }

    /// No part under `base` this pass: was one promoted in an EARLIER pass?
    /// Only a staged capture (its `.mp4` in this pass's snapshot) whose own
    /// sidecar is `recovered` and names `base` as `recoveredFrom` answers;
    /// two that do are ambiguous and the companion is left in place. None:
    /// the companion keeps its own base (a capture published normally whose
    /// webcam part was retained, GAP-200 item 5).
    fn landed_earlier(&self, base: &str) -> Resolve {
        let suffixed = format!("{base} (");
        let staged: HashSet<&str> = self
            .found
            .iter()
            .filter_map(|f| match &f.entry {
                Entry::Staged(b) => Some(b.as_str()),
                _ => None,
            })
            .collect();
        let claims = |s: &&str| {
            staging::read_sidecar(&self.dir.join(staging::sidecar_file_name(s))).is_some_and(
                |read| {
                    read.extra.get("recovered") == Some(&serde_json::Value::Bool(true))
                        && read.extra.get(RECOVERED_FROM).and_then(|v| v.as_str()) == Some(base)
                },
            )
        };
        let owners: Vec<&str> = self
            .found
            .iter()
            .filter_map(|f| match &f.entry {
                Entry::Sidecar(s) if s == base || s.starts_with(&suffixed) => Some(s.as_str()),
                _ => None,
            })
            .filter(|s| staged.contains(s))
            .filter(claims)
            .collect();
        match owners.as_slice() {
            [] => Resolve::To(base.to_string()),
            [one] => Resolve::To(one.to_string()),
            _ => {
                log::warn!(
                    "screen-recovery: {} recovered captures claim {}; leaving its companion",
                    owners.len(),
                    redact_name(base)
                );
                Resolve::Leave
            }
        }
    }
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

/// A stem index written the way this app writes one (`u32`'s own text), the
/// only shape `staging_files::capture_file_names` will ever own.
fn canonical_index(index: &str) -> bool {
    index.parse::<u32>().is_ok_and(|n| n.to_string() == index)
}

/// A webcam or stem part (F-22, F24) is promoted to its OWN published name
/// under the base its capture's part LANDED on (`Owners::resolve`):
/// `.<base>.webcam.mp4.part` beside a capture promoted to `<base> (2)`
/// becomes `<base> (2).webcam.mp4`, which that capture owns
/// (`staging_files::capture_file_names`; a stem once `list_recovered_stem`
/// has added it to the sidecar). It is never given a free name of its own:
/// the name IS its link to the capture. A taken name, a `Held` capture's
/// companion and a stem whose index this app would not write are left
/// alone and not counted pending: no later pass would answer differently.
/// A companion whose capture is `Waiting` is left and counted pending. The
/// base it landed under when promoted.
pub(super) fn promote_or_delete_companion_part(
    f: &Found,
    base: &str,
    owners: &Owners,
    sweep: &mut Sweep,
) -> Option<String> {
    if let Entry::StemPart(_, index) = &f.entry {
        if !canonical_index(index) {
            log::warn!(
                "screen-recovery: a stem part of {} has an index this app never writes; left as it is",
                redact_name(base)
            );
            return None;
        }
    }
    let owner = match owners.resolve(base) {
        Resolve::Wait => {
            sweep.pending += 1;
            return None;
        }
        Resolve::Leave => return None,
        Resolve::To(owner) => owner,
    };
    // The part's published name, `<base><suffix>`, with the suffix (the
    // webcam infix, or the stem's `.stem-<n>.m4a`) kept and the base swapped
    // for the owner's. `classify` took `base` from this very name, so the
    // strip cannot fail.
    let suffix = f
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix('.'))
        .and_then(|n| n.strip_suffix(".part"))
        .and_then(|n| n.strip_prefix(base));
    debug_assert!(suffix.is_some(), "classify took the base from this name");
    let Some(suffix) = suffix else {
        log::warn!(
            "screen-recovery: a companion part of {} does not carry its base; left as it is",
            redact_name(base)
        );
        return None;
    };
    let to = f.path.with_file_name(format!("{owner}{suffix}"));
    if sniff_part(f, false, sweep) != Sniff::Footage {
        return None;
    }
    match vault_buddy_core::capture_paths::rename_noreplace(&f.path, &to) {
        Ok(()) => {
            log::info!("screen-recovery: recovered {}", redact_path(&to));
            sweep.actions.push(RecoveryAction::Promoted(to));
            Some(owner)
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
        assert_eq!(first.pending, 3, "all three come back next pass: {first:?}");
        assert!(d.join(staging::webcam_part_file_name(BASE)).is_file());

        stale(d);

        assert!(d.join(staging::mp4_file_name(BASE)).is_file());
        assert!(d.join(staging::webcam_file_name(BASE)).is_file());
        assert_eq!(listed(d, BASE), [(1, staging::stem_file_name(BASE, 1))]);
    }

    /// Backdate a file's mtime by `secs`, so a sweep at "now" judges it stale.
    fn backdate(path: &Path, now: SystemTime, secs: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(now - Duration::from_secs(secs)).unwrap();
    }

    /// The OTHER capture under `BASE`: footage and a (non-recovered) sidecar.
    fn other_capture(d: &Path) {
        std::fs::write(d.join(staging::mp4_file_name(BASE)), b"an earlier capture").unwrap();
        let mut other = minimal_sidecar(BASE, SystemTime::now());
        other.extra.clear();
        let json = staging::write_sidecar(d, BASE, &other).unwrap();
        // Long settled, so a sweep at "now" judges only the parts.
        backdate(&json, SystemTime::now(), 3600);
        backdate(
            &d.join(staging::mp4_file_name(BASE)),
            SystemTime::now(),
            3600,
        );
    }

    // Fix round 1, I-1: the REVERSE timing. The capture's own part is stale
    // but its webcam and stem parts are still fresh. Deciding the part alone
    // would promote it to `<base> (2)` and leave the companions to a later
    // pass that no longer knows where it went -- which then put them beside
    // the OTHER capture. A capture's parts are decided as a group: the part
    // waits for its companions.
    #[test]
    fn a_captures_part_waits_for_its_fresh_companions() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let now = SystemTime::now();
        other_capture(d);
        let part = d.join(staging::part_file_name(BASE));
        std::fs::write(&part, footage()).unwrap();
        backdate(&part, now, 3600);
        std::fs::write(d.join(staging::webcam_part_file_name(BASE)), footage()).unwrap();
        std::fs::write(d.join(staging::stem_part_file_name(BASE, 1)), footage()).unwrap();

        let first = sweep_staging_dir(d, now, Duration::from_secs(60));

        assert!(first.actions.is_empty(), "{first:?}");
        assert_eq!(first.pending, 3, "all three come back next pass: {first:?}");
        assert!(part.is_file(), "the part waited for its companions");

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
        assert!(listed(d, BASE).is_empty(), "listed in the other capture");
        assert!(!d.join(staging::webcam_file_name(BASE)).exists());
        assert!(!d.join(staging::stem_file_name(BASE, 1)).exists());
    }

    // Fix round 1, I-1 (b): a companion whose capture was promoted in an
    // EARLIER pass (it was unreadable then, or appeared later) finds where
    // that capture landed through the `recoveredFrom` stamp in the recovered
    // capture's own sidecar. Only a sidecar that is itself `recovered` counts:
    // a hand-written `recoveredFrom` on any other sidecar is ignored.
    #[test]
    fn a_companion_finds_where_its_capture_landed_in_an_earlier_pass() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        other_capture(d);
        std::fs::write(d.join(staging::part_file_name(BASE)), footage()).unwrap();
        stale(d);
        let landed = format!("{BASE} (2)");
        let read = staging::read_sidecar(&d.join(staging::sidecar_file_name(&landed))).unwrap();
        assert_eq!(
            read.extra.get("recoveredFrom"),
            Some(&serde_json::Value::String(BASE.into()))
        );
        // A spoof: claims the base, but is not a recovered capture.
        let spoof = format!("{BASE} (3)");
        std::fs::write(d.join(staging::mp4_file_name(&spoof)), footage()).unwrap();
        let mut forged = minimal_sidecar(&spoof, SystemTime::now());
        forged.extra.clear();
        forged.extra.insert(
            "recoveredFrom".into(),
            serde_json::Value::String(BASE.into()),
        );
        staging::write_sidecar(d, &spoof, &forged).unwrap();
        // A recovered capture's sidecar whose footage is gone: an orphan,
        // never an owner.
        let gone = format!("{BASE} (4)");
        let mut orphan = minimal_sidecar(&gone, SystemTime::now());
        stamp_recovered_from(&mut orphan, BASE);
        staging::write_sidecar(d, &gone, &orphan).unwrap();
        // The companions turn up now.
        std::fs::write(d.join(staging::webcam_part_file_name(BASE)), footage()).unwrap();
        std::fs::write(d.join(staging::stem_part_file_name(BASE, 1)), footage()).unwrap();

        let sweep = stale(d);

        assert!(
            d.join(staging::webcam_file_name(&landed)).is_file(),
            "{sweep:?}"
        );
        assert!(d.join(staging::stem_file_name(&landed, 1)).is_file());
        assert_eq!(
            listed(d, &landed),
            [(1, staging::stem_file_name(&landed, 1))]
        );
        assert!(listed(d, BASE).is_empty(), "listed in the other capture");
        assert!(listed(d, &spoof).is_empty(), "listed in the forged capture");
        assert!(!d.join(staging::webcam_file_name(BASE)).exists());
    }

    // Fix round 1, M-1: a capture whose own part held no footage, while
    // `<base>.mp4` belongs to a DIFFERENT capture. Deleting that empty part
    // would leave its companions with nothing to say they are not the other
    // capture's, and the next pass would attach them there. The empty part
    // is kept as that marker, and its companions are left where they are --
    // in every pass, not only the first.
    #[test]
    fn an_empty_part_keeps_its_companions_off_another_capture() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        other_capture(d);
        let part = d.join(staging::part_file_name(BASE));
        std::fs::write(&part, header_only()).unwrap();
        let webcam_part = d.join(staging::webcam_part_file_name(BASE));
        std::fs::write(&webcam_part, footage()).unwrap();

        let first = stale(d);
        let second = stale(d);

        assert!(part.is_file(), "the marker part was deleted: {first:?}");
        assert!(webcam_part.is_file(), "the webcam part was moved");
        assert!(!d.join(staging::webcam_file_name(BASE)).exists());
        assert!(first.actions.is_empty() && second.actions.is_empty());
        assert_eq!(second.pending, 0, "no later pass would decide otherwise");
    }

    // Fix round 1, M-4: a stem part whose index is not written the way this
    // app writes one (`stem-01`) would be promoted under that text but
    // listed as `stem-1`, and so owned by nothing. It is left as it is.
    #[test]
    fn a_stem_part_with_a_non_canonical_index_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join(staging::part_file_name(BASE)), footage()).unwrap();
        let odd = d.join(format!(".{BASE}.stem-01.m4a.part"));
        std::fs::write(&odd, footage()).unwrap();

        let sweep = stale(d);

        assert!(
            d.join(staging::mp4_file_name(BASE)).is_file(),
            "POSITIVE CONTROL: {sweep:?}"
        );
        assert!(odd.is_file(), "the odd stem part was moved");
        assert!(!d.join(format!("{BASE}.stem-01.m4a")).exists());
        assert!(listed(d, BASE).is_empty());
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
