//! Region-selection geometry: converting the overlay's LOGICAL (CSS pixel)
//! rectangle into the PHYSICAL pixels a captured frame is measured in, and
//! clamping it to the frame that actually arrived.
//!
//! These are two different coordinate spaces and confusing them is the
//! region feature's most likely bug, so the conversion is one pure function
//! tested across the DPI scales Windows actually ships (spec §5.2).
//!
//! Division of labour between the two public functions: `to_physical`
//! guarantees only that a dimension is at least 1 physical pixel — it has
//! no frame to clamp against yet, so it cannot know the final encodable
//! size. `clamp_to_frame` runs later, once the real frame is known, and is
//! the last pure gate before the encoder — it is what enforces both
//! "at least 2" and "even", because H.264 with NV12 4:2:0 chroma requires
//! even width and height on both axes (GAP-101).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Non-finite or negative values read as 0 — the overlay is a webview and a
/// NaN from a pointer event must degrade, never panic or wrap.
fn non_negative(v: f64) -> f64 {
    if v.is_finite() && v > 0.0 {
        v
    } else {
        0.0
    }
}

/// Scale a logical rectangle into physical pixels.
///
/// Width and height are floored to at least 1: a sub-pixel selection that
/// rounded to zero would be an unencodable crop.
pub fn to_physical(rect: LogicalRect, scale: f64) -> PhysicalRect {
    // A non-positive or non-finite scale means we failed to read the monitor;
    // 1.0 (no scaling) is the safe reading, not 0.
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    PhysicalRect {
        x: (non_negative(rect.x) * scale) as u32,
        y: (non_negative(rect.y) * scale) as u32,
        width: ((non_negative(rect.width) * scale) as u32).max(1),
        height: ((non_negative(rect.height) * scale) as u32).max(1),
    }
}

