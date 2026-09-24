//! Tests for `session_commands` — every one runs the `AppHandle`-free half
//! against a tempdir (`root` = the app's local data dir, staging beneath it
//! exactly as in production).

use super::*;
use vault_buddy_core::editor::commands::payloads::{DetachAudioPayload, RenamePayload};
use vault_buddy_screen::staging::StagedSidecar;

const BASE: &str = "2026-09-20 1432 Demo";

struct Fixture {
    root: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let f = Self {
            root: tempfile::tempdir().unwrap(),
        };
        std::fs::create_dir_all(f.staging()).unwrap();
        f
    }
    fn root(&self) -> &Path {
        self.root.path()
    }
    fn staging(&self) -> std::path::PathBuf {
        staging::staging_dir(self.root.path())
    }
    fn stage(&self, sidecar: &StagedSidecar) {
        staging::write_sidecar(&self.staging(), &sidecar.base, sidecar).unwrap();
        std::fs::write(
            self.staging().join(staging::mp4_file_name(&sidecar.base)),
            b"not really an mp4",
        )
        .unwrap();
    }
    fn sidecar(&self, base: &str) -> StagedSidecar {
        staging::read_sidecar(&self.staging().join(staging::sidecar_file_name(base))).unwrap()
    }
    fn project_dirs(&self) -> Vec<String> {
        match std::fs::read_dir(self.root().join("editor-projects")) {
            Ok(entries) => entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        }
    }
}

// Asymmetric on purpose: a non-16:9 width/height and a non-round duration,
// so a swapped axis or a dropped duration shows up.
fn sidecar(base: &str, vault_id: &str) -> StagedSidecar {
    StagedSidecar {
        base: base.to_string(),
        vault_id: vault_id.to_string(),
        source_title: "Demo window".into(),
        source_kind: "window".into(),
        inputs: vec!["mic-1".into()],
        duration_ms: 61_500,
        paused_ms: 0,
        width: 1600,
        height: 900,
        recorded_at: "2026-09-20T14:32:00Z".into(),
        timeline: None,
        webcam: None,
        extra: serde_json::Map::new(),
    }
}

// A01: the destination vault comes from the capture's OWN sidecar, never
// from any store/UI state — there is none in this fixture to come from.
#[test]
fn open_staged_resolves_the_vault_from_the_sidecar() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let opened = open_staged_in(f.root(), &f.staging(), BASE).expect("opens");
    assert_eq!(opened.envelope.project.destination.vault, "vaultA");
    assert_eq!(
        opened.sources[STAGED_ASSET_ID].locator,
        SourceLocator::Staging {
            base: BASE.to_string()
        }
    );
    assert!(opened.sources[STAGED_ASSET_ID].has_audio);
    assert_eq!(
        pinned_project(&f.sidecar(BASE)).as_deref(),
        Some(opened.envelope.project.id.as_str()),
        "the sidecar must be pinned to the project it opened"
    );
}

// F26: a capture recorded with a webcam migrates with a SECOND source
// record. Without it the webcam asset has no resolvable file: the preview
// (`editor_media_url`) cannot reach it, a portable package drops it, and
// `missing_media` reports a track that is sitting right there in staging.
#[test]
fn open_staged_registers_a_resolvable_webcam_source() {
    let f = Fixture::new();
    let mut s = sidecar(BASE, "vaultA");
    s.webcam = Some(staging::WebcamSidecar {
        file: staging::webcam_file_name(BASE),
        width: 640,
        height: 480,
        device_label: "Integrated Camera".into(),
        offset_ms: 120,
        extra: serde_json::Map::new(),
    });
    f.stage(&s);
    let webcam_path = f.staging().join(staging::webcam_file_name(BASE));
    std::fs::write(&webcam_path, b"webcam bytes, 27 long......").unwrap();

    let opened = open_staged_in(f.root(), &f.staging(), BASE).expect("opens");
    let project = &opened.envelope.project;
    let record = opened
        .sources
        .get(migrate::WEBCAM_ASSET_ID)
        .expect("the webcam asset has a source record");
    assert_eq!(
        record.locator,
        SourceLocator::StagingFile {
            base: BASE.to_string(),
            file: staging::webcam_file_name(BASE),
        }
    );
    assert_eq!(
        resolve_source(f.root(), &project.id, record),
        Some(webcam_path.clone()),
        "the record resolves to the real webcam file"
    );
    assert_eq!(record.size, 27);
    assert_eq!((record.width, record.height), (Some(640), Some(480)));
    assert!(record.has_video && !record.has_audio, "a video-only sink");
    assert!(missing_media(f.root(), project, &opened.sources).is_empty());

    let clip = project
        .clips
        .iter()
        .find(|c| c.asset_id == migrate::WEBCAM_ASSET_ID)
        .expect("the webcam was placed");
    assert_eq!(clip.start_ms, 120);
    // Synchronized: it runs to the capture's end from where it started.
    assert_eq!(clip.out_ms, 61_500 - 120);

    // The pin still names the capture, and only the capture.
    assert_eq!(opened.sources.len(), 2);
    assert_eq!(
        pinned_project(&f.sidecar(BASE)).as_deref(),
        Some(project.id.as_str())
    );
}

