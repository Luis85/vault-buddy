/**
 * The editor's EDITING surface: the operations, what they do to the
 * selection and the playhead the root owns, the strip-to-composable seam,
 * and spec 8.2's keyboard shortcuts.
 *
 * Split out of `editorRoot.test.ts`, which keeps this window's other half —
 * draining the stash, loading and resuming a capture, and the preview's two
 * clocks. The fixtures both halves need are single-sourced in
 * `helpers/editorMount.ts`; only the `vi.mock` calls stay here, because they
 * are hoisted per file.
 */
import { mockConvertFileSrc, mockIPC } from "@tauri-apps/api/mocks";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Nothing here drives `editor:open` — that is the lifecycle half's business —
// so the event module only has to stop the real `listen` reaching IPC.
vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => undefined),
}));

// `mockIPC` installs `__TAURI_INTERNALS__`, which stops `logging.ts` being a
// no-op and routes it through the log plugin — harmless, but it would put
// `plugin:log|log` calls in the recorded array the assertions read.
vi.mock("../src/logging", () => ({
  logBreadcrumb: vi.fn(),
  logWarning: vi.fn(),
}));

import EditorRoot from "../src/roots/EditorRoot.vue";
import {
  DETAIL,
  lastSaved,
  mockEditor,
  open,
  pressed,
  segments,
  sizeStrip,
  THREE,
  video,
} from "./helpers/editorMount";

// The strip's window-level pointerup listener (and the editor's own future
// listeners) must not outlive their test.
enableAutoUnmount(afterEach);

beforeEach(() => mockConvertFileSrc("windows"));

afterEach(() => {
  vi.restoreAllMocks();
});

