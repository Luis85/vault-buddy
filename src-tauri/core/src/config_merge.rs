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
/// document_extract_images/document_extra_frontmatter/document_body_template;
/// `set_tasks_config`: tasks_folder; `set_task_lists_config`:
/// default_list/list_order/archived_lists; `set_task_id_config`:
/// task_id_enabled/task_id_property; and the future task-template-editing
/// surface: task_extra_frontmatter/task_body_template). The preserved fields
/// are listed explicitly and everything else comes from `incoming` via `..`
/// (which is how the capture-owned note_extra_frontmatter/note_body_template
/// pair flows through), so a capture save can never transpose an owned field
/// with a preserved one.
pub fn merge_capture_owned(
    existing: &VaultCaptureConfig,
    incoming: VaultCaptureConfig,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        tasks_folder: existing.tasks_folder.clone(),
        documents_folder: existing.documents_folder.clone(),
        default_list: existing.default_list.clone(),
        list_order: existing.list_order.clone(),
        archived_lists: existing.archived_lists.clone(),
        document_date_folders: existing.document_date_folders,
        document_extract_images: existing.document_extract_images,
        // The Task ID settings are owned by set_task_id_config; a capture save
        // must never reset them, same as the lists/documents fields above.
        task_id_enabled: existing.task_id_enabled,
        task_id_property: existing.task_id_property.clone(),
        // The task/document template fields are owned by their own future
        // template-editing surfaces, not by capture. The note_* pair is
        // capture-owned and stays in `..incoming` below.
        task_extra_frontmatter: existing.task_extra_frontmatter.clone(),
        task_body_template: existing.task_body_template.clone(),
        document_extra_frontmatter: existing.document_extra_frontmatter.clone(),
        document_body_template: existing.document_body_template.clone(),
        ..incoming
    }
}

/// The `set_documents_config` counterpart: owns exactly documents_folder,
/// document_date_folders, document_extract_images, document_extra_frontmatter,
/// document_body_template; every other field is preserved from `existing` via
/// `..`.
pub fn merge_documents_owned(
    existing: &VaultCaptureConfig,
    documents_folder: Option<String>,
    document_date_folders: bool,
    document_extract_images: bool,
    document_extra_frontmatter: Option<String>,
    document_body_template: Option<String>,
) -> VaultCaptureConfig {
    VaultCaptureConfig {
        documents_folder,
        document_date_folders,
        document_extract_images,
        document_extra_frontmatter,
        document_body_template,
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
            archived_lists: vec!["Old".into()],
            document_date_folders: false,
            document_extract_images: false,
            task_id_enabled: true,
            task_id_property: Some("tid".into()),
            task_extra_frontmatter: Some("project: A".into()),
            task_body_template: Some("- [ ] {{title}}".into()),
            document_extra_frontmatter: Some("area: X".into()),
            document_body_template: Some("{{content}}".into()),
            // STALE note-template values: a capture save must OVERWRITE these
            // from `incoming`, never preserve them (note templates are
            // capture-owned). Distinct from incoming's so the assertion below
            // can tell "flowed from incoming" apart from "accidentally kept".
            note_extra_frontmatter: Some("stale-fm: 1".into()),
            note_body_template: Some("stale note body".into()),
            ..VaultCaptureConfig::default()
        };
        // incoming carries the capture-owned fields (non-owned left at defaults).
        let incoming = VaultCaptureConfig {
            mode: RecordingMode::VoiceNote,
            bitrate_kbps: 192,
            recording_date_folders: false,
            note_extra_frontmatter: Some("attendees: 3".into()),
            note_body_template: Some("## Notes\n{{type}}".into()),
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
        // The branch's newer per-vault fields (archivedLists, task ID settings)
        // must ALSO survive a capture save — merge_capture_owned lists them
        // explicitly, so this fails if a refactor drops them back to ..incoming.
        assert_eq!(merged.archived_lists, vec!["Old"]);
        assert!(merged.task_id_enabled);
        assert_eq!(merged.task_id_property.as_deref(), Some("tid"));
        // The task/document template fields (owned by the future template-editing
        // surfaces, not by capture) must ALSO survive a capture save.
        assert_eq!(
            merged.task_body_template.as_deref(),
            Some("- [ ] {{title}}")
        );
        assert_eq!(
            merged.document_extra_frontmatter.as_deref(),
            Some("area: X")
        );
        // Note template fields are CAPTURE-owned: they must come from `incoming`
        // (a capture save sets them), NOT be preserved from `existing`. This
        // fails if a regression adds them to merge_capture_owned's explicit
        // preserve list — freezing a user's note template on the next save.
        assert_eq!(
            merged.note_extra_frontmatter.as_deref(),
            Some("attendees: 3"),
            "note_extra_frontmatter must flow from incoming, not existing"
        );
        assert_eq!(
            merged.note_body_template.as_deref(),
            Some("## Notes\n{{type}}"),
            "note_body_template must flow from incoming, not existing"
        );
    }

    #[test]
    fn merge_documents_owned_writes_owned_and_preserves_the_rest() {
        let existing = VaultCaptureConfig {
            mode: RecordingMode::VoiceNote,
            recording_date_folders: false,
            tasks_folder: Some("T".into()),
            ..VaultCaptureConfig::default()
        };
        let merged = merge_documents_owned(
            &existing,
            Some("Docs".into()),
            false,
            false,
            Some("area: Legal".into()),
            Some("{{content}}".into()),
        );
        // owned
        assert_eq!(merged.documents_folder.as_deref(), Some("Docs"));
        assert!(!merged.document_date_folders);
        assert!(!merged.document_extract_images);
        assert_eq!(
            merged.document_extra_frontmatter.as_deref(),
            Some("area: Legal")
        );
        assert_eq!(
            merged.document_body_template.as_deref(),
            Some("{{content}}")
        );
        // preserved (would break if set_documents_config touched them)
        assert_eq!(merged.mode, RecordingMode::VoiceNote);
        assert!(!merged.recording_date_folders);
        assert_eq!(merged.tasks_folder.as_deref(), Some("T"));
    }
}
