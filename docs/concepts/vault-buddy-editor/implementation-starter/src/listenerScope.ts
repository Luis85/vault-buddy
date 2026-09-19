export type Unlisten = () => void;
/** Release even listeners whose asynchronous registration resolves after unmount. */
export function createListenerScope() {
  let closed = false;
  const unlisteners = new Set<Unlisten>();
  return {
    async add(registration: Promise<Unlisten>): Promise<void> {
      const unlisten = await registration;
      if (closed) unlisten(); else unlisteners.add(unlisten);
    },
    dispose(): void {
      closed = true;
      for (const unlisten of unlisteners) { try { unlisten(); } catch { /* Other listeners must still be released. */ } }
      unlisteners.clear();
    },
  };
}
