//! `addEffect`/`updateEffect`/`removeEffect` (Task 34; F-27–F-33,
//! `DATA-MODEL.md` § Entities: "An effect's geometry/style depends on its
//! kind: text, arrow, highlight, spotlight, zoom, step, mask. Do not
//! flatten them into drawn pixels inside project storage.").
//!
//! **`start_ms`/`end_ms` are SOURCE times, stored VERBATIM.** Every other
//! clip-placement command's `startMs` is an OUTPUT time; a teaching cue's
//! is not (`global-constraints.md`'s Commands paragraph: "Effect/caption
//! startMs/endMs and marker sourceMs are SOURCE time"). `add_effect`/
//! `update_effect` therefore never call `time::output_at`/`time::source_at`
//! on the wire values at all -- they copy `payload.start_ms`/`end_ms`
//! straight onto the `Effect`, after checking the pair falls inside the
//! target clip's OWN source range `[in_ms, out_ms)`. That is what makes a
//! cue survive `moveClips`/`trimClip`/`setSpeed` untouched (Task 7's own
//! claim, verified rather than re-implemented here): those commands never
//! write `project.effects` at all, so a cue's stored times are exactly what
//! this file put there, forever, until `splitClip` reassigns them
//! (`cue_follow.rs`) or `updateEffect`/`removeEffect` touches them
//! directly. Deriving where a cue actually PAINTS at the clip's current
//! speed is `time::cue_output_span`'s job, called by a later render/preview
//! task -- never this one's.
//!
//! **Defaults per kind when omitted (the brief's own words), applied by
//! ONE overlay function for both commands.** `default_shell` builds a
//! brand-new `Effect` with every field its `EffectKind` cares about already
//! set to a sensible default (`DEFAULT_*` below); `patch_props` then
//! overlays whatever the caller's `EffectProps` variant actually carries,
//! field by field, `Option::is_some()` meaning "the caller said so." Both
//! `add_effect` (shell, then patch) and `update_effect` (the EXISTING
//! effect, cloned, then patch) call the exact same `patch_props` -- so
//! there is only ever one seven-way "which field belongs to which kind"
//! match in this file, not two independently-maintained copies of it.
//!
//! **Every numeric field `workspace.schema.json` bounds is now enforced in
//! `validate.rs`, not new to this file.** `x`/`y`/`x2`/`y2`/`dim` `[0,1]`;
//! `w`/`h` `[0.01,1]` (NOT `check_clip`'s own `0.1` floor -- a cue's box
//! can be far thinner than a video frame ever should, and the reference
//! workspace fixture proves it, down to `h: 0.05`); `factor` `[1,4]`;
//! `font_size` `[12,100]`; `stroke` `[1,20]`; `easing` `[0,10000]`;
//! `number` `[1,99]` -- see `validate.rs`'s own doc on `check_effect` for
//! the schema citation. Every default here already lands inside these, so
//! a caller who omits every optional field still gets a
//! `validate_project`-clean effect.

use crate::editor::commands::clips::{ensure_unlocked, find_clip, invalid_request};
use crate::editor::commands::payloads::{
    AddEffectPayload, AddMarkerPayload, EffectProps, RemoveEffectPayload, RemoveMarkerPayload,
    UpdateEffectPayload, UpdateMarkerPayload,
};
use crate::editor::error::EditorError;
use crate::editor::ids::new_entity_id;
use crate::editor::limits;
use crate::editor::model::{Clip, Project};
use crate::editor::model_cues::{Effect, EffectKind, Marker};
use crate::editor::time::{output_at, ClipSpan};
use crate::editor::validate::speed_or_default;
use crate::editor::{Map, Num};

/// Every kind's own default position (JS reference `drawing-primitives.js`
/// has no single canonical spot; top-left-of-centre is a reasonable,
/// harmless place for a cue nobody positioned yet).
const DEFAULT_X: f64 = 0.1;
const DEFAULT_Y: f64 = 0.1;
/// The reference renderer's own fallback (`e.color||'#ffd279'` in
/// `drawing-primitives.js`, applied to every kind before its own
/// kind-specific drawing) -- kept identical here so a cue added with no
/// colour looks the way the existing browser prototype already draws one.
const DEFAULT_COLOR: &str = "#ffd279";

const DEFAULT_TEXT: &str = "Text";
const DEFAULT_TEXT_W: f64 = 0.4;
const DEFAULT_TEXT_H: f64 = 0.15;
/// `drawing-primitives.js`'s own `e.fontSize||32`.
const DEFAULT_FONT_SIZE: i64 = 32;
const DEFAULT_TEXT_BACKGROUND: bool = false;