// A hand-edited sidecar naming a file the capture does not own is not a
// webcam track: it migrates as a plain capture rather than registering a
// source that would never resolve.
#[test]
fn open_staged_ignores_a_webcam_block_naming_a_file_the_capture_does_not_own() {
    let f = Fixture::new();
    let mut s = sidecar(BASE, "vaultA");
    s.webcam = Some(staging::WebcamSidecar {
        file: "../elsewhere.mp4".into(),
        width: 640,
        height: 480,
        device_label: String::new(),
        offset_ms: 0,
        extra: serde_json::Map::new(),
    });
    f.stage(&s);
    let opened = open_staged_in(f.root(), &f.staging(), BASE).expect("opens");
    assert_eq!(opened.sources.len(), 1, "{:?}", opened.sources.keys());
    assert!(opened
        .envelope
        .project
        .assets
        .iter()
        .all(|a| a.id != migrate::WEBCAM_ASSET_ID));
}

// A duplicated `editor:open` (or a second Edit click) must never mint a
// second project for the same capture.
#[test]
fn open_staged_twice_returns_the_same_project() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let first = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    let second = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(first.envelope.project.id, second.envelope.project.id);
    assert_eq!(
        f.project_dirs().len(),
        1,
        "one directory under editor-projects"
    );
}

// A crash after `create_project` but before `pin_staged` leaves a project
// and an unpinned capture; the reopen must ADOPT that project rather than
// mint a second one beside it.
#[test]
fn open_staged_crash_between_create_and_pin_is_adopted_on_reopen() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let first = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    let pid = first.envelope.project.id.clone();
    // Simulate the crash: the pin never landed.
    unpin_staged(&f.staging(), BASE, &pid).unwrap();
    assert_eq!(pinned_project(&f.sidecar(BASE)), None);

    let reopened = open_staged_in(f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(reopened.envelope.project.id, pid);
    assert_eq!(f.project_dirs(), vec![pid.clone()]);
    assert_eq!(pinned_project(&f.sidecar(BASE)), Some(pid), "re-pinned");
}

#[test]
fn open_staged_refuses_an_unsafe_base() {
    let f = Fixture::new();
    let e = open_staged_in(f.root(), &f.staging(), "../x")
        .err()
        .expect("refused");
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(f.project_dirs().is_empty());
}

#[test]
fn open_staged_reports_a_missing_sidecar_as_source_missing() {
    let f = Fixture::new();
    let e = open_staged_in(f.root(), &f.staging(), BASE)
        .err()
        .expect("refused");
    assert_eq!(e.code, EditorErrorCode::SourceMissing);
}

// F7: a recovered capture (and, separately, one with no known length)
// migrated as-is would become an empty, permanently-pinned project whose
// capture could never be discarded. Each fixture trips exactly ONE of the
// two conditions.
#[test]
fn open_staged_refuses_a_recovered_or_zero_duration_sidecar() {
    let recovered = {
        let mut s = sidecar(BASE, "vaultA");
        s.extra
            .insert("recovered".into(), serde_json::Value::Bool(true));
        s
    };
    let zero = StagedSidecar {
        duration_ms: 0,
        ..sidecar(BASE, "vaultA")
    };
    for (what, s) in [("recovered", recovered), ("zero-duration", zero)] {
        let f = Fixture::new();
        f.stage(&s);
        let e = open_staged_in(f.root(), &f.staging(), BASE)
            .err()
            .unwrap_or_else(|| panic!("a {what} sidecar must be refused"));
        assert_eq!(e.code, EditorErrorCode::InvalidRequest, "{what}");
        assert_eq!(e.message, UNKNOWN_LENGTH, "{what}");
        assert!(f.project_dirs().is_empty(), "{what}: no project created");
        assert_eq!(
            pinned_project(&f.sidecar(BASE)),
            None,
            "{what}: nothing pinned"
        );
    }
}

