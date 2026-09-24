//! The ASS generator's tests (Task 43).

use super::*;
use crate::render::test_support::{plan, zoom, CANVAS_H, CANVAS_W, PIP};
use serde::Deserialize;
use vault_buddy_core::editor::model::{Canvas, Card, CardPreset, FadeCurve};
use vault_buddy_core::editor::render_plan::Cut;
use vault_buddy_core::editor::Map;

fn canvas(width: u32, height: u32) -> Canvas {
    Canvas {
        width,
        height,
        fps: 30,
        extra: Map::new(),
    }
}

fn effect(kind: EffectKind, value: serde_json::Value) -> Effect {
    let mut merged = serde_json::json!({
        "id": "e1", "clip_id": "c1", "kind": kind_str(kind),
        "start_ms": 0, "end_ms": 1_000, "x": 0.0, "y": 0.0, "color": "#ffffff",
    });
    for (k, v) in value.as_object().expect("object") {
        merged[k] = v.clone();
    }
    serde_json::from_value(merged).expect("a valid effect")
}

fn kind_str(kind: EffectKind) -> &'static str {
    match kind {
        EffectKind::Text => "text",
        EffectKind::Arrow => "arrow",
        EffectKind::Highlight => "highlight",
        EffectKind::Spotlight => "spotlight",
        EffectKind::Zoom => "zoom",
        EffectKind::Step => "step",
        EffectKind::Mask => "mask",
    }
}

fn cue(kind: EffectKind, start: u64, end: u64, effect: Effect) -> PlannedCue {
    PlannedCue {
        effect_id: "e1".into(),
        clip_id: "c1".into(),
        kind,
        output_start: start,
        output_end: end,
        effect,
        cut: Cut::default(),
    }
}

// ---------------------------------------------------------------------
// The three pure conversions.
// ---------------------------------------------------------------------

// Mutation check: swap the byte order (emit rr,gg,bb instead of bb,gg,rr)
// and this goes red.
#[test]
fn colour_converts_to_ass_bgr_order() {
    assert_eq!(ass_colour("#112233"), "&H00332211");
    assert_eq!(ass_colour("#ffffff"), "&H00FFFFFF");
    // An invalid string degrades to opaque white rather than propagating a
    // malformed colour into the filtergraph.
    assert_eq!(ass_colour("not-a-colour"), "&H00FFFFFF");
}

// Mutation check: round the end DOWN instead of up and the boundary
// assertion (1234 -> 0:00:01.24) goes red.
#[test]
fn times_floor_start_and_ceil_end() {
    assert_eq!(ass_time(1_234, false), "0:00:01.23");
    assert_eq!(ass_time(1_234, true), "0:00:01.24");
    // A value that already divides evenly needs no extra centisecond in
    // either direction -- a half-open end must not bleed into whatever
    // starts exactly where this one ends.
    assert_eq!(ass_time(1_230, false), "0:00:01.23");
    assert_eq!(ass_time(1_230, true), "0:00:01.23");
    // An hour-scale value is unpadded on the hour field.
    assert_eq!(ass_time(3_661_000, false), "1:01:01.00");
}

#[test]
fn text_is_escaped() {
    // A hand-typed override sequence must survive as literal text, never
    // be read as an ASS `{...}` override block.
    assert_eq!(escape_ass_text(r"{\b1}"), r"\{\\b1\}");
    assert_eq!(escape_ass_text("line one\nline two"), r"line one\Nline two");
}

// ---------------------------------------------------------------------
// The shared arrow fixture (Task 35's geometry, read by both languages).
// ---------------------------------------------------------------------

#[derive(Deserialize)]
struct ArrowFixture {
    cases: Vec<ArrowCase>,
}

#[derive(Deserialize)]
struct ArrowCase {
    name: String,
    canvas: FixtureCanvas,
    x: f64,
    y: f64,
    x2: f64,
    y2: f64,
    stroke: f64,
    expected: Option<ExpectedArrow>,
}

#[derive(Deserialize)]
struct FixtureCanvas {
    width: f64,
    height: f64,
}

