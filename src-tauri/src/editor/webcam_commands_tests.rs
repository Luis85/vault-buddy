//! `webcam_commands.rs`' tests: a real tempdir project, real chunk files,
//! and a FAKE `TakeIo` (no ffmpeg) whose "remux" copies the part and whose
//! "probe" answers fixed, asymmetric facts — plus one round trip through a
//! REAL ffmpeg that skips visibly without it.

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde_json::json;
use tauri::http::{HeaderName, HeaderValue};
use vault_buddy_core::editor::commands::payloads::InsertClipPayload;
use vault_buddy_core::editor::commands::CommandContext;
use vault_buddy_core::editor::{
    AssetKind, EditorCommand, EditorSession, ExecuteRequest, Map, Num, Track, TrackKind,
};

use super::*;
use crate::editor::project_store::minimal_project;
use crate::editor::store_io::create_project;

const SESSION: &str = "ses-proj1";

/// The fake: `remux` copies the part verbatim (so the landed bytes are
/// provably the recorded ones); `probe` answers 640x360, 4.2 s, with audio.
#[derive(Default)]
struct FakeIo {
    no_ffmpeg: bool,
}

impl TakeIo for FakeIo {
    fn remux(&self, part: &Path, out: &Path) -> Result<(), EditorError> {
        if self.no_ffmpeg {
            return Err(err(
                EditorErrorCode::EncoderUnavailable,
                "ffmpeg is not installed",
            ));
        }
        std::fs::copy(part, out)
            .map(|_| ())
            .map_err(|e| internal(e.to_string()))
    }

    fn probe(&self, _path: &Path) -> Result<ProbeFacts, EditorError> {
        Ok(ProbeFacts {
            duration_ms: 4_200,
            width: Some(640),
            height: Some(360),
            has_video: true,
            has_audio: true,
        })
    }
}

fn video_track(id: &str) -> Track {
    Track {
        id: id.to_string(),
        kind: TrackKind::Video,
        name: id.to_string(),
        visible: true,
        locked: false,
        muted: false,
        solo: false,
        volume: Num::from(1),
        extra: Map::new(),
    }
}

struct Fixture {
    root: tempfile::TempDir,
    state: EditorState,
}

impl Fixture {
    /// A project holding the staged capture's `src` asset with one clip on
    /// `v1`, and an empty video track `v2` a take can be placed on.
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let mut project = minimal_project("proj1");
        let mut src = vault_buddy_core::editor::Asset {
            id: "src".into(),
            kind: AssetKind::Video,
            name: "Screen".into(),
            duration_ms: 61_500,
            width: Some(Num::from(1280)),
            height: Some(Num::from(720)),
            size: None,
            builtin: None,
            media_type: None,
            linked_asset: None,
            original_name: None,
            extra: Map::new(),
        };
        src.size = Some(1);
        project.assets.push(src);
        project.tracks.push(video_track("v1"));
        project.tracks.push(video_track("v2"));
        create_project(root.path(), &project, &BTreeMap::new()).unwrap();
        let mut session = EditorSession::resume(SESSION, project, 1);
        execute(
            &mut session,
            "cmd-seed",
            EditorCommand::InsertClip(InsertClipPayload {
                asset_id: "src".into(),
                track_id: "v1".into(),
                start_ms: 0,
                in_ms: 1_500,
                out_ms: 61_500,
            }),
        );
        lock_ignoring_poison(&state.sessions).insert(SESSION.to_string(), session);
        Self { root, state }
    }

    fn takes(&self) -> PathBuf {
        self.root
            .path()
            .join("editor-projects")
            .join("proj1")
            .join("takes")
    }

    fn begin(&self) -> String {
        begin_in(
            &self.state,
            self.root.path(),
            SESSION,
            "video/webm;codecs=vp9,opus",
            None,
        )
        .unwrap()
        .take_id
    }

    fn append(&self, take_id: &str, seq: u64, bytes: &[u8]) -> Result<(), EditorError> {
        append_in(&self.state, &at(take_id, seq), Ok(bytes))
    }

    fn finish(&self, io: &FakeIo, take_id: &str, last: u64) -> Result<TakeDto, EditorError> {
        finish_in(&self.state, self.root.path(), io, SESSION, take_id, last)
    }

    fn project(&self) -> vault_buddy_core::editor::Project {
        lock_ignoring_poison(&self.state.sessions)[SESSION]
            .project()
            .clone()
    }

    fn sources(&self) -> BTreeMap<String, SourceRecord> {
        load_sources(self.root.path(), "proj1").unwrap()
    }
}

fn execute(session: &mut EditorSession, command_id: &str, command: EditorCommand) {
    let no_audio = BTreeSet::new();
    let revision = session.snapshot().revision;
    session
        .execute(
            &ExecuteRequest {
                session_id: SESSION.into(),
                expected_revision: revision,
                command_id: command_id.into(),
                command,
            },
            &CommandContext {
                assets_with_audio: &no_audio,
            },
        )
        .unwrap();
}

