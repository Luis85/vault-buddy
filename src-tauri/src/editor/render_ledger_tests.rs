//! `render_jobs.rs`' LEDGER tests (Task 46), split from
//! `render_jobs_tests.rs` for the 800-line cap and sharing its fixtures:
//! what `products.json` holds, what reads it (the product list, a save, a
//! package export) and what it refuses.

use vault_buddy_core::editor::EditorErrorCode;
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::tests::{
    begin, opened, refused, snapshot_revision, Behaviour, FakeRunner, PROJECT, SESSION,
};
use super::*;
use crate::editor::media_jobs::tests::CollectingSink;
use crate::editor::session_commands::{close_in, CloseDisposition};
use crate::editor::store_io::load_project;

/// Render one product through the fake; its id.
fn rendered(state: &EditorState, root: &Path) -> String {
    let (job, runner) = begin(
        state,
        root,
        FakeRunner::new(Behaviour::Writes(b"bytes".to_vec())),
    )
    .unwrap();
    let product_id = job.product_id.clone();
    run_render_job(state, job, &runner, &CollectingSink::default());
    product_id
}

// The 41st product is refused rather than evicting an older one (the
// removal UI is a later product decision -- docs/Gaps.md GAP-187).
#[test]
fn the_forty_first_render_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let project = lock_ignoring_poison(&state.sessions)[SESSION]
        .project()
        .clone();
    let full: Vec<_> = (0..limits::MAX_PRODUCTS)
        .map(|i| {
            let id = format!("prod-{i}");
            new_product(
                &project,
                1,
                &id,
                "old",
                &product_file_name(&id),
                10,
                None,
                "t",
            )
        })
        .collect();
    write_ledger(root.path(), PROJECT, &full).unwrap();
    let e = refused(begin(
        &state,
        root.path(),
        FakeRunner::new(Behaviour::Writes(b"x".to_vec())),
    ));
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(e.message.contains("Remove an older product first"));
    assert_eq!(
        e.message,
        too_many_products().message,
        "one wording for both refusals"
    );
}

// R5's departure from the bundle: the ledger is committed WITH the product,
// not by a later manual save -- close the session unsaved, reopen the
// project from the store, and the product is still listed.
#[test]
fn ledger_survives_without_saving_the_project() {
    let root = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let (job, runner) = begin(
        &state,
        root.path(),
        FakeRunner::new(Behaviour::Writes(b"bytes".to_vec())),
    )
    .unwrap();
    let product_id = job.product_id.clone();
    run_render_job(&state, job, &runner, &CollectingSink::default());
    close_in(
        &state,
        root.path(),
        staging.path(),
        SESSION,
        CloseDisposition::Keep,
    )
    .unwrap();
    let reopened =
        crate::editor::save_commands::open_project_session(&state, root.path(), PROJECT, false)
            .unwrap();
    let listed = products_in(&state, root.path(), &reopened.snapshot.session_id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, product_id);
    assert!(listed[0].available);
}

// A product whose file is gone is still LISTED (its lineage survives,
// DATA-MODEL.md) but reads unavailable; a ledger naming any file but
// `<productId>.mp4` is refused whole rather than trusted.
#[test]
fn products_list_availability_and_refuse_a_foreign_file_name() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let project = lock_ignoring_poison(&state.sessions)[SESSION]
        .project()
        .clone();
    let gone = new_product(
        &project,
        1,
        "prod-gone",
        "g",
        "prod-gone.mp4",
        10,
        None,
        "t",
    );
    write_ledger(root.path(), PROJECT, std::slice::from_ref(&gone)).unwrap();
    let listed = products_in(&state, root.path(), SESSION).unwrap();
    assert!(!listed[0].available);

    let mut foreign = gone;
    foreign.filename = "../project.json".into();
    write_ledger(root.path(), PROJECT, &[foreign]).unwrap();
    let e = products_in(&state, root.path(), SESSION).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
}

// ADR R5: "on save/export, `record.products` is assembled from the ledger"
// -- a save writes the ledger's products into `project.json`, and so does
// a package export's envelope. A damaged ledger refuses the save rather
// than silently writing a project with its products dropped.
#[test]
fn save_and_export_assemble_record_products_from_the_ledger() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let product_id = rendered(&state, root.path());
    let rev = snapshot_revision(&state);
    crate::editor::save_commands::save_project_in(&state, root.path(), SESSION, rev).unwrap();
    let (saved, _) = load_project(root.path(), PROJECT).unwrap();
    let ids: Vec<&str> = saved
        .record
        .products
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(ids, [product_id.as_str()]);
    let ledger = read_ledger(root.path(), PROJECT).unwrap();
    let without_snapshots: Vec<_> = ledger
        .iter()
        .cloned()
        .map(|mut p| {
            p.snapshot = None;
            p
        })
        .collect();
    assert_eq!(saved.record.products, without_snapshots);
    // A package carries the lineage WITH its snapshots (A17's collector
    // reads them); only `project.json` leaves them to the ledger.
    let exported =
        crate::editor::package_commands::export_envelope(&state, root.path(), SESSION, rev)
            .unwrap();
    assert_eq!(exported.record.products, ledger);

    let project_json = std::fs::read(dir.join("project.json")).unwrap();
    std::fs::write(dir.join(PRODUCTS_FILE), b"{ not a ledger").unwrap();
    let e = crate::editor::save_commands::save_project_in(&state, root.path(), SESSION, rev)
        .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
    assert_eq!(
        std::fs::read(dir.join("project.json")).unwrap(),
        project_json
    );
}

