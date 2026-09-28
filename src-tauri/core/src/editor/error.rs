//! The editor's IPC error shape (Contract reference `EditorErrorCode` /
//! `EditorError`). Serialized camelCase like every other DTO in the repo
//! (R3) — the project graph it can wrap travels in document spelling, but
//! this envelope does not.

use super::ids::new_entity_id;
use serde::{Deserialize, Serialize};

/// The editor's IPC error codes (Contract reference, 15 variants).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorErrorCode {
    InvalidRequest,
    InvalidProject,
    RevisionConflict,
    SessionGone,
    UnauthorizedSource,
    SourceMissing,
    UnsupportedMedia,
    DeviceUnavailable,
    PermissionDenied,
    DiskFull,
    WriteDenied,
    DestinationUnavailable,
    EncoderUnavailable,
    Cancelled,
    Internal,
}

impl EditorErrorCode {
    /// Whether retrying the same operation, unchanged, might succeed —
    /// a revision conflict resolves after a refetch, a full disk after
    /// space frees, an unavailable destination/device after it returns.
    /// Every other code names a defect in the request or the environment
    /// that retrying alone cannot fix.
    fn is_retryable(self) -> bool {
        matches!(
            self,
            EditorErrorCode::RevisionConflict
                | EditorErrorCode::DiskFull
                | EditorErrorCode::DestinationUnavailable
                | EditorErrorCode::DeviceUnavailable
        )
    }
}

/// The editor's IPC error envelope (Contract reference). `operation_id` is
/// always present (never optional) so a failed command can be correlated
/// with its audit/log trail even when nothing was retained;
/// `retained_asset_ids` is the exception, present only when an operation
/// left assets behind that a caller must reconcile (R20: nothing is faked,
/// so a partial outcome is never silently swallowed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorError {
    pub code: EditorErrorCode,
    pub message: String,
    pub retryable: bool,
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_asset_ids: Option<Vec<String>>,
}

impl EditorError {
    /// Builds an error with `retryable` derived from `code` and a fresh
    /// `operation_id` — callers never invent either by hand.
    pub fn new(code: EditorErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: code.is_retryable(),
            operation_id: new_entity_id("op"),
            retained_asset_ids: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serializes_camel_case_literal() {
        let error = EditorError {
            code: EditorErrorCode::RevisionConflict,
            message: "m".to_string(),
            retryable: true,
            operation_id: "op-x".to_string(),
            retained_asset_ids: None,
        };
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::json!({
                "code": "revisionConflict",
                "message": "m",
                "retryable": true,
                "operationId": "op-x",
            }),
        );
    }

    #[test]
    fn new_sets_retryable_per_code_and_a_fresh_operation_id() {
        for code in [
            EditorErrorCode::RevisionConflict,
            EditorErrorCode::DiskFull,
            EditorErrorCode::DestinationUnavailable,
            EditorErrorCode::DeviceUnavailable,
        ] {
            let error = EditorError::new(code, "m");
            assert!(error.retryable, "{code:?} should be retryable");
        }
        for code in [
            EditorErrorCode::InvalidRequest,
            EditorErrorCode::InvalidProject,
            EditorErrorCode::SessionGone,
            EditorErrorCode::UnauthorizedSource,
            EditorErrorCode::SourceMissing,
            EditorErrorCode::UnsupportedMedia,
            EditorErrorCode::PermissionDenied,
            EditorErrorCode::WriteDenied,
            EditorErrorCode::EncoderUnavailable,
            EditorErrorCode::Cancelled,
            EditorErrorCode::Internal,
        ] {
            let error = EditorError::new(code, "m");
            assert!(!error.retryable, "{code:?} should not be retryable");
        }
        let a = EditorError::new(EditorErrorCode::Internal, "m");
        let b = EditorError::new(EditorErrorCode::Internal, "m");
        assert_ne!(a.operation_id, b.operation_id);
        assert!(super::super::is_valid_id(&a.operation_id));
    }
}
