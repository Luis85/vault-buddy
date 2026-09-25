use std::cell::Cell;

use serde_json::json;

use super::*;

fn progress(overrides: Value) -> Value {
    let mut base = json!({
        "contentRevision": 1,
        "currentStepId": "layout",
        "reviewed": ["welcome", "tracks"],
        "explored": ["tracks"],
        "invitationDismissed": true,
        "active": true,
        "collapsed": true,
        "completed": false,
        "preferences": { "dimming": false, "motion": "reduced" }
    });
    for (k, v) in overrides.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

fn progress_file(root: &Path) -> std::path::PathBuf {
    root.join(PREFS_DIR).join(GUIDE_PROGRESS_FILE)
}

#[test]
fn guide_progress_round_trips_through_the_app_wide_prefs_folder() {
    let root = tempfile::tempdir().unwrap();

    save_guide_progress_in(root.path(), progress(json!({}))).unwrap();

    assert!(progress_file(root.path()).is_file());
    let read = read_guide_progress(root.path()).unwrap();
    assert_eq!(read.current_step_id.as_deref(), Some("layout"));
    assert_eq!(read.reviewed, ["welcome", "tracks"]);
    assert!(read.collapsed);
    // App-wide: nothing lands in the project store.
    assert!(!root.path().join("editor-projects").exists());
}

// This task's named Rust case: a save naming a lesson this build does
// not know, or carrying anything path-shaped, is refused and writes
// NOTHING — the last good file stays byte-identical.
#[test]
fn guide_progress_rejects_unknown_ids_and_paths() {
    let root = tempfile::tempdir().unwrap();
    save_guide_progress_in(root.path(), progress(json!({}))).unwrap();
    let before = std::fs::read(progress_file(root.path())).unwrap();

    let refused = [
        progress(json!({ "currentStepId": "trim" })),
        progress(json!({ "currentStepId": r"C:\Users\me\capture.mp4" })),
        progress(json!({ "reviewed": [r"..\..\project.json"] })),
        progress(json!({ "explored": ["welcome", "/tmp/x"] })),
        progress(json!({ "mediaPath": r"C:\Users\me\capture.mp4" })),
    ];
    for raw in refused {
        let err = save_guide_progress_in(root.path(), raw.clone())
            .expect_err(&format!("{raw} must be refused"));
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(!err.message.contains("Users"), "{}", err.message);
    }
    assert_eq!(std::fs::read(progress_file(root.path())).unwrap(), before);
}

#[test]
fn a_missing_malformed_or_oversized_file_reads_as_fresh_progress() {
    let root = tempfile::tempdir().unwrap();
    assert_eq!(
        read_guide_progress(root.path()).unwrap(),
        GuideProgress::default()
    );

    std::fs::create_dir(root.path().join(PREFS_DIR)).unwrap();
    std::fs::write(progress_file(root.path()), "{ not json").unwrap();
    assert_eq!(
        read_guide_progress(root.path()).unwrap(),
        GuideProgress::default()
    );

    let mut huge = progress(json!({})).to_string();
    huge.push_str(&" ".repeat(guide::MAX_GUIDE_PROGRESS_BYTES));
    std::fs::write(progress_file(root.path()), huge).unwrap();
    assert_eq!(
        read_guide_progress(root.path()).unwrap(),
        GuideProgress::default()
    );
}

// Storage that is there but cannot be read is NOT fresh progress: the
// webview must learn it, so it can say "Session only" instead of
// quietly starting over and then overwriting what it could not read.
#[test]
fn a_progress_path_that_is_not_a_plain_file_is_an_error_not_fresh_progress() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(progress_file(root.path())).unwrap();

    let err = read_guide_progress(root.path()).unwrap_err();

    assert_eq!(err.code, EditorErrorCode::Internal);
    assert_eq!(
        err.message,
        "Guide progress could not be read: not a plain file or folder"
    );
}

#[test]
fn a_stored_lesson_this_build_does_not_know_resumes_at_the_first_lesson() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(PREFS_DIR)).unwrap();
    let stored = progress(json!({ "currentStepId": "trim", "reviewed": ["trim", "welcome"] }));
    std::fs::write(progress_file(root.path()), stored.to_string()).unwrap();

    let read = read_guide_progress(root.path()).unwrap();

    assert_eq!(read.current_step_id.as_deref(), Some("welcome"));
    assert_eq!(read.reviewed, ["welcome"]);
    assert_eq!(read.preferences.motion, guide::GuideMotion::Reduced);
}

// ---- the portable progress file (Task 57) -----------------------------------

/// The dialogs, answered: `save`/`open` are what the user "picked";
/// `asked` counts every dialog opened, so a refusal that must come BEFORE
/// the dialog can be told apart from one after it.
struct Chooser {
    save: Option<PathBuf>,
    open: Option<PathBuf>,
    asked: Cell<u32>,
}

impl Chooser {
    fn saving(path: PathBuf) -> Self {
        Self {
            save: Some(path),
            open: None,
            asked: Cell::new(0),
        }
    }
    fn opening(path: PathBuf) -> Self {
        Self {
            save: None,
            open: Some(path),
            asked: Cell::new(0),
        }
    }
    fn dismissed() -> Self {
        Self {
            save: None,
            open: None,
            asked: Cell::new(0),
        }
    }
}