#[test]
fn open_staged_session_reuses_a_live_session_and_reports_missing_media() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let a = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(a.source_base.as_deref(), Some(BASE));
    assert!(a.missing.is_empty(), "the staged mp4 is on disk");
    assert_eq!(a.snapshot.persisted_revision, Some(a.snapshot.revision));
    let b = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    assert_eq!(
        a.snapshot.session_id, b.snapshot.session_id,
        "live session reused"
    );

    std::fs::remove_file(f.staging().join(staging::mp4_file_name(BASE))).unwrap();
    let missing = missing_media(
        f.root(),
        &a.project,
        &open_staged_in(f.root(), &f.staging(), BASE)
            .unwrap()
            .sources,
    );
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].asset_id, STAGED_ASSET_ID);
    assert_eq!(missing[0].name, "Demo window");
    assert_eq!(missing[0].expected_duration_ms, 61_500);
    assert_eq!(missing[0].expected_size, b"not really an mp4".len() as u64);
}

#[test]
fn execute_in_applies_to_the_named_session_and_refuses_a_stale_one() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let request = ExecuteRequest {
        session_id: open.snapshot.session_id.clone(),
        expected_revision: open.snapshot.revision,
        command_id: "cmd-1".into(),
        command: EditorCommand::Rename(RenamePayload {
            title: "Renamed".into(),
        }),
    };
    let p = execute_in(&state, f.root(), &request).unwrap();
    assert_eq!(p.project.title, "Renamed");
    assert_eq!(p.snapshot.revision, open.snapshot.revision + 1);
    assert_eq!(snapshot_in(&state, &open.snapshot.session_id).unwrap(), p);

    let gone = ExecuteRequest {
        session_id: "ses-unknown".into(),
        ..request
    };
    assert_eq!(
        execute_in(&state, f.root(), &gone).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
}

fn detach_request(open: &EditorOpenResult, command_id: &str) -> ExecuteRequest {
    ExecuteRequest {
        session_id: open.snapshot.session_id.clone(),
        expected_revision: open.snapshot.revision,
        command_id: command_id.into(),
        command: EditorCommand::DetachAudio(DetachAudioPayload {
            clip_id: open.project.clips[0].id.clone(),
            audio_track_id: None,
        }),
    }
}

// F15: `detachAudio` through `execute_in` succeeds only when the SESSION'S
// `sources.json` says the asset has audio -- proving the shell fills
// `CommandContext` from the project store, not just that the pure `apply`
// honours whatever set it is handed. A staged capture's record carries
// what its sidecar recorded (audio inputs or none), so the same wiring
// agrees with reality for a capture made with no audio device.
#[test]
fn session_execute_builds_the_context_from_sources_json() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let pid = open.project.id.clone();

    // Rewrite `sources.json` to say the capture is SILENT: the sidecar
    // still lists a microphone, so only a context read from sources.json
    // can refuse this.
    let mut sources = load_sources(f.root(), &pid).unwrap();
    sources.get_mut(STAGED_ASSET_ID).unwrap().has_audio = false;
    super::super::store_io::write_sources(f.root(), &pid, &sources).unwrap();
    let err = execute_in(&state, f.root(), &detach_request(&open, "cmd-silent")).unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("no audio"), "{}", err.message);

    sources.get_mut(STAGED_ASSET_ID).unwrap().has_audio = true;
    super::super::store_io::write_sources(f.root(), &pid, &sources).unwrap();
    let p = execute_in(&state, f.root(), &detach_request(&open, "cmd-audible")).unwrap();
    let linked = format!("{STAGED_ASSET_ID}-audio");
    assert!(p.project.assets.iter().any(|a| a.id == linked));
    assert!(p.project.clips[0].muted, "the capture's own clip is muted");

    // A capture recorded with NO audio input is registered silent at open.
    let silent_base = "2026-09-20 1500 Silent";
    let mut silent = sidecar(silent_base, "vaultA");
    silent.inputs.clear();
    f.stage(&silent);
    let open = open_staged_session(&state, f.root(), &f.staging(), silent_base).unwrap();
    let err = execute_in(&state, f.root(), &detach_request(&open, "cmd-none")).unwrap_err();
    assert!(err.message.contains("no audio"), "{}", err.message);
}

