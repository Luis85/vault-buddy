//! Implemented `EditorCommand`/`InternalCommand` arms: `rename`,
//! `setDestination`, and the native `AddAssets` -- the ones this task
//! itself delivers (`commands/mod.rs`'s module doc explains why every
//! other kind is `apply`'s shared "not available yet" fallback instead).

use std::collections::HashSet;
use std::path::Path;

use super::payloads::{AddAssetsPayload, RenamePayload, SetDestinationPayload};
use crate::capture_paths::safe_recording_root;
use crate::editor::error::{EditorError, EditorErrorCode};
use crate::editor::ids::is_valid_id;
use crate::editor::limits;
use crate::editor::model::{Destination, Project};

fn invalid_request(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidRequest, message)
}

/// `rename{title}`: trims the title and refuses an empty result -- the one
/// bound `validate::check_title_and_names` does not itself enforce (it caps
/// the MAXIMUM title length, never a minimum). The maximum (160 chars) is
/// deliberately left for `validate_project`, run by `EditorSession::execute`
/// right after `apply` returns: enforcing it here too would mean a
/// too-long-title `rename` never reaches the validate-then-install step at
/// all, which is exactly the path `session.rs`'s
/// `invalid_candidate_is_rejected_atomically` test (and its "install before
/// validating" mutation check) exists to exercise.
pub(super) fn rename(
    project: &Project,
    payload: &RenamePayload,
) -> Result<(Project, String), EditorError> {
    let trimmed = payload.title.trim();
    if trimmed.is_empty() {
        return Err(invalid_request("title must not be empty"));
    }
    let mut candidate = project.clone();
    candidate.title = trimmed.to_string();
    Ok((candidate, "Rename".to_string()))
}

/// `setDestination{vaultId, folder, dated}`: the vault id must be
/// non-empty and the folder must pass `capture_paths::safe_recording_root`'s
/// LEXICAL rule -- the same validator every other per-vault folder setting
/// reuses. Checked against a dummy root because this layer never touches
/// disk (a later publish task resolves the real vault path); `extra` is
/// carried over from the project's current destination so an edit here can
/// never silently drop a forward-compatible field this command does not
/// itself know about (R3).
pub(super) fn set_destination(
    project: &Project,
    payload: &SetDestinationPayload,
) -> Result<(Project, String), EditorError> {
    if payload.vault_id.trim().is_empty() {
        return Err(invalid_request("vaultId must not be empty"));
    }
    safe_recording_root(Path::new("dummy-vault-root"), &payload.folder).map_err(invalid_request)?;

    let mut candidate = project.clone();
    candidate.destination = Destination {
        vault: payload.vault_id.clone(),
        folder: payload.folder.clone(),
        dated: payload.dated,
        extra: project.destination.extra.clone(),
    };
    Ok((candidate, "Set destination".to_string()))
}