const DEFAULT_ARROW_X2: f64 = 0.6;
const DEFAULT_ARROW_Y2: f64 = 0.6;
/// `drawing-primitives.js`'s own `e.stroke||5` for an arrow's line weight.
const DEFAULT_ARROW_STROKE: i64 = 5;

const DEFAULT_HIGHLIGHT_W: f64 = 0.2;
const DEFAULT_HIGHLIGHT_H: f64 = 0.12;
/// `drawing-primitives.js`'s own `e.stroke||4` for a highlight's outline.
const DEFAULT_HIGHLIGHT_STROKE: i64 = 4;

const DEFAULT_SPOTLIGHT_W: f64 = 0.3;
const DEFAULT_SPOTLIGHT_H: f64 = 0.3;
/// `drawing-primitives.js`'s own `e.dim||.65`.
const DEFAULT_SPOTLIGHT_DIM: f64 = 0.65;

const DEFAULT_ZOOM_FACTOR: f64 = 2.0;
const DEFAULT_ZOOM_EASING: f64 = 0.0;

const DEFAULT_STEP_NUMBER: i64 = 1;
const DEFAULT_STEP_TEXT: &str = "Step";

const DEFAULT_MASK_W: f64 = 0.2;
const DEFAULT_MASK_H: f64 = 0.2;

fn frac(v: f64) -> Num {
    Num::from_f64(v).expect("every DEFAULT_* fraction constant above is finite")
}

fn int(v: i64) -> Num {
    Num::from(v)
}

fn find_effect<'a>(project: &'a Project, id: &str) -> Result<&'a Effect, EditorError> {
    project
        .effects
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| invalid_request(format!("effect {id} does not resolve")))
}

/// Refuses a proposed `[start_ms, end_ms)` cue span that is not strictly
/// ordered or that falls outside `clip`'s own SOURCE range -- the brief's
/// "startMs/endMs ... inside the clip's source range." Shared by
/// `add_effect` (the new cue's span) and `update_effect` (its span AFTER
/// applying whatever `startMs`/`endMs` the caller sent, defaulting to the
/// effect's current ones).
pub(super) fn check_cue_range(start_ms: u64, end_ms: u64, clip: &Clip) -> Result<(), EditorError> {
    if start_ms >= end_ms {
        return Err(invalid_request("startMs must be before endMs"));
    }
    if start_ms < clip.in_ms || end_ms > clip.out_ms {
        return Err(invalid_request(format!(
            "the cue's source range [{start_ms}, {end_ms}) must lie within clip {}'s own source range [{}, {})",
            clip.id, clip.in_ms, clip.out_ms
        )));
    }
    Ok(())
}

/// Sets `e.x`/`e.y`/`e.color` from whichever of the three `Some` values are
/// present, leaving the rest of `e` untouched -- shared by `default_shell`
/// (which already pre-seeded all three with the kind-agnostic defaults
/// above, so "untouched" there still means "the default") and `patch_props`
/// (where "untouched" means "the effect's current value").
fn apply_xy_color(e: &mut Effect, x: Option<&Num>, y: Option<&Num>, color: Option<&str>) {
    if let Some(v) = x {
        e.x = v.clone();
    }
    if let Some(v) = y {
        e.y = v.clone();
    }
    if let Some(v) = color {
        e.color = v.to_string();
    }
}

/// Overwrites `*slot` only when `incoming` carries a value -- `None` means
/// "the caller didn't say," never "clear it."
fn set_opt<T: Clone>(slot: &mut Option<T>, incoming: Option<&T>) {
    if let Some(v) = incoming {
        *slot = Some(v.clone());
    }
}

