//! Tests for `package_import` — the IMPORT half (`editor_import_package`):
//! what is installed, what is refused, what a round trip preserves.
//! Fixtures: `package_test_support.rs`; the export half is
//! `package_commands_tests.rs`.

use serde_json::json;
use vault_buddy_core::editor::package_plan::PackageFormat;
use vault_buddy_core::editor::probe::{ImportKind, ProbeFacts};
use vault_buddy_core::editor::{new_product, EditorErrorCode};
use vault_buddy_core::sync_util::lock_ignoring_poison;
use vault_buddy_screen::staging;

use crate::editor::package_commands::export_envelope;
use crate::editor::package_import::{import_package_in, FfprobeProber};
use crate::editor::package_test_support::*;
use crate::editor::project_store::{project_dir, SourceLocator, SourceMediaKind};
use crate::editor::render_jobs::{products_in, read_ledger};
use crate::editor::session_commands::open_staged_session;
use crate::editor::store_io::{list_projects, load_project, load_sources};

#[test]
fn lightweight_import_lists_missing_media() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let chooser = FakeChooser::saving_to(out.path().join("Demo.json"));
    let receipt = export(&a, &session_id, &chooser, PackageFormat::Lightweight)
        .unwrap()
        .unwrap();
    assert_eq!(receipt.file_name, "Demo.vbproject.json");
    assert_eq!(receipt.format, PackageFormat::Lightweight);

    let b = Machine::new();
    let opened = import(&b, out.path().join("Demo.vbproject.json"))
        .unwrap()
        .unwrap();
    assert_eq!(opened.project, a.project(&session_id));
    assert_eq!(
        sorted_missing(&opened),
        ["img1", "src"],
        "the card needs no file"
    );
    let sources = load_sources(b.root(), &project_id).unwrap();
    assert_eq!(
        sources["src"].locator,
        SourceLocator::Media {
            file: "src.mp4".into()
        }
    );
    assert_eq!(
        sources["img1"].locator,
        SourceLocator::Media {
            file: "img1.png".into()
        }
    );
    assert_eq!(sources["img1"].size, IMAGE_BYTES.len() as u64);
    assert_eq!(sources["img1"].media_kind, SourceMediaKind::Image);
    let media = project_dir(b.root(), &project_id).unwrap().join("media");
    assert!(!media.join("src.mp4").exists() && !media.join("img1.png").exists());
}

#[test]
fn failed_import_installs_nothing() {
    let (a, session_id, _) = machine_a();
    let envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    // A well-formed archive whose SECOND media entry does not hash to what
    // the manifest says: the first extracts cleanly, then the check fails.
    let mut corrupt = packed("src", "media/src.mp4", SCREEN_BYTES);
    corrupt.sha = sha256_hex(b"not the packaged bytes");
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Corrupt.vbproject.zip");
    write_test_package(
        &path,
        &envelope,
        &[packed("img1", "media/img1.png", IMAGE_BYTES), corrupt],
        &[],
    );

    let b = Machine::new();
    let err = import(&b, path).expect_err("a corrupt entry refuses the import");
    assert_eq!(err.code, EditorErrorCode::InvalidProject, "{}", err.message);
    // By kind and position, never by name (S-11).
    assert_eq!(
        err.message,
        "The package's media file 2 does not match its manifest (size or SHA-256); the file is damaged."
    );
    assert_eq!(
        b.store_entries(),
        Vec::<String>::new(),
        "nothing installed, nothing left"
    );
    assert!(lock_ignoring_poison(&b.state.sessions).is_empty());
    assert!(list_projects(b.root()).is_empty());
}

