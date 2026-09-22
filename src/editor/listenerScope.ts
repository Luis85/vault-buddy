/**
 * An unmount-safe scope for Tauri event listeners (F31; the starter's
 * `listenerScope.ts` is the model, copied verbatim in shape). `listen(...)`
 * returns a `Promise<UnlistenFn>` — the registration itself is async, so a
 * component that calls `listen` on mount and disposes the scope before that
 * promise resolves must not leak the listener it never got a synchronous
 * handle to. `add` awaits the registration and, if `dispose` already ran by
 * the time it resolves, un-listens IMMEDIATELY instead of adding it to the
 * live set — the whole reason this exists rather than a plain
 * `Set<Unlisten>` a caller fills in directly.
 */
export type Unlisten = () => void;

export interface ListenerScope {
  /** Awaits `registration`; un-listens right away if the scope is already
   * disposed, otherwise tracks the listener for `dispose()`. */
  add(registration: Promise<Unlisten>): Promise<void>;
  /** Un-listens every tracked listener (and any still-in-flight
   * registration, as soon as it resolves) and marks the scope closed. */
  dispose(): void;
}

export function createListenerScope(): ListenerScope {
  let closed = false;
  const unlisteners = new Set<Unlisten>();
  return {
    async add(registration: Promise<Unlisten>): Promise<void> {
      const unlisten = await registration;
      if (closed) {
        unlisten();
      } else {
        unlisteners.add(unlisten);
      }
    },
    dispose(): void {
      closed = true;
      for (const unlisten of unlisteners) {
        try {
          unlisten();
        } catch {
          // One listener's teardown failing must not stop the rest from
          // being released — this loop is the last chance to release them.
        }
      }
      unlisteners.clear();
    },
  };
}
