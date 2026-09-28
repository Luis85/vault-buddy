//! The render's ASS documents (tutorial-editor Task 43; F-27--F-33, F-35,
//! F-37). PURE: a `RenderPlan` in, an ASS subtitle document out -- no file
//! I/O, no ffmpeg invocation. `render::mod`'s `render_args` burns the
//! result at the video graph's two hooks (`video_graph`'s module doc,
//! GAP-173's controller ruling): teaching cues and card text at the
//! PRE-zoom hook (`[vcomp]`), so the preview's zoom scales them with the
//! media; burned-in captions at the POST-zoom hook (`[vzoomed]`), so the
//! preview keeps them unzoomed on the frame. That is why this module
//! builds TWO documents, `build_cue_ass` and `build_caption_ass`, never
//! one -- a single document has no way to land at two different hooks.
//!
//! **The arrow is the shared geometry.** `arrow_path` is the Rust twin of
//! `src/editor/cueGeometry.ts`'s `arrowPath`: same operations in the same
//! order (`sqrt`, never `hypot` -- correctly rounded in IEEE-754
//! everywhere, where `hypot` is library-dependent), same 2-decimal
//! half-away-from-zero rounding, so the preview's SVG arrow and the
//! rendered ASS arrow are the same polygons. `arrow_matches_the_shared_fixture`
//! (in the test sibling) reads the same
//! `tests/fixtures/editor-arrow-cases.json` Task 35 wrote and Task 35's
//! own test reads, with the same 4-row size guard.
//!
//! **Colours.** A cue/card colour is `#rrggbb` (`Effect.color`,
//! `Card.foreground`, ...); ASS wants `&HAABBGGRR`. `ass_colour` is the
//! ONE conversion both cue and card colours go through -- swapping the
//! byte order is the exact defect its mutation check is for. An invalid
//! hex string (never emitted by the validated editor, but this module
//! reads the same string a hand-edited project could carry) degrades to
//! opaque white rather than propagating a malformed `&H...` into the
//! filtergraph.
//!
//! **Times.** ASS timestamps are `H:MM:SS.cc` (H unpadded, cc =
//! centiseconds). `output_start`/`output_end` are already OUTPUT time,
//! range-rebased by Task 41's `render_plan::plan` -- this module never
//! re-derives an absolute time, only formats the field it is given. The
//! start rounds DOWN and the end rounds UP to the centisecond a cue's ms
//! span does not divide evenly by: a cue must never show later than its
//! real start (rounding the start up would delay it) and must never
//! disappear before its real end (rounding the end down would cut it
//! short) -- `times_floor_start_and_ceil_end`'s boundary case pins that an
//! EXACT centisecond value needs no extra rounding in either direction.
//!
//! **Text.** `escape_ass_text` escapes `{`, `}` and `\` (ASS reads an
//! unescaped `{` as the start of an override block) and turns a real
//! newline into the hard-break code `\N` -- never left as a literal
//! newline, which a `Dialogue` line cannot contain at all.
//!
//! **Fonts.** Every style names `Segoe UI` with `Arial` as the documented
//! fallback (the `[Script Info]` comment): ASS carries one font name per
//! style, so "fallback" here is a note for whoever configures the
//! render's `fontsdir` (`render::mod`'s `AssHooks`), not a second font
//! this module selects. No font is bundled (AGENTS.md: ffmpeg stays
//! user-installed).
//!
//! **Drawings (`\p1`).** Arrow: the shared shaft quad + head triangle.
//! Highlight: an outer rect and an inset inner rect in one path -- ASS
//! drawings fill even-odd, so the overlap is automatically a ring; no
//! winding-direction trick needed. Spotlight: the four dimmed bands
//! around its box, the same four rectangles `src/editor/cueGeometry.ts`'s
//! `spotlightRects` computes, with `\1a` derived from `dim` (0 = no
//! dimming = fully transparent band, 1 = fully opaque). Step: a circle
//! via four cubic-Bezier quarter-arcs (the standard kappa = 0.55228475
//! approximation) plus a SEPARATE text `Dialogue` for the number -- a
//! drawing and glyph text do not reliably share one event with correct
//! centring, since this module has no font metrics to centre the number
//! against the circle's own drawn radius. Mask: an opaque rect with
//! `\fad(0,0)` -- explicit, not merely omitted, because a privacy cover
//! that faded in would show the very frame it exists to hide for its
//! first 150 ms.
//!
//! **`\pos`/`\an7` uniformly.** Every cue `Dialogue` (drawing or text)
//! carries `\an7` (top-left alignment): a drawing's `\pos(0,0)` then makes
//! its `\p1` coordinates literal canvas pixels with no translation to
//! undo, and a text cue's `\pos(x,y)` places that point at the text's own
//! top-left corner -- both are the standard libass technique for
//! positioning a vector drawing or a caption-style text block by absolute
//! coordinate rather than by the engine's own line-layout.

