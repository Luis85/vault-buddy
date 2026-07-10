import { defineStore } from "pinia";

export type NotifyKind = "error" | "warning" | "success" | "info";
export interface Notification { id: number; kind: NotifyKind; message: string; }

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

export const useNotificationsStore = defineStore("notifications", {
  state: () => ({ items: [] as Notification[] }),
  actions: {
    notify(kind: NotifyKind, message: string, opts?: { ttlMs?: number | null }): number {
      // Dedupe a retried command spamming the same line: if the newest item is
      // an identical kind+message, reuse it rather than stacking duplicates.
      const ttlOf = (k: NotifyKind) => (opts?.ttlMs !== undefined ? opts.ttlMs : DEFAULT_TTL[k]);
      const last = this.items[this.items.length - 1];
      if (last && last.kind === kind && last.message === message) {
        // GAP-32: reusing the newest identical toast must also restart its
        // TTL — a re-raise moments before expiry otherwise reads as flicker.
        const ttlMs = ttlOf(kind);
        const t = timers.get(last.id);
        if (t) clearTimeout(t);
        if (ttlMs != null) timers.set(last.id, setTimeout(() => this.dismiss(last.id), ttlMs));
        return last.id;
      }
      const id = ++seq;
      this.items.push({ id, kind, message });
      if (this.items.length > MAX_ITEMS) this.items.splice(0, this.items.length - MAX_ITEMS);
      const ttlMs = ttlOf(kind);
      if (ttlMs != null) timers.set(id, setTimeout(() => this.dismiss(id), ttlMs));
      return id;
    },
    error(message: string) { return this.notify("error", message); },
    warning(message: string) { return this.notify("warning", message); },
    success(message: string) { return this.notify("success", message); },
    info(message: string) { return this.notify("info", message); },
    dismiss(id: number) {
      const t = timers.get(id);
      if (t) clearTimeout(t);
      timers.delete(id);
      this.items = this.items.filter((i) => i.id !== id);
    },
    clear() {
      for (const t of timers.values()) clearTimeout(t);
      timers.clear();
      this.items = [];
    },
  },
});
