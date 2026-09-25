//! Asset-link acyclicity and pairwise-transition semantics
//! (`DATA-MODEL.md` § Fade and transition rules, § Validation order steps
//! 5-6) — split out of `validate.rs` so that file stays under the 800-line
//! Rust cap. Private to `editor`; `validate::validate_project` is the only
//! caller.

use std::collections::{HashMap, HashSet};

use super::error::{EditorError, EditorErrorCode};
use super::model::{Asset, AssetKind, Clip, MediaType};
use super::model_cues::{Transition, TransitionKind};
use super::validate::{output_duration_ms, speed_or_default};

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

/// Every `linked_asset` must resolve, and the link graph must be acyclic.
/// A white/gray/black-marked DFS catches an indirect cycle (a→b→c→a), not
/// only a direct one (`DATA-MODEL.md` § Validation order step 5: "Reject
/// cyclic/dangling linked assets").
///
/// Then its IDENTITY (Task 27, F-24; the reference editor's own rule,
/// `session-safety.js`): a link is detached audio, so the linked asset is
/// `audio`, its root is a direct (itself unlinked) non-image `video`, and
/// the two share one duration -- `commands::mix::detach_audio` makes
/// exactly that, and a hand-edited project claiming anything else would
/// point the render at media that is not what the asset says it is.
/// Deliberately NOT the reference's "root is not builtin": a staged
/// capture's asset is `builtin: screen` here AND has a real file in
/// `sources.json`, which the reference's synthesized builtins never had.
/// Checked AFTER the cycle walk, so a cycle is still reported as one.
pub(super) fn check_linked_assets(assets: &[Asset]) -> Result<(), EditorError> {
    let by_id: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();
    for asset in assets {
        if let Some(linked) = &asset.linked_asset {
            if !by_id.contains_key(linked.as_str()) {
                return Err(invalid(format!(
                    "asset {}: linked_asset {} does not resolve",
                    asset.id, linked
                )));
            }
        }
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Colour {
        White,
        Gray,
        Black,
    }

    fn visit<'a>(
        id: &'a str,
        by_id: &HashMap<&'a str, &'a Asset>,
        colour: &mut HashMap<&'a str, Colour>,
    ) -> Result<(), EditorError> {
        match colour.get(id) {
            Some(Colour::Black) => return Ok(()),
            Some(Colour::Gray) => {
                return Err(invalid(format!(
                    "asset {id}: linked_asset graph has a cycle"
                )))
            }
            _ => {}
        }
        colour.insert(id, Colour::Gray);
        if let Some(linked) = by_id[id].linked_asset.as_deref() {
            visit(linked, by_id, colour)?;
        }
        colour.insert(id, Colour::Black);
        Ok(())
    }

    let mut colour: HashMap<&str, Colour> = by_id.keys().map(|id| (*id, Colour::White)).collect();
    for id in by_id.keys().copied().collect::<Vec<_>>() {
        visit(id, &by_id, &mut colour)?;
    }

    for asset in assets {
        let Some(root) = asset.linked_asset.as_deref().map(|id| by_id[id]) else {
            continue;
        };
        let root_is_video = root.kind == AssetKind::Video
            && !matches!(root.media_type, Some(MediaType::Image))
            && root.linked_asset.is_none();
        if asset.kind != AssetKind::Audio || !root_is_video || asset.duration_ms != root.duration_ms
        {
            return Err(invalid(format!(
                "asset {}: a linked asset must be audio detached from a direct video of the same duration ({} is not)",
                asset.id, root.id
            )));
        }
    }
    Ok(())
}

/// The geometry every installed transition must keep (`DATA-MODEL.md` §
/// Fade and transition rules; Task 30, F-19): both clips on one track, the
/// `from` clip ending exactly `duration_ms` AFTER the `to` clip starts --
/// the overlap `addTransition` creates by shifting `to` (and everything
/// after it on that track) earlier -- and a positive duration no longer
/// than half the shorter clip. Returns the human reason, or `None`.
///
/// `pub(super)`: the ONE statement of this rule. `validate_project` wraps
/// a failure as `invalidProject` (a graph that should never have been
/// installed), while `commands::transitions::ensure_intact` wraps the same
/// text as `invalidRequest` for a trim/move/split/ripple that would break
/// an existing transition -- two callers, one geometry, never two copies.
pub(super) fn transition_geometry_error(t: &Transition, from: &Clip, to: &Clip) -> Option<String> {
    if from.track_id != to.track_id {
        return Some(format!(
            "transition {}: from clip {} and to clip {} are on different tracks",
            t.id, from.id, to.id
        ));
    }
    let from_duration = output_duration_ms(
        from.in_ms,
        from.out_ms,
        speed_or_default(from.speed.as_ref()),
    );
    let to_duration = output_duration_ms(to.in_ms, to.out_ms, speed_or_default(to.speed.as_ref()));
    let from_end = from.start_ms.saturating_add(from_duration);
    if from_end.checked_sub(to.start_ms) != Some(t.duration_ms) {
        return Some(format!(
            "transition {}: clip {} must end exactly {} ms after clip {} starts (ends at {from_end}, starts at {})",
            t.id, from.id, t.duration_ms, to.id, to.start_ms
        ));
    }
    let shorter = from_duration.min(to_duration);
    if t.duration_ms == 0 || t.duration_ms.saturating_mul(2) > shorter {
        return Some(format!(
            "transition {}: duration_ms {} must be positive and at most half the shorter clip ({shorter} ms)",
            t.id, t.duration_ms
        ));
    }
    None
}

