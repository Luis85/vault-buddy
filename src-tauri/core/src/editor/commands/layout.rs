//! `setSpeed{clipId, speed, preservePitch}`, `setLayout{clipIds, …}`,
//! `setCanvas{width, height}` and `setAdjustments{clipIds, adjustments}`
//! (Tasks 31-32; F-16, F-21, F-23, F-38, F-39; DATA-MODEL.md § Timing
//! rules, § Layout and compositing properties). ADR §3 names this file
//! `commands::layout` for the whole speed/layout/canvas/colour family.
//!
//! **`setCanvas` accepts only the four supported presets** (F-38,
//! `limits::CANVASES` -- the SAME list `migrate::nearest_canvas` and
//! `validate::check_canvas` already read, never re-grown here) and refuses
//! a pair equal to the project's current canvas as a no-op (the
//! `setSpeed`/mandatory-field precedent). "Review crop, text and caption
//! placement" is deliberately NOT a flag this command writes: whether a
//! text/caption/effect box now sits outside the safe area, or a source's
//! aspect no longer matches, is a COMPUTED check (Task 54) evaluated
//! against whatever canvas is current, so there is nothing here that could
//! go stale. The frontend shows a one-time toast after a successful call
//! instead, pointing at Checks.
//!
//! **`setAdjustments` sets or clears brightness/contrast/saturation/sepia/
//! grayscale identically on every target** (F-39), the `setLayout` whole-
//! selection/atomic discipline: every target is resolved, confirmed on an
//! unlocked VIDEO track, and confirmed NOT a title-card clip before
//! anything changes. A clip is a "card" by its ASSET's `builtin` kind
//! (`Builtin::Card`) -- never by the clip's own inline `Card` content
//! (`Clip.card`, which is what a card LOOKS like, not what makes it one) --
//! the same identification `src/editor/previewLayers.ts`'s
//! `BUILTIN_HAS_FILE` makes on the frontend (task brief): colour treats
//! footage, and a card has none. `adjustments: None` CLEARS -- the whole
//! object is nullable on the wire, never merged field by field, so a
//! clip's `adjustments` is simply replaced with whatever the payload
//! carries, `None` included. Ranges (`model::Adjustments`'s own doc:
//! every field required once the object is present at all) mirror
//! DATA-MODEL.md: brightness/contrast `[0.25, 2]`, saturation `[0, 2]`,
//! sepia/grayscale `[0, 1]`.
//!
//! **Speed changes the OUTPUT span, never the source range or any cue.**
//! A clip's output duration is `round((out_ms - in_ms) / speed)`
//! (`time::clip_output_duration`, the one formula), so `setSpeed` rewrites
//! exactly one number and every clip-linked cue -- effects, captions,
//! markers, all SOURCE time -- follows through the mapping without being
//! touched (`time::cue_output_span`). Other clips never move: a slowdown
//! that would run into the next clip on the track is REFUSED with a message
//! saying to make room first; nothing ripples to make room automatically
//! (the reference editor's "ripple this track" behaviour is deliberately
//! not reproduced -- a speed change silently moving other clips is exactly
//! the surprise the brief rules out).
//!
//! Two rules carried in from earlier tasks, both because speed changes a
//! clip's output duration exactly the way a trim does:
//! - **Fades are clamped, never refused** (Task 29, F11): each edge fade
//!   longer than `fades::fade_limit` of the NEW duration is clamped to it,
//!   and the undo label names the adjustment ("Change speed (fades
//!   adjusted)"), `clips::trim_clip`'s own arm restated for speed.
//! - **Transitions are respected** (Task 30): the overlap scan is
//!   `transitions::overlap_refusal` (the one overlap a transition allows),
//!   and `transitions::ensure_intact` refuses a speed that would break a
//!   transition's geometry -- e.g. a faster `to` clip whose half-duration
//!   drops below the dissolve -- up front as `invalidRequest`, rather than
//!   leaving it for `validate_project`'s `invalidProject` backstop.
//!
//! **Layout values are the schema's ranges** (`workspace.schema.json`,
//! mirrored by `validate::check_clip`): `x`/`y`/`opacity`/`cropX`/`cropY`
//! in `[0,1]`, `w`/`h` in `[0.1,1]`, `cropZoom` in `[1,3]`; rotation is a
//! quarter turn by construction (`model::Rotation`'s own decoder), and
//! `fit`/`frameShape` are closed enums. A box is NOT required to stay
//! inside the frame here: the schema does not require it, and the frontend
//! (`layoutGeometry.clampBox`) is where a drag or preset keeps it inside.
//! A multi-clip `setLayout` applies the same values to every target and is
//! atomic: every target is resolved and checked before anything changes.

use std::collections::HashSet;

