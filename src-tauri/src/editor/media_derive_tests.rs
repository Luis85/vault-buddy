//! `media_derive.rs`'s tests: a real tempdir project and session. The
//! ffmpeg round trips run the user-installed ffmpeg and SKIP VISIBLY
//! without it (`eprintln!("SKIP: …")`) — a skip is not a pass.

use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use serde_json::json;
use vault_buddy_core::editor::{Asset, AssetKind, EditorErrorCode, EditorSession, Map};
use vault_buddy_core::sync_util::lock_ignoring_poison;

use super::*;
use crate::editor::project_store::{minimal_project, SourceLocator, SourceRecord};
use crate::editor::store_io::create_project;

const SESSION: &str = "ses-proj1";
const PROJECT: &str = "proj1";

fn asset(id: &str, kind: AssetKind, duration_ms: u64) -> Asset {
    Asset {
        id: id.to_string(),
        kind,
        name: format!("{id}.media"),
        duration_ms,
        width: None,
        height: None,
        size: None,
        builtin: None,
        media_type: None,
        linked_asset: None,
        original_name: None,
        extra: Map::new(),
    }
}

/// A `media\` source record; `kind` decides which streams it claims.
fn record(file: &str, kind: SourceMediaKind, duration_ms: u64) -> SourceRecord {
    SourceRecord {
        locator: SourceLocator::Media { file: file.into() },
        sha256: None,
        size: 1,
        duration_ms,
        width: None,
        height: None,
        has_audio: kind == SourceMediaKind::Audio,
        has_video: kind != SourceMediaKind::Audio,
        media_kind: kind,
    }
}

/// A live session over a project holding one asset per entry, each backed
/// by `media\<file>`.
fn opened(
    root: &Path,
    state: &EditorState,
    entries: &[(&str, AssetKind, SourceRecord)],
) -> PathBuf {
    let mut project = minimal_project(PROJECT);
    let mut sources = BTreeMap::new();
    for (id, kind, rec) in entries {
        project.assets.push(asset(id, *kind, rec.duration_ms));
        sources.insert(id.to_string(), rec.clone());
    }
    create_project(root, &project, &sources).unwrap();
    lock_ignoring_poison(&state.sessions).insert(
        SESSION.to_string(),
        EditorSession::resume(SESSION, project, 1),
    );
    let media = project_dir(root, PROJECT).unwrap().join("media");
    std::fs::create_dir_all(&media).unwrap();
    media
}

fn req(asset_id: &str) -> MediaRequest<'_> {
    MediaRequest {
        session_id: SESSION,
        asset_id,
    }
}

fn no_ffmpeg() -> Option<String> {
    None
}

fn installed_ffmpeg(test: &str) -> Option<String> {
    let found = crate::ffmpeg::resolve_working_ffmpeg().map(|t| t.ffmpeg);
    if found.is_none() {
        eprintln!("SKIP: {test} needs ffmpeg on PATH");
    }
    found
}

fn synthesize(ffmpeg: &str, input: &[&str], out: &Path) {
    let status = tool_command(ffmpeg)
        .args(["-v", "error", "-y"])
        .args(input)
        .arg(out)
        .status()
        .unwrap();
    assert!(status.success(), "ffmpeg could not synthesize {out:?}");
}

// The IPC reply, pinned as LITERAL JSON (never re-serialized against
// itself): `{ peaks: number[] }`.
#[test]
fn media_peaks_wire_shape_is_pinned() {
    assert_eq!(
        serde_json::to_value(MediaPeaks {
            peaks: vec![0.5, 0.25, 0.0]
        })
        .unwrap(),
        json!({ "peaks": [0.5, 0.25, 0.0] })
    );
}

