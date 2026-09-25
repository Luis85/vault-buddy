//! The render's video filter graph (tutorial-editor Task 42; F-04, F-16,
//! F-19, F-31, F-37). PURE: a `RenderPlan` in, the `filter_complex` video
//! part out.
//!
//! **The shape.** A black `color` canvas of the plan's size, rate and
//! length (an extra lavfi input, after the plan's own) is the bottom of an
//! overlay ladder; every visual item -- a media layer or a card, both in
//! the plan's bottom -> top order (`Project.tracks[0]` is the TOP lane) --
//! is overlaid on it in turn, `eof_action=pass` so an ended item leaves the
//! picture below untouched and `enable` so it paints only over its own
//! span. The ladder ends at `[vcomp]`, the composed stage.
//!
//! **The two hooks and the zoom (controller ruling, GAP-173).** The render
//! zooms the WHOLE composed picture, like the preview. So: `[vcomp]` ->
//! the pre-zoom hook (`render::ass::build_cue_ass`'s teaching cues and
//! card text, which the preview zooms with the media) -> the zoom ->
//! `[vzoomed]` -> the post-zoom hook (`render::ass::build_caption_ass`'s
//! burned captions, which the preview keeps unzoomed on the frame) ->
//! `[vout]`. An absent hook or zoom is a `null`, so the labels always
//! exist. `render::mod`'s `render_args`/`AssHooks` is what fills both
//! hooks in practice (Task 43); this module only knows them as opaque
//! filter-chain strings.
//!
//! **Dissolves.** Task 41 plans both halves of a transition (the `from`
//! clip's `transition_out`, the `to` clip's `transition_in`), and a track
//! overlaps only across a transition (`validate_media::
//! check_track_overlaps`). So consecutive items of one track that overlap
//! with the pair's halves set form a GROUP: each member is padded onto a
//! transparent canvas at its own box and the members are chained through
//! `xfade=transition=fade`, whose offset is where the next member starts
//! inside the group and whose duration is the overlap. Both transition
//! kinds render as the same picture dissolve -- equal-power is an AUDIO
//! curve (Task 44). A range that cuts THROUGH an overlap dissolves over the
//! part of it left in the range (xfade cannot start part-way), recorded in
//! GAP-173.
//!
//! **The zoom** is a `perspective` warp of the stage, not the `crop` +
//! `scale` the task brief sketched: ffmpeg evaluates `crop`'s width and
//! height ONCE, at configuration, where `t` is NaN (measured on 9.0.1:
//! `crop=w='iw/(1+t)'` fails with "Error when evaluating the expression"),
//! so a crop cannot ramp. `perspective` with `eval=frame` re-evaluates its
//! four corners per frame and samples between pixels, which is also what
//! the preview's CSS transform does. Its only clock is the frame counter
//! `in` -- 1-based, and counted on frames `enable` skips as well, both
//! measured on 9.0.1 -- so the stage time is `(in-1)/fps`, exact because
//! the canvas is constant-rate from 0 and every overlay emits one frame per
//! canvas frame. The ramp is the preview's (`src/editor/cueGeometry.ts`'
//! `zoomTransform`): the LAST active zoom wins, a smoothstep over the
//! cue's `zoom_ease_ms` (600 ms default, capped at half its span) at each
//! end of its ORIGINAL span, the window's centre clamped so the zoomed
//! stage always covers the frame.

use vault_buddy_core::editor::model_cues::EffectKind;
use vault_buddy_core::editor::render_plan::{
    PixelBox, PlannedCard, PlannedCue, RenderPlan, VideoLayer,
};

use super::expr::{num, plus_seconds, seconds};
use super::video_layers::{card_chain, card_source, even_box, media_layer_chain, Placement};

