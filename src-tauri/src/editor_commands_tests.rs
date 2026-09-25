//! Tests for `editor_commands` — split out at the 800-line Rust cap (Task 37
//! Part B), the `session_commands`/`session_commands_tests` precedent. `use
//! super::*;` below is what makes this file see everything
//! `editor_commands.rs` itself imports, including `crate::editor::store_io`.

use super::*;
use crate::editor::{project_store, EditorState};
use vault_buddy_screen::staging::StagedSidecar;

fn sidecar(base: &str) -> StagedSidecar {
    StagedSidecar {
        base: base.to_string(),
        vault_id: "v1".into(),
        source_title: "Screen 1".into(),
        source_kind: "screen".into(),
        inputs: vec!["Mic".into()],
        duration_ms: 42_000,
        paused_ms: 0,
        width: 1920,
        height: 1080,
        recorded_at: "2026-09-20T10:00:00Z".into(),
        timeline: None,
        ..Default::default()
    }
}

// A base name travels from the frontend, so it is untrusted input that
// becomes a PATH. `..` or a separator in it would read a file outside
// the staging directory — the `sanitize_title` lesson from phase 2,
// applied at the other end of the same pipe.
#[test]
fn a_base_that_could_escape_staging_is_refused() {
    assert!(is_safe_base("2026-09-20 1432 Figma walkthrough"));
    assert!(!is_safe_base("../../../etc/passwd"));
    assert!(!is_safe_base("a/b"));
    assert!(!is_safe_base("a\\b"));
    assert!(!is_safe_base(".."));
    assert!(!is_safe_base(""));
    // A leading dot is how our own in-progress `.part` files are named;
    // the editor must never be pointed at one.
    assert!(!is_safe_base(".hidden"));
    // An interior ".." is NOT rejected on its own (see `is_safe_base`'s
    // doc comment) — these three fixtures are the proof that dropping a
    // `contains("..")` check loses nothing: each is still refused, by a
    // DIFFERENT clause. ".." alone is caught by the leading-dot check,
    // "../../../etc/passwd" above by the separator check, and "a.." by
    // the trailing-dot check.
    assert!(!is_safe_base("a.."));
}

// I-1: `sanitize_title` maps the reserved CHARACTERS but never touches
// `.` (GAP-108's sibling gap), so a window titled with an ellipsis --
// "Loading...", "Saving...", "Installing... — Setup" — produces a base
// with interior dots. Before this fix `is_safe_base` refused every one
// of these forever: the capture staged correctly and could then never
// be opened, exported or discarded again. This is the test that stops
// someone re-adding a `contains("..")` check.
#[test]
fn a_base_with_interior_dots_from_an_ellipsis_title_is_accepted() {
    assert!(is_safe_base(
        "2026-09-20 1432 Saving... please wait - Figma"
    ));
}

// Windows silently strips a trailing dot or space from a file name, so a
// base carrying either would look up a file that was never actually
// written under that exact name (the `sanitize_title` trim exists for
// the same reason, at capture time rather than open time).
#[test]
fn a_base_with_a_trailing_dot_or_space_is_refused() {
    assert!(!is_safe_base("cap."));
    assert!(!is_safe_base("cap "));
}

// I-2: on Windows, `PathBuf::join`/`push` REPLACES `self` when the
// argument carries a prefix but no root (std's own doc), so
// `staging_dir.join("C:Windows.json")` is `C:Windows.json` --
// drive-C-relative, entirely outside staging. `cap:ads` is the NTFS
// alternate-data-stream form of the same hole: it addresses the
// `ads.json` STREAM of the file `cap`, not a file literally named
// `cap:ads.json`. `sanitize_title` already maps `:` to `-`, so no base
// this app itself produces can ever carry one.
#[test]
fn a_windows_drive_relative_or_alternate_stream_base_is_refused() {
    assert!(!is_safe_base("C:"));
    assert!(!is_safe_base("C:Windows"));
    assert!(!is_safe_base("cap:ads"));
}

