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

/// Top-left for the panel window, given the buddy rect, the monitor work area,
/// and the panel size.
///
/// Prefers RIGHT of the buddy and TOP-aligned with it; near the right edge it
/// flips LEFT, near the bottom edge it BOTTOM-aligns so the panel unfolds
/// upward. The result is clamped to the work area. With no work area (unknown
/// monitor) it degrades to right + top-aligned, unclamped — the same fallback
/// the old single-window code used.
pub fn panel_position(buddy: Rect, work_area: Option<Rect>, panel_w: i32, panel_h: i32) -> Point {
    let Some(area) = work_area else {
        return Point {
            x: buddy.x + buddy.w,
            y: buddy.y,
        };
    };
    // Horizontal: to the right of the buddy unless that overflows the right
    // edge, in which case flip to the left of the buddy.
    let right_x = buddy.x + buddy.w;
    let x = if right_x + panel_w <= area.x + area.w {
        right_x
    } else {
        buddy.x - panel_w
    };
    // Vertical: top-aligned with the buddy unless that overflows the bottom
    // edge, in which case bottom-align (panel unfolds upward).
    let y = if buddy.y + panel_h <= area.y + area.h {
        buddy.y
    } else {
        buddy.y + buddy.h - panel_h
    };
    // Clamp fully on-screen. A panel larger than the work area (or a buddy in
    // a corner) still lands inside; max is floored to the min so clamp never
    // sees max < min.
    let max_x = (area.x + area.w - panel_w).max(area.x);
    let max_y = (area.y + area.h - panel_h).max(area.y);
    Point {
        x: x.clamp(area.x, max_x),
        y: y.clamp(area.y, max_y),
    }
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
