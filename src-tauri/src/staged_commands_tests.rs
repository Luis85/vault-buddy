//! `staged_commands.rs`' tests, split out (final whole-branch review I2)
//! when the discard's open-lock fix needed room in the production half.

use super::*;

fn production_src() -> &'static str {
    include_str!("staged_commands.rs")
        .split("#[cfg(test)]")
        .next()
        .expect("the production prefix")
}

// Task 59 moved `screen:discarded`'s emitter here from the retired
// export module, and the rule came with it: the event goes through the
// ONE emitter that logs a failed send, from exactly one call site. A
// `let _ = app.emit(..)` is the most invisible swallowed error there is:
// the panel simply goes on offering Edit on a capture that is gone.
#[test]
fn screen_discarded_is_emitted_through_the_one_warning_emitter() {
    let src = production_src();
    assert!(!src.contains("let _ = app.emit"), "a swallowed emit");
    assert_eq!(src.matches("app.emit(").count(), 1, "one emit site");
    assert_eq!(src.matches("\"screen:discarded\"").count(), 1);
    let clear = include_str!("staging_commands.rs");
    let clear = clear.split("#[cfg(test)]").next().unwrap_or(clear);
    assert!(
        !clear.contains("app.emit(") && clear.contains("emit_discarded(&app,"),
        "the bulk clear must emit through this module's emitter"
    );
}

// Final review I2: the single discard read the pin and unlinked without
// `EditorState::open`, which `editor_open_staged` holds across its
// create-then-pin. A discard landing between the two deleted the only
// source of the project that open then registered.
#[test]
fn a_discard_waits_for_an_open_in_progress_and_then_sees_its_pin() {
    let dir = tempfile::tempdir().unwrap();
    let sidecar = staging::StagedSidecar {
        base: "A".into(),
        vault_id: "v1".into(),
        source_title: "Demo".into(),
        source_kind: "window".into(),
        duration_ms: 7_300,
        width: 1366,
        height: 768,
        recorded_at: "2026-09-21T10:00:00Z".into(),
        ..Default::default()
    };
    staging::write_sidecar(dir.path(), "A", &sidecar).unwrap();
    std::fs::write(dir.path().join(staging::mp4_file_name("A")), b"footage").unwrap();
    let open = Mutex::new(());
    let opening = open.lock().unwrap();
    let discarded = std::thread::scope(|scope| {
        let discarding = scope.spawn(|| discard_unpinned(&open, dir.path(), "A"));
        std::thread::sleep(std::time::Duration::from_millis(200));
        crate::editor::project_store::pin_staged(dir.path(), "A", "proj-a").unwrap();
        drop(opening);
        discarding.join().unwrap()
    });
    assert_eq!(
        discarded.unwrap_err(),
        "This capture is used by a tutorial project. Discard the project first."
    );
    assert!(dir.path().join(staging::mp4_file_name("A")).is_file());
}

// A capture that predates the editor has NO timeline field, and that is
// the whole capture, never an empty edit.
#[test]
fn an_absent_sidecar_timeline_is_the_whole_capture() {
    assert_eq!(timeline_from_sidecar(None, 5_000), Timeline::whole(5_000));
    assert!(timeline_from_sidecar(None, 5_000).is_untouched(5_000));
}

#[test]
fn a_staged_capture_is_summarised_as_edited_only_when_its_timeline_differs_from_the_whole() {
    // The SAME predicate the editor's migration keys on, surfaced to the
    // UI. Deriving "edited" from the FIELD'S PRESENCE would mark every
    // capture the phase-4 editor was ever opened on as edited: it wrote
    // a timeline on every operation and never wrote null.
    assert!(!summary_is_edited(None, 5_000));
    assert!(!summary_is_edited(
        Some(serde_json::json!({ "segments": [{"sourceStartMs": 0, "sourceEndMs": 5_000}] })),
        5_000
    ));
    assert!(summary_is_edited(
        Some(serde_json::json!({ "segments": [{"sourceStartMs": 1_000, "sourceEndMs": 5_000}] })),
        5_000
    ));
}