use super::clips::{
    checked_output_end, ensure_unlocked, find_asset, find_clip, find_track, invalid_request,
};
use super::fades::fade_limit;
use super::payloads::{SetAdjustmentsPayload, SetCanvasPayload, SetLayoutPayload, SetSpeedPayload};
use super::transitions;
use crate::editor::error::EditorError;
use crate::editor::limits;
use crate::editor::model::{Adjustments, Builtin, Canvas, Clip, Project, TrackKind};
use crate::editor::time::{self, ClipSpan};
use crate::editor::validate::speed_or_default;
use crate::editor::Num;

/// `value` as a finite `f64` inside the inclusive `[lo, hi]`, or a
/// refusal naming the wire field and the range.
fn in_range(field: &str, value: &Num, lo: f64, hi: f64) -> Result<f64, EditorError> {
    match value.as_f64() {
        Some(v) if (lo..=hi).contains(&v) => Ok(v),
        _ => Err(invalid_request(format!(
            "{field} {value} must be within [{lo}, {hi}]"
        ))),
    }
}

// ---- setSpeed ---------------------------------------------------------------

/// The undo label for a speed edit: which of the two fields actually moved,
/// and whether the fade clamp ran.
fn speed_label(speed_changed: bool, preserve_pitch: bool, fades_adjusted: bool) -> String {
    let base = if speed_changed {
        "Change speed"
    } else if preserve_pitch {
        "Preserve pitch on"
    } else {
        "Preserve pitch off"
    };
    if fades_adjusted {
        format!("{base} (fades adjusted)")
    } else {
        base.to_string()
    }
}

/// Refuses a new output span `[start, end)` for `clip` that would overlap
/// another clip on its track -- except by exactly a transition's window
/// (`transitions::overlap_refusal`). A plain overlap is reworded to say
/// what to do: nothing here moves the other clip.
fn check_room(project: &Project, clip: &Clip, end: u64, speed: f64) -> Result<(), EditorError> {
    for other in project
        .clips
        .iter()
        .filter(|c| c.track_id == clip.track_id && c.id != clip.id)
    {
        let Some(err) = transitions::overlap_refusal(project, &clip.id, clip.start_ms, end, other)
        else {
            continue;
        };
        let joined = project.transitions.iter().any(|t| {
            (t.from == clip.id && t.to == other.id) || (t.from == other.id && t.to == clip.id)
        });
        if joined {
            return Err(err);
        }
        return Err(invalid_request(format!(
            "At {speed}x clip {} would end at {end} ms and overlap clip {} on this track; \
             make room first (move or trim {}) -- nothing is moved automatically",
            clip.id, other.id, other.id
        )));
    }
    Ok(())
}

/// `setSpeed{clipId, speed, preservePitch}` (F-16). Refuses an unknown or
/// locked-track clip, a speed outside `[SPEED_MIN, SPEED_MAX]`, a payload
/// that changes neither field, a result shorter than `MIN_CLIP_MS` or past
/// `MAX_DURATION_MS`, an overlap with the next clip, and a broken
/// transition. See the module doc for what it deliberately leaves alone.
pub(super) fn set_speed(
    project: &Project,
    payload: &SetSpeedPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    let speed = in_range(
        "speed",
        &payload.speed,
        limits::SPEED_MIN,
        limits::SPEED_MAX,
    )?;

    let old_speed = speed_or_default(clip.speed.as_ref());
    let speed_changed = speed != old_speed;
    if !speed_changed && clip.preserve_pitch == Some(payload.preserve_pitch) {
        return Err(invalid_request(format!(
            "clip {} already plays at {speed}x with that pitch setting",
            clip.id
        )));
    }

    let new_duration = time::clip_output_duration(clip.in_ms, clip.out_ms, speed);
    if new_duration < limits::MIN_CLIP_MS {
        return Err(invalid_request(format!(
            "At {speed}x clip {} would last {new_duration} ms, below the {} ms minimum",
            clip.id,
            limits::MIN_CLIP_MS
        )));
    }
    let new_end = checked_output_end(&ClipSpan {
        start_ms: clip.start_ms,
        in_ms: clip.in_ms,
        out_ms: clip.out_ms,
        speed,
    })?;
    if new_end > limits::MAX_DURATION_MS {
        return Err(invalid_request(format!(
            "At {speed}x clip {} would end at {new_end} ms, past the {} ms maximum",
            clip.id,
            limits::MAX_DURATION_MS
        )));
    }
    check_room(project, clip, new_end, speed)?;

    // F11 (carried from Task 29): decided against the NEW duration before
    // any mutation, so the label states exactly what is about to change.
    let limit = fade_limit(new_duration);
    let clamp_in = clip.fade_in_ms > limit;
    let clamp_out = clip.fade_out_ms > limit;

    let mut candidate = project.clone();
    if let Some(c) = candidate.clips.iter_mut().find(|c| c.id == payload.clip_id) {
        c.speed = Some(payload.speed.clone());
        c.preserve_pitch = Some(payload.preserve_pitch);
        if clamp_in {
            c.fade_in_ms = limit;
        }
        if clamp_out {
            c.fade_out_ms = limit;
        }
    }
    transitions::ensure_intact(&candidate, "Change speed")?;
    Ok((
        candidate,
        speed_label(speed_changed, payload.preserve_pitch, clamp_in || clamp_out),
    ))
}

