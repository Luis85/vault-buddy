//! `media_import.rs`'s tests: a real tempdir project and a FAKE `ImportIo`
//! (no ffprobe) whose "probe" reads the copied file's own bytes — so the
//! outcome is decided by what the pipeline actually copied, never by the
//! original's name.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use vault_buddy_core::editor::probe::ImportKind;
use vault_buddy_core::editor::{
    AssetKind, EditorCommand, EditorErrorCode, EditorSession, ExecuteRequest, MediaType,
};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::*;
use crate::editor::media_jobs::tests::CollectingSink;
use crate::editor::media_jobs::{JobKind, JobProgressDto, JobReporter};
use crate::editor::project_store::minimal_project;
use crate::editor::store_io::create_project;

const SESSION: &str = "ses-proj1";

/// A reader that fails, as a source on a vanishing USB stick would —
/// chained after real bytes, so the `.part` has content when it dies.
struct Breaks;
impl Read for Breaks {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("the device was removed"))
    }
}

/// The fake: `VIDEO:<ms>` / `AUDIO:<ms>` bytes probe as that stream with
/// that length (1280x720 for video); anything else is damaged. A source
/// whose name contains `broken` fails partway through its copy.
#[derive(Default)]
struct FakeIo<'a> {
    no_ffmpeg: bool,
    /// Set on the FIRST probe — a user pressing Cancel mid-batch.
    cancel_on_probe: Option<Arc<AtomicBool>>,
    /// Closes the session on the probe of a file named `closes.mp4` — the
    /// editor closed while the import was still running.
    close_session_on: Option<&'a EditorState>,
}

impl ImportIo for FakeIo<'_> {
    fn av_ready(&self) -> Result<(), EditorError> {
        if self.no_ffmpeg {
            return Err(err(
                EditorErrorCode::EncoderUnavailable,
                "ffmpeg is not installed, so video and audio cannot be imported.",
            ));
        }
        Ok(())
    }

    fn probe_av(&self, path: &Path, _kind: ImportKind) -> Result<ProbeFacts, EditorError> {
        if let Some(flag) = &self.cancel_on_probe {
            flag.store(true, Ordering::SeqCst);
        }
        let text = std::fs::read_to_string(path).unwrap_or_default();
        if let (Some(state), "CLOSE") = (self.close_session_on, text.as_str()) {
            lock_ignoring_poison(&state.sessions).clear();
        }
        let (tag, ms) = text.split_once(':').unwrap_or(("", ""));
        let duration_ms: u64 = ms.trim().parse().unwrap_or(0);
        match tag {
            "VIDEO" => Ok(ProbeFacts {
                duration_ms,
                width: Some(1280),
                height: Some(720),
                has_video: true,
                has_audio: true,
            }),
            "AUDIO" => Ok(ProbeFacts {
                duration_ms,
                width: None,
                height: None,
                has_video: false,
                has_audio: true,
            }),
            _ => Err(unsupported(
                "ffprobe could not read the file; it may be damaged.",
            )),
        }
    }

    fn open_source(&self, path: &Path) -> io::Result<Box<dyn Read>> {
        if path.to_string_lossy().contains("broken") {
            // Real bytes first, THEN the failure — so the `.part` exists
            // with content when the copy dies.
            return Ok(Box::new(io::Read::chain(&b"VIDEO:4000"[..], Breaks)));
        }
        Ok(Box::new(File::open(path)?))
    }
}

/// A 640x360 PNG: signature + a complete IHDR chunk (asymmetric, so a
/// swapped width/height would show).
fn png_bytes() -> Vec<u8> {
    let mut b = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    b.extend_from_slice(&13u32.to_be_bytes());
    b.extend_from_slice(b"IHDR");
    b.extend_from_slice(&640u32.to_be_bytes());
    b.extend_from_slice(&360u32.to_be_bytes());
    b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    b
}

