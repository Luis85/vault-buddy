//! The render plan (Task 41; R1, R15; NATIVE-MEDIA.md § Render plan; F-04,
//! F-05, F-41, F-42; A06, A13): a PURE freeze of one acknowledged project
//! revision and an optional output range into everything the render needs
//! -- the canvas, the source registry, the visible video layers, the audio
//! contributions, the teaching cues, the burned-in captions, the chapter
//! map and the title cards. `screen::render` (Tasks 42-44) turns it into
//! ONE ffmpeg `filter_complex` invocation; nothing here spawns, probes or
//! reads a file.
//!
//! **What goes in, and what cannot.** `plan` takes the project, the
//! resolved facts of every source the shell could find on disk
//! (`PlanSource`, keyed by the `sources.json` record id) and the range.
//! There is no workspace parameter, so `Workspace.monitor_muted` and the
//! transport's monitoring volume CANNOT reach the plan (A06: "monitoring is
//! not export mute") -- the signature is the guard, and a test pins it.
//!
//! **Tracks.** `Project.tracks[0]` is the TOPMOST layer (`commands::tracks`'
//! module doc); `video_layers` is listed bottom -> top, so the renderer
//! overlays them in Vec order. A HIDDEN track contributes nothing at all --
//! no picture and no sound -- which is what the preview does
//! (`src/editor/previewLayers.ts`' `resolveLayerInputs` drops an invisible
//! track before it decides anything about audio) and what F-04 says
//! ("invisible tracks are excluded from output"). The reference editor's
//! `clipGain` ignores visibility; preview parity wins here (GAP-173).
//! Chapters and burned-in captions are the one exception, and share ONE
//! rule: they follow what their lists show (`src/editor/captionRules.ts`'
//! `chapterRows` / `captionRows`, and the preview's caption overlay) -- a
//! marker or caption inside its clip's source range counts WHATEVER its
//! track's visibility. Teaching cues, which paint onto their clip, need a
//! visible video track (`src/editor/cueGeometry.ts`).
//!
//! **Sources.** A clip's bytes live under its asset's `sources.json`
//! record: the asset itself, or its `linked_asset` root for detached audio
//! (`media_commands::resolve_asset`'s rule, one hop). The ONLY assets the
//! plan synthesizes are `Builtin::Card` ones, which go to `cards` rather
//! than `video_layers`; every other asset -- a migrated capture's
//! `builtin: screen` included (GAP-175) -- is an input like any imported
//! video. A needed clip whose record has no `PlanSource` fails the whole
//! plan with `sourceMissing`, naming the record ids in
//! `retained_asset_ids`: never a silent black layer or a silent gap in the
//! mix. That includes the reference format's file-less `presenter`/
//! `detail`/`cues`/`ambient` placeholders, which no render can draw.
//!
//! **Time.** Every timestamp is OUTPUT-time integer ms. Clip spans and cue
//! spans come from `time` (R15: `clip_output_duration`,
//! `cue_output_span`), never a second copy. A range `[start, end)` shifts
//! every timestamp by `-start` and clips it to `[0, end - start)`; an item
//! wholly outside is dropped, and whatever an edge LOST to the range is
//! kept in its `Cut` so an envelope that began before the range (a fade, a
//! crossfade, a zoom's ramp) can still be evaluated where it really is
//! (A13).
//!
//! Audio (`render_plan_audio.rs`) is split out for the 800-line cap.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

use super::commands::chapters;
use super::error::{EditorError, EditorErrorCode};
use super::migrate::{canvas_is_exact, nearest_canvas};
use super::model::{
    Adjustments, Asset, Builtin, Canvas, Card, Clip, FadeCurve, Fit, FrameShape, Project, Rotation,
    TrackKind,
};
use super::model_cues::{CaptionPosition, Effect, EffectKind, TransitionKind};
use super::render_plan_audio::{contribution, is_audible};
use super::time::{self, ClipSpan};
use super::validate::speed_or_default;
use super::Num;

pub use super::render_plan_audio::AudioContribution;

/// What the shell knows about one source file (from `sources.json` and its
/// probe): which ffmpeg input it is, and what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSource {
    pub input_index: usize,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: u32,
    pub height: u32,
    pub is_image: bool,
}

