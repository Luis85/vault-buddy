//! Review renders (Task 47; F-42; pre-flight F18): a short range rendered
//! by the REAL renderer so the user can watch the actual encoded output --
//! the one bridge from the webview's approximate preview to the ffmpeg
//! render (ADR GAP-N3).
//!
//! **A review is not a Rendered Product.** It never reaches `products\`
//! or `products.json`: at the 40-product cap, with no removal UI yet
//! (GAP-187), a review that consumed a ledger slot would make rendering
//! silently impossible after enough reviews. Its file is the project's
//! `cache\review-<jobId>.mp4` -- inside R7's pinned asset scope
//! (`editor-projects/*/cache/*`), so `editor_media_url({reviewJobId})` can
//! serve it; `jobs\` is deliberately outside that scope, which is why the
//! review is moved out of its job directory at all (controller ruling).
//!
//! **It is disposable.** Only the newest review is kept: landing one
//! removes every older `review-*.mp4` of the project, and closing the
//! session removes the last (`sweep_reviews`). Only names this module mints
//! are touched, no-follow -- a symlink wearing one of them, or anything
//! else in `cache\`, is left alone.
//!
//! **It is still render-kind work.** `render_jobs` runs it as a `render`
//! job, so it is one per session, counted by the shutdown gate, and
//! cancelled by the same quit and discard paths as a product render.

use std::path::{Path, PathBuf};

use vault_buddy_core::capture_paths::rename_noreplace;
use vault_buddy_core::editor::{is_valid_id, EditorError, EditorErrorCode};

use super::project_store::project_dir;

const PREFIX: &str = "review-";
const SUFFIX: &str = ".mp4";

/// The review file's name for `job_id`.
pub(crate) fn review_file_name(job_id: &str) -> String {
    format!("{PREFIX}{job_id}{SUFFIX}")
}

/// Is `name` a review file this module minted (`review-<valid id>.mp4`)?
fn is_review_name(name: &str) -> bool {
    name.strip_prefix(PREFIX)
        .and_then(|rest| rest.strip_suffix(SUFFIX))
        .is_some_and(is_valid_id)
}

fn cache_dir(root: &Path, project_id: &str) -> Result<PathBuf, EditorError> {
    project_dir(root, project_id)
        .map(|dir| dir.join("cache"))
        .ok_or_else(|| EditorError::new(EditorErrorCode::Internal, "Not a valid project id."))
}

/// Where `job_id`'s review lives (whether or not it is there): an invalid
/// id names nothing (`unauthorizedSource`).
pub(crate) fn review_path(
    root: &Path,
    project_id: &str,
    job_id: &str,
) -> Result<PathBuf, EditorError> {
    if !is_valid_id(job_id) {
        return Err(EditorError::new(
            EditorErrorCode::UnauthorizedSource,
            format!("review {job_id:?} is not a review of this project."),
        ));
    }
    Ok(cache_dir(root, project_id)?.join(review_file_name(job_id)))
}

/// Remove every review file of the project except `keep` -- owned names
/// only, never through a symlink. Logged, never raised: a review left
/// behind costs disk, not correctness.
pub(crate) fn sweep_reviews(root: &Path, project_id: &str, keep: Option<&str>) {
    let Ok(cache) = cache_dir(root, project_id) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&cache) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_review_name(&name) || Some(name.as_str()) == keep {
            continue;
        }
        let path = entry.path();
        let plain = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file());
        if !plain {
            continue;
        }
        if let Err(e) = std::fs::remove_file(&path) {
            log::warn!("editor review: could not remove an old review: {e}");
        }
    }
}

/// Move a finished review's `part` into the project's cache as
/// `review-<jobId>.mp4` and drop every older review. The caller holds the
/// session's save lock (a discard removing the directory cannot interleave)
/// and has re-checked cancellation under it.
pub(crate) fn keep_review(
    root: &Path,
    project_id: &str,
    job_id: &str,
    part: &Path,
) -> Result<(), EditorError> {
    let dest = review_path(root, project_id, job_id)?;
    let cache = cache_dir(root, project_id)?;
    // `create_dir`, not `create_dir_all`: the project directory exists
    // (the session is alive under its save lock) and must never be
    // recreated by a cache write.
    match std::fs::create_dir(&cache) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => {
            return Err(EditorError::new(
                EditorErrorCode::Internal,
                format!("The review could not be kept: {e}"),
            ))
        }
    }
    let name = review_file_name(job_id);
    sweep_reviews(root, project_id, Some(&name));
    rename_noreplace(part, &dest).map_err(|e| {
        EditorError::new(
            EditorErrorCode::Internal,
            format!("The review could not be kept: {e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_minted_review_names_are_ours() {
        assert_eq!(review_file_name("job-a1"), "review-job-a1.mp4");
        assert!(is_review_name("review-job-a1.mp4"));
        for other in [
            "review-.mp4",
            "review-job a.mp4",
            "review-job-a1.mp4.part",
            "prod-a.mp4",
            "cap-250.jpg",
            "review-../x.mp4",
        ] {
            assert!(!is_review_name(other), "{other}");
        }
    }
}