// REGRESSION (fix wave): a capture recovered by `screen_recovery` after
// an interrupted session carries `duration_ms: 0` — nothing on disk
// records the duration once the sidecar is gone. `Timeline::whole(0)` is
// the EMPTY timeline and an empty timeline is never `is_untouched`, so
// the naive predicate answered TRUE and every recovered capture was
// listed as "edited · 0:00". Unknown is not edited.
#[test]
fn a_capture_whose_source_duration_is_unknown_is_not_reported_as_edited() {
    assert!(!summary_is_edited(None, 0));
    // ...and not by accident of the timeline being absent either.
    assert!(!summary_is_edited(
        Some(serde_json::json!({ "segments": [] })),
        0
    ));
}

// A recovered capture must SAY it is recovered: the `recovered: true`
// marker `screen_recovery::minimal_sidecar` writes lives in the
// sidecar's flattened catch-all, and the DTO used to drop it — so the
// one surface the recovery sweep exists to feed hid the only fact the
// user needs (that this capture no longer knows its vault and cannot be
// saved).
#[test]
fn a_recovered_capture_is_marked_recovered_in_the_summary() {
    let mut s = sidecar_fixture();
    assert!(!summary_from_sidecar(&s).recovered, "an ordinary capture");
    s.extra
        .insert("recovered".into(), serde_json::Value::Bool(true));
    assert!(summary_from_sidecar(&s).recovered);
    // A non-boolean value (the sidecar is hand-editable) is not a claim.
    s.extra
        .insert("recovered".into(), serde_json::Value::String("yes".into()));
    assert!(!summary_from_sidecar(&s).recovered);
}

fn sidecar_fixture() -> staging::StagedSidecar {
    staging::StagedSidecar {
        base: "2026-09-20 1432 Demo".into(),
        vault_id: "v1".into(),
        source_title: "Demo".into(),
        source_kind: "screen".into(),
        inputs: Vec::new(),
        duration_ms: 5_000,
        paused_ms: 0,
        width: 1920,
        height: 1080,
        recorded_at: "2026-09-20T14:32:00Z".into(),
        timeline: None,
        ..Default::default()
    }
}

#[test]
fn the_summary_reports_the_output_duration_the_export_will_produce() {
    let trimmed =
        serde_json::json!({ "segments": [{"sourceStartMs": 1_000, "sourceEndMs": 4_000}] });
    assert_eq!(summary_output_duration_ms(Some(trimmed), 9_000), 3_000);
    assert_eq!(summary_output_duration_ms(None, 9_000), 9_000);
}

// NOTE — this test is NOT the discard guard's proof, and must not be
// read as one. It calls `is_safe_base` directly, so it would pass
// unchanged if `discard_staged_capture` dropped its guard entirely.
// `editor_commands` already owns tests for this function; the reason to
// restate the table beside the app's SECOND destructive command is that
// these seven strings are the ones a destructive path must refuse. The
// load-bearing test is the structural scan below — that one goes red
// when the guard is removed. Do not let this test's green stand in for
// that one's.
#[test]
fn the_strings_a_destructive_base_taker_must_refuse() {
    for bad in [
        "../obsidian",
        "a/b",
        ".hidden",
        "trailing.",
        "C:Windows",
        "COM1",
        "x ",
    ] {
        assert!(
            !crate::editor_commands::is_safe_base(bad),
            "{bad:?} must be refused before it becomes a path"
        );
    }
}

