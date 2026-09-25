//! `take.rs`'s tests: the chunk-sequence state machine, the MIME and
//! header rules, and the asset a finished take becomes.

use super::*;
use crate::editor::commands::payloads::AddAssetsPayload;
use crate::editor::model::{AssetKind, TrackKind};
use crate::editor::test_support::{asset, clip, minimal_project, track};
use crate::editor::{EditorSession, InternalCommand, Num};

fn recording(next_seq: u64, bytes: u64) -> TakeState {
    TakeState::Recording { next_seq, bytes }
}

// A lost chunk (a dropped append, a reordered retry) must never be written
// after its successor: every cluster from the gap on would be garbage with
// no error anywhere. The gap is refused and the state is left as it was.
#[test]
fn sequence_gaps_are_refused() {
    let mut take = TakeState::new();
    take.accept_chunk(0, 700).unwrap();
    take.accept_chunk(1, 300).unwrap();
    assert_eq!(
        take.accept_chunk(3, 10),
        Err(TakeError::OutOfSequence {
            expected: 2,
            got: 3
        }),
        "chunk 2 is missing, so chunk 3 must be refused"
    );
    assert_eq!(
        take.accept_chunk(1, 10),
        Err(TakeError::OutOfSequence {
            expected: 2,
            got: 1
        }),
        "a repeated chunk must be refused"
    );
    assert_eq!(take, recording(2, 1_000), "a refusal changes nothing");
    take.accept_chunk(2, 5).unwrap();
    assert_eq!(take, recording(3, 1_005));
}

// A chunk over 1 MiB is refused BEFORE it is counted (the webview's 1 s
// timeslice never needs more); so is one that would push the take past 4 GiB.
#[test]
fn oversized_chunk_is_refused() {
    let mut take = TakeState::new();
    assert_eq!(
        take.accept_chunk(0, MAX_TAKE_CHUNK_BYTES + 1),
        Err(TakeError::ChunkTooLarge {
            len: MAX_TAKE_CHUNK_BYTES + 1
        })
    );
    assert_eq!(take, recording(0, 0));
    take.accept_chunk(0, MAX_TAKE_CHUNK_BYTES).unwrap();
    assert_eq!(take, recording(1, MAX_TAKE_CHUNK_BYTES));

    let mut near_full = recording(7, MAX_TAKE_BYTES - 3);
    assert_eq!(
        near_full.accept_chunk(7, 4),
        Err(TakeError::TakeTooLarge {
            total: MAX_TAKE_BYTES + 1
        })
    );
    near_full.accept_chunk(7, 3).unwrap();
    assert_eq!(near_full, recording(8, MAX_TAKE_BYTES));
}

// `finish(last_seq)` needs `last_seq + 1 == next_seq`: a finish that names a
// chunk that never arrived (or an earlier one) would publish a take missing
// its tail. An empty take can never be finished.
#[test]
fn finish_requires_the_last_sequence() {
    let mut empty = TakeState::new();
    assert_eq!(
        empty.finish(0),
        Err(TakeError::WrongLastSequence {
            expected: None,
            got: 0
        })
    );
    let mut take = recording(4, 900);
    assert_eq!(
        take.finish(4),
        Err(TakeError::WrongLastSequence {
            expected: Some(3),
            got: 4
        }),
        "chunk 4 never arrived"
    );
    assert_eq!(
        take.finish(2),
        Err(TakeError::WrongLastSequence {
            expected: Some(3),
            got: 2
        }),
        "finishing early would drop chunk 3"
    );
    assert_eq!(take, recording(4, 900));
    take.finish(3).unwrap();
    assert_eq!(take, TakeState::Finished);
    assert_eq!(take.finish(3), Err(TakeError::NotRecording));
    assert_eq!(take.accept_chunk(4, 1), Err(TakeError::NotRecording));
}

#[test]
fn a_take_error_is_an_invalid_request() {
    let e = EditorError::from(TakeError::OutOfSequence {
        expected: 2,
        got: 5,
    });
    assert_eq!(e.code, EditorErrorCode::InvalidRequest);
    assert!(e.message.contains("chunk 5"), "{}", e.message);
    assert!(e.message.contains("chunk 2"), "{}", e.message);
}

#[test]
fn only_the_three_webm_types_are_takes() {
    for ok in TAKE_MIME_TYPES {
        assert!(is_take_mime(ok), "{ok}");
    }
    for bad in [
        "video/mp4",
        "video/webm;codecs=h264",
        "VIDEO/WEBM",
        "video/webm ",
        "",
    ] {
        assert!(!is_take_mime(bad), "{bad:?}");
    }
}

#[test]
fn a_sequence_header_is_canonical_decimal() {
    assert_eq!(parse_seq("0"), Some(0));
    assert_eq!(parse_seq("17"), Some(17));
    assert_eq!(parse_seq("18446744073709551615"), Some(u64::MAX));
    for bad in [
        "",
        "017",
        "+1",
        "-1",
        " 1",
        "1 ",
        "1e3",
        "18446744073709551616",
    ] {
        assert_eq!(parse_seq(bad), None, "{bad:?}");
    }
}

// A08 (core half): a finished take is a NEW asset beside the capture. It is
// registered through `AddAssets` and leaves the screen source's asset and
// every clip on it exactly as they were — nothing is baked into `src`.
#[test]
fn take_becomes_an_independent_asset_not_part_of_the_screen_clip() {
    let mut project = minimal_project();
    let mut src = asset("src", AssetKind::Video, 61_500);
    src.width = Some(Num::from(1280u32));
    src.height = Some(Num::from(720u32));
    project.assets.push(src);
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.clips.push(clip("c1", "v1", "src", 0, 0, 30_000));
    project
        .clips
        .push(clip("c2", "v1", "src", 30_000, 31_500, 61_500));
    let before = project.clone();
    assert_eq!(next_take_ordinal(&project), 1);

    let facts = ProbeFacts {
        duration_ms: 12_345,
        width: Some(640),
        height: Some(360),
        has_video: true,
        has_audio: true,
    };
    let take = take_asset("take-a1b2c3d4e5".to_string(), 1, facts, 98_765);
    assert_eq!(take.name, "Webcam take 1");
    assert_eq!(take.kind, AssetKind::Video);
    assert_eq!(take.duration_ms, 12_345);
    assert_eq!(
        (take.width.clone(), take.height.clone()),
        (Some(Num::from(640u32)), Some(Num::from(360u32)))
    );
    assert_eq!(take.size, Some(98_765));
    assert_eq!(
        take.linked_asset, None,
        "a take is not linked to the capture"
    );

    let mut session = EditorSession::new("ses-1", project);
    session
        .execute_internal(&InternalCommand::AddAssets(AddAssetsPayload {
            assets: vec![take.clone()],
        }))
        .unwrap();
    let after = session.project();
    assert_eq!(after.assets.len(), 2);
    assert_eq!(after.assets[0], before.assets[0], "src is untouched");
    assert_eq!(after.assets[1], take);
    assert_eq!(after.clips, before.clips, "no screen clip changes");
    assert_eq!(after.tracks, before.tracks, "no track appears");
    assert!(!asset_in_use(after, &take.id), "nothing plays the take yet");
    assert!(asset_in_use(after, "src"));
    assert_eq!(next_take_ordinal(after), 2, "the next take is number 2");
}
