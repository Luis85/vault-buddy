/**
 * The guide's typed targets (Task 55; F-46; ADR R18): what each lesson
 * points at, as a KEY a component registers — never a CSS selector the
 * guide would have to keep in step with every template. The concept
 * bundle's selectors (`.projectbar`, `#splitButton`, …) are a behaviour map
 * only; `content.ts` maps each lesson to one of these keys, and the
 * component that OWNS the control binds it through `useGuideTarget`.
 *
 * **Resolution.** A key may have several registrations:
 * - a *fallback* — the always-mounted way to reach a control that only
 *   exists while its tab or section is open (the library's Captions tab for
 *   `library.captions`, the inspector's Layout tab for `inspector.layout`);
 *   the owning panel itself registers without `fallback` and wins whenever
 *   it is mounted;
 * - an *active* predicate — a clip registers `clip.selected` only while it
 *   is the selected one;
 * - an *overflow rule* — when the owning control has moved into a toolbar's
 *   More menu, `resolve` returns the More item with `revealed: "overflow"`,
 *   so the guide highlights the command the user can actually see, never an
 *   invisible duplicate (ONBOARDING.md § Overlay).
 *
 * Module state, one registry per webview: the editor window loads this
 * module once, and every registration is removed when its component
 * unmounts.
 */

/** ADR R18's key list, in lesson order. */
export const GUIDE_TARGET_KEYS = [
  "projectbar",
  "library.import",
  "transport",
  "timeline.toolbar",
  "clip.selected",
  "timeline.split",
  "timeline.undo",
  "inspector",
  "timeline.more",
  "track.menu",
  "library.webcam",
  "inspector.layout",
  "inspector.fades",
  "mixer",
  "preview.toolstrip",
  "library.captions",
  "library.chapters",
  "header.checks",
  "header.save",
  "header.render",
  "library.products",
  "header.help",
] as const;
export type GuideTargetKey = (typeof GUIDE_TARGET_KEYS)[number];

type ElementSource = () => Element | null;

export interface TargetOptions {
  /** Reached only through this control while the owner is not mounted. */
  fallback?: boolean;
  /** Registered, but only a target while this answers true. */
  active?: () => boolean;
}

interface Entry {
  element: ElementSource;
  fallback: boolean;
  active: () => boolean;
}

interface OverflowRule {
  overflowed: () => boolean;
  more: ElementSource;
}

/** What `resolve` found: the element to highlight, and whether it is the
 * control itself or the More item it is hidden behind. */
export interface ResolvedGuideTarget {
  element: Element;
  revealed: "direct" | "overflow";
}

const entries = new Map<GuideTargetKey, Set<Entry>>();
const overflowRules = new Map<GuideTargetKey, Set<OverflowRule>>();

function add<T>(map: Map<GuideTargetKey, Set<T>>, key: GuideTargetKey, item: T): () => void {
  let set = map.get(key);
  if (!set) {
    set = new Set();
    map.set(key, set);
  }
  set.add(item);
  return () => {
    set.delete(item);
  };
}

/** Registers `element` as `key`'s control; returns the unregister. */
export function register(key: GuideTargetKey, element: ElementSource, options: TargetOptions = {}): () => void {
  return add(entries, key, { element, fallback: options.fallback ?? false, active: options.active ?? (() => true) });
}

/** Declares that `key`'s control can move into an overflow menu, reached
 * through `more` while `overflowed()` answers true. */
export function registerOverflow(key: GuideTargetKey, overflowed: () => boolean, more: ElementSource): () => void {
  return add(overflowRules, key, { overflowed, more });
}

function connected(source: ElementSource): Element | null {
  const el = source();
  return el?.isConnected ? el : null;
}

function overflowTarget(key: GuideTargetKey): Element | null {
  for (const rule of overflowRules.get(key) ?? []) {
    const more = rule.overflowed() ? connected(rule.more) : null;
    if (more) return more;
  }
  return null;
}

function directTarget(key: GuideTargetKey): Element | null {
  let fallback: Element | null = null;
  for (const entry of entries.get(key) ?? []) {
    const el = entry.active() ? connected(entry.element) : null;
    if (el && !entry.fallback) return el;
    fallback ??= el;
  }
  return fallback;
}

/** The mounted control a lesson should point at, or `null` when nothing
 * mounted registers it (the guide then explains rather than highlights). */
export function resolve(key: GuideTargetKey): ResolvedGuideTarget | null {
  const more = overflowTarget(key);
  if (more) return { element: more, revealed: "overflow" };
  const element = directTarget(key);
  return element ? { element, revealed: "direct" } : null;
}
