//! The render plan's audio half (Task 41; F-05, A06), split out of
//! `render_plan.rs` for the 800-line cap: which clips reach the EXPORT mix,
//! and at what gain.
//!
//! **Who is heard.** A clip on a visible track (`render_plan` drops hidden
//! tracks before asking) whose source has an audio stream, unless the clip
//! is muted, its track is muted, or another track is soloed and this one is
//! not -- `commands::tracks`' documented solo rule, the one
//! `src/editor/mixRules.ts`' `isTrackAudible` mirrors: once ANY track is
//! soloed only soloed tracks are audible, and a soloed track's own mute
//! still wins. A detached audio clip reads its bytes from its video's
//! record (`render_plan` resolves the input); the original clip was muted
//! by `detachAudio`, so the sound is never counted twice.
//!
//! **Gain** is clip volume x track volume. The master gain is NOT folded
//! in: it is applied once, to the summed mix (`RenderPlan::master_gain`).
//! Nothing here reads the workspace -- monitoring mute and monitoring
//! volume change what the user hears while previewing, never the export
//! (A06), and `render_plan::plan`'s signature has no way to pass them.

use super::model::{Clip, FadeCurve, Project, Track};
use super::model_cues::TransitionKind;
use super::render_plan::{f64_of, transition_into, transition_out_of, Cut, Placed};

/// One clip's contribution to the mix, OUTPUT time.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioContribution {
    pub clip_id: String,
    /// `Project.tracks` index of the clip's track.
    pub track_index: usize,
    pub input: usize,
    pub output_start: u64,
    pub output_end: u64,
    pub source_in: u64,
    pub source_out: u64,
    pub speed: f64,
    /// `true` unless the clip says otherwise (the preview's default).
    pub preserve_pitch: bool,
    /// Clip volume x track volume; the master is applied once, apart.
    pub gain: f64,
    pub fade_in: u64,
    pub fade_out: u64,
    pub curve: FadeCurve,
    /// The transition this clip is the `to` side of: its audio crossfades
    /// in over that many ms while the `from` clip fades out.
    pub crossfade_in: Option<(TransitionKind, u64)>,
    /// The transition this clip is the `from` side of: its audio fades out
    /// over the last that-many ms of its original span.
    pub crossfade_out: Option<(TransitionKind, u64)>,
    pub cut: Cut,
}

/// Does `track` reach the mix at all -- its own mute, then the solo rule?
fn track_is_audible(track: &Track, tracks: &[Track]) -> bool {
    !track.muted && (!tracks.iter().any(|t| t.solo) || track.solo)
}

/// Would `clip` on `track` be heard, if its source has sound?
pub(super) fn is_audible(tracks: &[Track], clip: &Clip, track: &Track) -> bool {
    !clip.muted && track_is_audible(track, tracks)
}

pub(super) fn contribution(
    project: &Project,
    clip: &Clip,
    track_index: usize,
    input: usize,
    placed: &Placed,
) -> AudioContribution {
    let track = &project.tracks[track_index];
    AudioContribution {
        clip_id: clip.id.clone(),
        track_index,
        input,
        output_start: placed.output_start,
        output_end: placed.output_end,
        source_in: placed.source_in,
        source_out: placed.source_out,
        speed: placed.speed,
        preserve_pitch: clip.preserve_pitch.unwrap_or(true),
        gain: f64_of(Some(&clip.volume), 1.0) * f64_of(Some(&track.volume), 1.0),
        fade_in: clip.fade_in_ms,
        fade_out: clip.fade_out_ms,
        curve: clip.fade_curve,
        crossfade_in: transition_into(project, &clip.id),
        crossfade_out: transition_out_of(project, &clip.id),
        cut: placed.cut,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::{Map, Num, TrackKind};

    // Final review I4: `src/editor/mixRules.ts`' `isTrackAudible` (the
    // preview and the mixer's "silenced by solo" label) restates this rule
    // in TypeScript. Both read this one table by the same relative path —
    // `include_str!`, so moving it breaks this build — and a disagreement
    // reddens a suite instead of making the preview play what the render
    // drops (the `editor-time-cases.json` precedent).
    const SHARED: &str = include_str!("../../../../tests/fixtures/editor-audibility-cases.json");

    fn track(index: usize, flags: &serde_json::Value) -> Track {
        Track {
            id: format!("a{index}"),
            kind: TrackKind::Audio,
            name: format!("Audio {index}"),
            visible: true,
            locked: false,
            muted: flags["muted"].as_bool().expect("muted"),
            solo: flags["solo"].as_bool().expect("solo"),
            volume: Num::from(1),
            extra: Map::new(),
        }
    }

    #[test]
    fn the_shared_audibility_table_agrees_with_the_render() {
        let table: serde_json::Value = serde_json::from_str(SHARED).expect("JSON");
        let cases = table["cases"].as_array().expect("cases");
        // The TypeScript half asserts the same count against the same file.
        assert_eq!(cases.len(), 8, "the shared table lost or gained a case");
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let tracks: Vec<Track> = case["tracks"]
                .as_array()
                .expect("tracks")
                .iter()
                .enumerate()
                .map(|(i, flags)| track(i, flags))
                .collect();
            let audible: Vec<bool> = tracks
                .iter()
                .map(|t| track_is_audible(t, &tracks))
                .collect();
            let expected: Vec<bool> = case["audible"]
                .as_array()
                .expect("audible")
                .iter()
                .map(|v| v.as_bool().expect("bool"))
                .collect();
            assert_eq!(audible, expected, "{name}");
        }
    }
}
