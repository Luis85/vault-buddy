/**
 * The editor's export bar (spec §8.3, §10): Save, Discard, the progress
 * readout and the post-save Open.
 *
 * Presentational — no `invoke`, no store, no IPC mock. Every decision it
 * makes is a function of its props, which is exactly what makes this suite
 * able to pin them one at a time. `EditorRoot` owns the commands and the
 * four `screen:export*` events; `tests/editorRoot.test.ts` pins that half.
 */
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import ExportBar from "../src/components/editor/ExportBar.vue";

type Phase = "idle" | "exporting" | "done" | "failed";

function bar(props: Partial<Record<string, unknown>> = {}) {
  return mount(ExportBar, {
    props: {
      phase: "idle" as Phase,
      fraction: 0,
      message: null,
      canSave: true,
      busy: false,
      ...props,
    },
  });
}

describe("ExportBar", () => {
  it("offers Save and Discard while idle and neither while exporting", async () => {
    const w = bar();
    expect(w.find('[data-testid="export-save"]').exists()).toBe(true);
    expect(w.find('[data-testid="export-discard"]').exists()).toBe(true);
    expect(w.find('[data-testid="export-cancel"]').exists()).toBe(false);
    await w.setProps({ phase: "exporting", fraction: 0.4 });
    expect(w.find('[data-testid="export-save"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-discard"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-cancel"]').exists()).toBe(true);
  });

  // The progress element must reflect the fraction, not merely exist. A bar
  // that renders at a fixed width looks identical in a screenshot and tells
  // the user nothing.
  it("renders the progress value it is given, and moves when it changes", async () => {
    const w = bar({ phase: "exporting", fraction: 0.25 });
    const progress = w.find('[data-testid="export-progress"]');
    expect(progress.attributes("aria-valuenow")).toBe("25");
    await w.setProps({ fraction: 0.75 });
    expect(w.find('[data-testid="export-progress"]').attributes("aria-valuenow")).toBe("75");
  });

  // The width the user actually sees is a separate channel from the ARIA
  // value: a bar wired to `aria-valuenow` alone renders a full-width block
  // at 25%, which is the same "looks fine in a screenshot" failure one
  // layer down.
  it("draws the filled width from the same number it announces", async () => {
    const w = bar({ phase: "exporting", fraction: 0.25 });
    expect(w.get('[data-testid="export-progress-fill"]').attributes("style")).toContain("25%");
    await w.setProps({ fraction: 0.6 });
    expect(w.get('[data-testid="export-progress-fill"]').attributes("style")).toContain("60%");
  });

  it("disables Save when there is nothing left to save", () => {
    const w = bar({ canSave: false });
    expect(w.find('[data-testid="export-save"]').attributes("disabled")).toBeDefined();
  });

  it("emits save when Save is clicked", async () => {
    const w = bar();
    await w.find('[data-testid="export-save"]').trigger("click");
    expect(w.emitted("save")).toHaveLength(1);
  });

  it("emits cancel from the exporting state", async () => {
    const w = bar({ phase: "exporting", fraction: 0.5 });
    await w.find('[data-testid="export-cancel"]').trigger("click");
    expect(w.emitted("cancel")).toHaveLength(1);
  });

  // Discard is irreversible: one click must not delete anything.
  it("requires a second click to discard", async () => {
    const w = bar();
    await w.find('[data-testid="export-discard"]').trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
    await w.find('[data-testid="export-discard"]').trigger("click");
    expect(w.emitted("discard")).toHaveLength(1);
  });

  // An armed discard that cannot be disarmed is a trap: the button now
  // deletes the recording on the next stray click, for the rest of the
  // window's life.
  it("lets an armed discard be backed out of", async () => {
    const w = bar();
    await w.find('[data-testid="export-discard"]').trigger("click");
    await w.find('[data-testid="export-discard-keep"]').trigger("click");
    expect(w.find('[data-testid="export-discard-keep"]').exists()).toBe(false);
    await w.find('[data-testid="export-discard"]').trigger("click");
    expect(w.emitted("discard")).toBeUndefined();
  });

  it("disables both verbs while a discard is in flight", () => {
    const w = bar({ busy: true });
    expect(w.find('[data-testid="export-save"]').attributes("disabled")).toBeDefined();
    expect(w.find('[data-testid="export-discard"]').attributes("disabled")).toBeDefined();
  });

  it("hides Save and Discard once the capture has been saved", () => {
    const w = bar({ phase: "done", fraction: 1, message: "Saved to Engineering" });
    expect(w.find('[data-testid="export-save"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-discard"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-open"]').exists()).toBe(true);
    expect(w.text()).toContain("Saved to Engineering");
  });

  it("emits open from the saved state", async () => {
    const w = bar({ phase: "done", fraction: 1, message: "Saved to Engineering" });
    await w.find('[data-testid="export-open"]').trigger("click");
    expect(w.emitted("open")).toHaveLength(1);
  });

  // A failed export is RECOVERABLE: the staged capture is still there, the
  // edit is still on screen, and Save has to stay reachable. Hiding it the
  // way `done` does would strand the recording with only Discard left.
  it("shows the failure and keeps Save offered", () => {
    const w = bar({ phase: "failed", message: "ffmpeg was not found." });
    expect(w.get('[data-testid="export-message"]').text()).toContain("ffmpeg was not found.");
    expect(w.find('[data-testid="export-save"]').exists()).toBe(true);
    expect(w.find('[data-testid="export-open"]').exists()).toBe(false);
  });

  // A cancel is not a failure (spec §14): no message, no banner, and the
  // bar reads exactly as it did before Save was pressed.
  it("shows no message at all when idle", () => {
    const w = bar({ message: null });
    expect(w.find('[data-testid="export-message"]').exists()).toBe(false);
    expect(w.find('[data-testid="export-progress"]').exists()).toBe(false);
  });
});
