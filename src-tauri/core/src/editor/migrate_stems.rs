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

/// Why `stem_parts` placed no stems, for the CALLER to log — `stem_parts`
/// never logs itself (D-3, hardening review): `core` has no log-capture
/// test helper, so the only way to pin "the no-audio case logs nothing" is
/// to make the decision a value a test can read directly instead of a line
/// a test would otherwise have to scrape from a global logger.
///
/// A stem that never got listed or registered (some `1..=input_count`
/// index has no covering `StemInput`) is deliberately NOT a variant here:
/// that is the ordinary, expected shape of one failed input degrading the
/// whole capture to stems-off (review fix round 1), not a limit — it stays
/// silent, `(None, None)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StemSkipReason {
    /// Every input has a stem, but placing them all would exceed the
    /// project's track or clip limit. Carries how many stems WOULD have
    /// covered every input, for the caller's log line.
    LimitExceeded { covering: usize },
}

/// The stems' assets, tracks and clips — ALL of them or none (review fix
/// round 1) — plus why, when there are none, for the caller to log.
/// `placed` is every screen segment as `(output start, source start, source
/// end)`; `duration_ms` the capture's; `input_count` how many inputs it
/// recorded; `tracks_so_far`/`clips_so_far` what the project will already
/// hold (the webcam's share included).
///
/// Stems REPLACE the mix (the caller mutes it), so they are placed only when
/// every input `1..=input_count` has one and they fit the track and clip
/// limits. Anything less — one stem failed and was never listed, a listed
/// file was not registered, too many inputs, too many cuts — would mute the
/// inputs left out, or produce a project `validate_project` refuses, so the
/// capture migrates exactly as with stems off instead.
///
/// **`input_count == 0 || stems.is_empty()` is silent** (D-3): a capture
/// that recorded no audio at all (or kept none) has no stem that "failed"
/// — there is nothing to stem. Before this guard, the empty range
/// `1..=0` still collected to `Some(vec![])` rather than `None`, so
/// EVERY no-audio capture read as "0 stem(s) for 0 input(s) not placed
/// (track or clip limit)" and warned on every migration.
/// `stem_parts`' own return shape, named so clippy's `type_complexity` lint
/// (and a human) can read the signature at a glance: the stems' assets,
/// tracks and clips, plus the reason there are none.
type StemParts = (
    Option<(Vec<Asset>, Vec<Track>, Vec<Clip>)>,
    Option<StemSkipReason>,
);

pub(super) fn stem_parts(
    stems: &[StemInput],
    input_count: usize,
    placed: &[(u64, u64, u64)],
    duration_ms: u64,
    (tracks_so_far, clips_so_far): (usize, usize),
) -> StemParts {
    if input_count == 0 || stems.is_empty() {
        return (None, None);
    }
    // One stem per input, in input order: the first entry for each index.
    let covering: Option<Vec<&StemInput>> = (1..=input_count)
        .map(|index| stems.iter().find(|s| usize::try_from(s.index) == Ok(index)))
        .collect();
    let Some(covering) = covering else {
        return (None, None);
    };
    let fits = tracks_so_far + covering.len() <= limits::MAX_TRACKS
        && clips_so_far + covering.len() * placed.len() <= limits::MAX_CLIPS;
    if !fits {
        return (
            None,
            Some(StemSkipReason::LimitExceeded {
                covering: covering.len(),
            }),
        );
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
    (Some((assets, tracks, clips)), None)
}
