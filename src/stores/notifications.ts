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

export const useNotificationsStore = defineStore("notifications", {
  state: () => ({ items: [] as Notification[] }),
  actions: {
    notify(kind: NotifyKind, message: string, opts?: NotifyOpts): number {
      const last = this.items[this.items.length - 1];
      if (isRepeat(last, kind, message, opts)) return last!.id;
      const id = ++seq;
      this.items.push({ id, kind, message, action: opts?.action });
      if (this.items.length > MAX_ITEMS) this.items.splice(0, this.items.length - MAX_ITEMS);
      const ttlMs = ttlFor(kind, opts);
      if (ttlMs != null) setTimeout(() => this.dismiss(id), ttlMs);
      return id;
    },
    error(message: string) { return this.notify("error", message); },
    warning(message: string) { return this.notify("warning", message); },
    success(message: string) { return this.notify("success", message); },
    info(message: string) { return this.notify("info", message); },
    dismiss(id: number) { this.items = this.items.filter((i) => i.id !== id); },
    clear() { this.items = []; },
  },
});