#[test]
fn close_keep_drops_the_session_but_not_the_project() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    close_in(&state, f.root(), &f.staging(), &sid, CloseDisposition::Keep).unwrap();
    assert_eq!(
        snapshot_in(&state, &sid).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
    assert!(lock_ignoring_poison(&state.by_project).is_empty());
    assert_eq!(f.project_dirs().len(), 1);
    assert!(pinned_project(&f.sidecar(BASE)).is_some());
}

// Discarding the EDIT must never delete what the user recorded (R6): the
// project goes, the pin is cleared, the staged capture stays.
#[test]
fn close_discard_project_removes_the_project_and_unpins_the_capture() {
    let f = Fixture::new();
    f.stage(&sidecar(BASE, "vaultA"));
    let state = EditorState::default();
    let open = open_staged_session(&state, f.root(), &f.staging(), BASE).unwrap();
    let sid = open.snapshot.session_id.clone();
    close_in(
        &state,
        f.root(),
        &f.staging(),
        &sid,
        CloseDisposition::DiscardProject,
    )
    .unwrap();
    assert!(f.project_dirs().is_empty(), "project directory removed");
    assert_eq!(pinned_project(&f.sidecar(BASE)), None, "pin cleared");
    assert!(
        f.staging().join(staging::mp4_file_name(BASE)).is_file(),
        "the recording survives"
    );
    assert_eq!(
        snapshot_in(&state, &sid).unwrap_err().code,
        EditorErrorCode::SessionGone
    );
}

#[test]
fn close_disposition_decodes_the_wire_spelling() {
    for (wire, want) in [
        ("\"keep\"", CloseDisposition::Keep),
        ("\"discardRecovery\"", CloseDisposition::DiscardRecovery),
        ("\"discardProject\"", CloseDisposition::DiscardProject),
    ] {
        assert_eq!(
            serde_json::from_str::<CloseDisposition>(wire).unwrap(),
            want
        );
    }
    assert!(serde_json::from_str::<CloseDisposition>("\"discard_project\"").is_err());
}

// Two `editor_open_staged` calls for the same UNPINNED capture (a double
// click, a duplicated `editor:open`) must not both miss the pin and the
// orphan scan and mint two projects — the second pin would win, leaving the
// first project an unreachable orphan. The open lock serializes them.
// Repeated, because the race window is only the scan-to-create gap.
#[test]
fn concurrent_opens_of_one_capture_mint_one_project() {
    for round in 0..20 {
        let f = Fixture::new();
        f.stage(&sidecar(BASE, "vaultA"));
        let state = EditorState::default();
        let barrier = std::sync::Barrier::new(2);
        let ids: Vec<String> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..2)
                .map(|n| {
                    std::thread::Builder::new()
                        .name(format!("editor-open-race-{n}"))
                        .spawn_scoped(s, || {
                            barrier.wait();
                            open_staged_session(&state, f.root(), &f.staging(), BASE)
                                .unwrap()
                                .project
                                .id
                        })
                        .unwrap()
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert_eq!(
            ids[0], ids[1],
            "round {round}: both opens must get one project"
        );
        assert_eq!(f.project_dirs().len(), 1, "round {round}: one directory");
    }
}

// A pin is a claim by a hand-editable sidecar; if it names a project whose
// sources belong to a DIFFERENT capture, opening this capture must not
// silently open (and later edit or discard) someone else's project.
#[test]
fn open_staged_refuses_a_pin_to_another_captures_project() {
    const OTHER: &str = "2026-09-21 0915 Other";
    let f = Fixture::new();
    f.stage(&sidecar(OTHER, "vaultB"));
    let other = open_staged_in(f.root(), &f.staging(), OTHER).unwrap();
    let other_id = other.envelope.project.id.clone();
    f.stage(&sidecar(BASE, "vaultA"));
    pin_staged(&f.staging(), BASE, &other_id).unwrap();

    let e = open_staged_in(f.root(), &f.staging(), BASE)
        .err()
        .expect("a cross-wired pin must be refused");
    assert_eq!(e.code, EditorErrorCode::InvalidProject);
    assert!(
        e.message.contains(BASE) && e.message.contains(OTHER),
        "{}",
        e.message
    );
    assert_eq!(f.project_dirs(), vec![other_id], "nothing new minted");
}
