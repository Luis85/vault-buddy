import { onBeforeUnmount, onMounted, ref, watch } from "vue";

/**
 * A wall-clock `now` ref that ticks once per second — the shared driver
 * behind every live elapsed-time display (RecordingBar, Transcriptions,
 * ImportProgress, ScreenCaptureBar), so a new one doesn't grow a fourth copy.
 *
 * The interval is bound to the component AND, where a call site passes
 * `enabled`, to whether there is anything to tick for. That second half
 * exists because the first stopped being enough: since phase 4
 * `ScreenCaptureBar` stays mounted once a capture has FINISHED (idle,
 * offering the staged capture's Edit action), and the panel window is hidden
 * rather than unmounted — so an ungated interval ran at 1 Hz for the rest of
 * the process, updating a value that state renders nowhere. `enabled` is a
 * getter, not a ref, so a call site can pass an expression; omitting it keeps
 * the always-on behaviour the other three call sites rely on.
 */
export function useNowTicker(enabled?: () => boolean) {
  const now = ref(Date.now());
  let timer: ReturnType<typeof setInterval> | null = null;
  const wanted = enabled ?? (() => true);

  function sync(on: boolean) {
    if (timer !== null) clearInterval(timer);
    // Re-read on the way in: `now` went stale while the ticker was off, and a
    // display that has just gone live must not first render the moment its
    // component happened to mount.
    if (on) now.value = Date.now();
    timer = on ? setInterval(() => (now.value = Date.now()), 1000) : null;
  }

  onMounted(() => sync(wanted()));
  watch(wanted, sync);
  onBeforeUnmount(() => sync(false));
  return now;
}