impl GuideFileChooser for Chooser {
    fn save_target(&self, suggested: &str) -> Option<PathBuf> {
        assert_eq!(suggested, SUGGESTED_FILE_NAME);
        self.asked.set(self.asked.get() + 1);
        self.save.clone()
    }
    fn file_to_open(&self) -> Option<PathBuf> {
        self.asked.set(self.asked.get() + 1);
        self.open.clone()
    }
}

// This task's named Rust case: Save progress file then Restore progress
// file gives back exactly the validated progress; a file that is not guide
// progress — a project document, an unknown lesson, a folder, padding past
// 16 KiB — is refused `invalidRequest`, never echoed and never changed.
#[test]
fn guide_progress_file_round_trips_and_rejects_foreign_json() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("backup.json");

    let written =
        export_guide_progress_in(&Chooser::saving(target.clone()), progress(json!({}))).unwrap();
    assert_eq!(written.as_deref(), Some("backup.json"));
    let restored = import_guide_progress_in(&Chooser::opening(target.clone()))
        .unwrap()
        .expect("a picked file");
    assert_eq!(
        restored,
        guide::validate_for_save(&progress(json!({}))).unwrap()
    );

    let project = dir.path().join("project.json");
    std::fs::write(
        &project,
        json!({ "schema": "vault-buddy-video-project/3", "id": "p", "title": r"C:\Users\me" })
            .to_string(),
    )
    .unwrap();
    let unknown = dir.path().join("unknown.json");
    std::fs::write(
        &unknown,
        progress(json!({ "currentStepId": "trim" })).to_string(),
    )
    .unwrap();
    let padded = dir.path().join("padded.json");
    let mut text = progress(json!({})).to_string();
    text.push_str(&" ".repeat(guide::MAX_GUIDE_PROGRESS_BYTES));
    std::fs::write(&padded, text).unwrap();
    let folder = dir.path().join("folder.json");
    std::fs::create_dir(&folder).unwrap();

    for path in [project.clone(), unknown, padded, folder] {
        let before = std::fs::read(&path).ok();
        let err = import_guide_progress_in(&Chooser::opening(path.clone()))
            .expect_err(&format!("{} must be refused", path.display()));
        assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
        assert!(!err.message.contains("Users"), "{}", err.message);
        assert_eq!(std::fs::read(&path).ok(), before);
    }
}

#[test]
fn a_dismissed_dialog_writes_and_returns_nothing() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        export_guide_progress_in(&Chooser::dismissed(), progress(json!({}))).unwrap(),
        None
    );
    assert_eq!(
        import_guide_progress_in(&Chooser::dismissed()).unwrap(),
        None
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

// A document the save gate refuses never reaches a dialog, so no file —
// not even an empty one — is created for it.
#[test]
fn an_invalid_export_is_refused_before_the_dialog_opens() {
    let dir = tempfile::tempdir().unwrap();
    let chooser = Chooser::saving(dir.path().join("x.json"));

    let err = export_guide_progress_in(
        &chooser,
        progress(json!({ "reviewed": [r"C:\Users\me\capture.mp4"] })),
    )
    .unwrap_err();

    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert_eq!(chooser.asked.get(), 0);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

// The replace rule: only a plain file that is itself guide progress AND is
// exactly the path the dialog confirmed. A name the extension rule moved
// onto an existing file was never confirmed.
#[test]
fn an_export_replaces_only_a_confirmed_progress_file() {
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.json");
    std::fs::write(&notes, r#"{"not":"progress"}"#).unwrap();
    let err =
        export_guide_progress_in(&Chooser::saving(notes.clone()), progress(json!({}))).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::WriteDenied);
    assert_eq!(
        err.message,
        "“notes.json” already exists. Choose a new name; only a guide progress file you pick in the dialog is replaced."
    );
    assert_eq!(
        std::fs::read_to_string(&notes).unwrap(),
        r#"{"not":"progress"}"#
    );

    let backup = dir.path().join("backup.json");
    let older = progress(json!({ "currentStepId": "welcome" })).to_string();
    std::fs::write(&backup, &older).unwrap();
    // "backup" normalizes onto the existing backup.json: not confirmed.
    let err = export_guide_progress_in(
        &Chooser::saving(dir.path().join("backup")),
        progress(json!({})),
    )
    .unwrap_err();
    assert_eq!(
        err.message,
        "“backup.json” already exists. Choose a new name; only a guide progress file you pick in the dialog is replaced."
    );
    assert_eq!(err.code, EditorErrorCode::WriteDenied);
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), older);

    // Picked in the dialog: replaced.
    export_guide_progress_in(&Chooser::saving(backup.clone()), progress(json!({}))).unwrap();
    let now = import_guide_progress_in(&Chooser::opening(backup))
        .unwrap()
        .unwrap();
    assert_eq!(now.current_step_id.as_deref(), Some("layout"));
    // No temporary file is left beside it.
    let names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.iter().all(|n| !n.ends_with(".part")), "{names:?}");
}
