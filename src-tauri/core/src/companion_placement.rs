//! Where the panel/bubble window goes relative to the buddy window. Pure and
//! unit-tested here; the shell calls it when it positions the (hidden) panel
//! window before showing it. All coordinates are physical pixels.

/// A rectangle in physical pixels (top-left origin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// A point in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/// Which side of the buddy a companion window sits on. Also the SpeechBubble
/// `side` prop: `Right` means the bubble is to the RIGHT of the buddy, so its
/// tail sits on its LEFT face pointing back toward the buddy (and vice versa).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

/// Vertical tail placement — the SpeechBubble `valign` prop. `Down` means the
/// bubble is TOP-aligned with the buddy (tail near the bubble's top, level with
/// the buddy); `Up` means it is BOTTOM-aligned (tail near the bottom, so the
/// bubble unfolds upward). Named for the `valign` prop values it maps to, not
/// for the alignment direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Up,
    Down,
}

/// The side + vertical alignment the bubble actually landed on, so the tail can
/// be drawn pointing at the buddy. Computed together with the window position
/// by `place_beside` — the two must agree or the tail points into empty space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub side: Side,
    pub valign: VAlign,
}

/// Top-left for the panel window, given the buddy rect, the monitor work area,
/// and the panel size. The panel always prefers the RIGHT side, so this is a
/// thin wrapper over `place_beside` that discards the anchor.
pub fn panel_position(buddy: Rect, work_area: Option<Rect>, panel_w: i32, panel_h: i32) -> Point {
    place_beside(buddy, work_area, panel_w, panel_h, Side::Right).0
}

