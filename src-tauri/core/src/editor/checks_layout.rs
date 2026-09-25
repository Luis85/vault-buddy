//! The before-you-share checks that are about WHERE things draw (Task 54,
//! a child of `checks` for its 800-line cap): text and step cues colliding
//! on screen, and the canvas review -- a full-frame source of another
//! aspect, text outside the 5 % safe area, a caption too tall for the
//! frame. Two geometric constants are mirrors, named where they live: a
//! step cue's pill (`src/editor/cueShapes.ts`) and the caption box
//! (`CaptionOverlay.vue`).

use super::{
    caption_rows, finding, on, on_visible_video, span, CheckAction, CheckCode, CheckFinding,
    CheckTarget, Ctx, Severity, TargetKind, EPS, GLYPH_WIDTH, SAFE_MARGIN,
};
use crate::editor::model::{Builtin, Canvas, Project};
use crate::editor::model_cues::{Effect, EffectKind};
use crate::editor::render_plan::f64_of;
use crate::editor::time;
use crate::editor::Num;

/// A clip at least this wide and tall fills the frame (the render's own
/// picture-in-picture line, `render_plan::frame_shape_of`).
const FULL_FRAME: f64 = 0.98;
/// `src/editor/cueShapes.ts`: a step's badge radius and label size.
const STEP_RADIUS: f64 = 22.0;
const STEP_LABEL_SIZE: f64 = 26.0;

// ---- textCollision --------------------------------------------------------

/// A box in canvas FRACTIONS.
#[derive(Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl Rect {
    fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }

    fn inside_safe_area(&self) -> bool {
        let (lo, hi) = (SAFE_MARGIN - EPS, 1.0 - SAFE_MARGIN + EPS);
        self.x >= lo && self.y >= lo && self.x + self.w <= hi && self.y + self.h <= hi
    }
}

/// Where a text or step cue draws, or `None` for any other kind. A step is
/// its pill (`cueShapes.ts`' `stepPillWidth`), sized in canvas pixels.
fn text_box(e: &Effect, canvas: &Canvas) -> Option<Rect> {
    let (x, y) = (f64_of(Some(&e.x), 0.0), f64_of(Some(&e.y), 0.0));
    match e.kind {
        EffectKind::Text => Some(Rect {
            x,
            y,
            w: f64_of(e.w.as_ref(), 0.0),
            h: f64_of(e.h.as_ref(), 0.0),
        }),
        EffectKind::Step => {
            let (cw, ch) = (f64::from(canvas.width), f64::from(canvas.height));
            let label = e.text.as_deref().unwrap_or("").chars().count() as f64;
            let width = 2.0 * STEP_RADIUS + 7.0 + label * STEP_LABEL_SIZE * GLYPH_WIDTH;
            Some(Rect {
                x: x - STEP_RADIUS / cw,
                y: y - STEP_RADIUS / ch,
                w: width / cw,
                h: (2.0 * STEP_RADIUS + 1.0) / ch,
            })
        }
        _ => None,
    }
}

/// A text or step cue the render draws: its box and its OUTPUT span.
struct TextCue<'a> {
    effect: &'a Effect,
    rect: Rect,
    start: u64,
    end: u64,
}

fn text_cues(project: &Project) -> Vec<TextCue<'_>> {
    let mut cues: Vec<TextCue> = project
        .effects
        .iter()
        .filter_map(|e| {
            let rect = text_box(e, &project.canvas)?;
            let clip = project.clips.iter().find(|c| c.id == e.clip_id)?;
            if !on_visible_video(project, clip) {
                return None;
            }
            let (start, end) = time::cue_output_span(&span(clip), e.start_ms, e.end_ms)?;
            Some(TextCue {
                effect: e,
                rect,
                start,
                end,
            })
        })
        .collect();
    cues.sort_by_key(|c| (c.start, c.effect.id.as_str()));
    cues
}

fn cue_name(e: &Effect) -> String {
    match e.text.as_deref().map(str::trim) {
        Some(text) if !text.is_empty() => text.to_string(),
        _ if e.kind == EffectKind::Step => "a step".to_string(),
        _ => "a text cue".to_string(),
    }
}