// ---- setLayout --------------------------------------------------------------

/// The numeric fields of a `setLayout` payload, each with its wire name and
/// the schema's inclusive range (module doc).
fn numeric_fields(p: &SetLayoutPayload) -> [(&'static str, Option<&Num>, f64, f64); 8] {
    [
        ("x", p.x.as_ref(), 0.0, 1.0),
        ("y", p.y.as_ref(), 0.0, 1.0),
        ("w", p.w.as_ref(), 0.1, 1.0),
        ("h", p.h.as_ref(), 0.1, 1.0),
        ("opacity", p.opacity.as_ref(), 0.0, 1.0),
        ("cropZoom", p.crop_zoom.as_ref(), 1.0, 3.0),
        ("cropX", p.crop_x.as_ref(), 0.0, 1.0),
        ("cropY", p.crop_y.as_ref(), 0.0, 1.0),
    ]
}

fn sets_nothing(p: &SetLayoutPayload) -> bool {
    numeric_fields(p).iter().all(|(_, v, _, _)| v.is_none())
        && p.fit.is_none()
        && p.frame_shape.is_none()
        && p.rotation.is_none()
        && p.mirror.is_none()
        && p.flip_y.is_none()
}

/// Every target resolves, sits on an unlocked VIDEO track -- checked for
/// all of them before anything changes, which is what makes a multi-clip
/// `setLayout`/`setAdjustments` atomic. `command`/`noun` name the calling
/// command in the two messages this shared loop produces (fix round 1:
/// `setAdjustments` used to carry a byte-for-byte copy of this exact loop
/// under its own name, `check_color_targets`, differing only in these two
/// strings -- `check_color_targets` below now calls THIS function and adds
/// only its own extra card rule on top): `("setLayout", "layout")` (an
/// audio clip has no picture to lay out) and `("setAdjustments", "colour")`
/// (an audio clip has nothing to colour).
fn check_targets(
    project: &Project,
    clip_ids: &[String],
    command: &str,
    noun: &str,
) -> Result<(), EditorError> {
    if clip_ids.is_empty() {
        return Err(invalid_request(format!(
            "{command} needs at least one clip"
        )));
    }
    for id in clip_ids {
        let clip = find_clip(project, id)?;
        if find_track(project, &clip.track_id)?.kind != TrackKind::Video {
            return Err(invalid_request(format!(
                "clip {id} is an audio clip and has no {noun}"
            )));
        }
        ensure_unlocked(project, &clip.track_id)?;
    }
    Ok(())
}

fn apply_layout(c: &mut Clip, p: &SetLayoutPayload) {
    let set = |slot: &mut Num, v: &Option<Num>| {
        if let Some(v) = v {
            *slot = v.clone();
        }
    };
    set(&mut c.x, &p.x);
    set(&mut c.y, &p.y);
    set(&mut c.w, &p.w);
    set(&mut c.h, &p.h);
    set(&mut c.opacity, &p.opacity);
    c.fit = p.fit.or(c.fit);
    c.frame_shape = p.frame_shape.or(c.frame_shape);
    c.rotation = p.rotation.or(c.rotation);
    c.mirror = p.mirror.or(c.mirror);
    c.flip_y = p.flip_y.or(c.flip_y);
    c.crop_zoom = p.crop_zoom.clone().or(c.crop_zoom.take());
    c.crop_x = p.crop_x.clone().or(c.crop_x.take());
    c.crop_y = p.crop_y.clone().or(c.crop_y.take());
}

/// `setLayout{clipIds, x?, y?, w?, h?, opacity?, fit?, frameShape?,
/// rotation?, mirror?, flipY?, cropZoom?, cropX?, cropY?}` (F-21, F-23):
/// sets whichever fields are present, identically on every target, and
/// leaves every absent field as it was.
pub(super) fn set_layout(
    project: &Project,
    payload: &SetLayoutPayload,
) -> Result<(Project, String), EditorError> {
    if sets_nothing(payload) {
        return Err(invalid_request("setLayout must set at least one field"));
    }
    for (field, value, lo, hi) in numeric_fields(payload) {
        if let Some(v) = value {
            in_range(field, v, lo, hi)?;
        }
    }
    check_targets(project, &payload.clip_ids, "setLayout", "layout")?;

    let targets: HashSet<&str> = payload.clip_ids.iter().map(String::as_str).collect();
    let mut candidate = project.clone();
    for c in candidate
        .clips
        .iter_mut()
        .filter(|c| targets.contains(c.id.as_str()))
    {
        apply_layout(c, payload);
    }
    let label = match targets.len() {
        1 => "Change layout".to_string(),
        n => format!("Change layout ({n} clips)"),
    };
    Ok((candidate, label))
}

// ---- setCanvas ----------------------------------------------------------

/// `setCanvas{width, height}` (F-38): accepts only the four supported
/// presets and refuses a pair equal to the current canvas as a no-op. See
/// the module doc for why nothing about crop/caption placement is stored
/// here.
pub(super) fn set_canvas(
    project: &Project,
    payload: &SetCanvasPayload,
) -> Result<(Project, String), EditorError> {
    let pair = (payload.width, payload.height);
    if !limits::CANVASES.contains(&pair) {
        return Err(invalid_request(format!(
            "canvas {}x{} is not one of the four supported presets",
            payload.width, payload.height
        )));
    }
    if pair == (project.canvas.width, project.canvas.height) {
        return Err(invalid_request(format!(
            "project canvas is already {}x{}",
            payload.width, payload.height
        )));
    }
    let mut candidate = project.clone();
    candidate.canvas = Canvas {
        width: payload.width,
        height: payload.height,
        // fps is not a setCanvas field (Contract reference); the current
        // value and any forward-compat extra keys ride through untouched.
        fps: project.canvas.fps,
        extra: project.canvas.extra.clone(),
    };
    Ok((candidate, "Change canvas".to_string()))
}

// ---- setAdjustments -------------------------------------------------------

fn validate_adjustments(adjustments: &Adjustments) -> Result<(), EditorError> {
    in_range("brightness", &adjustments.brightness, 0.25, 2.0)?;
    in_range("contrast", &adjustments.contrast, 0.25, 2.0)?;
    in_range("saturation", &adjustments.saturation, 0.0, 2.0)?;
    in_range("sepia", &adjustments.sepia, 0.0, 1.0)?;
    in_range("grayscale", &adjustments.grayscale, 0.0, 1.0)?;
    Ok(())
}

/// Every target of `setAdjustments` gets `check_targets`'s shared resolve/
/// video-track/unlocked discipline (`("setAdjustments", "colour")`), plus
/// ONE rule that command alone does not need: not a title-card clip -- see
/// the module doc for how "is a card" is decided. The card pass runs
/// SECOND, over its own loop, only once `check_targets` has already proven
/// every id resolves -- so `find_asset` below can never fail on an unknown
/// clip. Checked for every target BEFORE anything changes, so a multi-clip
/// `setAdjustments` is atomic like `setLayout`.
fn check_color_targets(project: &Project, clip_ids: &[String]) -> Result<(), EditorError> {
    check_targets(project, clip_ids, "setAdjustments", "colour")?;
    for id in clip_ids {
        let clip = find_clip(project, id)?;
        if find_asset(project, &clip.asset_id)?.builtin == Some(Builtin::Card) {
            return Err(invalid_request(
                "Colour applies to footage, not title cards.",
            ));
        }
    }
    Ok(())
}

/// `setAdjustments{clipIds, adjustments: Adjustments|null}` (F-39): sets or
/// clears every target's colour identically. See the module doc for why
/// `None` clears rather than being merged field by field.
pub(super) fn set_adjustments(
    project: &Project,
    payload: &SetAdjustmentsPayload,
) -> Result<(Project, String), EditorError> {
    if let Some(adjustments) = &payload.adjustments {
        validate_adjustments(adjustments)?;
    }
    check_color_targets(project, &payload.clip_ids)?;

    let targets: HashSet<&str> = payload.clip_ids.iter().map(String::as_str).collect();
    let mut candidate = project.clone();
    for c in candidate
        .clips
        .iter_mut()
        .filter(|c| targets.contains(c.id.as_str()))
    {
        c.adjustments = payload.adjustments.clone();
    }
    let verb = if payload.adjustments.is_some() {
        "Change colour"
    } else {
        "Reset colour"
    };
    let label = match targets.len() {
        1 => verb.to_string(),
        n => format!("{verb} ({n} clips)"),
    };
    Ok((candidate, label))
}

// Tests live in the sibling `layout_tests.rs`, the `clips.rs`/
// `clips_tests.rs` precedent.
#[cfg(test)]
#[path = "layout_tests.rs"]
mod tests;
