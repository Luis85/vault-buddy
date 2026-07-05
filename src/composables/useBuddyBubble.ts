import { onMounted, onUnmounted, ref, type Ref } from "vue";
import { greetingFor } from "../greeting";

// How long a message stays before auto-dismissing. The launch greeting lingers;
// action acknowledgements are quicker so a burst of them never piles up.
export const GREETING_MS = 5000;
export const ACK_MS = 3200;

/**
 * The buddy's single speech-bubble channel: one current message + one
 * auto-dismiss timer. `show` is latest-wins — a new message replaces the
 * current text and restarts the timer, so rapid events never stack or queue.
 * The launch greeting is simply the first message through the channel; every
 * later acknowledgement (see BubbleRoot's `bubble-message` listener) is another
 * `show` call.
 */
export function useBuddyBubble(): {
  visible: Ref<boolean>;
  text: Ref<string>;
  show: (message: string, durationMs: number) => void;
  dismiss: () => void;
} {
  const visible = ref(false);
  const text = ref("");
  let timer: ReturnType<typeof setTimeout> | undefined;

  function dismiss() {
    clearTimeout(timer);
    timer = undefined;
    visible.value = false;
  }

  function show(message: string, durationMs: number) {
    text.value = message;
    visible.value = true;
    clearTimeout(timer);
    timer = setTimeout(dismiss, durationMs);
  }

  onMounted(() => show(greetingFor(new Date()), GREETING_MS));
  onUnmounted(() => clearTimeout(timer));

  return { visible, text, show, dismiss };
}
