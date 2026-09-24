//! The render's audio filter graph (tutorial-editor Task 44; F-05, F-16,
//! F-18, F-19). PURE: `RenderPlan::audio` (Task 41) in, the
//! `filter_complex` audio part out, ending at `OUT_LABEL`.
//!
//! **The shape.** Each `AudioContribution` becomes its own chain --
//! `atrim`/`asetpts` off the plan's source range, a speed change, its own
//! gain and edge fades (`own_chain`) -- then either goes straight to
//! `adelay` (positioning it in the output) or, when it is one half of a
//! planned crossfade (Task 41 plans both: `crossfade_out` on the `from`
//! clip, `crossfade_in` on the `to` clip), is combined with its partner
//! through ONE `acrossfade` first (`emit_group`, mirroring `video_graph::
//! group`'s pairing rule for the picture side). Every resulting stream is
//! summed by `amix`, then the whole mix gets the project's master gain, a
//! sample-peak limiter, and is pinned to the capture's own rate and layout.
//! **No contribution at all** still produces a real stream (`anullsrc`,
//! trimmed to the plan's duration) -- the output always carries an audio
//! track, matching the capture format, so nothing downstream has to treat
//! "no audio" as a special case.
//!
//! **`adelay` adds ON TOP of whatever PTS offset its input already
//! carries**, rather than resetting it (measured against ffmpeg 9.0.1: a
//! stream whose PTS was pre-shifted 0.5 s and then `adelay=1000|1000`'d
//! starts its content at 1.5 s of silence, not 1.0 s -- confirmed with
//! `silencedetect`). So `own_chain` always resets its stream back to a
//! zero-based clock immediately before handing it to the caller, even
//! though it may have shifted that same clock away from zero a few filters
//! earlier to position a fade correctly (next paragraph).
//!
//! **Cut-aware fades, like `video_layers::alpha`'s A13 rule.** A render
//! range can cut into a clip's own edge fade, and `afade`'s `st` cannot be
//! negative (measured: ffmpeg refuses it, "out of range"), so the ONLY way
//! to evaluate the visible remainder of a fade at the gain it should
//! already be at is to keep the fade's `st`/`d` relative to the clip's
//! ORIGINAL (uncut) edge and let the stream's own start clip the front of
//! it off. `own_chain` therefore shifts its clock forward by `cut.head_ms`
//! (mirroring `video_layers::media_clock`'s `plus_seconds(head_ms)`)
//! before applying `volume`/`afade`, then resets to zero afterward for
//! `adelay` (previous paragraph). A fade the cut consumed entirely
//! (`fade_in <= cut.head_ms` / `fade_out <= cut.tail_ms`) is not emitted
//! at all -- already fully faded in, or already silent, before the visible
//! window even starts.
//!
//! **Curve names are ffmpeg's own, never evaluated here** (`curve_name`):
//! `linear -> tri`, `smooth -> hsin`, `equal-power -> qsin` (F23). `hsin`
//! is a close but not bit-identical stand-in for `smooth`'s smoothstep
//! polynomial -- the residual is `docs/Gaps.md` GAP-173, Task 29's own
//! entry, extended rather than duplicated by this task. A crossfade's
//! `TransitionKind` reads the SAME vocabulary as an audio curve
//! (`transition_curve`): `Dissolve` is a plain linear cross-fade (`tri`),
//! `EqualPower` is the constant-loudness curve its name promises (`qsin`)
//! -- `video_graph`'s module doc: "equal-power is an AUDIO curve (Task
//! 44)", where the picture side dissolves both kinds identically.
//!
//! **An orphaned crossfade half never panics.** A render range can cut
//! through a transition's overlap and drop only ONE of its two clips out
//! of the plan (`render_plan::place` filters each clip against the window
//! independently), leaving a contribution whose `crossfade_in`/
//! `crossfade_out` names a transition whose partner is simply not present.
//! `acrossfade` needs two streams, so `groups` cannot pair it with
//! anything and it falls back to a PLAIN edge fade using the transition's
//! own curve and duration (`orphan_fade`) -- Cut-aware the same way an
//! ordinary fade is, so a range that ALSO ate the remainder of that
//! envelope emits nothing rather than a fade with no visible effect.

use vault_buddy_core::editor::model::FadeCurve;
use vault_buddy_core::editor::model_cues::TransitionKind;
use vault_buddy_core::editor::render_plan::{AudioContribution, RenderPlan};

use super::expr::{num, plus_seconds, seconds};

/// The finished, limited, stereo mix.
pub const OUT_LABEL: &str = "[aout]";

/// The sample rate every audio path converges on -- the capture's own rate
/// (AGENTS.md: WASAPI/cpal capture), and what the silent fallback matches.
const SAMPLE_RATE: u32 = 48_000;

/// -1 dBFS sample-peak headroom (module doc): a peak limiter, never
/// loudness normalisation -- it only ever pulls a hot sample back down to
/// this ceiling, never raises a quiet mix.
const LIMITER: f64 = 0.891;

