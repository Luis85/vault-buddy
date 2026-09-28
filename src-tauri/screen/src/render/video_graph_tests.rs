//! Golden strings for the composed video graph (Task 42).

use super::test_support::{card, layer, plan, zoom, FULL, PIP};
use super::video_graph::{build_video_graph, build_video_graph_with_hooks};
use vault_buddy_core::editor::model_cues::TransitionKind;
use vault_buddy_core::editor::render_plan::PixelBox;

fn position(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} missing from {haystack}"))
}

/// A full-frame bottom layer (input 0, track 1) under a picture-in-picture
/// top layer (input 1, track 0) -- track 0 is the TOP lane.
fn two_layer_plan() -> vault_buddy_core::editor::render_plan::RenderPlan {
    plan(
        5_000,
        vec![layer(0, 1, 0, 5_000, FULL), layer(1, 0, 1_000, 4_000, PIP)],
    )
}

// `RenderPlan::video_layers` is bottom -> top; overlaying in that order is
// what puts the top lane on top. Reversed, the full-frame layer is drawn
// OVER the picture-in-picture and hides it completely -- a valid file with
// the webcam simply missing.
#[test]
fn layers_overlay_bottom_to_top() {
    let (extra, graph, out) = build_video_graph(&two_layer_plan());
    // The canvas is the input after the plan's own two.
    assert_eq!(
        extra,
        vec!["-f", "lavfi", "-i", "color=c=black:s=1280x720:r=30:d=5.000"]
    );
    let bottom = "[2:v][l0]overlay=x=0:y=0:eof_action=pass:enable='between(t,0.000,5.000)'[c0]";
    let top = "[c0][l1]overlay=x=992:y=44:eof_action=pass:enable='between(t,1.000,4.000)'[vcomp]";
    assert!(position(&graph, bottom) < position(&graph, top), "{graph}");
    assert!(
        graph.starts_with("[0:v]trim=start=1.000:end=6.000,"),
        "{graph}"
    );
    assert!(
        graph.contains(";[1:v]trim=start=1.000:end=4.000,"),
        "{graph}"
    );
    assert_eq!(out, "[vout]");
}

// Chroma subsampling needs even geometry. The plan already rounds edges to
// even pixels, but the graph is the last line of defence and rounds DOWN:
// an odd box or offset reaching the encoder shifts the chroma by a pixel
// or fails the scale outright.
#[test]
fn box_is_even_rounded() {
    let odd = PixelBox {
        x: 993,
        y: 43,
        w: 243,
        h: 241,
    };
    let (_, graph, _) = build_video_graph(&plan(5_000, vec![layer(0, 0, 0, 5_000, odd)]));
    assert!(graph.contains("scale=242:240:"), "{graph}");
    assert!(graph.contains("pad=242:240:"), "{graph}");
    assert!(graph.contains("overlay=x=992:y=42:"), "{graph}");
    assert!(!graph.contains("243"), "{graph}");
}

// F-19: a dissolve is the two clips of ONE track blended over their
// overlap. Task 41 plans both halves (transition_out on `from`, _in on
// `to`); the graph pairs them into one xfade whose OFFSET is where `to`
// starts inside the pair -- 2.4 s here, distinct from the 0.6 s duration
// and from `from`'s own 3 s length, so a swapped or mis-derived argument
// fails. Each member is padded onto a transparent canvas at its own box
// so two differently-sized boxes can still be blended.
#[test]
fn dissolve_uses_xfade_with_the_overlap_offset() {
    let mut from = layer(0, 0, 0, 3_000, FULL);
    from.transition_out = Some((TransitionKind::Dissolve, 600));
    let mut to = layer(1, 0, 2_400, 5_000, PIP);
    to.transition_in = Some((TransitionKind::Dissolve, 600));
    let (_, graph, _) = build_video_graph(&plan(5_000, vec![from, to]));
    assert!(
        graph.contains("[g0m0][g0m1]xfade=transition=fade:duration=0.600:offset=2.400[g0x1]"),
        "{graph}"
    );
    assert!(
        graph.contains("setpts=PTS-STARTPTS,format=rgba,pad=1280:720:992:44:color=black@0[g0m1]"),
        "{graph}"
    );
    assert!(graph.contains("[g0x1]setpts=PTS+0.000/TB[g0]"), "{graph}");
    assert!(
        graph.contains(
            "[2:v][g0]overlay=x=0:y=0:eof_action=pass:enable='between(t,0.000,5.000)'[vcomp]"
        ),
        "{graph}"
    );
    // The members are never ALSO overlaid on their own.
    assert!(!graph.contains("[l0]"), "{graph}");
}