/// Two text/step cues on screen at once, their boxes intersecting --
/// reported once, on the later of each colliding pair.
pub(super) fn text_collisions(ctx: &Ctx) -> Vec<CheckFinding> {
    let cues = text_cues(ctx.project);
    cues.iter()
        .enumerate()
        .filter_map(|(j, b)| {
            let a = cues[..j]
                .iter()
                .find(|a| a.start < b.end && b.start < a.end && a.rect.intersects(&b.rect))?;
            Some(finding(
                Severity::Info,
                CheckCode::TextCollision,
                format!(
                    "Two text cues overlap on screen: \"{}\" and \"{}\".",
                    cue_name(a.effect),
                    cue_name(b.effect)
                ),
                on(TargetKind::Effect, &b.effect.id),
                Some(CheckAction::Select),
            ))
        })
        .collect()
}

// ---- canvasReview -------------------------------------------------------

pub(super) fn canvas_review(ctx: &Ctx) -> Vec<CheckFinding> {
    let mut out = source_aspects(ctx.project);
    out.extend(text_outside_safe_area(ctx.project));
    out.extend(captions_too_tall(ctx.project));
    out
}

fn review(message: String, target: CheckTarget) -> CheckFinding {
    finding(
        Severity::Warning,
        CheckCode::CanvasReview,
        message,
        target,
        Some(CheckAction::ReviewCanvas),
    )
}

fn pixels(n: Option<&Num>) -> Option<u64> {
    n.and_then(Num::as_u64).filter(|v| *v > 0)
}

/// A picture source shown FULL FRAME whose aspect is not the canvas's
/// (exact integer cross-multiplication, `migrate::canvas_is_exact`'s rule),
/// once per source, on its earliest full-frame clip. A picture-in-picture
/// box is framed on purpose and never reported.
fn source_aspects(project: &Project) -> Vec<CheckFinding> {
    let canvas = &project.canvas;
    project
        .assets
        .iter()
        .filter(|a| a.builtin != Some(Builtin::Card))
        .filter_map(|asset| {
            let (w, h) = (pixels(asset.width.as_ref())?, pixels(asset.height.as_ref())?);
            if w * u64::from(canvas.height) == h * u64::from(canvas.width) {
                return None;
            }
            let clip = project
                .clips
                .iter()
                .filter(|c| c.asset_id == asset.id && on_visible_video(project, c))
                .filter(|c| f64_of(Some(&c.w), 1.0) >= FULL_FRAME && f64_of(Some(&c.h), 1.0) >= FULL_FRAME)
                .min_by_key(|c| (c.start_ms, c.id.as_str()))?;
            Some(review(
                format!(
                    "\"{}\" is {w}\u{d7}{h}, which does not match the {}\u{d7}{} canvas. Review its crop and framing.",
                    asset.name, canvas.width, canvas.height
                ),
                on(TargetKind::Clip, &clip.id),
            ))
        })
        .collect()
}

fn text_outside_safe_area(project: &Project) -> Vec<CheckFinding> {
    let mut cues = text_cues(project);
    cues.sort_by_key(|c| c.effect.id.as_str());
    cues.iter()
        .filter(|c| !c.rect.inside_safe_area())
        .map(|c| {
            review(
                format!(
                    "\"{}\" reaches outside the safe area. Keep text 5% inside the frame.",
                    cue_name(c.effect)
                ),
                on(TargetKind::Effect, &c.effect.id),
            )
        })
        .collect()
}

/// A shown caption whose wrapped block is taller than the frame's safe
/// band -- an ESTIMATE from `CaptionOverlay.vue`'s geometry: `font_size` px
/// at a 720-line canvas, lines at most 86 % of the width less the box's
/// side padding, `GLYPH_WIDTH` per character, 1.25 line height.
fn captions_too_tall(project: &Project) -> Vec<CheckFinding> {
    let Some(settings) = project.captions.as_ref().filter(|c| c.enabled) else {
        return Vec::new();
    };
    let (cw, ch) = (
        f64::from(project.canvas.width),
        f64::from(project.canvas.height),
    );
    let size = f64_of(Some(&settings.font_size), 30.0) * cw.min(ch) / 720.0;
    let per_line = ((0.86 * cw - 1.2 * size) / (GLYPH_WIDTH * size))
        .floor()
        .max(1.0);
    let band = (1.0 - 2.0 * SAFE_MARGIN) * ch;
    caption_rows(project)
        .iter()
        .filter(|row| {
            let lines: f64 = row
                .text
                .split('\n')
                .map(|l| (l.chars().count() as f64 / per_line).ceil().max(1.0))
                .sum();
            lines * size * 1.25 + 0.7 * size > band
        })
        .map(|row| {
            review(
                format!(
                    "Caption {} is too long to fit inside the safe area at this size.",
                    row.index
                ),
                on(TargetKind::Caption, row.id),
            )
        })
        .collect()
}
