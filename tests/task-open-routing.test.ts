import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";

import TaskRow from "../src/components/TaskRow.vue";
import type { AggTask } from "../src/types";

const task = (): AggTask => ({
  path: "p", title: "T", status: "new", created: "2026-07-01", done: false,
  due: null, scheduled: null, priority: null, tags: [], list: "", order: null,
  id: null, description: null, vaultId: "v1", vaultName: "V",
});

describe("TaskRow open emit", () => {
  it("emits open with the MouseEvent so the container can route ctrl/meta", async () => {
    const wrapper = mount(TaskRow, {
      props: { task: task(), busy: false, isAggregate: false, editing: false },
    });
    await wrapper.find('[data-testid="task-open"]').trigger("click");
    const ev = wrapper.emitted("open")?.[0]?.[0];
    expect(ev).toBeInstanceOf(MouseEvent);
  });

  it("labels the title button for its default action, not the Obsidian modifier", async () => {
    // A plain click now opens Task Detail, so the accessible name must not claim
    // "in Obsidian" (that's the Ctrl/⌘ shortcut, kept in the tooltip) — Codex P2, PR #76.
    const wrapper = mount(TaskRow, {
      props: { task: task(), busy: false, isAggregate: false, editing: false },
    });
    const btn = wrapper.find('[data-testid="task-open"]');
    expect(btn.attributes("aria-label")).toBe("Open T");
    expect(btn.attributes("aria-label")).not.toContain("Obsidian");
    expect(btn.attributes("title")).toContain("Obsidian"); // the modifier hint stays in the tooltip
  });
});