// I-3 / GAP-108's other end: Windows resolves a path component whose
// STEM matches a reserved device name to the PHYSICAL device, regardless
// of directory or extension -- `dir.join("COM1.json")` is the COM1
// serial port, and `std::fs::read` on an open port blocks forever,
// hanging `load_staged_capture`'s `spawn_blocking` closure (and leaking
// the thread) instead of erroring. The match is case-insensitive and
// applies even behind a real-looking extension.
#[test]
fn a_reserved_device_name_is_refused() {
    assert!(!is_safe_base("CON"));
    assert!(!is_safe_base("con"));
    assert!(!is_safe_base("NUL"));
    assert!(!is_safe_base("COM1"));
    assert!(!is_safe_base("LPT1"));
    assert!(!is_safe_base("com1.foo"));
}

// T-1: no existing fixture carried a control character, so the
// `is_control` clause named in the doc comment was pinned by nothing.
#[test]
fn a_base_with_a_control_character_is_refused() {
    assert!(!is_safe_base("cap\tx"));
    assert!(!is_safe_base("\u{0}"));
}

// The detail the editor renders is derived, not echoed: every field the
// wire contract names is mapped from the sidecar, and `asset_path` is
// the file's own path — what `convertFileSrc` needs to build a URL the
// asset protocol can serve.
#[test]
fn the_detail_maps_every_sidecar_field_and_carries_the_staged_path() {
    let d = detail_from_sidecar(&sidecar("cap one"), Path::new("/staging/cap one.mp4"));
    assert_eq!(d.base, "cap one");
    assert_eq!(d.asset_path, "/staging/cap one.mp4");
    assert_eq!(d.duration_ms, 42_000);
    assert_eq!(d.width, 1920);
    assert_eq!(d.height, 1080);
    assert_eq!(d.source_title, "Screen 1");
    // T-3: `recordedAt` is part of the wire contract the brief listed
    // and was mapped but never asserted.
    assert_eq!(d.recorded_at, "2026-09-20T10:00:00Z");
    assert!(
        d.timeline.is_none(),
        "an untouched capture has no timeline yet"
    );
}

// A sidecar written by a previous editing session must come back as a
// timeline, or every crash would silently discard the edit it promised
// to preserve.
#[test]
fn a_saved_timeline_round_trips_into_the_detail() {
    let mut s = sidecar("cap");
    s.timeline = Some(serde_json::json!({
        "segments": [{"sourceStartMs": 0, "sourceEndMs": 1000}]
    }));
    let d = detail_from_sidecar(&s, Path::new("/staging/cap.mp4"));
    let t = d
        .timeline
        .expect("a saved timeline must survive the round trip");
    assert_eq!(t["segments"][0]["sourceEndMs"], 1000);
}

// P-5: `assetPath` must be a path `convertFileSrc` can turn into a URL
// the asset protocol resolves -- i.e. THE STAGED FILE'S OWN path. It
// carried the bare file name, which `convertFileSrc` percent-encodes
// onto the asset origin without joining anything, so the URL named no
// file on disk, matched no scope entry, and the preview stayed blank.
// Pinned at the CALL SITE, which is the only place it can break:
// `detail_from_sidecar` takes the path as a separate argument.
//
// The two halves are asserted separately on purpose. `parent == dir`
// alone would also hold for a `dir`-relative name on some platforms, and
// `is_absolute` alone would hold for any absolute path anywhere on the
// disk -- it is the pair that says "this exact file, inside staging".
#[test]
fn load_from_staging_dir_returns_the_staged_mp4s_own_absolute_path() {
    let dir = tempfile::tempdir().expect("tempdir");
    let base = "cap one";
    staging::write_sidecar(dir.path(), base, &sidecar(base)).expect("write sidecar");
    std::fs::write(dir.path().join(staging::mp4_file_name(base)), b"x").expect("write mp4");

    let detail = load_from_staging_dir(dir.path(), base).expect("load");
    let asset = Path::new(&detail.asset_path);
    assert!(
        asset.is_absolute(),
        "assetPath must be absolute: convertFileSrc joins nothing, so a bare name \
         resolves to no file and matches no scope entry"
    );
    assert_eq!(
        asset.parent(),
        Some(dir.path()),
        "assetPath must name a file inside the staging directory"
    );
    assert_eq!(
        asset.file_name(),
        Some(std::ffi::OsStr::new("cap one.mp4")),
        "assetPath must name THIS capture's mp4"
    );
}