use vault_buddy_core::editor::model::Canvas;
use vault_buddy_core::editor::model_cues::{CaptionPosition, EffectKind};
use vault_buddy_core::editor::render_plan::{
    PlannedCaption, PlannedCaptions, PlannedCard, PlannedCue, RenderPlan,
};
use vault_buddy_core::editor::{model_cues::Effect, Num};

use super::expr::num as fmt_num;

/// The fade every teaching-cue drawing/text carries at each end (the
/// module doc); a mask is the one exception (`\fad(0,0)`, explicit).
const FADE_MS: u64 = 150;

const FONT: &str = "Segoe UI";
const TEXT_DEFAULT_FONT_SIZE: f64 = 32.0;
const STEP_DEFAULT_FONT_SIZE: f64 = 26.0;
const STEP_RADIUS: f64 = 22.0;
/// The cubic-Bezier control-point offset that approximates a quarter
/// circle of radius `r` to within about 0.03 % (the standard constant,
/// `4/3 * (sqrt(2) - 1)`).
const BEZIER_KAPPA: f64 = 0.552_284_75;
const ARROW_DEFAULT_STROKE: f64 = 5.0;
const HIGHLIGHT_DEFAULT_STROKE: f64 = 4.0;
const HIGHLIGHT_DEFAULT_W: f64 = 0.2;
const HIGHLIGHT_DEFAULT_H: f64 = 0.1;
/// The reference editor's own default (`drawing-primitives.js`: `e.dim||.65`).
const SPOTLIGHT_DEFAULT_DIM: f64 = 0.65;
const CARD_TITLE_FONT: f64 = 44.0;
const CARD_SUBTITLE_FONT: f64 = 24.0;
const CARD_LINE_GAP: f64 = 16.0;
/// The estimated line height as a multiple of font size -- an
/// approximation (this module reads no font file), unlike the preview's
/// own flex layout, which measures real glyph boxes (GAP-173).
const CARD_LINE_HEIGHT: f64 = 1.25;
/// ASS numpad alignment: horizontally centred, vertically centred on the
/// given point.
const ALIGN_MID_CENTER: u8 = 5;
/// ASS numpad alignment: horizontally centred, the point is the TOP edge.
const ALIGN_TOP_CENTER: u8 = 8;
const CAPTION_DEFAULT_FONT_SIZE: f64 = 32.0;

// ---------------------------------------------------------------------
// Colour, time and text -- the three pure conversions everything above
// leans on, each with its own named test and mutation check.
// ---------------------------------------------------------------------

/// `#rrggbb` -> ASS `&HAABBGGRR` (alpha 00 = opaque). An invalid string
/// degrades to opaque white rather than emitting a malformed colour into
/// the filtergraph -- never emitted by the validated editor, but a
/// hand-edited project's `Effect.color`/`Card.foreground` reaches this
/// same string unchanged.
pub(super) fn ass_colour(hex: &str) -> String {
    let valid = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()));
    match valid {
        Some(h) => {
            let (rr, gg, bb) = (&h[0..2], &h[2..4], &h[4..6]);
            format!(
                "&H00{}{}{}",
                bb.to_uppercase(),
                gg.to_uppercase(),
                rr.to_uppercase()
            )
        }
        None => "&H00FFFFFF".to_string(),
    }
}

