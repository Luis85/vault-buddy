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

// Fix round 1 (review Minor #2): a lone `\r` (old Mac line ending) is a
// line break too, not something to drop silently; a CRLF pair must still
// collapse to ONE break, not two.
#[test]
fn a_lone_cr_is_a_line_break_and_crlf_is_not_doubled() {
    assert_eq!(escape_ass_text("a\rb"), r"a\Nb");
    assert_eq!(escape_ass_text("a\r\nb"), r"a\Nb");
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

// Fix round 1 (review Important #2): highlight had zero coverage. The box
// and stroke are asymmetric on purpose so a swapped axis or an uninset
// inner rect fails.
//
// Mutation check: collapse the inner rect onto the outer one (drop the
// stroke inset) and this goes red.
#[test]
fn highlight_ring_is_two_nested_rects_at_the_stroke_inset() {
    let c = canvas(CANVAS_W, CANVAS_H);
    let e = effect(
        EffectKind::Highlight,
        serde_json::json!({"x": 0.1, "y": 0.2, "w": 0.3, "h": 0.25, "stroke": 6, "color": "#ff00ff"}),
    );
    let mut p = plan(1_000, vec![]);
    p.canvas = c;
    p.cues = vec![cue(EffectKind::Highlight, 0, 1_000, e)];
    let doc = build_cue_ass(&p).expect("a highlight cue produces a document");
    // Outer: x=128 y=144 w=384 h=180 (0.1*1280, 0.2*720, 0.3*1280, 0.25*720).
    // Inner: the same box inset by the 6 px stroke on every edge.
    assert!(
        doc.contains(
            "m 128 144 l 512 144 l 512 324 l 128 324 m 134 150 l 506 150 l 506 318 l 134 318"
        ),
        "{doc}"
    );
    assert!(doc.contains("&H00FF00FF"), "highlight colour: {doc}");
}

// Fix round 1 (review Important #2 + Strengths note): the spotlight
// `dim` -> `\1a` alpha inversion was correct but UNTESTED. Two values,
// each pinning one end of the inversion.
//
// Mutation check: invert the formula (`dim * 255` instead of
// `(1 - dim) * 255`) and this goes red.
#[test]
fn spotlight_dim_converts_to_ass_alpha_at_two_values() {
    let c = canvas(CANVAS_W, CANVAS_H);
    let box_json = serde_json::json!({"x": 0.2, "y": 0.2, "w": 0.3, "h": 0.3});

    let mut opaque = box_json.clone();
    opaque["dim"] = serde_json::json!(1.0);
    let mut p = plan(1_000, vec![]);
    p.canvas = c.clone();
    p.cues = vec![cue(
        EffectKind::Spotlight,
        0,
        1_000,
        effect(EffectKind::Spotlight, opaque),
    )];
    let doc = build_cue_ass(&p).expect("a spotlight cue produces a document");
    assert!(
        doc.contains(r"\1a&H00&"),
        "dim=1.0 must be fully opaque: {doc}"
    );

    let mut transparent = box_json;
    transparent["dim"] = serde_json::json!(0.0);
    let mut p = plan(1_000, vec![]);
    p.canvas = c;
    p.cues = vec![cue(
        EffectKind::Spotlight,
        0,
        1_000,
        effect(EffectKind::Spotlight, transparent),
    )];
    let doc = build_cue_ass(&p).expect("a spotlight cue produces a document");
    assert!(
        doc.contains(r"\1a&HFF&"),
        "dim=0.0 must be fully transparent: {doc}"
    );
}

// Fix round 1 (review Important #2): the step circle (a Bezier
// approximation, independently re-derived here from the documented
// formula rather than by calling `circle_cmd`) and its SEPARATE number
// `Dialogue` had zero coverage.
//
// Mutation check: change `circle_cmd`'s kappa constant and this goes red.
#[test]
fn step_draws_a_circle_and_a_separate_number_dialogue() {
    let c = canvas(CANVAS_W, CANVAS_H);
    let e = effect(
        EffectKind::Step,
        serde_json::json!({"x": 0.5, "y": 0.5, "number": 7, "color": "#112233"}),
    );
    let mut p = plan(1_000, vec![]);
    p.canvas = c;
    p.cues = vec![cue(EffectKind::Step, 0, 1_000, e)];
    let doc = build_cue_ass(&p).expect("a step cue produces a document");

    let (cx, cy, r) = (640.0_f64, 360.0_f64, STEP_RADIUS);
    // The kappa is a LITERAL here, not `super::BEZIER_KAPPA`: reading the
    // production constant back would make this test blind to a mutation
    // of that very constant (measured -- an earlier draft that did
    // reference it stayed green against a deliberately wrong kappa).
    let k = r * 0.552_284_75;
    let pt = |x: f64, y: f64| format!("{} {}", fmt_num(x), fmt_num(y));
    let expected_circle = format!(
        "m {} b {} {} {} b {} {} {} b {} {} {} b {} {} {}",
        pt(cx + r, cy),
        pt(cx + r, cy + k),
        pt(cx + k, cy + r),
        pt(cx, cy + r),
        pt(cx - k, cy + r),
        pt(cx - r, cy + k),
        pt(cx - r, cy),
        pt(cx - r, cy - k),
        pt(cx - k, cy - r),
        pt(cx, cy - r),
        pt(cx + k, cy - r),
        pt(cx + r, cy - k),
        pt(cx + r, cy),
    );
    assert!(doc.contains(&expected_circle), "{doc}");
    assert!(
        doc.contains("&H00332211"),
        "the circle uses the effect's own colour: {doc}"
    );
    // The number is a SEPARATE Dialogue, Step style, at the same point.
    assert!(
        doc.contains(&format!(
            "Step,,0,0,0,,{{\\an7\\pos({},{})",
            fmt_num(cx),
            fmt_num(cy)
        )),
        "{doc}"
    );
    assert!(doc.contains("}7"), "the escaped number text: {doc}");
}

// Fix round 1 (review Minor #4): white is a deliberate choice (contrast
// against a circle whose OWN colour can be anything), not an oversight --
// pinned so a future "derive from the effect like every other kind" edit
// is a conscious change, not an accidental regression.
#[test]
fn step_number_colour_is_white_regardless_of_the_circles_own_colour() {
    let c = canvas(CANVAS_W, CANVAS_H);
    let e = effect(
        EffectKind::Step,
        serde_json::json!({"x": 0.5, "y": 0.5, "number": 3, "color": "#112233"}),
    );
    let mut p = plan(1_000, vec![]);
    p.canvas = c;
    p.cues = vec![cue(EffectKind::Step, 0, 1_000, e)];
    let doc = build_cue_ass(&p).expect("a step cue produces a document");
    // The number dialogue (the one carrying `\fs`) is white...
    let number_line = doc
        .lines()
        .find(|l| l.contains("\\fs"))
        .unwrap_or_else(|| panic!("no number dialogue: {doc}"));
    assert!(number_line.contains("&H00FFFFFF"), "{number_line}");
    // ...even though the circle itself carries the effect's own colour.
    assert!(doc.contains("&H00332211"), "{doc}");
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

fn card(title: &str, subtitle: &str) -> vault_buddy_core::editor::render_plan::PlannedCard {
    vault_buddy_core::editor::render_plan::PlannedCard {
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
            title: title.into(),
            subtitle: subtitle.into(),
            background: "#000000".into(),
            foreground: "#112233".into(),
            accent: "#445566".into(),
            extra: Map::new(),
        }),
        cut: Cut::default(),
    }
}

