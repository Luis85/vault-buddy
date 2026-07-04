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
// A transient window just big enough for the buddy plus a greeting speech
// bubble beside it. Kept small so the invisible click area it creates at
// startup is minimal and short-lived (the bubble auto-dismisses).
export const BUBBLE = { width: 260, height: 150 };

type WindowState = "collapsed" | "bubble" | "expanded";

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
 * Grows the transparent window when the panel opens or a greeting bubble
 * shows, and shrinks it back when neither wants space, so the invisible
 * window never blocks clicks on the desktop beneath it. The growth
 * direction respects the monitor edges: near the right or bottom edge the
 * window is shifted so it unfolds toward free space, and the returned
 * `side`/`valign` let the layout mirror itself to keep the buddy visually
 * pinned. The panel always takes precedence over the bubble.
 */
export function useCompanionWindow(
  panelOpen: Ref<boolean>,
  bubbleOpen?: Ref<boolean>,
): {
  side: Ref<"right" | "left">;
  valign: Ref<"down" | "up">;
} {
  const side = ref<"right" | "left">("right");
  const valign = ref<"down" | "up">("down");
  // Physical px subtracted from the window position while it is grown. The
  // collapse path adds it back relative to the *current* position, so the
  // buddy stays put even if the window was dragged while open.
  let offset = { x: 0, y: 0 };

  // Mirror the offset to the Rust side: quitting from the tray saves the
  // window position, and it must save the unshifted home position even if
  // the window is grown at that moment.
  function reportOffset() {
    void invoke("set_panel_offset", { x: offset.x, y: offset.y }).catch(
      () => {
        // not running under Tauri (unit tests) — nothing to report to
      },
    );
  }

  // Panel beats bubble beats collapsed. When the panel is open its larger
  // window already contains the buddy, so the greeting bubble never drives
  // geometry.
  function desiredState(): WindowState {
    if (panelOpen.value) return "expanded";
    if (bubbleOpen?.value) return "bubble";
    return "collapsed";
  }

  const sizeFor = (target: WindowState) =>
    target === "expanded" ? EXPANDED : BUBBLE;

  // Position and size must change in ONE native call: applying them as two
  // IPC round-trips painted an intermediate geometry — the buddy flashed to
  // a corner whenever the window grew shifted (left/up placements).
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

  // `target` is the grown state this transition was queued for ("bubble" or
  // "expanded"). If the desired state has since changed (a newer toggle),
  // the transition is stale and must not paint an outdated geometry.
  async function applyGrow(target: WindowState): Promise<void> {
    const size = sizeFor(target);
    const win = getCurrentWindow();
    try {
      const [pos, scale, monitor] = await Promise.all([
        win.outerPosition(),
        win.scaleFactor(),
        currentMonitor(),
      ]);
      if (desiredState() !== target) return;
      // Plan from the unshifted "home" position. If a previous grow already
      // shifted the window (rapid grow→collapse→grow where the collapse was
      // superseded), planning from the raw position would conclude there is
      // room and reset the pending offset — the following collapse would
      // then never move the buddy back to where the user left it.
      const home = { x: pos.x + offset.x, y: pos.y + offset.y };
      const placement = planPanelPlacement(
        home,
        monitorRect(monitor),
        scale,
        COLLAPSED,
        size,
      );
      // Record before moving: if we're superseded right after the move, the
      // collapse transition still knows what to undo.
      offset = placement.offset;
      reportOffset();
      side.value = placement.side;
      valign.value = placement.valign;
      await setGeometry(
        {
          x: home.x - placement.offset.x,
          y: home.y - placement.offset.y,
        },
        size,
      );
    } catch {
      // No window/monitor info — grow right/down in place. Leave any
      // recorded offset untouched so a pending shift is still undone on
      // collapse.
      side.value = "right";
      valign.value = "down";
      await win
        .setSize(new LogicalSize(size.width, size.height))
        .catch(() => {});
    }
  }

  async function applyCollapse(): Promise<void> {
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

  function applyState(target: WindowState): Promise<void> {
    return target === "collapsed" ? applyCollapse() : applyGrow(target);
  }

  // Serialize transitions: a collapse never interleaves with an in-flight
  // grow (which could re-expand the window after it was collapsed, leaving
  // an invisible click-blocking area). Superseded transitions are skipped.
  let queue: Promise<void> = Promise.resolve();
  function schedule() {
    const target = desiredState();
    queue = queue
      .then(() => {
        if (desiredState() !== target) return; // a newer toggle already won
        return applyState(target);
      })
      .catch(() => {
        // a failed transition must not wedge the queue
      });
  }

  watch(panelOpen, schedule);
  if (bubbleOpen) watch(bubbleOpen, schedule);

  return { side, valign };
}