// A retained product travels with its record: its file lands at
// `products\<productId>.mp4` (Task 46's one product path, where
// `media_commands` resolves it) and the record lands in the LEDGER
// (`products.json`), so the imported project lists it before any save. A
// record naming any other file -- here a plausible `take-one.mp4` -- refuses
// the whole import rather than landing somewhere else.
#[test]
fn a_retained_product_is_installed_as_its_id_and_recorded_in_the_ledger() {
    let (a, session_id, project_id) = machine_a();
    let mut envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    let rendered: &[u8] = b"a rendered product, 30 long...";
    envelope.record.products.push(new_product(
        &envelope.project,
        5,
        "prod1",
        "Take one",
        "take-one.mp4",
        1_000,
        None,
        &envelope.saved_at,
    ));
    let media = [
        packed("img1", "media/img1.png", IMAGE_BYTES),
        packed("src", "media/src.mp4", SCREEN_BYTES),
    ];
    let product = [packed("prod1", "products/prod1.mp4", rendered)];
    let out = tempfile::tempdir().unwrap();
    let bad = out.path().join("Foreign.vbproject.zip");
    write_test_package(&bad, &envelope, &media, &product);
    envelope.record.products[0].filename = "prod1.mp4".to_string();
    let good = out.path().join("Product.vbproject.zip");
    write_test_package(&good, &envelope, &media, &product);

    let b = Machine::new();
    let err = import(&b, bad).expect_err("a foreign product file name is refused");
    // The file name is the file author's text, never echoed (S-11).
    assert_eq!(
        err.message,
        "The file name of product 1 in the project file is not its id followed by .mp4."
    );
    assert_eq!(b.store_entries(), Vec::<String>::new());

    let opened = import(&b, good).unwrap().unwrap();
    assert_eq!(opened.snapshot.project_id, project_id);
    let products = project_dir(b.root(), &project_id).unwrap().join("products");
    assert_eq!(std::fs::read(products.join("prod1.mp4")).unwrap(), rendered);
    let ledger = read_ledger(b.root(), &project_id).unwrap();
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].filename, "prod1.mp4");
    let listed = products_in(&b.state, b.root(), &opened.snapshot.session_id).unwrap();
    assert!(listed[0].available, "the imported product plays");
}

#[test]
fn existing_project_id_imports_as_a_copy() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Copy.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();
    let original = std::fs::read(
        project_dir(a.root(), &project_id)
            .unwrap()
            .join("project.json"),
    )
    .unwrap();

    let opened = import(&a, path).unwrap().unwrap();
    let copy_id = opened.snapshot.project_id.clone();
    assert_ne!(copy_id, project_id, "the id already exists here");
    assert_ne!(
        opened.snapshot.session_id, session_id,
        "a new session, not the live one"
    );
    assert_eq!(opened.project.id, copy_id);
    let mut expected = a.project(&session_id);
    expected.id = copy_id.clone();
    assert_eq!(opened.project, expected);
    let (envelope, _) = load_project(a.root(), &copy_id).unwrap();
    assert_eq!(envelope.record.id, copy_id);
    assert_eq!(media_file(&a, &copy_id, "src.mp4"), SCREEN_BYTES);
    assert_eq!(
        std::fs::read(
            project_dir(a.root(), &project_id)
                .unwrap()
                .join("project.json")
        )
        .unwrap(),
        original,
        "the original project is untouched"
    );
    let mut listed: Vec<String> = list_projects(a.root())
        .into_iter()
        .map(|p| p.project_file_id)
        .collect();
    listed.sort();
    let mut both = vec![project_id, copy_id];
    both.sort();
    assert_eq!(listed, both);
}

// Review Important 1: the import used to set `hasAudio: true` for every
// non-image source, so a screen capture recorded with NO audio device came
// back able to detach an empty audio clip, the edit Task 27 exists to
// refuse. The exporting side's facts now travel with the file.
#[test]
fn a_silent_capture_still_refuses_detach_audio_after_a_portable_round_trip() {
    let a = Machine::new();
    a.stage_with(&[]);
    let opened = open_staged_session(&a.state, a.root(), &a.staging(), BASE).unwrap();
    let session_id = opened.snapshot.session_id.clone();
    detach_src(&a, &session_id, &opened.project).expect_err("refused on the recording machine");
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Silent.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();

    let b = Machine::new();
    let imported = import(&b, path).unwrap().unwrap();
    let pid = imported.snapshot.project_id.clone();
    let record = load_sources(b.root(), &pid).unwrap()["src"].clone();
    assert!(!record.has_audio, "the silent capture stays silent");
    assert_eq!((record.width, record.height), (Some(1600), Some(900)));
    let err = detach_src(&b, &imported.snapshot.session_id, &imported.project)
        .expect_err("detaching nothing is still refused after the round trip");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
}

