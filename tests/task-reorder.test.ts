import { clearMocks, mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import Tasks from "../src/components/Tasks.vue";
import { useNotificationsStore } from "../src/stores/notifications";
import type { TaskItem } from "../src/types";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

const task = (title: string, order: number | null): TaskItem => ({
  path: `C:/v/Tasks/${title}.md`,
  title,
  status: "new",
  created: "2026-07-08",
  done: false,
  due: null,
  priority: null,
  tags: [],
  list: "",
  order,
});

function mountManual(
  fixtures: TaskItem[],
  handlers: Partial<Record<string, (args: unknown) => unknown>> = {},
) {
  localStorage.setItem("vault-buddy:task-sort", JSON.stringify({ v1: { key: "manual", dir: "asc" } }));
  const calls: Array<{ cmd: string; args: unknown }> = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "list_tasks") return fixtures;
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [] };
    if (cmd === "update_task") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: "v1" }, attachTo: document.body });
  return { wrapper, calls };
}

// The composable maps clientY against row rects; happy-dom rects are all
// zero, so stack them explicitly: row i occupies y = [i*30, i*30+30).
function stackRowRects(wrapper: ReturnType<typeof mount>) {
  wrapper.findAll('[data-testid="task-row"]').forEach((row, i) => {
    (row.element as HTMLElement).getBoundingClientRect = () =>
      ({ top: i * 30, bottom: i * 30 + 30, height: 30, left: 0, right: 100, width: 100, x: 0, y: i * 30, toJSON: () => ({}) }) as DOMRect;
  });
}

const rowTitles = (wrapper: ReturnType<typeof mount>) =>
  wrapper.findAll('[data-testid="task-open"]').map((r) => r.text());