struct Fixture {
    root: tempfile::TempDir,
    originals: tempfile::TempDir,
    state: EditorState,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let state = EditorState::default();
        let project = minimal_project("proj1");
        create_project(root.path(), &project, &BTreeMap::new()).unwrap();
        lock_ignoring_poison(&state.sessions).insert(
            SESSION.to_string(),
            EditorSession::resume(SESSION, project, 1),
        );
        Self {
            root,
            originals: tempfile::tempdir().unwrap(),
            state,
        }
    }

    fn original(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.originals.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    /// Run one import job to its end; returns every message it sent.
    fn import(&self, io: &FakeIo, cancel: &AtomicBool, files: &[PathBuf]) -> Vec<JobProgressDto> {
        let (job_id, _) = lock_ignoring_poison(&self.state.jobs).register(SESSION, JobKind::Import);
        let sink = CollectingSink::default();
        let reporter = JobReporter::new(&self.state.jobs, &sink, SESSION, &job_id, JobKind::Import);
        let job = ImportJob {
            state: &self.state,
            root: self.root.path(),
            session_id: SESSION,
            io,
            cancel,
        };
        run_import(&job, files, reporter);
        sink.messages()
    }

    fn media_files(&self) -> Vec<String> {
        let dir = project_dir(self.root.path(), "proj1")
            .unwrap()
            .join("media");
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn assets(&self) -> Vec<Asset> {
        lock_ignoring_poison(&self.state.sessions)[SESSION]
            .project()
            .assets
            .clone()
    }

    fn revision(&self) -> u64 {
        lock_ignoring_poison(&self.state.sessions)[SESSION]
            .snapshot()
            .revision
    }
}

fn terminal(messages: &[JobProgressDto]) -> &JobTerminal {
    messages
        .last()
        .and_then(|m| m.terminal.as_ref())
        .expect("the last message is the terminal")
}

fn per_file_names(t: &JobTerminal) -> Vec<&str> {
    t.per_file
        .as_ref()
        .unwrap()
        .iter()
        .map(|p| p.name.as_str())
        .collect()
}

// F-02's own acceptance: "valid items remain when another item fails". A
// damaged video, an unsupported type and a good video/audio/image in ONE
// batch: the three good files land (graph, `media\`, `sources.json` with a
// hash each), the two bad ones are per-file errors naming only the file,
// and the damaged file's copy does not survive its failed probe.
#[test]
fn a_failing_file_does_not_discard_the_successful_ones() {
    let fx = Fixture::new();
    let files = [
        fx.original("good.mp4", b"VIDEO:4000"),
        fx.original("corrupt.mov", b"garbage"),
        fx.original("notes.txt", b"hello"),
        fx.original("still.png", &png_bytes()),
        fx.original("voice.mp3", b"AUDIO:3000"),
    ];
    let messages = fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);

    let last = messages.last().unwrap();
    assert_eq!(last.phase, JobPhase::Complete);
    let t = terminal(&messages);
    assert_eq!(t.asset_ids.as_ref().unwrap().len(), 3);
    assert_eq!(per_file_names(t), ["corrupt.mov", "notes.txt"]);
    for row in t.per_file.as_ref().unwrap() {
        assert!(
            !row.error.contains(&*fx.originals.path().to_string_lossy()),
            "a per-file error must never carry a path: {}",
            row.error
        );
    }

    let assets = fx.assets();
    let by_name = |n: &str| assets.iter().find(|a| a.name == n).unwrap().clone();
    let video = by_name("good.mp4");
    assert_eq!((video.kind, video.duration_ms), (AssetKind::Video, 4_000));
    let audio = by_name("voice.mp3");
    assert_eq!((audio.kind, audio.duration_ms), (AssetKind::Audio, 3_000));
    let image = by_name("still.png");
    assert_eq!(image.media_type, Some(MediaType::Image));
    assert_eq!(
        (
            image.width.map(|w| w.to_string()),
            image.height.map(|h| h.to_string())
        ),
        (Some("640".into()), Some("360".into()))
    );
    assert_eq!(image.size, Some(png_bytes().len() as u64));

    let sources = load_sources(fx.root.path(), "proj1").unwrap();
    assert_eq!(sources.len(), 3);
    for asset in &assets {
        let record = &sources[&asset.id];
        assert_eq!(record.sha256.as_ref().map(String::len), Some(64));
        assert!(
            matches!(&record.locator, SourceLocator::Media { file } if file.starts_with(&asset.id))
        );
    }
    assert_eq!(fx.media_files().len(), 3, "{:?}", fx.media_files());
}

