import { onMounted, onUnmounted, ref, type Ref } from "vue";
import { greetingFor } from "../greeting";

// How long the greeting bubble stays before auto-dismissing.
export const GREETING_MS = 5000;

/**
 * Shows a one-shot greeting bubble on mount (app launch) and auto-dismisses
 * it after GREETING_MS. `dismiss()` lets a caller (App, when the panel
 * opens) cancel the timer and hide the bubble immediately. Shows once per
 * mount — a single-instance reveal of an already-running app does not
 * remount the frontend, so it does not re-greet.
 */
export function useGreeting(): {
  bubbleVisible: Ref<boolean>;
  bubbleText: Ref<string>;
  dismiss: () => void;
} {
  const bubbleVisible = ref(false);
  const bubbleText = ref("");
  let timer: ReturnType<typeof setTimeout> | undefined;

  function dismiss() {
    clearTimeout(timer);
    timer = undefined;
    bubbleVisible.value = false;
  }

  onMounted(() => {
    bubbleText.value = greetingFor(new Date());
    bubbleVisible.value = true;
    timer = setTimeout(dismiss, GREETING_MS);
  });

  onUnmounted(() => {
    clearTimeout(timer);
  });

  return { bubbleVisible, bubbleText, dismiss };
}
