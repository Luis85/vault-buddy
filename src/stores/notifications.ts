import { defineStore } from "pinia";

export type NotifyKind = "error" | "warning" | "success" | "info";
/** An optional call-to-action on a toast (e.g. "Open" the just-imported note).
 * `run` is invoked on click, then the toast is dismissed. */
export interface NotifyAction { label: string; run: () => void | Promise<void>; }
export interface Notification { id: number; kind: NotifyKind; message: string; action?: NotifyAction; }

/** Newest failure must not be pushed off by a burst; small cap keeps the panel readable. */
const MAX_ITEMS = 5;
const DEFAULT_TTL: Record<NotifyKind, number | null> = {
  error: null, warning: null, success: 4000, info: 4000,
};

let seq = 0; // monotonic id source — no Date.now needed
// Live setTimeout handles keyed by notification id, so a dedupe-reuse can
// restart the TTL and a dismiss can cancel a still-pending one (GAP-32:
// neither used to happen — a re-raise moments before expiry read as
// flicker, and a manually-dismissed id's timer still fired a no-op dismiss
// later).
const timers = new Map<number, ReturnType<typeof setTimeout>>();

interface NotifyOpts { ttlMs?: number | null; action?: NotifyAction; }

// Dedupe a retried command spamming the same line: the newest identical
// kind+message is reused. An actionable toast is never deduped — two imports
// that yield the same message carry different callbacks, and collapsing them
// would leave the second's "Open" pointing at the first note.
function isRepeat(last: Notification | undefined, kind: NotifyKind, message: string, opts?: NotifyOpts): boolean {
  return !opts?.action && !!last && last.kind === kind && last.message === message;
}

// An actionable toast must wait for the user's decision, so it defaults to
// sticky (no auto-dismiss) rather than the kind's normal TTL — otherwise a
// success "Open" would vanish on the 4s timer before it could be clicked. An
// explicit ttlMs always wins.
function ttlFor(kind: NotifyKind, opts?: NotifyOpts): number | null {
  if (opts?.ttlMs !== undefined) return opts.ttlMs;
  return opts?.action ? null : DEFAULT_TTL[kind];
}

// A TTL expiry, deliberately NOT the `dismiss` action: Pinia's action
// wrapper calls `setActivePinia` with the store's own pinia, so a timer that
// outlives its pinia (every test's `setActivePinia(createPinia())` reset)
// would re-activate the old one and every later `useXStore()` would read the
// stale stores. That made a success toast from one test swap the stores out
// from under an assertion several tests later.
function expire(store: { items: Notification[] }, id: number): void {
  timers.delete(id);
  store.items = store.items.filter((i) => i.id !== id);
}

export const useNotificationsStore = defineStore("notifications", {
  state: () => ({ items: [] as Notification[] }),
  actions: {
    notify(kind: NotifyKind, message: string, opts?: NotifyOpts): number {
      const ttlMs = ttlFor(kind, opts);
      const last = this.items[this.items.length - 1];
      if (isRepeat(last, kind, message, opts)) {
        // GAP-32: reusing the newest identical toast must also restart its
        // TTL — a re-raise moments before expiry otherwise reads as flicker.
        const t = timers.get(last!.id);
        if (t) clearTimeout(t);
        if (ttlMs != null) timers.set(last!.id, setTimeout(() => expire(this, last!.id), ttlMs));
        return last!.id;
      }
      const id = ++seq;
      this.items.push({ id, kind, message, action: opts?.action });
      if (this.items.length > MAX_ITEMS) this.items.splice(0, this.items.length - MAX_ITEMS);
      if (ttlMs != null) timers.set(id, setTimeout(() => expire(this, id), ttlMs));
      return id;
    },
    error(message: string) { return this.notify("error", message); },
    warning(message: string) { return this.notify("warning", message); },
    success(message: string) { return this.notify("success", message); },
    info(message: string) { return this.notify("info", message); },
    dismiss(id: number) {
      const t = timers.get(id);
      if (t) clearTimeout(t);
      expire(this, id);
    },
    clear() {
      for (const t of timers.values()) clearTimeout(t);
      timers.clear();
      this.items = [];
    },
  },
});
