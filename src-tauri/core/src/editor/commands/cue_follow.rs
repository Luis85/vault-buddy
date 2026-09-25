//! Pure cue-reassignment helpers for `splitClip` (F-08, `DATA-MODEL.md` §
//! Timing rules: "Split creates two valid source ranges and intersects/
//! reassigns linked cues; markers on the cut belong to exactly one
//! half-open child"). Split out of `clips.rs` per the task brief's own
//! allowance, since `splitClip` alone touches four different entity
//! collections (effects, captions, markers, transitions) and inlining all
//! four reassignment rules would have pushed `clips.rs` toward the
//! 800-line cap well before task 8/29/30 get to extend it.
//!
//! Every function here takes the SOURCE split point `s` (already computed
//! by `time::source_at` in `clips.rs`) and the original/right clip ids --
//! never a `ClipSpan` or the `Project` itself -- so they stay pure entity
//! surgery with no time-mapping arithmetic of their own to get wrong twice.

use crate::editor::ids::new_entity_id;
use crate::editor::model_cues::{CaptionCue, Effect, Marker, Transition};

/// Reassigns every `Effect` linked to `orig_id` around the source split
/// point `s`: entirely left of the cut (`end_ms <= s`) stays on `orig_id`
/// unchanged; entirely right of it (`start_ms >= s`) moves wholesale to
/// `right_id`; one straddling `s` splits into two effects -- the original
/// id stays on the left half (`end_ms` clamped to `s`), and a FRESH id on
/// `right_id` carries the right half (`start_ms` set to `s`, `end_ms`
/// unchanged).
pub(super) fn split_effects(effects: &mut Vec<Effect>, orig_id: &str, right_id: &str, s: u64) {
    let mut spawned = Vec::new();
    for effect in effects.iter_mut() {
        if effect.clip_id != orig_id {
            continue;
        }
        if effect.end_ms <= s {
            // Entirely left of the cut: stays on the original (left) id
            // unchanged.
        } else if effect.start_ms >= s {
            effect.clip_id = right_id.to_string();
        } else {
            let mut right_half = effect.clone();
            right_half.id = new_entity_id("effect");
            right_half.clip_id = right_id.to_string();
            right_half.start_ms = s;
            effect.end_ms = s;
            spawned.push(right_half);
        }
    }
    effects.extend(spawned);
}

/// The `CaptionCue` counterpart of `split_effects` -- identical rule, a
/// different entity shape (`DATA-MODEL.md` groups "Effects/captions" as
/// the same class of clip-linked, source-time cue).
pub(super) fn split_captions(cues: &mut Vec<CaptionCue>, orig_id: &str, right_id: &str, s: u64) {
    let mut spawned = Vec::new();
    for cue in cues.iter_mut() {
        if cue.clip_id != orig_id {
            continue;
        }
        if cue.end_ms <= s {
            // Entirely left of the cut.
        } else if cue.start_ms >= s {
            cue.clip_id = right_id.to_string();
        } else {
            let mut right_half = cue.clone();
            right_half.id = new_entity_id("caption");
            right_half.clip_id = right_id.to_string();
            right_half.start_ms = s;
            cue.end_ms = s;
            spawned.push(right_half);
        }
    }
    cues.extend(spawned);
}

/// A marker is an instant, not an interval, so it belongs to exactly one
/// half: the half-open range containing `source_ms` decides it. Left is
/// `[in_ms, s)`, right is `[s, out_ms)` -- a marker sitting exactly AT `s`
/// therefore belongs to the RIGHT half (the brief's own worked example: "a
/// marker at `s` → right half only").
pub(super) fn reassign_markers(markers: &mut [Marker], orig_id: &str, right_id: &str, s: u64) {
    for marker in markers.iter_mut() {
        if marker.clip_id == orig_id && marker.source_ms >= s {
            marker.clip_id = right_id.to_string();
        }
    }
}

/// Re-points every transition attached to the split clip's TAIL (`from ==
/// orig_id`) onto the new right half, since the tail -- and therefore the
/// transition INTO the clip that follows it -- now belongs to the right
/// piece. A transition attached to the clip's HEAD (`to == orig_id`)
/// needs no code change: the left half keeps `orig_id`, so `to` already
/// names the right entity.
pub(super) fn repoint_transitions(transitions: &mut [Transition], orig_id: &str, right_id: &str) {
    for transition in transitions.iter_mut() {
        if transition.from == orig_id {
            transition.from = right_id.to_string();
        }
    }
}
