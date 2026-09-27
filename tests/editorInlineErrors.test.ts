/**
 * Ruling T7-1 (visual-parity Task 21): `useEditorFeedback` toasts every new
 * `editorProject.lastError`, so a dialog that ALSO renders that error inline
 * showed it twice. A surface that renders `lastError` inline claims it
 * (`useInlineLastError`) while it is on screen: an error raised then is
 * shown there and not toasted — one visible surface, never zero — and the
 * same error with no such surface open toasts as before.
 *
 * The claim is general, never a per-dialog special case inside the
 * composable: the Checks dialog's destination picker and the Save a copy
 * dialog's rename both use it here, and Task 22 reuses it.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { defineComponent, h, ref } from "vue";

import ChecksDestination from "../src/components/editor/dialogs/ChecksDestination.vue";
import SaveProjectDialog from "../src/components/editor/dialogs/SaveProjectDialog.vue";
import { useEditorFeedback } from "../src/composables/useEditorFeedback";
import { useInlineLastError } from "../src/composables/useInlineLastError";
import { EditorPortError } from "../src/editor/port";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useNotificationsStore } from "../src/stores/notifications";
import { openWithRenders } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

const refusal = (message: string) =>
  Promise.reject(new EditorPortError({ code: "invalidRequest", message, retryable: false, operationId: "op" }));

/** The shell's one feedback watcher, mounted on its own. */
const FeedbackHost = defineComponent({
  setup() {
    useEditorFeedback();
    return () => h("div");
  },
});

const errorToasts = () => useNotificationsStore().items.filter((n) => n.kind === "error").map((n) => n.message);

async function open() {
  let message = "That vault is gone.";
  const env = await openWithRenders({
    listVaults: () => Promise.resolve([{ id: "vault-1", name: "Notes" }]),
    execute: () => refusal(message),
  });
  mount(FeedbackHost);
  return { ...env, setMessage: (m: string) => (message = m) };
}

describe("an error shown inline is not also toasted (T7-1)", () => {
  it("the Checks destination picker shows its refusal inline and no toast", async () => {
    await open();
    const w = mount(ChecksDestination);
    await flushPromises();
    await w.get('[data-testid="checks-destination-vault"]').setValue("vault-1");
    await w.get('[data-testid="checks-destination-save"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="checks-destination-error"]').text()).toBe("That vault is gone.");
    expect(errorToasts()).toEqual([]);
  });

  it("the same refusal with no inline surface open toasts", async () => {
    const { setMessage } = await open();
    const w = mount(ChecksDestination);
    await flushPromises();
    w.unmount();
    setMessage("That vault is gone again.");
    await useEditorProjectStore().execute({ kind: "rename", title: "x" });
    await flushPromises();
    expect(errorToasts()).toEqual(["That vault is gone again."]);
  });

  it("an error raised while the surface is inactive is not claimed", async () => {
    await open();
    const active = ref(false);
    const Surface = defineComponent({
      setup() {
        const inline = useInlineLastError(() => active.value);
        return () => h("p", inline.error.value?.message ?? "");
      },
    });
    const w = mount(Surface);
    await useEditorProjectStore().execute({ kind: "rename", title: "x" });
    await flushPromises();
    expect(w.text()).toBe("");
    expect(errorToasts()).toEqual(["That vault is gone."]);
  });

  it("the Save a copy dialog's refused rename is its status line, not a toast", async () => {
    await open();
    const w = mount(SaveProjectDialog, { props: { open: true, initialFormat: "portable" }, attachTo: document.body });
    await flushPromises();
    await w.get('[data-testid="save-project-name"]').setValue("Atlas");
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    const status = w.get('[data-testid="save-project-status"]');
    expect(status.attributes("role")).toBe("alert");
    expect(status.text()).toBe("The copy was not saved. That vault is gone.");
    expect(errorToasts()).toEqual([]);
  });
});
