//! Tests for `store_sweep` (hardening Task 7; GAP-174 item 2, GAP-189,
//! GAP-191, review finding D-4). Every removal rule is pinned with its KEEP
//! twin: a fresh file, a link wearing the name (or standing in for the
//! folder it lives in), a recorded product, an unreadable ledger.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::*;
use crate::editor::project_store::{minimal_project, project_dir, store_dir};
use crate::editor::render_jobs::write_ledger;
use crate::editor::store_io::{create_project, list_projects};
use crate::editor::EditorState;
use vault_buddy_core::editor::new_product;
use vault_buddy_core::sync_util::lock_ignoring_poison;

const PROJECT: &str = "proj1";

/// Two hours on: past the hour every age-gated rule waits for.
fn later(now: SystemTime) -> SystemTime {
    now + Duration::from_secs(2 * 60 * 60)
}

/// `proj1`'s folder with a real `project.json`, and its `sub` folder.
fn project_sub(root: &Path, sub: &str) -> PathBuf {
    if project_dir(root, PROJECT).is_some_and(|d| !d.exists()) {
        create_project(root, &minimal_project(PROJECT), &BTreeMap::new()).unwrap();
    }
    let dir = project_dir(root, PROJECT).unwrap().join(sub);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path) {
    std::fs::write(path, b"leftover").unwrap();
}

/// A directory link: a symlink on Unix, an NTFS junction on Windows (no
/// privilege needed; Rust reports it as a symlink). `false` where neither
/// could be made.
fn dir_link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link).is_ok();
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .output()
        .is_ok_and(|o| o.status.success());
    made
}

/// A file symlink, or `false` where this host cannot make one (Windows
/// without Developer Mode or elevation).
fn file_link(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(target, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(target, link);
    made.is_ok()
}

// GAP-174 item 2: a crash mid-copy leaves `media\.<assetId>.<ext>.part`.
#[test]
fn a_stale_media_part_is_removed_and_a_fresh_one_or_a_stranger_kept() {
    let root = tempfile::tempdir().unwrap();
    let media = project_sub(root.path(), "media");
    let part = media.join(".asset-a1.mp4.part");
    write(&part);
    let keep = [
        "asset-a1.mp4",         // the landed copy itself
        ".asset-a1.part",       // no extension: not a name the import mints
        ".bad!id.mp4.part",     // not a valid asset id
        ".asset-a1.mp4.part.x", // not a part
    ];
    for name in keep {
        write(&media.join(name));
    }

    let now = SystemTime::now();
    let fresh = sweep_project_leftovers(root.path(), now);
    assert_eq!(fresh.removed, 0, "a part an hour younger is left alone");
    assert!(part.is_file());

    let report = sweep_project_leftovers(root.path(), later(now));
    assert_eq!(report.removed, 1);
    assert!(!part.exists());
    for name in keep {
        assert!(media.join(name).is_file(), "{name} is not a leftover part");
    }
}

#[test]
fn a_link_wearing_a_part_name_or_standing_in_for_media_is_kept() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let precious = outside.path().join(".asset-a1.mp4.part");
    write(&precious);
    // `media` itself is a link to a folder this app never made.
    let project = project_sub(root.path(), "cache")
        .parent()
        .unwrap()
        .to_path_buf();
    if !dir_link(outside.path(), &project.join("media")) {
        eprintln!("SKIP: this host cannot create a directory link");
        return;
    }
    sweep_project_leftovers(root.path(), later(SystemTime::now()));
    assert!(
        precious.is_file(),
        "the sweep walked through a linked media folder"
    );

    let other = tempfile::tempdir().unwrap();
    let media = project_sub(other.path(), "media");
    let link = media.join(".asset-b2.mp4.part");
    if !file_link(&precious, &link) {
        eprintln!("SKIP: this host cannot create a file symlink");
        return;
    }
    sweep_project_leftovers(other.path(), later(SystemTime::now()));
    assert!(
        std::fs::symlink_metadata(&link).is_ok(),
        "a link is never ours"
    );
    assert!(precious.is_file());
}

// GAP-189: a crash mid-render leaves `jobs\<jobId>\` behind.
#[test]
fn a_stale_job_directory_is_removed_and_a_fresh_one_kept() {
    let root = tempfile::tempdir().unwrap();
    let jobs = project_sub(root.path(), "jobs");
    let job = jobs.join("job-1");
    std::fs::create_dir(&job).unwrap();
    write(&job.join("out.mp4.part"));
    write(&job.join("cues.ass"));
    let stranger = jobs.join("bad!id");
    std::fs::create_dir(&stranger).unwrap();
    write(&jobs.join("job-2")); // a FILE wearing a job's name

    let now = SystemTime::now();
    assert_eq!(sweep_project_leftovers(root.path(), now).removed, 0);
    assert!(job.join("out.mp4.part").is_file());

    assert_eq!(sweep_project_leftovers(root.path(), later(now)).removed, 1);
    assert!(!job.exists());
    assert!(stranger.is_dir());
    assert!(jobs.join("job-2").is_file());
}

// GAP-192: an interrupted publish's journal is REPORTED on every start and
// never deleted -- it is the only record of a partial vault write.
#[test]
fn a_job_directory_holding_a_publish_journal_is_kept() {
    let root = tempfile::tempdir().unwrap();
    let jobs = project_sub(root.path(), "jobs");
    let job = jobs.join("job-pub");
    std::fs::create_dir(&job).unwrap();
    write(&job.join("publish.json"));
    sweep_project_leftovers(root.path(), later(SystemTime::now()));
    assert!(job.join("publish.json").is_file());
}