/// `ms` as `H:MM:SS.cc` (H unpadded), rounded to the centisecond DOWN
/// (`round_up = false`, a `Dialogue` Start) or UP (`round_up = true`, a
/// `Dialogue` End) -- the module doc's "never shows late, never ends
/// early" rule.
pub(super) fn ass_time(ms: u64, round_up: bool) -> String {
    let cs = if round_up { ms.div_ceil(10) } else { ms / 10 };
    let (h, rem) = (cs / 360_000, cs % 360_000);
    let (m, rem) = (rem / 6_000, rem % 6_000);
    let (s, cs) = (rem / 100, rem % 100);
    format!("{h}:{m:02}:{s:02}.{cs:02}")
}

/// `{`, `}` and `\` escaped so a hand-typed override sequence in cue/card
/// text (`{\b1}`) survives as literal text rather than being read as an
/// ASS override block; a real newline becomes the hard-break code `\N`,
/// which a `Dialogue` line's single text field cannot otherwise carry.
pub(super) fn escape_ass_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '{' => out.push_str(r"\{"),
            '}' => out.push_str(r"\}"),
            '\n' => out.push_str(r"\N"),
            // A lone `\r` (old Mac line ending) is a line break too; a
            // CRLF pair must collapse to the SAME one break, not two, so
            // the trailing `\n` of a pair is consumed here rather than
            // matched again on the next iteration.
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str(r"\N");
            }
            _ => out.push(ch),
        }
    }
    out
}

fn num_or(v: Option<&Num>, default: f64) -> f64 {
    v.and_then(Num::as_f64).unwrap_or(default)
}

/// Round half away from zero to 2 decimals, never `-0` -- the same rule
/// `src/editor/cueGeometry.ts`'s `round2` uses, so a shared fixture value
/// computed by hand against that rule lands on the same float here.
fn round2(v: f64) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let sign = if v < 0.0 { -1.0 } else { 1.0 };
    let r = sign * (v.abs() * 100.0).round() / 100.0;
    if r == 0.0 {
        0.0
    } else {
        r
    }
}

// ---------------------------------------------------------------------
// The arrow: shared geometry with the preview (module doc).
// ---------------------------------------------------------------------

/// An arrow as two filled polygons, canvas px, 2-decimal rounded.
pub(super) struct ArrowPath {
    pub shaft: [(f64, f64); 4],
    pub head: [(f64, f64); 3],
}

/// The arrow from `(x, y)` to `(x2, y2)` (canvas fractions), `stroke`
/// canvas px thick, in canvas px -- the Rust twin of
/// `src/editor/cueGeometry.ts`'s `arrowPath` (module doc). `None` for a
/// zero-length arrow, which draws nothing.
pub(super) fn arrow_path(
    x: f64,
    y: f64,
    x2: f64,
    y2: f64,
    stroke: f64,
    canvas_w: f64,
    canvas_h: f64,
) -> Option<ArrowPath> {
    let (ax, ay) = (x * canvas_w, y * canvas_h);
    let (bx, by) = (x2 * canvas_w, y2 * canvas_h);
    let (dx, dy) = (bx - ax, by - ay);
    let len = (dx * dx + dy * dy).sqrt();
    if len == 0.0 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (nx, ny) = (-uy, ux);
    let mut head_len = 3.0 * stroke + 12.0;
    let mut half_width = 2.0 * stroke + 5.0;
    if len < head_len {
        half_width *= len / head_len;
        head_len = len;
    }
    let (base_x, base_y) = (bx - ux * head_len, by - uy * head_len);
    let s = stroke / 2.0;
    let pt = |px: f64, py: f64| (round2(px), round2(py));
    Some(ArrowPath {
        shaft: [
            pt(ax + nx * s, ay + ny * s),
            pt(base_x + nx * s, base_y + ny * s),
            pt(base_x - nx * s, base_y - ny * s),
            pt(ax - nx * s, ay - ny * s),
        ],
        head: [
            pt(bx, by),
            pt(base_x + nx * half_width, base_y + ny * half_width),
            pt(base_x - nx * half_width, base_y - ny * half_width),
        ],
    })
}

