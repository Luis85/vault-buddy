/**
 * The editor window's LIFECYCLE half: draining the Rust-owned stash, loading
 * and resuming a staged capture, surviving a re-open, and the preview's two
 * clocks (the element reports source time; every surface the user sees
 * speaks output time).
 *
 * The EDITING half — the operations, the selection and playhead they
 * re-index, the strip-to-composable seam and spec 8.2's shortcuts — lives in
 * `editorEditing.test.ts`. The two files repeat the fixtures below rather
 * than sharing a helper module: `vi.mock` is hoisted per file in any case,
 * and each suite stays readable on its own.
 */
import { mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Captured so a test can drive `editor:open` the way Rust does — from the
// same main-thread closure that shows the editor window, immediately before
// the show. The editor window is hidden and reused, never destroyed, so its
// webview mounts exactly ONCE per process and the event is the only thing
// that makes an already-mounted editor re-read the stash.
const listeners: Record<string, () => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: () => void) => {
    listeners[event] = cb;
    return Promise.resolve(() => {
      delete listeners[event];
    });
  },
}));

// `mockIPC` installs `__TAURI_INTERNALS__`, which stops `logging.ts` being a
// no-op and routes it through the log plugin — harmless, but it would put
// `plugin:log|log` calls in the recorded array the assertions read.
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import { logWarning } from "../src/logging";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { type Call, DETAIL, mockEditor, open, segments, STAGED_MP4, video } from "./helpers/editorMount";