#[test]
fn a_linked_job_directory_is_kept_with_everything_behind_it() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("keep.txt"));
    let jobs = project_sub(root.path(), "jobs");
    if !dir_link(outside.path(), &jobs.join("job-1")) {
        eprintln!("SKIP: this host cannot create a directory link");
        return;
    }
    let report = sweep_project_leftovers(root.path(), later(SystemTime::now()));
    assert_eq!(report.removed, 0);
    assert!(outside.path().join("keep.txt").is_file());
    assert!(std::fs::symlink_metadata(jobs.join("job-1")).is_ok());
}

fn ledger_with(root: &Path, product_id: &str) {
    let product = new_product(
        &minimal_project(PROJECT),
        1,
        product_id,
        "p",
        &format!("{product_id}.mp4"),
        10,
        None,
        "t",
    );
    write_ledger(root, PROJECT, &[product]).unwrap();
}

// GAP-189: a crash between the move into `products\` and the ledger commit
// leaves an unrecorded product. The ledger is the authority.
#[test]
fn an_unrecorded_product_is_removed_and_a_recorded_one_kept() {
    let root = tempfile::tempdir().unwrap();
    let products = project_sub(root.path(), "products");
    ledger_with(root.path(), "prod-a");
    write(&products.join("prod-a.mp4"));
    write(&products.join("prod-b.mp4"));
    write(&products.join("not a product.mp4"));

    let now = SystemTime::now();
    assert_eq!(sweep_project_leftovers(root.path(), now).removed, 0);
    assert!(
        products.join("prod-b.mp4").is_file(),
        "a fresh file is kept"
    );

    assert_eq!(sweep_project_leftovers(root.path(), later(now)).removed, 1);
    assert!(!products.join("prod-b.mp4").exists());
    assert!(products.join("prod-a.mp4").is_file(), "a recorded product");
    assert!(products.join("not a product.mp4").is_file());
}

#[test]
fn nothing_in_products_is_removed_when_the_ledger_cannot_be_read() {
    let root = tempfile::tempdir().unwrap();
    let products = project_sub(root.path(), "products");
    std::fs::write(
        project_dir(root.path(), PROJECT)
            .unwrap()
            .join("products.json"),
        b"not json",
    )
    .unwrap();
    write(&products.join("prod-b.mp4"));
    let report = sweep_project_leftovers(root.path(), later(SystemTime::now()));
    assert_eq!(report.removed, 0);
    assert!(report.kept >= 1);
    assert!(products.join("prod-b.mp4").is_file());
}

// GAP-191: a Review render left by a quit or a crash goes at the next start.
#[test]
fn every_review_render_is_removed_and_nothing_else_in_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let cache = project_sub(root.path(), "cache");
    let review = cache.join("review-job-1.mp4");
    write(&review);
    let keep = ["review-not an id.mp4", "review-.mp4", "asset-a1-1000.jpg"];
    for name in keep {
        write(&cache.join(name));
    }
    let outside = tempfile::tempdir().unwrap();
    let linked = dir_link(outside.path(), &cache.join("review-job-2.mp4"));

    // A review is never in use at startup: no age is waited for.
    let report = sweep_project_leftovers(root.path(), SystemTime::now());
    assert_eq!(report.removed, 1);
    assert!(!review.exists());
    for name in keep {
        assert!(cache.join(name).is_file(), "{name} is not a review render");
    }
    if linked {
        assert!(std::fs::symlink_metadata(cache.join("review-job-2.mp4")).is_ok());
    }
}

// D-4: a crash between `create_dir` and `project.json` used to leave a
// project folder with only `sources.json` -- the build now happens in
// `.<id>.creating`, which the sweep clears once it is an hour old.
#[test]
fn a_stale_half_created_project_is_removed_and_a_fresh_one_kept() {
    let root = tempfile::tempdir().unwrap();
    let store = store_dir(root.path());
    let creating = store.join(".proj9.creating");
    std::fs::create_dir_all(&creating).unwrap();
    std::fs::write(creating.join("sources.json"), b"{}").unwrap();
    std::fs::create_dir(store.join(".bad!id.creating")).unwrap();
    write(&store.join(".proj8.creating"));
    assert!(list_projects(root.path()).is_empty());

    let now = SystemTime::now();
    assert_eq!(sweep_project_leftovers(root.path(), now).removed, 0);
    assert!(creating.is_dir());

    assert_eq!(sweep_project_leftovers(root.path(), later(now)).removed, 1);
    assert!(!creating.exists());
    assert!(store.join(".bad!id.creating").is_dir());
    assert!(store.join(".proj8.creating").is_file());
}

// The sweep assumes nothing in the store is in use. It runs under `open`
// at startup, before any session can exist; should one exist anyway it
// skips the whole pass rather than race a live project's own files.
#[test]
fn the_startup_sweep_skips_while_any_session_is_open() {
    let root = tempfile::tempdir().unwrap();
    let cache = project_sub(root.path(), "cache");
    let review = cache.join("review-job-1.mp4");
    write(&review);
    let state = EditorState::default();
    lock_ignoring_poison(&state.by_project).insert(PROJECT.into(), "s1".into());
    assert!(sweep_at_startup(&state, root.path(), SystemTime::now()).is_none());
    assert!(review.is_file());

    lock_ignoring_poison(&state.by_project).clear();
    let report = sweep_at_startup(&state, root.path(), SystemTime::now());
    assert_eq!(report.map(|r| r.removed), Some(1));
}
