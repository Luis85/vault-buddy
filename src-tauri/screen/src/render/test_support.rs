//! Hand-built render plans for the graph tests (Task 42).
//!
//! Built directly rather than through `render_plan::plan` so each test
//! states exactly the one layer property it is about; the plan's own
//! derivation is Task 41's and has its own suite. The fixture is
//! deliberately ASYMMETRIC (global constraints): a 1280x720 canvas and a
//! picture-in-picture box of 0.19 x 0.338 of it at (0.775, 0.06) -- the
//! plan's even-edge rounding of that is x 992, y 44, w 244, h 242, so a
//! swapped axis or a dropped offset cannot pass.

use serde_json::json;
use vault_buddy_core::editor::model::{Canvas, Card, FadeCurve, Fit, FrameShape, Rotation};
use vault_buddy_core::editor::model_cues::{Effect, EffectKind};
use vault_buddy_core::editor::render_plan::{
    Cut, PixelBox, PlanInput, PlannedCard, PlannedCue, RenderPlan, VideoLayer,
};

pub const CANVAS_W: u32 = 1280;
pub const CANVAS_H: u32 = 720;

/// The fixture's picture-in-picture box (see the module doc).
pub const PIP: PixelBox = PixelBox {
    x: 992,
    y: 44,
    w: 244,
    h: 242,
};

pub const FULL: PixelBox = PixelBox {
    x: 0,
    y: 0,
    w: CANVAS_W,
    h: CANVAS_H,
};

pub fn canvas() -> Canvas {
    Canvas {
        width: CANVAS_W,
        height: CANVAS_H,
        fps: 30,
        extra: Default::default(),
    }
}

/// A plain 1x rectangle layer of `input`, `[start, end)` output ms, whose
/// source starts at 1 s (so a trim that ignored `source_in` fails).
pub fn layer(
    input: usize,
    track_index: usize,
    start: u64,
    end: u64,
    bounds: PixelBox,
) -> VideoLayer {
    VideoLayer {
        clip_id: format!("clip-{input}-{start}"),
        track_index,
        input,
        output_start: start,
        output_end: end,
        source_in: 1_000,
        source_out: 1_000 + (end - start),
        speed: 1.0,
        bounds,
        fit: Fit::Contain,
        frame_shape: FrameShape::Rectangle,
        rotation: Rotation::Deg0,
        mirror: false,
        flip_y: false,
        crop: None,
        opacity: 1.0,
        fade_in: 0,
        fade_out: 0,
        fade_curve: FadeCurve::Linear,
        adjustments: None,
        transition_in: None,
        transition_out: None,
        cut: Cut::default(),
    }
}

pub fn input(index: usize, is_image: bool) -> PlanInput {
    PlanInput {
        asset_id: format!("asset-{index}"),
        input_index: index,
        has_video: true,
        has_audio: false,
        width: 1920,
        height: 1080,
        is_image,
        duration_ms: 60_000,
    }
}

/// A plan over `layers`, with one input per distinct layer input.
pub fn plan(duration_ms: u64, layers: Vec<VideoLayer>) -> RenderPlan {
    let mut indices: Vec<usize> = layers.iter().map(|l| l.input).collect();
    indices.sort_unstable();
    indices.dedup();
    RenderPlan {
        canvas: canvas(),
        duration_ms,
        inputs: indices.into_iter().map(|i| input(i, false)).collect(),
        video_layers: layers,
        audio: Vec::new(),
        cues: Vec::new(),
        captions: None,
        chapters: Vec::new(),
        cards: Vec::new(),
        master_gain: 1.0,
    }
}

pub fn card(track_index: usize, start: u64, end: u64, background: &str) -> PlannedCard {
    PlannedCard {
        clip_id: format!("card-{start}"),
        track_index,
        output_start: start,
        output_end: end,
        bounds: PIP,
        opacity: 1.0,
        fade_in: 0,
        fade_out: 0,
        fade_curve: FadeCurve::Linear,
        transition_in: None,
        transition_out: None,
        card: Some(Card {
            preset: vault_buddy_core::editor::model::CardPreset::Chapter,
            title: "Title".into(),
            subtitle: "Sub".into(),
            background: background.into(),
            foreground: "#ffffff".into(),
            accent: "#ff0000".into(),
            extra: Default::default(),
        }),
        cut: Cut::default(),
    }
}

/// A zoom cue over `[start, end)` output ms.
pub fn zoom(
    id: &str,
    start: u64,
    end: u64,
    factor: f64,
    x: f64,
    y: f64,
    easing: Option<f64>,
) -> PlannedCue {
    let mut value = json!({
        "id": id, "clip_id": "clip", "kind": "zoom", "start_ms": start, "end_ms": end,
        "x": x, "y": y, "color": "#ffffff", "factor": factor,
    });
    if let Some(e) = easing {
        value["easing"] = json!(e);
    }
    let effect: Effect = serde_json::from_value(value).expect("a zoom effect");
    PlannedCue {
        effect_id: id.into(),
        clip_id: "clip".into(),
        kind: EffectKind::Zoom,
        output_start: start,
        output_end: end,
        effect,
        cut: Cut::default(),
    }
}