fn at(take_id: &str, seq: u64) -> ChunkHeaders {
    ChunkHeaders {
        session_id: SESSION.into(),
        take_id: take_id.into(),
        seq,
    }
}

fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.insert(
            HeaderName::from_static(name),
            HeaderValue::from_str(value).unwrap(),
        );
    }
    map
}

// R10: no sample crosses JSON. A chunk sent as a JSON body (an array of
// byte values, say) is refused, writes nothing — and, like every append
// error, fails the take, so a later raw chunk cannot paper over the hole.
#[test]
fn append_rejects_json_bodies() {
    let f = Fixture::new();
    let take = f.begin();
    let json_body = InvokeBody::Json(json!([26, 69, 223, 163]));
    let chunk = raw_body(&json_body);
    let e = append_in(&f.state, &at(&take, 0), chunk).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(e.message.contains("raw bytes"), "{}", e.message);
    let part = f.takes().join(format!(".{take}.webm.part"));
    assert_eq!(std::fs::read(&part).unwrap(), b"", "nothing was written");

    let raw = InvokeBody::Raw(b"cluster".to_vec());
    let e = append_in(&f.state, &at(&take, 0), raw_body(&raw)).unwrap_err();
    assert!(e.message.contains("stopped recording"), "{}", e.message);
    assert_eq!(std::fs::read(&part).unwrap(), b"");
}

#[test]
fn chunk_headers_name_the_session_take_and_canonical_sequence() {
    let good = headers(&[
        (HEADER_SESSION, SESSION),
        (HEADER_TAKE, "take-abc123"),
        (HEADER_SEQ, "41"),
    ]);
    assert_eq!(
        chunk_headers(&good).unwrap(),
        ChunkHeaders {
            session_id: SESSION.into(),
            take_id: "take-abc123".into(),
            seq: 41,
        }
    );
    for bad in [
        headers(&[(HEADER_SESSION, SESSION), (HEADER_TAKE, "take-abc123")]),
        headers(&[(HEADER_SESSION, SESSION), (HEADER_SEQ, "1")]),
        headers(&[(HEADER_TAKE, "take-abc123"), (HEADER_SEQ, "1")]),
        headers(&[
            (HEADER_SESSION, SESSION),
            (HEADER_TAKE, "../x"),
            (HEADER_SEQ, "1"),
        ]),
        headers(&[
            (HEADER_SESSION, SESSION),
            (HEADER_TAKE, "take-abc123"),
            (HEADER_SEQ, "01"),
        ]),
    ] {
        assert_eq!(
            chunk_headers(&bad).unwrap_err().code,
            EditorErrorCode::InvalidRequest
        );
    }
}

// F35: while a screen (or audio) capture holds `CaptureGuard`, a take is
// refused natively, naming what to stop — and leaves no file and no take.
#[test]
fn begin_is_refused_during_a_screen_capture() {
    let f = Fixture::new();
    let e = begin_in(
        &f.state,
        f.root.path(),
        SESSION,
        "video/webm",
        Some(CaptureKind::Screen),
    )
    .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::DeviceUnavailable);
    assert_eq!(e.message, "Stop the screen recording first.");
    let e = begin_in(
        &f.state,
        f.root.path(),
        SESSION,
        "video/webm",
        Some(CaptureKind::Audio),
    )
    .unwrap_err();
    assert_eq!(e.message, "Stop the audio recording first.");
    assert!(
        !f.takes().exists(),
        "a refused take must not create the takes folder"
    );
    assert!(f.state.takes.open_takes(SESSION).is_empty());

    let take = f.begin();
    assert!(take.starts_with("take-"), "{take}");
    assert!(f.takes().join(format!(".{take}.webm.part")).is_file());
    assert_eq!(f.state.takes.open_takes(SESSION), vec![take]);
}

#[test]
fn begin_refuses_other_mime_types_and_unknown_sessions() {
    let f = Fixture::new();
    let e = begin_in(&f.state, f.root.path(), SESSION, "video/mp4", None).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    let e = begin_in(&f.state, f.root.path(), "ses-nope", "video/webm", None).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::SessionGone);
    assert!(!f.takes().exists());
}