// The brief's argv verbatim for peaks; thumbnails seek BEFORE the input
// (a demuxer seek, not a decode from zero) and scale to 160 wide.
#[test]
fn peaks_and_thumbnail_argv_are_pinned() {
    let src = Path::new("media/clip one.mp4");
    let args = |v: Vec<OsString>| -> Vec<String> {
        v.into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(
        args(peaks_args(src)),
        [
            "-v",
            "error",
            "-i",
            "media/clip one.mp4",
            "-ac",
            "1",
            "-ar",
            "8000",
            "-f",
            "s16le",
            "-"
        ]
    );
    assert_eq!(
        args(thumbnail_args(src, 1_250, Path::new("cache/.t.tmp"))),
        [
            "-v",
            "error",
            "-ss",
            "1.250",
            "-i",
            "media/clip one.mp4",
            "-frames:v",
            "1",
            "-vf",
            "scale=160:-2",
            "-f",
            "mjpeg",
            "-y",
            "cache/.t.tmp"
        ]
    );
    assert_eq!(seconds(5), "0.005");
    assert_eq!(seconds(61_040), "61.040");
}

// At most one thumbnail per 250 ms of source, never past its last ms, and
// one for a still image whatever time is asked.
#[test]
fn thumbnail_times_are_quantized_and_clamped() {
    assert_eq!(thumbnail_ms(740, 4_000, false), 500);
    assert_eq!(thumbnail_ms(249, 4_000, false), 0);
    assert_eq!(thumbnail_ms(250, 4_000, false), 250);
    assert_eq!(thumbnail_ms(9_999, 4_000, false), 3_750, "clamped to 3999");
    assert_eq!(thumbnail_ms(740, 4_000, true), 0);
    assert_eq!(thumbnail_ms(740, 0, false), 0);
}

#[test]
fn only_our_thumbnail_names_are_recognised() {
    assert!(is_thumbnail_name("asset-1-500.jpg"));
    assert!(is_thumbnail_name("a-0.jpg"));
    for other in [
        "a.peaks.10.json",
        ".a-5.jpg.t-1.tmp",
        "a-.jpg",
        "a-5x.jpg",
        "a-5.jpeg",
        "-5.jpg",
        "..\\a-5.jpg",
        "notes.txt",
    ] {
        assert!(!is_thumbnail_name(other), "{other}");
    }
}

// The brief's polite refusal: no ffmpeg (and nothing cached) is
// `encoderUnavailable` naming the remedy, and leaves no job behind.
#[test]
fn missing_ffmpeg_refuses_politely() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[
            (
                "snd",
                AssetKind::Audio,
                record("snd.wav", SourceMediaKind::Audio, 2_000),
            ),
            (
                "pic",
                AssetKind::Video,
                record("pic.mp4", SourceMediaKind::Video, 2_000),
            ),
        ],
    );
    std::fs::write(media.join("snd.wav"), b"wav").unwrap();
    std::fs::write(media.join("pic.mp4"), b"mp4").unwrap();

    let e = peaks_in(&state, root.path(), &req("snd"), 100, &no_ffmpeg).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::EncoderUnavailable);
    assert_eq!(e.message, "Install ffmpeg to see waveforms.");
    let e = thumbnail_in(&state, root.path(), &req("pic"), 0, &no_ffmpeg).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::EncoderUnavailable);
    assert!(e.message.contains("Install ffmpeg"), "{}", e.message);
    assert!(lock_ignoring_poison(&state.jobs)
        .records_for(SESSION)
        .is_empty());
}

#[test]
fn malformed_and_unsuitable_requests_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[
            (
                "snd",
                AssetKind::Audio,
                record("snd.wav", SourceMediaKind::Audio, 2_000),
            ),
            (
                "pic",
                AssetKind::Video,
                record("pic.png", SourceMediaKind::Image, 5_000),
            ),
        ],
    );
    std::fs::write(media.join("snd.wav"), b"wav").unwrap();
    std::fs::write(media.join("pic.png"), b"png").unwrap();
    let never = || -> Option<String> { panic!("ffmpeg must not be needed") };

    for buckets in [0, MAX_BUCKETS + 1] {
        let e = peaks_in(&state, root.path(), &req("snd"), buckets, &never).unwrap_err();
        assert_eq!(e.code, EditorErrorCode::InvalidRequest, "{buckets}");
    }
    let e = peaks_in(&state, root.path(), &req("pic"), 10, &never).unwrap_err();
    assert_eq!(
        e.code,
        EditorErrorCode::UnsupportedMedia,
        "an image has no sound"
    );
    let e = thumbnail_in(&state, root.path(), &req("snd"), 0, &never).unwrap_err();
    assert_eq!(
        e.code,
        EditorErrorCode::UnsupportedMedia,
        "a sound has no picture"
    );
    let e = peaks_in(&state, root.path(), &req("nope"), 10, &never).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::UnauthorizedSource);
    let gone = MediaRequest {
        session_id: "ses-gone",
        asset_id: "snd",
    };
    let e = peaks_in(&state, root.path(), &gone, 10, &never).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::SessionGone);
    let e = thumbnail_in(&state, root.path(), &gone, 0, &never).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::SessionGone);
}

