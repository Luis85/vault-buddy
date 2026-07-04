import { ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  currentMonitor,
  LogicalSize,
} from "@tauri-apps/api/window";
import { planPanelPlacement, type Rect } from "./companionPlacement";

export const COLLAPSED = { width: 88, height: 88 };
export const EXPANDED = { width: 440, height: 340 };

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

  // Mirror the offset to the Rust side: quitting from the tray saves the
  // window position, and it must save the unshifted home position even if
  // the panel is open at that moment.
  function reportOffset() {
    void invoke("set_panel_offset", { x: offset.x, y: offset.y }).catch(
      () => {
        // not running under Tauri (unit tests) — nothing to report to
      },
    );
  }

  // A transition was superseded when the panel state changed while its
  // window calls were still in flight (e.g. a quick double-click).
  const stale = (expected: boolean) => panelOpen.value !== expected;

  // Position and size must change in ONE native call: applying them as two
  // IPC round-trips painted an intermediate geometry — the buddy flashed to
  // a corner whenever the panel opened shifted (left/up placements).
  function setGeometry(
    pos: { x: number; y: number },
    size: { width: number; height: number },
  ): Promise<void> {
    return invoke("set_window_geometry", {
      x: pos.x,
      y: pos.y,
      width: size.width,
      height: size.height,
    });
  }

  async function applyOpen(): Promise<void> {
    const win = getCurrentWindow();
    try {
      const [pos, scale, monitor] = await Promise.all([
        win.outerPosition(),
        win.scaleFactor(),
        currentMonitor(),
      ]);
      if (stale(true)) return;
      // Plan from the unshifted "home" position. If a previous open already
      // shifted the window (rapid open→close→open where the close was
      // superseded), planning from the raw position would conclude there is
      // room and reset the pending offset — the following close would then
      // never move the buddy back to where the user left it.
      const home = { x: pos.x + offset.x, y: pos.y + offset.y };
      const placement = planPanelPlacement(
        home,
        monitorRect(monitor),
        scale,
        COLLAPSED,
        EXPANDED,
      );
      // Record before moving: if we're superseded right after the move,
      // the close transition still knows what to undo.
      offset = placement.offset;
      reportOffset();
      side.value = placement.side;
      valign.value = placement.valign;
      await setGeometry(
        {
          x: home.x - placement.offset.x,
          y: home.y - placement.offset.y,
        },
        EXPANDED,
      );
    } catch {
      // No window/monitor info — grow right/down in place. Leave any
      // recorded offset untouched so a pending shift is still undone on
      // close.
      side.value = "right";
      valign.value = "down";
      await win
        .setSize(new LogicalSize(EXPANDED.width, EXPANDED.height))
        .catch(() => {});
    }
  }

  async function applyClose(): Promise<void> {
    const win = getCurrentWindow();
    try {
      const pos = await win.outerPosition();
      await setGeometry(
        { x: pos.x + offset.x, y: pos.y + offset.y },
        COLLAPSED,
      );
    } catch {
      // window may be gone during shutdown — best-effort collapse
      await win
        .setSize(new LogicalSize(COLLAPSED.width, COLLAPSED.height))
        .catch(() => {});
    }
    if (offset.x !== 0 || offset.y !== 0) {
      offset = { x: 0, y: 0 };
      reportOffset();
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
