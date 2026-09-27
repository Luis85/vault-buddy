/**
 * Task 21 fix round 2 (Ruling T21-5): Save a copy OWNS the revision
 * conflict its own rename meets. The conflict is detected POSITIVELY —
 * `conflictIntent` parked by that request — never inferred from the
 * absence of an inline error, so a refusal that is not a conflict (no open
 * session) never claims "the project changed". While the dialog owns it,
 * the shell's conflict toast is not raised and the parked rename is
 * dropped, so no Retry can complete the rename without the copy; the
 * dialog's own Save copy reruns the WHOLE save. A conflict raised with the
 * dialog closed toasts with Retry exactly as before.
 *
 * Also the unmount-mid-request race of `useInlineLastError`, and Render's
 * two footer reasons kept apart.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { defineComponent, h } from "vue";

import RenderDialog from "../src/components/editor/dialogs/RenderDialog.vue";
import SaveProjectDialog from "../src/components/editor/dialogs/SaveProjectDialog.vue";
import { useEditorFeedback } from "../src/composables/useEditorFeedback";
import { lastErrorShownInline, useInlineLastError } from "../src/composables/useInlineLastError";
import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import type { PackageReceipt } from "../src/editorTypes";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useNotificationsStore } from "../src/stores/notifications";
import { openResult, openWithRenders, SESSION } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

const FeedbackHost = defineComponent({
  setup() {
    useEditorFeedback();
    return () => h("div");
  },
});

const toasts = (kind: string) =>
  useNotificationsStore()
    .items.filter((n) => n.kind === kind)
    .map((n) => n.message);

const conflictError = () =>
  new EditorPortError({ code: "revisionConflict", message: "changed", retryable: true, operationId: "o" });

function receipt(revision: number): PackageReceipt {
  return { sessionId: SESSION, savedRevision: revision, fileName: "Atlas.vbproject.zip", format: "portable" };
}

/** A session whose first `execute` meets a conflict and whose later ones
 * land; every request is recorded. */
async function openConflicting() {
  const commands: unknown[] = [];
  let calls = 0;
  const execute = vi.fn((req: { expectedRevision: number; command: unknown }) => {
    commands.push(req.command);
    calls += 1;
    if (calls === 1) return Promise.reject(conflictError());
    return Promise.resolve({
      snapshot: { ...openResult().snapshot, revision: req.expectedRevision + 1, title: "Atlas" },
      project: openResult().project,
    });
  });
  const exportPackage = vi.fn((_s: string, revision: number) => Promise.resolve(receipt(revision)));
  await openWithRenders({
    execute: execute as unknown as EditorPort["execute"],
    getSnapshot: () => Promise.resolve(openResult()),
    exportPackage,
  } as Partial<EditorPort>);
  mount(FeedbackHost);
  return { commands, exportPackage };
}

async function saveAs(name: string) {
  const w = mount(SaveProjectDialog, { props: { open: true, initialFormat: "portable" }, attachTo: document.body });
  await flushPromises();
  await w.get('[data-testid="save-project-name"]').setValue(name);
  await w.get('[data-testid="save-project-confirm"]').trigger("click");
  await flushPromises();
  return w;
}

