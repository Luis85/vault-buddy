import type { Ref } from "vue";
import { onBeforeUnmount, onMounted, ref } from "vue";

/**
 * A ref that re-samples `sample()` on a fixed interval while the owning
 * component is mounted — never at frame rate. `MixerPeakMeter.vue` (Task
 * 27) and the transport's own peak bar (visual-parity Task 12) both poll
 * `PreviewController.readPeak()` this way; pulling the mount/unmount
 * interval out here is what keeps a second copy of it from becoming a
 * clone the quality ratchet would catch.
 */
export function usePolledValue<T>(sample: () => T, intervalMs: number): Ref<T> {
  const value = ref(sample()) as Ref<T>;
  let timer: ReturnType<typeof setInterval> | null = null;
  onMounted(() => {
    value.value = sample();
    timer = setInterval(() => {
      value.value = sample();
    }, intervalMs);
  });
  onBeforeUnmount(() => {
    if (timer !== null) clearInterval(timer);
  });
  return value;
}