/// Intersect a rectangle with the frame. `None` when the intersection is
/// empty — a region whose monitor changed resolution, or a zero-sized frame.
///
/// This runs at capture START, not only at selection time: the resolution can
/// change in between, and an unclamped stale rectangle indexes outside the
/// frame buffer.
pub fn clamp_to_frame(rect: PhysicalRect, frame_w: u32, frame_h: u32) -> Option<PhysicalRect> {
    if frame_w == 0 || frame_h == 0 || rect.width == 0 || rect.height == 0 {
        return None;
    }
    if rect.x >= frame_w || rect.y >= frame_h {
        return None;
    }
    // Both guards above make this subtraction non-zero and each min() result
    // >= 1: rect.width/height are >= 1, and rect.x < frame_w (rect.y <
    // frame_h), so each min() takes the smaller of two values that are both
    // >= 1. But H.264 with NV12 4:2:0 chroma subsampling requires EVEN width
    // and height on both axes — each chroma plane is half resolution per
    // axis, so an odd luma dimension has no integer chroma-plane size, and
    // Windows Media Foundation's H.264 encoder MFT rejects or silently pads
    // an odd-dimension input media type. Rounding each dimension down to
    // even is exactly what can take a >= 1 result down to 0 (a clamped 1
    // rounds to 0), which is why the zero check below is reachable now, not
    // dead code: a sub-2-pixel crop is not encodable, the same reasoning
    // already applied to a zero-width crop above.
    let width = (rect.width.min(frame_w - rect.x)) & !1;
    let height = (rect.height.min(frame_h - rect.y)) & !1;
    if width == 0 || height == 0 {
        return None;
    }
    Some(PhysicalRect {
        x: rect.x,
        y: rect.y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn logical(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
        LogicalRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn scale_of_one_is_a_straight_cast() {
        let p = to_physical(logical(10.0, 20.0, 300.0, 200.0), 1.0);
        assert_eq!(
            p,
            PhysicalRect {
                x: 10,
                y: 20,
                width: 300,
                height: 200
            }
        );
    }

    #[test]
    fn common_dpi_scales_multiply_every_component() {
        assert_eq!(
            to_physical(logical(10.0, 20.0, 300.0, 200.0), 2.0),
            PhysicalRect {
                x: 20,
                y: 40,
                width: 600,
                height: 400
            }
        );
        assert_eq!(
            to_physical(logical(100.0, 100.0, 400.0, 300.0), 1.5),
            PhysicalRect {
                x: 150,
                y: 150,
                width: 600,
                height: 450
            }
        );
        assert_eq!(
            to_physical(logical(0.0, 0.0, 800.0, 600.0), 1.25),
            PhysicalRect {
                x: 0,
                y: 0,
                width: 1000,
                height: 750
            }
        );
    }

    // A fractional scale must never round a dimension down to zero; a
    // zero-width crop is not encodable.
    #[test]
    fn a_tiny_rect_keeps_at_least_one_pixel() {
        let p = to_physical(logical(0.0, 0.0, 0.4, 0.4), 1.0);
        assert_eq!(p.width, 1);
        assert_eq!(p.height, 1);
    }

    #[test]
    fn a_negative_or_nonfinite_input_is_treated_as_zero() {
        let p = to_physical(logical(-5.0, -5.0, 100.0, 100.0), 1.0);
        assert_eq!((p.x, p.y), (0, 0), "negative origin clamps to 0");
        let n = to_physical(logical(f64::NAN, 0.0, f64::INFINITY, 100.0), 1.0);
        assert_eq!(n.x, 0, "NaN origin is 0");
        assert_eq!(n.height, 100);
    }

    #[test]
    fn a_nonpositive_scale_falls_back_to_one() {
        let p = to_physical(logical(10.0, 10.0, 100.0, 100.0), 0.0);
        assert_eq!(
            p,
            PhysicalRect {
                x: 10,
                y: 10,
                width: 100,
                height: 100
            }
        );
    }

    #[test]
    fn a_rect_inside_the_frame_is_unchanged() {
        let r = PhysicalRect {
            x: 10,
            y: 10,
            width: 100,
            height: 100,
        };
        assert_eq!(clamp_to_frame(r, 1920, 1080), Some(r));
    }

    // Regression: the monitor's resolution can change between selecting a
    // region and starting the capture. Without re-clamping at start, the
    // stale rectangle indexes outside the frame buffer (spec §5.2).
    #[test]
    fn a_rect_overflowing_the_frame_is_truncated() {
        let r = PhysicalRect {
            x: 1800,
            y: 1000,
            width: 400,
            height: 400,
        };
        assert_eq!(
            clamp_to_frame(r, 1920, 1080),
            Some(PhysicalRect {
                x: 1800,
                y: 1000,
                width: 120,
                height: 80
            })
        );
    }

    #[test]
    fn a_rect_fully_outside_the_frame_is_none() {
        let r = PhysicalRect {
            x: 5000,
            y: 5000,
            width: 100,
            height: 100,
        };
        assert_eq!(clamp_to_frame(r, 1920, 1080), None, "empty intersection");
    }

    #[test]
    fn a_rect_starting_exactly_at_the_frame_edge_is_none() {
        let r = PhysicalRect {
            x: 1920,
            y: 0,
            width: 100,
            height: 100,
        };
        assert_eq!(clamp_to_frame(r, 1920, 1080), None);
    }

    #[test]
    fn a_zero_sized_rect_or_frame_is_none() {
        assert_eq!(
            clamp_to_frame(
                PhysicalRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 10
                },
                1920,
                1080
            ),
            None
        );
        assert_eq!(
            clamp_to_frame(
                PhysicalRect {
                    x: 0,
                    y: 0,
                    width: 10,
                    height: 10
                },
                0,
                1080
            ),
            None
        );
    }

    #[test]
    fn the_overlay_to_capture_round_trip_holds_at_150_percent() {
        // The overlay reports CSS pixels; the frame is physical. A 1280x720
        // selection on a 150%-scaled 2560x1440 monitor is 1920x1080 physical.
        let selected = logical(0.0, 0.0, 1280.0, 720.0);
        let physical = to_physical(selected, 1.5);
        assert_eq!(
            physical,
            PhysicalRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080
            }
        );
        assert_eq!(
            clamp_to_frame(physical, 2560, 1440),
            Some(physical),
            "fits the real frame"
        );
    }

    // GAP-101: H.264 with NV12 4:2:0 chroma requires even width/height, but
    // the plain min()-clamp can land on an odd value — the real review case:
    // a 400x400 rect at x=1800,y=1000 against a 1921x1081 frame clamps to
    // 121x81 (odd/odd), which must round DOWN to 120x80, not up (rounding up
    // would grow the crop past the selection the user drew).
    #[test]
    fn an_odd_clamped_dimension_rounds_down_to_even() {
        let r = PhysicalRect {
            x: 1800,
            y: 1000,
            width: 400,
            height: 400,
        };
        assert_eq!(
            clamp_to_frame(r, 1921, 1081),
            Some(PhysicalRect {
                x: 1800,
                y: 1000,
                width: 120,
                height: 80
            })
        );
    }

    // An already-even clamp must be left untouched by the new rounding step
    // — this is the pre-existing overflow test's exact shape, re-asserted
    // here to pin that the even-rounding is a no-op on already-even input.
    #[test]
    fn an_already_even_clamp_is_untouched_by_rounding() {
        let r = PhysicalRect {
            x: 1800,
            y: 1000,
            width: 400,
            height: 400,
        };
        assert_eq!(
            clamp_to_frame(r, 1920, 1080),
            Some(PhysicalRect {
                x: 1800,
                y: 1000,
                width: 120,
                height: 80
            })
        );
    }

    // GAP-101: a clamped dimension of exactly 1 rounds down to 0, which is
    // not encodable — this must return None, the same as a zero-width rect,
    // not a silently-empty Some(PhysicalRect { width: 0, .. }).
    #[test]
    fn a_clamped_dimension_of_exactly_one_is_none() {
        // A 1-px-wide rect at the origin: min(1, frame_w) = 1, rounds to 0.
        let one_px_wide = PhysicalRect {
            x: 0,
            y: 0,
            width: 1,
            height: 100,
        };
        assert_eq!(clamp_to_frame(one_px_wide, 1920, 1080), None);

        // A rect starting at frame_w - 1: min(width, frame_w - x) = 1.
        let starting_at_edge_minus_one = PhysicalRect {
            x: 1919,
            y: 0,
            width: 100,
            height: 100,
        };
        assert_eq!(clamp_to_frame(starting_at_edge_minus_one, 1920, 1080), None);
    }

    // GAP-101 round-trip: to_physical alone can still hand back an odd
    // dimension (it only floors to >= 1, it never rounds to even — that is
    // clamp_to_frame's job, per the module doc's division of labour). A
    // 401-px logical width at 1.5 scale gives 601 physical; clamp_to_frame
    // against a frame with plenty of room is what makes it encodable (600).
    #[test]
    fn to_physical_can_be_odd_and_clamp_to_frame_makes_it_even() {
        let physical = to_physical(logical(0.0, 0.0, 401.0, 300.0), 1.5);
        assert_eq!(physical.width, 601, "to_physical does not round to even");
        assert_eq!(
            clamp_to_frame(physical, 1920, 1080).map(|r| r.width),
            Some(600)
        );
    }
}