// F29 (Task 59): the phase-5 export and its `ExportState` reservation
// are retired, so there is no "being exported" to refuse on any more --
// a pin (R6) is the ONLY reason a discard is refused. The render and
// publish jobs never route through here: they work on the PROJECT's own
// copies, and a pinned capture is refused whole before a job matters.
// Structural first, because an `exporting` parameter nobody sets would
// compile, pass every value test with `None`, and read to the next
// author as a live guard.
#[test]
fn discard_conflict_no_longer_takes_an_exporting_flag() {
    let src = production_src();
    let at = src
        .find("pub(crate) fn discard_conflict(")
        .expect("discard_conflict must exist");
    let signature = &src[at..at + src[at..].find('{').expect("its body")];
    assert!(
        !signature.contains("exporting"),
        "discard_conflict still takes the retired export's flag: {signature}"
    );
    assert!(
        !src.contains("ExportState"),
        "the staged-capture surface still reads the retired export reservation"
    );
}

// R6, mutation check: drop the pin arm in `discard_conflict` and this
// test goes red because a pinned capture stops being refused.
#[test]
fn pinned_capture_cannot_be_discarded() {
    let msg = discard_conflict(Some("proj1")).expect("a pinned capture must refuse discard");
    assert_eq!(
        msg,
        "This capture is used by a tutorial project. Discard the project first."
    );
    assert!(discard_conflict(None).is_none());
}

#[test]
fn a_pinned_captures_summary_carries_its_project_id_camel_case() {
    let mut s = sidecar_fixture();
    s.extra.insert(
        "editorProjectId".into(),
        serde_json::Value::String("proj1".into()),
    );
    let dto = summary_from_sidecar(&s);
    assert_eq!(dto.project_id.as_deref(), Some("proj1"));
    let json = serde_json::to_value(&dto).unwrap();
    assert_eq!(json["projectId"], serde_json::json!("proj1"));
    assert_eq!(summary_from_sidecar(&sidecar_fixture()).project_id, None);
}

// A video is an ATTACHMENT and a note is a note, and Obsidian's `file`
// parameter treats them differently: it resolves an extensionless name
// as `<name>.md`. Dropping the extension unconditionally — the
// `open_task`/`open_recording` shape, which only ever opens markdown —
// would make the Open button beside a saved capture open the note, or
// nothing at all in a vault that opted out of notes.
#[test]
fn the_open_uri_keeps_a_videos_extension_and_drops_a_notes() {
    let vault = Path::new("/vault");
    assert_eq!(
        capture_file_param(Path::new("/vault/Screen Captures/2026/09/Demo.mp4"), vault).as_deref(),
        Some("Screen Captures/2026/09/Demo.mp4")
    );
    assert_eq!(
        capture_file_param(Path::new("/vault/Screen Captures/2026/09/Demo.md"), vault).as_deref(),
        Some("Screen Captures/2026/09/Demo")
    );
    assert_eq!(
        capture_file_param(Path::new("/elsewhere/Demo.mp4"), vault),
        None
    );
}

fn staged_sidecar(base: &str, recorded_at: &str) -> staging::StagedSidecar {
    staging::StagedSidecar {
        base: base.to_string(),
        vault_id: "v1".into(),
        source_title: "Figma".into(),
        source_kind: "screen".into(),
        inputs: vec!["Mic".into()],
        duration_ms: 9_000,
        paused_ms: 0,
        width: 1280,
        height: 720,
        recorded_at: recorded_at.to_string(),
        timeline: None,
        ..Default::default()
    }
}

/// A staged capture on disk: its sidecar and its video.
fn stage(dir: &Path, base: &str, recorded_at: &str) {
    staging::write_sidecar(dir, base, &staged_sidecar(base, recorded_at)).unwrap();
    std::fs::write(dir.join(staging::mp4_file_name(base)), b"footage").unwrap();
}

