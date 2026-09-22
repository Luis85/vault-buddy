//! `moveClips`/`reorderClip` tests (plus the locked-track table test that
//! spans all four track-locking commands), split out of `clips_tests.rs`
//! (the same `#[path]` precedent that file itself uses relative to
//! `clips.rs`) once the overflow regression tests pushed that file past
//! the 800-nonblank-line Rust cap. A child of `clips::tests`, not a
//! sibling of it, so it reaches that module's fixture builders
//! (`track`/`asset`/`clip`/...) and every `use super::*` import already in
//! scope there via ordinary descendant-module privacy -- nothing here is
//! duplicated.

use super::*;

// ---- moveClips ----------------------------------------------------------

#[test]
fn move_group_clamps_the_shared_delta() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 100, 0, 200));
    project.clips.push(clip("c2", "v1", "a1", 500, 0, 200));

    let (candidate, _) = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            delta_ms: -300,
            track_id: None,
        },
    )
    .unwrap();

    let c1 = candidate.clips.iter().find(|c| c.id == "c1").unwrap();
    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(c1.start_ms, 0);
    assert_eq!(c2.start_ms, 400);
}

#[test]
fn move_into_overlap_rejects_the_whole_group() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 100, 0, 200));
    project.clips.push(clip("c2", "v1", "a1", 500, 0, 200));
    project.clips.push(clip("blocker", "v1", "a1", 550, 0, 100));

    let err = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            delta_ms: 50,
            track_id: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    let c1 = project.clips.iter().find(|c| c.id == "c1").unwrap();
    assert_eq!(
        c1.start_ms, 100,
        "a rejected move must leave the caller's project untouched"
    );
}

#[test]
fn move_clips_handles_extreme_deltas_without_overflow() {
    // Regression: before the fix, `(clip.start_ms as i64) + clamped_delta`
    // was unchecked i64 addition, and a positive `deltaMs` was never
    // clamped at all -- `i64::MAX` overflowed i64 (panics in debug, wraps
    // in release).
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 100, 0, 200));

    let err = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: i64::MAX,
            track_id: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);

    // The shared-delta clamp already neutralizes an unreasonably large
    // NEGATIVE delta before it is ever applied (it clamps to whatever
    // brings the earliest clip to 0) -- confirmed here as a "must not
    // panic and must clamp to 0" regression, not a refusal.
    let (candidate, _) = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: i64::MIN,
            track_id: None,
        },
    )
    .unwrap();
    assert_eq!(candidate.clips[0].start_ms, 0);
}

#[test]
fn move_clips_refuses_track_id_with_multiple_clips_and_a_kind_incompatible_track() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.tracks.push(track("a1", TrackKind::Audio, false));
    project.assets.push(asset("va1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "va1", 0, 0, 200));
    project.clips.push(clip("c2", "v1", "va1", 500, 0, 200));

    let multi = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into(), "c2".into()],
            delta_ms: 0,
            track_id: Some("v1".into()),
        },
    )
    .unwrap_err();
    assert_eq!(multi.code, EditorErrorCode::InvalidRequest);

    let mismatched = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: 0,
            track_id: Some("a1".into()),
        },
    )
    .unwrap_err();
    assert_eq!(mismatched.code, EditorErrorCode::InvalidRequest);
}

// ---- moveClips group expansion (Task 8, F13) ------------------------------

#[test]
fn group_move_preserves_relative_offsets() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c1 = clip("c1", "v1", "a1", 100, 0, 200);
    c1.group_id = Some("g1".into());
    project.clips.push(c1);
    let mut c2 = clip("c2", "v1", "a1", 500, 0, 200);
    c2.group_id = Some("g1".into());
    project.clips.push(c2);

    // Only c1 is named in the request -- c2 must move too (a grouped
    // partner pulled in only by expansion), by the SAME delta, so their
    // 400ms relative offset survives unchanged.
    let (candidate, _) = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: 50,
            track_id: None,
        },
    )
    .unwrap();

    let c1 = candidate.clips.iter().find(|c| c.id == "c1").unwrap();
    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(c1.start_ms, 150);
    assert_eq!(
        c2.start_ms, 550,
        "the grouped partner must move by the SAME delta even though it \
         was never named in clipIds"
    );
}

