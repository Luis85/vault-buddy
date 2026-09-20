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

// The strip's window-level pointerup listener (and the editor's own future
// listeners) must not outlive their test.
enableAutoUnmount(afterEach);

/** `assetPath` is the staged file's OWN absolute path, the way
 * `load_staged_capture` hands it over. It was a bare file name until P-5;
 * `convertFileSrc` joins nothing, so that produced a URL naming no file on
 * disk and matching no entry in the asset protocol's scope. A Windows path
 * on purpose: it is the only platform this ships on, and it is the one whose
 * separators and drive letter have to survive percent-encoding. */
const STAGED_MP4 =
  "C:\\Users\\me\\AppData\\Local\\com.vaultbuddy.desktop\\screen-captures\\cap one.mp4";

const DETAIL = {
  base: "cap one",
  assetPath: STAGED_MP4,
  durationMs: 10_000,
  sourceTitle: "Screen 1",
  width: 1920,
  height: 1080,
  recordedAt: "2026-09-20T10:00:00Z",
  timeline: null as unknown,
};

type Call = Record<string, unknown> & { cmd: string };

/** Serve one staged capture, plus whatever `take_editor_request` should
 * hand back on each successive drain. */
function mockEditor(
  detail: unknown = DETAIL,
  requests: (string | null)[] = ["cap one"],
  details?: Record<string, unknown>,
) {
  const seen: Call[] = [];
  const queue = [...requests];
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    if (cmd === "take_editor_request") return queue.length > 0 ? queue.shift() : null;
    if (cmd === "load_staged_capture") {
      const base = (args as { base: string }).base;
      return details?.[base] ?? detail;
    }
    return undefined;
  });
  return seen;
}

async function open(detail?: unknown, requests?: (string | null)[], details?: Record<string, unknown>) {
  mockEditor(detail, requests, details);
  const w = mount(EditorRoot);
  await flushPromises();
  return w;
}

function segments(w: ReturnType<typeof mount>) {
  return w.findAll('[data-testid^="segment-"]');
}

function video(w: ReturnType<typeof mount>) {
  return w.get('[data-testid="preview-video"]').element as HTMLVideoElement;
}

/** happy-dom rects are zero-sized; the strip reads its own width for a drop.
 * 200px at the origin, so a clientX is the percentage doubled. */