/// `AddAssets{assets}` (native-only, this task's own `InternalCommand`
/// arm): every new asset id must be a valid entity id, unique among the
/// new batch AND against the project's existing assets, and the total
/// afterward must not exceed `limits::MAX_ASSETS`.
pub(super) fn add_assets(
    project: &Project,
    payload: &AddAssetsPayload,
) -> Result<(Project, String), EditorError> {
    let mut seen: HashSet<&str> = project.assets.iter().map(|a| a.id.as_str()).collect();
    for asset in &payload.assets {
        if !is_valid_id(&asset.id) {
            return Err(invalid_request(format!(
                "asset {}: id is not a valid entity id",
                asset.id
            )));
        }
        if !seen.insert(asset.id.as_str()) {
            return Err(invalid_request(format!("asset {}: duplicate id", asset.id)));
        }
    }
    let total = project.assets.len() + payload.assets.len();
    if total > limits::MAX_ASSETS {
        return Err(invalid_request(format!(
            "adding {} assets would bring the project to {} entries, exceeding the {} maximum",
            payload.assets.len(),
            total,
            limits::MAX_ASSETS
        )));
    }

    let mut candidate = project.clone();
    candidate.assets.extend(payload.assets.iter().cloned());
    Ok((candidate, "Add assets".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::model::{Asset, AssetKind};
    use crate::editor::test_support::minimal_project;
    use crate::editor::Map;

    fn asset(id: &str) -> Asset {
        Asset {
            id: id.to_string(),
            kind: AssetKind::Video,
            name: id.to_string(),
            duration_ms: 1_000,
            width: None,
            height: None,
            size: None,
            builtin: None,
            media_type: None,
            linked_asset: None,
            original_name: None,
            extra: Map::new(),
        }
    }

    #[test]
    fn rename_trims_whitespace() {
        let project = minimal_project();
        let (candidate, label) = rename(
            &project,
            &RenamePayload {
                title: "  Spaced  ".to_string(),
            },
        )
        .unwrap();
        assert_eq!(candidate.title, "Spaced");
        assert_eq!(label, "Rename");
    }

    #[test]
    fn rename_rejects_empty_after_trim() {
        let project = minimal_project();
        let err = rename(
            &project,
            &RenamePayload {
                title: "   ".to_string(),
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn rename_does_not_itself_reject_an_overlong_title() {
        // The 160-char maximum is validate_project's job, deferred to on
        // purpose -- see this function's own doc comment.
        let project = minimal_project();
        let long = "x".repeat(200);
        let (candidate, _) = rename(
            &project,
            &RenamePayload {
                title: long.clone(),
            },
        )
        .unwrap();
        assert_eq!(candidate.title, long);
    }

    #[test]
    fn set_destination_rejects_empty_vault_id() {
        let project = minimal_project();
        let err = set_destination(
            &project,
            &SetDestinationPayload {
                vault_id: "".to_string(),
                folder: "Tutorials".to_string(),
                dated: false,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn set_destination_rejects_an_escaping_folder() {
        let project = minimal_project();
        let err = set_destination(
            &project,
            &SetDestinationPayload {
                vault_id: "vault-1".to_string(),
                folder: "../outside".to_string(),
                dated: false,
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn set_destination_accepts_a_safe_folder_and_preserves_extra() {
        let mut project = minimal_project();
        project
            .destination
            .extra
            .insert("future".to_string(), serde_json::json!(true));
        let (candidate, label) = set_destination(
            &project,
            &SetDestinationPayload {
                vault_id: "vault-1".to_string(),
                folder: "Tutorials/2026".to_string(),
                dated: true,
            },
        )
        .unwrap();
        assert_eq!(candidate.destination.vault, "vault-1");
        assert_eq!(candidate.destination.folder, "Tutorials/2026");
        assert!(candidate.destination.dated);
        assert_eq!(
            candidate.destination.extra.get("future"),
            Some(&serde_json::json!(true)),
            "an edit here must not drop an unrelated forward-compat field (R3)"
        );
        assert_eq!(label, "Set destination");
    }

    #[test]
    fn add_assets_rejects_a_duplicate_within_the_batch() {
        let project = minimal_project();
        let err = add_assets(
            &project,
            &AddAssetsPayload {
                assets: vec![asset("a1"), asset("a1")],
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn add_assets_rejects_a_duplicate_against_the_existing_project() {
        let mut project = minimal_project();
        project.assets.push(asset("a1"));
        let err = add_assets(
            &project,
            &AddAssetsPayload {
                assets: vec![asset("a1")],
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn add_assets_rejects_an_invalid_id() {
        let project = minimal_project();
        let err = add_assets(
            &project,
            &AddAssetsPayload {
                assets: vec![asset("bad id")],
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn add_assets_enforces_the_count_limit() {
        let mut project = minimal_project();
        for i in 0..limits::MAX_ASSETS {
            project.assets.push(asset(&format!("existing-{i}")));
        }
        let err = add_assets(
            &project,
            &AddAssetsPayload {
                assets: vec![asset("one-too-many")],
            },
        )
        .unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }

    #[test]
    fn add_assets_appends_valid_assets() {
        let project = minimal_project();
        let (candidate, label) = add_assets(
            &project,
            &AddAssetsPayload {
                assets: vec![asset("a1"), asset("a2")],
            },
        )
        .unwrap();
        assert_eq!(candidate.assets.len(), 2);
        assert_eq!(label, "Add assets");
    }
}
