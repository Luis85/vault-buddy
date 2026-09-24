//! One visual item's filter chain (tutorial-editor Task 42; F-16, F-17,
//! F-23, F-37, F-39). PURE: a `VideoLayer` (or a `PlannedCard`) in, the
//! comma-joined filters that turn its source into a positioned, timed,
//! alpha-carrying picture out. `video_graph` wires the chains together.
//!
//! **The order is the preview's** (`src/editor/previewTransform.ts`, the
//! authority for layout -- GAP-173): the SOURCE is flipped vertically, then
//! turned; its colour is adjusted while it is still opaque; it is fitted
//! into the box (`contain` scales down and pads transparent, `cover` cuts a
//! window of the box's own aspect -- shrunk by the crop zoom and centred on
//! the anchor, clamped inside the source -- and scales it to fill); the
//! finished FRAME is mirrored; the frame shape masks it; opacity and the
//! fades multiply its alpha; and it is moved to its output time.
//!
//! **Colour before any alpha.** `eq` works on YUV, so running it after the
//! transparent `pad` would convert the picture back to an opaque format and
//! turn a contain letterbox black.
//!
//! **Every clip has its own clock, started at its ORIGINAL start** (A13).
//! A range that cut `head_ms` off a clip starts that clock at `head_ms`
//! rather than 0, so a fade or a ramp measured from the clip's real start
//! is evaluated where it really is, and the placement subtracts the same
//! amount back out. ffmpeg's `fade` cannot start before 0, which is why the
//! clock moves rather than the fade.
//!
//! **Fades are ALPHA fades** (`alpha=1`). DATA-MODEL.md: "Edge fade
//! envelopes multiply the clip's alpha"; a plain video `fade` fades the
//! picture to BLACK, which over a lower layer is a black flash. ffmpeg's
//! video fade is linear whatever the clip's `fade_curve` -- the curve is
//! honoured on the audio side (Task 44: linear -> tri, smooth -> hsin,
//! equal-power -> qsin); GAP-173 records the video approximation.

use vault_buddy_core::editor::model::{Adjustments, Fit, FrameShape, Rotation};
use vault_buddy_core::editor::render_plan::{Crop, Cut, PixelBox, PlannedCard, VideoLayer};
use vault_buddy_core::editor::Num;

use super::expr::{num, plus_seconds, seconds};

/// Where a finished chain goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Overlaid alone at its own box: its timestamps are moved to its
    /// output start.
    OnCanvas,
    /// A member of a transition pair (`video_graph`): rebased to zero and
    /// padded onto a transparent canvas at its box, so two members with
    /// different boxes can still be blended by `xfade`.
    InGroup { canvas_w: u32, canvas_h: u32 },
}

/// `b` with every edge rounded DOWN to an even pixel, and never narrower
/// than 2 -- the plan already rounds to even (Task 41), this is the last
/// line of defence before the encoder's chroma subsampling.
pub fn even_box(b: PixelBox) -> PixelBox {
    PixelBox {
        x: b.x & !1,
        y: b.y & !1,
        w: (b.w & !1).max(2),
        h: (b.h & !1).max(2),
    }
}

/// An item's placement on the output timeline.
#[derive(Debug, Clone, Copy)]
pub(super) struct Timing {
    pub start: u64,
    pub end: u64,
    pub cut: Cut,
}

impl Timing {
    fn head(&self) -> i64 {
        self.cut.head_ms as i64
    }

    /// The item's whole ORIGINAL length, what a fade-out is measured from.
    fn original_len(&self) -> u64 {
        self.cut.head_ms + (self.end - self.start) + self.cut.tail_ms
    }
}

/// The visual properties every item shares, card or media.
#[derive(Debug, Clone, Copy)]
pub(super) struct Surface {
    pub bounds: PixelBox,
    pub opacity: f64,
    pub fade_in: u64,
    pub fade_out: u64,
    pub timing: Timing,
}

/// A media layer's chain from `[input:v]`, without an output label.
pub fn media_layer_chain(layer: &VideoLayer, fps: u32, placement: Placement) -> String {
    let surface = Surface {
        bounds: even_box(layer.bounds),
        opacity: layer.opacity,
        fade_in: layer.fade_in,
        fade_out: layer.fade_out,
        timing: Timing {
            start: layer.output_start,
            end: layer.output_end,
            cut: layer.cut,
        },
    };
    let b = surface.bounds;
    let mut f = vec![
        format!(
            "trim=start={}:end={}",
            seconds(layer.source_in),
            seconds(layer.source_out)
        ),
        format!("setpts={}", media_clock(layer.speed, layer.cut.head_ms)),
        // One frame clock for every layer: a variable-rate capture would
        // otherwise reach `xfade`, which refuses inputs whose rates differ.
        format!("fps={fps}"),
    ];
    f.extend(orientation(layer.rotation, layer.flip_y));
    f.extend(adjustments(layer.adjustments.as_ref()));
    f.extend(fit(layer.fit, layer.crop, b));
    if layer.mirror {
        f.push("hflip".into());
    }
    f.extend(mask(layer.frame_shape, b));
    f.extend(alpha(&surface));
    f.extend(place(&surface, placement));
    format!("[{}:v]{}", layer.input, f.join(","))
}