// A file without facts (an older build's, another editor's) must never
// INVENT a sound track: a video's `hasAudio` defaults to false.
#[test]
fn a_project_file_without_source_facts_never_invents_audio() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Facts.vbproject.json");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Lightweight,
    )
    .unwrap()
    .unwrap();
    let with_facts = Machine::new();
    import(&with_facts, path.clone()).unwrap().unwrap();
    let sources = load_sources(with_facts.root(), &project_id).unwrap();
    assert!(
        sources["src"].has_audio,
        "a capture with a microphone keeps its sound"
    );
    assert_eq!(sources["img1"].media_kind, SourceMediaKind::Image);

    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    raw["record"]
        .as_object_mut()
        .unwrap()
        .remove("vaultBuddySourceFacts")
        .expect("the export carried the facts");
    let bare = out.path().join("Bare.vbproject.json");
    std::fs::write(&bare, serde_json::to_vec(&raw).unwrap()).unwrap();
    let without = Machine::new();
    import(&without, bare).unwrap().unwrap();
    let sources = load_sources(without.root(), &project_id).unwrap();
    assert!(!sources["src"].has_audio, "no fact, no invented audio");
    assert!(!sources["img1"].has_audio);
}

// Review Minor: untrusted package content must never land verbatim in the
// store: `project.json` carries the SANITIZED workspace, like
// `workspace.json` and like `editor_save_project`, and never the facts'
// transport key.
#[test]
fn the_stored_envelope_carries_only_the_sanitized_workspace() {
    let (a, session_id, project_id) = machine_a();
    let mut envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    envelope.workspace = json!({"theme": "light", "smuggled": {"script": "x"}});
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Dirty.vbproject.json");
    std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let b = Machine::new();
    import(&b, path).unwrap().unwrap();
    let (stored, _) = load_project(b.root(), &project_id).unwrap();
    assert!(
        stored.workspace.get("smuggled").is_none(),
        "{}",
        stored.workspace
    );
    assert_eq!(stored.workspace["theme"], "light");
    assert!(!stored.record.extra.contains_key("vaultBuddySourceFacts"));
}

// M-V2 (post-merge review): the import re-serialises the envelope PRETTY,
// so a compact file under the 8 MiB read bound (here: a large, deeply
// laid-out `extra` on the project) became a `project.json` past it -- an
// installed project no save, list or discard could read. The final file is
// now measured before anything is created.
#[test]
fn a_compact_file_whose_stored_form_is_too_large_installs_nothing() {
    let (a, session_id, _) = machine_a();
    let mut envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    let rows: Vec<serde_json::Value> = (0..400_000).map(|_| json!([[1]])).collect();
    envelope
        .project
        .extra
        .insert("padding".to_string(), serde_json::Value::Array(rows));
    let compact = serde_json::to_vec(&envelope).unwrap();
    let limit = vault_buddy_core::editor::limits::MAX_PROJECT_JSON_BYTES;
    assert!((compact.len() as u64) < limit, "{} bytes", compact.len());
    assert!(serde_json::to_vec_pretty(&envelope).unwrap().len() as u64 > limit);
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Padded.vbproject.json");
    std::fs::write(&path, compact).unwrap();

    let b = Machine::new();
    let err = import(&b, path).expect_err("its stored form is past the bound");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert_eq!(err.message, "This project file is too large to install.");
    assert_eq!(
        b.store_entries(),
        Vec::<String>::new(),
        "no project and no .importing directory"
    );
    assert!(lock_ignoring_poison(&b.state.sessions).is_empty());
}

/// What a real ffprobe would say about a silent 1280x720 screen recording.
fn silent_720p() -> ProbeFacts {
    ProbeFacts {
        duration_ms: 61_500,
        width: Some(1280),
        height: Some(720),
        has_video: true,
        has_audio: false,
    }
}

/// Machine A's project (a capture WITH a microphone: its carried facts say
/// `hasAudio: true`, 1600x900) exported as a portable file.
fn portable_from_machine_a() -> (tempfile::TempDir, std::path::PathBuf, String) {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Probe.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();
    (out, path, project_id)
}

fn probed_calls(prober: &FakeProber) -> Vec<(ImportKind, Vec<u8>)> {
    let mut calls = prober.calls.borrow().clone();
    calls.sort_by_key(|(_, bytes)| bytes.len());
    calls
}

