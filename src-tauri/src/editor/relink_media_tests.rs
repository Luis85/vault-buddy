//! `relink_media.rs`'s tests: a real tempdir project whose original is
//! missing, real picked files, and a FAKE `ImportIo` (no ffprobe) whose
//! "probe" reads the file's own bytes (`VIDEO:<ms>` / `AUDIO:<ms>`) — so
//! every verdict comes from what the picked file IS, never its name.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use vault_buddy_core::editor::import_io::copy_hashing;
use vault_buddy_core::editor::probe::{ImportKind, ProbeFacts};
use vault_buddy_core::editor::{EditorErrorCode, EditorSession, Project};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::*;
use crate::editor::media_import::ImportIo;
use crate::editor::project_store::{project_dir, SourceLocator, SourceMediaKind, SourceRecord};
use crate::editor::store_io::{create_project, load_sources};

const SESSION: &str = "ses-proj1";
const ASSET: &str = "a-talk";

/// `changes` in a picked file's name: its SECOND open (the copy) reads
/// other bytes than its first (the hash) — a file edited mid-reconnect.
#[derive(Default)]
struct FakeIo {
    opens: std::cell::Cell<u32>,
}

impl ImportIo for FakeIo {
    fn av_ready(&self) -> Result<(), EditorError> {
        Ok(())
    }

