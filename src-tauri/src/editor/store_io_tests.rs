//! `store_io.rs`' tests, split out (final whole-branch review) when the
//! retry-safe removal needed room in the production half.

use super::super::project_store::minimal_project;
use super::*;

#[test]
fn create_then_load_round_trips() {
    let root = tempfile::tempdir().unwrap();
    let project = minimal_project("proj1");
    let mut sources = BTreeMap::new();
    sources.insert(
        "src1".to_string(),
        SourceRecord {
            locator: super::super::project_store::SourceLocator::Staging {
                base: "2026-09-20 1432 Demo".to_string(),
            },
            sha256: None,
            size: 1_234,
            duration_ms: 60_000,
            width: Some(1920),
            height: Some(1080),
            has_audio: true,
            has_video: true,
            media_kind: super::super::project_store::SourceMediaKind::Video,
            replaced_from: None,
        },
    );

    create_project(root.path(), &project, &sources).expect("create succeeds");
    let (envelope, back_sources) = load_project(root.path(), "proj1").expect("load succeeds");

    assert_eq!(envelope.project, project);
    assert_eq!(envelope.record.revision, 1);
    assert_eq!(envelope.record.products.len(), 0);
    assert_eq!(back_sources, sources);
}

#[test]
fn create_project_refuses_a_duplicate_id() {
    let root = tempfile::tempdir().unwrap();
    let project = minimal_project("proj1");
    create_project(root.path(), &project, &BTreeMap::new()).unwrap();
    let err = create_project(root.path(), &project, &BTreeMap::new())
        .expect_err("a second create under the same id must be refused");
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn load_refuses_an_oversized_file() {
    // A VALID, well-formed envelope — not `b'x'` garbage, which the
    // earlier version of this test used and which is malformed JSON on
    // its own. That fixture passed for the wrong reason: dropping the
    // size check entirely left the parse failure to refuse it anyway,
    // so the test stayed green under a mutation that deleted the very
    // check it claims to cover. JSON tolerates trailing whitespace, so
    // padding a real envelope past the byte cap keeps it perfectly
    // loadable — except for the size check this test exists to pin.
    let root = tempfile::tempdir().unwrap();
    create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    let dir = project_dir(root.path(), "proj1").unwrap();
    let path = dir.join(PROJECT_FILE);
    let json = std::fs::read_to_string(&path).unwrap();
    let pad = (limits::MAX_PROJECT_JSON_BYTES as usize + 1).saturating_sub(json.len());
    let padded = format!("{json}{}", " ".repeat(pad));
    assert!(padded.len() as u64 > limits::MAX_PROJECT_JSON_BYTES);
    std::fs::write(&path, padded.as_bytes()).unwrap();

    let err = load_project(root.path(), "proj1").expect_err("an oversized file must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert!(
        err.message.contains("byte"),
        "expected a size-specific message, got: {}",
        err.message
    );
}

// A27: a malformed `project.json` must be reported, not "repaired" —
// `load_project` contains no write at all, so the file the user's disk
// holds is exactly what it held before the failed load.
#[test]
fn malformed_project_is_reported_and_left_byte_identical() {
    let root = tempfile::tempdir().unwrap();
    let dir = project_dir(root.path(), "proj1").unwrap();
    std::fs::create_dir_all(&dir).unwrap();
    let before = b"{ not actually json".to_vec();
    std::fs::write(dir.join(PROJECT_FILE), &before).unwrap();

    let err = load_project(root.path(), "proj1").expect_err("malformed JSON must be refused");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    let after = std::fs::read(dir.join(PROJECT_FILE)).unwrap();
    assert_eq!(after, before, "a failed load must never rewrite the file");
}

#[test]
fn commit_project_persists_a_new_revision() {
    let root = tempfile::tempdir().unwrap();
    let project = minimal_project("proj1");
    create_project(root.path(), &project, &BTreeMap::new()).unwrap();
    let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
    envelope.record.revision = 2;
    envelope.record.updated_at = "2026-09-22T00:00:00Z".to_string();

    commit_project(&RealWriter, root.path(), "proj1", &envelope).unwrap();

    let (reloaded, _) = load_project(root.path(), "proj1").unwrap();
    assert_eq!(reloaded.record.revision, 2);
}

#[test]
fn list_projects_reports_every_valid_project_and_skips_what_it_cannot_trust() {
    let root = tempfile::tempdir().unwrap();
    create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    create_project(root.path(), &minimal_project("proj2"), &BTreeMap::new()).unwrap();
    // A directory whose own project.id disagrees with its name.
    let mismatched = project_dir(root.path(), "proj3").unwrap();
    std::fs::create_dir_all(&mismatched).unwrap();
    let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
    envelope.project.id = "somewhere-else".to_string();
    write_json(&mismatched.join(PROJECT_FILE), &envelope).unwrap();

    let rows = list_projects(root.path());
    let ids: Vec<&str> = rows.iter().map(|r| r.project_file_id.as_str()).collect();
    assert!(ids.contains(&"proj1"));
    assert!(ids.contains(&"proj2"));
    assert!(!ids.contains(&"somewhere-else"));
    assert_eq!(rows.len(), 2, "the mismatched directory must be skipped");
}

// The doc says "will not parse OR validate" — this pins the second
// half, which nothing else here exercised: a file that is perfectly
// well-formed JSON matching the envelope shape, but semantically
// invalid (`validate_project`'s own rules), must not be listed either.
#[test]
fn list_projects_skips_a_project_that_parses_but_fails_semantic_validation() {
    let root = tempfile::tempdir().unwrap();
    create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
    envelope.project.master_gain = 5.0; // out of validate_project's [0,1] range
    let dir = project_dir(root.path(), "proj1").unwrap();
    write_json(&dir.join(PROJECT_FILE), &envelope).unwrap();

    let rows = list_projects(root.path());
    assert!(
        rows.is_empty(),
        "an invalid-but-parseable project must not be listed"
    );
}

#[test]
fn remove_project_refuses_a_directory_whose_project_id_differs() {
    let root = tempfile::tempdir().unwrap();
    create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    // Overwrite project.json under a DIFFERENT id than the directory name.
    let (mut envelope, _) = load_project(root.path(), "proj1").unwrap();
    envelope.project.id = "not-proj1".to_string();
    let dir = project_dir(root.path(), "proj1").unwrap();
    write_json(&dir.join(PROJECT_FILE), &envelope).unwrap();

    let err = remove_project(root.path(), "proj1").expect_err("an id mismatch must refuse removal");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert!(dir.is_dir(), "the directory must survive a refused removal");
}

#[test]
fn remove_project_removes_everything_it_created() {
    let root = tempfile::tempdir().unwrap();
    create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    let dir = project_dir(root.path(), "proj1").unwrap();
    std::fs::create_dir_all(dir.join("media")).unwrap();
    std::fs::write(dir.join("media").join("clip.mp4"), b"x").unwrap();

    remove_project(root.path(), "proj1").expect("removal succeeds");

    assert!(!dir.exists());
}

// Unix-only: a Windows symlink needs `SeCreateSymbolicLinkPrivilege`
// (Developer Mode or an elevated process). Skip VISIBLY rather than
// silently passing when this account lacks it, the `981bf67` posture
// `editor::vault_dir`'s own symlink-escape test already uses for
// the exact same reason.
#[test]
fn remove_project_never_follows_a_symlink() {
    let root = tempfile::tempdir().unwrap();
    create_project(root.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    let dir = project_dir(root.path(), "proj1").unwrap();
    std::fs::create_dir_all(dir.join("media")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let precious = outside.path().join("precious.bin");
    std::fs::write(&precious, b"not ours").unwrap();
    let link = dir.join("media").join("linked.bin");

    #[cfg(unix)]
    std::os::unix::fs::symlink(&precious, &link).unwrap();
    #[cfg(not(unix))]
    {
        if let Err(e) = std::os::windows::fs::symlink_file(&precious, &link) {
            if e.raw_os_error() == Some(1314) {
                eprintln!(
                    "SKIP: remove_project_never_follows_a_symlink — symlink_file needs \
                     SeCreateSymbolicLinkPrivilege (Developer Mode or an elevated process); \
                     this account lacks it (OS error 1314)"
                );
                return;
            }
            panic!("symlink_file failed unexpectedly: {e}");
        }
    }

    let err = remove_project(root.path(), "proj1").expect_err("a symlink must refuse removal");
    assert_eq!(err.code, EditorErrorCode::Internal);
    assert!(precious.is_file(), "the removal followed the symlink");
    assert!(
        dir.is_dir(),
        "a refused removal must leave the directory in place"
    );
}

// Finding: `walk_no_follow` only inspects what is INSIDE `dir` — it
// never checked `dir` itself. A REAL project sits outside the store, at
// a path a symlink (or, on Windows, an NTFS junction — reported as a
// symlink by Rust too) stands in for at the expected project location:
// both the ownership read AND `read_dir` would silently follow it, so
// the walk would delete files that were never this app's to remove.
#[test]
fn remove_project_refuses_a_symlinked_project_directory_itself() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    // A real project OUTSIDE the store — with a valid, matching
    // project.json, so if the link were followed the ownership check
    // would actually pass and only the walk's own no-follow discipline
    // would be left to save it.
    create_project(outside.path(), &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    let real_dir = project_dir(outside.path(), "proj1").unwrap();
    let link = project_dir(root.path(), "proj1").unwrap();
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&real_dir, &link).unwrap();
    #[cfg(not(unix))]
    {
        if let Err(e) = std::os::windows::fs::symlink_dir(&real_dir, &link) {
            if e.raw_os_error() == Some(1314) {
                eprintln!(
                    "SKIP: remove_project_refuses_a_symlinked_project_directory_itself — \
                     symlink_dir needs SeCreateSymbolicLinkPrivilege (Developer Mode or an \
                     elevated process); this account lacks it (OS error 1314)"
                );
                return;
            }
            panic!("symlink_dir failed unexpectedly: {e}");
        }
    }

    let err = remove_project(root.path(), "proj1")
        .expect_err("a symlinked project directory must refuse removal");
    assert_eq!(err.code, EditorErrorCode::Internal);
    assert!(
        real_dir.join(PROJECT_FILE).is_file(),
        "the real project was removed through the link"
    );
}

#[test]
fn project_summary_dto_serializes_camel_case_literal() {
    let dto = ProjectSummaryDto {
        project_file_id: "abc123".into(),
        title: "My Tutorial".into(),
        updated_at: "2026-09-21T10:00:00+02:00".into(),
        persisted_revision: 3,
        has_recovery: true,
        source_base: Some("2026-09-20 1432 Demo".into()),
    };
    assert_eq!(
        serde_json::to_value(&dto).unwrap(),
        serde_json::json!({
            "projectFileId": "abc123",
            "title": "My Tutorial",
            "updatedAt": "2026-09-21T10:00:00+02:00",
            "persistedRevision": 3,
            "hasRecovery": true,
            "sourceBase": "2026-09-20 1432 Demo",
        }),
    );
}

#[test]
fn project_summary_dto_carries_a_null_source_base_when_none() {
    let dto = ProjectSummaryDto {
        project_file_id: "abc123".into(),
        title: "Untitled".into(),
        updated_at: "2026-09-21T10:00:00+02:00".into(),
        persisted_revision: 1,
        has_recovery: false,
        source_base: None,
    };
    assert_eq!(
        serde_json::to_value(&dto).unwrap()["sourceBase"],
        serde_json::Value::Null
    );
}

/// A project with content in a sub-folder AND loose top-level files, the
/// shape a real project has once it holds takes, media and a ledger.
fn project_with_content(root: &Path) -> PathBuf {
    create_project(root, &minimal_project("proj1"), &BTreeMap::new()).unwrap();
    let dir = project_dir(root, "proj1").unwrap();
    std::fs::create_dir(dir.join("takes")).unwrap();
    std::fs::write(dir.join("takes").join("take-a.webm"), b"take").unwrap();
    std::fs::write(dir.join("products.json"), b"[]").unwrap();
    dir
}

// Final review I1: a removal that failed part way used to have unlinked
// `project.json` already (`read_dir` order: `products.json`,
// `project.json`, then `takes\`), and every retry then failed its
// ownership check on the missing file — a project nothing could open or
// discard again. Everything else goes first; `project.json` goes last.
#[test]
fn removal_order_keeps_project_json_until_everything_else_is_gone() {
    let root = tempfile::tempdir().unwrap();
    let dir = project_with_content(root.path());
    let order = removal_order(&dir, Some(PROJECT_FILE)).unwrap();
    let position = |p: PathBuf| order.iter().position(|q| *q == p).unwrap();
    let project_json = position(dir.join(PROJECT_FILE));
    for earlier in [
        dir.join("takes").join("take-a.webm"),
        dir.join("takes"),
        dir.join("products.json"),
        dir.join(SOURCES_FILE),
    ] {
        assert!(
            position(earlier.clone()) < project_json,
            "{earlier:?} must go before project.json in {order:?}"
        );
    }
    assert_eq!(order.last(), Some(&dir), "the folder itself goes last");
    assert_eq!(order[order.len() - 2], dir.join(PROJECT_FILE));
}

// The same on a real filesystem: a take file another process holds open
// without delete sharing (ffmpeg's output handle) fails the removal, the
// project stays discardable, and the retry finishes the job.
#[cfg(windows)]
#[test]
fn a_removal_that_fails_part_way_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let dir = project_with_content(root.path());
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(dir.join("takes").join("take-a.webm"))
        .unwrap();
    assert!(remove_project(root.path(), "proj1").is_err());
    assert!(
        dir.join(PROJECT_FILE).is_file(),
        "project.json survives a failed removal"
    );
    drop(held);
    remove_project(root.path(), "proj1").expect("the retry finishes the removal");
    assert!(!dir.exists());
}

// Final review I3: ownership is `project.json`'s own `project.id`, read
// from the JSON document — not from a fully valid project. A project whose
// graph no longer deserializes is exactly the one that must stay
// discardable; bytes that are not JSON at all still prove nothing.
#[test]
fn remove_project_proves_ownership_from_a_damaged_but_readable_project_json() {
    let root = tempfile::tempdir().unwrap();
    let dir = project_with_content(root.path());
    std::fs::write(
        dir.join(PROJECT_FILE),
        br#"{"project":{"id":"proj1","tracks":"not a list"}}"#,
    )
    .unwrap();
    remove_project(root.path(), "proj1").expect("its own id owns the folder");
    assert!(!dir.exists());

    let dir = project_with_content(root.path());
    std::fs::write(dir.join(PROJECT_FILE), b"\x00 not json").unwrap();
    assert_eq!(
        remove_project(root.path(), "proj1").unwrap_err().code,
        EditorErrorCode::InvalidProject
    );
    std::fs::write(
        dir.join(PROJECT_FILE),
        br#"{"project":{"id":"someone-else"}}"#,
    )
    .unwrap();
    assert!(remove_project(root.path(), "proj1").is_err());
    assert!(dir.join(PROJECT_FILE).is_file(), "nothing was removed");
}
