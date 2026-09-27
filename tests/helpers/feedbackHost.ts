import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";

import { useEditorFeedback } from "../../src/composables/useEditorFeedback";
import { useNotificationsStore } from "../../src/stores/notifications";

/**
 * The shell's one feedback watcher (`useEditorFeedback`), mounted on its
 * own — what toasts a refusal no dialog claims (ruling T7-1). Shared by the
 * suites that prove a dialog's own refusal is shown inline and NOT toasted,
 * while an unrelated one still is.
 */
const FeedbackHost = defineComponent({
  setup() {
    useEditorFeedback();
    return () => h("div");
  },
});

export function mountFeedback(): void {
  mount(FeedbackHost);
}

/** The error toasts' messages, oldest first. */
export function errorToasts(): string[] {
  return useNotificationsStore()
    .items.filter((n) => n.kind === "error")
    .map((n) => n.message);
}