#[derive(Deserialize)]
struct ExpectedArrow {
    shaft: Vec<[f64; 2]>,
    head: Vec<[f64; 2]>,
}

const ARROW_FIXTURE: &str = include_str!("../../../../tests/fixtures/editor-arrow-cases.json");

#[test]
fn arrow_matches_the_shared_fixture() {
    let fixture: ArrowFixture = serde_json::from_str(ARROW_FIXTURE).expect("fixture parses");
    assert_eq!(
        fixture.cases.len(),
        4,
        "fixture row count drifted -- re-count, never increment"
    );
    for case in &fixture.cases {
        let got = arrow_path(
            case.x,
            case.y,
            case.x2,
            case.y2,
            case.stroke,
            case.canvas.width,
            case.canvas.height,
        );
        match &case.expected {
            None => assert!(
                got.is_none(),
                "{}: expected a zero-length arrow to draw nothing",
                case.name
            ),
            Some(exp) => {
                let got =
                    got.unwrap_or_else(|| panic!("{}: expected an arrow, got None", case.name));
                let shaft: Vec<[f64; 2]> = got.shaft.iter().map(|&(x, y)| [x, y]).collect();
                let head: Vec<[f64; 2]> = got.head.iter().map(|&(x, y)| [x, y]).collect();
                assert_eq!(shaft, exp.shaft, "{}: shaft", case.name);
                assert_eq!(head, exp.head, "{}: head", case.name);
            }
        }
    }
}

// ---------------------------------------------------------------------
// Drawings and text.
// ---------------------------------------------------------------------

// Mutation check: default the mask to the cue fade instead of `\fad(0,0)`
// and this goes red.
#[test]
fn mask_has_no_fade() {
    let c = canvas(CANVAS_W, CANVAS_H);
    let e = effect(
        EffectKind::Mask,
        serde_json::json!({"x": 0.1, "y": 0.1, "w": 0.3, "h": 0.2}),
    );
    let mut p = plan(1_000, vec![]);
    p.canvas = c;
    p.cues = vec![cue(EffectKind::Mask, 0, 1_000, e)];
    let doc = build_cue_ass(&p).expect("a mask cue produces a document");
    assert!(doc.contains(r"\fad(0,0)"), "{doc}");
    assert!(
        !doc.contains(r"\fad(150"),
        "a privacy cover must never fade in: {doc}"
    );
}

#[test]
fn captions_at_bottom_use_the_bottom_alignment() {
    let bottom = super::PlannedCaptions {
        font_size: 24.0,
        position: CaptionPosition::Bottom,
        background: false,
        cues: vec![],
    };
    assert_eq!(
        caption_style_line(Some(&bottom)),
        "Style: Caption,Segoe UI,24,&H00FFFFFF,&H000000FF,&H00000000,&H00000000,\
         0,0,0,0,100,100,0,0,1,2,0,2,10,10,20,1"
    );
    let top = super::PlannedCaptions {
        position: CaptionPosition::Top,
        background: true,
        ..bottom
    };
    assert_eq!(
        caption_style_line(Some(&top)),
        "Style: Caption,Segoe UI,24,&H00FFFFFF,&H000000FF,&H00000000,&H80000000,\
         0,0,0,0,100,100,0,0,3,2,0,8,10,10,20,1"
    );
}

// End to end: a plan whose captions are set to burn in at the bottom
// produces a caption document whose Caption style carries the bottom
// alignment.
#[test]
fn build_caption_ass_wires_the_position_into_the_style() {
    let mut p = plan(2_000, vec![]);
    p.captions = Some(super::PlannedCaptions {
        font_size: 28.0,
        position: CaptionPosition::Bottom,
        background: false,
        cues: vec![PlannedCaption {
            id: "q1".into(),
            output_start: 0,
            output_end: 1_000,
            text: "hello".into(),
        }],
    });
    let doc = build_caption_ass(&p).expect("captions produce a document");
    assert!(
        doc.contains(&caption_style_line(p.captions.as_ref())),
        "{doc}"
    );
    assert!(doc.contains("hello"), "{doc}");
}