/// The audio `filter_complex` part for `plan`, ending at `OUT_LABEL` (see
/// the module doc for the shape).
pub fn build_audio_graph(plan: &RenderPlan) -> String {
    if plan.audio.is_empty() {
        return format!(
            "anullsrc=r={SAMPLE_RATE}:cl=stereo,atrim=end={}{OUT_LABEL}",
            seconds(plan.duration_ms)
        );
    }
    let mut chains = Vec::new();
    let mut mixed = Vec::new();
    for (k, group) in groups(&plan.audio).into_iter().enumerate() {
        let (label, start) = emit_group(&group, k, &mut chains);
        let delayed = format!("[m{k}]");
        chains.push(format!("{label}adelay={start}|{start}{delayed}"));
        mixed.push(delayed);
    }
    chains.push(format!(
        "{}amix=inputs={}:normalize=0:dropout_transition=0,volume={},\
         alimiter=limit={},aresample={SAMPLE_RATE},aformat=channel_layouts=stereo{OUT_LABEL}",
        mixed.join(""),
        mixed.len(),
        num(plan.master_gain),
        num(LIMITER),
    ));
    chains.join(";")
}

/// Consecutive same-track contributions joined by a planned crossfade
/// (mirrors `video_graph::group`'s pairing rule for the SAME "from"/"to"
/// halves Task 41 plans) become one group; everything else stays alone.
/// Sorted locally by `(track_index, output_start)` first so adjacency is
/// meaningful -- `RenderPlan::audio` itself is time-then-track ordered.
fn groups(contributions: &[AudioContribution]) -> Vec<Vec<&AudioContribution>> {
    let mut sorted: Vec<&AudioContribution> = contributions.iter().collect();
    sorted.sort_by_key(|c| (c.track_index, c.output_start));
    let mut units: Vec<Vec<&AudioContribution>> = Vec::new();
    for c in sorted {
        let joins = units
            .last()
            .and_then(|u| u.last())
            .is_some_and(|prev: &&AudioContribution| {
                prev.track_index == c.track_index
                    && prev.crossfade_out.is_some()
                    && c.crossfade_in.is_some()
                    && c.output_start < prev.output_end
            });
        match units.last_mut() {
            Some(u) if joins => u.push(c),
            _ => units.push(vec![c]),
        }
    }
    units
}

/// One group's chains, pushed onto `chains`; returns its label (ready for
/// `adelay`) and the output-time ms it should be delayed to.
fn emit_group(group: &[&AudioContribution], k: usize, chains: &mut Vec<String>) -> (String, u64) {
    match group {
        [only] => {
            let label = format!("[o{k}]");
            chains.push(format!("{}{label}", own_chain(only, true)));
            (label, only.output_start)
        }
        [from, to, ..] => {
            // The reference model permits one paired transition per clip
            // (`model_cues::Transition`'s own doc); a third member here
            // would mean two transitions claim the same clip, which is not
            // a shape this plan produces. Folding it in as a second pair
            // member rather than asserting keeps the "without panicking"
            // brief even if that assumption is ever wrong.
            let from_label = format!("[o{k}f]");
            let to_label = format!("[o{k}t]");
            chains.push(format!("{}{from_label}", own_chain(from, false)));
            chains.push(format!("{}{to_label}", own_chain(to, false)));
            let (kind, duration_ms) = to
                .crossfade_in
                .or(from.crossfade_out)
                .unwrap_or((TransitionKind::Dissolve, 0));
            let curve = transition_curve(kind);
            let merged = format!("[x{k}]");
            chains.push(format!(
                "{from_label}{to_label}acrossfade=d={}:c1={curve}:c2={curve}{merged}",
                seconds(duration_ms)
            ));
            (merged, from.output_start)
        }
        [] => unreachable!("groups() never emits an empty unit"),
    }
}

/// One contribution's trim, speed, gain and edge fades -- NOT yet delayed
/// into place (`emit_group` does that, alone or paired through
/// `acrossfade`; see the module doc for why `adelay` needs a zero-based
/// clock and why the fades need a head-shifted one first).
///
/// `treat_crossfade_as_fade`: true for a solo contribution, where an
/// orphaned crossfade half (module doc) becomes a plain edge fade; false
/// for a paired member, where `acrossfade` already carries that curve.
fn own_chain(c: &AudioContribution, treat_crossfade_as_fade: bool) -> String {
    let mut f = vec![
        format!(
            "atrim=start={}:end={}",
            seconds(c.source_in),
            seconds(c.source_out)
        ),
        "asetpts=PTS-STARTPTS".to_string(),
    ];
    f.extend(speed_filters(c.speed, c.preserve_pitch));
    let shifted = c.cut.head_ms > 0;
    if shifted {
        f.push(format!(
            "asetpts=PTS{}/TB",
            plus_seconds(c.cut.head_ms as i64)
        ));
    }
    f.push(format!("volume={}", num(c.gain)));
    f.extend(fade_filters(c));
    if treat_crossfade_as_fade {
        f.extend(orphan_fade(c));
    }
    if shifted {
        f.push("asetpts=PTS-STARTPTS".to_string());
    }
    format!("[{}:a]{}", c.input, f.join(","))
}