/// One source the plan actually reads, with its asset's duration (the
/// identity check's "full source range" needs it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanInput {
    /// The `sources.json` record id (the asset, or a detached audio's root).
    pub asset_id: String,
    pub input_index: usize,
    pub has_video: bool,
    pub has_audio: bool,
    pub width: u32,
    pub height: u32,
    pub is_image: bool,
    pub duration_ms: u64,
}

/// A clip's box in canvas pixels, each value rounded to the nearest EVEN
/// integer (the encoder's chroma subsampling needs even geometry).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelBox {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// A crop zoom above 1x and its anchor (fractions of the source).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crop {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

/// How much of an item's own OUTPUT span the range cut off at each edge --
/// both 0 without a range. An envelope measured from an original edge (a
/// fade, a crossfade, a zoom ramp) starts `head_ms` before the planned
/// `output_start`, so the renderer can evaluate it where it really is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cut {
    pub head_ms: u64,
    pub tail_ms: u64,
}

/// One visible video clip, composited in `RenderPlan::video_layers` order.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoLayer {
    pub clip_id: String,
    /// `Project.tracks` index -- 0 is the TOP layer.
    pub track_index: usize,
    pub input: usize,
    pub output_start: u64,
    pub output_end: u64,
    pub source_in: u64,
    pub source_out: u64,
    pub speed: f64,
    pub bounds: PixelBox,
    pub fit: Fit,
    pub frame_shape: FrameShape,
    pub rotation: Rotation,
    pub mirror: bool,
    pub flip_y: bool,
    pub crop: Option<Crop>,
    pub opacity: f64,
    pub fade_in: u64,
    pub fade_out: u64,
    pub fade_curve: FadeCurve,
    pub adjustments: Option<Adjustments>,
    pub transition_in: Option<(TransitionKind, u64)>,
    /// The transition this clip is the `from` side of: it fades out over
    /// the last that-many ms of its original span, under the `to` clip.
    pub transition_out: Option<(TransitionKind, u64)>,
    pub cut: Cut,
}

/// A generated title card (`Builtin::Card`): no input, its content drawn by
/// the renderer. `track_index` places it among `video_layers`.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCard {
    pub clip_id: String,
    pub track_index: usize,
    pub output_start: u64,
    pub output_end: u64,
    pub bounds: PixelBox,
    pub opacity: f64,
    pub fade_in: u64,
    pub fade_out: u64,
    pub fade_curve: FadeCurve,
    pub transition_in: Option<(TransitionKind, u64)>,
    /// The transition this clip is the `from` side of: it fades out over
    /// the last that-many ms of its original span, under the `to` clip.
    pub transition_out: Option<(TransitionKind, u64)>,
    pub card: Option<Card>,
    pub cut: Cut,
}

/// A teaching cue on a visible video clip, mapped to OUTPUT time. Listed in
/// `Project.effects` order: between overlapping zooms the LAST one wins
/// (the preview's `activeZoom`, the reference `camera`'s `.at(-1)`), and
/// whether a zoom scales its clip or the composed stage is Tasks 42/43's
/// decision (GAP-173), not this plan's.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCue {
    pub effect_id: String,
    pub clip_id: String,
    pub kind: EffectKind,
    pub output_start: u64,
    pub output_end: u64,
    /// The effect as stored (position, colour, text, factor, easing...).
    pub effect: Effect,
    pub cut: Cut,
}

/// The reference ramp when a zoom's `easing` is unset or zero.
pub const DEFAULT_ZOOM_EASING_MS: f64 = 600.0;

impl PlannedCue {
    /// A zoom cue's ramp length at each end, in output ms: its `easing`
    /// (600 when unset or zero), capped at half of its WHOLE mapped span --
    /// the unclipped one, `cut` restored, so a range never changes the ramp
    /// (`src/editor/cueGeometry.ts`' `zoomAmount`). `None` for other kinds.
    pub fn zoom_ease_ms(&self) -> Option<f64> {
        if self.kind != EffectKind::Zoom {
            return None;
        }
        let whole = self.cut.head_ms + (self.output_end - self.output_start) + self.cut.tail_ms;
        let easing = f64_of(self.effect.easing.as_ref(), 0.0);
        let easing = if easing > 0.0 {
            easing
        } else {
            DEFAULT_ZOOM_EASING_MS
        };
        Some(easing.min(whole as f64 / 2.0))
    }
}