// ---------------------------------------------------------------------
// `\p1` drawing commands.
// ---------------------------------------------------------------------

fn polygon_cmd(points: &[(f64, f64)]) -> String {
    let mut it = points.iter();
    let Some(&(x0, y0)) = it.next() else {
        return String::new();
    };
    let mut s = format!("m {} {}", fmt_num(x0), fmt_num(y0));
    for &(x, y) in it {
        s.push_str(&format!(" l {} {}", fmt_num(x), fmt_num(y)));
    }
    s
}

/// A circle of radius `r` centred at `(cx, cy)`, canvas px, as four
/// cubic-Bezier quarter-arcs (module doc).
fn circle_cmd(cx: f64, cy: f64, r: f64) -> String {
    let k = r * BEZIER_KAPPA;
    let p = |x: f64, y: f64| format!("{} {}", fmt_num(x), fmt_num(y));
    format!(
        "m {} b {} {} {} b {} {} {} b {} {} {} b {} {} {}",
        p(cx + r, cy),
        p(cx + r, cy + k),
        p(cx + k, cy + r),
        p(cx, cy + r),
        p(cx - k, cy + r),
        p(cx - r, cy + k),
        p(cx - r, cy),
        p(cx - r, cy - k),
        p(cx - k, cy - r),
        p(cx, cy - r),
        p(cx + k, cy - r),
        p(cx + r, cy - k),
        p(cx + r, cy),
    )
}

// ---------------------------------------------------------------------
// One `Dialogue` line.
// ---------------------------------------------------------------------

fn dialogue(style: &str, start: u64, end: u64, tags: &str, text: &str) -> String {
    format!(
        "Dialogue: 0,{},{},{style},,0,0,0,,{{{tags}}}{text}",
        ass_time(start, false),
        ass_time(end, true),
    )
}

/// Like `dialogue`, but closes the `\p1` drawing scope with `{\p0}` so a
/// line that is ALL drawing (no trailing glyph text) still leaves the
/// event in text mode, defensively.
fn dialogue_drawing(style: &str, start: u64, end: u64, tags: &str, drawing: &str) -> String {
    format!(
        "Dialogue: 0,{},{},{style},,0,0,0,,{{{tags}}}{drawing}{{\\p0}}",
        ass_time(start, false),
        ass_time(end, true),
    )
}

fn fade_tag() -> String {
    format!("\\fad({FADE_MS},{FADE_MS})")
}

// ---------------------------------------------------------------------
// Per-kind cue dialogues.
// ---------------------------------------------------------------------

fn text_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    let e = &cue.effect;
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let x = e.x.as_f64().unwrap_or(0.0) * w;
    let y = e.y.as_f64().unwrap_or(0.0) * h;
    let size = num_or(e.font_size.as_ref(), TEXT_DEFAULT_FONT_SIZE);
    let colour = ass_colour(&e.color);
    let tags = format!(
        "\\an7\\pos({},{})\\1c{colour}\\fs{}{}",
        fmt_num(x),
        fmt_num(y),
        fmt_num(size),
        fade_tag()
    );
    let text = escape_ass_text(e.text.as_deref().unwrap_or(""));
    vec![dialogue(
        "Callout",
        cue.output_start,
        cue.output_end,
        &tags,
        &text,
    )]
}

fn effect_xy(e: &Effect) -> (f64, f64) {
    (e.x.as_f64().unwrap_or(0.0), e.y.as_f64().unwrap_or(0.0))
}

