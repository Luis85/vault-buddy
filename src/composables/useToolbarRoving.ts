/**
 * Roving tabindex for a `role="toolbar"` whose buttons live in more than
 * one component (visual-parity Task 16 fix round 1; design D16): the
 * timeline toolbar's own buttons and its zoom group's. The whole set is one
 * Tab stop; ←/→ move and wrap, Home/End jump — `useRovingTablist`, fed the
 * buttons in DOM order at the moment of the key.
 *
 * A button joins with `v-bind="roving.bind(key)"` — its `data-roving-key`,
 * its tabindex and a focus listener that makes it the stop (provided, so a
 * child component's buttons join the same set). A control
 * that is NOT a button — the Delete-mode select, the zoom range — stays its
 * own Tab stop and keeps its own arrow keys, the APG toolbar pattern: the
 * handler acts only on a key pressed on a roving button.
 *
 * `useRovingTablist` calls `preventDefault()` on the keys it takes, so the
 * editor's own Home/End (seek) never also runs from inside the toolbar.
 */
import type { InjectionKey, Ref } from "vue";
import { inject, provide, ref } from "vue";

import { useRovingTablist } from "./useRovingTablist";

export interface RovingAttrs {
  "data-roving-key": string;
  tabindex: 0 | -1;
  onFocus: () => void;
}

export interface ToolbarRoving {
  bind(key: string): RovingAttrs;
}

const TOOLBAR_ROVING: InjectionKey<ToolbarRoving> = Symbol("toolbarRoving");

/** Outside a roving toolbar every button is its own Tab stop. */
const STANDALONE: ToolbarRoving = { bind: (key) => ({ "data-roving-key": key, tabindex: 0, onFocus: () => {} }) };

export function injectToolbarRoving(): ToolbarRoving {
  return inject(TOOLBAR_ROVING, STANDALONE);
}

export function useToolbarRoving(root: Ref<HTMLElement | null>, firstKey: string) {
  const activeKey = ref(firstKey);
  const buttons = () => Array.from(root.value?.querySelectorAll<HTMLElement>("[data-roving-key]") ?? []);

  const { setTabRef, onKeydown } = useRovingTablist(
    () => buttons().length,
    () => buttons().findIndex((el) => el.dataset.rovingKey === activeKey.value),
    (i) => {
      activeKey.value = buttons()[i]?.dataset.rovingKey ?? activeKey.value;
    },
  );

  const roving: ToolbarRoving = {
    bind: (key) => ({
      "data-roving-key": key,
      tabindex: key === activeKey.value ? 0 : -1,
      onFocus: () => {
        activeKey.value = key;
      },
    }),
  };
  provide(TOOLBAR_ROVING, roving);

  function onToolbarKeydown(event: KeyboardEvent): void {
    if (!(event.target as HTMLElement).dataset?.rovingKey) return;
    buttons().forEach((el, i) => setTabRef(i, el));
    void onKeydown(event);
  }

  return { roving, onToolbarKeydown };
}