/// One burned-in caption, OUTPUT time.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCaption {
    pub id: String,
    pub output_start: u64,
    pub output_end: u64,
    pub text: String,
}

/// The caption track to burn in -- present only when captions are enabled
/// AND set to burn in AND at least one cue lands in the output.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCaptions {
    pub font_size: f64,
    pub position: CaptionPosition,
    pub background: bool,
    /// Sorted by output start, then end.
    pub cues: Vec<PlannedCaption>,
}

/// A frozen, validated description of one render.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderPlan {
    pub canvas: Canvas,
    pub duration_ms: u64,
    /// Sorted by `input_index`, one per record the plan reads.
    pub inputs: Vec<PlanInput>,
    /// Bottom -> top.
    pub video_layers: Vec<VideoLayer>,
    pub audio: Vec<AudioContribution>,
    pub cues: Vec<PlannedCue>,
    pub captions: Option<PlannedCaptions>,
    /// `(output_ms, title)`, sorted by output time (`commands::chapters`).
    pub chapters: Vec<(u64, String)>,
    pub cards: Vec<PlannedCard>,
    pub master_gain: f64,
}

/// The absolute output window a plan covers: the whole project, or the
/// requested range.
#[derive(Debug, Clone, Copy)]
struct Window {
    start: u64,
    end: u64,
}

impl Window {
    /// `range` validated against the project's own duration: a range must be
    /// non-empty and end inside the project -- a render of time the project
    /// does not have would pad with frames nobody edited.
    fn new(total_ms: u64, range: Option<(u64, u64)>) -> Result<Window, EditorError> {
        match range {
            None => Ok(Window {
                start: 0,
                end: total_ms,
            }),
            Some((start, end)) if start < end && end <= total_ms => Ok(Window { start, end }),
            Some((start, end)) => Err(EditorError::new(
                EditorErrorCode::InvalidRequest,
                format!(
                    "range {start}-{end} ms must be non-empty and end within the project's {total_ms} ms"
                ),
            )),
        }
    }

    /// `[s, e)` clipped to the window and rebased to its start, with what
    /// each edge lost; `None` when nothing of it is inside.
    fn clip(&self, s: u64, e: u64) -> Option<(u64, u64, Cut)> {
        let (a, b) = (s.max(self.start), e.min(self.end));
        (a < b).then(|| {
            let cut = Cut {
                head_ms: a - s,
                tail_ms: e - b,
            };
            (a - self.start, b - self.start, cut)
        })
    }
}

/// A clip's OUTPUT span inside the window, with the source range that
/// plays over exactly that span.
#[derive(Debug, Clone, Copy)]
pub(super) struct Placed {
    pub output_start: u64,
    pub output_end: u64,
    pub source_in: u64,
    pub source_out: u64,
    pub speed: f64,
    pub cut: Cut,
}

fn clip_span(clip: &Clip) -> ClipSpan {
    ClipSpan {
        start_ms: clip.start_ms,
        in_ms: clip.in_ms,
        out_ms: clip.out_ms,
        speed: speed_or_default(clip.speed.as_ref()),
    }
}

/// `clip` placed into `window`. A cut edge moves the source bound by the
/// cut output time x speed (R15's mapping); an uncut edge keeps the clip's
/// own bound exactly, so an unranged plan never re-derives a source time.
fn place(clip: &Clip, window: &Window) -> Option<Placed> {
    let span = clip_span(clip);
    let end = time::clip_output_end(&span);
    let (output_start, output_end, cut) = window.clip(clip.start_ms, end)?;
    let source_at = |elapsed: u64| {
        let s = clip.in_ms + (elapsed as f64 * span.speed).round() as u64;
        s.min(clip.out_ms)
    };
    let source_in = if cut.head_ms == 0 {
        clip.in_ms
    } else {
        source_at(cut.head_ms)
    };
    let source_out = if cut.tail_ms == 0 {
        clip.out_ms
    } else {
        source_at(end - cut.tail_ms - clip.start_ms)
    };
    Some(Placed {
        output_start,
        output_end,
        source_in,
        source_out,
        speed: span.speed,
        cut,
    })
}