function sizeStrip(w: ReturnType<typeof mount>) {
  const strip = w.get('[data-testid="timeline-strip"]');
  strip.element.getBoundingClientRect = () =>
    ({ left: 0, width: 200, top: 0, height: 64, right: 200, bottom: 64, x: 0, y: 0 }) as DOMRect;
  return strip;
}

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

  it("refuses a split on a boundary rather than consuming an undo step", async () => {
    const w = await open();
    // The playhead starts at 0, which is segment 0's own start.
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(segments(w)).toHaveLength(1);
    expect(w.get('[data-testid="editor-undo"]').attributes("disabled")).toBeDefined();
  });

  it("splits at the playhead, enables undo, and undoes", async () => {
    const w = await open();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(segments(w)).toHaveLength(2);
    // The real point of the shallowRef: a non-reactive binding leaves this
    // disabled forever, because the computed that reads it never
    // re-evaluates.
    expect(w.get('[data-testid="editor-undo"]').attributes("disabled")).toBeUndefined();
    await w.get('[data-testid="editor-undo"]').trigger("click");
    expect(segments(w)).toHaveLength(1);
    expect(w.get('[data-testid="editor-redo"]').attributes("disabled")).toBeUndefined();
    await w.get('[data-testid="editor-redo"]').trigger("click");
    expect(segments(w)).toHaveLength(2);
  });

  it("deletes the selected segment and nothing else", async () => {
    const w = await open();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    await w.get('[data-testid="segment-1"]').trigger("click");
    await w.get('[data-testid="editor-delete"]').trigger("click");
    expect(segments(w)).toHaveLength(1);
    // One block is 100% whichever one survived, so that alone proves
    // nothing. Scrub to an output moment and read where it lands in the
    // SOURCE: output 1000 is source 1000 in the first segment and source
    // 5000 in the second, so this distinguishes them.
    expect(segments(w)[0].attributes("style")).toContain("100%");
    await w.get('[data-testid="preview-scrub"]').setValue("1000");
    expect(video(w).currentTime).toBeCloseTo(1, 3);
  });

  // THE SEAM. `TimelineStrip` emits an insertion SLOT in [0, n];
  // `useEditorTimeline.reorder` takes a destination INDEX in [0, n) and
  // REFUSES anything else. Passing the slot straight through is not
  // reinterpreted, it is rejected — so a missing conversion is a drag that
  // silently does nothing, which no assertion on "did it error" would catch.
  // Only the order itself proves the conversion ran.
  it("converts the strip's drop slot into a destination index", async () => {
    const w = await open({
      ...DETAIL,
      timeline: {
        segments: [
          { sourceStartMs: 0, sourceEndMs: 2000 },
          { sourceStartMs: 6000, sourceEndMs: 9000 },
        ],
      },
    });
    expect(segments(w)[0].attributes("style")).toContain("40%");
    const strip = sizeStrip(w);
    await w.get('[data-testid="segment-0"]').trigger("pointerdown", { clientX: 10 });
    await strip.trigger("pointerup", { clientX: 180 });
    // Slot 2 on a two-segment timeline is "past the last block" -> index 1.
    expect(segments(w)[0].attributes("style")).toContain("60%");
    expect(segments(w)[1].attributes("style")).toContain("40%");
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

  // Spec 8.2's shortcuts. The editor fills its own window, so there is no
  // narrower focus target than `window` to bind them to.
  it("undoes and redoes from the keyboard", async () => {
    const w = await open();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(segments(w)).toHaveLength(2);

    // A bare `z` is a keystroke, not a shortcut: without the modifier gate
    // typing into any future field in this window would rewrite the edit.
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z" }));
    await flushPromises();
    expect(segments(w)).toHaveLength(2);

    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }));
    await flushPromises();
    expect(segments(w)).toHaveLength(1);

    window.dispatchEvent(
      new KeyboardEvent("keydown", { key: "z", ctrlKey: true, shiftKey: true }),
    );
    await flushPromises();
    expect(segments(w)).toHaveLength(2);
  });

  // Ctrl+Y is what half of Windows expects; Ctrl+Shift+Z is what spec 8.2
  // names. Both are asserted because supporting one is not supporting the
  // other, and the whole app ships on Windows.
  it("redoes on Ctrl+Y as well", async () => {
    const w = await open();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }));
    await flushPromises();
    expect(segments(w)).toHaveLength(1);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "y", ctrlKey: true }));
    await flushPromises();
    expect(segments(w)).toHaveLength(2);
  });

  // The listener is on `window`, so an unmounted editor that kept listening
  // would go on editing a timeline nobody can see — and, worse, go on
  // WRITING it to the sidecar. That write is the observable: after the
  // unmount there is no rendered strip left to count, which is exactly why
  // an assertion on a segment count captured BEFORE the unmount proves
  // nothing at all.
  it("stops listening for shortcuts once unmounted", async () => {
    const seen = mockEditor();
    const w = mount(EditorRoot);
    await flushPromises();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    await flushPromises();
    const saves = () => seen.filter((c) => c.cmd === "save_capture_timeline").length;
    expect(saves()).toBe(1);

    w.unmount();
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true }));
    await flushPromises();
    // A surviving listener undoes the split and persists the result.
    expect(saves()).toBe(1);
  });

  // I-7. Spec 10 promises "saved on each edit, so a crash loses at most the
  // last one". When the sidecar write fails that promise is off and only the
  // user can act on it, so it cannot stay a log line — but it also must not
  // take the editor away: the edit is on screen and undo still works.
  it("says so when an edit could not be saved, and keeps the editor usable", async () => {
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return "cap one";
      if (cmd === "load_staged_capture") return DETAIL;
      if (cmd === "save_capture_timeline") throw new Error("no space left on device");
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.find('[data-testid="editor-save-failed"]').exists()).toBe(false);

    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    await flushPromises();

    expect(w.get('[data-testid="editor-save-failed"]').text()).toContain("could not be saved");
    // The edit itself landed, and the load-error banner (which REPLACES the
    // editor) is not what was rendered.
    expect(segments(w)).toHaveLength(2);
    expect(w.find('[data-testid="editor-error"]').exists()).toBe(false);
  });
});
