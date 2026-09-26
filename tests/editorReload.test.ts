/**
 * A reload must not orphan the open project (hardening Task 15; review
 * finding I-3, GAP-208; decision D1-a).
 *
 * WebView2's browser accelerator keys are on by default, so F5 / Ctrl+R
 * reload the editor webview in ANY build, and Ctrl+F / Ctrl+P open the
 * browser's own find bar and print dialog. A reload drops the webview's
 * stores and every job Channel while Rust keeps the session — and any render
 * it runs — alive. So:
 *   - the editor window swallows those keys (`preventDefault`, no new
 *     dependency — option D1-b, WebView2's own setting, was not chosen);
 *   - a reload that still happens (a devtools build) reopens the project on
 *     screen: `take_editor_request` hands it back when nothing new is
 *     stashed (Rust's `RequestSlots`, pinned in `editor_commands_tests.rs`),
 *     and Rust reuses the live session;
 *   - a render still running is found through `editor_get_jobs` and its
 *     progress shown by polling it once a second until its terminal — its
 *     Channel is gone.
 */
import { mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const listeners: Record<string, (e?: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e?: { payload: unknown }) => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import type { EditorPort } from "../src/editor/port";
import { completionText } from "../src/editor/renderProgress";
import type { JobRecordDto } from "../src/editorTypes";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { openResult } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const key of Object.keys(listeners)) delete listeners[key];
  mockConvertFileSrc("windows");
  setActivePinia(createPinia());
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

type Request = { kind: "staged" | "project"; value: string };

/** Rust's `RequestSlots`, as the webview sees it through
 * `take_editor_request`: a stashed request drains once, then the request on
 * screen comes back — replaced by every successful open's project. */
function rustStash(first: Request | null) {
  let pending = first;
  let current: Request | null = null;
  mockIPC((cmd) => {
    if (cmd !== "take_editor_request") return undefined;
    if (pending) {
      current = pending;
      pending = null;
    }
    return current;
  });
  return {
    opened(projectId: string) {
      current = { kind: "project", value: projectId };
    },
  };
}

/** A port over the render fixtures' session whose open REUSES it, the way
 * Rust's open paths reuse a live session, and records every open. */
function reusingPort(rust: ReturnType<typeof rustStash>, extra: Partial<EditorPort> = {}) {
  const opened: string[] = [];
  const port = fakeEditorPort({
    openProject: (id) => {
      opened.push(id);
      rust.opened(id);
      return Promise.resolve(openResult());
    },
    getJobs: () => Promise.resolve([]),
    getChecks: () => Promise.resolve([]),
    getProducts: () => Promise.resolve([]),
    ...extra,
  });
  return { port, opened };
}

async function mountRoot(port: EditorPort, stubs: Record<string, boolean> = {}) {
  useEditorProjectStore().setPort(port);
  const w = mount(EditorRoot, { attachTo: document.body, global: { stubs } });
  await flushPromises();
  return w;
}