// A cached waveform is served with NO ffmpeg at all — and only while it
// still describes the same file at the same bucket count. The on-disk
// cache format is pinned as literal JSON.
#[test]
fn a_cached_waveform_needs_no_ffmpeg_until_its_source_changes() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[(
            "snd",
            AssetKind::Audio,
            record("snd.wav", SourceMediaKind::Audio, 2_000),
        )],
    );
    let src = media.join("snd.wav");
    std::fs::write(&src, b"sound bytes").unwrap();
    let stamp = SourceStamp::of(&src).unwrap();
    let cache = ensure_cache_dir(root.path(), PROJECT).unwrap();
    let on_disk = json!({
        "sourceSize": stamp.source_size,
        "sourceModifiedMs": stamp.source_modified_ms,
        "peaks": [0.25, 1.0, 0.5]
    });
    std::fs::write(cache.join("snd.peaks.3.json"), on_disk.to_string()).unwrap();

    let hit = peaks_in(&state, root.path(), &req("snd"), 3, &no_ffmpeg).unwrap();
    assert_eq!(hit.peaks, vec![0.25, 1.0, 0.5]);
    let e = peaks_in(&state, root.path(), &req("snd"), 4, &no_ffmpeg).unwrap_err();
    assert_eq!(
        e.code,
        EditorErrorCode::EncoderUnavailable,
        "another bucket count"
    );

    std::fs::write(&src, b"a different, longer recording").unwrap();
    let e = peaks_in(&state, root.path(), &req("snd"), 3, &no_ffmpeg).unwrap_err();
    assert_eq!(
        e.code,
        EditorErrorCode::EncoderUnavailable,
        "stale cache ignored"
    );
}

// A cancel before the decode starts spawns nothing and says `cancelled`.
#[test]
fn a_cancelled_decode_reports_cancelled() {
    let e = decode_peaks(
        "ffmpeg",
        Path::new("missing.wav"),
        1_000,
        10,
        &AtomicBool::new(true),
    )
    .unwrap_err();
    assert_eq!(e.code, EditorErrorCode::Cancelled);
}

// The LRU bound: the oldest-used thumbnails go, the newest stay, and
// nothing that is not one of our thumbnails is touched.
#[test]
fn prune_keeps_the_most_recently_used_thumbnails_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let base = SystemTime::now() - Duration::from_secs(10_000);
    for i in 0..205u64 {
        let path = dir.path().join(thumbnail_name("clip-7", i * 250));
        std::fs::write(&path, b"jpg").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(base + Duration::from_secs(i))
            .unwrap();
    }
    std::fs::write(dir.path().join("clip-7.peaks.10.json"), b"{}").unwrap();
    std::fs::write(dir.path().join("notes.txt"), b"keep").unwrap();

    prune_thumbnails(dir.path(), 200);

    for i in 0..5u64 {
        assert!(
            !dir.path().join(thumbnail_name("clip-7", i * 250)).exists(),
            "{i}"
        );
    }
    for i in 5..205u64 {
        assert!(
            dir.path().join(thumbnail_name("clip-7", i * 250)).exists(),
            "{i}"
        );
    }
    assert!(dir.path().join("clip-7.peaks.10.json").exists());
    assert!(dir.path().join("notes.txt").exists());
}

// The real decode: one second of a 0.5-amplitude tone then one second of
// silence (asymmetric, so a reversed or shifted waveform fails). The
// peaks follow the sound, land in the cache, come back from it with no
// ffmpeg, and the job is gone from the registry once the reply exists.
#[test]
fn peaks_round_trip_through_real_ffmpeg() {
    let Some(ffmpeg) = installed_ffmpeg("peaks_round_trip_through_real_ffmpeg") else {
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[(
            "snd",
            AssetKind::Audio,
            record("snd.wav", SourceMediaKind::Audio, 2_000),
        )],
    );
    synthesize(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "aevalsrc=exprs='if(lt(t,1),0.5*sin(2*PI*440*t),0)':d=2:s=44100",
        ],
        &media.join("snd.wav"),
    );
    let program = ffmpeg.clone();
    let found = move || Some(program.clone());

    let peaks = peaks_in(&state, root.path(), &req("snd"), 20, &found)
        .unwrap()
        .peaks;
    assert_eq!(peaks.len(), 20);
    for (i, p) in peaks.iter().enumerate().take(9) {
        assert!((0.4..=0.56).contains(p), "tone bucket {i}: {p}");
    }
    for (i, p) in peaks.iter().enumerate().skip(11) {
        assert!(*p < 0.02, "silent bucket {i}: {p}");
    }
    let cache = cache_path(root.path(), PROJECT).unwrap();
    assert!(cache.join("snd.peaks.20.json").is_file());
    assert!(lock_ignoring_poison(&state.jobs)
        .records_for(SESSION)
        .is_empty());

    let again = peaks_in(&state, root.path(), &req("snd"), 20, &no_ffmpeg).unwrap();
    assert_eq!(again.peaks, peaks, "served from the cache");
}

