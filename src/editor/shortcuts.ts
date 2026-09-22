/**
 * The tutorial editor's keyboard shortcut map (Task 17; F-15, F-49).
 * Deliberately pure and DOM-listener-free — this module exports a lookup
 * table plus a couple of pure predicates over a `KeyboardEvent`; nothing
 * here calls `window.addEventListener`. A future task wires the actual
 * `window`-level dispatcher once there is a real timeline/canvas to bind it
 * against and a real selection to act on (`editorWorkspace`, Task 18) —
 * wiring one now, with nothing yet to select and nothing yet listening for
 * its result, would just be an untestable no-op with an extra failure mode
 * (a leaked listener) for no behavior. `shouldHandle` is what that future
 * dispatcher must call before acting on any key — see its own doc below.
 *
 * `SHORTCUTS` binds a normalized key combo directly to an `ActionId`
 * (`resolveActions`/`commandFor` in `actions.ts` decide whether that action
 * is actually available). Two of the Behavior section's bindings do NOT
 * name an `ActionId` at all and are exposed as their own predicates instead
 * of table entries:
 *   - `Shift+F10` / the Menu key opens the CONTEXT MENU at whatever is
 *     focused — that is a menu-visibility event, not an edit, so it has no
 *     `ActionId` to resolve. `isContextMenuShortcut` is what a focused
 *     clip's own keydown handler (a later task, once clips render) checks.
 *   - `F6` is spelled "guide focus" in the brief, but this task's `ActionId`
 *     union has no separate guide-focus id — the closest live concept is
 *     `focusPreview` (SCREENS-AND-INTERACTIONS.md §02: "focus-preview layout
 *     change[s] view state"), so F6 is bound to it. Recorded as a decision
 *     in this task's report, not invented silently.
 */
import type { ActionId } from "./actions";

// The human-readable "Ctrl+Z" display string (`SHORTCUT_DISPLAY`) lives in
// `actions.ts`, not here: `resolveActions` needs it synchronously while
// building its result, and `actions.ts` must not import FROM this file
// (the reverse import already runs the other way, and a back-edge would be
// the exact cycle `circularDependencies 0` exists to catch) — so the
// shared data sits in the file the two share a one-way edge toward. Import
// it from `./actions` directly rather than through here.

/** Normalized-combo -> action id. Combo grammar: lowercase modifiers in
 * `ctrl+shift+` order, then the key exactly as `shortcutKey` normalizes it
 * below — see that function's own doc for why punctuation keys never carry
 * an explicit `shift+` prefix. */
export const SHORTCUTS: ReadonlyMap<string, ActionId> = new Map<string, ActionId>([
  ["s", "split"],
  ["delete", "delete"],
  ["backspace", "delete"],
  ["shift+delete", "deleteClose"],
  ["shift+backspace", "deleteClose"],
  ["ctrl+z", "undo"],
  ["ctrl+shift+z", "redo"],
  ["ctrl+y", "redo"],
  ["ctrl+c", "copy"],
  ["ctrl+x", "cut"],
  ["ctrl+v", "paste"],
  ["ctrl+d", "duplicate"],
  ["ctrl+g", "group"],
  ["ctrl+shift+g", "ungroup"],
  ["ctrl+s", "save"],
  ["ctrl+e", "render"],
  ["f1", "help"],
  ["?", "help"],
  ["f6", "focusPreview"],
]);

/**
 * Normalizes a `KeyboardEvent` into `SHORTCUTS`' combo grammar.
 *
 * `shift+` is added explicitly in two cases, both because `event.key` alone
 * cannot be trusted to carry the modifier there:
 *   - `event.ctrlKey` is also held. Browsers suppress the ordinary
 *     shift-driven case change while Ctrl is down — `Ctrl+Shift+Z` still
 *     reports `key: "z"` (lowercase), the same as plain `Ctrl+Z` — so
 *     `shiftKey` is the ONLY signal distinguishing undo from redo, and it
 *     has to be read explicitly.
 *   - `event.key` is a NAMED, multi-character key (`Delete`, `F1`…`F12`,
 *     `ContextMenu`, arrows — `key.length > 1`). Named keys are never
 *     case-shifted, so `Shift+Delete` and plain `Delete` both report
 *     `key: "Delete"` — again, only `shiftKey` tells them apart.
 *
 * Every OTHER case — a single printable character with no Ctrl held (a
 * plain letter, or a shifted punctuation character like `?`) — is left
 * alone: the browser already folds Shift into `event.key` for those
 * (`Shift+/` arrives as `event.key === "?"`, `Shift+s` arrives as
 * `event.key === "S"`), so adding an explicit `shift+` prefix on top would
 * double-count the modifier and produce a combo (`"shift+?"`) nothing in
 * `SHORTCUTS` binds — and plain `s` is what `split` is bound to, not
 * `Shift+s` (`"S"` lower-cased is still `"s"`, so an unshifted `s` and an
 * accidental `Shift+s` deliberately collide onto the same binding here;
 * Rust's own split refusal at a clip boundary is the real guard against a
 * stray keypress, not this normalization).
 */
export function shortcutKey(event: KeyboardEvent): string {
  const key = event.key;
  const isNamedKey = key.length > 1;
  const includeShift = event.shiftKey && (event.ctrlKey || isNamedKey);
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("ctrl");
  if (includeShift) parts.push("shift");
  parts.push(key.toLowerCase());
  return parts.join("+");
}

/** The action id `event` is bound to, or `null` when it matches nothing. */
export function matchShortcut(event: KeyboardEvent): ActionId | null {
  return SHORTCUTS.get(shortcutKey(event)) ?? null;
}

/**
 * Whether a global shortcut dispatcher should act on `event` at all — the
 * gate every caller of `matchShortcut` must apply FIRST. False inside any
 * text-entry surface (a plain `<input>`/`<textarea>` or a `contenteditable`
 * region — typing "s" into a rename field must not split a clip), and false
 * whenever a menu/dialog has already claimed the keyboard
 * (`opts.menuOwnsKeys` — e.g. `ContextMenu.vue`'s own open popover, or the
 * legacy capture editor's surface described below) so two owners can never
 * both react to the same keypress.
 *
 * The LEGACY capture editor (`useEditorExport.ts`/`EditorRoot.vue`,
 * AGENTS.md "Frontend state") already binds Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y
 * on `window` for its own undo/redo. This task wires NO window listener at
 * all (see the module doc), so there is nothing here to collide with it
 * yet — `shouldHandle` exists so that whenever a later task DOES install a
 * tutorial-editor dispatcher, routing every keydown through this gate first
 * is what keeps the two surfaces from double-handling the same combo once
 * both exist. A dispatcher for the new surface must check `document.
 * activeElement`/its own mount surface before calling this, since which
 * surface owns the window at all is a wiring decision this module cannot
 * see.
 */
export function shouldHandle(event: KeyboardEvent, opts?: { menuOwnsKeys?: boolean }): boolean {
  if (opts?.menuOwnsKeys) return false;
  const target = event.target as HTMLElement | null;
  if (!target) return true;
  if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return false;
  if (target.isContentEditable) return false;
  return true;
}

/** Shift+F10 or the Menu/Application key — the two "open the context menu
 * here" bindings (SCREENS-AND-INTERACTIONS.md §03). Not in `SHORTCUTS`: it
 * names no `ActionId`, only a menu to open. */
export function isContextMenuShortcut(event: KeyboardEvent): boolean {
  return (event.key === "F10" && event.shiftKey) || event.key === "ContextMenu";
}