#[test]
fn group_move_with_a_locked_member_changes_nothing() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.tracks.push(track("v2", TrackKind::Video, true)); // locked
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    let mut c1 = clip("c1", "v1", "a1", 100, 0, 200);
    c1.group_id = Some("g1".into());
    project.clips.push(c1);
    // c2 is on the LOCKED track but is NOT named in the request -- it is
    // only reachable via group expansion.
    let mut c2 = clip("c2", "v2", "a1", 500, 0, 200);
    c2.group_id = Some("g1".into());
    project.clips.push(c2);

    let err = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: 50,
            track_id: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.code, EditorErrorCode::InvalidRequest);
    assert!(err.message.contains("locked"), "{}", err.message);

    let c1 = project.clips.iter().find(|c| c.id == "c1").unwrap();
    let c2 = project.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(
        (c1.start_ms, c2.start_ms),
        (100, 500),
        "ANY locked member -- even one only reachable through group \
         expansion -- must reject the WHOLE move and change nothing"
    );
}

// ---- locked-track table test (split/trim/delete/move) --------------------

#[test]
fn locked_track_refuses_split_trim_delete_move() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, true));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 1_000));

    let split_err = split_clip(
        &project,
        &SplitClipPayload {
            clip_id: "c1".into(),
            at_ms: 500,
        },
    )
    .unwrap_err();
    let trim_err = trim_clip(
        &project,
        &TrimClipPayload {
            clip_id: "c1".into(),
            start_ms: 0,
            in_ms: 100,
            out_ms: 900,
        },
    )
    .unwrap_err();
    let delete_err = delete_clips(
        &project,
        &DeleteClipsPayload {
            clip_ids: vec!["c1".into()],
            close_gap: false,
        },
    )
    .unwrap_err();
    let move_err = move_clips(
        &project,
        &MoveClipsPayload {
            clip_ids: vec!["c1".into()],
            delta_ms: 100,
            track_id: None,
        },
    )
    .unwrap_err();

    for (name, err) in [
        ("split", &split_err),
        ("trim", &trim_err),
        ("delete", &delete_err),
        ("move", &move_err),
    ] {
        assert_eq!(err.code, EditorErrorCode::InvalidRequest, "{name}");
        assert!(err.message.contains("locked"), "{name}: {}", err.message);
    }
}

// ---- reorderClip ----------------------------------------------------------

#[test]
fn reorder_swaps_adjacent_clips_keeping_span() {
    // Anchored at a NON-ZERO a_start, with a 200ms GAP between the two
    // clips -- this fixture fails both a "re-anchors at 0" regression
    // (a_start must survive as-is) and a "collapses the gap" regression
    // (the old algorithm packed the pair contiguously, discarding it).
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 1_000, 0, 300)); // [1000,1300)
    project.clips.push(clip("c2", "v1", "a1", 1_500, 0, 200)); // [1500,1700), 200ms gap

    let (candidate, _) = reorder_clip(
        &project,
        &ReorderClipPayload {
            clip_id: "c1".into(),
            direction: ReorderDirection::Later,
        },
    )
    .unwrap();

    let c1 = candidate.clips.iter().find(|c| c.id == "c1").unwrap();
    let c2 = candidate.clips.iter().find(|c| c.id == "c2").unwrap();
    assert_eq!(
        c2.start_ms, 1_000,
        "the later clip takes the earlier clip's original start (a_start)"
    );
    assert_eq!(
        c1.start_ms, 1_400,
        "the earlier clip's new start is b_end - a_duration (1700 - 300), so both the pair's \
         outer span [1000,1700) and the original 200ms gap between them survive the swap"
    );
    assert_eq!(
        clip_end(c1),
        1_700,
        "the pair's combined outer span end (b's original end) must be preserved"
    );
}

#[test]
fn reorder_refuses_when_already_first_or_last() {
    let mut project = base_project();
    project.tracks.push(track("v1", TrackKind::Video, false));
    project.assets.push(asset("a1", AssetKind::Video, 5_000));
    project.clips.push(clip("c1", "v1", "a1", 0, 0, 300));
    project.clips.push(clip("c2", "v1", "a1", 500, 0, 200));

    let err = reorder_clip(
        &project,
        &ReorderClipPayload {
            clip_id: "c1".into(),
            direction: ReorderDirection::Earlier,
        },
    )
    .unwrap_err();
    assert_eq!(err.message, "Already first");

    let err = reorder_clip(
        &project,
        &ReorderClipPayload {
            clip_id: "c2".into(),
            direction: ReorderDirection::Later,
        },
    )
    .unwrap_err();
    assert_eq!(err.message, "Already last");
}