/// A brand-new `Effect` with every field ITS `kind` cares about already
/// carrying a default (the brief's "Defaults per kind when omitted"), and
/// every other kind's fields left `None` -- `patch_props` (below) is what
/// then overlays whatever the caller actually sent.
fn default_shell(
    id: String,
    clip_id: String,
    kind: EffectKind,
    start_ms: u64,
    end_ms: u64,
) -> Effect {
    let mut e = Effect {
        id,
        clip_id,
        kind,
        start_ms,
        end_ms,
        x: frac(DEFAULT_X),
        y: frac(DEFAULT_Y),
        color: DEFAULT_COLOR.to_string(),
        text: None,
        w: None,
        h: None,
        x2: None,
        y2: None,
        factor: None,
        font_size: None,
        stroke: None,
        dim: None,
        easing: None,
        number: None,
        background: None,
        extra: Map::new(),
    };
    match kind {
        EffectKind::Text => {
            e.w = Some(frac(DEFAULT_TEXT_W));
            e.h = Some(frac(DEFAULT_TEXT_H));
            e.text = Some(DEFAULT_TEXT.to_string());
            e.font_size = Some(int(DEFAULT_FONT_SIZE));
            e.background = Some(DEFAULT_TEXT_BACKGROUND);
        }
        EffectKind::Arrow => {
            e.x2 = Some(frac(DEFAULT_ARROW_X2));
            e.y2 = Some(frac(DEFAULT_ARROW_Y2));
            e.stroke = Some(int(DEFAULT_ARROW_STROKE));
        }
        EffectKind::Highlight => {
            e.w = Some(frac(DEFAULT_HIGHLIGHT_W));
            e.h = Some(frac(DEFAULT_HIGHLIGHT_H));
            e.stroke = Some(int(DEFAULT_HIGHLIGHT_STROKE));
        }
        EffectKind::Spotlight => {
            e.w = Some(frac(DEFAULT_SPOTLIGHT_W));
            e.h = Some(frac(DEFAULT_SPOTLIGHT_H));
            e.dim = Some(frac(DEFAULT_SPOTLIGHT_DIM));
        }
        EffectKind::Zoom => {
            e.factor = Some(frac(DEFAULT_ZOOM_FACTOR));
            e.easing = Some(frac(DEFAULT_ZOOM_EASING));
        }
        EffectKind::Step => {
            e.number = Some(int(DEFAULT_STEP_NUMBER));
            e.text = Some(DEFAULT_STEP_TEXT.to_string());
        }
        EffectKind::Mask => {
            e.w = Some(frac(DEFAULT_MASK_W));
            e.h = Some(frac(DEFAULT_MASK_H));
        }
    }
    e
}

/// Overlays whatever `props` carries onto `e`. Every field is `Option` on
/// the wire, so `None` always means "leave it" -- called against a fresh
/// `default_shell` by `add_effect` (so an omitted field keeps its kind's
/// default) and against the EXISTING effect by `update_effect` (so an
/// omitted field keeps its current value). `Spotlight`/`Zoom` never carry a
/// `color` field on the wire at all (the brief's own per-kind prop lists),
/// so their arms pass `None` for it rather than reading one that cannot
/// exist.
fn patch_props(e: &mut Effect, props: &EffectProps) {
    match props {
        EffectProps::Text(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), p.color.as_deref());
            set_opt(&mut e.w, p.w.as_ref());
            set_opt(&mut e.h, p.h.as_ref());
            set_opt(&mut e.text, p.text.as_ref());
            set_opt(&mut e.font_size, p.font_size.as_ref());
            set_opt(&mut e.background, p.background.as_ref());
        }
        EffectProps::Arrow(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), p.color.as_deref());
            set_opt(&mut e.x2, p.x2.as_ref());
            set_opt(&mut e.y2, p.y2.as_ref());
            set_opt(&mut e.stroke, p.stroke.as_ref());
        }
        EffectProps::Highlight(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), p.color.as_deref());
            set_opt(&mut e.w, p.w.as_ref());
            set_opt(&mut e.h, p.h.as_ref());
            set_opt(&mut e.stroke, p.stroke.as_ref());
        }
        EffectProps::Spotlight(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), None);
            set_opt(&mut e.w, p.w.as_ref());
            set_opt(&mut e.h, p.h.as_ref());
            set_opt(&mut e.dim, p.dim.as_ref());
        }
        EffectProps::Zoom(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), None);
            set_opt(&mut e.factor, p.factor.as_ref());
            set_opt(&mut e.easing, p.easing.as_ref());
        }
        EffectProps::Step(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), p.color.as_deref());
            set_opt(&mut e.number, p.number.as_ref());
            set_opt(&mut e.text, p.text.as_ref());
        }
        EffectProps::Mask(p) => {
            apply_xy_color(e, p.x.as_ref(), p.y.as_ref(), p.color.as_deref());
            set_opt(&mut e.w, p.w.as_ref());
            set_opt(&mut e.h, p.h.as_ref());
        }
    }
}