// The strip's window-level pointerup listener (and the editor's own future
// listeners) must not outlive their test.
enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const key of Object.keys(listeners)) delete listeners[key];
  mockConvertFileSrc("windows");
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("EditorRoot", () => {
  it("drains the request and loads that capture", async () => {
    const seen = mockEditor();
    const w = mount(EditorRoot);
    await flushPromises();
    expect(seen.find((c) => c.cmd === "load_staged_capture")).toMatchObject({ base: "cap one" });
    expect(w.text()).toContain("Screen 1");
  });

  // An untouched capture opens as ONE segment spanning the whole recording —
  // the timeline the sidecar does not carry yet.
  it("seeds a whole-capture timeline when the sidecar has none", async () => {
    const w = await open();
    expect(segments(w)).toHaveLength(1);
    expect(segments(w)[0].attributes("style")).toContain("100%");
  });

  it("resumes a saved timeline instead of reseeding it", async () => {
    const w = await open({
      ...DETAIL,
      timeline: {
        segments: [
          { sourceStartMs: 0, sourceEndMs: 2000 },
          { sourceStartMs: 6000, sourceEndMs: 9000 },
        ],
      },
    });
    expect(segments(w)).toHaveLength(2);
  });

  // Opening the window with nothing staged is not an error: a user can
  // alt-tab back to an editor that is already showing a capture.
  it("says so plainly when no capture was requested", async () => {
    const w = await open(DETAIL, [null]);
    expect(w.text()).toContain("No capture open");
  });

  // The drain itself failing is not the same as an empty stash, but it has
  // the same remedy: say nothing is open and leave a log line behind, rather
  // than throwing out of `onMounted` into an empty window.
  it("logs and stays empty when the stash cannot be drained", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") throw new Error("ipc is down");
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.text()).toContain("No capture open");
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringContaining("take_editor_request failed"),
    );
  });

  it("surfaces a load failure inline rather than a blank window", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return "cap one";
      if (cmd === "load_staged_capture") throw new Error("That capture's video file is missing.");
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.get('[data-testid="editor-error"]').text()).toContain("video file is missing");
  });

  // The stash alone is not enough: this webview mounts once per process, so
  // a second `open_capture_editor` would leave the FIRST capture on screen
  // forever without the event (`editor_commands.rs`'s module doc).
  it("re-reads the stash on editor:open, not only on mount", async () => {
    const w = await open(undefined, ["cap one", "cap two"], {
      "cap one": DETAIL,
      "cap two": { ...DETAIL, base: "cap two", sourceTitle: "Firefox" },
    });
    expect(w.text()).toContain("Screen 1");
    listeners["editor:open"]();
    await flushPromises();
    expect(w.text()).toContain("Firefox");
  });

  // Re-opening must reset the EDIT SURFACE, not just the header. This is the
  // fixture that pins `editor` being a shallowRef: with a non-reactive
  // binding the derived computeds keep the dependency they picked up from
  // the FIRST capture's own refs, so replacing the composable never
  // invalidates them and the strip goes on rendering the previous capture's
  // timeline under the new capture's title. Every other test here loads its
  // capture before the editor branch renders at all, so the computeds pick
  // up a live composable on their first evaluation and the mutation hides.
  it("re-seeds the strip from the new capture, not just the header", async () => {
    const w = await open(undefined, ["cap one", "cap two"], {
      "cap one": DETAIL,
      "cap two": {
        ...DETAIL,
        base: "cap two",
        sourceTitle: "Firefox",
        timeline: {
          segments: [
            { sourceStartMs: 0, sourceEndMs: 2000 },
            { sourceStartMs: 6000, sourceEndMs: 9000 },
          ],
        },
      },
    });
    expect(segments(w)).toHaveLength(1);
    listeners["editor:open"]();
    await flushPromises();
    expect(segments(w)).toHaveLength(2);
    expect(segments(w)[0].attributes("style")).toContain("40%");
  });

  // A drain that comes back empty means "nothing new", never "close what you
  // are showing" — blanking a live edit on a spurious event would be worse
  // than doing nothing.
  it("keeps the capture on screen when a re-open drains an empty stash", async () => {
    const w = await open(undefined, ["cap one"]);
    listeners["editor:open"]();
    await flushPromises();
    expect(w.text()).toContain("Screen 1");
    expect(w.text()).not.toContain("No capture open");
  });


  it("scrubs in output time and seeks the element in source time", async () => {
    const w = await open({
      ...DETAIL,
      timeline: { segments: [{ sourceStartMs: 6000, sourceEndMs: 9000 }] },
    });
    await w.get('[data-testid="preview-scrub"]').setValue("1500");
    expect(w.get('[data-testid="playhead"]').attributes("style")).toContain("50%");
    // Output 1500 into a segment that starts at source 6000 is source 7500.
    expect(video(w).currentTime).toBeCloseTo(7.5, 3);
  });

  it("reports the element's source time to the strip as output time", async () => {
    const w = await open({
      ...DETAIL,
      timeline: {
        segments: [
          { sourceStartMs: 2000, sourceEndMs: 4000 },
          { sourceStartMs: 6000, sourceEndMs: 9000 },
        ],
      },
    });
    video(w).currentTime = 7;
    await w.get('[data-testid="preview-video"]').trigger("timeupdate");
    // Source 7000 is 1000ms into the second segment: output 3000 of 5000.
    expect(w.get('[data-testid="playhead"]').attributes("style")).toContain("60%");
  });

  it("seeks forward when playback runs into footage the edit removed", async () => {
    const w = await open({
      ...DETAIL,
      timeline: {
        segments: [
          { sourceStartMs: 2000, sourceEndMs: 4000 },
          { sourceStartMs: 6000, sourceEndMs: 9000 },
        ],
      },
    });
    // Source 5000 was cut out entirely, so it has no output time at all.
    video(w).currentTime = 5;
    await w.get('[data-testid="preview-video"]').trigger("timeupdate");
    // The playhead is still at output 0, which is source 2000.
    expect(video(w).currentTime).toBeCloseTo(2, 3);
  });

  it("stops playing rather than reporting a position the strip cannot draw", async () => {
    const w = await open({
      ...DETAIL,
      timeline: { segments: [{ sourceStartMs: 2000, sourceEndMs: 4000 }] },
    });
    await w.get('[data-testid="preview-toggle"]').trigger("click");
    expect(w.get('[data-testid="preview-toggle"]').text()).toBe("Pause");
    // Park the playhead past the end of the output, then report a source
    // moment that is not in the edit either.
    await w.get('[data-testid="preview-scrub"]').setValue("2000");
    video(w).currentTime = 9;
    await w.get('[data-testid="preview-video"]').trigger("timeupdate");
    expect(video(w).paused).toBe(true);
    expect(w.get('[data-testid="preview-toggle"]').text()).toBe("Play");
  });

  it("plays and pauses the same element", async () => {
    const w = await open();
    const toggle = w.get('[data-testid="preview-toggle"]');
    await toggle.trigger("click");
    expect(video(w).paused).toBe(false);
    expect(toggle.text()).toBe("Pause");
    await toggle.trigger("click");
    expect(video(w).paused).toBe(true);
    expect(toggle.text()).toBe("Play");
  });

  // P-5. `convertFileSrc` percent-encodes its argument onto the asset origin
  // and JOINS NOTHING, so the URL has to carry the staged file's whole path:
  // a bare `cap one.mp4` produced `http://asset.localhost/cap%20one.mp4`,
  // which resolves to no file and matches no entry in the
  // `$APPLOCALDATA/screen-captures/*` scope — a preview that silently never
  // loads. Asserting the origin ALONE could not see that, which is how it
  // shipped.
  it("points the preview at the staged file through the asset protocol", async () => {
    const w = await open();
    const src = video(w).getAttribute("src");
    expect(src).toContain("asset.localhost");
    expect(src).toBe(`http://asset.localhost/${encodeURIComponent(STAGED_MP4)}`);
  });


  // m-8. `load` replaces the composable outright, abandoning its save chain,
  // and that chain writes the very sidecar the next load reads. Reopening
  // inside the write window seeded the editor from the PRE-write content, so
  // the next edit was derived from a timeline one operation out of date.
  it("lets the previous capture's save land before reading the next sidecar", async () => {
    const seen: Call[] = [];
    let releaseSave: () => void = () => undefined;
    const saveLanded = new Promise<void>((resolve) => {
      releaseSave = resolve;
    });
    const queue = ["cap one", "cap two"];
    mockIPC((cmd, args) => {
      seen.push({ cmd, ...(args as object) });
      if (cmd === "take_editor_request") return queue.shift() ?? null;
      if (cmd === "load_staged_capture") {
        return (args as { base: string }).base === "cap two"
          ? { ...DETAIL, base: "cap two", sourceTitle: "Firefox" }
          : DETAIL;
      }
      if (cmd === "save_capture_timeline") return saveLanded;
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");

    const loadsOfTwo = () =>
      seen.filter((c) => c.cmd === "load_staged_capture" && c.base === "cap two").length;

    listeners["editor:open"]();
    await flushPromises();
    // The save is still in flight, so the read has not happened yet.
    expect(loadsOfTwo()).toBe(0);

    releaseSave();
    await flushPromises();
    expect(loadsOfTwo()).toBe(1);
    expect(w.text()).toContain("Firefox");
  });

  // m-1. The strip's empty state used to read "Nothing left to save — undo a
  // delete or revert the edit": it named a delete that cannot have happened
  // (`deleteSegment` refuses to remove the last segment, so this state is
  // unreachable from the editor) and a Revert verb this window does not
  // offer at all.
  it("names no verb it does not offer when there is nothing to show", async () => {
    const w = await open({ ...DETAIL, timeline: { segments: [] } });
    const empty = w.get('[data-testid="strip-empty"]').text();
    expect(empty).not.toMatch(/revert/i);
    expect(empty).not.toMatch(/undo/i);
  });


});
