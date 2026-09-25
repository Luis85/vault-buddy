/**
 * A fixed-row-height window over a long list (Task 36: a project may hold
 * 2000 captions, and rendering every row -- each an editable text box and
 * two time fields -- would make every projection install re-render
 * thousands of inputs). Only the rows inside the scroll viewport, plus
 * `overscan` either side, are rendered; two spacers keep the scrollbar the
 * height the whole list would be.
 *
 * `clientHeight` is 0 before layout (and always in happy-dom), so an
 * unmeasured viewport falls back to `fallbackHeight` rather than rendering
 * nothing.
 */
import type { Ref } from "vue";
import { computed, ref } from "vue";

export interface VirtualRange {
  first: number;
  /** Exclusive. */
  last: number;
  padTop: number;
  padBottom: number;
}

export interface VirtualRows {
  viewport: Ref<HTMLElement | null>;
  range: Ref<VirtualRange>;
  onScroll: (event: Event) => void;
  /** Scrolls row `index` to the top of the viewport. */
  scrollToIndex: (index: number) => void;
}

export function useVirtualRows(
  count: () => number,
  rowHeight: number,
  fallbackHeight = 360,
  overscan = 4,
): VirtualRows {
  const viewport = ref<HTMLElement | null>(null);
  const scrollTop = ref(0);

  const range = computed<VirtualRange>(() => {
    const total = count();
    const height = viewport.value?.clientHeight || fallbackHeight;
    const first = Math.max(0, Math.floor(scrollTop.value / rowHeight) - overscan);
    const last = Math.min(total, Math.ceil((scrollTop.value + height) / rowHeight) + overscan);
    return { first, last, padTop: first * rowHeight, padBottom: Math.max(0, total - last) * rowHeight };
  });

  function onScroll(event: Event): void {
    scrollTop.value = (event.target as HTMLElement).scrollTop;
  }

  function scrollToIndex(index: number): void {
    const top = Math.max(0, index) * rowHeight;
    if (viewport.value) viewport.value.scrollTop = top;
    scrollTop.value = top;
  }

  return { viewport, range, onScroll, scrollToIndex };
}
