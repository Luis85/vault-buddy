import { onBeforeUnmount, onMounted, ref } from "vue";

/**
 * A wall-clock `now` ref that ticks once per second while the component is
 * mounted — the shared driver behind every live elapsed-time display
 * (RecordingBar, Transcriptions, ImportProgress). One place instead of the
 * three identical inline tickers those components used to carry, so a future
 * elapsed display doesn't grow a fourth copy. Interval lifecycle is bound to
 * the component: call sites that only mount while work is running (all of
 * them today) therefore only tick while work is running.
 */
export function useNowTicker() {
  const now = ref(Date.now());
  let timer: ReturnType<typeof setInterval> | null = null;
  onMounted(() => {
    timer = setInterval(() => (now.value = Date.now()), 1000);
  });
  onBeforeUnmount(() => {
    if (timer) clearInterval(timer);
  });
  return now;
}
