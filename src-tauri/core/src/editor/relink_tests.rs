//! `relink.rs`'s tests. Every fixture is asymmetric (distinct sizes,
//! durations and names), so a rule that compared the wrong field, or the
//! right field in the wrong direction, fails.

use serde_json::json;

use super::*;
use crate::editor::session::EditorSnapshot;
use crate::editor::test_support::minimal_project;

fn expected(sha256: Option<&str>, size: u64, duration_ms: u64) -> ExpectedSource {
    ExpectedSource {
        sha256: sha256.map(str::to_string),
        size,
        duration_ms,
        kind: ImportKind::Video,
    }
}

fn candidate(name: &str, sha256: &str, size: u64, duration_ms: u64) -> CandidateFacts {
    CandidateFacts {
        name: name.to_string(),
        sha256: sha256.to_string(),
        size,
        duration_ms,
        kind: ImportKind::Video,
    }
}

// Named case. The file that carries the original's NAME is a re-export
// with the same size and length; the renamed file carries its exact bytes.
// A name- or shape-based rule would pick the first (or call it ambiguous);
// the hash picks the second, and the look-alike is not even a contender.
#[test]
fn hash_match_beats_name() {
    let source = vec![(
        "a-intro".to_string(),
        expected(Some("ab12"), 4_200_000, 31_000),
    )];
    let files = [
        candidate("intro.mp4", "ffff", 4_200_000, 31_000),
        candidate("take-2 (final).mp4", "AB12", 3_900_000, 30_900),
    ];
    let report = match_candidates(&source, &files);
    assert_eq!(report.matched, vec![("a-intro".to_string(), 1)]);
    assert!(report.ambiguous.is_empty(), "{report:?}");
    assert!(report.unmatched.is_empty(), "{report:?}");
    assert!(report.mismatched.is_empty(), "{report:?}");
}

// Named case (A19): with no hash to go on, two files of the same size and
// length (and kind) are equally good. Neither is chosen, and the source is
// reported ambiguous — never matched, never mismatched.
#[test]
fn same_size_same_duration_twice_is_ambiguous_and_unselected() {
    let source = vec![("a-demo".to_string(), expected(None, 7_340_000, 62_500))];
    let files = [
        candidate("demo.mp4", "1111", 7_340_000, 62_480),
        candidate("unrelated.mp4", "2222", 7_340_001, 62_500),
        candidate("demo (copy).mp4", "3333", 7_340_000, 62_530),
    ];
    let report = match_candidates(&source, &files);
    assert!(report.matched.is_empty(), "never auto-selected: {report:?}");
    assert_eq!(report.ambiguous, vec![("a-demo".to_string(), vec![0, 2])]);
    assert!(report.unmatched.is_empty());
    assert!(report.mismatched.is_empty());
}

// Named case: the one file picked for the one missing source is shorter.
// It is reported against that source with the reason, chosen file first.
#[test]
fn different_duration_is_mismatched_with_a_reason() {
    let source = vec![("a-talk".to_string(), expected(None, 5_000_000, 31_000))];
    let files = [candidate("talk.mp4", "9999", 5_000_000, 12_400)];
    let report = match_candidates(&source, &files);
    assert!(report.matched.is_empty());
    assert!(report.ambiguous.is_empty());
    assert!(report.unmatched.is_empty());
    assert_eq!(
        report.mismatched,
        vec![(
            "a-talk".to_string(),
            0,
            "different duration: 12.4 s vs 31.0 s".to_string()
        )]
    );
}

#[test]
fn a_duration_inside_the_tolerance_still_matches() {
    let source = vec![("a".to_string(), expected(None, 900, 10_000))];
    let within = [candidate("x.mp4", "1", 900, 10_050)];
    assert_eq!(match_candidates(&source, &within).matched.len(), 1);
    let outside = [candidate("x.mp4", "1", 900, 10_051)];
    assert_eq!(match_candidates(&source, &outside).mismatched.len(), 1);
}

#[test]
fn without_a_hash_the_kind_and_the_size_must_agree_too() {
    let source = vec![("a".to_string(), expected(None, 900, 10_000))];
    let mut audio = candidate("x.m4a", "1", 900, 10_000);
    audio.kind = ImportKind::Audio;
    let report = match_candidates(&source, &[audio]);
    assert_eq!(report.mismatched[0].2, "different kind: audio vs video");

    let bigger = candidate("x.mp4", "1", 2_500_000, 10_000);
    let report = match_candidates(&source, &[bigger]);
    assert_eq!(report.mismatched[0].2, "different size: 2.5 MB vs 0.0 MB");
}

// A hashed source is its bytes: the same size and length is not enough.
#[test]
fn a_hashed_source_rejects_a_look_alike_as_different_content() {
    let source = vec![("a".to_string(), expected(Some("aa"), 900, 10_000))];
    let report = match_candidates(&source, &[candidate("x.mp4", "bb", 900, 10_000)]);
    assert!(report.matched.is_empty());
    assert_eq!(
        report.mismatched[0].2,
        "different content: the file's fingerprint does not match the original's"
    );
    // Equal megabytes, unequal bytes: the exact counts are named.
    let report = match_candidates(&source, &[candidate("x.mp4", "bb", 901, 10_000)]);
    assert_eq!(
        report.mismatched[0].2,
        "different size: 901 bytes vs 900 bytes"
    );
}