// The real thumbnail: a JPEG at the quantized time under `cache\`, then a
// cache hit that needs no ffmpeg.
#[test]
fn thumbnail_round_trip_through_real_ffmpeg() {
    let Some(ffmpeg) = installed_ffmpeg("thumbnail_round_trip_through_real_ffmpeg") else {
        return;
    };
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[(
            "pic",
            AssetKind::Video,
            record("pic.mp4", SourceMediaKind::Video, 2_000),
        )],
    );
    synthesize(
        &ffmpeg,
        &[
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x180:rate=10:duration=2",
            "-c:v",
            "mpeg4",
        ],
        &media.join("pic.mp4"),
    );
    let program = ffmpeg.clone();
    let found = move || Some(program.clone());

    let path = thumbnail_in(&state, root.path(), &req("pic"), 740, &found).unwrap();
    let cache = cache_path(root.path(), PROJECT).unwrap();
    assert_eq!(path, cache.join("pic-500.jpg"));
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "a JPEG");
    let names: Vec<String> = std::fs::read_dir(&cache)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["pic-500.jpg"], "no temp left behind");

    let hit = thumbnail_in(&state, root.path(), &req("pic"), 600, &no_ffmpeg).unwrap();
    assert_eq!(hit, path);
    let end = thumbnail_in(&state, root.path(), &req("pic"), 99_999, &found).unwrap();
    assert_eq!(end, cache.join("pic-1750.jpg"));
}

/// A stand-in "ffmpeg" that ignores its arguments and runs for 30 s — the
/// long decode a closing session must be able to stop.
fn slow_tool(dir: &Path) -> String {
    let (name, body) = if cfg!(windows) {
        (
            "slow.cmd",
            // An ABSOLUTE ping: a bare `ping` was not found by cmd.exe under
            // `tool_command`'s augmented PATH on the dev host (docs/Gaps.md
            // GAP-177).
            "@\"%SystemRoot%\\System32\\PING.EXE\" -n 30 127.0.0.1 >nul\r\n",
        )
    } else {
        ("slow.sh", "#!/bin/sh\nsleep 30\n")
    };
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path.to_string_lossy().into_owned()
}

// "Cancelable" end to end: a decode in flight is a registered `peaks` job
// in `preparing`, a closing session's `cancel_session` kills its child,
// the request answers `cancelled` promptly, and the record is gone.
#[test]
fn a_closing_session_cancels_a_running_decode() {
    let root = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[(
            "snd",
            AssetKind::Audio,
            record("snd.wav", SourceMediaKind::Audio, 2_000),
        )],
    );
    std::fs::write(media.join("snd.wav"), b"wav").unwrap();
    let program = slow_tool(tools.path());
    let found = move || Some(program.clone());
    let started = std::time::Instant::now();

    let outcome = std::thread::scope(|scope| {
        let decode = std::thread::Builder::new()
            .name("test-peaks".into())
            .spawn_scoped(scope, || {
                peaks_in(&state, root.path(), &req("snd"), 10, &found)
            })
            .unwrap();
        let running = (0..200).any(|_| {
            let rows = lock_ignoring_poison(&state.jobs).records_for(SESSION);
            let live = rows
                .iter()
                .any(|r| r.kind == JobKind::Peaks && r.phase == JobPhase::Preparing);
            if !live {
                std::thread::sleep(Duration::from_millis(25));
            }
            live
        });
        assert!(running, "the decode is a registered, preparing peaks job");
        // Long enough for the child to be spawned and running, so this is a
        // KILL, not the pre-spawn check.
        std::thread::sleep(Duration::from_millis(700));
        lock_ignoring_poison(&state.jobs).cancel_session(SESSION);
        decode.join().unwrap()
    });

    assert_eq!(outcome.unwrap_err().code, EditorErrorCode::Cancelled);
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "killed, not waited out"
    );
    assert!(lock_ignoring_poison(&state.jobs)
        .records_for(SESSION)
        .is_empty());
}

// ---- fix round 1 -----------------------------------------------------------

// Review Minor 9: the whole point of `create_dir` over `create_dir_all` —
// a project directory that is gone stays gone.
#[test]
fn ensure_cache_dir_never_recreates_a_removed_project() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state, &[]);
    let dir = project_dir(root.path(), PROJECT).unwrap();
    std::fs::remove_dir_all(&dir).unwrap();

    let e = ensure_cache_dir(root.path(), PROJECT).unwrap_err();
    assert_eq!(e.code, EditorErrorCode::SourceMissing);
    assert!(!dir.exists(), "the project directory was recreated");
}

