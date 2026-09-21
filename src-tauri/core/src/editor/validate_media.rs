//! Asset-link acyclicity and pairwise-transition semantics
//! (`DATA-MODEL.md` § Fade and transition rules, § Validation order steps
//! 5-6) — split out of `validate.rs` so that file stays under the 800-line
//! Rust cap. Private to `editor`; `validate::validate_project` is the only
//! caller.

use std::collections::{HashMap, HashSet};

use super::error::{EditorError, EditorErrorCode};
use super::model::{Asset, Clip};
use super::model_cues::Transition;
use super::validate::{output_duration_ms, speed_or_default};

fn invalid(message: impl Into<String>) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

/// Every `linked_asset` must resolve, and the link graph must be acyclic.
/// A white/gray/black-marked DFS catches an indirect cycle (a→b→c→a), not
/// only a direct one (`DATA-MODEL.md` § Validation order step 5: "Reject
/// cyclic/dangling linked assets").
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
    Ok(())
}

/// Pairwise transitions (`DATA-MODEL.md` § Fade and transition rules): no
/// dangling, self-referential or cross-kind transition is valid, and the
/// reference model permits one paired transition association per clip.
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

        if from.track_id != to.track_id {
            return Err(invalid(format!(
                "transition {}: from clip {} and to clip {} are on different tracks",
                t.id, from.id, to.id
            )));
        }

        // Believed unreachable in practice: `validate::check_clip` (run on
        // every clip before this function is ever called) already rejects
        // a clip whose asset kind disagrees with its track's kind, and the
        // `from.track_id != to.track_id` check just above already requires
        // `from` and `to` to share one track — so two clips that get this
        // far necessarily share one asset kind too. Kept anyway as
        // defence: nothing in this function's own signature guarantees
        // `check_clip` ran first (a future caller could construct `clips`/
        // `assets` maps by hand), and the check is one cheap comparison.
        if let (Some(from_asset), Some(to_asset)) = (
            assets.get(from.asset_id.as_str()),
            assets.get(to.asset_id.as_str()),
        ) {
            if from_asset.kind != to_asset.kind {
                return Err(invalid(format!(
                    "transition {}: from clip {} and to clip {} use different asset kinds",
                    t.id, from.id, to.id
                )));
            }
        }

        let from_speed = speed_or_default(from.speed.as_ref());
        let to_speed = speed_or_default(to.speed.as_ref());
        let from_duration = output_duration_ms(from.in_ms, from.out_ms, from_speed);
        let to_duration = output_duration_ms(to.in_ms, to.out_ms, to_speed);

        let from_end = from.start_ms.saturating_add(from_duration);
        if from_end != to.start_ms {
            return Err(invalid(format!(
                "transition {}: clip {} must end exactly where clip {} starts (ends at {from_end}, starts at {})",
                t.id, from.id, to.id, to.start_ms
            )));
        }

        let shorter = from_duration.min(to_duration);
        if t.duration_ms == 0 || t.duration_ms.saturating_mul(2) > shorter {
            return Err(invalid(format!(
                "transition {}: duration_ms {} must be positive and at most half the shorter clip ({shorter} ms)",
                t.id, t.duration_ms
            )));
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