// The export temp is the THIRD file, and it is often the largest. A
// discard that removes the video and the sidecar but leaves
// `.<base>.export.mp4.part` strands a file keyed to a capture the user
// just told us to forget, until some later session's recovery sweep
// happens to reach it. A two-file assertion cannot see that.
#[test]
fn discarding_a_capture_removes_its_video_its_sidecar_and_any_export_temp() {
    let dir = tempfile::tempdir().unwrap();
    let base = "2026-09-20 1432 Demo";
    stage(dir.path(), base, "2026-09-20T14:32:00+02:00");
    let temp = dir.path().join(staging::export_part_file_name(base));
    std::fs::write(&temp, b"abandoned").unwrap();
    // A file that is NOT ours, so a discard that simply emptied the
    // directory would be caught rather than passing as thorough.
    let foreign = dir.path().join("notes.txt");
    std::fs::write(&foreign, b"keep me").unwrap();

    discard_staged_files(dir.path(), base).expect("the discard lands");

    assert!(!dir.path().join(staging::mp4_file_name(base)).exists());
    assert!(!dir.path().join(staging::sidecar_file_name(base)).exists());
    assert!(
        !temp.exists(),
        "the abandoned export temp survived the discard"
    );
    assert!(
        foreign.is_file(),
        "the discard deleted a file that was not ours"
    );
}

// "The path is clear" is success — the `delete_transcription_model`
// precedent. A capture whose files a sweep already removed must not
// leave the user with an error they cannot act on.
#[test]
fn discarding_a_capture_that_is_already_gone_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    discard_staged_files(dir.path(), "2026-09-20 1432 Gone").expect("nothing to do is success");
}

// No-follow, `delete_task`'s discipline: a symlink where one of our
// files should be is refused outright, and NOTHING is unlinked — not
// even the files inspected before it — so a discard can never be half
// done.
#[cfg(unix)]
#[test]
fn a_symlinked_staged_file_is_refused_and_nothing_is_removed() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("precious.json");
    std::fs::write(&target, b"not ours").unwrap();
    let base = "2026-09-20 1432 Demo";
    std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"footage").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join(staging::sidecar_file_name(base))).unwrap();

    discard_staged_files(dir.path(), base).expect_err("a symlink leaf must be refused");

    assert!(target.is_file(), "the discard deleted through a symlink");
    assert!(
        dir.path().join(staging::mp4_file_name(base)).is_file(),
        "a refused discard removed the video anyway"
    );
}

#[test]
fn the_staged_list_is_newest_first_and_skips_what_it_cannot_trust() {
    let dir = tempfile::tempdir().unwrap();
    stage(dir.path(), "A", "2026-09-18T09:00:00+02:00");
    stage(dir.path(), "B", "2026-09-20T14:32:00+02:00");
    // A sidecar with no video: the resume row would open an editor on
    // nothing.
    staging::write_sidecar(
        dir.path(),
        "C",
        &staged_sidecar("C", "2026-09-21T09:00:00+02:00"),
    )
    .unwrap();
    // A sidecar whose own base disagrees with its file name: the row
    // would NAME one capture and ADDRESS another, and that field is
    // hand-editable.
    std::fs::write(
        dir.path().join("D.json"),
        serde_json::to_vec(&staged_sidecar("elsewhere", "2026-09-22T09:00:00+02:00")).unwrap(),
    )
    .unwrap();
    std::fs::write(dir.path().join(staging::mp4_file_name("D")), b"x").unwrap();
    // A name that could never have come from this app, and must never
    // become a path.
    std::fs::write(dir.path().join(".hidden.json"), b"{}").unwrap();

    let rows = staged_summaries(dir.path());
    let bases: Vec<&str> = rows.iter().map(|r| r.base.as_str()).collect();
    assert_eq!(bases, ["B", "A"], "newest first, and nothing untrusted");
    assert_eq!(rows[0].duration_ms, 9_000);
    assert_eq!(rows[0].output_duration_ms, 9_000);
    assert!(!rows[0].edited);
    assert_eq!(rows[0].source_title, "Figma");
    assert_eq!(rows[0].vault_id, "v1");
}