describe("EditorRoot editing", () => {
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

  // I-1. `selected` is a bare index into an array a split REBUILDS: inserting
  // a block ahead of the selection shifts every later index by one, so the
  // highlight stayed on its old NUMBER while the footage moved under it, and
  // Delete then removed a block the user never selected. No error, no log
  // line, and the sidecar write lands immediately — the one operation in this
  // phase that destroys content.
  //
  // The assertion is on the timeline that was WRITTEN, not on the highlight
  // alone: a remap that moved the highlight to the wrong place would satisfy
  // "something changed" while deleting the same wrong block.
  it("deletes the footage the user selected, not the index they selected", async () => {
    const seen = mockEditor(THREE);
    const w = mount(EditorRoot);
    await flushPromises();

    await w.get('[data-testid="segment-2"]').trigger("click");
    expect(pressed(w)).toBe(2);

    // Split INSIDE the first block, which is entirely ahead of the selection.
    await w.get('[data-testid="preview-scrub"]').setValue("1000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(segments(w)).toHaveLength(4);
    // The same footage (source 4000..6000), one index further along.
    expect(pressed(w)).toBe(3);

    await w.get('[data-testid="editor-delete"]').trigger("click");
    await flushPromises();
    expect(lastSaved(seen)).toEqual([
      { sourceStartMs: 0, sourceEndMs: 1000 },
      { sourceStartMs: 1000, sourceEndMs: 2000 },
      { sourceStartMs: 2000, sourceEndMs: 4000 },
    ]);
  });

  // Undo and redo replace the segment array wholesale, so they re-index it
  // exactly as a split does. Without the remap, undo left `selected` pointing
  // one PAST the end of the restored timeline, where `deleteSegment` filters
  // nothing and `apply` refuses an operation that changed nothing: Delete
  // became a silent no-op instead of deleting the wrong block.
  it("keeps the selection on its own footage across undo and redo", async () => {
    const seen = mockEditor(THREE);
    const w = mount(EditorRoot);
    await flushPromises();

    await w.get('[data-testid="segment-2"]').trigger("click");
    await w.get('[data-testid="preview-scrub"]').setValue("1000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(pressed(w)).toBe(3);

    await w.get('[data-testid="editor-undo"]').trigger("click");
    expect(segments(w)).toHaveLength(3);
    expect(pressed(w)).toBe(2);

    await w.get('[data-testid="editor-redo"]').trigger("click");
    expect(segments(w)).toHaveLength(4);
    expect(pressed(w)).toBe(3);

    await w.get('[data-testid="editor-delete"]').trigger("click");
    await flushPromises();
    expect(lastSaved(seen)).toEqual([
      { sourceStartMs: 0, sourceEndMs: 1000 },
      { sourceStartMs: 1000, sourceEndMs: 2000 },
      { sourceStartMs: 2000, sourceEndMs: 4000 },
    ]);
  });

  // A split DIVIDES the selected block, so neither half is the block that was
  // selected: the selection is dropped rather than guessed at. This is the
  // other half of the remap rule, and the fixture that stops "keep the
  // selection" being implemented as "leave the index alone".
  it("drops the selection when the split divides the selected block", async () => {
    const w = await open(THREE);
    await w.get('[data-testid="segment-1"]').trigger("click");
    expect(pressed(w)).toBe(1);
    // Output 3000 is the middle of the SECOND block.
    await w.get('[data-testid="preview-scrub"]').setValue("3000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(segments(w)).toHaveLength(4);
    expect(pressed(w)).toBeNull();
    expect(w.get('[data-testid="editor-delete"]').attributes("disabled")).toBeDefined();
  });

  // m-3. The playhead lives on the OUTPUT clock and a delete shortens it, so
  // an unclamped playhead claims a moment the timeline no longer has. The DOM
  // clamps the range control's own `value` to its `max`, which is exactly why
  // the BOUND value is what this reads: the desync it hides is the bug.
  it("clamps the playhead into the output a delete just shortened", async () => {
    const w = await open(THREE);
    await w.get('[data-testid="preview-scrub"]').setValue("5000");
    const scrub = () => w.get('[data-testid="preview-scrub"]');
    expect(scrub().attributes("value")).toBe("5000");

    await w.get('[data-testid="segment-2"]').trigger("click");
    await w.get('[data-testid="editor-delete"]').trigger("click");

    expect(scrub().attributes("max")).toBe("4000");
    expect(scrub().attributes("value")).toBe("4000");
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

  // THE SEAM, IN THE OTHER DIRECTION — and the direction that actually pins
  // the conversion. The test above drags RIGHTWARDS, which exercises the
  // `slot > from` branch alone: replacing the whole expression with an
  // unconditional `slot - 1` left the entire suite green (the phase review's
  // I-4). That mutation breaks every leftward drag, because `from = 1,
  // slot = 0` becomes `to = -1`, which `reorder`'s range guard correctly
  // refuses — a drag that silently does nothing, the exact failure the seam
  // was tested for. Asymmetric widths again: with equal blocks a refused
  // reorder and a completed one render identically.
  it("converts a LEFTWARD drop slot into a destination index too", async () => {
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
    // Pick up the SECOND block and drop it at the very front: slot 0, which
    // is not greater than `from`, so it passes through unchanged.
    await w.get('[data-testid="segment-1"]').trigger("pointerdown", { clientX: 180 });
    await strip.trigger("pointerup", { clientX: 4 });
    expect(segments(w)[0].attributes("style")).toContain("60%");
    expect(segments(w)[1].attributes("style")).toContain("40%");
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

  // Windows reports AltGr as Ctrl+Alt, and on several Central-European
  // layouts AltGr+Z or AltGr+Y is how a character is typed. Without the
  // `altKey` clause that keystroke both rewrote the edit and was
  // `preventDefault`ed, so the character never arrived either.
  it("ignores AltGr, which Windows reports as Ctrl+Alt", async () => {
    const w = await open();
    await w.get('[data-testid="preview-scrub"]').setValue("4000");
    await w.get('[data-testid="editor-split"]').trigger("click");
    expect(segments(w)).toHaveLength(2);

    const altGrZ = new KeyboardEvent("keydown", { key: "z", ctrlKey: true, altKey: true, cancelable: true });
    window.dispatchEvent(altGrZ);
    await flushPromises();
    expect(segments(w)).toHaveLength(2);
    // The keystroke must still reach the layout that was producing a
    // character with it.
    expect(altGrZ.defaultPrevented).toBe(false);
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
