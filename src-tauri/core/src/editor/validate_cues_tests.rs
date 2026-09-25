//! GAP-179 (closed by Task 38): caption presentation, caption text and
//! chapter-title bounds. The commands (`commands::captions`,
//! `commands::cues`) always refused these, but `validate_project` did not,
//! so a hand-edited `project.json` -- or, from Task 38 on, a project that
//! arrives inside an untrusted package (`editor::package::inspect_archive`)
//! -- could carry a font size, a blank caption or an over-long title no
//! command would ever write. Nested inside `validate_tests.rs` (the
//! `validate_transitions_tests.rs` precedent) so `base_project()` is in
//! scope and that file stays under the 800-line Rust cap.

use super::*;

/// `base_project` plus caption settings holding ONE cue on c1. The font
/// size is a raw JSON value so a fractional bound (17.9, 56.5) can be
/// written exactly as a hand-edited document would carry it.
fn with_caption(font_size: serde_json::Value, text: &str) -> Project {
    let mut project = base_project();
    project.captions = Some(
        serde_json::from_value(serde_json::json!({
            "enabled": true, "burn_in": true, "font_size": font_size,
            "position": "bottom", "background": true,
            "cues": [{"id": "q1", "clip_id": "c1", "start_ms": 100, "end_ms": 900, "text": text}]
        }))
        .unwrap(),
    );
    project
}

fn with_marker(title: &str) -> Project {
    let mut project = base_project();
    project.markers.push(
        serde_json::from_value(serde_json::json!({
            "id": "m1", "clip_id": "c1", "source_ms": 500, "title": title
        }))
        .unwrap(),
    );
    project
}

fn rejects(project: &Project, needle: &str) {
    let err = validate_project(project).expect_err("must be rejected");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert!(
        err.message.contains(needle),
        "expected {needle:?} in: {}",
        err.message
    );
}

// Regression (GAP-179): `workspace.schema.json`'s `captions.font_size` is
// `minimum 18, maximum 56` and `captions.js` refuses the same -- both ends
// probed a fraction outside, so a bound that is off by one integer, or a
// check applied to one end only, still fails.
#[test]
fn caption_font_size_is_bounded_to_the_schema() {
    rejects(&with_caption(serde_json::json!(17.9), "Hello"), "font_size");
    rejects(&with_caption(serde_json::json!(56.5), "Hello"), "font_size");
    for ok in [
        serde_json::json!(18),
        serde_json::json!(56),
        serde_json::json!(30),
    ] {
        let project = with_caption(ok.clone(), "Hello");
        assert!(
            validate_project(&project).is_ok(),
            "font_size {ok} must be accepted"
        );
    }
}

// Regression (GAP-179): the reference validator refuses a caption whose
// text is blank after trimming or longer than 500 characters. Counted in
// CHARACTERS, like `commands::captions::checked_text`: 500 two-byte `é`s
// (1000 bytes) are legal, so a byte-length check fails this test.
#[test]
fn caption_text_must_be_non_blank_and_within_the_limit() {
    rejects(&with_caption(serde_json::json!(30), "   \n\t"), "text");
    let too_long = "é".repeat(limits::MAX_CAPTION_TEXT_CHARS + 1);
    rejects(&with_caption(serde_json::json!(30), &too_long), "text");
    let at_limit = "é".repeat(limits::MAX_CAPTION_TEXT_CHARS);
    assert!(validate_project(&with_caption(serde_json::json!(30), &at_limit)).is_ok());
}

// Regression (GAP-179): `workspace.schema.json`'s `marker.title` is
// `maxLength 160` (`limits::MAX_TITLE_CHARS`); characters again, not bytes.
#[test]
fn marker_title_is_bounded() {
    let too_long = "é".repeat(limits::MAX_TITLE_CHARS + 1);
    rejects(&with_marker(&too_long), "title");
    let at_limit = "é".repeat(limits::MAX_TITLE_CHARS);
    assert!(validate_project(&with_marker(&at_limit)).is_ok());
}

// The defaults every command path writes (`commands::captions::
// default_settings`, used by addCaption, the caption import and the paste
// path since Task 36 moved it to the shared 30) must themselves validate,
// or the first caption a user adds would make the project unsaveable.
#[test]
fn the_command_caption_defaults_validate() {
    let mut project = base_project();
    let mut settings = crate::editor::commands::captions::default_settings();
    settings.cues.push(
        serde_json::from_value(serde_json::json!({
            "id": "q1", "clip_id": "c1", "start_ms": 100, "end_ms": 900, "text": "Hi"
        }))
        .unwrap(),
    );
    project.captions = Some(settings);
    assert!(validate_project(&project).is_ok());
}