/// `addEffect{clipId, effectKind, startMs, endMs, props}`. `props` already
/// arrived decoded into the matching `EffectProps` variant (`AddEffectPayload`'s
/// own hand-written `Deserialize`), so there is nothing left to dispatch on
/// here beyond building the effect and checking its span.
pub(super) fn add_effect(
    project: &Project,
    payload: &AddEffectPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    check_cue_range(payload.start_ms, payload.end_ms, clip)?;

    // Mirrors `tracks::add_track`'s own explicit `MAX_TRACKS` check: no
    // caller of `add_effect` alone runs `validate_project` afterward, so
    // the 1200-effect ceiling would otherwise never be exercised by a unit
    // test against this function directly.
    if project.effects.len() >= limits::MAX_EFFECTS {
        return Err(invalid_request(format!(
            "adding an effect would bring the project to {} entries, exceeding the {} maximum",
            project.effects.len() + 1,
            limits::MAX_EFFECTS
        )));
    }

    let mut effect = default_shell(
        new_entity_id("effect"),
        payload.clip_id.clone(),
        payload.kind,
        payload.start_ms,
        payload.end_ms,
    );
    patch_props(&mut effect, &payload.props);

    let mut candidate = project.clone();
    candidate.effects.push(effect);
    Ok((candidate, "Add effect".to_string()))
}

/// `updateEffect{effectId, startMs?, endMs?, props?}`. An effect's `kind`
/// never changes once created (there is no wire field for it here), so
/// `props` -- a raw JSON value on this payload, unlike `addEffect`'s own
/// already-typed one -- is decoded against the EXISTING effect's kind.
pub(super) fn update_effect(
    project: &Project,
    payload: &UpdateEffectPayload,
) -> Result<(Project, String), EditorError> {
    let effect = find_effect(project, &payload.effect_id)?;
    let clip = find_clip(project, &effect.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    if payload.start_ms.is_none() && payload.end_ms.is_none() && payload.props.is_none() {
        return Err(invalid_request("updateEffect must set at least one field"));
    }
    let new_start = payload.start_ms.unwrap_or(effect.start_ms);
    let new_end = payload.end_ms.unwrap_or(effect.end_ms);
    check_cue_range(new_start, new_end, clip)?;

    let decoded = match &payload.props {
        Some(value) => Some(
            EffectProps::from_kind_and_value(effect.kind, value.clone())
                .map_err(|err| invalid_request(format!("props: {err}")))?,
        ),
        None => None,
    };

    let mut candidate = project.clone();
    let target = candidate
        .effects
        .iter_mut()
        .find(|e| e.id == payload.effect_id)
        .expect("effect_id was just resolved above against this same project");
    target.start_ms = new_start;
    target.end_ms = new_end;
    if let Some(props) = &decoded {
        patch_props(target, props);
    }
    Ok((candidate, "Update effect".to_string()))
}

/// `removeEffect{effectId}` -- an ordinary retain, gated on the owning
/// clip's track being unlocked like every other clip-linked mutation.
pub(super) fn remove_effect(
    project: &Project,
    payload: &RemoveEffectPayload,
) -> Result<(Project, String), EditorError> {
    let effect = find_effect(project, &payload.effect_id)?;
    let clip = find_clip(project, &effect.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;

    let mut candidate = project.clone();
    candidate.effects.retain(|e| e.id != payload.effect_id);
    Ok((candidate, "Remove effect".to_string()))
}

// ---- chapter markers (Task 36; F-36) ---------------------------------------
//
// A marker is an INSTANT on a clip's source, `source_ms` stored verbatim --
// the effects' rule above, for the same reason: it follows its footage
// through every move/trim/speed change for free, and through `splitClip`
// via `cue_follow::reassign_markers` (the half-open half containing it).
// Its OUTPUT time is derived on demand (`chapters` below), never stored.

/// One chapter as the timeline shows it right now: its marker, where it
/// plays in OUTPUT time, and its title.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub marker_id: String,
    pub output_ms: u64,
    pub title: String,
}

/// Every marker whose source instant is still inside its clip's CURRENT
/// source range, mapped to output time through `time::output_at`, sorted
/// by output time (then title, for a stable order between two markers on
/// the same instant). A marker trimmed out of its clip -- or orphaned by a
/// hand-edited document -- is simply not a chapter of THIS edit; the
/// marker itself stays, so extending the trim again brings it back. The
/// companion note and the render plan (Tasks 41, 48) read this list.
pub fn chapters(project: &Project) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = project
        .markers
        .iter()
        .filter_map(|m| {
            let clip = project.clips.iter().find(|c| c.id == m.clip_id)?;
            let span = ClipSpan {
                start_ms: clip.start_ms,
                in_ms: clip.in_ms,
                out_ms: clip.out_ms,
                speed: speed_or_default(clip.speed.as_ref()),
            };
            Some(Chapter {
                marker_id: m.id.clone(),
                output_ms: output_at(&span, m.source_ms)?,
                title: m.title.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.output_ms
            .cmp(&b.output_ms)
            .then_with(|| a.title.cmp(&b.title))
    });
    out
}

fn find_marker<'a>(project: &'a Project, id: &str) -> Result<&'a Marker, EditorError> {
    project
        .markers
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| invalid_request(format!("chapter marker {id} does not resolve")))
}