pub(super) fn f64_of(v: Option<&Num>, default: f64) -> f64 {
    v.and_then(Num::as_f64).unwrap_or(default)
}

/// `v` rounded to the nearest even integer, never negative.
fn even_px(v: f64) -> u32 {
    ((v / 2.0).round() * 2.0).max(0.0) as u32
}

/// One axis of a box: the two EDGES rounded to even pixels and clamped to
/// the canvas, the size derived from them. Rounding the origin and the size
/// separately let two odd ties push the far edge 2 px past the canvas
/// (y 0.3375, h 0.6625 on 720: 244 + 478 = 722).
fn even_span(origin: f64, size: f64, canvas: u32) -> (u32, u32) {
    let extent = f64::from(canvas);
    let far = even_px((origin + size) * extent).min(canvas);
    let near = even_px(origin * extent).min(far);
    (near, far - near)
}

fn bounds_of(clip: &Clip, canvas: &Canvas) -> PixelBox {
    let (x, w) = even_span(
        f64_of(Some(&clip.x), 0.0),
        f64_of(Some(&clip.w), 1.0),
        canvas.width,
    );
    let (y, h) = even_span(
        f64_of(Some(&clip.y), 0.0),
        f64_of(Some(&clip.h), 1.0),
        canvas.height,
    );
    PixelBox { x, y, w, h }
}

/// The preview's own look defaults (`src/editor/previewTransform.ts`): a
/// clip with no frame shape is `rounded` as a picture-in-picture box
/// (narrower than 0.98 of the canvas) and `rectangle` full-frame.
fn frame_shape_of(clip: &Clip) -> FrameShape {
    clip.frame_shape
        .unwrap_or(if f64_of(Some(&clip.w), 1.0) < 0.98 {
            FrameShape::Rounded
        } else {
            FrameShape::Rectangle
        })
}

/// A crop only when it actually zooms (the default is 1x, anchor centred).
fn crop_of(clip: &Clip) -> Option<Crop> {
    let zoom = f64_of(clip.crop_zoom.as_ref(), 1.0);
    (zoom > 1.0).then(|| Crop {
        zoom,
        x: f64_of(clip.crop_x.as_ref(), 0.5),
        y: f64_of(clip.crop_y.as_ref(), 0.5),
    })
}

/// The transition `clip` is the `to` side of, if any.
pub(super) fn transition_into(project: &Project, clip_id: &str) -> Option<(TransitionKind, u64)> {
    project
        .transitions
        .iter()
        .find(|t| t.to == clip_id)
        .map(|t| (t.kind, t.duration_ms))
}

/// The transition `clip` is the `from` side of, if any -- the outgoing half
/// of the same overlap: without it the renderer would play `from` at full
/// level under `to`'s fade-in.
pub(super) fn transition_out_of(project: &Project, clip_id: &str) -> Option<(TransitionKind, u64)> {
    project
        .transitions
        .iter()
        .find(|t| t.from == clip_id)
        .map(|t| (t.kind, t.duration_ms))
}

fn layer(
    project: &Project,
    clip: &Clip,
    track_index: usize,
    input: usize,
    placed: &Placed,
) -> VideoLayer {
    VideoLayer {
        clip_id: clip.id.clone(),
        track_index,
        input,
        output_start: placed.output_start,
        output_end: placed.output_end,
        source_in: placed.source_in,
        source_out: placed.source_out,
        speed: placed.speed,
        bounds: bounds_of(clip, &project.canvas),
        fit: clip.fit.unwrap_or(Fit::Contain),
        frame_shape: frame_shape_of(clip),
        rotation: clip.rotation.unwrap_or(Rotation::Deg0),
        mirror: clip.mirror.unwrap_or(false),
        flip_y: clip.flip_y.unwrap_or(false),
        crop: crop_of(clip),
        opacity: f64_of(Some(&clip.opacity), 1.0),
        fade_in: clip.fade_in_ms,
        fade_out: clip.fade_out_ms,
        fade_curve: clip.fade_curve,
        adjustments: clip.adjustments.clone(),
        transition_in: transition_into(project, &clip.id),
        transition_out: transition_out_of(project, &clip.id),
        cut: placed.cut,
    }
}