// GAP-215: the facts a file carries are the package author's claim; for a
// file the package CARRIES, the extracted bytes are probed and the probe
// wins. A package claiming sound for a silent recording no longer gets
// Detach audio accepted (and a render that fails late in ffmpeg); and a
// lying size/kind can no longer choose the render's untouched-capture fast
// path (R1's identity plan reads these very width/height/kind facts).
#[test]
fn a_carried_file_is_reprobed_and_the_probe_outranks_its_carried_facts() {
    let (_out, path, project_id) = portable_from_machine_a();
    let b = Machine::new();
    let prober = FakeProber::answering(Some(silent_720p()));
    let imported = import_probed(&b, path, &prober).unwrap().unwrap();

    // Each EXTRACTED file was asked about, as the kind its asset is.
    assert_eq!(
        probed_calls(&prober),
        [
            (ImportKind::Image, IMAGE_BYTES.to_vec()),
            (ImportKind::Video, SCREEN_BYTES.to_vec()),
        ]
    );
    let sources = load_sources(b.root(), &project_id).unwrap();
    let src = &sources["src"];
    assert!(
        !src.has_audio,
        "the probe found no sound, whatever the file said"
    );
    assert!(src.has_video);
    assert_eq!((src.width, src.height), (Some(1280), Some(720)));
    assert_eq!(src.media_kind, SourceMediaKind::Video);
    assert_eq!(src.duration_ms, 61_500);
    assert_eq!(src.size, SCREEN_BYTES.len() as u64, "the extracted size");
    assert_eq!(
        src.sha256.as_deref(),
        Some(sha256_hex(SCREEN_BYTES).as_str())
    );
    let img = &sources["img1"];
    assert_eq!(img.media_kind, SourceMediaKind::Image);
    assert_eq!((img.width, img.height), (Some(1280), Some(720)));
    assert_eq!(
        img.duration_ms, 5_000,
        "an image's length is assigned, never probed"
    );
    let err = detach_src(&b, &imported.snapshot.session_id, &imported.project)
        .expect_err("the silent recording refuses Detach audio");
    assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{}", err.message);
}

// No ffmpeg (or a file ffprobe cannot read): the probe cannot say, and the
// carried facts stand -- today's behaviour, never a refused import.
#[test]
fn without_a_probe_the_carried_facts_stand() {
    let (_out, path, project_id) = portable_from_machine_a();
    let b = Machine::new();
    let prober = FakeProber::absent();
    import_probed(&b, path, &prober).unwrap().unwrap();
    assert_eq!(prober.calls.borrow().len(), 2, "it was asked");
    let src = &load_sources(b.root(), &project_id).unwrap()["src"];
    assert!(src.has_audio, "the carried microphone stands");
    assert_eq!((src.width, src.height), (Some(1600), Some(900)));
}

// A placeholder's media is not in the file: there is nothing to probe, and
// it keeps what the file carried (a reconnect re-probes it, Task 40).
#[test]
fn a_placeholder_is_never_probed_and_keeps_its_carried_facts() {
    let (a, session_id, project_id) = machine_a();
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Light.vbproject.json");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Lightweight,
    )
    .unwrap()
    .unwrap();
    let b = Machine::new();
    let prober = FakeProber::answering(Some(silent_720p()));
    import_probed(&b, path, &prober).unwrap().unwrap();
    assert!(prober.calls.borrow().is_empty(), "no file, no probe");
    let src = &load_sources(b.root(), &project_id).unwrap()["src"];
    assert!(src.has_audio);
    assert_eq!((src.width, src.height), (Some(1600), Some(900)));
}

// GAP-182 (a): a file WITHOUT carried facts (an older build's, another
// editor's) used to read every video as silent until a reconnect; a file
// the package carries is now probed, so its real sound track is known.
#[test]
fn a_carried_file_without_facts_takes_its_probed_sound() {
    let (a, session_id, project_id) = machine_a();
    // The bare envelope: the facts are attached only by the export itself.
    let envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    assert!(!envelope.record.extra.contains_key("vaultBuddySourceFacts"));
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Bare.vbproject.zip");
    write_test_package(
        &path,
        &envelope,
        &[
            packed("img1", "media/img1.png", IMAGE_BYTES),
            packed("src", "media/src.mp4", SCREEN_BYTES),
        ],
        &[],
    );
    let b = Machine::new();
    let sound = ProbeFacts {
        has_audio: true,
        ..silent_720p()
    };
    import_probed(&b, path, &FakeProber::answering(Some(sound)))
        .unwrap()
        .unwrap();
    let src = &load_sources(b.root(), &project_id).unwrap()["src"];
    assert!(
        src.has_audio,
        "the probed sound track, not the no-facts default"
    );
}

