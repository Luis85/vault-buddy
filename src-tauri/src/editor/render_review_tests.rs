//! Review renders (Task 47, pre-flight F18): a range render the editor
//! plays back as the REAL encoded output, which is not a Rendered Product.
//! It lands in the project's `cache\review-<jobId>.mp4` -- inside R7's
//! asset scope, never `products\`, never `products.json` -- so the 40-product
//! cap (GAP-187) can never be consumed by previews. It is still render-kind
//! work: one per session, counted by the shutdown gate, cancelled by the
//! same quit and discard paths.

use std::time::Duration;

use serde_json::json;
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::tests::{
    entries, opened, snapshot_revision, terminal, Behaviour, CancelOnDrop, FakeRunner, PROJECT,
    SESSION,
};
use super::*;
use crate::editor::media_commands::{media_path_in, parse_media_ref, MediaRef};
use crate::editor::media_jobs::tests::CollectingSink;
use crate::editor::media_jobs::{JobPhase, JobProgressDto};
use crate::editor::render_review::review_file_name;
use crate::editor::session_commands::{close_in, CloseDisposition};

fn review_request(state: &EditorState) -> RenderRequest {
    serde_json::from_value(json!({
        "sessionId": SESSION,
        "expectedRevision": snapshot_revision(state),
        "name": "Review",
        "range": {"startMs": 250, "endMs": 1_500},
        "quality": "balanced",
        "review": true,
    }))
    .unwrap()
}

/// Run one review to its terminal through the fake; its job id and the
/// terminal message.
fn reviewed(state: &EditorState, root: &Path) -> (String, JobProgressDto) {
    let runner = FakeRunner::new(Behaviour::Writes(b"review bytes".to_vec()));
    let (job, runner) = begin_render(state, root, &review_request(state), || Ok(runner)).unwrap();
    let job_id = job.job_id.clone();
    let sink = CollectingSink::default();
    run_render_job(state, job, &runner, &sink);
    (job_id, terminal(&sink))
}

// Literal JSON: `review` is optional on the wire and absent means a
// product render -- every Task 46 caller keeps its meaning.
#[test]
fn review_request_wire_shape_is_pinned() {
    let review: RenderRequest = serde_json::from_value(json!({
        "sessionId": "s", "expectedRevision": 1, "name": "Review",
        "range": {"startMs": 0, "endMs": 10}, "quality": "low", "review": true
    }))
    .unwrap();
    assert!(review.review);
    let product: RenderRequest = serde_json::from_value(json!({
        "sessionId": "s", "expectedRevision": 1, "name": "n", "range": null, "quality": "low"
    }))
    .unwrap();
    assert!(!product.review, "an absent flag is a product render");
    assert_eq!(
        parse_media_ref(&json!({ "reviewJobId": "job-1" })).unwrap(),
        MediaRef::Review("job-1".into())
    );
}

// F18: forty consecutive Reviews -- the product cap's worth -- leave the
// product ledger empty, write nothing into `products\`, send no
// `productId`, and leave exactly ONE review file behind (the newest).
#[test]
fn review_renders_never_add_a_product_library_entry() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let mut last = String::new();
    for _ in 0..limits::MAX_PRODUCTS {
        let (job_id, done) = reviewed(&state, root.path());
        assert_eq!(done.phase, JobPhase::Complete, "{done:?}");
        let terminal = done.terminal.expect("a terminal record");
        assert_eq!(terminal.product_id, None, "a review is not a product");
        last = job_id;
    }
    assert!(products_in(&state, root.path(), SESSION)
        .unwrap()
        .is_empty());
    assert!(!dir.join(PRODUCTS_FILE).exists(), "no ledger was written");
    assert!(entries(&dir.join(PRODUCTS_DIR)).is_empty());
    assert_eq!(entries(&dir.join("cache")), [review_file_name(&last)]);
    assert!(entries(&dir.join(JOBS_DIR)).is_empty(), "no job scratch");
}

// A full ledger refuses a PRODUCT render (GAP-187) but never a review --
// which is the whole reason a review is not a product.
#[test]
fn a_review_still_starts_at_the_product_cap() {
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
    let (_, done) = reviewed(&state, root.path());
    assert_eq!(done.phase, JobPhase::Complete, "{done:?}");
    assert_eq!(
        read_ledger(root.path(), PROJECT).unwrap().len(),
        limits::MAX_PRODUCTS
    );
}

// The review is served by `editor_media_url({reviewJobId})` from the
// project's cache, and a session close removes it (it is disposable).
#[test]
fn a_review_is_served_from_the_cache_and_removed_on_close() {
    let root = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let (job_id, _) = reviewed(&state, root.path());
    let path = media_path_in(
        &state,
        root.path(),
        SESSION,
        &MediaRef::Review(job_id.clone()),
    )
    .unwrap();
    assert_eq!(path, dir.join("cache").join(review_file_name(&job_id)));
    assert_eq!(std::fs::read(&path).unwrap(), b"review bytes");
    let e = media_path_in(
        &state,
        root.path(),
        SESSION,
        &MediaRef::Review("job-none".into()),
    )
    .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::SourceMissing);
    // Something that is not ours in the cache survives the close.
    std::fs::write(dir.join("cache").join("notes.txt"), b"keep").unwrap();
    close_in(
        &state,
        root.path(),
        staging.path(),
        SESSION,
        CloseDisposition::Keep,
    )
    .unwrap();
    assert_eq!(entries(&dir.join("cache")), ["notes.txt"]);
}

// A review is render-kind work: the shutdown gate counts it and the quit's
// bounded cancel stops it, leaving no review file and no job scratch.
#[test]
fn a_review_is_counted_and_cancelled_like_a_render() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let dir = opened(root.path(), &state);
    let (runner, started) = FakeRunner::signalling(Behaviour::UntilCancelled);
    let (job, runner) =
        begin_render(&state, root.path(), &review_request(&state), || Ok(runner)).unwrap();
    let sink = CollectingSink::default();
    std::thread::scope(|scope| {
        let _stop = CancelOnDrop(job.cancel.clone());
        scope.spawn(|| run_render_job(&state, job, &runner, &sink));
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(render_blocks_shutdown(&state), "a review blocks shutdown");
        assert!(cancel_all_in(
            &state,
            Duration::from_secs(10),
            Duration::from_millis(5)
        ));
    });
    assert_eq!(terminal(&sink).phase, JobPhase::Cancelled);
    assert!(entries(&dir.join("cache")).is_empty());
    assert!(entries(&dir.join(JOBS_DIR)).is_empty());
}