/// A card's lavfi `color` source: its background at its box's size, for
/// its placed length.
///
/// The background reaches a filtergraph, so it is accepted only as the
/// `#rrggbb` the model documents -- anything else is black, never escaped
/// and passed through.
pub fn card_source(card: &PlannedCard, fps: u32) -> String {
    let b = even_box(card.bounds);
    let colour = card
        .card
        .as_ref()
        .and_then(|c| hex_colour(&c.background))
        .unwrap_or_else(|| "black".into());
    format!(
        "color=c={colour}:s={}x{}:r={fps}:d={}",
        b.w,
        b.h,
        seconds(card.output_end - card.output_start)
    )
}

/// A card's chain from its own lavfi input `[input:v]`, without an output
/// label. Its title and subtitle are Task 43's (the pre-zoom ASS hook).
pub fn card_chain(card: &PlannedCard, input: usize, placement: Placement) -> String {
    let surface = Surface {
        bounds: even_box(card.bounds),
        opacity: card.opacity,
        fade_in: card.fade_in,
        fade_out: card.fade_out,
        timing: Timing {
            start: card.output_start,
            end: card.output_end,
            cut: card.cut,
        },
    };
    let mut f = vec![
        "format=rgba".to_string(),
        format!("setpts={}", media_clock(1.0, card.cut.head_ms)),
    ];
    f.extend(alpha(&surface));
    f.extend(place(&surface, placement));
    format!("[{input}:v]{}", f.join(","))
}

fn hex_colour(s: &str) -> Option<String> {
    let hex = s.strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("0x{hex}"))
}

/// The clip's clock: rebased to zero, divided by its speed, started at its
/// original start (the module doc).
fn media_clock(speed: f64, head_ms: u64) -> String {
    let base = if speed == 1.0 {
        "PTS-STARTPTS".to_string()
    } else {
        format!("(PTS-STARTPTS)/{}", num(speed))
    };
    if head_ms == 0 {
        base
    } else {
        format!("{base}{}/TB", plus_seconds(head_ms as i64))
    }
}

/// Flip, then turn -- the preview's order. CSS `rotate(90deg)` is
/// clockwise, as is `transpose=1`.
fn orientation(rotation: Rotation, flip_y: bool) -> Vec<String> {
    let mut f = Vec::new();
    if flip_y {
        f.push("vflip".to_string());
    }
    match rotation {
        Rotation::Deg0 => {}
        Rotation::Deg90 => f.push("transpose=1".into()),
        Rotation::Deg180 => f.extend(["hflip".to_string(), "vflip".to_string()]),
        Rotation::Deg270 => f.push("transpose=2".into()),
    }
    f
}

fn value(n: &Num, default: f64) -> f64 {
    n.as_f64().unwrap_or(default)
}

/// Brightness / contrast / saturation through `eq`, then sepia and
/// grayscale as the W3C Filter Effects colour matrices the preview's CSS
/// `sepia()` / `grayscale()` are defined by, each blended toward identity
/// by its amount -- in CSS's own order.
///
/// `eq`'s brightness is ADDITIVE where CSS `brightness()` multiplies, so
/// `b - 1` is an approximation of the preview, not an equivalence (GAP-173).
fn adjustments(adjustments: Option<&Adjustments>) -> Vec<String> {
    let Some(a) = adjustments else {
        return Vec::new();
    };
    let (b, c, s) = (
        value(&a.brightness, 1.0),
        value(&a.contrast, 1.0),
        value(&a.saturation, 1.0),
    );
    let mut f = Vec::new();
    if b != 1.0 || c != 1.0 || s != 1.0 {
        f.push(format!(
            "eq=brightness={}:contrast={}:saturation={}",
            num((b - 1.0).clamp(-1.0, 1.0)),
            num(c.clamp(-1000.0, 1000.0)),
            num(s.clamp(0.0, 3.0))
        ));
    }
    let sepia = value(&a.sepia, 0.0).clamp(0.0, 1.0);
    if sepia > 0.0 {
        let k = 1.0 - sepia;
        f.push(mixer([
            0.393 + 0.607 * k,
            0.769 - 0.769 * k,
            0.189 - 0.189 * k,
            0.349 - 0.349 * k,
            0.686 + 0.314 * k,
            0.168 - 0.168 * k,
            0.272 - 0.272 * k,
            0.534 - 0.534 * k,
            0.131 + 0.869 * k,
        ]));
    }
    let gray = value(&a.grayscale, 0.0).clamp(0.0, 1.0);
    if gray > 0.0 {
        let k = 1.0 - gray;
        f.push(mixer([
            0.2126 + 0.7874 * k,
            0.7152 - 0.7152 * k,
            0.0722 - 0.0722 * k,
            0.2126 - 0.2126 * k,
            0.7152 + 0.2848 * k,
            0.0722 - 0.0722 * k,
            0.2126 - 0.2126 * k,
            0.7152 - 0.7152 * k,
            0.0722 + 0.9278 * k,
        ]));
    }
    f
}