/// The one transition kind a clip of `kind` may carry (F-19: "Explicit
/// same-track video dissolve or equal-power audio crossfade").
pub(super) fn transition_kind_for(kind: AssetKind) -> TransitionKind {
    match kind {
        AssetKind::Video => TransitionKind::Dissolve,
        AssetKind::Audio => TransitionKind::EqualPower,
    }
}

/// Pairwise transitions (`DATA-MODEL.md` § Fade and transition rules): no
/// dangling, self-referential or cross-kind transition is valid, and the
/// reference model permits one paired transition association per clip
/// SIDE (a clip may be the `to` of one and the `from` of another).
///
/// **Cross-kind is the transition's OWN kind against its clips' media**
/// (Task 30): a `dissolve` joins video clips, an `equal-power` crossfade
/// joins audio clips. The earlier from-asset-vs-to-asset comparison this
/// replaced was unreachable (`check_clip` already ties a clip's asset kind
/// to its track's kind, and both clips share one track), so it could never
/// reject anything; this rule can.
pub(super) fn check_transitions(
    transitions: &[Transition],
    clips: &HashMap<&str, &Clip>,
    assets: &HashMap<&str, &Asset>,
) -> Result<(), EditorError> {
    let mut from_seen: HashSet<&str> = HashSet::new();
    let mut to_seen: HashSet<&str> = HashSet::new();

    for t in transitions {
        if t.from == t.to {
            return Err(invalid(format!(
                "transition {}: from and to must differ",
                t.id
            )));
        }
        let from = *clips.get(t.from.as_str()).ok_or_else(|| {
            invalid(format!(
                "transition {}: from clip {} does not resolve",
                t.id, t.from
            ))
        })?;
        let to = *clips.get(t.to.as_str()).ok_or_else(|| {
            invalid(format!(
                "transition {}: to clip {} does not resolve",
                t.id, t.to
            ))
        })?;

        if let Some(reason) = transition_geometry_error(t, from, to) {
            return Err(invalid(reason));
        }

        if let Some(asset) = assets.get(from.asset_id.as_str()) {
            let expected = transition_kind_for(asset.kind);
            if t.kind != expected {
                return Err(invalid(format!(
                    "transition {}: a {:?} clip needs a {expected:?} transition, not {:?}",
                    t.id, asset.kind, t.kind
                )));
            }
        }

        if !from_seen.insert(t.from.as_str()) {
            return Err(invalid(format!(
                "transition {}: clip {} already has a transition on this side",
                t.id, t.from
            )));
        }
        if !to_seen.insert(t.to.as_str()) {
            return Err(invalid(format!(
                "transition {}: clip {} already has a transition on this side",
                t.id, t.to
            )));
        }
    }
    Ok(())
}

/// No two clips on one track may overlap, EXCEPT a transitioned pair by
/// exactly its transition's window (F-19: "transitions cannot dangle or
/// create unexplained overlaps", IMPLEMENTATION-PLAN.md). Runs AFTER
/// `check_transitions`, so every transition's own geometry is already
/// known good here -- this only has to ask "is this overlap one of them".
/// Each clip is compared against the latest-ending clip seen so far on its
/// track (sorted by start), which catches an overlap with ANY earlier clip,
/// not only the immediately preceding one.
pub(super) fn check_track_overlaps(
    clips: &[Clip],
    transitions: &[Transition],
) -> Result<(), EditorError> {
    let mut by_track: HashMap<&str, Vec<&Clip>> = HashMap::new();
    for clip in clips {
        by_track
            .entry(clip.track_id.as_str())
            .or_default()
            .push(clip);
    }
    let end = |c: &Clip| {
        c.start_ms.saturating_add(output_duration_ms(
            c.in_ms,
            c.out_ms,
            speed_or_default(c.speed.as_ref()),
        ))
    };
    for (track_id, mut list) in by_track {
        list.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then_with(|| a.id.cmp(&b.id)));
        let mut latest: Option<&Clip> = None;
        for clip in list {
            if let Some(prev) = latest {
                let paired = transitions
                    .iter()
                    .any(|t| t.from == prev.id && t.to == clip.id);
                if clip.start_ms < end(prev) && !paired {
                    return Err(invalid(format!(
                        "clip {} overlaps clip {} on track {track_id} without a transition between them",
                        clip.id, prev.id
                    )));
                }
            }
            if latest.is_none_or(|prev| end(clip) > end(prev)) {
                latest = Some(clip);
            }
        }
    }
    Ok(())
}
