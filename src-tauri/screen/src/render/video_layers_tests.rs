//! Golden strings for one visual item's chain (Task 42). Every fixture is
//! asymmetric (a 244x242 box, a source starting at 1 s, a non-unit speed)
//! so a swapped axis, a dropped trim start or a missing speed divide fails.

use super::test_support::{card, layer, PIP};
use super::video_layers::{card_chain, card_source, media_layer_chain, Placement};
use vault_buddy_core::editor::model::{Fit, Rotation};
use vault_buddy_core::editor::render_plan::{Crop, Cut};

fn chain_of(l: &vault_buddy_core::editor::render_plan::VideoLayer) -> String {
    media_layer_chain(l, 30, Placement::OnCanvas)
}

fn position(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} missing from {haystack}"))
}

// Cover FILLS the box (scale up, crop the overflow); contain FITS inside it
// (scale down, pad transparent). Swapping the two ratios renders a picture
// that is either letterboxed where the user asked for fill or cut off where
// they asked to see all of it -- and both are valid ffmpeg.
#[test]
fn cover_scales_up_and_crops_contain_pads() {
    let mut l = layer(0, 0, 1_000, 4_000, PIP);
    l.fit = Fit::Cover;
    let cover = chain_of(&l);
    assert!(
        cover.contains(",scale=244:242:force_original_aspect_ratio=increase,crop=244:242,"),
        "{cover}"
    );
    assert!(!cover.contains("pad="), "cover never pads: {cover}");

    l.fit = Fit::Contain;
    let contain = chain_of(&l);
    assert!(
        contain.contains(
            ",scale=244:242:force_original_aspect_ratio=decrease,format=rgba,\
             pad=244:242:(ow-iw)/2:(oh-ih)/2:color=black@0,"
        ),
        "{contain}"
    );
    assert!(!contain.contains("increase"), "{contain}");

    // A cover crop zoom is the PREVIEW's window (previewTransform.ts'
    // drawnRect): the box's own aspect cut from the source, shrunk by the
    // zoom, centred on the anchor and clamped inside the source -- then
    // scaled to fill. Anchor x 0.25 / y 0.75 so a swapped axis fails.
    l.fit = Fit::Cover;
    l.crop = Some(Crop {
        zoom: 2.0,
        x: 0.25,
        y: 0.75,
    });
    let zoomed = chain_of(&l);
    assert!(
        zoomed.contains(
            ",crop=w='min(iw,ih*244/242)/2':h='min(ih,iw*242/244)/2':\
             x='clip(0.25*iw-ow/2,0,iw-ow)':y='clip(0.75*ih-oh/2,0,ih-oh)',scale=244:242,"
        ),
        "{zoomed}"
    );
}

// CSS rotate(90deg) -- what the preview draws -- is CLOCKWISE, and
// ffmpeg's transpose=1 is "rotate by 90 degrees clockwise"; transpose=2 is
// counter-clockwise. The two differ only in direction, so a picture turned
// the wrong way is still a perfectly valid file.
#[test]
fn rotation_ninety_uses_transpose_one() {
    let mut l = layer(0, 0, 1_000, 4_000, PIP);
    l.rotation = Rotation::Deg90;
    let ninety = chain_of(&l);
    assert!(ninety.contains(",transpose=1,"), "{ninety}");
    assert!(!ninety.contains("transpose=2"), "{ninety}");
    // The turn happens BEFORE the fit, so the box is fitted with the
    // turned source's swapped axes.
    assert!(position(&ninety, "transpose=1") < position(&ninety, "scale="));

    l.rotation = Rotation::Deg270;
    let two_seventy = chain_of(&l);
    assert!(two_seventy.contains(",transpose=2,"), "{two_seventy}");
    assert!(!two_seventy.contains("transpose=1"), "{two_seventy}");

    l.rotation = Rotation::Deg180;
    let half = chain_of(&l);
    assert!(half.contains(",hflip,vflip,"), "{half}");
    assert!(!half.contains("transpose"), "{half}");
}

// The preview (previewTransform.ts) flips the SOURCE vertically before it
// turns it, and mirrors the finished FRAME horizontally -- so vflip sits
// before the fit and hflip after it. A mirror applied to the source instead
// of the frame would mirror a cover crop's WINDOW the wrong way.
#[test]
fn mirror_and_flip_map_to_hflip_vflip() {
    let mut l = layer(0, 0, 1_000, 4_000, PIP);
    l.mirror = true;
    let mirrored = chain_of(&l);
    assert_eq!(mirrored.matches("hflip").count(), 1, "{mirrored}");
    assert!(!mirrored.contains("vflip"), "{mirrored}");
    assert!(position(&mirrored, "hflip") > position(&mirrored, "pad="));

    l.mirror = false;
    l.flip_y = true;
    l.rotation = Rotation::Deg90;
    let flipped = chain_of(&l);
    assert_eq!(flipped.matches("vflip").count(), 1, "{flipped}");
    assert!(!flipped.contains("hflip"), "{flipped}");
    assert!(position(&flipped, "vflip") < position(&flipped, "transpose=1"));
    assert!(position(&flipped, "vflip") < position(&flipped, "scale="));
}

