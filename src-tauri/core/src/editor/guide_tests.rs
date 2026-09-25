use serde_json::json;

use super::*;

fn saved(overrides: Value) -> Value {
    let mut base = json!({
        "contentRevision": 1,
        "currentStepId": "split",
        "reviewed": ["welcome", "media"],
        "explored": ["preview"],
        "invitationDismissed": true,
        "active": true,
        "collapsed": false,
        "completed": false,
        "preferences": { "dimming": false, "motion": "reduced" }
    });
    for (k, v) in overrides.as_object().unwrap() {
        base[k] = v.clone();
    }
    base
}

#[test]
fn the_compiled_steps_are_the_22_lessons_in_order() {
    assert_eq!(
        known_step_ids(),
        [
            "welcome", "media", "preview", "timeline", "select", "split", "undo", "arrange",
            "context", "tracks", "webcam", "layout", "fades", "audio", "callouts", "captions",
            "chapters", "checks", "save", "render", "products", "help",
        ]
    );
}

#[test]
fn the_retired_map_names_real_chapters_and_no_live_lesson() {
    for (old, chapter) in &content().retired {
        assert!(!is_known(old), "retired id {old:?} is still a live lesson");
        assert!(
            first_step_of(chapter).is_some(),
            "retired id {old:?} names no chapter {chapter:?}"
        );
    }
}

// The Contract reference's `GuideProgress`, written as a LITERAL so a
// renamed field cannot be made green by re-serializing the struct against
// itself.
#[test]
fn guide_progress_serializes_to_the_contract_literal() {
    let progress = GuideProgress {
        content_revision: 1,
        current_step_id: None,
        reviewed: vec!["welcome".into()],
        explored: vec![],
        invitation_dismissed: true,
        active: false,
        collapsed: true,
        completed: false,
        preferences: GuidePreferences {
            dimming: true,
            motion: GuideMotion::Full,
        },
    };
    assert_eq!(
        serde_json::to_value(&progress).unwrap(),
        json!({
            "contentRevision": 1,
            "currentStepId": null,
            "reviewed": ["welcome"],
            "explored": [],
            "invitationDismissed": true,
            "active": false,
            "collapsed": true,
            "completed": false,
            "preferences": { "dimming": true, "motion": "full" }
        })
    );
}

#[test]
fn a_valid_save_round_trips_and_dedupes() {
    let raw = saved(json!({ "reviewed": ["welcome", "media", "welcome"] }));
    let progress = validate_for_save(&raw).unwrap();
    assert_eq!(progress.current_step_id.as_deref(), Some("split"));
    assert_eq!(progress.reviewed, ["welcome", "media"]);
    assert_eq!(progress.explored, ["preview"]);
    assert_eq!(progress.preferences.motion, GuideMotion::Reduced);
    assert!(!progress.preferences.dimming);
}

#[test]
fn a_save_refuses_unknown_ids_paths_and_extra_fields() {
    let cases = [
        saved(json!({ "currentStepId": "trim" })),
        saved(json!({ "currentStepId": "C:\\Users\\me\\Videos\\secret.mp4" })),
        saved(json!({ "reviewed": ["welcome", "../welcome"] })),
        saved(json!({ "explored": ["/home/me/project.json"] })),
        saved(json!({ "sourcePath": "C:\\Users\\me\\capture.mp4" })),
        saved(json!({ "preferences": { "dimming": true, "motion": "fast" } })),
        saved(json!({ "preferences": { "dimming": true, "motion": "full", "folder": "D:\\x" } })),
        saved(json!({ "contentRevision": 0 })),
        json!("C:\\Users\\me"),
    ];
    for raw in cases {
        let err = validate_for_save(&raw).expect_err(&format!("{raw} must be refused"));
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(
            !err.message.contains("Users") && !err.message.contains("home"),
            "a refusal must not echo what it refused: {}",
            err.message
        );
    }
}

#[test]
fn a_save_over_16_kib_is_refused_by_size() {
    let raw = saved(json!({ "reviewed": vec!["welcome"; 2_000] }));
    let err = validate_for_save(&raw).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(
        err.message.contains("16384 byte maximum"),
        "{}",
        err.message
    );
}

#[test]
fn a_stored_file_keeps_known_ids_and_its_preferences() {
    let stored = saved(json!({}));
    let read = parse_stored(stored.to_string().as_bytes()).unwrap();
    assert_eq!(read, validate_for_save(&stored).unwrap());
}