// Cancel stops FUTURE files only: the file being probed when Cancel
// arrived finishes and lands; the two after it never start.
#[test]
fn cancel_stops_future_files_and_keeps_finished_ones() {
    let fx = Fixture::new();
    let cancel = Arc::new(AtomicBool::new(false));
    let io = FakeIo {
        cancel_on_probe: Some(Arc::clone(&cancel)),
        ..FakeIo::default()
    };
    let files = [
        fx.original("one.mp4", b"VIDEO:1000"),
        fx.original("two.mp4", b"VIDEO:2000"),
        fx.original("three.mp4", b"VIDEO:3000"),
    ];
    let messages = fx.import(&io, &cancel, &files);

    assert_eq!(messages.last().unwrap().phase, JobPhase::Cancelled);
    let t = terminal(&messages);
    assert_eq!(t.asset_ids.as_ref().unwrap().len(), 1);
    assert!(t.per_file.as_ref().unwrap().is_empty());
    let assets = fx.assets();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].name, "one.mp4");
    assert_eq!(fx.media_files().len(), 1);
}

// A batch is ONE `AddAssets`: one revision step, and one Undo removes every
// asset it added.
#[test]
fn a_batch_is_one_undo_step() {
    let fx = Fixture::new();
    let before = fx.revision();
    let files = [
        fx.original("a.mp4", b"VIDEO:1000"),
        fx.original("b.png", &png_bytes()),
    ];
    fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);
    assert_eq!(fx.assets().len(), 2);
    assert_eq!(fx.revision(), before + 1, "two files, one edit");

    let mut sessions = lock_ignoring_poison(&fx.state.sessions);
    let session = sessions.get_mut(SESSION).unwrap();
    let snap = session.snapshot();
    assert!(snap.can_undo);
    let no_audio = std::collections::BTreeSet::new();
    session
        .execute(
            &ExecuteRequest {
                session_id: SESSION.into(),
                expected_revision: snap.revision,
                command_id: "cmd-undo".into(),
                command: EditorCommand::Undo,
            },
            &vault_buddy_core::editor::commands::CommandContext {
                assets_with_audio: &no_audio,
            },
        )
        .unwrap();
    assert!(session.project().assets.is_empty());
}

// A copy that dies partway leaves no `.part` (and no copy) behind, and the
// other file in the batch still lands.
#[test]
fn no_part_file_survives_a_failed_copy() {
    let fx = Fixture::new();
    let files = [
        fx.original("fine.mp4", b"VIDEO:1000"),
        fx.original("broken.mp4", b"VIDEO:4000"),
    ];
    let messages = fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);

    assert_eq!(per_file_names(terminal(&messages)), ["broken.mp4"]);
    let media = fx.media_files();
    assert_eq!(media.len(), 1, "{media:?}");
    assert!(media.iter().all(|n| !n.ends_with(".part")), "{media:?}");
    assert_eq!(fx.assets().len(), 1);
    assert_eq!(load_sources(fx.root.path(), "proj1").unwrap().len(), 1);
}

// The frontend drops a non-increasing sequence and ignores everything after
// a terminal — both only safe if Rust sends strictly increasing sequences
// and exactly ONE terminal, last.
#[test]
fn progress_sequence_strictly_increases_and_ends_with_exactly_one_terminal() {
    let fx = Fixture::new();
    let files = [
        fx.original("a.mp4", b"VIDEO:1000"),
        fx.original("bad.mp4", b"nope"),
        fx.original("c.png", &png_bytes()),
    ];
    let messages = fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);

    assert!(messages.len() >= 2);
    for pair in messages.windows(2) {
        assert!(pair[1].sequence > pair[0].sequence, "{messages:?}");
    }
    let terminals = messages.iter().filter(|m| m.terminal.is_some()).count();
    assert_eq!(terminals, 1, "{messages:?}");
    let (last, rest) = messages.split_last().unwrap();
    assert!(last.terminal.is_some() && last.phase.is_terminal());
    assert!(rest.iter().all(|m| !m.phase.is_terminal()));
    assert!(messages
        .iter()
        .all(|m| m.session_id == SESSION && m.job_id == messages[0].job_id));
    // The registry holds what the channel last said (reconcile's source).
    let rows = lock_ignoring_poison(&fx.state.jobs).records_for(SESSION);
    assert_eq!(rows.last().unwrap().phase, last.phase);
    assert_eq!(rows.last().unwrap().terminal, last.terminal);
}

