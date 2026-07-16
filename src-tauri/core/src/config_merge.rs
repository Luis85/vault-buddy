//! Config preserve-vs-write merge helpers (GAP-60). Extracted from
//! `vault_config` for LOC headroom: `set_capture_config` and
//! `set_documents_config` each own a subset of `VaultCaptureConfig`'s fields
//! and must carry the rest forward from the existing config. These pure
//! helpers make that split explicit (struct-update syntax lists exactly the
//! preserved fields) and unit-testable in the core crate, so a transposed
//! field assignment fails a test instead of silently resetting a user's
//! settings on their next save. `capture_config` re-exports both names, so
//! callers use them as `capture_config::merge_*_owned`.

use crate::vault_config::VaultCaptureConfig;

/// Merge the fields `set_capture_config` OWNS from `incoming` onto `existing`,
/// preserving every field another settings command owns
/// (`set_documents_config`: documents_folder/document_date_folders/
/// document_extract_images; `set_tasks_config`: tasks_folder;
/// `set_task_lists_config`: default_list/list_order). The preserved fields are
/// listed explicitly and everything else comes from `incoming` via `..`, so a
/// capture save can never transpose an owned field with a preserved one.
pub fn merge_capture_owned(
    existing: &VaultCaptureConfig,
    incoming: VaultCaptureConfig,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        tasks_folder: existing.tasks_folder.clone(),
        documents_folder: existing.documents_folder.clone(),
        default_list: existing.default_list.clone(),
        list_order: existing.list_order.clone(),
        document_date_folders: existing.document_date_folders,
        document_extract_images: existing.document_extract_images,
        ..incoming
    }
}

/// The `set_documents_config` counterpart: owns exactly documents_folder,
/// document_date_folders, document_extract_images; every other field is
/// preserved from `existing` via `..`.
pub fn merge_documents_owned(
    existing: &VaultCaptureConfig,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        documents_folder,
        document_date_folders,
        document_extract_images,
        ..existing.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault_config::RecordingMode;

    #[test]
    fn merge_capture_owned_writes_owned_and_preserves_the_rest() {
        // existing carries distinctive values for every NON-capture-owned field.
        let existing = VaultCaptureConfig {
            tasks_folder: Some("Inbox/Tasks".into()),
            documents_folder: Some("Inbox/Docs".into()),
            default_list: Some("Inbox".into()),
            list_order: vec!["Inbox".into(), "Next".into()],
            document_date_folders: false,
            document_extract_images: false,
            ..VaultCaptureConfig::default()
        };
        // incoming carries the capture-owned fields (non-owned left at defaults).
        let incoming = VaultCaptureConfig {
            mode: RecordingMode::VoiceNote,
            bitrate_kbps: 192,
            recording_date_folders: false,
            ..VaultCaptureConfig::default()
        };
        let merged = merge_capture_owned(&existing, incoming);
        // owned fields come from incoming
        assert_eq!(merged.mode, RecordingMode::VoiceNote);
        assert_eq!(merged.bitrate_kbps, 192);
        assert!(!merged.recording_date_folders);
        // every non-owned field is preserved from existing (a transposed
        // document/recording date-folder pair would fail here)
        assert_eq!(merged.tasks_folder.as_deref(), Some("Inbox/Tasks"));
        assert_eq!(merged.documents_folder.as_deref(), Some("Inbox/Docs"));
        assert_eq!(merged.default_list.as_deref(), Some("Inbox"));
        assert_eq!(merged.list_order, vec!["Inbox", "Next"]);
        assert!(!merged.document_date_folders);
        assert!(!merged.document_extract_images);
    }

    #[test]
    fn merge_documents_owned_writes_owned_and_preserves_the_rest() {
        let existing = VaultCaptureConfig {
            mode: RecordingMode::VoiceNote,
            recording_date_folders: false,
            tasks_folder: Some("T".into()),
            ..VaultCaptureConfig::default()
        };
        let merged = merge_documents_owned(&existing, Some("Docs".into()), false, false);
        // owned
        assert_eq!(merged.documents_folder.as_deref(), Some("Docs"));
        assert!(!merged.document_date_folders);
        assert!(!merged.document_extract_images);
        // preserved (would break if set_documents_config touched them)
        assert_eq!(merged.mode, RecordingMode::VoiceNote);
        assert!(!merged.recording_date_folders);
        assert_eq!(merged.tasks_folder.as_deref(), Some("T"));
    }
}