/// `speed`'s filters: an `atempo` chain that preserves pitch, or a real
/// resample that does not. Unity speed emits nothing -- there is no
/// perceptible difference and one fewer filter to escape/parse.
fn speed_filters(speed: f64, preserve_pitch: bool) -> Vec<String> {
    if speed == 1.0 {
        return Vec::new();
    }
    if preserve_pitch {
        tempo_factors(speed)
            .into_iter()
            .map(|factor| format!("atempo={}", num(factor)))
            .collect()
    } else {
        vec![
            format!("asetrate={SAMPLE_RATE}*{}", num(speed)),
            format!("aresample={SAMPLE_RATE}"),
        ]
    }
}

/// `speed` split into factors `atempo` accepts (each in ITS OWN documented
/// 0.5..=2.0 range: `ffmpeg -h filter=atempo` refuses anything outside it)
/// whose product is `speed` -- repeatedly halve (or double) the remainder
/// until it lands in range. `4.0` -> `[2.0, 2.0]`; `0.25` -> `[0.5, 0.5]`
/// (the brief's own worked examples). The project's own speed contract
/// (`0.25..=4.0`, `global-constraints.md`) never needs a third factor.
fn tempo_factors(speed: f64) -> Vec<f64> {
    let mut remaining = speed;
    let mut factors = Vec::new();
    while remaining > 2.0 {
        factors.push(2.0);
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        factors.push(0.5);
        remaining /= 0.5;
    }
    factors.push(remaining);
    factors
}

/// `curve` as ffmpeg's own `afade`/`acrossfade` curve name (F23: `hsin`,
/// never `esin`, for `smooth` -- see the module doc / GAP-173).
fn curve_name(curve: FadeCurve) -> &'static str {
    match curve {
        FadeCurve::Linear => "tri",
        FadeCurve::Smooth => "hsin",
        FadeCurve::EqualPower => "qsin",
    }
}

/// A crossfade's picture-dissolve kind read as an audio curve (module
/// doc): `Dissolve` is a plain linear cross-fade, `EqualPower` is the
/// constant-loudness curve its name promises.
fn transition_curve(kind: TransitionKind) -> &'static str {
    match kind {
        TransitionKind::Dissolve => "tri",
        TransitionKind::EqualPower => "qsin",
    }
}

/// The contribution's OWN edge fades, Cut-aware like `video_layers::alpha`
/// (module doc): a fade the range cut off entirely is not emitted, and the
/// fade is evaluated on the clock `own_chain` has already shifted to the
/// clip's original (uncut) start so the visible remainder reaches the
/// gain it should already be at.
fn fade_filters(c: &AudioContribution) -> Vec<String> {
    let curve = curve_name(c.curve);
    [
        edge_fade_in(c, c.fade_in, curve),
        edge_fade_out(c, c.fade_out, curve),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// An orphaned crossfade half's fallback (module doc): its OWN
/// transition's duration and curve, read as a plain edge fade at whichever
/// edge the transition named -- `crossfade_in` fades the clip IN,
/// `crossfade_out` fades it OUT, each with its own curve (independent, in
/// case both edges are orphaned crossfades of different kinds), same Cut
/// guard as an ordinary fade.
fn orphan_fade(c: &AudioContribution) -> Vec<String> {
    let mut f = Vec::new();
    if let Some((kind, ms)) = c.crossfade_in {
        f.extend(edge_fade_in(c, ms, transition_curve(kind)));
    }
    if let Some((kind, ms)) = c.crossfade_out {
        f.extend(edge_fade_out(c, ms, transition_curve(kind)));
    }
    f
}

/// A fade-in of `duration` ms at `curve`, unless the range's own `Cut`
/// already consumed all of it (`duration <= cut.head_ms`).
fn edge_fade_in(c: &AudioContribution, duration: u64, curve: &str) -> Option<String> {
    (duration > c.cut.head_ms)
        .then(|| format!("afade=t=in:st=0.000:d={}:curve={curve}", seconds(duration)))
}

/// A fade-out of `duration` ms at `curve`, positioned at the clip's
/// ORIGINAL (uncut) tail, unless the range's own `Cut` already consumed
/// all of it (`duration <= cut.tail_ms`).
fn edge_fade_out(c: &AudioContribution, duration: u64, curve: &str) -> Option<String> {
    (duration > c.cut.tail_ms).then(|| {
        let original_len = c.cut.head_ms + (c.output_end - c.output_start) + c.cut.tail_ms;
        format!(
            "afade=t=out:st={}:d={}:curve={curve}",
            seconds(original_len.saturating_sub(duration)),
            seconds(duration)
        )
    })
}
