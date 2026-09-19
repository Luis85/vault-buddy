//! BGRA -> NV12, the one per-frame cost in the capture path.
//!
//! `windows-capture` delivers 8-bit BGRA; every hardware H.264 encoder MFT
//! wants NV12. Feeding the encoder BGRA works only where Windows happens to
//! insert a software colour converter, which is the kind of machine-specific
//! behaviour the dropped-frame counter would report and nobody could explain.
//!
//! This is pure arithmetic over two byte slices, so it is fully unit-tested
//! on Linux. Given no CI runner can record a screen, everything that CAN be
//! proven without one is proven here.
//!
//! COLOUR SPACE: BT.709, LIMITED range (luma 16-235, chroma centred on 128).
//! Screen captures are HD or larger and BT.709 is what every HD decoder
//! assumes; using BT.601 while the decoder assumes BT.709 is the classic
//! slightly-wrong-colours bug. `sink.rs` declares the same matrix on its
//! NV12 input type, so the encoder is told what this produced rather than
//! guessing from the frame size.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertError {
    /// The source buffer is smaller than `stride * height`. Converting it
    /// anyway would read past the end and ship unrelated memory into the
    /// recording.
    ShortInput { needed: usize, got: usize },
    /// NV12 subsamples chroma 2x2, so an odd dimension has half a chroma
    /// sample. Callers round down once via `even_dims` at capture start.
    OddDimensions,
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConvertError::ShortInput { needed, got } => {
                write!(
                    f,
                    "frame buffer too small: needed {needed} bytes, got {got}"
                )
            }
            ConvertError::OddDimensions => write!(f, "NV12 requires even dimensions"),
        }
    }
}

/// NV12 is 1.5 bytes per pixel: a full-size luma plane plus a half-size
/// interleaved chroma plane.
pub fn nv12_len(width: u32, height: u32) -> usize {
    let y = width as usize * height as usize;
    y + y / 2
}

/// Round a frame size down to even. DOWN, never up: rounding up would index
/// past the end of the source frame. At most one row and one column are
/// lost, deliberately, once, at capture start.
pub fn even_dims(width: u32, height: u32) -> (u32, u32) {
    (width & !1, height & !1)
}

/// BT.709 limited-range luma, 16..=235.
#[inline]
fn luma709(r: f32, g: f32, b: f32) -> u8 {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    (16.0 + y * (219.0 / 255.0)).round().clamp(16.0, 235.0) as u8
}

/// BT.709 limited-range chroma pair, 16..=240, centred on 128.
#[inline]
fn chroma709(r: f32, g: f32, b: f32) -> (u8, u8) {
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let u = 128.0 + ((b - y) / 1.8556) * (224.0 / 255.0);
    let v = 128.0 + ((r - y) / 1.5748) * (224.0 / 255.0);
    (
        u.round().clamp(16.0, 240.0) as u8,
        v.round().clamp(16.0, 240.0) as u8,
    )
}

/// Convert one BGRA frame into `out`, reusing its allocation.
///
/// `stride` is the source's BYTES PER ROW, which for a D3D11 staging texture
/// is usually wider than `width * 4`. Reading the buffer as tightly packed
/// shears the image progressively down the frame -- a bug that presents as a
/// capture glitch rather than as a stride mistake.
///
/// This is the whole-frame (top-left) case of `bgra_crop_to_nv12`, kept as
/// its own entry point because every non-region capture uses it and its
/// behaviour must not drift: it delegates rather than duplicating, so there
/// is one conversion, not two that can diverge.
pub fn bgra_to_nv12(
    bgra: &[u8],
    stride: usize,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), ConvertError> {
    bgra_crop_to_nv12(bgra, stride, 0, 0, width, height, out)
}