// No ffmpeg: every video/audio file is a per-file `encoderUnavailable`
// that NAMES ffmpeg, and is never copied; an image (sniffed natively)
// still imports.
#[test]
fn missing_ffmpeg_fails_only_video_and_audio_and_names_ffmpeg() {
    let fx = Fixture::new();
    let io = FakeIo {
        no_ffmpeg: true,
        ..FakeIo::default()
    };
    let files = [
        fx.original("clip.mp4", b"VIDEO:1000"),
        fx.original("still.png", &png_bytes()),
    ];
    let messages = fx.import(&io, &AtomicBool::new(false), &files);
    let t = terminal(&messages);
    assert_eq!(per_file_names(t), ["clip.mp4"]);
    assert!(t.per_file.as_ref().unwrap()[0].error.contains("ffmpeg"));
    assert_eq!(fx.media_files().len(), 1);
    assert_eq!(fx.assets()[0].name, "still.png");
}

// A file whose bytes are not an image, wearing an image name, is refused by
// the sniff — the name is only a hint.
#[test]
fn an_image_name_on_non_image_bytes_is_refused() {
    let fx = Fixture::new();
    let files = [fx.original("fake.png", b"VIDEO:1000")];
    let messages = fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);
    assert_eq!(per_file_names(terminal(&messages)), ["fake.png"]);
    assert!(fx.media_files().is_empty());
    assert!(fx.assets().is_empty());
}

// The session ended under the import (the second file's probe closes it):
// nothing to add the batch to, so the job FAILS and the FIRST file's copy
// and source record are rolled back rather than orphaned.
#[test]
fn a_batch_whose_session_ends_mid_import_fails_and_rolls_back() {
    let fx = Fixture::new();
    let io = FakeIo {
        close_session_on: Some(&fx.state),
        ..FakeIo::default()
    };
    let files = [
        fx.original("a.mp4", b"VIDEO:1000"),
        fx.original("closes.mp4", b"CLOSE"),
    ];
    let messages = fx.import(&io, &AtomicBool::new(false), &files);
    assert_eq!(messages.last().unwrap().phase, JobPhase::Failed);
    let t = terminal(&messages);
    assert_eq!(
        t.error.as_ref().map(|e| e.code),
        Some(EditorErrorCode::SessionGone)
    );
    assert_eq!(t.asset_ids.as_deref(), Some(&[][..]));
    assert!(fx.media_files().is_empty(), "{:?}", fx.media_files());
    assert!(load_sources(fx.root.path(), "proj1").unwrap().is_empty());
}

// A session that is already gone when the job starts fails at once,
// copying nothing.
#[test]
fn an_import_for_a_closed_session_fails_before_copying() {
    let fx = Fixture::new();
    lock_ignoring_poison(&fx.state.sessions).clear();
    let files = [fx.original("a.mp4", b"VIDEO:1000")];
    let messages = fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);
    assert_eq!(messages.len(), 1, "{messages:?}");
    assert_eq!(messages[0].phase, JobPhase::Failed);
    assert!(fx.media_files().is_empty());
}

// Fix round 1: a per-file error carries a DISPLAY name and a message, never
// an app-internal path. An unreadable `sources.json` (here a directory
// wearing its name) used to surface `store_io`'s "Cannot read <full path>"
// verbatim; the path goes to the log instead, and the copy is removed.
#[test]
fn an_unreadable_source_registry_never_puts_a_path_in_a_per_file_error() {
    let fx = Fixture::new();
    let sources = project_dir(fx.root.path(), "proj1")
        .unwrap()
        .join("sources.json");
    std::fs::remove_file(&sources).unwrap();
    std::fs::create_dir(&sources).unwrap();
    let files = [fx.original("clip.mp4", b"VIDEO:1000")];
    let messages = fx.import(&FakeIo::default(), &AtomicBool::new(false), &files);

    let t = terminal(&messages);
    assert_eq!(per_file_names(t), ["clip.mp4"]);
    let error = &t.per_file.as_ref().unwrap()[0].error;
    let root = fx.root.path().to_string_lossy();
    assert!(!error.contains(&*root), "path leaked: {error}");
    assert!(
        !error.contains("sources.json"),
        "internal file named: {error}"
    );
    assert!(fx.assets().is_empty());
    assert!(fx.media_files().is_empty(), "{:?}", fx.media_files());
}