// A09: no ffmpeg means no index — but the take is never lost. The raw
// bytes become `takes\<takeId>.webm`, recorded in `sources.json` and added
// as an asset, and the refusal names that asset so the UI can offer it.
#[test]
fn finish_without_ffmpeg_keeps_and_registers_the_raw_take() {
    let f = Fixture::new();
    let take = f.begin();
    let chunks: [&[u8]; 3] = [b"EBML-head", b"cluster-1", b"cluster-2!"];
    for (seq, chunk) in chunks.iter().enumerate() {
        f.append(&take, seq as u64, chunk).unwrap();
    }
    let e = f.finish(&FakeIo { no_ffmpeg: true }, &take, 2).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::EncoderUnavailable);
    assert_eq!(e.retained_asset_ids, Some(vec![take.clone()]));

    let kept = f.takes().join(format!("{take}.webm"));
    let recorded: Vec<u8> = chunks.concat();
    assert_eq!(std::fs::read(&kept).unwrap(), recorded);
    assert!(!f.takes().join(format!(".{take}.webm.part")).exists());

    let record = &f.sources()[&take];
    assert_eq!(
        record.locator,
        SourceLocator::Takes {
            file: format!("{take}.webm")
        }
    );
    let (_, sha) = copy_hashing(&mut &recorded[..], &mut io::sink()).unwrap();
    assert_eq!(record.sha256.as_deref(), Some(sha.as_str()));
    assert_eq!(record.size, recorded.len() as u64);
    assert_eq!(
        record.duration_ms, 0,
        "the length is unknown, never guessed"
    );

    let project = f.project();
    let asset = project.assets.iter().find(|a| a.id == take).unwrap();
    assert_eq!(asset.name, "Webcam take 1");
    assert_eq!(asset.kind, AssetKind::Video);
    assert!(f.state.takes.open_takes(SESSION).is_empty(), "it landed");
}

// A08 (shell half): a finished take is a NEW asset registered beside the
// capture — the `src` asset and its clip on `v1` are exactly as before.
#[test]
fn a_finished_take_is_indexed_registered_and_leaves_the_capture_alone() {
    let f = Fixture::new();
    let before = f.project();
    let take = f.begin();
    f.append(&take, 0, b"head").unwrap();
    f.append(&take, 1, b"tail").unwrap();
    let dto = f.finish(&FakeIo::default(), &take, 1).unwrap();
    assert_eq!(
        serde_json::to_value(&dto).unwrap(),
        json!({
            "takeId": take,
            "assetId": take,
            "durationMs": 4_200,
            "width": 640,
            "height": 360,
            "hasAudio": true,
        })
    );
    assert_eq!(
        std::fs::read(f.takes().join(format!("{take}.webm"))).unwrap(),
        b"headtail"
    );
    assert!(!f.takes().join(format!(".{take}.webm.part")).exists());
    assert!(!f.takes().join(format!(".{take}.remux.webm")).exists());
    assert_eq!(f.sources()[&take].width, Some(640));
    let after = f.project();
    assert_eq!(after.clips, before.clips, "no screen clip changes");
    assert_eq!(after.assets[0], before.assets[0], "src is untouched");
    assert_eq!(after.assets.len(), 2);
    assert_eq!(after.assets[1].duration_ms, 4_200);
}

#[test]
fn take_started_serializes_the_wire_shape() {
    let started = TakeStarted {
        take_id: "take-abc".into(),
    };
    assert_eq!(
        serde_json::to_value(&started).unwrap(),
        json!({"takeId": "take-abc"})
    );
}

// A finish that names the wrong last chunk changes nothing: the part and
// every accepted byte stay, and the right finish still lands. A refused
// chunk (a gap) fails the take but keeps what was accepted, so it can
// still be finished at the last ACCEPTED chunk.
#[test]
fn a_refused_finish_or_chunk_keeps_the_part_for_a_later_finish() {
    let f = Fixture::new();
    let take = f.begin();
    f.append(&take, 0, b"aa").unwrap();
    f.append(&take, 1, b"bb").unwrap();
    let e = f.finish(&FakeIo::default(), &take, 2).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    let part = f.takes().join(format!(".{take}.webm.part"));
    assert_eq!(std::fs::read(&part).unwrap(), b"aabb");

    let e = f.append(&take, 3, b"dd").unwrap_err();
    assert!(e.message.contains("out of order"), "{}", e.message);
    assert_eq!(
        std::fs::read(&part).unwrap(),
        b"aabb",
        "the gap wrote nothing"
    );
    assert!(
        f.append(&take, 2, b"cc").is_err(),
        "a failed take takes no more"
    );

    let dto = f.finish(&FakeIo::default(), &take, 1).unwrap();
    assert_eq!(dto.asset_id, take);
    assert_eq!(
        std::fs::read(f.takes().join(format!("{take}.webm"))).unwrap(),
        b"aabb"
    );
}

