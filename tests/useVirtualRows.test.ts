/**
 * `useVirtualRows` (Task 36; visual-parity Task 10 fix round 1): the
 * Captions tab is ONE scroller with the caption list inside it, below the
 * tab's heading and actions, so the window is measured from where the list
 * starts in the scrolled content (`listOffset`), not from the scroller's
 * top.
 */
import { describe, expect, it } from "vitest";

import { useVirtualRows } from "../src/composables/useVirtualRows";

describe("useVirtualRows with the list below other content", () => {
  it("counts scrolling only past the list's own top, and scrolls a row to the list's offset", () => {
    const { range, onScroll, scrollToIndex, viewport } = useVirtualRows(() => 100, 100, 300, 0, () => 400);
    // Scrolled 300px: still above the list, so the window starts at row 0.
    onScroll({ target: { scrollTop: 300 } } as unknown as Event);
    expect(range.value.first).toBe(0);
    // Scrolled 900px: 500px into the list = row 5.
    onScroll({ target: { scrollTop: 900 } } as unknown as Event);
    expect(range.value.first).toBe(5);
    expect(range.value.last).toBe(8);

    const el = { scrollTop: 0, clientHeight: 300 } as HTMLElement;
    viewport.value = el;
    scrollToIndex(3);
    expect(el.scrollTop).toBe(700);
    expect(range.value.first).toBe(3);
  });
});