fn card(project: &Project, clip: &Clip, track_index: usize, placed: &Placed) -> PlannedCard {
    PlannedCard {
        clip_id: clip.id.clone(),
        track_index,
        output_start: placed.output_start,
        output_end: placed.output_end,
        bounds: bounds_of(clip, &project.canvas),
        opacity: f64_of(Some(&clip.opacity), 1.0),
        fade_in: clip.fade_in_ms,
        fade_out: clip.fade_out_ms,
        fade_curve: clip.fade_curve,
        transition_in: transition_into(project, &clip.id),
        transition_out: transition_out_of(project, &clip.id),
        card: clip.card.clone(),
        cut: placed.cut,
    }
}

/// The `sources.json` record a non-card asset's bytes live under: the
/// asset itself, or its detached-audio root (one hop, `validate_media`).
fn record_of(asset: &Asset) -> &str {
    asset.linked_asset.as_deref().unwrap_or(&asset.id)
}

fn invalid_project(message: String) -> EditorError {
    EditorError::new(EditorErrorCode::InvalidProject, message)
}

/// Everything the per-clip pass accumulates.
#[derive(Default)]
struct Accumulator {
    layers: Vec<VideoLayer>,
    cards: Vec<PlannedCard>,
    audio: Vec<AudioContribution>,
    used: BTreeMap<String, PlanSource>,
    missing: BTreeSet<String>,
}

impl Accumulator {
    /// One clip on the VISIBLE track `track_index`, already placed inside
    /// the window. A card is drawn, never read; anything else that would
    /// paint or sound must have its record in `sources`.
    fn add(
        &mut self,
        project: &Project,
        sources: &BTreeMap<String, PlanSource>,
        clip: &Clip,
        track_index: usize,
        placed: &Placed,
    ) -> Result<(), EditorError> {
        let track = &project.tracks[track_index];
        let asset = project
            .assets
            .iter()
            .find(|a| a.id == clip.asset_id)
            .ok_or_else(|| {
                invalid_project(format!(
                    "clip {}: asset {} does not resolve",
                    clip.id, clip.asset_id
                ))
            })?;
        let is_video = track.kind == TrackKind::Video;
        if asset.builtin == Some(Builtin::Card) {
            if is_video {
                self.cards.push(card(project, clip, track_index, placed));
            }
            return Ok(());
        }
        let audible = is_audible(&project.tracks, clip, track);
        if !is_video && !audible {
            return Ok(());
        }
        let record = record_of(asset);
        let Some(source) = sources.get(record) else {
            self.missing.insert(record.to_string());
            return Ok(());
        };
        if is_video && source.has_video {
            self.layers.push(layer(
                project,
                clip,
                track_index,
                source.input_index,
                placed,
            ));
            self.used.insert(record.to_string(), *source);
        }
        if audible && source.has_audio {
            self.audio.push(contribution(
                project,
                clip,
                track_index,
                source.input_index,
                placed,
            ));
            self.used.insert(record.to_string(), *source);
        }
        Ok(())
    }
}

fn inputs_of(project: &Project, used: &BTreeMap<String, PlanSource>) -> Vec<PlanInput> {
    let mut inputs: Vec<PlanInput> = used
        .iter()
        .map(|(id, s)| PlanInput {
            asset_id: id.clone(),
            input_index: s.input_index,
            has_video: s.has_video,
            has_audio: s.has_audio,
            width: s.width,
            height: s.height,
            is_image: s.is_image,
            duration_ms: project
                .assets
                .iter()
                .find(|a| &a.id == id)
                .map_or(0, |a| a.duration_ms),
        })
        .collect();
    inputs.sort_by_key(|i| i.input_index);
    inputs
}

/// Is `clip` on a VISIBLE VIDEO track -- where a teaching cue paints
/// (`src/editor/cueGeometry.ts`: a hidden track does not paint, and an
/// audio clip has no picture to annotate)?
fn on_visible_video(project: &Project, clip: &Clip) -> bool {
    project
        .tracks
        .iter()
        .any(|t| t.id == clip.track_id && t.visible && t.kind == TrackKind::Video)
}