// A take a clip plays cannot be discarded (its file would vanish from under
// the timeline); once no clip uses it, discard removes its own file.
#[test]
fn discard_refuses_a_take_in_use() {
    let f = Fixture::new();
    let take = f.begin();
    f.append(&take, 0, b"only").unwrap();
    f.finish(&FakeIo::default(), &take, 0).unwrap();
    {
        let mut sessions = lock_ignoring_poison(&f.state.sessions);
        execute(
            sessions.get_mut(SESSION).unwrap(),
            "cmd-place",
            EditorCommand::InsertClip(InsertClipPayload {
                asset_id: take.clone(),
                track_id: "v2".into(),
                start_ms: 0,
                in_ms: 0,
                out_ms: 4_200,
            }),
        );
    }
    let e = discard_in(&f.state, SESSION, &take).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(e.message.contains("on the timeline"), "{}", e.message);
    let file = f.takes().join(format!("{take}.webm"));
    assert!(file.is_file(), "a refused discard removes nothing");

    {
        let mut sessions = lock_ignoring_poison(&f.state.sessions);
        execute(
            sessions.get_mut(SESSION).unwrap(),
            "cmd-undo",
            EditorCommand::Undo,
        );
    }
    discard_in(&f.state, SESSION, &take).unwrap();
    assert!(!file.exists());
    let e = discard_in(&f.state, SESSION, &take).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest, "the take is gone");
}

#[test]
fn discarding_a_recording_take_removes_its_part() {
    let f = Fixture::new();
    let take = f.begin();
    f.append(&take, 0, b"x").unwrap();
    discard_in(&f.state, SESSION, &take).unwrap();
    assert!(!f.takes().join(format!(".{take}.webm.part")).exists());
    assert!(f.state.takes.open_takes(SESSION).is_empty());
    assert!(f.append(&take, 1, b"y").is_err());
}

// Another session's take id is not this session's to touch.
#[test]
fn a_take_answers_only_to_its_own_session() {
    let f = Fixture::new();
    let take = f.begin();
    lock_ignoring_poison(&f.state.sessions).insert(
        "ses-other".into(),
        EditorSession::resume("ses-other", minimal_project("proj2"), 1),
    );
    let other = ChunkHeaders {
        session_id: "ses-other".into(),
        take_id: take.clone(),
        seq: 0,
    };
    assert!(append_in(&f.state, &other, Ok(b"x")).is_err());
    assert!(discard_in(&f.state, "ses-other", &take).is_err());
    assert!(f.takes().join(format!(".{take}.webm.part")).is_file());
}

// A closing session's unfinished take can never be finished: its part goes.
// A finished take's file stays with its asset.
#[test]
fn a_closing_session_removes_only_its_unfinished_parts() {
    let f = Fixture::new();
    let open = f.begin();
    f.append(&open, 0, b"x").unwrap();
    let done = f.begin();
    f.append(&done, 0, b"y").unwrap();
    f.finish(&FakeIo::default(), &done, 0).unwrap();
    f.state.takes.forget_session(SESSION);
    assert!(!f.takes().join(format!(".{open}.webm.part")).exists());
    assert!(f.takes().join(format!("{done}.webm")).is_file());
    assert!(discard_in(&f.state, SESSION, &done).is_err(), "forgotten");
}

fn run_ffmpeg(args: &[&str]) -> bool {
    crate::external_tool::tool_command("ffmpeg")
        .args(["-v", "error", "-y"])
        .args(args)
        .status()
        .is_ok_and(|s| s.success())
}

/// A09's other half against a REAL ffmpeg: a VP8/Opus WebM streamed in
/// ≤ 1 MiB chunks, finished through `FfmpegTakeIo`, lands indexed with its
/// own size and length. Skips VISIBLY without ffmpeg or its WebM encoders.
#[test]
fn a_real_webm_streamed_in_chunks_lands_indexed() {
    let src_dir = tempfile::tempdir().unwrap();
    let webm = src_dir.path().join("take.webm");
    if !run_ffmpeg(&[
        "-f",
        "lavfi",
        "-i",
        "testsrc=size=320x176:rate=15",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000",
        "-t",
        "2",
        "-c:v",
        "libvpx",
        "-c:a",
        "libopus",
        webm.to_str().unwrap(),
    ]) {
        eprintln!(
            "SKIP a_real_webm_streamed_in_chunks_lands_indexed: no ffmpeg with libvpx/libopus"
        );
        return;
    }
    let bytes = std::fs::read(&webm).unwrap();
    let f = Fixture::new();
    let take = f.begin();
    let mut seq = 0;
    for chunk in bytes.chunks(64 * 1024) {
        f.append(&take, seq, chunk).unwrap();
        seq += 1;
    }
    let dto = finish_in(
        &f.state,
        f.root.path(),
        &FfmpegTakeIo::default(),
        SESSION,
        &take,
        seq - 1,
    )
    .unwrap();
    assert_eq!((dto.width, dto.height), (320, 176));
    assert!(dto.has_audio);
    assert!((1_800..=2_300).contains(&dto.duration_ms), "{dto:?}");
    assert!(f.takes().join(format!("{take}.webm")).is_file());
}