fn arrow_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    let e = &cue.effect;
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let (x, y) = effect_xy(e);
    let x2 = e.x2.as_ref().and_then(Num::as_f64).unwrap_or(x);
    let y2 = e.y2.as_ref().and_then(Num::as_f64).unwrap_or(y);
    let stroke = num_or(e.stroke.as_ref(), ARROW_DEFAULT_STROKE);
    let Some(path) = arrow_path(x, y, x2, y2, stroke, w, h) else {
        return Vec::new();
    };
    let drawing = format!("{} {}", polygon_cmd(&path.shaft), polygon_cmd(&path.head));
    let colour = ass_colour(&e.color);
    let tags = format!("\\an7\\pos(0,0)\\1c{colour}{}\\p1", fade_tag());
    vec![dialogue_drawing(
        "Callout",
        cue.output_start,
        cue.output_end,
        &tags,
        &drawing,
    )]
}

fn highlight_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    let e = &cue.effect;
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let (fx, fy) = effect_xy(e);
    let (x, y) = (fx * w, fy * h);
    let bw = num_or(e.w.as_ref(), HIGHLIGHT_DEFAULT_W) * w;
    let bh = num_or(e.h.as_ref(), HIGHLIGHT_DEFAULT_H) * h;
    let stroke = num_or(e.stroke.as_ref(), HIGHLIGHT_DEFAULT_STROKE)
        .min(bw / 2.0)
        .min(bh / 2.0)
        .max(0.0);
    let outer = [(x, y), (x + bw, y), (x + bw, y + bh), (x, y + bh)];
    let inner = [
        (x + stroke, y + stroke),
        (x + bw - stroke, y + stroke),
        (x + bw - stroke, y + bh - stroke),
        (x + stroke, y + bh - stroke),
    ];
    let drawing = format!("{} {}", polygon_cmd(&outer), polygon_cmd(&inner));
    let colour = ass_colour(&e.color);
    let tags = format!("\\an7\\pos(0,0)\\1c{colour}{}\\p1", fade_tag());
    vec![dialogue_drawing(
        "Callout",
        cue.output_start,
        cue.output_end,
        &tags,
        &drawing,
    )]
}

/// The four bands a spotlight dims around its box (canvas fractions) --
/// `src/editor/cueGeometry.ts`'s `spotlightRects`, same clamp order.
fn spotlight_rects(x: f64, y: f64, w: f64, h: f64) -> [(f64, f64, f64, f64); 4] {
    let top = x_clamp(y, 0.0, 1.0);
    let bottom = x_clamp(y + h, top, 1.0);
    let left = x_clamp(x, 0.0, 1.0);
    let right = x_clamp(x + w, left, 1.0);
    let mid_h = bottom - top;
    [
        (0.0, 0.0, 1.0, top),
        (0.0, bottom, 1.0, 1.0 - bottom),
        (0.0, top, left, mid_h),
        (right, top, 1.0 - right, mid_h),
    ]
}

fn x_clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.min(hi).max(lo)
}

fn spotlight_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    let e = &cue.effect;
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let (fx, fy) = effect_xy(e);
    let bw = num_or(e.w.as_ref(), HIGHLIGHT_DEFAULT_W);
    let bh = num_or(e.h.as_ref(), HIGHLIGHT_DEFAULT_H);
    let rects = spotlight_rects(fx, fy, bw, bh);
    let drawing = rects
        .iter()
        .filter(|r| r.2 > 0.0 && r.3 > 0.0)
        .map(|&(rx, ry, rw, rh)| {
            let (px, py, pw, ph) = (rx * w, ry * h, rw * w, rh * h);
            polygon_cmd(&[(px, py), (px + pw, py), (px + pw, py + ph), (px, py + ph)])
        })
        .collect::<Vec<_>>()
        .join(" ");
    if drawing.is_empty() {
        return Vec::new();
    }
    let dim = num_or(e.dim.as_ref(), SPOTLIGHT_DEFAULT_DIM).clamp(0.0, 1.0);
    let alpha = ((1.0 - dim) * 255.0).round() as u8;
    let tags = format!(
        "\\an7\\pos(0,0)\\1c&H000000&\\1a&H{alpha:02X}&{}\\p1",
        fade_tag()
    );
    vec![dialogue_drawing(
        "Callout",
        cue.output_start,
        cue.output_end,
        &tags,
        &drawing,
    )]
}

