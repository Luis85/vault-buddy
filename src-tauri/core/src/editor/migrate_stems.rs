//! `migrate::from_staged`'s per-input audio STEMS (Task 53; F-05, F26) — a
//! child module of `migrate` because that file sits near the 800-line cap.
//!
//! Each stem becomes its own `Audio` asset on its own audio track, one clip
//! per placed screen segment at the SAME output start and source range: a
//! stem was teed off the very mixing round the capture's audio track was
//! written from, on the same pacer, so its file time IS the capture's audio
//! time and the screen's source -> output mapping applies unchanged. The
//! capture's own (mixed) sound is muted by the caller — with the inputs laid
//! out separately, keeping the mix audible would play every voice twice.

use super::{limits, plain_clip, track, Asset, AssetKind, Clip, Map, StemInput, Track, TrackKind};

/// The asset id stem `index` migrates to — and so the key its
/// `sources.json` `StagingFile` entry lives under.
pub fn stem_asset_id(index: u32) -> String {
    format!("stem-{index}")
}

/// The stems' assets, tracks and clips. `placed` is every screen segment as
/// `(output start, source start, source end)`; `duration_ms` the capture's.
/// Stems beyond the track limit are left out rather than producing a project
/// `validate_project` would refuse.
pub(super) fn stem_parts(
    stems: &[StemInput],
    placed: &[(u64, u64, u64)],
    duration_ms: u64,
    tracks_so_far: usize,
) -> (Vec<Asset>, Vec<Track>, Vec<Clip>) {
    let room = limits::MAX_TRACKS.saturating_sub(tracks_so_far);
    let mut assets = Vec::new();
    let mut tracks = Vec::new();
    let mut clips = Vec::new();
    for stem in stems.iter().take(room) {
        let asset_id = stem_asset_id(stem.index);
        let track_id = format!("as{}", stem.index);
        assets.push(Asset {
            id: asset_id.clone(),
            kind: AssetKind::Audio,
            name: stem.input.clone(),
            duration_ms,
            width: None,
            height: None,
            size: None,
            builtin: None,
            media_type: None,
            linked_asset: None,
            original_name: Some(stem.file.clone()),
            extra: Map::new(),
        });
        tracks.push(track(&track_id, TrackKind::Audio, &stem.input));
        clips.extend(
            placed
                .iter()
                .enumerate()
                .map(|(n, &(start, source_start, source_end))| {
                    plain_clip(
                        format!("s{}-{}", stem.index, n + 1),
                        &asset_id,
                        &track_id,
                        format!("{} {}", stem.input, n + 1),
                        start,
                        (source_start, source_end),
                    )
                }),
        );
    }
    (assets, tracks, clips)
}