/// Top-left AND resolved anchor for a companion window placed beside the buddy.
///
/// Opens on the `prefer` side and TOP-aligned with the buddy; if that side
/// overflows its screen edge it flips to the other side, and near the bottom
/// edge it BOTTOM-aligns so the window unfolds upward. The returned `Anchor`
/// reports the side/valign actually chosen, so the caller can point the tail
/// back at the buddy. The position is clamped to the work area. With no work
/// area (unknown monitor) it honors `prefer`, top-aligned, unclamped.
pub fn place_beside(
    buddy: Rect,
    work_area: Option<Rect>,
    w: i32,
    h: i32,
    prefer: Side,
) -> (Point, Anchor) {
    let right_x = buddy.x + buddy.w;
    let left_x = buddy.x - w;
    let Some(area) = work_area else {
        let (x, side) = match prefer {
            Side::Right => (right_x, Side::Right),
            Side::Left => (left_x, Side::Left),
        };
        return (
            Point { x, y: buddy.y },
            Anchor {
                side,
                valign: VAlign::Down,
            },
        );
    };
    // Horizontal: try the preferred side; flip to the other if it overflows
    // that screen edge.
    let (x, side) = match prefer {
        Side::Right => {
            if right_x + w <= area.x + area.w {
                (right_x, Side::Right)
            } else {
                (left_x, Side::Left)
            }
        }
        Side::Left => {
            if left_x >= area.x {
                (left_x, Side::Left)
            } else {
                (right_x, Side::Right)
            }
        }
    };
    // Vertical: top-aligned with the buddy unless that overflows the bottom
    // edge, in which case bottom-align (window unfolds upward).
    let (y, valign) = if buddy.y + h <= area.y + area.h {
        (buddy.y, VAlign::Down)
    } else {
        (buddy.y + buddy.h - h, VAlign::Up)
    };
    // Clamp fully on-screen. A window larger than the work area (or a buddy in
    // a corner) still lands inside; max is floored to the min so clamp never
    // sees max < min. The side/valign decision already fit the window, so the
    // clamp only nudges corner cases and never contradicts the anchor.
    let max_x = (area.x + area.w - w).max(area.x);
    let max_y = (area.y + area.h - h).max(area.y);
    (
        Point {
            x: x.clamp(area.x, max_x),
            y: y.clamp(area.y, max_y),
        },
        Anchor { side, valign },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        w: 1920,
        h: 1080,
    };
    const PANEL_W: i32 = 360;
    const PANEL_H: i32 = 340;
    const BUBBLE_W: i32 = 260;
    const BUBBLE_H: i32 = 150;
    const BUDDY: i32 = 88;

    fn buddy_at(x: i32, y: i32) -> Rect {
        Rect {
            x,
            y,
            w: BUDDY,
            h: BUDDY,
        }
    }

    #[test]
    fn opens_right_and_top_aligned_with_room() {
        let p = panel_position(buddy_at(100, 100), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(
            p,
            Point {
                x: 100 + BUDDY,
                y: 100
            }
        );
    }

    #[test]
    fn flips_left_near_the_right_edge() {
        // buddy hugging the right edge: right of it would overflow → open left
        let p = panel_position(buddy_at(1900, 100), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p.x, 1900 - PANEL_W);
        assert_eq!(p.y, 100);
    }

    #[test]
    fn bottom_aligns_near_the_bottom_edge() {
        // buddy near the bottom: top-aligned would overflow → panel bottom
        // meets the buddy bottom (unfolds upward), then clamped to fit on-screen
        let p = panel_position(buddy_at(100, 1000), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p.x, 100 + BUDDY);
        // Panel would be at 1000 + 88 - 340 = 748, but clamped to fit in area
        assert_eq!(p.y, 740);
    }

    #[test]
    fn handles_the_bottom_right_corner() {
        let p = panel_position(buddy_at(1900, 1000), Some(AREA), PANEL_W, PANEL_H);
        assert_eq!(p.x, 1900 - PANEL_W);
        // Panel would be at 1000 + 88 - 340 = 748, but clamped to fit in area
        assert_eq!(p.y, 740);
    }

    #[test]
    fn no_monitor_falls_back_to_right_top() {
        let p = panel_position(buddy_at(100, 100), None, PANEL_W, PANEL_H);
        assert_eq!(p, Point { x: 188, y: 100 });
    }

    #[test]
    fn clamps_a_panel_larger_than_the_work_area() {
        let small = Rect {
            x: 0,
            y: 0,
            w: 200,
            h: 200,
        };
        let p = panel_position(buddy_at(10, 10), Some(small), PANEL_W, PANEL_H);
        // max is floored to the area origin — never panics, lands at origin
        assert_eq!(p, Point { x: 0, y: 0 });
    }

    #[test]
    fn place_beside_opens_on_the_preferred_right() {
        // faces right → bubble to the right of the buddy, tail on its left
        let (p, a) = place_beside(
            buddy_at(100, 100),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Right,
        );
        assert_eq!(
            p,
            Point {
                x: 100 + BUDDY,
                y: 100
            }
        );
        assert_eq!(
            a,
            Anchor {
                side: Side::Right,
                valign: VAlign::Down,
            }
        );
    }

    #[test]
    fn place_beside_opens_on_the_preferred_left() {
        // faces left → bubble to the left of the buddy, tail on its right
        let (p, a) = place_beside(
            buddy_at(400, 100),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Left,
        );
        assert_eq!(
            p,
            Point {
                x: 400 - BUBBLE_W,
                y: 100
            }
        );
        assert_eq!(
            a,
            Anchor {
                side: Side::Left,
                valign: VAlign::Down,
            }
        );
    }

    #[test]
    fn place_beside_flips_a_right_preference_at_the_right_edge() {
        let (p, a) = place_beside(
            buddy_at(1900, 100),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Right,
        );
        assert_eq!(p.x, 1900 - BUBBLE_W);
        assert_eq!(a.side, Side::Left);
    }

    #[test]
    fn place_beside_flips_a_left_preference_at_the_left_edge() {
        // buddy hugging the left edge: opening left overflows → flip right
        let (p, a) = place_beside(buddy_at(0, 100), Some(AREA), BUBBLE_W, BUBBLE_H, Side::Left);
        assert_eq!(p.x, BUDDY); // buddy at x=0, so right side is at 0 + BUDDY
        assert_eq!(a.side, Side::Right);
    }

    #[test]
    fn place_beside_bottom_aligns_and_flips_valign_up() {
        let (_, a) = place_beside(
            buddy_at(100, 1000),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Right,
        );
        assert_eq!(a.valign, VAlign::Up);
    }

    #[test]
    fn place_beside_honors_the_preferred_side_with_no_monitor() {
        let (p, a) = place_beside(buddy_at(400, 100), None, BUBBLE_W, BUBBLE_H, Side::Left);
        assert_eq!(
            p,
            Point {
                x: 400 - BUBBLE_W,
                y: 100
            }
        );
        assert_eq!(a.side, Side::Left);
    }

    #[test]
    fn respects_a_non_zero_work_area_origin() {
        let area = Rect {
            x: -1920,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let p = panel_position(buddy_at(-1800, 100), Some(area), PANEL_W, PANEL_H);
        assert_eq!(
            p,
            Point {
                x: -1800 + BUDDY,
                y: 100
            }
        );
    }
}