fn cues_of(project: &Project, window: &Window) -> Vec<PlannedCue> {
    project
        .effects
        .iter()
        .filter_map(|e| {
            let clip = project.clips.iter().find(|c| c.id == e.clip_id)?;
            if !on_visible_video(project, clip) {
                return None;
            }
            let (s, t) = time::cue_output_span(&clip_span(clip), e.start_ms, e.end_ms)?;
            let (output_start, output_end, cut) = window.clip(s, t)?;
            Some(PlannedCue {
                effect_id: e.id.clone(),
                clip_id: clip.id.clone(),
                kind: e.kind,
                output_start,
                output_end,
                effect: e.clone(),
                cut,
            })
        })
        .collect()
}

/// Every caption the Captions list shows (`src/editor/captionRules.ts`'
/// `captionRows`: a cue inside its clip's range, whatever the track),
/// burned in only when enabled AND set to burn in; `None` when no cue lands
/// in the output.
fn captions_of(project: &Project, window: &Window) -> Option<PlannedCaptions> {
    let settings = project
        .captions
        .as_ref()
        .filter(|c| c.enabled && c.burn_in)?;
    let mut cues: Vec<PlannedCaption> = settings
        .cues
        .iter()
        .filter_map(|q| {
            let clip = project.clips.iter().find(|c| c.id == q.clip_id)?;
            let (s, e) = time::cue_output_span(&clip_span(clip), q.start_ms, q.end_ms)?;
            let (output_start, output_end, _) = window.clip(s, e)?;
            Some(PlannedCaption {
                id: q.id.clone(),
                output_start,
                output_end,
                text: q.text.clone(),
            })
        })
        .collect();
    if cues.is_empty() {
        return None;
    }
    cues.sort_by_key(|q| (q.output_start, q.output_end));
    Some(PlannedCaptions {
        font_size: f64_of(Some(&settings.font_size), 0.0),
        position: settings.position,
        background: settings.background,
        cues,
    })
}

/// `commands::chapters`, kept only inside the window and rebased (A13).
fn chapters_of(project: &Project, window: &Window) -> Vec<(u64, String)> {
    chapters(project)
        .into_iter()
        .filter_map(|c| {
            let (at, _, _) = window.clip(c.output_ms, c.output_ms + 1)?;
            Some((at, c.title))
        })
        .collect()
}

fn missing_error(missing: BTreeSet<String>) -> EditorError {
    let ids: Vec<String> = missing.into_iter().collect();
    let mut error = EditorError::new(
        EditorErrorCode::SourceMissing,
        format!(
            "The render needs original media that is not available: {}",
            ids.join(", ")
        ),
    );
    error.retained_asset_ids = Some(ids);
    error
}

/// Freezes `project` and `range` into a `RenderPlan` (see the module doc).
pub fn plan(
    project: &Project,
    sources: &BTreeMap<String, PlanSource>,
    range: Option<(u64, u64)>,
) -> Result<RenderPlan, EditorError> {
    let window = Window::new(time::project_duration(project), range)?;
    let mut acc = Accumulator::default();
    for clip in &project.clips {
        let index = project
            .tracks
            .iter()
            .position(|t| t.id == clip.track_id)
            .ok_or_else(|| {
                invalid_project(format!(
                    "clip {}: track {} does not resolve",
                    clip.id, clip.track_id
                ))
            })?;
        if !project.tracks[index].visible {
            continue;
        }
        if let Some(placed) = place(clip, &window) {
            acc.add(project, sources, clip, index, &placed)?;
        }
    }
    if !acc.missing.is_empty() {
        return Err(missing_error(acc.missing));
    }
    // Bottom -> top: the highest track index first; within a track, by time.
    acc.layers
        .sort_by_key(|l| (Reverse(l.track_index), l.output_start));
    acc.cards
        .sort_by_key(|c| (Reverse(c.track_index), c.output_start));
    acc.audio.sort_by_key(|a| (a.output_start, a.track_index));
    Ok(RenderPlan {
        canvas: project.canvas.clone(),
        duration_ms: window.end - window.start,
        inputs: inputs_of(project, &acc.used),
        video_layers: acc.layers,
        audio: acc.audio,
        cues: cues_of(project, &window),
        captions: captions_of(project, &window),
        chapters: chapters_of(project, &window),
        cards: acc.cards,
        master_gain: project.master_gain,
    })
}

