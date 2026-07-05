import { nextTick, ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  currentMonitor,
  LogicalSize,
} from "@tauri-apps/api/window";
import { planPanelPlacement, type Rect } from "./companionPlacement";
import { logBreadcrumb, logWarning } from "../logging";

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

// Tail of the transition queue. The updater awaits this before handing off
// to the installer, so the close transition — including the offset reset
// reported to Rust — has fully landed and prepare_update_install can't
// race it into a double-shifted position.
let transitionsTail: Promise<void> = Promise.resolve();

export async function panelTransitionsSettled(): Promise<void> {
  // the watcher that extends the queue runs on the pre-flush tick — let it
  // fire first, or a just-toggled close would be missed
  await nextTick();
  await transitionsTail;
}

// Resolves once the browser has painted at least one frame. A queued rAF
// callback runs just before a paint, so a second rAF guarantees the frame
// scheduled by the first was composited. Used to bracket the window resize
// with real paints so the buddy mask is on-screen before the move and the
// grown layout is on-screen before the reveal. Degrades to a macrotask
// where rAF is unavailable (non-browser test runners).
function afterPaint(): Promise<void> {
  return new Promise((resolve) => {
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(() => requestAnimationFrame(() => resolve()));
    } else {
      setTimeout(resolve, 0);
    }
  });
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
  maskBuddy: Ref<boolean>;
} {
  const side = ref<"right" | "left">("right");
  const valign = ref<"down" | "up">("down");
  // Hides the buddy for the blink of a *shifted* open. A shifted open moves
  // the window's top-left corner (up and/or left); WebView2 keeps compositing
  // its last-painted collapsed frame at that raised origin until the webview
  // reflows for the larger viewport, flashing the buddy to the corner for a
  // frame (an upstream wry/WebView2 resize race — the native move is already
  // one atomic SetWindowPos). Masking the buddy so the stale frame has nothing
  // to show, then revealing it once the grown layout has painted, is the only
  // lever available at this layer.
  const maskBuddy = ref(false);
  // Physical px subtracted from the window position while it is grown. The
  // collapse path adds it back relative to the *current* position, so the
  // buddy stays put even if the window was dragged while open.
  let offset = { x: 0, y: 0 };

  // Mirror the offset to the Rust side: quitting from the tray saves the
  // window position, and it must save the unshifted home position even if
  // the window is grown at that moment.
  function reportOffset(): Promise<void> {
    return invoke("set_panel_offset", { x: offset.x, y: offset.y }).catch(
      () => {
        // not running under Tauri (unit tests) — nothing to report to
      },
    ) as Promise<void>;
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
    logBreadcrumb(
      `geometry → ${pos.x},${pos.y} ${size.width}×${size.height}`,
    );
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
      void reportOffset();
      side.value = placement.side;
      valign.value = placement.valign;
      // Only a shifted open moves the origin and so exposes the stale-frame
      // flash; an in-place (down/right) grow keeps the top-left fixed and
      // needs no mask (masking it would be a gratuitous buddy blink).
      const shifts = placement.offset.x !== 0 || placement.offset.y !== 0;
      if (shifts) {
        maskBuddy.value = true;
        // Paint the masked frame BEFORE the window moves, or the stale frame
        // WebView2 re-shows at the raised origin still contains the buddy.
        await afterPaint();
      }
      await setGeometry(
        {
          x: home.x - placement.offset.x,
          y: home.y - placement.offset.y,
        },
        size,
      );
      // Let the grown, bottom-anchored layout paint while still masked, so the
      // reveal lands on the correct final frame rather than a mid-reflow one.
      if (shifts) await afterPaint();
    } catch (e) {
      // No window/monitor info — grow right/down in place. Leave any
      // recorded offset untouched so a pending shift is still undone on
      // collapse.
      logWarning(`applyGrow fell back: ${String(e)}`);
      side.value = "right";
      valign.value = "down";
      await win
        .setSize(new LogicalSize(size.width, size.height))
        .catch(() => {});
    } finally {
      // Always reveal — a superseded or failed grow must never strand the
      // buddy invisible.
      maskBuddy.value = false;
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
    } catch (e) {
      // window may be gone during shutdown — best-effort collapse
      logWarning(`applyCollapse fell back: ${String(e)}`);
      await win
        .setSize(new LogicalSize(COLLAPSED.width, COLLAPSED.height))
        .catch(() => {});
    }
    if (offset.x !== 0 || offset.y !== 0) {
      offset = { x: 0, y: 0 };
      // awaited so the transition queue settles only once Rust knows the
      // offset is gone — the updater relies on this ordering
      await reportOffset();
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
      .catch((e) => {
        // a failed transition must not wedge the queue
        logWarning(`window transition failed: ${String(e)}`);
      });
    // the updater awaits panelTransitionsSettled() → transitionsTail, so it
    // must always track the latest queued transition (bubble or panel)
    transitionsTail = queue;
  }

  watch(panelOpen, schedule);
  if (bubbleOpen) watch(bubbleOpen, schedule);

  return { side, valign, maskBuddy };
}