fn step_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    let e = &cue.effect;
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let (fx, fy) = effect_xy(e);
    let (cx, cy) = (fx * w, fy * h);
    let colour = ass_colour(&e.color);
    let circle_tags = format!("\\an7\\pos(0,0)\\1c{colour}{}\\p1", fade_tag());
    let circle = circle_cmd(cx, cy, STEP_RADIUS);
    let number = num_or(e.number.as_ref(), 0.0).round() as i64;
    let size = num_or(e.font_size.as_ref(), STEP_DEFAULT_FONT_SIZE);
    // The number is DELIBERATELY white, not `colour`: unlike every other
    // kind's text (which is the one thing drawn, so it takes the
    // author's chosen colour), this number sits ON TOP of a circle
    // that already carries `colour` -- same colour on same colour would
    // make the number unreadable regardless of what the author picked
    // (fix round 1, review Minor #4; pinned by
    // `step_number_colour_is_white_regardless_of_the_circles_own_colour`).
    let number_tags = format!(
        "\\an7\\pos({},{})\\1c&H00FFFFFF&\\fs{}{}",
        fmt_num(cx),
        fmt_num(cy),
        fmt_num(size),
        fade_tag()
    );
    vec![
        dialogue_drawing(
            "Step",
            cue.output_start,
            cue.output_end,
            &circle_tags,
            &circle,
        ),
        dialogue(
            "Step",
            cue.output_start,
            cue.output_end,
            &number_tags,
            &escape_ass_text(&number.to_string()),
        ),
    ]
}

fn mask_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    let e = &cue.effect;
    let (w, h) = (f64::from(canvas.width), f64::from(canvas.height));
    let (fx, fy) = effect_xy(e);
    let (x, y) = (fx * w, fy * h);
    let bw = num_or(e.w.as_ref(), 1.0) * w;
    let bh = num_or(e.h.as_ref(), 1.0) * h;
    let rect = polygon_cmd(&[(x, y), (x + bw, y), (x + bw, y + bh), (x, y + bh)]);
    let colour = ass_colour(&e.color);
    // A privacy cover must not fade in over the visible content it exists
    // to hide: explicit `\fad(0,0)`, never the cue default.
    let tags = format!("\\an7\\pos(0,0)\\1c{colour}\\1a&H00&\\fad(0,0)\\p1");
    vec![dialogue_drawing(
        "Callout",
        cue.output_start,
        cue.output_end,
        &tags,
        &rect,
    )]
}

fn cue_dialogues(cue: &PlannedCue, canvas: &Canvas) -> Vec<String> {
    match cue.kind {
        EffectKind::Zoom => Vec::new(),
        EffectKind::Text => text_dialogues(cue, canvas),
        EffectKind::Arrow => arrow_dialogues(cue, canvas),
        EffectKind::Highlight => highlight_dialogues(cue, canvas),
        EffectKind::Spotlight => spotlight_dialogues(cue, canvas),
        EffectKind::Step => step_dialogues(cue, canvas),
        EffectKind::Mask => mask_dialogues(cue, canvas),
    }
}

// ---------------------------------------------------------------------
// Card title/subtitle text (F-37).
// ---------------------------------------------------------------------