describe("manual reordering", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => {
    clearMocks();
    localStorage.clear();
    document.body.innerHTML = "";
  });

  it("shows drag handles only in Manual sort", async () => {
    const { wrapper } = mountManual([task("a", 1024), task("b", 2048)]);
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(2);
    // Switching to Default hides them.
    await wrapper.get('[data-testid="task-sort"]').trigger("click");
    await flushPromises();
    (document.body.querySelector('[data-testid="task-sort-option-default"]') as HTMLElement).click();
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(0);
  });

  it("hides the handles while a filter narrows the list", async () => {
    // Reordering a filtered subset would rank against invisible neighbors.
    const many = Array.from({ length: 6 }, (_, i) => task(`t${i}`, (i + 1) * 1024));
    const { wrapper } = mountManual(many);
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(6);
    await wrapper.get('[data-testid="task-filter"]').setValue("t1");
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(0);
  });

  it("ArrowDown on the handle moves the row one slot with a midpoint write", async () => {
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", 2048), task("c", 3072)]);
    await flushPromises();
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")?.args).toEqual({
      id: "v1",
      path: "C:/v/Tasks/a.md",
      patch: { order: 2560 }, // midpoint of b (2048) and c (3072)
    });
    expect(rowTitles(wrapper)).toEqual(["b", "a", "c"]);
  });

  it("a pointer drag to the top writes a below-first rank", async () => {
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", 2048), task("c", 3072)]);
    await flushPromises();
    stackRowRects(wrapper);
    const handle = wrapper.findAll('[data-testid="task-drag"]')[2];
    await handle.trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 75 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 5 })); // above row 0's midpoint
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")?.args).toEqual({
      id: "v1",
      path: "C:/v/Tasks/c.md",
      patch: { order: 0 }, // 1024 - RANK_STEP
    });
    expect(rowTitles(wrapper)).toEqual(["c", "a", "b"]);
  });

  it("Escape cancels an in-flight drag without a write", async () => {
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", 2048)]);
    await flushPromises();
    stackRowRects(wrapper);
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("pointerdown", {
      pointerType: "mouse",
      button: 0,
      clientY: 5,
    });
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 55 }));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
    expect(rowTitles(wrapper)).toEqual(["a", "b"]);
  });

  it("materializes spaced ranks (serialized writes) when neighbors are unranked", async () => {
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", null), task("c", null)]);
    await flushPromises();
    // Manual: ranked a first; unranked b, c follow in default order. Move c
    // (index 2) up one slot — its neighbor b is unranked → materialize.
    await wrapper.findAll('[data-testid="task-drag"]')[2].trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    const writes = calls.filter((c) => c.cmd === "update_task").map((c) => c.args);
    // Final order a, c, b: a already sits at 1*1024; c and b get writes
    // (serialized in the pre-move section order — b then c; each write is
    // independent, so sequence is an implementation detail of the batch).
    expect(writes).toEqual([
      { id: "v1", path: "C:/v/Tasks/b.md", patch: { order: 3072 } },
      { id: "v1", path: "C:/v/Tasks/c.md", patch: { order: 2048 } },
    ]);
    expect(rowTitles(wrapper)).toEqual(["a", "c", "b"]);
  });

  it("reverts the optimistic order and toasts when the write fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper } = mountManual([task("a", 1024), task("b", 2048)], {
      update_task: () => {
        throw new Error("disk full");
      },
    });
    await flushPromises();
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(rowTitles(wrapper)).toEqual(["a", "b"]); // back where it was
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });

  it("reorders a Lists section whose name contains a quote without throwing (Codex #53 re-review)", async () => {
    // is_valid_list_name allows a double quote, so the section key
    // `list:a"b` would break an interpolated attribute selector and throw in
    // querySelectorAll. rowsFor now filters by dataset instead.
    const inQuoted = (title: string, order: number): TaskItem => ({
      ...task(title, order),
      list: 'a"b',
    });
    const { wrapper, calls } = mountManual([inQuoted("x", 1024), inQuoted("y", 2048)]);
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    // Drag handles are present in the quoted-name list section.
    expect(wrapper.findAll('[data-testid="task-drag"]').length).toBeGreaterThan(0);
    // Keyboard reorder invokes rowsFor("list:a\"b") — must not throw, must commit.
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")?.args).toEqual({
      id: "v1",
      path: "C:/v/Tasks/x.md",
      patch: { order: 2048 + 1024 }, // x moved below y → one step past the end
    });
  });

  it("blocks further reorders until an in-flight single-rank write lands (Codex #53 re-review)", async () => {
    // While a midpoint write is pending, every handle must disappear (the
    // view-level guard) so a second reorder can't be computed against the
    // optimistic, not-yet-persisted order — which would diverge if the first
    // write later fails and reverts.
    let resolveWrite!: (v: unknown) => void;
    const { wrapper } = mountManual([task("a", 1024), task("b", 2048), task("c", 3072)], {
      update_task: () => new Promise((r) => (resolveWrite = r)),
    });
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(3);
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    // Guard engaged: no handle is reorderable until the write resolves.
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(0);
    resolveWrite(null);
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(3);
  });

  it("keeps already-written ranks on a partial materialize failure (Codex #53 re-review)", async () => {
    // The batch writes b then c; fail the SECOND write. b already reached
    // disk, so its new rank must stay in memory (matching disk) — a blanket
    // revert would show a phantom un-reorder that a reload contradicts. Only
    // the unwritten c reverts.
    const notifications = useNotificationsStore();
    let n = 0;
    const { wrapper } = mountManual([task("a", 1024), task("b", null), task("c", null)], {
      update_task: () => {
        n += 1;
        if (n >= 2) throw new Error("locked");
        return null;
      },
    });
    await flushPromises();
    await wrapper.findAll('[data-testid="task-drag"]')[2].trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    const rows = (wrapper.vm as unknown as { tasks: { path: string; order: number | null }[] }).tasks;
    const orderOf = (p: string) => rows.find((t) => t.path === p)?.order;
    expect(orderOf("C:/v/Tasks/b.md")).toBe(3072); // written → kept (matches disk)
    expect(orderOf("C:/v/Tasks/c.md")).toBeNull(); // never written → reverted
    expect(orderOf("C:/v/Tasks/a.md")).toBe(1024); // untouched
    expect(notifications.items.some((n2) => n2.kind === "error")).toBe(true);
  });

  it("marks every materialized row busy while the rank writes run (Codex #53 re-review)", async () => {
    // A materialize write and a toggle/edit/archive on the same row are both
    // read-modify-write frontmatter saves. The single-rank path busy-guards
    // its one row; the batch must guard ALL the rows it will write up front,
    // or a concurrent row action could clobber the order mid-batch. Hang the
    // first of the serialized writes and assert both affected rows are busy.
    let resolveFirst!: (v: unknown) => void;
    let n = 0;
    const { wrapper } = mountManual([task("a", 1024), task("b", null), task("c", null)], {
      update_task: () => {
        n += 1;
        return n === 1 ? new Promise((r) => (resolveFirst = r)) : null;
      },
    });
    await flushPromises();
    // Move c up one slot: neighbor b is unranked → materialize writes b then c.
    await wrapper.findAll('[data-testid="task-drag"]')[2].trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    const busy = (wrapper.vm as unknown as { busy: Set<string> }).busy;
    expect(busy.has("C:/v/Tasks/b.md")).toBe(true);
    expect(busy.has("C:/v/Tasks/c.md")).toBe(true);
    expect(busy.has("C:/v/Tasks/a.md")).toBe(false); // untouched, never written
    // Draining the batch clears the guard for every row.
    resolveFirst(null);
    await flushPromises();
    expect(busy.size).toBe(0);
  });
});