/// The composed stage, before the pre-zoom hook.
pub const PRE_ZOOM_LABEL: &str = "[vcomp]";
/// The stage after the zoom, before the post-zoom hook.
pub const POST_ZOOM_LABEL: &str = "[vzoomed]";
/// The finished picture.
pub const OUT_LABEL: &str = "[vout]";
/// Between the pre-zoom hook and the zoom, when a hook is given.
const CUED_LABEL: &str = "[vcued]";

/// `(extra input args, filter_complex video part, final label)` with no
/// hooks: see `build_video_graph_with_hooks`.
pub fn build_video_graph(plan: &RenderPlan) -> (Vec<String>, String, String) {
    build_video_graph_with_hooks(plan, None, None)
}

/// The plan's number of FILE inputs: every lavfi input comes after them.
pub fn file_input_count(plan: &RenderPlan) -> usize {
    let layers = plan.video_layers.iter().map(|l| l.input + 1);
    let inputs = plan.inputs.iter().map(|i| i.input_index + 1);
    layers.chain(inputs).max().unwrap_or(0)
}

/// The video graph with the pre-zoom and post-zoom hooks filled in (a
/// comma-joined filter chain each, e.g. Task 43's `ass=...`).
///
/// Returns the extra input args (the canvas, then one `color` source per
/// card, each `-f lavfi -i <spec>`), the graph text, and the final label.
pub fn build_video_graph_with_hooks(
    plan: &RenderPlan,
    pre_zoom: Option<&str>,
    post_zoom: Option<&str>,
) -> (Vec<String>, String, String) {
    let fps = plan.canvas.fps;
    let base = file_input_count(plan);
    let mut extra = lavfi(format!(
        "color=c=black:s={}x{}:r={fps}:d={}",
        plan.canvas.width,
        plan.canvas.height,
        seconds(plan.duration_ms)
    ));
    for card in &plan.cards {
        extra.extend(lavfi(card_source(card, fps)));
    }
    let units = group(items(plan, base + 1));
    let mut sources = Vec::new();
    let mut ladder = Vec::new();
    let mut below = format!("[{base}:v]");
    for (k, unit) in units.iter().enumerate() {
        let (label, x, y) = unit_source(unit, k, plan, &mut sources);
        let above = if k + 1 == units.len() {
            PRE_ZOOM_LABEL.to_string()
        } else {
            format!("[c{k}]")
        };
        let (start, end) = unit_span(unit);
        ladder.push(format!(
            "{below}{label}overlay=x={x}:y={y}:eof_action=pass:enable='between(t,{},{})'{above}",
            seconds(start),
            seconds(end)
        ));
        below = above;
    }
    if units.is_empty() {
        ladder.push(format!("{below}null{PRE_ZOOM_LABEL}"));
    }
    let mut chains = sources;
    chains.extend(ladder);
    let zoom_in = match pre_zoom {
        Some(hook) => {
            chains.push(format!("{PRE_ZOOM_LABEL}{hook}{CUED_LABEL}"));
            CUED_LABEL
        }
        None => PRE_ZOOM_LABEL,
    };
    let zoom = zoom_filter(&plan.cues, fps).unwrap_or_else(|| "null".into());
    chains.push(format!("{zoom_in}{zoom}{POST_ZOOM_LABEL}"));
    chains.push(format!(
        "{POST_ZOOM_LABEL}{}{OUT_LABEL}",
        post_zoom.unwrap_or("null")
    ));
    (extra, chains.join(";"), OUT_LABEL.to_string())
}

fn lavfi(spec: String) -> Vec<String> {
    vec!["-f".into(), "lavfi".into(), "-i".into(), spec]
}