// The probe goes through the media import's own rule (`settle_av_import`):
// a "video" whose bytes hold only sound is recorded as AUDIO, and a probe
// that rule refuses (no measurable length) says nothing, so the carried
// facts stand.
#[test]
fn a_probe_is_settled_by_the_media_import_rule() {
    let (_out, path, project_id) = portable_from_machine_a();
    let sound_only = ProbeFacts {
        width: None,
        height: None,
        has_video: false,
        has_audio: true,
        ..silent_720p()
    };
    let b = Machine::new();
    import_probed(&b, path.clone(), &FakeProber::answering(Some(sound_only)))
        .unwrap()
        .unwrap();
    let src = &load_sources(b.root(), &project_id).unwrap()["src"];
    assert_eq!(src.media_kind, SourceMediaKind::Audio);
    assert!(!src.has_video && src.has_audio);
    assert_eq!((src.width, src.height), (None, None));

    let no_length = ProbeFacts {
        duration_ms: 0,
        ..silent_720p()
    };
    let c = Machine::new();
    import_probed(&c, path, &FakeProber::answering(Some(no_length)))
        .unwrap()
        .unwrap();
    let src = &load_sources(c.root(), &project_id).unwrap()["src"];
    assert!(src.has_audio, "an unsettled probe keeps the carried facts");
    assert_eq!((src.width, src.height), (Some(1600), Some(900)));
}

// The PRODUCTION prober against a real ffprobe: machine A's capture is a
// real, SILENT 320x180 video, while its sidecar (a microphone, 1600x900)
// makes the export carry `hasAudio: true`. After the import the record is
// what ffprobe measured. Skips VISIBLY without ffmpeg (a skip proves
// nothing).
#[test]
fn the_production_prober_measures_a_real_silent_video() {
    let ffmpeg_ok = |args: &[&str]| {
        crate::external_tool::tool_command("ffmpeg")
            .args(["-v", "error", "-y"])
            .args(args)
            .status()
            .is_ok_and(|s| s.success())
    };
    if !ffmpeg_ok(&["-version"]) {
        crate::editor::test_announce::announce_skip(
            "the_production_prober_measures_a_real_silent_video: no ffmpeg on PATH",
        );
        return;
    }
    let (a, session_id, project_id) = machine_a();
    let capture = a.staging().join(staging::mp4_file_name(BASE));
    assert!(ffmpeg_ok(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=320x180:rate=10",
        "-t",
        "2",
        "-c:v",
        "mpeg4",
        capture.to_str().unwrap(),
    ]));
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Real.vbproject.zip");
    export(
        &a,
        &session_id,
        &FakeChooser::saving_to(path.clone()),
        PackageFormat::Portable,
    )
    .unwrap()
    .unwrap();

    let b = Machine::new();
    let prober = FfprobeProber::default();
    import_package_in(&b.state, b.root(), &FakeChooser::opening(path), &prober)
        .unwrap()
        .unwrap();
    let src = &load_sources(b.root(), &project_id).unwrap()["src"];
    assert!(!src.has_audio, "ffprobe found no sound track");
    assert!(src.has_video);
    assert_eq!((src.width, src.height), (Some(320), Some(180)));
    assert!(
        (1_900..=2_200).contains(&src.duration_ms),
        "{}",
        src.duration_ms
    );
}

// Carried from hardening Task 5: a lightweight file that does not parse is
// refused in fixed wording -- serde's message would quote the author's
// value (a bidi control included) back into the dialog.
#[test]
fn an_unparsable_project_file_is_refused_without_echoing_it() {
    let (a, session_id, _) = machine_a();
    let envelope =
        export_envelope(&a.state, a.root(), &session_id, a.revision(&session_id)).unwrap();
    let mut raw = serde_json::to_value(&envelope).unwrap();
    raw["record"]["revision"] = json!("evil\u{202E}txt");
    let out = tempfile::tempdir().unwrap();
    let path = out.path().join("Odd.vbproject.json");
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    let b = Machine::new();
    let err = import(&b, path).expect_err("refused");
    assert_eq!(err.code, EditorErrorCode::InvalidProject);
    assert_eq!(err.message, "The project file is not valid.");
    assert!(b.store_entries().is_empty());
}