// A 2x clip plays 6 s of source in 3 s of output. Rebasing the trimmed
// source to zero and then dividing by the speed is the whole mapping; a
// chain that multiplied instead (or forgot) would still render, at the
// wrong length, with the next layer landing on the wrong frame.
#[test]
fn speed_two_divides_pts() {
    let mut l = layer(0, 0, 500, 3_500, PIP);
    l.speed = 2.0;
    l.source_in = 1_000;
    l.source_out = 7_000;
    assert_eq!(
        chain_of(&l),
        "[0:v]trim=start=1.000:end=7.000,setpts=(PTS-STARTPTS)/2,fps=30,\
         scale=244:242:force_original_aspect_ratio=decrease,format=rgba,\
         pad=244:242:(ow-iw)/2:(oh-ih)/2:color=black@0,setpts=PTS+0.500/TB"
    );
    // ...and 1x does not divide at all.
    l.speed = 1.0;
    assert!(chain_of(&l).contains(",setpts=PTS-STARTPTS,fps=30,"));
}

// DATA-MODEL.md: "Edge fade envelopes multiply the clip's ALPHA". A video
// `fade` without alpha=1 fades the picture to BLACK, which over a lower
// layer is a black flash instead of a see-through dissolve into it.
#[test]
fn fade_is_alpha_not_black() {
    let mut l = layer(0, 0, 1_000, 4_000, PIP);
    l.fade_in = 500;
    l.fade_out = 750;
    l.opacity = 0.8;
    let chain = chain_of(&l);
    assert!(
        chain.contains(
            ",format=rgba,colorchannelmixer=aa=0.8,fade=t=in:st=0.000:d=0.500:alpha=1,\
             fade=t=out:st=2.250:d=0.750:alpha=1,setpts=PTS+1.000/TB"
        ),
        "{chain}"
    );
    assert_eq!(
        chain.matches("fade=t=").count(),
        chain.matches(":alpha=1").count(),
        "every fade is an alpha fade: {chain}"
    );
}

// A13: a range that starts 200 ms into a clip keeps the fade where it
// really is. The clip's local clock is started at 0.2 s (its ORIGINAL
// start is 0), so the fade-in is already 40 % in on the first frame, and
// the placement subtracts the same 0.2 s back out.
#[test]
fn a_range_cut_fade_is_evaluated_where_it_really_is() {
    let mut l = layer(0, 0, 0, 3_000, PIP);
    l.fade_in = 500;
    l.cut = Cut {
        head_ms: 200,
        tail_ms: 0,
    };
    let chain = chain_of(&l);
    assert!(
        chain.contains(",setpts=PTS-STARTPTS+0.200/TB,fps=30,"),
        "{chain}"
    );
    assert!(
        chain.contains("fade=t=in:st=0.000:d=0.500:alpha=1"),
        "{chain}"
    );
    assert!(chain.ends_with(",setpts=PTS-0.200/TB"), "{chain}");
    // A fade the range cut off entirely is not emitted at all.
    l.cut.head_ms = 600;
    assert!(!chain_of(&l).contains("fade=t=in"));
    // The fade-OUT is measured from the ORIGINAL end: 200 ms of this clip
    // lie past the range, so its 0.5 s fade starts at 3.2 - 0.5 = 2.7 s of
    // its own clock, not at 2.5.
    let mut tail = layer(0, 0, 0, 3_000, PIP);
    tail.fade_out = 500;
    tail.cut = Cut {
        head_ms: 0,
        tail_ms: 200,
    };
    let chain = chain_of(&tail);
    assert!(
        chain.contains("fade=t=out:st=2.700:d=0.500:alpha=1"),
        "{chain}"
    );
}

// Cards have no file (Task 43 draws their text); the picture is a colour
// source of the card's background at the box's size.
#[test]
fn a_card_is_a_color_source_sized_to_its_box() {
    let c = card(0, 1_000, 3_500, "#1a2B3c");
    assert_eq!(
        card_source(&c, 30),
        "color=c=0x1a2B3c:s=244x242:r=30:d=2.500"
    );
    assert_eq!(
        card_chain(&c, 3, Placement::OnCanvas),
        "[3:v]format=rgba,setpts=PTS-STARTPTS,setpts=PTS+1.000/TB"
    );
    // The background reaches a filtergraph, so anything but #rrggbb is
    // refused rather than escaped: an unparseable colour is black.
    let hostile = card(0, 1_000, 3_500, "red:s=9x9,evil");
    assert_eq!(
        card_source(&hostile, 30),
        "color=c=black:s=244x242:r=30:d=2.500"
    );
}