// GAP-173 / Task 35: the render zooms like the preview -- the LAST active
// zoom wins, the ramp is a smoothstep over `easing` ms (600 when unset),
// capped at half the cue's span. z1 (listed first) spans 1-3 s with no
// easing (0.6 s ramp); z2 (listed last) spans 2-2.8 s with easing 1000
// capped to 0.4 s. So z2 is the OUTER branch and each carries its own ramp.
#[test]
fn zoom_expression_is_piecewise_and_eased() {
    let mut p = plan(5_000, vec![layer(0, 0, 0, 5_000, FULL)]);
    p.cues = vec![
        zoom("z1", 1_000, 3_000, 2.0, 0.25, 0.75, None),
        zoom("z2", 2_000, 2_800, 3.0, 0.8, 0.3, Some(1_000.0)),
    ];
    let (_, graph, _) = build_video_graph(&p);
    let t = "(in-1)/30";
    let z2 = format!(
        "st(0,min(1,min(({t}-2.000)/0.4,(2.800-{t})/0.4)));\
         st(1,1+2*ld(0)*ld(0)*(3-2*ld(0)));st(2,0.5/ld(1));(clip(0.8,ld(2),1-ld(2))-ld(2))*W"
    );
    let z1 = format!(
        "st(0,min(1,min(({t}-1.000)/0.6,(3.000-{t})/0.6)));\
         st(1,1+1*ld(0)*ld(0)*(3-2*ld(0)));st(2,0.5/ld(1));(clip(0.25,ld(2),1-ld(2))-ld(2))*W"
    );
    let x0 = format!("x0='if(between({t},2.000,2.800),{z2},if(between({t},1.000,3.000),{z1},0))'");
    assert!(graph.contains(&x0), "{graph}\n--- want ---\n{x0}");
    // y uses the cue's y and H, and the far edges add the half-window.
    assert!(
        graph.contains("(clip(0.3,ld(2),1-ld(2))+ld(2))*H"),
        "{graph}"
    );
    assert!(
        graph.contains(":interpolation=linear:eval=frame:enable='between(t,1.000,3.000)+between(t,2.000,2.800)'"),
        "{graph}"
    );
    // The zoom sits between the two hook labels (controller ruling).
    assert!(position(&graph, "[vcomp]perspective=") < position(&graph, "[vzoomed]"));
}

// A13: a range starting 300 ms into a zoom keeps the ramp where it really
// is -- the cue's original start is -0.3 s on the render's clock, so the
// first frame is already half-way up a 0.6 s ramp.
#[test]
fn a_range_cut_zoom_ramps_from_its_original_start() {
    let mut p = plan(2_000, vec![layer(0, 0, 0, 2_000, FULL)]);
    let mut z = zoom("z", 0, 1_700, 2.0, 0.5, 0.5, None);
    z.cut.head_ms = 300;
    p.cues = vec![z];
    let (_, graph, _) = build_video_graph(&p);
    assert!(
        graph.contains("st(0,min(1,min(((in-1)/30+0.300)/0.6,(1.700-(in-1)/30)/0.6)))"),
        "{graph}"
    );
}

// Controller ruling (GAP-173 zoom scope): composed layers -> pre-zoom hook
// ([vcomp], Task 43's cues + card text) -> zoom -> post-zoom hook
// ([vzoomed], Task 43's burned captions) -> the final label.
#[test]
fn the_hooks_bracket_the_zoom_and_the_final_label_follows_the_post_hook() {
    let (_, bare, out) = build_video_graph(&two_layer_plan());
    assert!(
        bare.ends_with("[vcomp]null[vzoomed];[vzoomed]null[vout]"),
        "{bare}"
    );
    assert_eq!(out, "[vout]");
    let (_, hooked, out) = build_video_graph_with_hooks(
        &two_layer_plan(),
        Some("ass=cues.ass"),
        Some("ass=caps.ass"),
    );
    assert!(
        hooked.ends_with(
            "[vcomp]ass=cues.ass[vcued];[vcued]null[vzoomed];[vzoomed]ass=caps.ass[vout]"
        ),
        "{hooked}"
    );
    assert_eq!(out, "[vout]");
}

// A card is one more layer in the same bottom -> top order, its picture an
// extra lavfi colour input after the canvas.
#[test]
fn a_card_is_overlaid_in_track_order_from_its_own_colour_input() {
    let mut p = plan(5_000, vec![layer(0, 1, 0, 5_000, FULL)]);
    p.cards = vec![card(0, 1_000, 2_000, "#123456")];
    let (extra, graph, _) = build_video_graph(&p);
    assert_eq!(
        extra,
        vec![
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=1280x720:r=30:d=5.000",
            "-f",
            "lavfi",
            "-i",
            "color=c=0x123456:s=244x242:r=30:d=1.000",
        ]
    );
    assert!(graph.contains(";[2:v]format=rgba,"), "{graph}");
    assert!(
        graph.contains(
            "[c0][l1]overlay=x=992:y=44:eof_action=pass:enable='between(t,1.000,2.000)'[vcomp]"
        ),
        "{graph}"
    );
}

// An empty composition is still a canvas of the plan's length.
#[test]
fn a_plan_with_nothing_visible_renders_the_black_canvas() {
    let (extra, graph, _) = build_video_graph(&plan(2_500, Vec::new()));
    assert_eq!(extra[3], "color=c=black:s=1280x720:r=30:d=2.500");
    assert!(graph.starts_with("[0:v]null[vcomp];"), "{graph}");
}
