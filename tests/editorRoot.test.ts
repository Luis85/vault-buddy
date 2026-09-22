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
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Captured so a test can drive `editor:open` the way Rust does — from the
// same main-thread closure that shows the editor window, immediately before
// the show. The editor window is hidden and reused, never destroyed, so its
// webview mounts exactly ONCE per process and the event is the only thing
// that makes an already-mounted editor re-read the stash.
const listeners: Record<string, (e?: { payload: unknown }) => void> = {};
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, cb: (e?: { payload: unknown }) => void) => {
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

import type { EditorPort } from "../src/editor/port";
import { EditorPortError } from "../src/editor/port";
import type { EditorOpenResult, EditorSnapshot, Project } from "../src/editorTypes";
import { logWarning } from "../src/logging";
import EditorRoot from "../src/roots/EditorRoot.vue";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import {
  type Call,
  DETAIL,
  mockEditor,
  open,
  segments,
  STAGED_MP4,
  THREE,
  video,
} from "./helpers/editorMount";

// The strip's window-level pointerup listener (and the editor's own future
// listeners) must not outlive their test.
enableAutoUnmount(afterEach);

beforeEach(() => {
  for (const key of Object.keys(listeners)) delete listeners[key];
  mockConvertFileSrc("windows");
  // Task 15: EditorRoot now calls `useEditorProjectStore()`, which needs an
  // active Pinia at mount time — a fresh instance per test, the PanelRoot
  // test's own precedent, since this window installs a real store for the
  // first time.
  setActivePinia(createPinia());
});

afterEach(() => {
  vi.restoreAllMocks();
});

/** Deliver a Rust-side event the way `app.emit` does. Deliberately NOT
 * optional-chained: a typo in the event name, or a listener the root stopped
 * registering, must fail the test rather than quietly deliver nothing. */
function emit(event: string, payload: unknown) {
  listeners[event]({ payload });
}

function valueNow(w: ReturnType<typeof mount>) {
  return w.find('[data-testid="export-progress"]').attributes("aria-valuenow");
}

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

  // Drive the element the way playback does: it advances on its own and
  // reports each position through `timeupdate`. The playhead is a CONSEQUENCE
  // of those reports, never something a test may park by hand — parking it
  // manufactures a state real playback cannot reach, and both tests below
  // used to do exactly that, which is why neither noticed that the preview
  // could not cross a cut at all.
  async function tick(w: ReturnType<typeof mount>, sourceSeconds: number) {
    video(w).currentTime = sourceSeconds;
    await w.get('[data-testid="preview-video"]').trigger("timeupdate");
    return video(w).currentTime;
  }

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
    // Inside the first segment: reported, not seeked.
    expect(await tick(w, 3.9)).toBeCloseTo(3.9, 3);
    // Playback runs past that segment's end into footage the edit removed.
    // The seek target is the BOUNDARY's next side — the second segment's
    // own source start — never the playhead, which is always still BEHIND
    // the boundary and would seek backwards into the segment that just
    // ended.
    expect(await tick(w, 4.1)).toBeCloseTo(6, 3);
  });

  // The failure this pins is a LOOP, so one crossing cannot pin it: seeking
  // backwards to the playhead looks like "a seek happened" to any assertion
  // that only checks the element moved. Play on past the cut instead and
  // require the second segment's footage to actually reach the strip.
  it("plays on through a cut instead of looping at the boundary", async () => {
    const w = await open({
      ...DETAIL,
      timeline: {
        segments: [
          { sourceStartMs: 2000, sourceEndMs: 4000 },
          { sourceStartMs: 6000, sourceEndMs: 9000 },
        ],
      },
    });
    const outputs: string[] = [];
    // 3.9 is in segment one; 4.1 is the removed footage; the element then
    // carries on from wherever the handler put it.
    for (const t of [3.9, 4.1]) {
      await tick(w, t);
      outputs.push(w.get('[data-testid="playhead"]').attributes("style") ?? "");
    }
    for (let i = 0; i < 3; i += 1) {
      await tick(w, video(w).currentTime + 0.5);
      outputs.push(w.get('[data-testid="playhead"]').attributes("style") ?? "");
    }
    // Source 7.5 is 1500ms into the second segment: output 3500 of 5000.
    expect(video(w).currentTime).toBeCloseTo(7.5, 3);
    expect(outputs[outputs.length - 1]).toContain("70%");
    // And the playhead never went backwards. It does not MOVE on the
    // crossing tick — the handler seeks the element and the strip catches up
    // on the next report, which is the one-tick lag the boundary hitch in the
    // caption under the video already admits to — but it must never retreat,
    // and it must end past where the cut was.
    const left = outputs.map((s) => Number(/left: ([\d.]+)%/.exec(s)?.[1] ?? NaN));
    for (let i = 1; i < left.length; i += 1) expect(left[i]).toBeGreaterThanOrEqual(left[i - 1]);
    expect(left[left.length - 1]).toBeGreaterThan(left[0]);
  });

  it("stops playing rather than reporting a position the strip cannot draw", async () => {
    const w = await open({
      ...DETAIL,
      timeline: { segments: [{ sourceStartMs: 2000, sourceEndMs: 4000 }] },
    });
    await w.get('[data-testid="preview-toggle"]').trigger("click");
    expect(w.get('[data-testid="preview-toggle"]').text()).toBe("Pause");
    // Reached by PLAYBACK, not by a scrub: the last footage in the edit,
    // then one tick past it. There is no next segment, so the output has
    // genuinely ended and the only honest answer is to stop.
    await tick(w, 3.9);
    await tick(w, 4.1);
    expect(video(w).paused).toBe(true);
    expect(w.get('[data-testid="preview-toggle"]').text()).toBe("Play");
  });

  // The seek guard's own half of the contract. `timeupdate` feeds the
  // watcher its own reported position back, so an equality-free re-seek
  // would stall playback on every tick — the failure the tolerance exists
  // to prevent. Removing the guard left the whole suite green, because
  // every other test only ever proves that it seeks when it MUST.
  it("does not re-seek the element to the position it just reported", async () => {
    const w = await open({
      ...DETAIL,
      timeline: { segments: [{ sourceStartMs: 2000, sourceEndMs: 9000 }] },
    });
    const el = video(w);
    let position = 2.5;
    let writes = 0;
    Object.defineProperty(el, "currentTime", {
      configurable: true,
      get: () => position,
      set: (v: number) => {
        writes += 1;
        position = v;
      },
    });
    // Two ordinary ticks, each well within the 250ms tolerance of where the
    // element already is.
    await w.get('[data-testid="preview-video"]').trigger("timeupdate");
    position = 2.6;
    await w.get('[data-testid="preview-video"]').trigger("timeupdate");
    expect(writes).toBe(0);
    expect(position).toBe(2.6);
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


  // ---- Export: Save, Discard, progress (spec 8.3, 10) --------------------

  // The stash is drained per open, so the base the editor SHOWS is the only
  // right argument. A base captured once at mount would export capture A
  // while the user is looking at capture B — and the editor window is
  // reused, so that is the ordinary case, not a corner.
  it("sends the open base to export_and_save_capture, not a stale one", async () => {
    const w = await open(undefined, ["cap one", "cap two"], {
      "cap one": DETAIL,
      "cap two": { ...DETAIL, base: "cap two", sourceTitle: "Firefox" },
    });
    const seen: Call[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, ...(args as object) });
      if (cmd === "take_editor_request") return "cap two";
      if (cmd === "load_staged_capture")
        return { ...DETAIL, base: "cap two", sourceTitle: "Firefox" };
      return undefined;
    });
    listeners["editor:open"]();
    await flushPromises();
    expect(w.text()).toContain("Firefox");

    await w.get('[data-testid="export-save"]').trigger("click");
    await flushPromises();
    const exports = seen
      .filter((c) => c.cmd === "export_and_save_capture")
      .map((c) => c.base as string);
    expect(exports).toEqual(["cap two"]);
  });

  // The Rust side emits a fraction in 0..1 and the bar renders a percent.
  // Emitting 0..100 by mistake renders 4000% with nothing to catch it on the
  // Rust side, which has no frontend to assert against.
  it("reads screen:exportProgress as a fraction between zero and one", async () => {
    const w = await open();
    emit("screen:exportProgress", { base: "cap one", fraction: 0.4 });
    await flushPromises();
    expect(valueNow(w)).toBe("40");
  });

  // A progress event for a DIFFERENT capture must not drive this window's
  // bar: the events are app-wide, and an editor reopened on capture B while
  // A is still exporting would otherwise show A's progress.
  it("ignores an export event addressed to another capture", async () => {
    const w = await open();
    emit("screen:exportProgress", { base: "cap one", fraction: 0.2 });
    await flushPromises();
    expect(valueNow(w)).toBe("20");
    // Now a tick for somebody else. The bar must not move, and it must not
    // be torn down either.
    emit("screen:exportProgress", { base: "some other capture", fraction: 0.9 });
    await flushPromises();
    expect(valueNow(w)).toBe("20");
    // The terminal events are addressed the same way and must be ignored
    // just as hard: a foreign `exported` would claim this capture was saved.
    emit("screen:exported", {
      base: "some other capture",
      videoPath: "C:\\vault\\Other.mp4",
      notePath: null,
      vaultId: "v1",
      warning: null,
    });
    await flushPromises();
    expect(valueNow(w)).toBe("20");
    expect(w.find('[data-testid="export-open"]').exists()).toBe(false);
  });

  // Spec 14: a cancel keeps the staged capture and its timeline. It is not a
  // failure, so there is nothing to apologise for and Save must come back.
  it("returns to idle without an error banner when an export is cancelled", async () => {
    const w = await open();
    await w.get('[data-testid="export-save"]').trigger("click");
    await flushPromises();
    emit("screen:exportProgress", { base: "cap one", fraction: 0.5 });
    await flushPromises();
    expect(w.find('[data-testid="export-cancel"]').exists()).toBe(true);

    emit("screen:exportCancelled", { base: "cap one" });
    await flushPromises();
    expect(w.find('[data-testid="export-message"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-progress"]').exists()).toBe(false);
    expect(w.get('[data-testid="export-save"]').attributes("disabled")).toBeUndefined();
  });

  it("shows the failure message and keeps the edit on screen when an export fails", async () => {
    const w = await open(undefined, ["cap one"], { "cap one": { ...DETAIL, ...THREE } });
    await w.get('[data-testid="export-save"]').trigger("click");
    await flushPromises();
    emit("screen:exportFailed", { base: "cap one", message: "ffmpeg was not found." });
    await flushPromises();
    expect(w.get('[data-testid="export-message"]').text()).toContain("ffmpeg was not found.");
    // The edit is still there and still saveable: a failure is recoverable.
    expect(segments(w)).toHaveLength(3);
    expect(w.find('[data-testid="export-save"]').exists()).toBe(true);
  });

  // The saved video's note is the richer destination -- it embeds the video,
  // the way the audio domain's note embeds the audio. With notes turned off
  // there is no note, and the video is the only answer.
  it("opens the note when there is one and the video when there is not", async () => {
    const w = await open();
    const seen: Call[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, ...(args as object) });
      return undefined;
    });
    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\Screen Captures\\cap one.mp4",
      notePath: "C:\\vault\\Screen Captures\\cap one.md",
      vaultId: "vault-7",
      warning: null,
    });
    await flushPromises();
    await w.get('[data-testid="export-open"]').trigger("click");
    await flushPromises();
    expect(seen.filter((c) => c.cmd === "open_screen_capture")).toEqual([
      {
        cmd: "open_screen_capture",
        id: "vault-7",
        path: "C:\\vault\\Screen Captures\\cap one.md",
      },
    ]);

    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\Screen Captures\\cap one.mp4",
      notePath: null,
      vaultId: "vault-7",
      warning: null,
    });
    await flushPromises();
    await w.get('[data-testid="export-open"]').trigger("click");
    await flushPromises();
    expect(seen.filter((c) => c.cmd === "open_screen_capture").pop()).toMatchObject({
      path: "C:\\vault\\Screen Captures\\cap one.mp4",
    });
  });

  // The success line NAMES the vault, and that is why `screen:exported`
  // carries `vaultName` beside `vaultId`. The id is Obsidian's opaque hex
  // registry key; this window installs no store and has no vault list, so it
  // cannot turn one into the other. The negative half is the load-bearing
  // one: rendering the id instead would look like a filled-in name to any
  // assertion that only checked the line was non-empty.
  it("names the vault on the success line, and never its registry id", async () => {
    const w = await open();
    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\cap one.mp4",
      notePath: "C:\\vault\\cap one.md",
      vaultId: "9f3c1a77bd0e4412",
      vaultName: "Engineering",
      warning: null,
    });
    await flushPromises();
    const line = w.get('[data-testid="export-message"]').text();
    expect(line).toContain("Engineering");
    expect(line).not.toContain("9f3c1a77bd0e4412");
  });

  // The video landed but its note did not: the export SUCCEEDED, so this is
  // a warning on the success line, never a failure. Dropping it would leave
  // the user believing a note exists that does not.
  it("surfaces an exported capture's warning beside the success line", async () => {
    const w = await open();
    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\cap one.mp4",
      notePath: null,
      vaultId: "vault-7",
      warning: "The companion note could not be written.",
    });
    await flushPromises();
    expect(w.get('[data-testid="export-message"]').text()).toContain(
      "The companion note could not be written.",
    );
    expect(w.find('[data-testid="export-open"]').exists()).toBe(true);
  });

  // M9. The editor window is hidden and REUSED, so every ref that outlives a
  // capture is a chance to show the previous one's state. A stale `done`
  // leaves capture B with no Save button at all — the phase-4 "the editor
  // comes back showing capture A" bug, in a new field.
  it("opens the next capture with a fresh export bar, not the last one's success", async () => {
    const w = await open(undefined, ["cap one", "cap two"], {
      "cap one": DETAIL,
      "cap two": { ...DETAIL, base: "cap two", sourceTitle: "Firefox" },
    });
    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\cap one.mp4",
      notePath: null,
      vaultId: "vault-7",
      warning: null,
    });
    await flushPromises();
    expect(w.find('[data-testid="export-open"]').exists()).toBe(true);
    expect(w.find('[data-testid="export-save"]').exists()).toBe(false);

    listeners["editor:open"]();
    await flushPromises();
    expect(w.text()).toContain("Firefox");
    expect(w.find('[data-testid="export-open"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-message"]').exists()).toBe(false);
    expect(w.get('[data-testid="export-save"]').attributes("disabled")).toBeUndefined();
  });

  // Discard destroys the only copy of a recording, so the confirm is the
  // bar's; what this half owns is that the SECOND click really deletes, and
  // that the editor stops offering a capture that is no longer on disk.
  it("discards the open capture and stops showing it", async () => {
    const w = await open();
    const seen: Call[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, ...(args as object) });
      return undefined;
    });
    await w.get('[data-testid="export-discard"]').trigger("click");
    await flushPromises();
    expect(seen.filter((c) => c.cmd === "discard_staged_capture")).toHaveLength(0);

    await w.get('[data-testid="export-discard"]').trigger("click");
    await flushPromises();
    expect(seen.filter((c) => c.cmd === "discard_staged_capture")).toEqual([
      { cmd: "discard_staged_capture", base: "cap one" },
    ]);
    expect(w.text()).toContain("No capture open");
  });

  // A refused discard must leave the capture exactly where it was: the file
  // is still on disk, so a window that blanked itself would strand it.
  it("keeps the capture when a discard is refused", async () => {
    const w = await open();
    mockIPC((cmd) => {
      if (cmd === "discard_staged_capture") throw new Error("That capture is in use.");
      return undefined;
    });
    await w.get('[data-testid="export-discard"]').trigger("click");
    await w.get('[data-testid="export-discard"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="export-message"]').text()).toContain("That capture is in use.");
    expect(w.text()).toContain("Screen 1");
  });

  it("asks Rust to cancel a running export", async () => {
    const w = await open();
    const seen: Call[] = [];
    mockIPC((cmd, args) => {
      seen.push({ cmd, ...(args as object) });
      return undefined;
    });
    emit("screen:exportProgress", { base: "cap one", fraction: 0.3 });
    await flushPromises();
    await w.get('[data-testid="export-cancel"]').trigger("click");
    await flushPromises();
    expect(seen.filter((c) => c.cmd === "cancel_export")).toHaveLength(1);
  });

  // `export_and_save_capture` rejects when ffmpeg is missing or the base is
  // refused. That reply never becomes a `screen:exportFailed` event, so a
  // root that only listened would sit at "exporting" forever.
  it("surfaces a refused export from the command's own reply", async () => {
    const w = await open();
    mockIPC((cmd) => {
      if (cmd === "export_and_save_capture") throw new Error("ffmpeg was not found.");
      return undefined;
    });
    await w.get('[data-testid="export-save"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="export-message"]').text()).toContain("ffmpeg was not found.");
    expect(w.find('[data-testid="export-cancel"]').exists()).toBe(false);
  });

  // A refused cancel is not a user-facing failure: the export is still
  // running and its own terminal event still arrives, so the remedy is a log
  // line rather than a banner claiming something went wrong.
  it("logs rather than banners when a cancel is refused", async () => {
    const w = await open();
    mockIPC((cmd) => {
      if (cmd === "cancel_export") throw new Error("no export is running");
      return undefined;
    });
    emit("screen:exportProgress", { base: "cap one", fraction: 0.3 });
    await flushPromises();
    await w.get('[data-testid="export-cancel"]').trigger("click");
    await flushPromises();
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringContaining("cancel_export failed"),
    );
    expect(w.find('[data-testid="export-message"]').exists()).toBe(false);
  });

  // Open CAN fail -- the saved file was moved or renamed in Obsidian between
  // the save and the click -- and a click that does nothing visible is what
  // the diagnostics invariant forbids.
  it("surfaces a refused open instead of doing nothing", async () => {
    const w = await open();
    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\cap one.mp4",
      notePath: null,
      vaultId: "vault-7",
      warning: null,
    });
    await flushPromises();
    mockIPC((cmd) => {
      if (cmd === "open_screen_capture") throw new Error("That capture is outside its vault.");
      return undefined;
    });
    await w.get('[data-testid="export-open"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="export-message"]').text()).toContain("outside its vault");
  });

  // Spec 8.1 lets the last segment be deleted only through undo; an EMPTY
  // timeline still reaches this window through a hand-edited sidecar, and
  // `export::export_refusal` refuses it server-side. Save reads as disabled
  // rather than failing on click.
  it("disables Save when the timeline holds no footage", async () => {
    const w = await open({ ...DETAIL, timeline: { segments: [] } });
    expect(w.get('[data-testid="export-save"]').attributes("disabled")).toBeDefined();
  });

  // ---- The new Rust session (Task 15) -------------------------------------

  function snapshotFixture(overrides: Partial<EditorSnapshot> = {}): EditorSnapshot {
    return {
      sessionId: "ses-a",
      projectId: "project-a",
      revision: 1,
      persistedRevision: null,
      title: "Tutorial",
      durationMs: 6000,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
      ...overrides,
    };
  }

  function projectFixture(overrides: Partial<Project> = {}): Project {
    return {
      schema: "vault-buddy-video-project/3",
      id: "project-a",
      title: "Tutorial",
      canvas: { width: 1280, height: 720, fps: 30 },
      master_gain: 1,
      assets: [],
      tracks: [],
      clips: [],
      effects: [],
      markers: [],
      transitions: [],
      captions: null,
      destination: { vault: "vault-a", folder: "", dated: false },
      ...overrides,
    };
  }

  function openResultFixture(overrides: Partial<EditorOpenResult> = {}): EditorOpenResult {
    return {
      snapshot: snapshotFixture(),
      project: projectFixture(),
      workspace: {},
      missing: [],
      sourceBase: "cap one",
      recovered: false,
      ...overrides,
    };
  }

  /** A minimal fake `EditorPort` for exercising the store's `openStaged`
   * seam without a full `mockIPC`-decoded round trip — `editorPort.test.ts`
   * already pins the `editor_open_staged` wire mapping; this level pins that
   * `EditorRoot` calls it exactly the way the store expects. */
  function fakeEditorPort(overrides: Partial<EditorPort> = {}): EditorPort {
    const unimplemented = (name: string) => (): never => {
      throw new Error(`fakeEditorPort.${name} not stubbed for this test`);
    };
    return {
      openStaged: unimplemented("openStaged"),
      openProject: unimplemented("openProject"),
      listProjects: unimplemented("listProjects"),
      getSnapshot: unimplemented("getSnapshot"),
      execute: unimplemented("execute"),
      save: unimplemented("save"),
      closeSession: unimplemented("closeSession"),
      hideWindow: unimplemented("hideWindow"),
      getWorkspace: unimplemented("getWorkspace"),
      saveWorkspace: unimplemented("saveWorkspace"),
      ...overrides,
    };
  }

  // Fix round 1: the shell's duration must use the shared
  // `src/utils/formatDuration.ts` (h:mm:ss, negative-clamped), not a local
  // mm:ss-only copy that overflows past an hour — 2h read "120:00" instead
  // of "2:00:00" before this fix.
  it("renders the shell's duration past an hour as h:mm:ss, not overflowed mm:ss", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) =>
          Promise.resolve(
            openResultFixture({
              sourceBase: base,
              snapshot: snapshotFixture({ durationMs: 7_200_000 }), // 2h
            }),
          ),
      }),
    );
    const w = await open();

    expect(w.get('[data-testid="editor-shell-duration"]').text()).toBe("2:00:00");
  });

  // Opens the stashed base through the store's own `openStaged` — which is
  // the ONE seam that calls `editor_open_staged` (`src/editor/port.ts`'s own
  // module doc). `open()` still drives `take_editor_request`/
  // `load_staged_capture` for the legacy preview through `mockIPC`; the new
  // session opens alongside it through the store's injected port.
  it("opens the stashed base through editor_open_staged", async () => {
    const calls: string[] = [];
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          calls.push(base);
          return Promise.resolve(openResultFixture({ sourceBase: base }));
        },
      }),
    );
    await open();

    expect(calls).toEqual(["cap one"]);
    expect(store.sessionId).toBe("ses-a");
  });

  // Task 18 fix round 1 (controller ruling): a successful open must hydrate
  // `editorWorkspace` with the new session id, or `editor_get_workspace`/
  // `editor_save_workspace` never get a production caller at all.
  it("hydrates the workspace store with the new session id after a successful open", async () => {
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const hydrateCalls: string[] = [];
    workspace.setPort(
      fakeEditorPort({
        getWorkspace: (id) => {
          hydrateCalls.push(id);
          return Promise.resolve({});
        },
      }),
    );

    await open();

    expect(hydrateCalls).toEqual(["ses-a"]);
  });

  it("does not hydrate the workspace when the open fails", async () => {
    const project = useEditorProjectStore();
    project.setPort(
      fakeEditorPort({
        openStaged: () =>
          Promise.reject(
            new EditorPortError({
              code: "sourceMissing",
              message: "gone",
              retryable: false,
              operationId: "op-1",
            }),
          ),
      }),
    );
    const workspace = useEditorWorkspaceStore();
    const hydrateCalls: string[] = [];
    workspace.setPort(
      fakeEditorPort({
        getWorkspace: (id) => {
          hydrateCalls.push(id);
          return Promise.resolve({});
        },
      }),
    );

    await open();

    expect(hydrateCalls).toEqual([]);
  });

  // A second `editor:open` for the SAME base while that session is open must
  // not mint a second one — the store-level guard Task 15 adds to
  // `openStaged` (`editorProjectStore.test.ts` pins the guard itself; this
  // is the scenario it exists for: a duplicate Edit click, or a re-emitted
  // `editor:open`, for the capture already showing).
  it("a second editor:open for the same base reuses the session", async () => {
    const calls: string[] = [];
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          calls.push(base);
          return Promise.resolve(openResultFixture({ sourceBase: base }));
        },
      }),
    );
    // `open()`'s default request queue is `["cap one"]`; a second
    // `editor:open` with nothing further queued still resolves to "cap one"
    // here because Rust re-stashes the SAME base for a re-Edit click on a
    // capture that is already open — modelled by handing `open()` the base
    // twice.
    const w = await open(undefined, ["cap one", "cap one"]);
    listeners["editor:open"]();
    await flushPromises();

    // MUTATION CHECK (this task's brief): drop the same-base short-circuit
    // in `editorProject.openStaged` and this reads 2, red for the reason
    // this test names — see `editorProjectStore.test.ts` for the guard's
    // own isolated pin.
    expect(calls).toEqual(["cap one"]);
    expect(w.text()).toContain("Screen 1");
  });

  // ---- Fix round 1: the LEGACY surface must reload on every drain, even a
  // repeated base (a request nonce alongside `stagedBase`), or a failed load
  // can never be retried and a stale export-bar state can never clear. ----

  // Reopening the SAME base after a `load_staged_capture` failure must
  // retry the load, not sit on the stale error forever — Rust can
  // legitimately re-stash the same base (a re-Edit click on the capture
  // whose load just failed) and the fix is exactly what `load()` already
  // guarantees: it flushes pending edits first, so retrying is safe.
  it("retries after a load_staged_capture failure when the same base is re-opened", async () => {
    let loadCalls = 0;
    mockIPC((cmd) => {
      if (cmd === "take_editor_request") return "cap one";
      if (cmd === "load_staged_capture") {
        loadCalls += 1;
        if (loadCalls === 1) throw new Error("That capture's video file is missing.");
        return DETAIL;
      }
      return undefined;
    });
    const w = mount(EditorRoot);
    await flushPromises();
    expect(w.get('[data-testid="editor-error"]').text()).toContain("video file is missing");

    listeners["editor:open"]();
    await flushPromises();

    expect(loadCalls).toBe(2);
    expect(w.find('[data-testid="editor-error"]').exists()).toBe(false);
    expect(w.text()).toContain("Screen 1");
  });

  // A legacy Save no longer removes the staged capture once the editor has
  // pinned it (the store opens a session alongside every legacy load, this
  // task on) — `export_worker` keeps a pinned capture staged, so re-opening
  // the SAME capture after a successful Save is the ordinary case, not a
  // corner. The export bar must not go on claiming "done" for a capture the
  // user can still act on.
  it("resets a stale done export bar when the same capture is re-opened after a legacy Save", async () => {
    const w = await open(undefined, ["cap one", "cap one"]);
    emit("screen:exported", {
      base: "cap one",
      videoPath: "C:\\vault\\cap one.mp4",
      notePath: null,
      vaultId: "vault-7",
      warning: null,
    });
    await flushPromises();
    expect(w.find('[data-testid="export-open"]').exists()).toBe(true);
    expect(w.find('[data-testid="export-save"]').exists()).toBe(false);

    listeners["editor:open"]();
    await flushPromises();

    expect(w.find('[data-testid="export-open"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-message"]').exists()).toBe(false);
    expect(w.get('[data-testid="export-save"]').attributes("disabled")).toBeUndefined();
  });

  // ---- Fix round 1: a failed session open must not leave the shell
  // showing the PREVIOUS capture's identity under the new one. ----

  it("hides the shell rather than showing a stale capture's title or vault when a later open fails", async () => {
    const store = useEditorProjectStore();
    let openCalls = 0;
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          openCalls += 1;
          if (openCalls === 1) {
            return Promise.resolve(
              openResultFixture({
                sourceBase: base,
                snapshot: snapshotFixture({ title: "Capture A" }),
                project: projectFixture({ destination: { vault: "vault-a", folder: "", dated: false } }),
              }),
            );
          }
          return Promise.reject(
            new EditorPortError({
              code: "sourceMissing",
              message: "That capture's video file is missing.",
              retryable: false,
              operationId: "op-1",
            }),
          );
        },
      }),
    );
    const w = await open(undefined, ["cap one", "cap two"], {
      "cap one": DETAIL,
      "cap two": { ...DETAIL, base: "cap two", sourceTitle: "Firefox" },
    });
    expect(w.get('[data-testid="editor-shell-title"]').text()).toBe("Capture A");
    expect(w.get('[data-testid="editor-shell-vault"]').text()).toBe("vault-a");

    listeners["editor:open"]();
    await flushPromises();

    // The legacy surface still shows "cap two" (its own load succeeded),
    // but the shell around it must not go on attributing capture A's title
    // and vault to whatever is now open — the new session's own open FAILED.
    expect(w.find('[data-testid="editor-shell"]').exists()).toBe(false);
    expect(w.text()).not.toContain("Capture A");
    expect(w.text()).not.toContain("vault-a");
    expect(vi.mocked(logWarning)).toHaveBeenCalledWith(
      expect.stringContaining("editor_open_staged"),
    );
  });

  // ---- Fix round 1, controller ruling: opening a staged capture in the
  // editor now PINS it to a tutorial project (every legacy load opens a
  // session alongside it, this task on), and `discard_staged_capture`
  // refuses outright while a capture is pinned ("Discard the project
  // first."). Legacy Discard must close-then-discard, in that order, or it
  // is permanently refused for every capture the editor has ever opened. ----

  it("closes the live session (discardProject) before discarding a pinned staged capture", async () => {
    const order: string[] = [];
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
        closeSession: (_sessionId, disposition) => {
          order.push(`close:${disposition}`);
          return Promise.resolve();
        },
      }),
    );
    const w = await open();
    // Sanity: the session really opened, or this test would prove nothing
    // about ordering (a null `sessionId` before Discard is a different bug).
    expect(store.sessionId).toBe("ses-a");

    mockIPC((cmd) => {
      if (cmd === "discard_staged_capture") {
        order.push("discard");
        return undefined;
      }
      return undefined;
    });
    await w.get('[data-testid="export-discard"]').trigger("click");
    await w.get('[data-testid="export-discard"]').trigger("click");
    await flushPromises();

    expect(order).toEqual(["close:discardProject", "discard"]);
    expect(w.text()).toContain("No capture open");
  });

  it("surfaces the close failure and never discards when closing the session fails", async () => {
    const store = useEditorProjectStore();
    const discardCalls: string[] = [];
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => Promise.resolve(openResultFixture({ sourceBase: base })),
        closeSession: () =>
          Promise.reject(
            new EditorPortError({
              code: "internal",
              message: "This capture is still open in a tutorial project.",
              retryable: false,
              operationId: "op-1",
            }),
          ),
      }),
    );
    const w = await open();
    expect(store.sessionId).toBe("ses-a");

    mockIPC((cmd) => {
      if (cmd === "discard_staged_capture") {
        discardCalls.push(cmd);
        return undefined;
      }
      return undefined;
    });
    await w.get('[data-testid="export-discard"]').trigger("click");
    await w.get('[data-testid="export-discard"]').trigger("click");
    await flushPromises();

    expect(discardCalls).toHaveLength(0);
    expect(w.get('[data-testid="export-message"]').text()).toContain(
      "This capture is still open in a tutorial project.",
    );
    // The capture is still showing: a refused close must not have blanked it.
    expect(w.text()).toContain("Screen 1");
  });

  // ---- Fix round 2: the close guard must be keyed on WHICH capture is
  // being discarded, not merely "is any session open". `editorProject` is a
  // single-session store, so a session left over from a PREVIOUS capture
  // (still open because its own close was never attempted, or because a
  // later capture's own session-open failed and the store never blanks on
  // failure) must not be torn down as a side effect of discarding a
  // DIFFERENT capture. ----

  it("does not close an unrelated session when discarding a different capture", async () => {
    const closeCalls: string[] = [];
    const store = useEditorProjectStore();
    let openCalls = 0;
    store.setPort(
      fakeEditorPort({
        openStaged: (base) => {
          openCalls += 1;
          // "cap one" opens a real session; "cap two"'s own session-open
          // FAILS, but its legacy `load_staged_capture` succeeds
          // independently (mocked below) — the exact scenario the
          // coordinator named.
          if (openCalls === 1) {
            return Promise.resolve(openResultFixture({ sourceBase: base }));
          }
          return Promise.reject(
            new EditorPortError({
              code: "sourceMissing",
              message: "That capture's video file is missing.",
              retryable: false,
              operationId: "op-2",
            }),
          );
        },
        closeSession: (sessionId, disposition) => {
          closeCalls.push(`${sessionId}:${disposition}`);
          return Promise.resolve();
        },
      }),
    );
    const w = await open(undefined, ["cap one", "cap two"], {
      "cap one": DETAIL,
      "cap two": { ...DETAIL, base: "cap two", sourceTitle: "Firefox" },
    });
    // Sanity: A's session really opened.
    expect(store.sessionId).toBe("ses-a");
    expect(store.sourceBase).toBe("cap one");

    listeners["editor:open"]();
    await flushPromises();
    // B's own session-open failed, so the store's `sessionId`/`sourceBase`
    // are UNCHANGED (A's) — the store's own documented "never blank on a
    // failed open" behavior — while the legacy surface has moved on to B.
    expect(w.text()).toContain("Firefox");
    expect(store.sessionId).toBe("ses-a");
    expect(store.sourceBase).toBe("cap one");

    const discardCalls: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "discard_staged_capture") {
        discardCalls.push((args as { base: string }).base);
        return undefined;
      }
      return undefined;
    });
    await w.get('[data-testid="export-discard"]').trigger("click");
    await w.get('[data-testid="export-discard"]').trigger("click");
    await flushPromises();

    // MUTATION CHECK (this fix round): guard the close on
    // `sourceBase === base` rather than `sessionId !== null` alone, and
    // this reads `["ses-a:discardProject"]` — A's session, unpinned and
    // removed as a side effect of discarding an unrelated capture B.
    expect(closeCalls).toEqual([]);
    expect(discardCalls).toEqual(["cap two"]);
  });
});
