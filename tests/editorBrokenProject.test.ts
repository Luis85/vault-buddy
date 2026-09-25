/**
 * Final whole-branch review I3: a tutorial project whose files no longer
 * parse could be neither opened (its load fails) nor discarded (the only
 * discard ran through a session, which needs an open). The editor window,
 * where the user meets the failed open, now offers to discard it through
 * `editor_discard_project` — only for a PROJECT that Rust reports as
 * damaged (`invalidProject`), never for a staged capture or a failure a
 * retry could fix.
 */
import { mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { EditorPortError } from "../src/editor/port";
import type { EditorErrorCode } from "../src/editorTypes";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";

enableAutoUnmount(afterEach);

beforeEach(() => {
  setActivePinia(createPinia());
});

function refusal(code: EditorErrorCode, message: string): EditorPortError {
  return new EditorPortError({ code, message, retryable: false, operationId: "op-1" });
}

/** Mount the window over one drained request whose open is refused. */
async function mountRefused(kind: "staged" | "project", code: EditorErrorCode, discarded: string[] = []) {
  const reject = () => Promise.reject(refusal(code, "The sources file <path:#1a2b3c4d> is not valid."));
  useEditorProjectStore().setPort(
    fakeEditorPort({
      openStaged: reject,
      openProject: reject,
      discardProject: (id) => {
        discarded.push(id);
        return Promise.resolve();
      },
    }),
  );
  let drained = false;
  mockIPC((cmd) => {
    if (cmd === "take_editor_request" && !drained) {
      drained = true;
      return { kind, value: kind === "project" ? "proj-broken" : "cap one" };
    }
    return null;
  });
  const w = mount(EditorRoot, { attachTo: document.body });
  await flushPromises();
  return w;
}

describe("a project that cannot be opened", () => {
  it("offers a confirmed discard for a damaged project and says when it is gone", async () => {
    const discarded: string[] = [];
    const w = await mountRefused("project", "invalidProject", discarded);
    expect(w.get('[data-testid="editor-open-failed"]').text()).toContain(
      "The sources file is not valid.",
    );

    await w.get('[data-testid="broken-project-discard"]').trigger("click");
    expect(discarded).toEqual([]);
    expect(w.get('[data-testid="broken-project-confirm"]').text()).toContain("Discard for good");
    await w.get('[data-testid="broken-project-confirm"]').trigger("click");
    await flushPromises();

    expect(discarded).toEqual(["proj-broken"]);
    expect(w.get('[data-testid="broken-project-discarded"]').text()).toContain("was discarded");
    expect(w.find('[data-testid="broken-project-discard"]').exists()).toBe(false);
  });

  it("Keep it backs out without discarding", async () => {
    const discarded: string[] = [];
    const w = await mountRefused("project", "invalidProject", discarded);
    await w.get('[data-testid="broken-project-discard"]').trigger("click");
    await w.get('[data-testid="broken-project-keep"]').trigger("click");
    expect(discarded).toEqual([]);
    expect(w.find('[data-testid="broken-project-discard"]').exists()).toBe(true);
  });

  it("says a refused discard in Rust's words and keeps the offer", async () => {
    const w = await mountRefused("project", "invalidProject");
    useEditorProjectStore().setPort(
      fakeEditorPort({
        discardProject: () =>
          Promise.reject(refusal("invalidProject", "The project file <path:#99aa00bb> is not valid.")),
      }),
    );
    await w.get('[data-testid="broken-project-discard"]').trigger("click");
    await w.get('[data-testid="broken-project-confirm"]').trigger("click");
    await flushPromises();
    const alert = w.get('[data-testid="broken-project-error"]').text();
    expect(alert).toContain("The project file is not valid.");
    expect(alert).not.toContain("<path:#");
    expect(w.find('[data-testid="broken-project-discard"]').exists()).toBe(true);
  });

  it("offers nothing for a staged capture or for a failure a retry could fix", async () => {
    for (const [kind, code] of [
      ["staged", "invalidProject"],
      ["project", "internal"],
      ["project", "sourceMissing"],
    ] as const) {
      setActivePinia(createPinia());
      const w = await mountRefused(kind, code);
      expect(w.find('[data-testid="editor-open-failed"]').exists()).toBe(true);
      expect(w.find('[data-testid="broken-project-discard"]').exists()).toBe(false);
      w.unmount();
    }
  });
});
