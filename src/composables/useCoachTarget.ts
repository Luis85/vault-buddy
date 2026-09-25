/**
 * Where the guide's current control is on screen (Task 56; ONBOARDING.md
 * § Overlay: "Observe target position after panel changes, overflow,
 * scrolling, viewport resizing and zoom."). While the coach is showing, the
 * lesson's key is re-resolved through the registry (`targets.ts` — so a
 * tool that moved into More, or a tab that opened, is followed) and its box
 * re-measured on every resize, on every scroll anywhere (capture phase: the
 * editor scrolls in `main` and in its panels, not the document), and on a
 * short poll for the changes neither event reports (a drawer opening, a
 * panel resizing).
 *
 * `found` is the registered element; `rect` is its box only when it has
 * one — a control inside a closed drawer is registered but has no size,
 * and the coach says so instead of drawing a ring around nothing.
 */
import type { ShallowRef } from "vue";
import { onBeforeUnmount, shallowRef, watch } from "vue";

import type { Rect, Size } from "../editor/guide/position";
import type { GuideTargetKey, ResolvedGuideTarget } from "../editor/guide/targets";
import { resolve } from "../editor/guide/targets";

/** How often the coach re-checks its target while showing. */
const POLL_MS = 250;

export type TargetState = "direct" | "overflow" | "hidden" | "missing";

export interface CoachTarget {
  found: ShallowRef<ResolvedGuideTarget | null>;
  rect: ShallowRef<Rect | null>;
  viewport: ShallowRef<Size>;
  state: () => TargetState;
  measure: () => void;
}

function boxOf(el: Element | undefined): Rect | null {
  const r = el?.getBoundingClientRect();
  return r && r.width > 0 && r.height > 0 ? { x: r.left, y: r.top, width: r.width, height: r.height } : null;
}

function sameRect(a: Rect | null, b: Rect | null): boolean {
  if (a === null || b === null) return a === b;
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
}

function sameTarget(a: ResolvedGuideTarget | null, b: ResolvedGuideTarget | null): boolean {
  return a?.element === b?.element && a?.revealed === b?.revealed;
}

/** Tracks `key()`'s control while `live()` answers true. Unchanged
 * measurements are not re-assigned, so a still editor re-renders nothing
 * on the poll. */
export function useCoachTarget(key: () => GuideTargetKey | null, live: () => boolean): CoachTarget {
  const found = shallowRef<ResolvedGuideTarget | null>(null);
  const rect = shallowRef<Rect | null>(null);
  const viewport = shallowRef<Size>({ width: window.innerWidth, height: window.innerHeight });

  function measure(): void {
    const k = key();
    const next = live() && k !== null ? resolve(k) : null;
    if (!sameTarget(found.value, next)) found.value = next;
    const box = boxOf(next?.element);
    if (!sameRect(rect.value, box)) rect.value = box;
    const vp = viewport.value;
    if (vp.width !== window.innerWidth || vp.height !== window.innerHeight) {
      viewport.value = { width: window.innerWidth, height: window.innerHeight };
    }
  }

  function state(): TargetState {
    if (!found.value) return "missing";
    return rect.value ? found.value.revealed : "hidden";
  }

  // Poll and listeners share ONE switch: nothing is measured, on any
  // scroll or resize, while the coach is not showing.
  let timer: ReturnType<typeof setInterval> | null = null;
  function stop(): void {
    if (timer !== null) clearInterval(timer);
    timer = null;
    window.removeEventListener("resize", measure);
    window.removeEventListener("scroll", measure, true);
  }
  function start(): void {
    timer = setInterval(measure, POLL_MS);
    window.addEventListener("resize", measure);
    window.addEventListener("scroll", measure, true);
  }
  watch(
    live,
    (on) => {
      stop();
      if (on) start();
      measure();
    },
    { immediate: true },
  );
  onBeforeUnmount(stop);

  return { found, rect, viewport, state, measure };
}
