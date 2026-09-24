//! A capture's per-input audio STEMS in staging (Task 53, F-05, F24): the
//! names a stem is written and published under, the pattern the recovery
//! sweep owns an orphaned stem part by, and the sidecar's `stems` block.
//!
//! Split from `staging.rs` at its 800-line cap; `staging` re-exports every
//! public item here, so no caller's path changed.
//!
//! **The sidecar's list is untrusted.** It is hand-editable, and whatever
//! `stem_files` returns flows into `staging_files::capture_file_names` — the
//! set a discard DELETES and `resolve_source` SERVES. So `stem_files` returns
//! only the `file` values, and `capture_file_names` admits a name only when it
//! is literally `stem_file_name(base, n)` for this capture's own base.

use serde::{Deserialize, Serialize};

use crate::staging::StagedSidecar;

/// The infix an audio STEM carries between its base and its index.
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

/// One stem in the sidecar: which input it is (1-based, in `inputs` order),
/// the input's device name, and its staging file NAME (never a path).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StemSidecar {
    pub index: u32,
    pub input: String,
    pub file: String,
    /// Keys a newer build adds to an entry, carried through verbatim for the
    /// reason `StagedSidecar::extra` exists.
    #[serde(flatten, default)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl StagedSidecar {
    /// The stem file names the sidecar lists — the `stems` argument every
    /// `staging_files::capture_file_names` caller passes (F24), replacing
    /// Task 51's empty placeholder.
    pub fn stem_files(&self) -> Vec<String> {
        self.stems.iter().map(|s| s.file.clone()).collect()
    }

    /// Set the typed `stems` block, dropping any raw `stems` value the
    /// lenient read parked in `extra` (the `set_webcam` reason: never write
    /// one key twice).
    pub fn set_stems(&mut self, stems: Vec<StemSidecar>) {
        self.extra.remove("stems");
        self.stems = stems;
    }
}

/// Does a raw `stems` value read as this build's block? The lenient sidecar
/// read (`staging::read_sidecar`) parks one that does not in `extra` — a
/// malformed or future-shaped list costs the capture its stems, never the
/// capture itself.
pub(crate) fn stems_block_reads(raw: &serde_json::Value) -> bool {
    serde_json::from_value::<Vec<StemSidecar>>(raw.clone()).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staging::{read_sidecar, write_sidecar};
    use crate::staging_files::capture_file_names;

    const BASE: &str = "2026-09-24 1015 Demo";

    fn sidecar(stems: Vec<StemSidecar>) -> StagedSidecar {
        StagedSidecar {
            base: BASE.into(),
            vault_id: "v1".into(),
            source_title: "Demo".into(),
            source_kind: "screen".into(),
            inputs: vec!["USB Mic".into(), "Speakers".into(), "Line In".into()],
            duration_ms: 12_345,
            width: 1280,
            height: 720,
            recorded_at: "2026-09-24T10:15:00+02:00".into(),
            stems,
            ..Default::default()
        }
    }

    fn stem(index: u32, input: &str) -> StemSidecar {
        StemSidecar {
            index,
            input: input.into(),
            file: stem_file_name(BASE, index),
            extra: serde_json::Map::new(),
        }
    }

    // F24 (closing Task 51's placeholder): with a REAL three-stem sidecar
    // written to disk and read back, the capture owns exactly its video, its
    // sidecar, its export temp, its webcam file and those three stems — not
    // one more, not one fewer.
    #[test]
    fn capture_file_names_enumerates_the_sidecars_actual_stems() {
        let dir = tempfile::tempdir().unwrap();
        let written = sidecar(vec![
            stem(1, "USB Mic"),
            stem(2, "Speakers"),
            stem(3, "Line In"),
        ]);
        write_sidecar(dir.path(), BASE, &written).unwrap();
        let read = read_sidecar(&dir.path().join(format!("{BASE}.json"))).expect("readable");
        assert_eq!(read.stems, written.stems);

        let mut names = capture_file_names(BASE, &read.stem_files());
        names.sort();
        let mut expected = vec![
            format!("{BASE}.mp4"),
            format!("{BASE}.json"),
            format!(".{BASE}.export.mp4.part"),
            format!("{BASE}.webcam.mp4"),
            format!("{BASE}.stem-1.m4a"),
            format!("{BASE}.stem-2.m4a"),
            format!("{BASE}.stem-3.m4a"),
        ];
        expected.sort();
        assert_eq!(names, expected);
    }

    // The list is hand-editable: an entry naming anything but this capture's
    // own stem shape is not owned, so it can be neither deleted nor served.
    #[test]
    fn a_listed_file_outside_this_captures_stem_shape_is_not_owned() {
        const HOSTILE: [&str; 3] = [
            "../../obsidian.json",
            "2026-09-24 1015 Other.stem-1.m4a",
            "notes.md",
        ];
        let mut listed = sidecar(vec![stem(1, "USB Mic")]);
        for file in HOSTILE {
            listed.stems.push(StemSidecar {
                index: 9,
                input: "x".into(),
                file: file.into(),
                extra: serde_json::Map::new(),
            });
        }
        let names = capture_file_names(BASE, &listed.stem_files());
        assert!(names.contains(&stem_file_name(BASE, 1)));
        for file in HOSTILE {
            assert!(
                !names.contains(&file.to_string()),
                "{file} must not be owned"
            );
        }
    }

    // A sidecar with no stems stays byte-identical on a rewrite (no `stems`
    // key), and the wire spelling is pinned against a literal.
    #[test]
    fn the_stems_block_is_skipped_when_empty_and_spelled_in_camel_case() {
        let plain = serde_json::to_value(sidecar(Vec::new())).unwrap();
        assert!(plain.get("stems").is_none(), "{plain}");
        let with = serde_json::to_value(sidecar(vec![stem(2, "Speakers")])).unwrap();
        assert_eq!(
            with["stems"],
            serde_json::json!([{
                "index": 2,
                "input": "Speakers",
                "file": "2026-09-24 1015 Demo.stem-2.m4a"
            }])
        );
    }

    // Per-field defensive read: a malformed `stems` block costs the capture
    // its stems, never the capture — and the raw value survives a rewrite.
    #[test]
    fn a_malformed_stems_block_degrades_to_no_stems_and_is_carried_through() {
        let dir = tempfile::tempdir().unwrap();
        let mut value = serde_json::to_value(sidecar(Vec::new())).unwrap();
        value["stems"] = serde_json::json!([{ "index": "one" }]);
        let path = dir.path().join(format!("{BASE}.json"));
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let read = read_sidecar(&path).expect("the capture is still readable");
        assert!(read.stems.is_empty());
        assert!(read.stem_files().is_empty());
        write_sidecar(dir.path(), BASE, &read).unwrap();
        let back: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(back["stems"], serde_json::json!([{ "index": "one" }]));

        let mut replaced = read;
        replaced.set_stems(vec![stem(1, "USB Mic")]);
        assert!(
            !replaced.extra.contains_key("stems"),
            "set_stems drops the parked value"
        );
    }
}
