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

/// The stems' assets, tracks and clips — ALL of them or none (review fix
/// round 1). `placed` is every screen segment as `(output start, source
/// start, source end)`; `duration_ms` the capture's; `input_count` how many
/// inputs it recorded; `tracks_so_far`/`clips_so_far` what the project will
/// already hold (the webcam's share included).
///
/// Stems REPLACE the mix (the caller mutes it), so they are placed only when
/// every input `1..=input_count` has one and they fit the track and clip
/// limits. Anything less — one stem failed and was never listed, a listed
/// file was not registered, too many inputs, too many cuts — would mute the
/// inputs left out, or produce a project `validate_project` refuses. `None`
/// then: the capture migrates exactly as with stems off (logged).
pub(super) fn stem_parts(
    stems: &[StemInput],
    input_count: usize,
    placed: &[(u64, u64, u64)],
    duration_ms: u64,
    (tracks_so_far, clips_so_far): (usize, usize),
) -> Option<(Vec<Asset>, Vec<Track>, Vec<Clip>)> {
    // One stem per input, in input order: the first entry for each index.
    let covering: Vec<&StemInput> = (1..=input_count)
        .map(|index| stems.iter().find(|s| usize::try_from(s.index) == Ok(index)))
        .collect::<Option<_>>()?;
    let fits = !covering.is_empty()
        && tracks_so_far + covering.len() <= limits::MAX_TRACKS
        && clips_so_far + covering.len() * placed.len() <= limits::MAX_CLIPS;
    if !fits {
        log::warn!(
            "migrate: {} stem(s) for {input_count} input(s) not placed (track or clip limit); the mix stays audible",
            covering.len()
        );
        return None;
    }
    let mut assets = Vec::new();
    let mut tracks = Vec::new();
    let mut clips = Vec::new();
    for stem in covering {
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
    Some((assets, tracks, clips))
}