// Review Minor 2: discarding a project while a thumbnail renders stops the
// render (it is killed, not waited out for 30 s) and nothing it made is
// left behind — no project directory, no cache.
#[test]
fn discarding_a_project_mid_render_stops_the_render() {
    use crate::editor::session_commands::{close_in, CloseDisposition};

    let root = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    let media = opened(
        root.path(),
        &state,
        &[(
            "pic",
            AssetKind::Video,
            record("pic.mp4", SourceMediaKind::Video, 2_000),
        )],
    );
    std::fs::write(media.join("pic.mp4"), b"mp4").unwrap();
    let program = slow_tool(tools.path());
    let found = move || Some(program.clone());
    let started = std::time::Instant::now();

    let (render, discard) = std::thread::scope(|scope| {
        let render = std::thread::Builder::new()
            .name("test-thumbnail".into())
            .spawn_scoped(scope, || {
                thumbnail_in(&state, root.path(), &req("pic"), 0, &found)
            })
            .unwrap();
        // Long enough for the child to be running: this is a KILL.
        std::thread::sleep(Duration::from_millis(700));
        let discard = close_in(
            &state,
            root.path(),
            staging.path(),
            SESSION,
            CloseDisposition::DiscardProject,
        );
        // The render had ENDED before the project was removed (the discard
        // stops and waits for it first), not merely been told to stop by
        // the session's close afterwards.
        assert!(
            !derivations_running(&state, SESSION),
            "a render was still in flight when the discard returned"
        );
        (render.join().unwrap(), discard)
    });

    discard.expect("the discard succeeds");
    let code = render.expect_err("the render was stopped").code;
    assert!(
        matches!(
            code,
            EditorErrorCode::Cancelled | EditorErrorCode::SessionGone
        ),
        "{code:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the render was killed, not waited out: {:?}",
        started.elapsed()
    );
    assert!(!project_dir(root.path(), PROJECT).unwrap().exists());
}

// Review Minor 2, the peaks half: a waveform that finishes after its
// session closed is not written into the project (the cache write takes
// the session's save lock and needs the session live under it).
#[test]
fn a_waveform_finished_after_its_session_closed_is_not_cached() {
    let root = tempfile::tempdir().unwrap();
    let state = EditorState::default();
    opened(root.path(), &state, &[]);
    lock_ignoring_poison(&state.sessions).remove(SESSION);
    let cached = CachedPeaks {
        stamp: SourceStamp {
            source_size: 3,
            source_modified_ms: 4,
        },
        peaks: vec![0.5],
    };

    write_cached_peaks(
        &state,
        root.path(),
        SESSION,
        PROJECT,
        "snd.peaks.1.json",
        &cached,
    );

    assert!(!cache_path(root.path(), PROJECT).unwrap().exists());
}

// Review Minor 4: the remembered ffmpeg is keyed on the configured
// override, so changing the path in settings (`set_ffmpeg_path`, or a
// hand edit of config.json) resolves again instead of running the old
// binary until it happens to fail with NotFound.
#[test]
fn the_remembered_ffmpeg_follows_the_configured_override() {
    use std::cell::Cell;
    let cell = Mutex::new(None);
    let calls = Cell::new(0);
    let resolve = |program: &'static str| {
        calls.set(calls.get() + 1);
        Some(program.to_string())
    };

    assert_eq!(
        remembered_ffmpeg(&cell, None, || resolve("path-ffmpeg")).as_deref(),
        Some("path-ffmpeg")
    );
    assert_eq!(
        remembered_ffmpeg(&cell, None, || resolve("unused")).as_deref(),
        Some("path-ffmpeg")
    );
    assert_eq!(calls.get(), 1, "a remembered program is reused");

    let chosen = Some("D:/tools/ffmpeg.exe".to_string());
    assert_eq!(
        remembered_ffmpeg(&cell, chosen.clone(), || resolve("override-ffmpeg")).as_deref(),
        Some("override-ffmpeg")
    );
    assert_eq!(calls.get(), 2, "a changed override resolves again");

    assert_eq!(
        remembered_ffmpeg(&cell, chosen.clone(), || None),
        Some("override-ffmpeg".to_string())
    );
    assert_eq!(
        remembered_ffmpeg(&cell, None, || None),
        None,
        "a miss is not remembered"
    );
    assert_eq!(
        remembered_ffmpeg(&cell, None, || resolve("again")).as_deref(),
        Some("again")
    );
}