/// One visual item: a media layer, or a card with its lavfi input index.
#[derive(Debug, Clone, Copy)]
enum Item<'a> {
    Media(&'a VideoLayer),
    Card(&'a PlannedCard, usize),
}

impl Item<'_> {
    fn track_index(&self) -> usize {
        match self {
            Item::Media(l) => l.track_index,
            Item::Card(c, _) => c.track_index,
        }
    }

    fn span(&self) -> (u64, u64) {
        match self {
            Item::Media(l) => (l.output_start, l.output_end),
            Item::Card(c, _) => (c.output_start, c.output_end),
        }
    }

    fn bounds(&self) -> PixelBox {
        even_box(match self {
            Item::Media(l) => l.bounds,
            Item::Card(c, _) => c.bounds,
        })
    }

    fn joins_in(&self) -> bool {
        match self {
            Item::Media(l) => l.transition_in.is_some(),
            Item::Card(c, _) => c.transition_in.is_some(),
        }
    }

    fn joins_out(&self) -> bool {
        match self {
            Item::Media(l) => l.transition_out.is_some(),
            Item::Card(c, _) => c.transition_out.is_some(),
        }
    }

    fn chain(&self, fps: u32, placement: Placement) -> String {
        match self {
            Item::Media(l) => media_layer_chain(l, fps, placement),
            Item::Card(c, input) => card_chain(c, *input, placement),
        }
    }
}

/// Media layers and cards merged bottom -> top: the plan orders each list
/// by (lower lane first, then time), and the merge keeps that key.
fn items(plan: &RenderPlan, first_card_input: usize) -> Vec<Item<'_>> {
    let mut all: Vec<Item<'_>> = plan.video_layers.iter().map(Item::Media).collect();
    all.extend(
        plan.cards
            .iter()
            .enumerate()
            .map(|(i, c)| Item::Card(c, first_card_input + i)),
    );
    // Stable: equal keys keep the plan's own order.
    all.sort_by_key(|i| (std::cmp::Reverse(i.track_index()), i.span().0));
    all
}