/// Replace the live session's project with one carrying `pad` bytes of
/// round-tripping `extra`, at the same revision; the padded project.
fn padded(state: &EditorState, pad: usize) -> vault_buddy_core::editor::Project {
    let mut sessions = lock_ignoring_poison(&state.sessions);
    let session = &sessions[SESSION];
    let revision = session.snapshot().revision;
    let mut project = session.project().clone();
    project
        .extra
        .insert("pad".into(), serde_json::Value::String("x".repeat(pad)));
    sessions.insert(
        SESSION.to_string(),
        vault_buddy_core::editor::EditorSession::resume(SESSION, project.clone(), revision),
    );
    project
}

// Fix round 1 (review Important 1, data loss): `load_project` refuses a
// `project.json` over `MAX_PROJECT_JSON_BYTES`, and every product embeds a
// whole snapshot, so a save that copied the ledger verbatim wrote a file
// that could not be reopened once enough products existed. The schema
// makes `snapshot` optional, so `project.json` carries the products WITHOUT
// their snapshots (the ledger keeps them) -- 30 products whose snapshots
// total ~9 MiB still save into a file that reopens.
#[test]
fn many_products_never_make_a_saved_project_unopenable() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let project = padded(&state, 300_000);
    let products: Vec<_> = (0..30)
        .map(|i| {
            let id = format!("prod-{i}");
            new_product(
                &project,
                1,
                &id,
                "big",
                &product_file_name(&id),
                10,
                None,
                "t",
            )
        })
        .collect();
    write_ledger(root.path(), PROJECT, &products).unwrap();
    let ledger = project_dir(root.path(), PROJECT)
        .unwrap()
        .join(PRODUCTS_FILE);
    assert!(
        std::fs::metadata(ledger).unwrap().len() > limits::MAX_PROJECT_JSON_BYTES,
        "the fixture must exceed the load bound"
    );

    let rev = snapshot_revision(&state);
    crate::editor::save_commands::save_project_in(&state, root.path(), SESSION, rev).unwrap();
    let (saved, _) = load_project(root.path(), PROJECT).expect("the saved project reopens");
    assert_eq!(saved.record.products.len(), 30);
    assert!(saved.record.products.iter().all(|p| p.snapshot.is_none()));
    assert!(read_ledger(root.path(), PROJECT).unwrap()[0]
        .snapshot
        .is_some());
}

// And whatever the cause, a save whose encoded project would exceed the
// load bound is REFUSED with a typed error, leaving the last good
// `project.json` byte-identical -- never written and then unloadable.
#[test]
fn a_save_over_the_load_bound_is_refused_and_leaves_the_last_good_file() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let before = std::fs::read(dir.join("project.json")).unwrap();
    padded(&state, limits::MAX_PROJECT_JSON_BYTES as usize + 1);
    let rev = snapshot_revision(&state);
    let e = crate::editor::save_commands::save_project_in(&state, root.path(), SESSION, rev)
        .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject, "{}", e.message);
    assert!(e.message.contains("too large"), "{}", e.message);
    assert_eq!(std::fs::read(dir.join("project.json")).unwrap(), before);
}

// Fix round 1 (review Minor 5): the ledger refuses a duplicate product id,
// as `validate_envelope` does -- a save would otherwise copy it into a
// `project.json` that load then refuses.
#[test]
fn a_ledger_with_a_duplicate_product_id_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state);
    let project = lock_ignoring_poison(&state.sessions)[SESSION]
        .project()
        .clone();
    let one = new_product(&project, 1, "prod-a", "a", "prod-a.mp4", 10, None, "t");
    write_ledger(root.path(), PROJECT, &[one.clone(), one]).unwrap();
    let e = products_in(&state, root.path(), SESSION).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
}

// Final review I6: the product file is moved into `products\` BEFORE the
// ledger is written (a ledger entry must always have its file). When that
// write fails, the moved file is not a product — nothing lists it, nothing
// can remove it — so it is taken back out, and the ledger is unchanged.
#[test]
fn a_failed_ledger_write_takes_the_unrecorded_product_back_out() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let earlier = rendered(&state, root.path());
    let ledger_before = std::fs::read(dir.join(PRODUCTS_FILE)).unwrap();
    let (job, _runner) = begin(
        &state,
        root.path(),
        FakeRunner::new(Behaviour::Writes(b"x".to_vec())),
    )
    .unwrap();
    let part = root.path().join("rendered.mp4.part");
    std::fs::write(&part, b"rendered bytes").unwrap();
    let full_disk = |_: &Path, _: &str, _: &[Product]| {
        Err(EditorError::new(
            EditorErrorCode::DiskFull,
            "Not enough disk space to record the product.",
        ))
    };

    let e = record_product_with(&job, &part, 1_250, &full_disk).unwrap_err();

    assert_eq!(e.code, EditorErrorCode::DiskFull);
    let dest = dir
        .join(PRODUCTS_DIR)
        .join(product_file_name(&job.product_id));
    assert!(!dest.exists(), "an unrecorded product file was left behind");
    assert!(
        dir.join(PRODUCTS_DIR)
            .join(product_file_name(&earlier))
            .is_file(),
        "only the unrecorded file goes"
    );
    assert_eq!(
        std::fs::read(dir.join(PRODUCTS_FILE)).unwrap(),
        ledger_before,
        "the ledger is unchanged"
    );
}
