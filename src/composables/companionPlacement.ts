export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface PanelPlacement {
  /** Which side of the buddy the panel opens on. */
  side: "right" | "left";
  /** Whether the window grows downward or upward. */
  valign: "down" | "up";
  /**
   * Physical pixels subtracted from the window position while the panel is
   * open, so the buddy stays visually pinned while the window grows toward
   * free screen space. Applied inversely on close.
   */
  offset: Point;
}

const STAY: PanelPlacement = {
  side: "right",
  valign: "down",
  offset: { x: 0, y: 0 },
};

/**
 * Decides where the panel should unfold given the collapsed window's
 * position and the monitor's work area, all in physical pixels. Sizes are
 * logical and scaled by `scaleFactor`.
 */
export function planPanelPlacement(
  windowPos: Point,
  monitor: Rect | null,
  scaleFactor: number,
  collapsed: Size,
  expanded: Size,
): PanelPlacement {
  if (!monitor) return STAY;

  const dw = Math.round((expanded.width - collapsed.width) * scaleFactor);
  const dh = Math.round((expanded.height - collapsed.height) * scaleFactor);
  const expandedW = Math.round(expanded.width * scaleFactor);
  const expandedH = Math.round(expanded.height * scaleFactor);

  const fitsRight = windowPos.x + expandedW <= monitor.x + monitor.width;
  const fitsBelow = windowPos.y + expandedH <= monitor.y + monitor.height;
  const canShiftLeft = windowPos.x - dw >= monitor.x;
  const canShiftUp = windowPos.y - dh >= monitor.y;

  const side = !fitsRight && canShiftLeft ? "left" : "right";
  const valign = !fitsBelow && canShiftUp ? "up" : "down";

  return {
    side,
    valign,
    offset: {
      x: side === "left" ? dw : 0,
      y: valign === "up" ? dh : 0,
    },
  };
}