    fn probe_av(&self, path: &Path, _kind: ImportKind) -> Result<ProbeFacts, EditorError> {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let (tag, ms) = text.split_once(':').unwrap_or(("", ""));
        let duration_ms: u64 = ms.trim().parse().unwrap_or(0);
        match tag {
            "VIDEO" => Ok(ProbeFacts {
                duration_ms,
                width: Some(1920),
                height: Some(1080),
                has_video: true,
                has_audio: false,
            }),
            "AVIDEO" => Ok(ProbeFacts {
                duration_ms,
                width: Some(1920),
                height: Some(1080),
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
            _ => Err(EditorError::new(
                EditorErrorCode::UnsupportedMedia,
                "ffprobe could not read the file; it may be damaged.",
            )),
        }
    }

    fn open_source(&self, path: &Path) -> io::Result<Box<dyn Read>> {
        if path.to_string_lossy().contains("changes") {
            self.opens.set(self.opens.get() + 1);
            if self.opens.get() > 1 {
                return Ok(Box::new(&b"VIDEO:31000 but edited"[..]));
            }
        }
        Ok(Box::new(std::fs::File::open(path)?))
    }
}

/// The graph the project edits: one missing 31 s screen recording under a
/// clip (source 2–9 s) that carries an arrow cue and a chapter marker.
fn project() -> Project {
    serde_json::from_value(serde_json::json!({
        "schema": "vault-buddy-video-project/3",
        "id": "proj1",
        "title": "Talk",
        "canvas": { "width": 1280, "height": 720, "fps": 30 },
        "master_gain": 1,
        "assets": [{ "id": ASSET, "kind": "video", "name": "talk.mp4", "duration_ms": 31_000 }],
        "tracks": [{ "id": "t-video", "kind": "video", "name": "Video", "visible": true,
                     "locked": false, "muted": false, "solo": false, "volume": 1 }],
        "clips": [{ "id": "c-first", "asset_id": ASSET, "track_id": "t-video", "name": "First",
                    "start_ms": 0, "in_ms": 2_000, "out_ms": 9_000, "fade_in_ms": 0,
                    "fade_out_ms": 0, "fade_curve": "linear", "opacity": 1, "volume": 1,
                    "muted": false, "x": 0, "y": 0, "w": 1, "h": 1 }],
        "effects": [{ "id": "e-arrow", "clip_id": "c-first", "kind": "arrow", "start_ms": 2_500,
                      "end_ms": 4_000, "x": 0.25, "y": 0.4, "x2": 0.6, "y2": 0.7,
                      "color": "#ffcc00" }],
        "markers": [{ "id": "m-step", "clip_id": "c-first", "source_ms": 3_000, "title": "Step" }],
        "transitions": [],
        "destination": { "vault": "", "folder": "", "dated": false }
    }))
    .unwrap()
}

/// `"VIDEO:31000"` — the bytes every "same" original in these tests holds
/// (11 bytes, so a look-alike of another length differs in size too).
const ORIGINAL: &[u8] = b"VIDEO:31000";

fn missing_record() -> SourceRecord {
    SourceRecord {
        locator: SourceLocator::Media {
            file: format!("{ASSET}.mp4"),
        },
        sha256: None,
        size: ORIGINAL.len() as u64,
        duration_ms: 31_000,
        width: None,
        height: None,
        has_audio: true,
        has_video: true,
        media_kind: SourceMediaKind::Video,
        replaced_from: None,
    }
}

struct Fixture {
    root: tempfile::TempDir,
    picked: tempfile::TempDir,
    state: EditorState,
}

impl Fixture {
    fn new(record: SourceRecord) -> Self {
        let mut sources = BTreeMap::new();
        sources.insert(ASSET.to_string(), record);
        Self::with(project(), sources)
    }

    fn with(graph: Project, sources: BTreeMap<String, SourceRecord>) -> Self {
        let root = tempfile::tempdir().unwrap();
        create_project(root.path(), &graph, &sources).unwrap();
        let state = EditorState::default();
        lock_ignoring_poison(&state.sessions).insert(
            SESSION.to_string(),
            EditorSession::resume(SESSION, graph, 5),
        );
        Self {
            root,
            picked: tempfile::tempdir().unwrap(),
            state,
        }
    }

    fn pick(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.picked.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn relink(
        &self,
        files: &[PathBuf],
        confirm_replace: bool,
    ) -> Result<RelinkReportDto, EditorError> {
        self.relink_ids(&[ASSET], files, confirm_replace)
    }

    fn relink_ids(
        &self,
        ids: &[&str],
        files: &[PathBuf],
        confirm_replace: bool,
    ) -> Result<RelinkReportDto, EditorError> {
        let io = FakeIo::default();
        let job = RelinkJob {
            state: &self.state,
            root: self.root.path(),
            session_id: SESSION,
            io: &io,
        };
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        relink_in(&job, &ids, confirm_replace, files)
    }

    fn record(&self) -> SourceRecord {
        load_sources(self.root.path(), "proj1").unwrap()[ASSET].clone()
    }

    fn dir(&self) -> PathBuf {
        project_dir(self.root.path(), "proj1").unwrap()
    }

    fn media_files(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.dir().join("media"))
            .map(|rd| {
                rd.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    fn graph(&self) -> Project {
        lock_ignoring_poison(&self.state.sessions)[SESSION]
            .project()
            .clone()
    }

    fn revision(&self) -> u64 {
        lock_ignoring_poison(&self.state.sessions)[SESSION]
            .snapshot()
            .revision
    }
}

fn sha_of(bytes: &[u8]) -> String {
    let mut reader: &[u8] = bytes;
    copy_hashing(&mut reader as &mut dyn Read, &mut io::sink())
        .unwrap()
        .1
}

// The ordinary case: the one file that IS the original (same size, length
// and kind) is copied into `media\`, its record rewritten from its OWN probe
// and hash, the asset leaves the missing list, and the revision advances —
// while the graph (ids, clip, cue, marker) is exactly what it was.
#[test]
fn a_unique_match_is_copied_and_recorded_from_its_own_probe() {
    let f = Fixture::new(missing_record());
    let picked = f.pick("talk (from backup).mp4", ORIGINAL);

    let report = f.relink(&[picked], false).unwrap();

    assert_eq!(report.matched.len(), 1, "{report:?}");
    assert_eq!(report.matched[0].file, "talk (from backup).mp4");
    assert!(report.missing.is_empty(), "{:?}", report.missing);
    assert_eq!(f.media_files(), vec![format!("{ASSET}.mp4")]);
    let record = f.record();
    assert_eq!(record.sha256.as_deref(), Some(sha_of(ORIGINAL).as_str()));
    assert_eq!(
        (record.width, record.height, record.has_audio),
        (Some(1920), Some(1080), false),
        "facts come from the probe of the chosen file, never the old record"
    );
    assert_eq!(record.replaced_from, None);
    assert_eq!(f.graph(), project());
    assert_eq!(report.projection.snapshot.revision, 6);
    assert_eq!(f.revision(), 6);
}

// A19: two picked files are equally good. Nothing is copied, nothing is
// recorded, the revision does not move, and the asset stays missing until
// the user chooses.
#[test]
fn ambiguous_candidates_are_reported_and_nothing_changes() {
    let f = Fixture::new(missing_record());
    let a = f.pick("talk.mp4", ORIGINAL);
    let b = f.pick("talk (1).mp4", ORIGINAL);

    let report = f.relink(&[a, b], false).unwrap();

    assert!(report.matched.is_empty());
    assert_eq!(report.ambiguous.len(), 1);
    assert_eq!(report.ambiguous[0].files, vec!["talk.mp4", "talk (1).mp4"]);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(f.record(), missing_record());
    assert!(f.media_files().is_empty());
    assert_eq!(f.revision(), 5);
}

// Named case. A file that is not the original is reported with its reason
// and changes nothing; the SAME file with `confirmReplace` is copied in as
// a replacement — recorded with its own facts plus the identity it
// replaced — while the graph keeps its asset id and every link.
#[test]
fn mismatched_replace_requires_confirmation() {
    let f = Fixture::new(missing_record());
    let longer = f.pick("talk-extended.mp4", b"VIDEO:40000");

    let report = f.relink(std::slice::from_ref(&longer), false).unwrap();
    assert_eq!(report.mismatched.len(), 1, "{report:?}");
    assert_eq!(
        report.mismatched[0].reason,
        "different duration: 40.0 s vs 31.0 s"
    );
    assert!(report.replaced.is_empty());
    assert_eq!(f.record(), missing_record(), "unconfirmed: untouched");
    assert!(f.media_files().is_empty());
    assert_eq!(f.revision(), 5);

    let report = f.relink(&[longer], true).unwrap();
    assert!(report.mismatched.is_empty(), "{report:?}");
    assert_eq!(report.replaced.len(), 1);
    assert_eq!(report.replaced[0].file, "talk-extended.mp4");
    assert!(report.missing.is_empty());
    let record = f.record();
    assert_eq!(record.duration_ms, 40_000);
    assert_eq!(
        record.sha256.as_deref(),
        Some(sha_of(b"VIDEO:40000").as_str())
    );
    assert_eq!(
        record.replaced_from,
        Some(ReplacedFrom {
            sha256: None,
            size: ORIGINAL.len() as u64,
            duration_ms: 31_000,
            media_kind: SourceMediaKind::Video,
        })
    );
    assert_eq!(f.graph(), project(), "the graph keeps its ids and links");
    assert_eq!(f.revision(), 6);
}

// A confirmed replacement still may not break the edit: a shorter file
// could leave clips pointing past its end, and a different kind could not
// stand in at all. Refused, nothing written.
#[test]
fn a_replacement_that_would_break_the_edit_is_refused_even_confirmed() {
    let f = Fixture::new(missing_record());
    for (name, bytes) in [
        ("short.mp4", &b"VIDEO:12400"[..]),
        ("voice.m4a", &b"AUDIO:31000"[..]),
    ] {
        let picked = f.pick(name, bytes);
        let e = f.relink(&[picked], true).expect_err(name);
        assert_eq!(e.code, EditorErrorCode::InvalidRequest, "{name}: {e:?}");
        assert!(e.message.contains(name), "{name}: {}", e.message);
        assert!(!e.message.contains(f.picked.path().to_str().unwrap()));
    }
    assert_eq!(f.record(), missing_record());
    assert!(f.media_files().is_empty());
    assert_eq!(f.revision(), 5);
}

// GAP-176: a thumbnail is not tied to the file it was cut from, so a
// reconnect must purge the asset's cached thumbnails (and waveforms) —
// its own names only. Another asset's cache (`a-talk-2`), a stranger's
// file and a subdirectory stay.
#[test]
fn a_relink_purges_the_assets_cached_thumbnails_and_waveforms() {
    let f = Fixture::new(missing_record());
    let cache = f.dir().join("cache");
    std::fs::create_dir_all(&cache).unwrap();
    for name in [
        "a-talk-250.jpg",
        "a-talk-9750.jpg",
        "a-talk.peaks.400.json",
        "a-talk-2-250.jpg",
        "a-talk-2.peaks.400.json",
        "a-talk-250.jpg.bak",
        "notes.txt",
    ] {
        std::fs::write(cache.join(name), b"x").unwrap();
    }
    std::fs::create_dir(cache.join("a-talk-500.jpg")).unwrap();

    f.relink(&[f.pick("talk.mp4", ORIGINAL)], false).unwrap();

    let mut left: Vec<String> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec![
            "a-talk-2-250.jpg",
            "a-talk-2.peaks.400.json",
            "a-talk-250.jpg.bak",
            "a-talk-500.jpg",
            "notes.txt"
        ]
    );
}

// Only a file-backed source that is really missing can be reconnected: a
// present one, a builtin with no file, and a staged capture (whose file
// belongs to the staging folder, GAP-183) are refused before any dialog —
// by display name, never a path.
#[test]
fn only_a_missing_file_backed_source_can_be_reconnected() {
    let present = Fixture::new(missing_record());
    std::fs::create_dir_all(present.dir().join("media")).unwrap();
    std::fs::write(present.dir().join("media").join("a-talk.mp4"), ORIGINAL).unwrap();
    let builtin = Fixture::new(SourceRecord {
        locator: SourceLocator::Builtin,
        ..missing_record()
    });
    let staged = Fixture::new(SourceRecord {
        locator: SourceLocator::Staging {
            base: "2026-09-23 1000 Talk".into(),
        },
        ..missing_record()
    });
    // F-22: a capture's webcam track lives in staging beside it, exactly
    // like the capture -- reconnecting it here would copy a replacement into
    // `media\` and cut the project's tie to the capture it was recorded with.
    let staged_companion = Fixture::new(SourceRecord {
        locator: SourceLocator::StagingFile {
            base: "2026-09-23 1000 Talk".into(),
            file: "2026-09-23 1000 Talk.webcam.mp4".into(),
        },
        ..missing_record()
    });
    for (what, f) in [
        ("present", &present),
        ("builtin", &builtin),
        ("staged", &staged),
        ("staged companion", &staged_companion),
    ] {
        let job = RelinkJob {
            state: &f.state,
            root: f.root.path(),
            session_id: SESSION,
            io: &FakeIo::default(),
        };
        let e = targets(&job, &[ASSET.to_string()])
            .map(|_| ())
            .expect_err(what);
        assert_eq!(e.code, EditorErrorCode::InvalidRequest, "{what}: {e:?}");
        assert!(e.message.contains("talk.mp4"), "{what}: {}", e.message);
    }
}

#[test]
fn the_request_itself_is_checked() {
    let ids = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert!(check_request(&ids(&["a"]), false).is_ok());
    let too_many: Vec<String> = (0..=limits::MAX_ASSETS).map(|i| format!("a{i}")).collect();
    let e = check_request(&too_many, false).unwrap_err();
    assert!(
        e.message.contains(&limits::MAX_ASSETS.to_string()),
        "{}",
        e.message
    );
    assert!(check_request(&ids(&["a"]), true).is_ok());
    for (bad, confirm) in [
        (ids(&[]), false),
        (ids(&["a", "a"]), false),
        (ids(&["../x"]), false),
        (ids(&["a", "b"]), true),
    ] {
        let e = check_request(&bad, confirm).expect_err(&format!("{bad:?}"));
        assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    }
}

fn staged_record() -> SourceRecord {
    SourceRecord {
        locator: SourceLocator::Staging {
            base: "2026-09-23 1000 Capture".into(),
        },
        ..missing_record()
    }
}

fn graph_asset(json: serde_json::Value) -> vault_buddy_core::editor::Asset {
    serde_json::from_value(json).unwrap()
}

// Fix round 1 (review Minor 3): "Find all" names every missing original.
// One that cannot be reconnected here — a staged capture, one only a
// rendered snapshot still uses — is LEFT OUT with its reason instead of
// refusing the whole batch; the rest reconnect.
#[test]
fn a_batch_leaves_out_what_cannot_be_reconnected_and_says_why() {
    let mut graph = project();
    graph.assets.push(graph_asset(serde_json::json!({
        "id": "a-cap", "kind": "video", "name": "capture.mp4", "duration_ms": 9_000
    })));
    let mut sources = BTreeMap::new();
    sources.insert(ASSET.to_string(), missing_record());
    sources.insert("a-cap".to_string(), staged_record());
    sources.insert("a-old".to_string(), missing_record());
    let f = Fixture::with(graph, sources);

    let report = f
        .relink_ids(
            &[ASSET, "a-cap", "a-old"],
            &[f.pick("talk.mp4", ORIGINAL)],
            false,
        )
        .expect("one unreconnectable source must not refuse the batch");

    assert_eq!(report.matched.len(), 1, "{report:?}");
    let excluded: Vec<&str> = report
        .excluded
        .iter()
        .map(|e| e.asset_id.as_str())
        .collect();
    assert_eq!(excluded, vec!["a-cap", "a-old"]);
    assert!(
        report.excluded[0].reason.contains("capture.mp4"),
        "{:?}",
        report.excluded
    );
    assert!(!report.excluded[1].reason.is_empty());
    // A batch with nothing reconnectable left reports why and changes
    // nothing (the command opens no dialog for it).
    let only_out = f.relink_ids(&["a-cap", "a-old"], &[], false).unwrap();
    assert_eq!(only_out.excluded.len(), 2);
    assert!(only_out.matched.is_empty() && only_out.unmatched.is_empty());
    // One source alone is still refused outright, as before.
    let e = f
        .relink_ids(&["a-cap"], &[f.pick("x.mp4", ORIGINAL)], false)
        .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
}

// Fix round 1 (review Minor 6): a match whose copy fails is reported with
// its real cause — never as "no file matched" — and changes nothing.
#[test]
fn a_failed_copy_is_reported_as_its_cause_not_as_unmatched() {
    let f = Fixture::new(missing_record());
    let report = f
        .relink(&[f.pick("talk changes.mp4", ORIGINAL)], false)
        .unwrap();

    assert!(report.matched.is_empty(), "{report:?}");
    assert!(report.unmatched.is_empty(), "{report:?}");
    assert_eq!(report.failed.len(), 1);
    assert_eq!(report.failed[0].asset_id, ASSET);
    assert_eq!(report.failed[0].file, "talk changes.mp4");
    assert!(
        report.failed[0].error.contains("changed"),
        "{:?}",
        report.failed
    );
    assert_eq!(f.record(), missing_record());
    assert!(f.media_files().is_empty());
    assert_eq!(f.revision(), 5);
}

// Fix round 1 (review Minor 7): a detached-audio asset plays its video's
// sound, so a replacement WITHOUT a sound track would leave that clip
// referencing audio the file lacks. Refused; with sound it is accepted.
#[test]
fn a_replacement_without_sound_is_refused_while_detached_audio_depends_on_it() {
    let mut graph = project();
    graph.assets.push(graph_asset(serde_json::json!({
        "id": "a-talk-audio", "kind": "audio", "name": "talk.mp4 audio",
        "duration_ms": 31_000, "linked_asset": ASSET
    })));
    let mut sources = BTreeMap::new();
    sources.insert(ASSET.to_string(), missing_record());
    let f = Fixture::with(graph, sources);

    let silent = f.pick("silent-long.mp4", b"VIDEO:40000");
    let e = f.relink(&[silent], true).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(e.message.contains("silent-long.mp4"), "{}", e.message);
    assert_eq!(f.record(), missing_record());

    let voiced = f.pick("voiced-long.mp4", b"AVIDEO:40000");
    let report = f.relink(&[voiced], true).unwrap();
    assert_eq!(report.replaced.len(), 1, "{report:?}");
}

// The unused list reaches the wire: with two sources unresolved, a picked
// file nothing claimed is named rather than silently ignored.
#[test]
fn a_picked_file_nothing_claimed_is_reported_unused() {
    let mut graph = project();
    graph.assets.push(graph_asset(serde_json::json!({
        "id": "a-two", "kind": "video", "name": "two.mp4", "duration_ms": 5_000
    })));
    let mut sources = BTreeMap::new();
    sources.insert(ASSET.to_string(), missing_record());
    sources.insert(
        "a-two".to_string(),
        SourceRecord {
            locator: SourceLocator::Media {
                file: "a-two.mp4".into(),
            },
            duration_ms: 5_000,
            ..missing_record()
        },
    );
    let f = Fixture::with(graph, sources);
    let report = f
        .relink_ids(&[ASSET, "a-two"], &[f.pick("stray.mp4", b"AUDIO:1")], false)
        .unwrap();
    assert_eq!(
        report.unmatched,
        vec![ASSET.to_string(), "a-two".to_string()]
    );
    assert_eq!(report.unused, vec!["stray.mp4".to_string()]);
}
