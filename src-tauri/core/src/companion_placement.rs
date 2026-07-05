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

/// Where the tail sits on the card so it points at the buddy — the SpeechBubble
/// `valign` prop (`top`/`middle`/`bottom`). `Middle` is the common case (the
/// bubble is centered level with the buddy); `Top`/`Bottom` happen only when a
/// screen edge pushes the card above or below the buddy's center.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

/// Vertical placement strategy. `Edge` top-aligns with the buddy and flips to
/// bottom-align near the bottom edge (the panel, which unfolds downward).
/// `Center` sits the window level with the buddy's center (the bubble, so its
/// tail points at the character, not the top of the buddy window).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VMode {
    Edge,
    Center,
}

/// The side + vertical alignment the bubble actually landed on, so the tail can
/// be drawn pointing at the buddy. Computed together with the window position
/// by `place_beside` — the two must agree or the tail points into empty space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub side: Side,
    pub valign: VAlign,
}

/// Tolerance (px) within which the card counts as level with the buddy — a
/// small clamp near a screen edge then still reads as a `Middle` tail.
const VALIGN_TOL: i32 = 8;

/// Top-left for the panel window, given the buddy rect, the monitor work area,
/// and the panel size. The panel prefers the RIGHT side and edge-aligns
/// vertically, so this is a thin wrapper over `place_beside` that discards the
/// anchor.
pub fn panel_position(buddy: Rect, work_area: Option<Rect>, panel_w: i32, panel_h: i32) -> Point {
    place_beside(buddy, work_area, panel_w, panel_h, Side::Right, VMode::Edge).0
}

/// Top-left AND resolved anchor for a companion window placed beside the buddy.
///
/// Opens on the `prefer` side; if that side overflows its screen edge it flips
/// to the other side. Vertically it follows `vmode`: `Edge` top-aligns (flips
/// to bottom-align near the bottom edge), `Center` sits level with the buddy's
/// center. The result is clamped to the work area, and the returned `Anchor`
/// reports the side plus where the tail must sit (derived from where the card
/// actually landed relative to the buddy's center) so the tail points back at
/// the buddy. With no work area (unknown monitor) it honors `prefer`, unclamped.
pub fn place_beside(
    buddy: Rect,
    work_area: Option<Rect>,
    w: i32,
    h: i32,
    prefer: Side,
    vmode: VMode,
) -> (Point, Anchor) {
    let right_x = buddy.x + buddy.w;
    let left_x = buddy.x - w;
    // Horizontal: try the preferred side; flip to the other if it overflows
    // that screen edge (no work area → honor the preference).
    let (x_pref, side) = match (work_area, prefer) {
        (Some(area), Side::Right) if right_x + w > area.x + area.w => (left_x, Side::Left),
        (Some(area), Side::Left) if left_x < area.x => (right_x, Side::Right),
        (_, Side::Right) => (right_x, Side::Right),
        (_, Side::Left) => (left_x, Side::Left),
    };
    let buddy_center_y = buddy.y + buddy.h / 2;
    // Vertical (pre-clamp) per mode.
    let y_pref = match vmode {
        VMode::Edge => match work_area {
            Some(area) if buddy.y + h > area.y + area.h => buddy.y + buddy.h - h,
            _ => buddy.y,
        },
        VMode::Center => buddy_center_y - h / 2,
    };
    // Clamp fully on-screen. A window larger than the work area (or a buddy in
    // a corner) still lands inside; max is floored to the min so clamp never
    // sees max < min.
    let (x, y) = match work_area {
        Some(area) => {
            let max_x = (area.x + area.w - w).max(area.x);
            let max_y = (area.y + area.h - h).max(area.y);
            (x_pref.clamp(area.x, max_x), y_pref.clamp(area.y, max_y))
        }
        None => (x_pref, y_pref),
    };
    // The tail points at the buddy relative to where the card actually landed:
    // card above the buddy's center → tail low (Bottom), below → tail high
    // (Top), level → Middle. This is computed post-clamp, so an edge nudge is
    // reflected in the tail.
    let card_center_y = y + h / 2;
    let valign = if card_center_y + VALIGN_TOL < buddy_center_y {
        VAlign::Bottom
    } else if card_center_y > buddy_center_y + VALIGN_TOL {
        VAlign::Top
    } else {
        VAlign::Middle
    };
    (Point { x, y }, Anchor { side, valign })
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
    fn place_beside_centers_on_the_buddy_facing_right() {
        // faces right → bubble to the right, centered level with the buddy
        let (p, a) = place_beside(
            buddy_at(100, 100),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Right,
            VMode::Center,
        );
        // centered: y = buddy_center(144) - h/2(75) = 69
        assert_eq!(
            p,
            Point {
                x: 100 + BUDDY,
                y: 69
            }
        );
        assert_eq!(
            a,
            Anchor {
                side: Side::Right,
                valign: VAlign::Middle,
            }
        );
    }

    #[test]
    fn place_beside_centers_on_the_buddy_facing_left() {
        // faces left → bubble to the left, centered level with the buddy
        let (p, a) = place_beside(
            buddy_at(400, 100),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Left,
            VMode::Center,
        );
        assert_eq!(
            p,
            Point {
                x: 400 - BUBBLE_W,
                y: 69
            }
        );
        assert_eq!(
            a,
            Anchor {
                side: Side::Left,
                valign: VAlign::Middle,
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
            VMode::Center,
        );
        assert_eq!(p.x, 1900 - BUBBLE_W);
        assert_eq!(a.side, Side::Left);
    }

    #[test]
    fn place_beside_flips_a_left_preference_at_the_left_edge() {
        // buddy hugging the left edge: opening left overflows → flip right
        let (p, a) = place_beside(
            buddy_at(0, 100),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Left,
            VMode::Center,
        );
        assert_eq!(p.x, BUDDY); // buddy at x=0, so the right side is at 0 + BUDDY
        assert_eq!(a.side, Side::Right);
    }

    #[test]
    fn place_beside_points_the_tail_up_when_clamped_at_the_top_edge() {
        // buddy at the very top: the centered bubble can't rise to meet it, so
        // it clamps down and the tail points UP to the buddy above it.
        let (p, a) = place_beside(
            buddy_at(100, 0),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Right,
            VMode::Center,
        );
        assert_eq!(p.y, 0);
        assert_eq!(a.valign, VAlign::Top);
    }

    #[test]
    fn place_beside_points_the_tail_down_when_clamped_at_the_bottom_edge() {
        let (p, a) = place_beside(
            buddy_at(100, 1000),
            Some(AREA),
            BUBBLE_W,
            BUBBLE_H,
            Side::Right,
            VMode::Center,
        );
        // clamped up to fit: max_y = 1080 - 150 = 930
        assert_eq!(p.y, 930);
        assert_eq!(a.valign, VAlign::Bottom);
    }

    #[test]
    fn place_beside_honors_the_preferred_side_with_no_monitor() {
        let (p, a) = place_beside(
            buddy_at(400, 100),
            None,
            BUBBLE_W,
            BUBBLE_H,
            Side::Left,
            VMode::Center,
        );
        assert_eq!(
            p,
            Point {
                x: 400 - BUBBLE_W,
                y: 69
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