fn mixer(m: [f64; 9]) -> String {
    const KEYS: [&str; 9] = ["rr", "rg", "rb", "gr", "gg", "gb", "br", "bg", "bb"];
    let pairs: Vec<String> = KEYS
        .iter()
        .zip(m)
        .map(|(k, v)| format!("{k}={}", num(v)))
        .collect();
    format!("colorchannelmixer={}", pairs.join(":"))
}

/// The turned source into the box (see the module doc for the rules).
///
/// A crop zoom is honoured under `cover` only: the preview's `contain`
/// always shows the whole turned source (`previewTransform.drawnRect`).
fn fit(fit: Fit, crop: Option<Crop>, b: PixelBox) -> Vec<String> {
    let (w, h) = (b.w, b.h);
    match (fit, crop) {
        (Fit::Contain, _) => vec![
            format!("scale={w}:{h}:force_original_aspect_ratio=decrease"),
            "format=rgba".into(),
            format!("pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:color=black@0"),
        ],
        (Fit::Cover, None) => vec![
            format!("scale={w}:{h}:force_original_aspect_ratio=increase"),
            format!("crop={w}:{h}"),
        ],
        (Fit::Cover, Some(c)) => vec![
            format!(
                "crop=w='min(iw,ih*{w}/{h})/{z}':h='min(ih,iw*{h}/{w})/{z}':\
                 x='clip({cx}*iw-ow/2,0,iw-ow)':y='clip({cy}*ih-oh/2,0,ih-oh)'",
                z = num(c.zoom),
                cx = num(c.x),
                cy = num(c.y),
            ),
            format!("scale={w}:{h}"),
        ],
    }
}

/// The frame shape as an alpha mask, anti-aliased over one pixel: a
/// circle is the ellipse inscribed in the box (CSS `border-radius: 50%`),
/// a rounded frame has the preview's 7.5 %-of-the-short-side corners.
fn mask(shape: FrameShape, b: PixelBox) -> Vec<String> {
    let coverage = match shape {
        FrameShape::Rectangle => return Vec::new(),
        FrameShape::Circle => {
            "clip((1-hypot((X+0.5)/(W/2)-1,(Y+0.5)/(H/2)-1))*min(W,H)/2+0.5,0,1)".to_string()
        }
        FrameShape::Rounded => {
            let r = num(0.075 * f64::from(b.w.min(b.h)));
            format!(
                "clip({r}+0.5-hypot(max(max({r}-X-0.5,X+0.5-W+{r}),0),\
                 max(max({r}-Y-0.5,Y+0.5-H+{r}),0)),0,1)"
            )
        }
    };
    vec![
        "format=rgba".into(),
        format!("geq=r='r(X,Y)':g='g(X,Y)':b='b(X,Y)':a='alpha(X,Y)*{coverage}'"),
    ]
}

/// Opacity and the edge fades, all on the alpha channel. A fade the range
/// cut off entirely is not emitted.
fn alpha(s: &Surface) -> Vec<String> {
    let mut f = Vec::new();
    let opacity = s.opacity.clamp(0.0, 1.0);
    if opacity < 1.0 {
        f.push(format!("colorchannelmixer=aa={}", num(opacity)));
    }
    if s.fade_in > s.timing.cut.head_ms {
        f.push(format!(
            "fade=t=in:st=0.000:d={}:alpha=1",
            seconds(s.fade_in)
        ));
    }
    if s.fade_out > s.timing.cut.tail_ms {
        f.push(format!(
            "fade=t=out:st={}:d={}:alpha=1",
            seconds(s.timing.original_len().saturating_sub(s.fade_out)),
            seconds(s.fade_out)
        ));
    }
    if !f.is_empty() {
        f.insert(0, "format=rgba".into());
    }
    f
}

/// Move the finished picture to where the graph wants it.
fn place(s: &Surface, placement: Placement) -> Vec<String> {
    match placement {
        Placement::OnCanvas => vec![format!(
            "setpts=PTS{}/TB",
            plus_seconds(s.timing.start as i64 - s.timing.head())
        )],
        Placement::InGroup { canvas_w, canvas_h } => vec![
            "setpts=PTS-STARTPTS".into(),
            "format=rgba".into(),
            format!(
                "pad={canvas_w}:{canvas_h}:{}:{}:color=black@0",
                s.bounds.x, s.bounds.y
            ),
        ],
    }
}