/// A marker must sit on a frame its clip actually plays: the HALF-OPEN
/// `[in_ms, out_ms)` (at `out_ms` it would resolve to no output time at
/// all, so `chapters` could never show it).
fn check_marker_instant(source_ms: u64, clip: &Clip) -> Result<(), EditorError> {
    if source_ms < clip.in_ms || source_ms >= clip.out_ms {
        return Err(invalid_request(format!(
            "The chapter's source time {source_ms} must lie within clip {}'s source range [{}, {})",
            clip.id, clip.in_ms, clip.out_ms
        )));
    }
    Ok(())
}

/// Trimmed, non-empty, at most `MAX_TITLE_CHARS` characters.
fn checked_title(title: &str) -> Result<String, EditorError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(invalid_request("A chapter needs a title"));
    }
    if title.chars().count() > limits::MAX_TITLE_CHARS {
        return Err(invalid_request(format!(
            "Chapter titles are limited to {} characters",
            limits::MAX_TITLE_CHARS
        )));
    }
    Ok(title.to_string())
}

/// `addMarker{clipId, sourceMs, title}`.
pub(super) fn add_marker(
    project: &Project,
    payload: &AddMarkerPayload,
) -> Result<(Project, String), EditorError> {
    let clip = find_clip(project, &payload.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    check_marker_instant(payload.source_ms, clip)?;
    let title = checked_title(&payload.title)?;
    if project.markers.len() >= limits::MAX_MARKERS {
        return Err(invalid_request(format!(
            "A project may hold at most {} chapter markers",
            limits::MAX_MARKERS
        )));
    }
    let mut candidate = project.clone();
    candidate.markers.push(Marker {
        id: new_entity_id("marker"),
        clip_id: payload.clip_id.clone(),
        source_ms: payload.source_ms,
        title,
        extra: Map::new(),
    });
    Ok((candidate, "Add chapter".to_string()))
}

/// `updateMarker{markerId, sourceMs?, title?}` -- it stays on its clip.
pub(super) fn update_marker(
    project: &Project,
    payload: &UpdateMarkerPayload,
) -> Result<(Project, String), EditorError> {
    let marker = find_marker(project, &payload.marker_id)?;
    let clip = find_clip(project, &marker.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    if payload.source_ms.is_none() && payload.title.is_none() {
        return Err(invalid_request("updateMarker must set at least one field"));
    }
    let source_ms = payload.source_ms.unwrap_or(marker.source_ms);
    check_marker_instant(source_ms, clip)?;
    let title = payload.title.as_deref().map(checked_title).transpose()?;

    let mut candidate = project.clone();
    let target = candidate
        .markers
        .iter_mut()
        .find(|m| m.id == payload.marker_id)
        .expect("marker_id was just resolved above against this same project");
    target.source_ms = source_ms;
    if let Some(title) = title {
        target.title = title;
    }
    Ok((candidate, "Edit chapter".to_string()))
}

/// `removeMarker{markerId}`.
pub(super) fn remove_marker(
    project: &Project,
    payload: &RemoveMarkerPayload,
) -> Result<(Project, String), EditorError> {
    let marker = find_marker(project, &payload.marker_id)?;
    let clip = find_clip(project, &marker.clip_id)?;
    ensure_unlocked(project, &clip.track_id)?;
    let mut candidate = project.clone();
    candidate.markers.retain(|m| m.id != payload.marker_id);
    Ok((candidate, "Delete chapter".to_string()))
}

// Tests live in the sibling `cues_tests.rs`, the `clips.rs`/
// `clips_tests.rs` precedent.
#[cfg(test)]
#[path = "cues_tests.rs"]
mod tests;