#[test]
fn build_caption_ass_is_none_without_captions() {
    let p = plan(1_000, vec![]);
    assert!(build_caption_ass(&p).is_none());
}

#[test]
fn build_cue_ass_is_none_without_cues_or_cards() {
    let p = plan(1_000, vec![]);
    assert!(build_cue_ass(&p).is_none());
}

// ---------------------------------------------------------------------
// Canvas and range.
// ---------------------------------------------------------------------

#[test]
fn play_res_equals_the_canvas() {
    // Portrait, deliberately not the landscape fixture canvas, so a
    // swapped width/height fails.
    let mut p = plan(1_000, vec![]);
    p.canvas = canvas(720, 1_280);
    p.cues = vec![cue(
        EffectKind::Text,
        0,
        1_000,
        effect(EffectKind::Text, serde_json::json!({"text": "hi"})),
    )];
    let doc = build_cue_ass(&p).expect("a text cue produces a document");
    assert!(doc.contains("PlayResX: 720\n"), "{doc}");
    assert!(doc.contains("PlayResY: 1280\n"), "{doc}");
}

// A cue whose ORIGINAL span began before the render's window (a big `cut`)
// must still be timed off its already-rebased `output_start`/`output_end`
// -- never re-derived by adding the cut back in, which would place it
// outside the range the render actually covers.
#[test]
fn range_plan_emits_rebased_times() {
    let mut p = plan(2_000, vec![]);
    let mut c = cue(
        EffectKind::Text,
        200,
        700,
        effect(EffectKind::Text, serde_json::json!({"text": "hi"})),
    );
    c.cut = Cut {
        head_ms: 5_000,
        tail_ms: 100,
    };
    p.cues = vec![c];
    let doc = build_cue_ass(&p).expect("a text cue produces a document");
    assert!(
        doc.contains(&format!(
            "Dialogue: 0,{},{}",
            ass_time(200, false),
            ass_time(700, true)
        )),
        "{doc}"
    );
    // Nothing derived from the cut (5 000 ms) leaks into the timestamp.
    assert!(!doc.contains(&ass_time(5_200, false)), "{doc}");
}

// ---------------------------------------------------------------------
// Card text (F-37) -- not individually named in the brief's test list but
// in scope (F-37), so it gets a regression test too.
// ---------------------------------------------------------------------

#[test]
fn card_title_and_subtitle_are_rendered() {
    let mut p = plan(1_000, vec![]);
    p.cards = vec![vault_buddy_core::editor::render_plan::PlannedCard {
        clip_id: "card".into(),
        track_index: 0,
        output_start: 0,
        output_end: 1_000,
        bounds: PIP,
        opacity: 1.0,
        fade_in: 0,
        fade_out: 0,
        fade_curve: FadeCurve::Linear,
        transition_in: None,
        transition_out: None,
        card: Some(Card {
            preset: CardPreset::Chapter,
            title: "A Title".into(),
            subtitle: "A Subtitle".into(),
            background: "#000000".into(),
            foreground: "#ffffff".into(),
            accent: "#ff0000".into(),
            extra: Map::new(),
        }),
        cut: Cut::default(),
    }];
    let doc = build_cue_ass(&p).expect("a card produces a document");
    assert!(doc.contains("CardTitle"), "{doc}");
    assert!(doc.contains("A Title"), "{doc}");
    assert!(doc.contains("CardSubtitle"), "{doc}");
    assert!(doc.contains("A Subtitle"), "{doc}");
}

// A zoom cue is a camera move (`video_graph::zoom_filter`'s job), never a
// drawing -- it must not reach the ASS document at all.
#[test]
fn zoom_cues_produce_no_dialogue() {
    let mut p = plan(1_000, vec![]);
    p.cues = vec![zoom("z1", 0, 1_000, 2.0, 0.5, 0.5, None)];
    assert!(build_cue_ass(&p).is_none());
}
