/**
 * The ONE way a disabled editor control says why it did nothing (design
 * D14; final review, minor 7). Every `aria-disabled` control stays
 * focusable, so a keyboard user can press it; pressing it — a click, Enter,
 * Space, a shortcut — toasts its reason here. A held key repeats the
 * activation, so each reason is said at most once every 1.5 s — per
 * reason, so a DIFFERENT refusal a moment later still speaks.
 *
 * The clock readings are kept per notifications store (per Pinia): each
 * editor window, and each test, has its own. No timers (AGENTS.md's
 * Pinia-timer rule): the limit compares `Date.now()` readings.
 */
import { useNotificationsStore } from "../stores/notifications";

/** At most one toast per reason per this many milliseconds. */
const DISABLED_REASON_INTERVAL_MS = 1_500;

const lastSaidAt = new WeakMap<object, Map<string, number>>();

/** Say why a control did nothing — each reason rate-limited on its own. */
export function announceDisabled(reason: string | null | undefined): void {
  if (!reason) return;
  const notifications = useNotificationsStore();
  let said = lastSaidAt.get(notifications);
  if (!said) {
    said = new Map();
    lastSaidAt.set(notifications, said);
  }
  const now = Date.now();
  if (now - (said.get(reason) ?? Number.NEGATIVE_INFINITY) < DISABLED_REASON_INTERVAL_MS) return;
  said.set(reason, now);
  notifications.info(reason);
}