// M-1: `read_sidecar` exists precisely because a sidecar can be
// hand-edited (its own doc says so), so `sidecar.base` is untrusted. A
// hand-edited base that disagrees with the file it lives in must not
// become the identity `load_staged_capture` hands back -- that identity
// is what the editor later hands to save and discard.
#[test]
fn a_sidecar_whose_base_disagrees_with_its_file_name_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = sidecar("cap");
    s.base = "../../evil".to_string();
    let json = serde_json::to_vec_pretty(&s).expect("serialize sidecar");
    std::fs::write(dir.path().join("cap.json"), json).expect("write sidecar");
    std::fs::write(dir.path().join("cap.mp4"), b"x").expect("write mp4");

    assert!(load_from_staging_dir(dir.path(), "cap").is_err());
}

// C-1: the save path READ with the validated request base but WROTE
// with `sidecar.base`, a field read off disk out of a file
// `read_sidecar`'s own doc says may be hand-edited. `staging`'s own
// tests pin that a base escaping the directory is refused; what this
// pins is the shell's contribution -- WHICH base this call site hands
// to the writer. The fixture is deliberately an ordinary, perfectly
// safe name: "other" trips no containment or `is_safe_base` clause, so
// nothing but the requested-vs-stored mismatch can fail it, and the
// failure it demonstrates survives containment entirely -- saving an
// edit to "cap" would create and own a DIFFERENT capture's sidecar.
#[test]
fn a_save_never_writes_to_a_name_other_than_the_one_it_was_asked_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut s = sidecar("cap");
    s.base = "other".to_string();
    std::fs::write(
        dir.path().join("cap.json"),
        serde_json::to_vec_pretty(&s).expect("serialize sidecar"),
    )
    .expect("write sidecar");

    let result = save_to_staging_dir(
        dir.path(),
        "cap",
        Some(serde_json::json!({"segments": [{"sourceStartMs": 0, "sourceEndMs": 1}]})),
    );

    assert!(result.is_err(), "a mismatched sidecar must not be written");
    assert!(
        !dir.path().join("other.json").exists(),
        "the save must address the capture it was asked for, never the \
         name the file on disk claims"
    );
}

// The ordinary path: a save lands on the requested sidecar, in place.
#[test]
fn a_save_rewrites_the_requested_sidecar_in_place() {
    let dir = tempfile::tempdir().expect("tempdir");
    staging::write_sidecar(dir.path(), "cap", &sidecar("cap")).expect("write sidecar");

    save_to_staging_dir(dir.path(), "cap", Some(serde_json::json!({"segments": []})))
        .expect("save");

    let back = staging::read_sidecar(&dir.path().join("cap.json")).expect("reads back");
    assert_eq!(back.timeline, Some(serde_json::json!({"segments": []})));
    assert_eq!(back.duration_ms, 42_000, "the rest of the sidecar survives");
}