#[test]
fn a_stored_unknown_step_falls_back_to_the_first_lesson_and_drops_the_rest() {
    let stored = saved(json!({
        "currentStepId": "trim",
        "reviewed": ["welcome", "trim"],
        "explored": ["C:\\x"]
    }));
    let read = parse_stored(stored.to_string().as_bytes()).unwrap();
    assert_eq!(read.current_step_id.as_deref(), Some("welcome"));
    assert_eq!(read.reviewed, ["welcome"]);
    assert!(read.explored.is_empty());
    // Unrelated preferences survive an id the build no longer knows.
    assert_eq!(read.preferences.motion, GuideMotion::Reduced);
    assert!(read.invitation_dismissed);
}

#[test]
fn a_retired_step_resumes_at_its_chapters_first_lesson() {
    let retired = vec![("trim".to_owned(), "edit".to_owned())];
    assert_eq!(resolve_step_with("trim", &retired), "select");
    assert_eq!(resolve_step_with("undo", &retired), "undo");
    assert_eq!(resolve_step_with("gone", &retired), "welcome");

    let stored: GuideProgress =
        serde_json::from_value(saved(json!({ "currentStepId": "trim" }))).unwrap();
    assert_eq!(
        normalize_with(stored, &retired).current_step_id.as_deref(),
        Some("select")
    );
}

#[test]
fn an_unreadable_or_oversized_file_reads_as_nothing() {
    assert_eq!(parse_stored(b"not json"), None);
    assert_eq!(parse_stored(br#"{"contentRevision":1}"#), None);
    let huge = vec![b' '; MAX_GUIDE_PROGRESS_BYTES + 1];
    assert_eq!(parse_stored(&huge), None);
}

#[test]
fn nothing_saved_yet_is_the_fresh_default() {
    let fresh = GuideProgress::default();
    assert_eq!(fresh.content_revision, 0);
    assert_eq!(fresh.current_step_id, None);
    assert!(fresh.preferences.dimming);
    assert_eq!(fresh.preferences.motion, GuideMotion::System);
    assert_eq!(resolve_step_id("media"), "media");
}

// Task 57 (F-47): a progress FILE the user picked in the Restore dialog is
// judged by the SAME strict gate as a save — the raw byte bound first (so
// trailing padding cannot slip past a re-serialized size), JSON next, then
// `validate_for_save`. Foreign JSON (a project file) is refused, and no
// refusal echoes what the file held.
#[test]
fn a_progress_file_is_read_strictly_and_foreign_json_is_refused() {
    let good = saved(json!({})).to_string();
    assert_eq!(
        parse_progress_file(good.as_bytes()).unwrap(),
        validate_for_save(&saved(json!({}))).unwrap()
    );

    let project = json!({
        "schema": "vault-buddy-video-project/3",
        "id": "project-a",
        "title": r"C:\Users\me\secret"
    })
    .to_string();
    let mut padded = good.clone();
    padded.push_str(&" ".repeat(MAX_GUIDE_PROGRESS_BYTES));
    let unknown = saved(json!({ "currentStepId": "trim" })).to_string();
    let cases: [Vec<u8>; 5] = [
        b"not json".to_vec(),
        vec![0xff, 0xfe, 0x00],
        project.into_bytes(),
        padded.into_bytes(),
        unknown.into_bytes(),
    ];
    for bytes in cases {
        let err = parse_progress_file(&bytes).expect_err("must be refused");
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
        assert!(!err.message.contains("Users"), "{}", err.message);
    }
}

// Task 57 fix round 1: a progress FILE outlives builds (surviving a
// reinstall is what it is for), so a lesson retired since it was written
// resumes where the stored file would — its chapter's first lesson — and
// a retired id in `reviewed`/`explored` is dropped. An id that was never a
// lesson (retired or live) is still refused.
#[test]
fn a_progress_file_naming_a_retired_lesson_resumes_at_its_chapter() {
    let retired = vec![("trim".to_owned(), "edit".to_owned())];
    let file = saved(json!({
        "currentStepId": "trim",
        "reviewed": ["welcome", "trim"],
        "explored": ["trim", "preview"]
    }))
    .to_string();

    let read = parse_progress_file_with(file.as_bytes(), &retired).unwrap();

    assert_eq!(read.current_step_id.as_deref(), Some("select"));
    assert_eq!(read.reviewed, ["welcome"]);
    assert_eq!(read.explored, ["preview"]);
    for never in [
        saved(json!({ "currentStepId": "gone" })),
        saved(json!({ "reviewed": ["welcome", "gone"] })),
    ] {
        let err = parse_progress_file_with(never.to_string().as_bytes(), &retired).unwrap_err();
        assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    }
}