/// Convert the `width` x `height` rectangle at `(src_x, src_y)` of a BGRA
/// frame into NV12 in `out`.
///
/// This is how REGION capture works (spec 5.2): WGC hands us the whole
/// monitor and the region is a window onto it, so the crop happens here,
/// on the CPU, in a pure function -- which is the only reason any of
/// region capture's correctness is provable on Linux (docs/Gaps.md
/// GAP-117). Cropping on the GPU before readback would be cheaper at 4K60
/// (spec 17.2) and is deliberately not done: it would move this logic
/// somewhere nothing can test it.
///
/// `src_x` / `src_y` are NOT required to be even. The 2x2 chroma blocks are
/// averaged from the full-colour BGRA source *inside* the crop, so an odd
/// origin still yields a self-consistent NV12 frame; rounding the origin to
/// even would silently move the rectangle the user drew.
pub fn bgra_crop_to_nv12(
    bgra: &[u8],
    stride: usize,
    src_x: u32,
    src_y: u32,
    width: u32,
    height: u32,
    out: &mut Vec<u8>,
) -> Result<(), ConvertError> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err(ConvertError::OddDimensions);
    }
    // The crop's RIGHT edge must lie inside one row. A row that ends short
    // of it does not overrun the buffer until the last row -- every
    // earlier row's furthest read lands inside the following row's bytes,
    // so a length check alone stays satisfied right up to the end and then
    // panics, or (worse, and invisibly) reads the next row's pixels and
    // shears the image. u64 throughout: `src_x + width` is attacker-
    // adjacent arithmetic and must not wrap.
    let right_edge_bytes = (u64::from(src_x) + u64::from(width)) * 4;
    if right_edge_bytes > stride as u64 {
        return Err(ConvertError::ShortInput {
            needed: right_edge_bytes as usize,
            got: stride,
        });
    }
    // Conservative on purpose: a mapped D3D11 staging texture is always
    // `stride * rows` bytes, so requiring the whole final row costs
    // nothing real and keeps the zero-origin case byte-for-byte the check
    // it has always been.
    let needed = (u64::from(src_y) + u64::from(height)) * stride as u64;
    if (bgra.len() as u64) < needed {
        return Err(ConvertError::ShortInput {
            needed: needed as usize,
            got: bgra.len(),
        });
    }

    let (w, h) = (width as usize, height as usize);
    let (ox, oy) = (src_x as usize, src_y as usize);
    let y_len = w * h;
    // resize() keeps the existing allocation when the length is unchanged,
    // which is the whole point: at 60 fps and 4K a per-frame allocation is
    // megabytes per second of churn in the hot path.
    out.resize(nv12_len(width, height), 0);
    let (y_plane, uv_plane) = out.split_at_mut(y_len);

    for row in 0..h {
        let src_row = (oy + row) * stride + ox * 4;
        let dst_row = row * w;
        for col in 0..w {
            let i = src_row + col * 4;
            let b = bgra[i] as f32;
            let g = bgra[i + 1] as f32;
            let r = bgra[i + 2] as f32;
            y_plane[dst_row + col] = luma709(r, g, b);
        }
    }

    // Chroma is AVERAGED over each 2x2 block, not point-sampled from the
    // top-left. A screen recording is mostly fine coloured detail (text,
    // syntax highlighting); point-sampling it is visibly wrong.
    for by in 0..h / 2 {
        for bx in 0..w / 2 {
            let mut sr = 0.0f32;
            let mut sg = 0.0f32;
            let mut sb = 0.0f32;
            for dy in 0..2 {
                for dx in 0..2 {
                    let i = (oy + by * 2 + dy) * stride + (ox + bx * 2 + dx) * 4;
                    sb += bgra[i] as f32;
                    sg += bgra[i + 1] as f32;
                    sr += bgra[i + 2] as f32;
                }
            }
            let (u, v) = chroma709(sr / 4.0, sg / 4.0, sb / 4.0);
            let o = (by * w / 2 + bx) * 2;
            uv_plane[o] = u;
            uv_plane[o + 1] = v;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a `width` x `height` BGRA buffer with `stride` bytes per row,
    /// every pixel the same colour, and recognisable padding after each row.
    fn solid(width: u32, height: u32, stride: usize, b: u8, g: u8, r: u8) -> Vec<u8> {
        let mut buf = vec![0xAAu8; stride * height as usize];
        for row in 0..height as usize {
            for col in 0..width as usize {
                let i = row * stride + col * 4;
                buf[i] = b;
                buf[i + 1] = g;
                buf[i + 2] = r;
                buf[i + 3] = 255;
            }
        }
        buf
    }

    #[test]
    fn nv12_is_one_luma_plane_plus_a_half_size_chroma_plane() {
        // 1.5 bytes per pixel. A wrong size here does not fail loudly -- the
        // encoder reads past the luma plane into whatever follows.
        assert_eq!(nv12_len(1920, 1080), 1920 * 1080 * 3 / 2);
        assert_eq!(nv12_len(2, 2), 6);
    }

    #[test]
    fn even_dims_rounds_down_never_up() {
        // Rounding UP would index past the end of the source frame. Down
        // loses at most one row and one column, deliberately.
        assert_eq!(even_dims(1921, 1081), (1920, 1080));
        assert_eq!(even_dims(1920, 1080), (1920, 1080));
        assert_eq!(even_dims(1, 1), (0, 0));
    }

    #[test]
    fn black_converts_to_the_limited_range_floor() {
        // BT.709 LIMITED range: black is luma 16, not 0, and neutral chroma
        // is 128. Emitting 0 here would make every capture look crushed on a
        // player that correctly expands 16-235.
        let src = solid(2, 2, 8, 0, 0, 0);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[16, 16, 16, 16], "luma floor");
        assert_eq!(&out[4..6], &[128, 128], "neutral chroma");
    }

    #[test]
    fn white_converts_to_the_limited_range_ceiling() {
        let src = solid(2, 2, 8, 255, 255, 255);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[235, 235, 235, 235], "luma ceiling");
        assert_eq!(&out[4..6], &[128, 128], "white is achromatic");
    }

    #[test]
    fn pure_red_lands_on_the_bt709_coefficients_not_the_bt601_ones() {
        // Regression naming the failure mode: with BT.601 coefficients pure
        // red gives luma ~81; BT.709 gives ~63. A decoder assuming 709 while
        // we encoded 601 is the "greens are slightly off" bug nobody files
        // and everybody sees. Pinning one known value catches a silent swap.
        let src = solid(2, 2, 8, 0, 0, 255);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(out[0], 63, "BT.709 limited-range luma for pure red");
    }

    #[test]
    fn row_padding_between_rows_is_skipped() {
        // windows-capture hands back a D3D11 staging texture whose row pitch
        // is usually WIDER than width*4. Reading it as tightly packed
        // shears the image progressively down the frame -- a bug that looks
        // like a capture glitch rather than a stride bug.
        let stride = 64; // far wider than 2 px * 4 bytes
        let src = solid(2, 2, stride, 255, 255, 255);
        let mut out = Vec::new();
        bgra_to_nv12(&src, stride, 2, 2, &mut out).unwrap();
        assert_eq!(
            &out[0..4],
            &[235, 235, 235, 235],
            "padding must not leak into luma"
        );
    }

    #[test]
    fn chroma_is_averaged_over_each_two_by_two_block() {
        // NV12 subsamples chroma 2x2. Sampling only the top-left pixel would
        // be visibly wrong on fine coloured detail -- exactly what a screen
        // recording of text is made of.
        let mut src = vec![0u8; 8 * 2];
        // Top-left red, the other three black.
        src[0] = 0;
        src[1] = 0;
        src[2] = 255;
        src[3] = 255;
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        let (u, v) = (out[4], out[5]);
        // Averaged red-and-black sits between neutral and pure red's chroma;
        // a top-left-only sample would give pure red's values.
        let mut red_only = Vec::new();
        bgra_to_nv12(&solid(2, 2, 8, 0, 0, 255), 8, 2, 2, &mut red_only).unwrap();
        assert!(
            (u as i16 - 128).abs() < (red_only[4] as i16 - 128).abs(),
            "chroma must be averaged, not point-sampled"
        );
        assert!(
            (v as i16 - 128).abs() < (red_only[5] as i16 - 128).abs(),
            "chroma must be averaged, not point-sampled"
        );
    }

    #[test]
    fn chroma_averaging_is_distinguishable_from_point_sampling_at_any_corner() {
        // The sibling test above (three of four pixels black) only rules out
        // point-sampling the TOP-LEFT corner -- any implementation that
        // instead point-sampled one of the other three corners would also
        // read as neutral there and still pass. Four distinct, asymmetric
        // colours make the true per-channel average land somewhere none of
        // the four corners' own chroma sits, so sampling ANY single corner
        // is distinguishable from averaging.
        //
        // Pixels (R,G,B): top-left (180,20,20), top-right (20,180,60),
        // bottom-left (60,20,180), bottom-right (90,90,40).
        //
        // Hand-computed (not run through the implementation):
        //   R avg = (180+20+60+90)/4 = 87.5
        //   G avg = (20+180+20+90)/4 = 77.5
        //   B avg = (20+60+180+40)/4 = 75.0
        //   Y avg = 0.2126*87.5 + 0.7152*77.5 + 0.0722*75.0
        //         = 18.6025 + 55.428 + 5.415 = 79.4455
        //   U avg = 128 + ((75.0 - 79.4455)/1.8556)*(224/255)
        //         = 128 + (-2.3957)*0.878431 = 128 - 2.1045 = 125.8955 -> 126
        //   V avg = 128 + ((87.5 - 79.4455)/1.5748)*(224/255)
        //         = 128 + (5.1146)*0.878431 = 128 + 4.4928 = 132.4928 -> 132
        //
        // Each corner's OWN chroma (computed the same way, single pixel):
        //   top-left     (180,20,20)  -> Y=54.016  -> (U,V) ~= (112,198)
        //   top-right    (20,180,60)  -> Y=137.32  -> (U,V) ~= (91,63)
        //   bottom-left  (60,20,180)  -> Y=40.056  -> (U,V) ~= (194,139)
        //   bottom-right (90,90,40)   -> Y=86.39   -> (U,V) ~= (106,130)
        // None of the four equals the averaged (126,132), so point-sampling
        // any single corner would fail this assertion.
        #[rustfmt::skip]
        let src: [u8; 16] = [
            // row 0: top-left (b,g,r,a), top-right (b,g,r,a)
            20, 20, 180, 255,   60, 180, 20, 255,
            // row 1: bottom-left (b,g,r,a), bottom-right (b,g,r,a)
            180, 20, 60, 255,   40, 90, 90, 255,
        ];
        let mut out = Vec::new();
        bgra_to_nv12(&src, 8, 2, 2, &mut out).unwrap();
        assert_eq!(
            (out[4], out[5]),
            (126, 132),
            "chroma must be the true 2x2 average"
        );
    }

    #[test]
    fn a_short_input_is_refused_rather_than_read_past_the_end() {
        // A truncated frame is a real possibility (a texture map that
        // partially failed). Reading past the end is an out-of-bounds read
        // that ships whatever memory follows into the recording.
        let src = vec![0u8; 8]; // one row's worth for a 2x2 frame
        let mut out = Vec::new();
        assert!(matches!(
            bgra_to_nv12(&src, 8, 2, 2, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    #[test]
    fn a_narrow_stride_is_refused_rather_than_panicking_on_the_last_row() {
        // A stride short of one packed row only overruns on the LAST row --
        // every earlier row's furthest read still lands inside the next
        // row's bytes, so the plain `bgra.len() < stride * height` check
        // stays satisfied right up to the end. Without the dedicated stride
        // check this panics with an out-of-bounds index on the final row's
        // red-channel read instead of returning `ShortInput`; two reviewers
        // re-derived this failure by hand before it got a test.
        let width = 2u32;
        let height = 2u32;
        // Two bytes short: the alpha byte of the last pixel is never read, so
        // a stride only one byte short of `width * 4` still has enough slack
        // in the last row's own colour reads to stay in bounds -- it takes a
        // second missing byte to push the last row's furthest read (its
        // red-channel byte) past the end of the buffer.
        let stride = (width as usize * 4) - 2;
        let src = vec![0xAAu8; stride * height as usize]; // satisfies the length-only check
        let mut out = Vec::new();
        assert!(matches!(
            bgra_to_nv12(&src, stride, width, height, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    #[test]
    fn odd_dimensions_are_refused_rather_than_silently_sheared() {
        let src = solid(3, 3, 12, 0, 0, 0);
        let mut out = Vec::new();
        assert!(matches!(
            bgra_to_nv12(&src, 12, 3, 3, &mut out),
            Err(ConvertError::OddDimensions)
        ));
    }

    /// A 4x4 BGRA frame whose four 2x2 quadrants are four distinct
    /// luminances, packed (stride = 16). Every expected Y below is derived
    /// BY HAND from `luma709` = `16 + Y*(219/255)` rounded, never by
    /// running the converter:
    ///   top-left     black  (0,0,0)       -> Y=0    -> 16
    ///   top-right    white  (255,255,255) -> Y=255  -> 16 + 219      = 235
    ///   bottom-left  grey   (128,128,128) -> Y=128  -> 16 + 109.929  = 126
    ///   bottom-right blue   (b=255,g=0,r=0) -> Y=18.411 -> 16 + 15.812 = 32
    /// The four differ enough that reading the wrong quadrant can never
    /// coincidentally produce the right answer.
    #[rustfmt::skip]
    fn four_quadrants() -> Vec<u8> {
        vec![
            // row 0: black, black, white, white          (b, g, r, a)
            0,0,0,255,      0,0,0,255,      255,255,255,255, 255,255,255,255,
            // row 1: black, black, white, white
            0,0,0,255,      0,0,0,255,      255,255,255,255, 255,255,255,255,
            // row 2: grey, grey, blue, blue
            128,128,128,255, 128,128,128,255, 255,0,0,255,    255,0,0,255,
            // row 3: grey, grey, blue, blue
            128,128,128,255, 128,128,128,255, 255,0,0,255,    255,0,0,255,
        ]
    }

    // THE region-capture test. A crop that ignores its origin reads the
    // top-left quadrant (16); one that swaps x and y reads the opposite
    // off-diagonal quadrant; one that negates the offset reads the
    // top-left again. All three mutations produce a different constant
    // from the correct one, in every direction.
    #[test]
    fn a_crop_reads_the_quadrant_its_origin_names() {
        let src = four_quadrants();
        let mut out = Vec::new();

        bgra_crop_to_nv12(&src, 16, 2, 0, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[235, 235, 235, 235], "top-right is white");

        bgra_crop_to_nv12(&src, 16, 0, 2, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[126, 126, 126, 126], "bottom-left is grey");

        bgra_crop_to_nv12(&src, 16, 2, 2, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[32, 32, 32, 32], "bottom-right is blue");

        bgra_crop_to_nv12(&src, 16, 0, 0, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[16, 16, 16, 16], "top-left is black");
    }

    // A crop is not required to start on an even pixel, and that is
    // deliberate rather than an oversight: NV12's 2x2 chroma blocks are
    // computed from the FULL-COLOUR BGRA source inside the crop, so an odd
    // origin produces a self-consistent frame. (Slicing an existing NV12
    // buffer at an odd offset would not — do not "fix" this by rounding
    // the origin, which would silently move the region the user drew.)
    #[test]
    fn an_odd_crop_origin_is_allowed_and_reads_the_right_pixels() {
        let src = four_quadrants();
        let mut out = Vec::new();
        // x=1,y=1: one pixel of each quadrant. Row 0 of the crop is
        // (1,1)=black and (2,1)=white; row 1 is (1,2)=grey and (2,2)=blue.
        bgra_crop_to_nv12(&src, 16, 1, 1, 2, 2, &mut out).unwrap();
        assert_eq!(&out[0..4], &[16, 235, 126, 32]);
    }

    // The whole point of keeping one implementation: the no-crop entry
    // point must still be byte-for-byte what it was, or every frame of
    // every non-region capture changes.
    #[test]
    fn the_uncropped_entry_point_equals_a_zero_origin_crop() {
        let src = four_quadrants();
        let mut via_plain = Vec::new();
        let mut via_crop = Vec::new();
        bgra_to_nv12(&src, 16, 4, 4, &mut via_plain).unwrap();
        bgra_crop_to_nv12(&src, 16, 0, 0, 4, 4, &mut via_crop).unwrap();
        assert_eq!(via_plain, via_crop);
        // And it is not vacuously equal because both are empty.
        assert_eq!(via_plain.len(), nv12_len(4, 4));
    }

    // A crop whose RIGHT edge runs past the row is a read of the next
    // row's pixels, which shears the image rather than overrunning the
    // buffer -- so the length check alone cannot catch it. Same class as
    // the existing narrow-stride test, one origin to the right.
    #[test]
    fn a_crop_running_past_the_row_is_refused() {
        let src = four_quadrants();
        let mut out = Vec::new();
        // stride 16 holds 4 pixels; a 4-wide crop at x=2 needs 6.
        assert!(matches!(
            bgra_crop_to_nv12(&src, 16, 2, 0, 4, 2, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    // A crop whose BOTTOM edge runs past the frame overruns the buffer.
    #[test]
    fn a_crop_running_past_the_last_row_is_refused() {
        let src = four_quadrants(); // 4 rows
        let mut out = Vec::new();
        assert!(matches!(
            bgra_crop_to_nv12(&src, 16, 0, 2, 2, 4, &mut out),
            Err(ConvertError::ShortInput { .. })
        ));
    }

    // Odd DIMENSIONS stay refused whatever the origin: NV12 has no way to
    // express a half chroma sample.
    #[test]
    fn a_crop_with_odd_dimensions_is_refused() {
        let src = four_quadrants();
        let mut out = Vec::new();
        assert!(matches!(
            bgra_crop_to_nv12(&src, 16, 1, 1, 3, 2, &mut out),
            Err(ConvertError::OddDimensions)
        ));
    }

    #[test]
    fn the_output_buffer_is_reused_across_frames_without_growing() {
        // At 60 fps and 4K, allocating an NV12 frame per tick is megabytes
        // per second of allocator churn in the hot path. The converter must
        // resize once and then reuse.
        let src = solid(4, 4, 16, 10, 20, 30);
        let mut out = Vec::new();
        bgra_to_nv12(&src, 16, 4, 4, &mut out).unwrap();
        let first_cap = out.capacity();
        // `capacity()` alone does not pin this: an implementation that
        // reallocates a brand-new same-size `Vec` every call also reports an
        // unchanged capacity (both paths request exactly `nv12_len` bytes),
        // so that assertion alone would pass just as happily on a
        // regression. `Vec::resize` to an unchanged length is documented to
        // be a no-op that touches neither the allocation nor the pointer,
        // so the pointer is the signal that actually distinguishes "reused"
        // from "reallocated with the same size".
        let first_ptr = out.as_ptr();
        for _ in 0..50 {
            bgra_to_nv12(&src, 16, 4, 4, &mut out).unwrap();
        }
        assert_eq!(out.len(), nv12_len(4, 4));
        assert_eq!(out.capacity(), first_cap, "no reallocation across frames");
        assert_eq!(
            out.as_ptr(),
            first_ptr,
            "buffer must be reused in place, not reallocated with the same size"
        );
    }
}
