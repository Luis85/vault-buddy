/**
 * `menuModel.ts` (visual-parity Task 5; concept spec §8): the pure
 * placement and wording rules every editor menu shares.
 */
import { describe, expect, it } from "vitest";

import { anchorPoint, clampInto, formatMenuTime, hintFor, submenuPoint } from "../src/components/editor/menus/menuModel";

describe("menu placement", () => {
  it("pulls a menu 8 px inside the viewport, and pins one taller than it to the margin", () => {
    expect(clampInto(100, 282, 1_600)).toBe(100);
    expect(clampInto(1_500, 282, 1_600)).toBe(1_310);
    expect(clampInto(-40, 282, 1_600)).toBe(8);
    expect(clampInto(0, 900, 640)).toBe(8);
  });

  it("opens a submenu 4 px right of its item, and flips it left when it would overflow", () => {
    expect(submenuPoint(new DOMRect(900, 300, 270, 31), 1_600)).toEqual({ x: 1_174, y: 300 });
    expect(submenuPoint(new DOMRect(1_300, 300, 270, 31), 1_600)).toEqual({ x: 1_051, y: 300 });
  });

  it("opens at the pointer, or just below the control that opened it", () => {
    expect(anchorPoint({ x: 12, y: 34 })).toEqual({ x: 12, y: 34 });
  });
});

describe("menu wording", () => {
  it("formats a time as MM:SS.d", () => {
    expect(formatMenuTime(17_400)).toBe("00:17.4");
    expect(formatMenuTime(125_990)).toBe("02:05.9");
    expect(formatMenuTime(-5)).toBe("00:00.0");
  });

  it("hints the reason, then a submenu, then a destructive item, else how to apply", () => {
    expect(hintFor({ id: "a", label: "A", disabledReason: "Why not", danger: true })).toBe("Why not");
    expect(hintFor({ id: "a", label: "A", submenu: [] })).toBe("→ Open options");
    expect(hintFor({ id: "a", label: "A", danger: true })).toBe("Removes from this edit. You can undo.");
    expect(hintFor({ id: "a", label: "A" })).toBe("Enter to apply · Esc to dismiss");
  });

  // Final review, Important 2: every danger item read "You can undo." —
  // including Discard project…, which cannot be undone. An item's own hint
  // replaces the generic line; a disabled reason still comes first.
  it("an item's own hint replaces the generic one, after a disabled reason", () => {
    expect(hintFor({ id: "a", label: "A", danger: true, hint: "Cannot be undone." })).toBe("Cannot be undone.");
    expect(hintFor({ id: "a", label: "A", danger: true, hint: "Cannot be undone.", disabledReason: "Why not" })).toBe(
      "Why not",
    );
  });
});