describe("a reload of the editor webview", () => {
  // Frontend-green by construction — the RED half of this contract is Rust's
  // (`a_drain_with_nothing_new_hands_back_the_request_on_screen`): before
  // Task 15 the second drain answered `null` and this window said "No
  // capture open" over a live session. Kept as the webview's half of the
  // round trip.
  it("reopens the project on screen instead of saying no capture is open", async () => {
    const rust = rustStash({ kind: "project", value: "project-r" });
    const { port, opened } = reusingPort(rust);
    const first = await mountRoot(port);
    expect(first.find('[data-testid="editor-shell"]').exists()).toBe(true);
    first.unmount();

    // The reload: a fresh webview — fresh stores — over the same Rust.
    setActivePinia(createPinia());
    const w = await mountRoot(port);
    expect(opened).toEqual(["project-r", "project-r"]);
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(true);
    expect(w.find('[data-testid="editor-empty"]').exists()).toBe(false);
  });

  // With the stash empty Rust answers the project already on screen, so a
  // spurious or double-fired `editor:open` must not reopen it: a reopen
  // starts a new store generation and would drop the reply of an edit
  // still in flight.
  it("ignores a drain that hands back the project already on screen", async () => {
    const rust = rustStash({ kind: "project", value: "project-r" });
    const { port, opened } = reusingPort(rust);
    await mountRoot(port);
    listeners["editor:open"]();
    await flushPromises();
    expect(opened).toEqual(["project-r"]);
  });

  it("shows a render that was still running, polling its progress to the end", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    let rows: JobRecordDto[] = [
      { jobId: "job-7", kind: "render", phase: "rendering", fraction: 0.4, terminal: null },
    ];
    const getJobs = vi.fn(() => Promise.resolve(rows));
    const rust = rustStash({ kind: "project", value: "project-r" });
    const { port } = reusingPort(rust, { getJobs });
    const w = await mountRoot(port);

    const dialog = w.get('[data-testid="render-dialog"]');
    expect(dialog.get('[data-testid="render-progress-phase"]').text()).toBe("Rendering…");
    expect(dialog.get('[data-testid="render-progress-bar"]').attributes("aria-valuenow")).toBe("40");
    // The Channel is gone: the dialog cannot be dismissed while it runs,
    // and Cancel is still the explicit way out.
    expect(dialog.find('[data-testid="render-dialog-cancel"]').exists()).toBe(true);

    rows = [{ jobId: "job-7", kind: "render", phase: "rendering", fraction: 0.8, terminal: null }];
    await vi.advanceTimersByTimeAsync(1000);
    await flushPromises();
    expect(w.get('[data-testid="render-progress-bar"]').attributes("aria-valuenow")).toBe("80");

    rows = [
      { jobId: "job-7", kind: "render", phase: "complete", fraction: 1, terminal: { productId: "prod-9" } },
    ];
    await vi.advanceTimersByTimeAsync(1000);
    await flushPromises();
    expect(w.get('[data-testid="render-progress-bar"]').attributes("aria-valuenow")).toBe("100");
    expect(w.get('[data-testid="render-dialog-status"]').text()).toBe(
      "Render complete. The new video is now one of this project's products.",
    );

    // A terminal stops the polling.
    const reads = getJobs.mock.calls.length;
    await vi.advanceTimersByTimeAsync(5000);
    await flushPromises();
    expect(getJobs.mock.calls.length).toBe(reads);
  });

  // The media library reconciles on its own mount, but it is not mounted
  // while the Titles or Products tab is the one open: the root's reconcile
  // for every new session is what finds the render then.
  it("finds a running render even when the media library is not mounted", async () => {
    const rust = rustStash({ kind: "project", value: "project-r" });
    const { port } = reusingPort(rust, {
      getJobs: () =>
        Promise.resolve([{ jobId: "job-7", kind: "render", phase: "preparing", fraction: 0, terminal: null }]),
    });
    const w = await mountRoot(port, { MediaLibrary: true });
    expect(w.get('[data-testid="render-progress-phase"]').text()).toBe("Preparing the render…");
  });

  // A finished render is in the Products tab; there is nothing to follow.
  it("opens no dialog when no render is running", async () => {
    const rust = rustStash({ kind: "project", value: "project-r" });
    const { port } = reusingPort(rust, {
      getJobs: () =>
        Promise.resolve([
          { jobId: "job-1", kind: "render", phase: "complete", fraction: 1, terminal: { productId: "p" } },
        ]),
    });
    const w = await mountRoot(port);
    expect(w.find('[data-testid="render-dialog"]').exists()).toBe(false);
  });
});

describe("the browser's own keys in the editor window", () => {
  function press(init: KeyboardEventInit, target: EventTarget = document.body): KeyboardEvent {
    const event = new KeyboardEvent("keydown", { bubbles: true, cancelable: true, ...init });
    target.dispatchEvent(event);
    return event;
  }

  const SUPPRESSED: [string, KeyboardEventInit][] = [
    ["F5", { key: "F5" }],
    ["Ctrl+F5", { key: "F5", ctrlKey: true }],
    ["Shift+F5", { key: "F5", shiftKey: true }],
    ["Ctrl+R", { key: "r", ctrlKey: true }],
    ["Ctrl+Shift+R", { key: "R", ctrlKey: true, shiftKey: true }],
    ["Ctrl+F", { key: "f", ctrlKey: true }],
    ["Ctrl+P", { key: "p", ctrlKey: true }],
  ];

  it.each(SUPPRESSED)("%s does nothing (its default is prevented)", async (_name, init) => {
    rustStash(null);
    await mountRoot(fakeEditorPort());
    expect(press(init).defaultPrevented).toBe(true);
  });

  // "Everywhere in the editor window": this editor has no find feature, and
  // a reload from a text field orphans the project just the same.
  it("is suppressed in a text field too", async () => {
    rustStash(null);
    await mountRoot(fakeEditorPort());
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();
    expect(press({ key: "r", ctrlKey: true }, input).defaultPrevented).toBe(true);
    input.remove();
  });

  // Task 13's rule: an AltGr chord (Ctrl+Alt on Windows) types a character
  // on some layouts and is never a browser accelerator — never swallow it;
  // nor a plain letter.
  it.each([
    ["AltGr+R", { key: "r", ctrlKey: true, altKey: true }],
    ["Meta+R", { key: "r", metaKey: true }],
    ["plain r", { key: "r" }],
    ["plain p", { key: "p" }],
  ] as [string, KeyboardEventInit][])("%s is left alone", async (_name, init) => {
    rustStash(null);
    await mountRoot(fakeEditorPort());
    expect(press(init).defaultPrevented).toBe(false);
  });

  it("stops listening once the root unmounts", async () => {
    rustStash(null);
    const w = await mountRoot(fakeEditorPort());
    w.unmount();
    expect(press({ key: "F5" }).defaultPrevented).toBe(false);
  });
});

// The completion line of a render the dialog did not start: its name went
// with the old webview, and a complete terminal without a product is a
// Review render's, which makes no product.
describe("completionText", () => {
  it("names the product when the dialog started the render", () => {
    expect(completionText("Walkthrough v2", "p")).toBe(
      "Render complete. “Walkthrough v2” is now one of this project's products.",
    );
  });
  it("says a product was made without a name, and nothing about one for a review", () => {
    expect(completionText("", "p")).toBe("Render complete. The new video is now one of this project's products.");
    expect(completionText("", undefined)).toBe("Render complete.");
  });
});
