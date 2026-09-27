/**
 * The shape and the geometry behind `MenuPanel.vue` (visual-parity Task 5;
 * concept spec §8, design D13). Every editor menu — the timeline's context
 * menus today, the header's Project/View/Help menus later — is a list of
 * `MenuItem`s rendered by the one panel, so the item type lives here rather
 * than in any one caller.
 *
 * An item either runs (`run`), opens a submenu (`submenu`), or is disabled
 * with the reason a person needs (`disabledReason`). An item with none of
 * the three would be a control that does nothing, which design D14 forbids:
 * a surface with no backend omits the item instead.
 */
import type { EditorIconName } from "../icons/conceptIcons";

export interface MenuAction {
  id: string;
  label: string;
  icon?: EditorIconName;
  /** The shortcut shown on the right; only one that really is bound. */
  kbd?: string;
  /** Defined makes the item a `menuitemcheckbox`; `true` shows a check. */
  checked?: boolean;
  danger?: boolean;
  /** The hint line's text while this item has focus, in place of the
   * generic one — for an item the generic line would misdescribe (a danger
   * item that cannot be undone). A disabled reason still comes first. */
  hint?: string;
  disabledReason?: string | null;
  submenu?: MenuItem[];
  run?: () => void;
}

export interface MenuSeparator {
  separator: true;
}

export type MenuItem = MenuAction | MenuSeparator;

/** Where a menu opens: a pointer position, or the control that opened it. */
export type MenuAnchor = { x: number; y: number } | HTMLElement;

export const SEPARATOR: MenuSeparator = { separator: true };

export const MENU_SUBTITLE = "Direct editing · originals unchanged";
export const DEFAULT_HINT = "↑ ↓ navigate · Enter choose · Esc close";
export const SUBMENU_DELAY_MS = 180;
const SUBMENU_WIDTH_PX = 245;
const VIEWPORT_MARGIN_PX = 8;
const SUBMENU_GAP_PX = 4;
const ANCHOR_GAP_PX = 4;

export function isSeparator(item: MenuItem): item is MenuSeparator {
  return "separator" in item;
}

export function isDisabled(item: MenuAction): boolean {
  return Boolean(item.disabledReason);
}

/** The hint line's text while `item` has focus (concept §8's four cases),
 * or the item's own `hint`. "You can undo." is only true of an edit, so an
 * irreversible danger item carries its own. */
export function hintFor(item: MenuAction): string {
  if (item.disabledReason) return item.disabledReason;
  if (item.hint) return item.hint;
  if (item.submenu) return "→ Open options";
  if (item.danger) return "Removes from this edit. You can undo.";
  return "Enter to apply · Esc to dismiss";
}

/** A `size` box starting at `pos`, pulled `8px` inside `[0, viewport]`; a
 * box larger than the viewport still starts at the margin. */
export function clampInto(pos: number, size: number, viewport: number): number {
  return Math.max(VIEWPORT_MARGIN_PX, Math.min(pos, viewport - size - VIEWPORT_MARGIN_PX));
}

/** The point a menu opens at: the pointer itself, or just below a control. */
export function anchorPoint(anchor: MenuAnchor): { x: number; y: number } {
  if (!(anchor instanceof HTMLElement)) return anchor;
  const rect = anchor.getBoundingClientRect();
  return { x: rect.left, y: rect.bottom + ANCHOR_GAP_PX };
}

/** A submenu sits 4px right of its item, or flips to the item's left when
 * it would run past the viewport. */
export function submenuPoint(itemRect: DOMRect, viewportWidth: number): { x: number; y: number } {
  const right = itemRect.right + SUBMENU_GAP_PX;
  const fits = right + SUBMENU_WIDTH_PX + VIEWPORT_MARGIN_PX <= viewportWidth;
  return { x: fits ? right : itemRect.left - SUBMENU_WIDTH_PX - SUBMENU_GAP_PX, y: itemRect.top };
}

/** `MM:SS.d`, the concept's menu timecode ("Split at 00:17.4"). */
export function formatMenuTime(ms: number): string {
  const tenths = Math.floor(Math.max(0, ms) / 100);
  const seconds = Math.floor(tenths / 10);
  const mm = String(Math.floor(seconds / 60)).padStart(2, "0");
  const ss = String(seconds % 60).padStart(2, "0");
  return `${mm}:${ss}.${tenths % 10}`;
}