// With two sources left and stray files, no pairing is honest: both are
// unmatched and no stray is blamed on either.
#[test]
fn several_unresolved_sources_are_unmatched_not_paired_by_guess() {
    let sources = vec![
        ("a".to_string(), expected(None, 100, 1_000)),
        ("b".to_string(), expected(None, 200, 2_000)),
        ("c".to_string(), expected(Some("cc"), 300, 3_000)),
    ];
    let files = [
        candidate("c.mp4", "CC", 1, 1),
        candidate("stray.mp4", "zz", 999, 9_999),
    ];
    let report = match_candidates(&sources, &files);
    assert_eq!(report.matched, vec![("c".to_string(), 0)]);
    assert_eq!(report.unmatched, vec!["a".to_string(), "b".to_string()]);
    assert!(report.mismatched.is_empty());
}

#[test]
fn a_lone_unresolved_source_with_no_stray_file_is_unmatched() {
    let sources = vec![
        ("a".to_string(), expected(None, 100, 1_000)),
        ("b".to_string(), expected(None, 200, 2_000)),
    ];
    let report = match_candidates(&sources, &[candidate("a.mp4", "1", 100, 1_000)]);
    assert_eq!(report.matched, vec![("a".to_string(), 0)]);
    assert_eq!(report.unmatched, vec!["b".to_string()]);
    assert!(report.mismatched.is_empty());
}

// The wire reply, pinned as LITERAL JSON (never re-serialized against
// itself): camelCase envelope, every list present even when empty.
#[test]
fn relink_report_dto_wire_shape_is_pinned() {
    let snapshot = EditorSnapshot {
        session_id: "ses-1".into(),
        project_id: "project".into(),
        revision: 4,
        persisted_revision: Some(3),
        title: "Project".into(),
        duration_ms: 0,
        can_undo: false,
        can_redo: false,
        undo_label: None,
        redo_label: None,
    };
    let dto = RelinkReportDto {
        projection: EditorProjection {
            snapshot,
            project: minimal_project(),
        },
        missing: vec![MissingMedia {
            asset_id: "b".into(),
            name: "b.mp4".into(),
            expected_size: 20,
            expected_duration_ms: 2_000,
        }],
        matched: vec![RelinkedFile {
            asset_id: "a".into(),
            file: "a.mp4".into(),
        }],
        replaced: vec![],
        ambiguous: vec![AmbiguousSource {
            asset_id: "b".into(),
            files: vec!["b1.mp4".into(), "b2.mp4".into()],
        }],
        unmatched: vec![],
        mismatched: vec![MismatchedFile {
            asset_id: "c".into(),
            file: "c.mp4".into(),
            reason: "different size: 1 bytes vs 2 bytes".into(),
        }],
        per_file: vec![RelinkFileProblem {
            name: "d.mp4".into(),
            error: "damaged".into(),
        }],
    };
    let value = serde_json::to_value(&dto).unwrap();
    assert_eq!(value["projection"]["snapshot"]["revision"], json!(4));
    let mut rest = value.as_object().unwrap().clone();
    rest.remove("projection");
    assert_eq!(
        serde_json::Value::Object(rest),
        json!({
            "missing": [{ "assetId": "b", "name": "b.mp4", "expectedSize": 20, "expectedDurationMs": 2000 }],
            "matched": [{ "assetId": "a", "file": "a.mp4" }],
            "replaced": [],
            "ambiguous": [{ "assetId": "b", "files": ["b1.mp4", "b2.mp4"] }],
            "unmatched": [],
            "mismatched": [{ "assetId": "c", "file": "c.mp4", "reason": "different size: 1 bytes vs 2 bytes" }],
            "perFile": [{ "name": "d.mp4", "error": "damaged" }]
        })
    );
}

#[test]
fn named_report_turns_indices_into_file_names() {
    let files = [
        candidate("zero.mp4", "0", 1, 1),
        candidate("one.mp4", "1", 1, 1),
        candidate("two.mp4", "2", 1, 1),
    ];
    let report = RelinkReport {
        matched: vec![("a".into(), 2)],
        ambiguous: vec![("b".into(), vec![1, 0])],
        unmatched: vec!["c".into()],
        mismatched: vec![("d".into(), 1, "why".into())],
    };
    let named = NamedReport::of(&report, &files);
    assert_eq!(named.matched[0].file, "two.mp4");
    assert_eq!(named.ambiguous[0].files, vec!["one.mp4", "zero.mp4"]);
    assert_eq!(named.unmatched, vec!["c".to_string()]);
    assert_eq!(named.mismatched[0].file, "one.mp4");
    assert_eq!(named.mismatched[0].reason, "why");
}
