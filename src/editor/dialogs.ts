/**
 * The tutorial editor's dialog stack (Task 19; F-48/F-49;
 * ARCHITECTURE-AND-STACK.md "ContextMenu / DialogHost / LearningCenter /
 * GuideOverlay"; ONBOARDING.md: "A modal dialog suspends the coach; closing
 * it resumes the same lesson and restores appropriate focus").
 *
 * A tiny, typed, module-level stack of open `DialogHost` instances — NOT a
 * Pinia store, because nothing here is project or workspace state. It
 * exists only so more than one `DialogHost` mounted at once (a confirm
 * opened from inside a bigger dialog, e.g. a discard prompt over a Render
 * dialog) can agree on which of them is topmost: focus-trap and Escape
 * belong to the TOP entry alone, so an outer dialog's own keydown listener
 * never races an inner one's over the same keypress.
 *
 * `stack` is a plain module-level `ref` — every `DialogHost` instance
 * imports the SAME array, so pushing from one component and reading
 * `isTopDialog` from another sees the same state without any provide/inject
 * plumbing (a dialog can open from anywhere in the tree, not only from a
 * parent of the host that should suspend).
 */
import { ref } from "vue";

/** One open dialog's stack membership: enough for `DialogHost` instances to
 * agree on ordering and on whether Escape/backdrop may close each other. */
export interface DialogStackEntry {
  id: string;
  /** Whether Escape/backdrop may close THIS entry — a dialog mid an
   * irreversible operation (e.g. a render in flight) passes `false` and
   * offers its own explicit way out instead. */
  closable: boolean;
}

const stack = ref<DialogStackEntry[]>([]);

let seq = 0;
/** Mints a stack id, unique for the life of this module — never reused, so
 * a late callback from an already-closed dialog can never be mistaken for
 * a live one still on the stack. */
export function nextDialogId(): string {
  seq += 1;
  return `dialog-${seq}`;
}

export function pushDialog(entry: DialogStackEntry): void {
  stack.value.push(entry);
}

/** No-op when `id` isn't on the stack — a dialog that deactivates twice
 * (e.g. `open` flips to `false` the same tick its parent unmounts it) must
 * not throw. */
export function popDialog(id: string): void {
  const i = stack.value.findIndex((e) => e.id === id);
  if (i !== -1) stack.value.splice(i, 1);
}

/** The id of the entry a `DialogHost` should treat as active — the LAST
 * pushed, matching the visual stacking order (a later-opened dialog draws
 * on top and answers input first). `null` when nothing is open. Not
 * exported: `isTopDialog` is the one thing every caller actually needs, and
 * an unused second entry point is exactly the kind of export the dead-code
 * gate exists to catch. */
function topDialogId(): string | null {
  return stack.value.length > 0 ? stack.value[stack.value.length - 1].id : null;
}

export function isTopDialog(id: string): boolean {
  return topDialogId() === id;
}

/** Test-only observation point — nothing in production reads this; it
 * exists so a test can assert the stack unwinds correctly rather than
 * poking at a component's private state. */
export function dialogStackSize(): number {
  return stack.value.length;
}
