import { ref, watch, type Ref } from "vue";
import {
  getCurrentWindow,
  currentMonitor,
  LogicalSize,
  PhysicalPosition,
} from "@tauri-apps/api/window";
import {
  planPanelPlacement,
  type PanelPlacement,
  type Rect,
} from "./companionPlacement";

export const COLLAPSED = { width: 140, height: 170 };
export const EXPANDED = { width: 440, height: 340 };

const STAY: PanelPlacement = {
  side: "right",
  valign: "down",
  offset: { x: 0, y: 0 },
};

interface MonitorLike {
  position: { x: number; y: number };
  size: { width: number; height: number };
  workArea?: {
    position: { x: number; y: number };
    size: { width: number; height: number };
  };
}

/** Prefer the work area (excludes the taskbar) when the runtime provides it. */
function monitorRect(monitor: MonitorLike | null): Rect | null {
  if (!monitor) return null;
  const src = monitor.workArea ?? monitor;
  return {
    x: src.position.x,
    y: src.position.y,
    width: src.size.width,
    height: src.size.height,
  };
}

/**
 * Grows the transparent window when the panel opens and shrinks it back when
 * it closes, so the invisible window never blocks clicks on the desktop
 * beneath it. The growth direction respects the monitor edges: near the
 * right or bottom edge the window is shifted so the panel unfolds toward
 * free space, and the returned `side`/`valign` let the layout mirror itself
 * to keep the buddy visually pinned.
 */
export function useCompanionWindow(panelOpen: Ref<boolean>): {
  side: Ref<"right" | "left">;
  valign: Ref<"down" | "up">;
} {
  const side = ref<"right" | "left">("right");
  const valign = ref<"down" | "up">("down");
  // Physical px subtracted from the window position while the panel is open.
  // The close path adds it back relative to the *current* position, so the
  // buddy stays put even if the window was dragged while open.
  let offset = { x: 0, y: 0 };

  // A transition was superseded when the panel state changed while its
  // window calls were still in flight (e.g. a quick double-click).
  const stale = (expected: boolean) => panelOpen.value !== expected;

  async function applyOpen(): Promise<void> {
    const win = getCurrentWindow();
    let placement = STAY;
    try {
      const [pos, scale, monitor] = await Promise.all([
        win.outerPosition(),
        win.scaleFactor(),
        currentMonitor(),
      ]);
      if (stale(true)) return;
      placement = planPanelPlacement(
        pos,
        monitorRect(monitor),
        scale,
        COLLAPSED,
        EXPANDED,
      );
      if (placement.offset.x !== 0 || placement.offset.y !== 0) {
        // Record before moving: if we're superseded right after the move,
        // the close transition still knows what to undo.
        offset = placement.offset;
        await win.setPosition(
          new PhysicalPosition(
            pos.x - placement.offset.x,
            pos.y - placement.offset.y,
          ),
        );
      }
    } catch {
      placement = STAY; // no monitor info — grow right/down as before
    }
    if (stale(true)) return;
    side.value = placement.side;
    valign.value = placement.valign;
    offset = placement.offset;
    await win.setSize(new LogicalSize(EXPANDED.width, EXPANDED.height));
  }

  async function applyClose(): Promise<void> {
    const win = getCurrentWindow();
    await win.setSize(new LogicalSize(COLLAPSED.width, COLLAPSED.height));
    if (offset.x !== 0 || offset.y !== 0) {
      try {
        const pos = await win.outerPosition();
        await win.setPosition(
          new PhysicalPosition(pos.x + offset.x, pos.y + offset.y),
        );
      } catch {
        // window may be gone during shutdown; nothing to restore
      }
      offset = { x: 0, y: 0 };
    }
    side.value = "right";
    valign.value = "down";
  }

  // Serialize transitions: a close never interleaves with an in-flight open
  // (which could re-expand the window after it was collapsed, leaving an
  // invisible click-blocking area). Superseded transitions are skipped.
  let queue: Promise<void> = Promise.resolve();
  watch(panelOpen, (open) => {
    queue = queue
      .then(() => {
        if (stale(open)) return; // a newer toggle already won
        return open ? applyOpen() : applyClose();
      })
      .catch(() => {
        // a failed transition must not wedge the queue
      });
  });

  return { side, valign };
}