impl RenderPlan {
    /// R1: may this render be a lossless remux of its one source? Exactly
    /// one full-range, 1x layer of one source filling the canvas untouched,
    /// that source's aspect EXACTLY the project canvas' (F5: aspect, never
    /// raw pixels -- a 1920x1080 capture on its nearest 1280x720 canvas
    /// qualifies, and the remux then runs at SOURCE resolution), nothing
    /// drawn over it, and its sound: when the source HAS an audio stream,
    /// that stream is the one contribution, at unity; when it has none,
    /// there is no contribution at all -- the legacy fast path remuxed a
    /// silent capture too (`Timeline::is_untouched`), and this successor
    /// must not lose that (controller ruling, fix round 1).
    pub fn is_identity(&self) -> bool {
        let ([layer], [input]) = (self.video_layers.as_slice(), self.inputs.as_slice()) else {
            return false;
        };
        self.nothing_drawn_over()
            && self.master_gain == 1.0
            && self.source_fits_canvas(input)
            && layer.input == input.input_index
            && layer_is_whole(layer, input, self.duration_ms)
            && self.layer_is_plain(layer)
            && self.sound_is_untouched(layer, input)
    }

    fn sound_is_untouched(&self, layer: &VideoLayer, input: &PlanInput) -> bool {
        match (input.has_audio, self.audio.as_slice()) {
            (false, []) => true,
            (true, [audio]) => audio.input == input.input_index && audio_is_whole(audio, layer),
            _ => false,
        }
    }

    fn nothing_drawn_over(&self) -> bool {
        self.cues.is_empty() && self.captions.is_none() && self.cards.is_empty()
    }

    /// F5: `canvas_is_exact` for the source's own dims, AND that nearest
    /// canvas is the project's -- a 16:9 capture on a square canvas the
    /// user chose is exact against 1280x720, not against what they render.
    fn source_fits_canvas(&self, input: &PlanInput) -> bool {
        input.has_video
            && !input.is_image
            && canvas_is_exact(input.width, input.height)
            && nearest_canvas(input.width, input.height) == (self.canvas.width, self.canvas.height)
    }

    fn layer_is_plain(&self, layer: &VideoLayer) -> bool {
        let full = PixelBox {
            x: 0,
            y: 0,
            w: self.canvas.width,
            h: self.canvas.height,
        };
        layer.bounds == full
            && layer.opacity == 1.0
            && layer.fade_in == 0
            && layer.fade_out == 0
            && layer.transition_in.is_none()
            && layer.transition_out.is_none()
            && layer.adjustments.is_none()
            && layer.crop.is_none()
            && layer.rotation == Rotation::Deg0
            && !layer.mirror
            && !layer.flip_y
            && layer.frame_shape == FrameShape::Rectangle
    }
}

/// The whole source, from 0, at 1x, spanning the whole output.
fn layer_is_whole(layer: &VideoLayer, input: &PlanInput, duration_ms: u64) -> bool {
    layer.source_in == 0
        && layer.source_out == input.duration_ms
        && layer.speed == 1.0
        && layer.output_start == 0
        && layer.output_end == duration_ms
}

/// The layer's own sound, over the same source range, untouched.
fn audio_is_whole(audio: &AudioContribution, layer: &VideoLayer) -> bool {
    audio.clip_id == layer.clip_id
        && audio.gain == 1.0
        && audio.speed == 1.0
        && audio.fade_in == 0
        && audio.fade_out == 0
        && audio.crossfade_in.is_none()
        && audio.crossfade_out.is_none()
        && (audio.source_in, audio.source_out) == (layer.source_in, layer.source_out)
}

#[cfg(test)]
#[path = "render_plan_range_tests.rs"]
mod range_tests;
#[cfg(test)]
#[path = "render_plan_tests.rs"]
mod tests;