/// Consecutive items of one track joined by a planned transition (see the
/// module doc) become one group; everything else stays alone. The
/// join predicate itself lives in `grouping::joins_run`, shared with
/// `audio_graph::groups` so the two sides cannot again disagree about
/// what counts as a run (Task 44 fix round 1).
fn group(items: Vec<Item<'_>>) -> Vec<Vec<Item<'_>>> {
    let mut units: Vec<Vec<Item<'_>>> = Vec::new();
    for item in items {
        let joins = units.last().and_then(|u| u.last()).is_some_and(|prev| {
            super::grouping::joins_run(
                prev.track_index(),
                prev.joins_out(),
                prev.span().1,
                item.track_index(),
                item.joins_in(),
                item.span().0,
            )
        });
        match units.last_mut() {
            Some(unit) if joins => unit.push(item),
            _ => units.push(vec![item]),
        }
    }
    units
}

fn unit_span(unit: &[Item<'_>]) -> (u64, u64) {
    let start = unit.first().map_or(0, |i| i.span().0);
    let end = unit.iter().map(|i| i.span().1).max().unwrap_or(start);
    (start, end)
}

/// Pushes the unit's source chains; returns its label and overlay origin.
fn unit_source(
    unit: &[Item<'_>],
    k: usize,
    plan: &RenderPlan,
    sources: &mut Vec<String>,
) -> (String, u32, u32) {
    let fps = plan.canvas.fps;
    if let [single] = unit {
        let b = single.bounds();
        sources.push(format!("{}[l{k}]", single.chain(fps, Placement::OnCanvas)));
        return (format!("[l{k}]"), b.x, b.y);
    }
    let placement = Placement::InGroup {
        canvas_w: plan.canvas.width,
        canvas_h: plan.canvas.height,
    };
    for (j, member) in unit.iter().enumerate() {
        sources.push(format!("{}[g{k}m{j}]", member.chain(fps, placement)));
    }
    let first_start = unit[0].span().0;
    let mut acc = format!("[g{k}m0]");
    for j in 1..unit.len() {
        let (start, _) = unit[j].span();
        let overlap = unit[j - 1].span().1 - start;
        let out = format!("[g{k}x{j}]");
        sources.push(format!(
            "{acc}[g{k}m{j}]xfade=transition=fade:duration={}:offset={}{out}",
            seconds(overlap),
            seconds(start - first_start)
        ));
        acc = out;
    }
    sources.push(format!(
        "{acc}setpts=PTS{}/TB[g{k}]",
        plus_seconds(first_start as i64)
    ));
    (format!("[g{k}]"), 0, 0)
}

/// The stage zoom (module doc), or `None` when no zoom cue is planned.
pub fn zoom_filter(cues: &[PlannedCue], fps: u32) -> Option<String> {
    let zooms: Vec<&PlannedCue> = cues.iter().filter(|c| c.kind == EffectKind::Zoom).collect();
    if zooms.is_empty() {
        return None;
    }
    let t = format!("(in-1)/{fps}");
    let left = piecewise(&zooms, &t, Edge::Near, Axis::X, "0");
    let right = piecewise(&zooms, &t, Edge::Far, Axis::X, "W");
    let top = piecewise(&zooms, &t, Edge::Near, Axis::Y, "0");
    let bottom = piecewise(&zooms, &t, Edge::Far, Axis::Y, "H");
    let enable: Vec<String> = zooms
        .iter()
        .map(|z| {
            format!(
                "between(t,{},{})",
                seconds(z.output_start),
                seconds(z.output_end)
            )
        })
        .collect();
    Some(format!(
        "perspective=x0='{left}':y0='{top}':x1='{right}':y1='{top}':\
         x2='{left}':y2='{bottom}':x3='{right}':y3='{bottom}':\
         interpolation=linear:eval=frame:enable='{}'",
        enable.join("+")
    ))
}

#[derive(Clone, Copy)]
enum Edge {
    Near,
    Far,
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

/// One corner coordinate over every zoom: the LAST listed cue is the
/// outermost branch, so it wins where cues overlap.
fn piecewise(zooms: &[&PlannedCue], t: &str, edge: Edge, axis: Axis, identity: &str) -> String {
    zooms.iter().fold(identity.to_string(), |inner, z| {
        format!(
            "if(between({t},{},{}),{},{inner})",
            seconds(z.output_start),
            seconds(z.output_end),
            zoom_edge(z, t, edge, axis)
        )
    })
}

/// One zoom's corner coordinate at stage time `t`: `st(0)` holds the ramp
/// (0..1, before the smoothstep), `st(1)` the scale, `st(2)` the half
/// window; the window's centre is clamped into `[half, 1 - half]`.
fn zoom_edge(z: &PlannedCue, t: &str, edge: Edge, axis: Axis) -> String {
    // The ORIGINAL span, `cut` restored (A13): a range must not move the
    // ramp.
    let from = z.output_start as i64 - z.cut.head_ms as i64;
    let to = z.output_end + z.cut.tail_ms;
    let ease_ms = z.zoom_ease_ms().unwrap_or(0.0);
    let ramp = if ease_ms <= 0.0 {
        "1".to_string()
    } else {
        let e = num(ease_ms / 1_000.0);
        format!(
            "min(1,min(({t}{})/{e},({}-{t})/{e}))",
            plus_seconds(-from),
            seconds(to)
        )
    };
    let factor = z
        .effect
        .factor
        .as_ref()
        .and_then(|n| n.as_f64())
        .unwrap_or(1.0);
    let (centre, extent) = match axis {
        Axis::X => (z.effect.x.as_f64().unwrap_or(0.5), "W"),
        Axis::Y => (z.effect.y.as_f64().unwrap_or(0.5), "H"),
    };
    let sign = match edge {
        Edge::Near => '-',
        Edge::Far => '+',
    };
    format!(
        "st(0,{ramp});st(1,1+{}*ld(0)*ld(0)*(3-2*ld(0)));st(2,0.5/ld(1));\
         (clip({},ld(2),1-ld(2)){sign}ld(2))*{extent}",
        num(factor - 1.0),
        num(centre)
    )
}
