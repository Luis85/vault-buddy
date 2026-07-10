import { onMounted, onUnmounted, type Ref,ref } from "vue";

import { greetingFor } from "../greeting";
import { type MessageDuration,useSettingsStore } from "../stores/settings";

// How long a message stays before auto-dismissing, per the user's
// messageDuration setting. The launch greeting lingers; action
// acknowledgements are quicker so a burst of them never piles up. `normal`
// is the pre-setting behavior and must stay byte-identical to it.
export const BUBBLE_MS: Record<MessageDuration, { ack: number; greeting: number }> = {
  short: { ack: 2000, greeting: 3000 },
  normal: { ack: 3200, greeting: 5000 },
  long: { ack: 6000, greeting: 9000 },
};

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
  const settings = useSettingsStore();
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

  onMounted(() =>
    show(greetingFor(new Date()), BUBBLE_MS[settings.messageDuration].greeting),
  );
  onUnmounted(() => clearTimeout(timer));

  return { visible, text, show, dismiss };
}