// Fix round 1 (review Important #1): the render used to top-left-anchor
// both lines with a flat padding and colour the subtitle with
// `foreground`, diverging from the ALREADY-SHIPPED preview
// (`src/editor/previewCardDom.ts`: a centered flex column, title in
// `foreground`, subtitle in `accent`). `PIP` (992,44,244,242) is
// asymmetric on purpose, so a swapped axis or an unswapped colour fails.
// Box centre: (1114, 165). title_h = 44*1.25 = 55, subtitle_h =
// 24*1.25 = 30, gap = 16 -> stack_h = 101 -> stack top = 165 - 50.5 =
// 114.5; subtitle_y = 114.5 + 55 + 16 = 185.5 -- both hand-computed from
// the module's own documented constants, not captured from its output.
//
// Mutation check: revert to the flat top-left `\an7`/padding anchor and
// this goes red.
#[test]
fn card_layout_matches_the_preview_centered_stack() {
    let mut p = plan(1_000, vec![]);
    p.cards = vec![card("A Title", "A Subtitle")];
    let doc = build_cue_ass(&p).expect("a card produces a document");
    assert!(
        doc.contains("CardTitle,,0,0,0,,{\\an8\\pos(1114,114.5)\\1c&H00332211"),
        "title: centered stack, top-anchored, foreground-coloured: {doc}"
    );
    assert!(doc.contains("A Title"), "{doc}");
    assert!(
        doc.contains("CardSubtitle,,0,0,0,,{\\an8\\pos(1114,185.5)\\1c&H00665544"),
        "subtitle: below the title, accent-coloured (not foreground): {doc}"
    );
    assert!(doc.contains("A Subtitle"), "{doc}");
}

// Fix round 1 (review Important #1's "subtitle-only card" case): no space
// is reserved for an absent title -- the lone subtitle centres on the
// BOX's own centre, `\an5`, not at the top-anchored position a title
// would have pushed it down from.
#[test]
fn card_with_only_a_subtitle_centres_it_alone() {
    let mut p = plan(1_000, vec![]);
    p.cards = vec![card("", "Only Sub")];
    let doc = build_cue_ass(&p).expect("a card produces a document");
    // "CardTitle" itself still names a STYLE (every style is always
    // declared); what must be absent is a CardTitle *event*.
    assert!(
        !doc.contains("CardTitle,,0,0,0,,"),
        "no title dialogue at all: {doc}"
    );
    assert!(
        doc.contains("CardSubtitle,,0,0,0,,{\\an5\\pos(1114,165)\\1c&H00665544"),
        "the subtitle alone centres on the box centre: {doc}"
    );
}

// A zoom cue is a camera move (`video_graph::zoom_filter`'s job), never a
// drawing -- it must not reach the ASS document at all.
#[test]
fn zoom_cues_produce_no_dialogue() {
    let mut p = plan(1_000, vec![]);
    p.cues = vec![zoom("z1", 0, 1_000, 2.0, 0.5, 0.5, None)];
    assert!(build_cue_ass(&p).is_none());
}