/// One card text line: `\pos` at `(x, y)`, `align` (`ALIGN_MID_CENTER` for
/// a line centred alone, `ALIGN_TOP_CENTER` for a line stacked with the
/// other), the given colour and font size.
#[allow(clippy::too_many_arguments)]
fn card_line(
    style: &str,
    card: &PlannedCard,
    x: f64,
    y: f64,
    align: u8,
    colour: &str,
    size: f64,
    text: &str,
) -> String {
    let tags = format!(
        "\\an{align}\\pos({},{})\\1c{colour}\\fs{}{}",
        fmt_num(x),
        fmt_num(y),
        fmt_num(size),
        fade_tag()
    );
    dialogue(
        style,
        card.output_start,
        card.output_end,
        &tags,
        &escape_ass_text(text),
    )
}

/// Title/subtitle text, matching the ALREADY-SHIPPED preview's own layout
/// (`src/editor/previewCardDom.ts`, Task 33): a flex column, both lines
/// centred horizontally AND, as a whole stack, vertically within the
/// card's box -- never top-left-anchored, and no space is reserved for a
/// title that is not there. Colours mirror the preview exactly: the
/// title in `foreground`, the subtitle in `accent` (`previewCardDom.ts`'s
/// `title.style.color`/`subtitle.style.color`) -- NOT the same colour for
/// both. The per-line height used to stack the two lines is estimated
/// (`CARD_LINE_HEIGHT`), since this module reads no font file the way the
/// preview's real DOM layout does; recorded as a remaining approximation
/// in `docs/Gaps.md` GAP-173.
fn card_dialogues(card: &PlannedCard, _canvas: &Canvas) -> Vec<String> {
    let Some(c) = card.card.as_ref() else {
        return Vec::new();
    };
    let has_title = !c.title.is_empty();
    let has_subtitle = !c.subtitle.is_empty();
    if !has_title && !has_subtitle {
        return Vec::new();
    }
    let (bx, by, bw, bh) = (
        f64::from(card.bounds.x),
        f64::from(card.bounds.y),
        f64::from(card.bounds.w),
        f64::from(card.bounds.h),
    );
    let (cx, cy) = (bx + bw / 2.0, by + bh / 2.0);
    let fg = ass_colour(&c.foreground);
    let accent = ass_colour(&c.accent);
    let title_h = CARD_TITLE_FONT * CARD_LINE_HEIGHT;
    let subtitle_h = CARD_SUBTITLE_FONT * CARD_LINE_HEIGHT;

    if has_title && has_subtitle {
        let top = cy - (title_h + CARD_LINE_GAP + subtitle_h) / 2.0;
        vec![
            card_line(
                "CardTitle",
                card,
                cx,
                top,
                ALIGN_TOP_CENTER,
                &fg,
                CARD_TITLE_FONT,
                &c.title,
            ),
            card_line(
                "CardSubtitle",
                card,
                cx,
                top + title_h + CARD_LINE_GAP,
                ALIGN_TOP_CENTER,
                &accent,
                CARD_SUBTITLE_FONT,
                &c.subtitle,
            ),
        ]
    } else if has_title {
        vec![card_line(
            "CardTitle",
            card,
            cx,
            cy,
            ALIGN_MID_CENTER,
            &fg,
            CARD_TITLE_FONT,
            &c.title,
        )]
    } else {
        vec![card_line(
            "CardSubtitle",
            card,
            cx,
            cy,
            ALIGN_MID_CENTER,
            &accent,
            CARD_SUBTITLE_FONT,
            &c.subtitle,
        )]
    }
}

// ---------------------------------------------------------------------
// The document: `[Script Info]` + `[V4+ Styles]` + `[Events]`.
// ---------------------------------------------------------------------

fn style_line(name: &str, size: f64, alignment: u8) -> String {
    format!(
        "Style: {name},{FONT},{},&H00FFFFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,{alignment},10,10,10,1",
        fmt_num(size),
    )
}

