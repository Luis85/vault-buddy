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
    assert_eq!(
        saved.record.products,
        read_ledger(root.path(), PROJECT).unwrap()
    );
    let exported =
        crate::editor::package_commands::export_envelope(&state, root.path(), SESSION, rev)
            .unwrap();
    assert_eq!(exported.record.products, saved.record.products);

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
