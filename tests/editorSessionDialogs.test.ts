/**
 * Visual-parity Task 22 (concept spec §9.9–9.10): Discard project and the
 * Review dialog in the session dialogs' chrome — `SessionLead`,
 * `DialogButton`s and the reason the footer waits on screen (D14) — with
 * their behaviour kept: a discard never deletes the recording and a refused
 * one reattaches; a review in flight cannot be closed. Discard project's own
 * refusal is said in it and not toasted (ruling T7-1), while an unrelated
 * refusal still toasts.
 */
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import DiscardProjectDialog from "../src/components/editor/dialogs/DiscardProjectDialog.vue";
import ReviewDialog from "../src/components/editor/dialogs/ReviewDialog.vue";
import { EditorPortError } from "../src/editor/port";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { errorToasts, mountFeedback } from "./helpers/feedbackHost";
import { openResult, openWithRenders, progress } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

const refusal = (message: string) =>
  new EditorPortError({ code: "internal", message, retryable: true, operationId: "op" });

function footer(w: VueWrapper): string[] {
  return w.findAll("footer button").map((b) => b.text());
}

async function discardDialog(closeSession: () => Promise<void>, extra = {}) {
  await openWithRenders({ closeSession, openProject: () => Promise.resolve(openResult()), ...extra });
  const reattach = vi.fn(async (id: string) => {
    await useEditorProjectStore().openProject(id, false);
    return !useEditorProjectStore().lastError;
  });
  const w = mount(DiscardProjectDialog, { props: { open: true, reattach }, attachTo: document.body });
  await flushPromises();
  return { w, reattach };
}

describe("DiscardProjectDialog — the concept chrome", () => {
  it("leads with the trash tile, says the recording stays, and ends Cancel · Discard project", async () => {
    const { w } = await discardDialog(async () => {});
    const lead = w.get('[data-testid="discard-project-lead"]');
    expect(lead.get("h3").text()).toBe("The recording stays");
    expect(lead.text()).toContain("the recording stays in your staged captures");
    expect(footer(w)).toEqual(["Cancel", "Discard project"]);
    expect(w.get('[data-testid="discard-project-confirm"]').classes()).toContain("text-danger-fg");
  });

  it("with no project open, Discard project waits and says why on screen", async () => {
    const { w } = await discardDialog(async () => {});
    useEditorProjectStore().sessionId = null;
    await flushPromises();
    const confirm = w.get('[data-testid="discard-project-confirm"]');
    expect(confirm.attributes("title")).toBe("No project is open.");
    expect(w.get('[data-testid="discard-project-reason"]').text()).toBe("No project is open.");
  });

  it("while discarding, both buttons wait and say why on screen", async () => {
    const { w } = await discardDialog(() => new Promise(() => {}));
    await w.get('[data-testid="discard-project-confirm"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="discard-project-reason"]').text()).toBe("Discarding the project…");
    expect(w.get('[data-testid="discard-project-cancel"]').attributes("title")).toBe("Discarding the project…");
  });
});

describe("DiscardProjectDialog — its own refusal is inline", () => {
  it("a refused discard is said in the dialog, reattaches, and is not toasted", async () => {
    const { w, reattach } = await discardDialog(() => Promise.reject(refusal("The project folder is in use.")));
    mountFeedback();
    await w.get('[data-testid="discard-project-confirm"]').trigger("click");
    await flushPromises();
    expect(reattach).toHaveBeenCalledWith("project-r");
    expect(w.get('[data-testid="discard-project-dialog"] [role="alert"]').text()).toBe(
      "The project could not be discarded. The project folder is in use.",
    );
    expect(errorToasts()).toEqual([]);
  });

  // The reattach's own refusal belongs to the same choice: the dialog says
  // the project could not be reopened, and the shell does not toast it too.
  it("a refused discard whose reattach also fails is said once, in the dialog", async () => {
    const { w } = await discardDialog(() => Promise.reject(refusal("The project folder is in use.")), {
      openProject: () => Promise.reject(refusal("The project could not be opened.")),
    });
    mountFeedback();
    await w.get('[data-testid="discard-project-confirm"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="discard-project-dialog"] [role="alert"]').text()).toContain(
      "it could not be reopened here",
    );
    expect(errorToasts()).toEqual([]);
  });

  it("an unrelated refusal while the dialog is open still toasts", async () => {
    await discardDialog(async () => {}, { execute: () => Promise.reject(refusal("Elsewhere.")) });
    mountFeedback();
    await useEditorProjectStore().execute({ kind: "rename", title: "x" });
    await flushPromises();
    expect(errorToasts()).toEqual(["Elsewhere."]);
  });
});

describe("ReviewDialog — the concept chrome", () => {
  it("while the start is in flight, the footer says why nothing can close yet", async () => {
    await openWithRenders({ startRender: () => new Promise(() => {}) });
    const w = mount(ReviewDialog, { props: { open: true, range: { startMs: 0, endMs: 5_000 } } });
    await flushPromises();
    expect(w.get('[data-testid="review-dialog-reason"]').text()).toBe("Starting the review…");
    expect(footer(w)).toEqual([]);
  });

  it("a running review offers Cancel review; a finished one Close", async () => {
    const env = await openWithRenders();
    const w = mount(ReviewDialog, { props: { open: true, range: { startMs: 0, endMs: 5_000 } } });
    await flushPromises();
    env.deliver(0, progress("job-0", { sequence: 1, phase: "rendering", fraction: 0.4 }));
    await flushPromises();
    expect(footer(w)).toEqual(["Cancel review"]);
    expect(w.get('[data-testid="review-dialog-reason"]').text()).toBe("A review is running.");
    env.deliver(0, progress("job-0", { sequence: 2, phase: "cancelled", fraction: 0.4, terminal: {} }));
    await flushPromises();
    expect(footer(w)).toEqual(["Close"]);
    await w.get('[data-testid="review-dialog-done"]').trigger("click");
    expect(w.emitted("close")).toHaveLength(1);
  });
});