/// The `Caption` style's own line: its `BorderStyle`/`Alignment` are the
/// only pieces of the whole style block that depend on the plan's own
/// caption settings (`captions_at_bottom_use_the_bottom_alignment`) --
/// every other style is a fixed constant.
pub(super) fn caption_style_line(settings: Option<&PlannedCaptions>) -> String {
    let (size, alignment, border_style, back) = match settings {
        Some(c) => (
            c.font_size,
            match c.position {
                CaptionPosition::Top => 8,
                CaptionPosition::Bottom => 2,
            },
            if c.background { 3 } else { 1 },
            if c.background {
                "&H80000000"
            } else {
                "&H00000000"
            },
        ),
        None => (CAPTION_DEFAULT_FONT_SIZE, 2, 1, "&H00000000"),
    };
    format!(
        "Style: Caption,{FONT},{},&H00FFFFFF,&H000000FF,&H00000000,{back},0,0,0,0,100,100,0,0,{border_style},2,0,{alignment},10,10,20,1",
        fmt_num(size),
    )
}

fn style_block(settings: Option<&PlannedCaptions>) -> String {
    let mut out = String::from("[V4+ Styles]\n");
    out.push_str(
        "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, \
         BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, \
         BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\n",
    );
    out.push_str(&style_line("Callout", TEXT_DEFAULT_FONT_SIZE, 7));
    out.push('\n');
    out.push_str(&style_line("Step", STEP_DEFAULT_FONT_SIZE, 7));
    out.push('\n');
    out.push_str(&style_line("CardTitle", CARD_TITLE_FONT, 7));
    out.push('\n');
    out.push_str(&style_line("CardSubtitle", CARD_SUBTITLE_FONT, 7));
    out.push('\n');
    out.push_str(&caption_style_line(settings));
    out.push('\n');
    out
}

fn document(canvas: &Canvas, events: &[String], captions: Option<&PlannedCaptions>) -> String {
    let mut out = String::new();
    out.push_str("[Script Info]\n");
    out.push_str("; Vault Buddy tutorial-editor render (Task 43)\n");
    out.push_str("; Fonts: Segoe UI (primary), Arial (fallback if Segoe UI is unavailable)\n");
    out.push_str("ScriptType: v4.00+\n");
    out.push_str(&format!("PlayResX: {}\n", canvas.width));
    out.push_str(&format!("PlayResY: {}\n", canvas.height));
    out.push_str("WrapStyle: 2\n");
    out.push_str("ScaledBorderAndShadow: yes\n\n");
    out.push_str(&style_block(captions));
    out.push_str("\n[Events]\n");
    out.push_str(
        "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n",
    );
    for line in events {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Teaching cues (every kind but `zoom`, which is a camera move, not a
/// drawing -- `video_graph::zoom_filter`'s job) and card title/subtitle
/// text (F-37), burned at the PRE-zoom hook. `None` when the render has
/// neither -- the hook is then a `null` filter, never an empty `ass=`.
pub fn build_cue_ass(plan: &RenderPlan) -> Option<String> {
    let mut events = Vec::new();
    for cue in &plan.cues {
        events.extend(cue_dialogues(cue, &plan.canvas));
    }
    for card in &plan.cards {
        events.extend(card_dialogues(card, &plan.canvas));
    }
    if events.is_empty() {
        return None;
    }
    Some(document(&plan.canvas, &events, None))
}

/// Burned-in captions (F-35), at the POST-zoom hook. `None` when captions
/// are off, not set to burn in, or land no cue in the output --
/// `render_plan::plan` already applies all three (`captions_of`), so this
/// is a direct mirror of `plan.captions.is_some()`.
pub fn build_caption_ass(plan: &RenderPlan) -> Option<String> {
    let settings = plan.captions.as_ref()?;
    if settings.cues.is_empty() {
        return None;
    }
    let events: Vec<String> = settings.cues.iter().map(caption_dialogue).collect();
    Some(document(&plan.canvas, &events, Some(settings)))
}

fn caption_dialogue(cue: &PlannedCaption) -> String {
    dialogue(
        "Caption",
        cue.output_start,
        cue.output_end,
        "",
        &escape_ass_text(&cue.text),
    )
}

#[cfg(test)]
#[path = "ass_tests.rs"]
mod ass_tests;