// T-2: `is_safe_base` is well tested as a function above, but its
// APPLICATION was not -- removing the guard from a command body left
// the whole shell test suite green, because `AppHandle` makes a unit
// test of the commands themselves impossible. The
// `capture_exclusion`/`clear_active_screen` precedent: scan this file's
// own production source and fail if a command stops calling it.
//
// The scan is GENERIC over the commands rather than hardcoded to the
// ones that existed when it was written, because the hardcoded version
// failed exactly once it mattered: it named three commands and checked
// the last of them against a slice running to end-of-file, so when a
// fourth base-taking command was appended with its own guard, THAT
// guard satisfied the assertion about `load_staged_capture` -- the test
// stayed green with the load path unguarded (mutation-proven). Each
// body is now bounded by its own column-0 closing brace, so a guard in
// one command can never stand in for another's, and a command added
// later is scanned without anybody remembering to widen this.
#[test]
fn every_command_taking_a_base_guards_it_with_is_safe_base() {
    let src = include_str!("editor_commands.rs");
    let production = src.split("#[cfg(test)]").next().unwrap_or(src);

    let mut all: Vec<&str> = Vec::new();
    let mut checked: Vec<&str> = Vec::new();
    // Matched by PREFIX, not against the exact literal
    // `#[tauri::command]`: an attribute written with arguments --
    // `#[tauri::command(rename_all = "snake_case")]`, a shape this
    // codebase already uses -- is invisible to a literal match, so a
    // command added in it would be scanned by nothing at all
    // (mutation-proven: an unguarded `discard_staged_capture` in that
    // shape left this test green).
    for (offset, _) in production.match_indices("#[tauri::command") {
        let rest = &production[offset..];
        // `cargo fmt` closes every top-level item with a brace in
        // column 0 and indents everything inside one, so this is an
        // exact body boundary -- unlike a slice to the next command,
        // which would swallow any helper declared between them.
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
        if !signature.contains("base: String") {
            continue;
        }
        let guard = body.find("if !is_safe_base(&base) {").unwrap_or_else(|| {
            panic!(
                "{name} accepts a base from the frontend but does not refuse \
                 an unsafe one before it becomes a path"
            )
        });
        // Presence is not refusal: a guard whose body only logs passes
        // a `contains` check while validating nothing (mutation-proven).
        // The block cannot be bounded by its first `}` -- every one of
        // these logs the refused base with a `{base:?}` interpolation --
        // so it is bounded by the four-space dedent `cargo fmt`
        // guarantees for a block at function-body level.
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

    // Without this the scan would pass vacuously on a file whose
    // commands had all drifted out of the shape it matches -- the
    // failure mode a source scan is most prone to. It lists EVERY
    // command, not only the base-taking ones, because the evasion that
    // matters is a NEW command that takes the untrusted name under some
    // other parameter name (`capture: String`) and is therefore skipped
    // by the loop above and invisible to a base-takers-only list. Tasks
    // 5 and 6 add discard and export -- a discard written in that shape
    // is an arbitrary DELETE. Adding any command here must force a human
    // to decide whether it takes a base and guards it.
    //
    // Task 37 Part B: `open_project_editor` and `list_tutorial_projects`
    // joined the file, neither taking a `base: String` -- so `checked`
    // stays the same three commands, but `all` (every command, base-taking
    // or not) must widen, or a NEW command that DOES take an untrusted name
    // under some other parameter would sail past this scan unnoticed.
    assert_eq!(
        all,
        [
            "open_capture_editor",
            "open_project_editor",
            "take_editor_request",
            "list_tutorial_projects",
            "load_staged_capture",
            "save_capture_timeline"
        ],
        "this file's command set changed; confirm whether the new command \
         turns frontend text into a path, then update this list"
    );
    assert_eq!(
        checked,
        [
            "open_capture_editor",
            "load_staged_capture",
            "save_capture_timeline"
        ],
        "the set of commands taking a base changed; confirm the new one \
         guards it, then update this list"
    );
}

// The sidecar is read-modify-written, never rebuilt: everything except
// the timeline is the capture's own recorded truth (duration, inputs,
// the vault it belongs to), and a save that reconstructed those fields
// from what the editor happens to know would quietly lose whatever the
// editor does not carry.
#[test]
fn saving_a_timeline_preserves_every_other_sidecar_field() {
    let mut s = sidecar("cap");
    s.inputs = vec!["Mic".into(), "Speakers".into()];
    s.paused_ms = 7_000;
    s.extra
        .insert("exportedTo".into(), serde_json::json!("Work/cap.md"));
    let timeline = serde_json::json!({"segments": [{"sourceStartMs": 5, "sourceEndMs": 9}]});
    let updated = with_timeline(s.clone(), Some(timeline.clone()));

    // I-2: this asserted six of eleven fields, and a mutation that
    // rebuilt the struct while preserving those six -- blanking
    // `source_title`, `source_kind`, `width` and `height` -- left it
    // green. `width`/`height` are what phase 5's export encodes
    // against. Comparing the WHOLE struct is what stops the assertion
    // going stale the way a hand-listed six did: `..s` carries a field
    // added later automatically, so the next field to join the sidecar
    // is pinned here without anybody remembering to widen this.
    assert_eq!(
        updated,
        StagedSidecar {
            timeline: Some(timeline),
            webcam: None,
            ..s
        }
    );
}

// NO production caller passes `None` today, and that is deliberate
// rather than an oversight: "clearing means untouched" WAS the design,
// and it was removed (C-1) because it is true only for a capture opened
// unedited — on spec 10's Resume the editor is handed a previous
// session's edit, so undoing back to it cleared the field and told the
// exporter the recording had never been touched. `useEditorTimeline`
// therefore always writes the timeline, never `null`, and "untouched"
// has exactly one authority: `Timeline::is_untouched(source_duration_ms)`.
//
// The `Option` stays for phase 5's discard, and this test stays with it:
// it pins that clearing REMOVES the field rather than storing an empty
// segment list — a timeline Save refuses (spec 8.1), which is not the
// same thing as an absent one.
#[test]
fn clearing_a_timeline_removes_it_rather_than_storing_an_empty_one() {
    let mut s = sidecar("cap");
    s.timeline = Some(serde_json::json!({"segments": []}));
    let updated = with_timeline(s, None);
    assert!(updated.timeline.is_none());
}

// --- Task 37 Part B (F4): list_tutorial_projects / open_project_editor ---

// The wire pin: `EditorRequestKind` is what `take_editor_request` now
// returns instead of a bare `Option<String>` (global-constraints.md's
// Contract reference, "take_editor_request's widened {kind, value} return").
// A literal JSON pin, never a struct re-serialized against itself -- only a
// literal catches a missing `content = "value"`, which would nest the
// payload under a bare `"Staged"`/`"Project"` key instead.
#[test]
fn editor_request_stash_carries_staged_or_project() {
    let state = EditorRequest::default();
    *lock_ignoring_poison(&state.0) = Some(EditorRequestKind::Staged("cap one".to_string()));
    let taken = lock_ignoring_poison(&state.0).take();
    assert_eq!(
        taken,
        Some(EditorRequestKind::Staged("cap one".to_string()))
    );
    assert!(
        lock_ignoring_poison(&state.0).is_none(),
        "take() must leave the stash empty"
    );

    assert_eq!(
        serde_json::to_value(EditorRequestKind::Staged("cap one".to_string())).unwrap(),
        serde_json::json!({"kind": "staged", "value": "cap one"})
    );
    assert_eq!(
        serde_json::to_value(EditorRequestKind::Project("proj1".to_string())).unwrap(),
        serde_json::json!({"kind": "project", "value": "proj1"})
    );
}

// F4: the underlying call `list_tutorial_projects` wraps
// (`store_io::list_projects`) takes only a `&Path` -- no `&EditorState` at
// all -- so it structurally CANNOT open or register a session. Proven at
// the value level too: an `EditorState` built alongside the call, over a
// store that genuinely holds a project, comes back untouched.
#[test]
fn list_tutorial_projects_never_opens_or_touches_a_session() {
    let root = tempfile::tempdir().expect("tempdir");
    store_io::create_project(
        root.path(),
        &project_store::minimal_project("proj1"),
        &Default::default(),
    )
    .expect("seed a real project on disk");

    let state = EditorState::default();
    let rows = store_io::list_projects(root.path());

    assert_eq!(
        rows.len(),
        1,
        "the listing must reach the real project on disk, not an empty directory"
    );
    assert!(
        state.sessions.lock().unwrap().is_empty(),
        "list_tutorial_projects must never register a session"
    );
    assert!(
        state.by_project.lock().unwrap().is_empty(),
        "list_tutorial_projects must never index a project to a session"
    );
}

// F4: a structural assertion that Task 10's `authz_guard` file scan under
// `src-tauri/src/editor/` does not include, and was never meant to include,
// this file -- `editor_commands.rs` sits at `src/editor_commands.rs`, a
// SIBLING of that directory, so `list_tutorial_projects` needs no
// `window: WebviewWindow` parameter and no `require_editor_window` call.
//
// Mutation check (named in the task brief): routing it through
// `require_editor_window` anyway makes this test fail AND wrongly refuses
// every real panel call -- exactly the bug F4 exists to prevent.
#[test]
fn list_tutorial_projects_is_not_editor_window_scoped() {
    let editor_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("editor");
    let this_file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("editor_commands.rs");
    assert!(
        this_file.exists(),
        "editor_commands.rs must exist at src/editor_commands.rs"
    );
    assert!(
        !this_file.starts_with(&editor_dir),
        "editor_commands.rs must stay OUTSIDE src/editor/, or Task 10's authz_guard scan would \
         start forcing every command in it onto the editor window alone"
    );

    let src = include_str!("editor_commands.rs");
    let production = src.split("#[cfg(test)]").next().unwrap_or(src);
    let start = production
        .find("pub async fn list_tutorial_projects")
        .expect("list_tutorial_projects must exist");
    let rest = &production[start..];
    let sig_end = rest.find('{').unwrap_or(rest.len());
    let signature = &rest[..sig_end];
    assert!(
        !signature.contains("window: WebviewWindow")
            && !signature.contains("window: tauri::WebviewWindow"),
        "list_tutorial_projects must not take a WebviewWindow -- it is panel-callable, not \
         scoped to the editor window"
    );
    let body = match rest.find("\n}\n") {
        Some(i) => &rest[..i + 3],
        None => rest,
    };
    assert!(
        !body.contains("require_editor_window"),
        "list_tutorial_projects must never call require_editor_window -- that would wrongly \
         refuse every real panel call, exactly the bug F4 exists to prevent"
    );
}

// Task 37 Part B (F4), fix round 1 (review Important #4): `open_project_editor`
// is `open_capture_editor`'s own shape -- stash, THEN emit `editor:open`
// (rolling the stash back on a failed emit), THEN `show()` (rolling the
// stash back on a failed show too) -- so a Resume click can never show a
// blank editor with nothing staged, or leave a stale request a LATER drain
// would silently open. Neither command is unit-testable directly (both take
// `AppHandle`; see this file's own
// `every_command_taking_a_base_guards_it_with_is_safe_base` comment for
// why), so this is a structural scan of the production source, the same
// technique that test already uses.
//
// Fix round 1 extracted the shared sequence into one `stash_and_open`
// helper both commands call (review Important #4: the two command bodies
// used to duplicate it verbatim), so this test now asserts TWO things
// instead of one: each command's own body DELEGATES to the helper rather
// than re-duplicating the sequence, and the helper itself still does the
// stash/emit/show ordering and both rollbacks -- pinned ONCE, for both
// callers, instead of once per caller.
#[test]
fn open_project_editor_stashes_and_emits_like_open_capture_editor() {
    let src = include_str!("editor_commands.rs");
    let production = src.split("#[cfg(test)]").next().unwrap_or(src);

    fn bounded_body<'a>(production: &'a str, needle: &str) -> &'a str {
        let start = production
            .find(needle)
            .unwrap_or_else(|| panic!("{needle:?} not found in editor_commands.rs"));
        let rest = &production[start..];
        match rest.find("\n}\n") {
            Some(i) => &rest[..i + 3],
            None => rest,
        }
    }

    // MUTATION CHECK target: if either command stopped calling the shared
    // helper and went back to duplicating the sequence inline, this fails
    // naming which one -- the whole point of the fix round 4 extraction.
    for name in ["open_capture_editor", "open_project_editor"] {
        let body = bounded_body(production, &format!("pub fn {name}"));
        assert!(
            body.contains("stash_and_open("),
            "{name} must delegate the stash-then-emit-then-show sequence to the shared \
             stash_and_open helper rather than duplicating it inline"
        );
    }

    let body = bounded_body(production, "fn stash_and_open");
    let stash_at = body
        .find(".0) = Some(request)")
        .expect("stash_and_open must stash the request");
    let emit_at = body
        .find("emit_to(EDITOR_LABEL, EDITOR_OPEN_EVENT")
        .expect("stash_and_open must emit EDITOR_OPEN_EVENT to the editor window alone");
    let show_at = body
        .find("window.show()")
        .expect("stash_and_open must show the editor window");
    assert!(
        stash_at < emit_at && emit_at < show_at,
        "stash_and_open must stash, THEN emit, THEN show -- in that order"
    );

    // Both failure branches roll the stash back to None -- a failed emit or
    // a failed show must never leave a request a LATER drain would silently
    // open.
    let rollbacks = body.matches(".0) = None;").count();
    assert_eq!(
        rollbacks, 2,
        "stash_and_open must roll the stash back on BOTH the failed-emit and the failed-show \
         branches"
    );
}

// Task 58 fix round 1: the refusal is logged by its REASON (a capture base
// carries a window title, so the base itself is never logged), and the
// reason and `is_safe_base` can never disagree.
#[test]
fn an_unsafe_base_names_why_without_repeating_it() {
    for (base, reason) in [
        ("", "empty"),
        (".hidden", "leading or trailing dot or space"),
        ("a..", "leading or trailing dot or space"),
        ("trailing ", "leading or trailing dot or space"),
        ("a/b", "path separator"),
        ("a\\b", "path separator"),
        ("C:x", "drive or stream colon"),
        ("CON", "reserved device name"),
        ("a\u{7}b", "control character"),
    ] {
        assert_eq!(unsafe_base_reason(base), Some(reason), "{base:?}");
        assert!(!is_safe_base(base), "{base:?}");
    }
    let fine = "2026-09-20 1432 Saving... please wait";
    assert_eq!(unsafe_base_reason(fine), None);
    assert!(is_safe_base(fine));
}