describe("Save a copy owns its rename's conflict (Ruling T21-5)", () => {
  it("a conflict inside the dialog says so, raises no shell toast, and leaves nothing parked", async () => {
    await openConflicting();
    // Every toast ever raised, not only those left standing: a toast raised
    // and then dismissed would still have flashed a Retry.
    const notify = vi.spyOn(useNotificationsStore(), "notify");
    const w = await saveAs("Atlas");
    expect(w.get('[data-testid="save-project-status"]').text()).toBe(
      "The project changed while saving the copy. Try again.",
    );
    expect(notify.mock.calls.filter(([kind]) => kind === "warning")).toEqual([]);
    expect(useEditorProjectStore().conflictIntent).toBeNull();
  });

  it("the dialog's Save copy after a conflict reruns the whole save: the rename and the copy", async () => {
    const { commands, exportPackage } = await openConflicting();
    const w = await saveAs("Atlas");
    expect(exportPackage).not.toHaveBeenCalled();
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    expect(commands).toEqual([
      { kind: "rename", title: "Atlas" },
      { kind: "rename", title: "Atlas" },
    ]);
    expect(exportPackage).toHaveBeenCalledTimes(1);
    expect(w.get('[data-testid="save-project-status"]').text()).toBe("Saved to Atlas.vbproject.zip");
  });

  it("a conflict raised while the dialog is closed still toasts with Retry", async () => {
    await openConflicting();
    const w = mount(SaveProjectDialog, { props: { open: false, initialFormat: "portable" } });
    await flushPromises();
    await useEditorProjectStore().execute({ kind: "rename", title: "elsewhere" });
    await flushPromises();
    expect(toasts("warning")).toEqual(["Your edit wasn't applied because the project changed. Retry?"]);
    expect(useEditorProjectStore().conflictIntent).toEqual({ kind: "rename", title: "elsewhere" });
    w.unmount();
  });

  // Finding 1: detected positively — a refusal that is not a conflict (no
  // session: `acknowledged` returns false, setting neither `lastError` nor
  // `conflictIntent`) never says the project changed.
  it("a session-less refusal reads neutrally, never as a conflict", async () => {
    await openConflicting();
    const w = mount(SaveProjectDialog, { props: { open: true, initialFormat: "portable" }, attachTo: document.body });
    await flushPromises();
    useEditorProjectStore().$patch({ sessionId: null });
    await w.get('[data-testid="save-project-name"]').setValue("Atlas");
    await w.get('[data-testid="save-project-confirm"]').trigger("click");
    await flushPromises();
    const status = w.get('[data-testid="save-project-status"]').text();
    expect(status).toBe("The copy was not saved.");
    expect(status).not.toMatch(/changed/);
  });
});

describe("useInlineLastError: unmounted mid-request (minor 3)", () => {
  it("returns the shared claim to zero, and a later unrelated error toasts", async () => {
    let settle!: () => void;
    await openWithRenders({
      execute: () =>
        Promise.reject(new EditorPortError({ code: "invalidRequest", message: "Later.", retryable: false, operationId: "o" })),
    });
    mount(FeedbackHost);
    const Surface = defineComponent({
      setup() {
        const inline = useInlineLastError(undefined, { ownConflicts: true });
        void inline.track(() => new Promise<void>((resolve) => (settle = resolve)));
        return () => h("p");
      },
    });
    const w = mount(Surface);
    expect(lastErrorShownInline()).toBe(true);
    w.unmount();
    expect(lastErrorShownInline()).toBe(false);
    settle();
    await flushPromises();
    expect(lastErrorShownInline()).toBe(false);
    await useEditorProjectStore().execute({ kind: "rename", title: "x" });
    await flushPromises();
    expect(toasts("error")).toEqual(["Later."]);
  });
});

describe("Render's footer keeps its two reasons apart (minor 4)", () => {
  it("while a start is in flight only the start reason speaks; while saving, only the save reason", async () => {
    await openWithRenders({
      startRender: () => new Promise<never>(() => {}),
      save: () => new Promise<never>(() => {}),
      getProducts: () => Promise.resolve([]),
    });
    const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
    await flushPromises();
    const start = () => w.get('[data-testid="render-dialog-start-reason"]').text();
    const save = () => w.get('[data-testid="render-dialog-save-reason"]').text();
    void useEditorProjectStore().save();
    await flushPromises();
    expect(save()).toBe("Saving…");
    expect(start()).toBe("");
  });

  it("a start in flight is said once, by the start reason", async () => {
    await openWithRenders({ startRender: () => new Promise<never>(() => {}), getProducts: () => Promise.resolve([]) });
    const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
    await flushPromises();
    await w.get('[data-testid="render-dialog-start"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="render-dialog-start-reason"]').text()).toBe("Starting the render…");
    expect(w.get('[data-testid="render-dialog-save-reason"]').text()).toBe("");
  });
});