#[test]
fn an_edited_staged_capture_reports_its_edit_and_its_shorter_length() {
    let dir = tempfile::tempdir().unwrap();
    let base = "2026-09-20 1432 Demo";
    let mut s = staged_sidecar(base, "2026-09-20T14:32:00+02:00");
    s.timeline =
        Some(serde_json::json!({ "segments": [{"sourceStartMs": 1_000, "sourceEndMs": 4_000}] }));
    staging::write_sidecar(dir.path(), base, &s).unwrap();
    std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"footage").unwrap();

    let rows = staged_summaries(dir.path());
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].edited,
        "an edited capture was summarised as untouched"
    );
    assert_eq!(rows[0].output_duration_ms, 3_000);
    assert_eq!(rows[0].duration_ms, 9_000, "the source length must survive");
}

#[test]
fn every_new_command_is_registered_and_the_base_takers_are_guarded() {
    let lib = include_str!("lib.rs");

    let mut all: Vec<&str> = Vec::new();
    let mut checked: Vec<&str> = Vec::new();
    // One module since Task 59 retired its export-lifecycle sibling; a
    // list of (module, source) pairs still, so a command split out into
    // a new module is added here rather than scanned by nothing.
    for (module, src) in [("staged_commands", production_src())] {
        // Matched by PREFIX, not against the literal `#[tauri::command]`: an
        // attribute written with arguments — `#[tauri::command(rename_all =
        // "snake_case")]`, a shape this codebase already uses — is invisible
        // to a literal match, so a command added in it would be scanned by
        // nothing at all. The same evasion was mutation-proven against
        // `editor_commands`' sibling scan.
        for (offset, _) in src.match_indices("#[tauri::command") {
            let rest = &src[offset..];
            // `cargo fmt` closes every top-level item with a brace in column
            // 0 and indents everything inside one, so this is an exact body
            // boundary. An open-ended slice is the latent form of the bug
            // that let one command's guard satisfy another's assertion in
            // phase 4.
            let body = match rest.find("\n}\n") {
                Some(i) => &rest[..i + 3],
                None => rest,
            };
            let signature = &body[..body.find('{').unwrap_or(body.len())];
            let name = signature
                .split("fn ")
                .nth(1)
                .and_then(|s| s.split('(').next())
                .expect("a #[tauri::command] must declare a function");
            all.push(name);
            // Registered, or the frontend cannot call it at all — the half
            // of "this command exists" that a scan of this file alone
            // cannot see.
            assert!(
                lib.contains(&format!("{module}::{name},")),
                "{name} is not in lib.rs's generate_handler! list"
            );
            if !signature.contains("base: String") {
                continue;
            }
            let guard = body.find("is_safe_base(&base)").unwrap_or_else(|| {
                panic!(
                    "{name} accepts a base from the frontend but does not refuse \
                     an unsafe one before it becomes a path"
                )
            });
            // Presence is not refusal: a guard whose body only logs passes a
            // `contains` check while validating nothing. Bounded by the
            // four-space dedent `cargo fmt` guarantees for a block at
            // function-body level, because every one of these logs the
            // refused base with a `{base:?}` interpolation and so contains
            // its own `}` first.
            let end = body[guard..]
                .find("\n    }")
                .map(|i| guard + i)
                .unwrap_or(body.len());
            assert!(
                body[guard..end].contains("return Err("),
                "{name} checks is_safe_base but does not REFUSE an unsafe base"
            );
            checked.push(name);
        }
    }
    // EVERY command, not only the base-taking ones: the evasion that
    // matters is a new command taking the untrusted name under some
    // other parameter name (`capture: String`), which the loop above
    // skips and a base-takers-only list cannot see. A discard written in
    // that shape is an arbitrary DELETE.
    assert_eq!(
        all,
        [
            "discard_staged_capture",
            "list_staged_captures",
            "open_screen_capture",
        ],
        "this surface's command set changed; confirm whether the new command \
         turns frontend text into a path, then update this list"
    );
    assert_eq!(
        checked,
        ["discard_staged_capture"],
        "the set of commands taking a base changed; confirm the new one \
         guards it, then update this list"
    );
}
