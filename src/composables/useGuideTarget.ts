/**
 * Binds a guide target (Task 55; ADR R18) from the component that OWNS the
 * control: `const target = useGuideTarget("header.save")`, then
 * `:ref="target"` on the control. The registration lives exactly as long as
 * the component, and the bound element carries its key in
 * `data-guide-target` (a space-separated list: the library's Media tab is the
 * route to two lessons) while registered and active — what a person
 * inspecting the DOM reads to see which controls the guide can point at.
 * WHICH of a key's registrations wins is `targets.ts`' `resolve`.
 *
 * A component ref resolves to its root element (`$el`), so a primitive like
 * `AppButton` binds the same way a native `<button>` does.
 */
import type { ComponentPublicInstance } from "vue";
import { onBeforeUnmount, shallowRef, watchEffect } from "vue";

import type { GuideTargetKey, TargetOptions } from "../editor/guide/targets";
import { register, registerOverflow } from "../editor/guide/targets";

type Bindable = Element | ComponentPublicInstance | null;
/** A template ref callback. */
export type GuideRef = (value: Bindable) => void;

function toElement(value: Bindable): Element | null {
  if (value === null || value instanceof Element) return value;
  const root: unknown = value.$el;
  return root instanceof Element ? root : null;
}

function mark(el: HTMLElement, key: GuideTargetKey, on: boolean): void {
  const keys = new Set((el.dataset.guideTarget ?? "").split(" ").filter(Boolean));
  if (on) keys.add(key);
  else keys.delete(key);
  if (keys.size > 0) el.dataset.guideTarget = [...keys].join(" ");
  else delete el.dataset.guideTarget;
}

function bindable(key: GuideTargetKey, marks: () => boolean): { el: () => Element | null; ref: GuideRef } {
  const el = shallowRef<Element | null>(null);
  watchEffect((onCleanup) => {
    const current = el.value;
    if (!(current instanceof HTMLElement)) return;
    mark(current, key, marks());
    onCleanup(() => mark(current, key, false));
    // Synchronous: the mark follows the element the moment a render binds
    // it, not a scheduler tick later.
  }, { flush: "sync" });
  return { el: () => el.value, ref: (value) => (el.value = toElement(value)) };
}

/** Registers the bound element as `key`'s control. */
export function useGuideTarget(key: GuideTargetKey, options: TargetOptions = {}): GuideRef {
  const active = options.active ?? (() => true);
  const { el, ref } = bindable(key, active);
  onBeforeUnmount(register(key, el, { ...options, active }));
  return ref;
}

/** Registers the bound element as the More item `key`'s control hides
 * behind while `overflowed()` answers true. */
export function useGuideOverflow(key: GuideTargetKey, overflowed: () => boolean): GuideRef {
  const { el, ref } = bindable(key, overflowed);
  onBeforeUnmount(registerOverflow(key, overflowed, el));
  return ref;
}

/** One ref callback for a tablist: each tab id binds the keys it is the
 * fallback route to (`LibraryPanel`, `InspectorPanel`). */
export function useGuideTabTargets<T extends string>(
  tabs: Partial<Record<T, GuideTargetKey[]>>,
): (tab: T, value: Bindable) => void {
  const refs = new Map<T, GuideRef[]>();
  for (const tab of Object.keys(tabs) as T[]) {
    refs.set(tab, (tabs[tab] ?? []).map((key) => useGuideTarget(key, { fallback: true })));
  }
  return (tab, value) => refs.get(tab)?.forEach((bind) => bind(value));
}
