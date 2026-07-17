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
  id: null,
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

// Section wrappers carry data-section-key; stack their rects so sectionAt's
// hit-test resolves (happy-dom rects are all zero otherwise). Section i
// occupies y = [i*60, i*60+60).
function stackSectionRects(wrapper: ReturnType<typeof mount>) {
  wrapper.findAll("[data-section-key]").forEach((sec, i) => {
    (sec.element as HTMLElement).getBoundingClientRect = () =>
      ({ top: i * 60, bottom: i * 60 + 60, height: 60, left: 0, right: 100, width: 100, x: 0, y: i * 60, toJSON: () => ({}) }) as DOMRect;
  });
}

const rowTitles = (wrapper: ReturnType<typeof mount>) =>
  wrapper.findAll('[data-testid="task-open"]').map((r) => r.text());

const inList = (title: string, list: string, order: number): TaskItem => ({
  path: `C:/v/Tasks/${list}/${title}.md`,
  title, status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list, order, id: null,
});

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

  it("re-enables the handles when the filter input hides with stale text (review)", async () => {
    // Archiving below the 5-task threshold hides the filter INPUT, and
    // filteredTasks deliberately ignores the stale query then (the user has
    // no control left to clear it) — the list is unfiltered, every neighbor
    // visible, so reordering is safe. reorderView's hand-rolled
    // `filter === ""` check lacked that showFilter gate and kept the grips
    // hidden forever; it must consult the same filterActive rule the list
    // itself uses.
    const six = Array.from({ length: 6 }, (_, i) => task(`t${i}`, (i + 1) * 1024));
    const { wrapper } = mountManual(six, { set_task_status: () => null });
    await flushPromises();
    await wrapper.get('[data-testid="task-filter"]').setValue("t3");
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(0); // narrowed → no grips
    // Archive the one matching row → 5 tasks remain → the input hides.
    await wrapper.get('[data-testid="task-archive"]').trigger("click");
    await flushPromises();
    expect(wrapper.find('[data-testid="task-filter"]').exists()).toBe(false);
    // The stale "t3" no longer narrows anything: all 5 rows render — and the
    // grips must be back.
    expect(wrapper.findAll('[data-testid="task-row"]')).toHaveLength(5);
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(5);
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
    // While a midpoint write is pending, a second reorder must not be
    // computed against the optimistic, not-yet-persisted order — it would
    // diverge if the first write later fails and reverts. The guard is
    // INERTNESS (aria-disabled + dropped events), never unmounting or
    // `disabled`: both rip the grip out from under keyboard focus, so every
    // Arrow step would strand the user on <body> and cost a re-Tab.
    let resolveWrite!: (v: unknown) => void;
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", 2048), task("c", 3072)], {
      update_task: () => new Promise((r) => (resolveWrite = r)),
    });
    await flushPromises();
    expect(wrapper.findAll('[data-testid="task-drag"]')).toHaveLength(3);
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    // Guard engaged: the grips stay MOUNTED (focus survives) but inert.
    const midFlight = wrapper.findAll('[data-testid="task-drag"]');
    expect(midFlight).toHaveLength(3);
    expect(midFlight.every((g) => g.attributes("aria-disabled") === "true")).toBe(true);
    // A second reorder mid-flight is dropped, not ranked against the
    // unpersisted order.
    await midFlight[2].trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "update_task")).toHaveLength(1);
    resolveWrite(null);
    await flushPromises();
    // Released: grips re-arm and the next reorder goes through.
    const after = wrapper.findAll('[data-testid="task-drag"]');
    expect(after.every((g) => g.attributes("aria-disabled") === undefined)).toBe(true);
    await after[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "update_task")).toHaveLength(2);
  });

  it("keeps keyboard focus on the grip across an Arrow step (a11y: consecutive moves)", async () => {
    // The keyboard fallback is consecutive Arrow presses. The old guard
    // unmounted every grip during the rank write (and `disabled` dropped the
    // busy row's), so each step kicked focus to <body> and the "fallback" was
    // effectively single-shot. The SAME element must stay mounted, enabled,
    // and focused through its own write so the next press keeps working.
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", 2048), task("c", 3072)]);
    await flushPromises();
    const grip = wrapper.findAll('[data-testid="task-drag"]')[0].element as HTMLElement;
    grip.focus();
    expect(document.activeElement).toBe(grip);
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("keydown", { key: "ArrowDown" });
    await flushPromises();
    expect(rowTitles(wrapper)).toEqual(["b", "a", "c"]);
    expect(document.activeElement).toBe(grip);
    // …so the next Arrow continues from where the user stands: a → last.
    grip.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }));
    await flushPromises();
    expect(calls.filter((c) => c.cmd === "update_task")).toHaveLength(2);
    expect(rowTitles(wrapper)).toEqual(["b", "c", "a"]);
  });

  it("a drag ended after the view unmounts never commits (listeners torn down)", async () => {
    // The drag's pointer listeners live on `window`; without scope cleanup a
    // view unmounted mid-drag (panel view switch) leaves them armed, and the
    // eventual pointerup would commit a reorder computed against the dead
    // view's rows.
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", 2048)]);
    await flushPromises();
    stackRowRects(wrapper);
    await wrapper.findAll('[data-testid="task-drag"]')[0].trigger("pointerdown", {
      pointerType: "mouse",
      button: 0,
      clientY: 5,
    });
    window.dispatchEvent(new PointerEvent("pointermove", { clientY: 55 }));
    wrapper.unmount();
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
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

  it("aborts materialization when an affected row is already busy (Codex #53 re-review)", async () => {
    // A neighbor with an in-flight write (e.g. a slow status toggle) sits in
    // the shared busy guard. Reordering an unranked section would materialize
    // ranks for EVERY affected row — including that busy one — starting a
    // second read-modify-write frontmatter save against its file that races
    // the first, so whichever lands last drops the other's change.
    // Materialize can't skip a row (it sets the section's total order), so the
    // whole reorder aborts and writes nothing; the user retries once the save
    // lands.
    const { wrapper, calls } = mountManual([task("a", 1024), task("b", null), task("c", null)]);
    await flushPromises();
    // Simulate an in-flight write on b by seeding the shared per-path guard.
    (wrapper.vm as unknown as { busy: Set<string> }).busy.add("C:/v/Tasks/b.md");
    await wrapper.vm.$nextTick();
    // Move c up one slot: neighbor b is unranked → this would materialize {b, c}.
    await wrapper.findAll('[data-testid="task-drag"]')[2].trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    // Aborted: no order write at all, and the optimistic order never applied.
    expect(calls.some((c) => c.cmd === "update_task")).toBe(false);
    const rows = (wrapper.vm as unknown as { tasks: { path: string; order: number | null }[] }).tasks;
    expect(rows.find((t) => t.path === "C:/v/Tasks/c.md")?.order).toBeNull();
    expect(rowTitles(wrapper)).toEqual(["a", "b", "c"]); // unchanged
  });

  it("reflects an id stamped by a reorder write so copy-id shows without reload (Codex)", async () => {
    // A reorder is an order-only update_task, which stamps + returns a missing
    // id on an id-enabled vault (like a field edit). Capture it here too, not
    // just in the editor save, so the row reveals copy-id immediately.
    const { wrapper } = mountManual([task("a", 1024), task("b", 2048)], {
      update_task: () => "id9",
    });
    await flushPromises();
    // ArrowUp on b (its neighbor a is ranked) → one midpoint writeSingleRank.
    await wrapper.findAll('[data-testid="task-drag"]')[1].trigger("keydown", { key: "ArrowUp" });
    await flushPromises();
    const rows = (wrapper.vm as unknown as { tasks: { title: string; id: string | null }[] }).tasks;
    expect(rows.find((t) => t.title === "b")?.id).toBe("id9");
  });

  it("drags a task onto another list's section to move it (Task 11)", async () => {
    const { wrapper, calls } = mountManual([inList("x", "A", 1024), inList("y", "B", 2048)], {
      move_task_to_list: () => ({ path: "C:/v/Tasks/B/x.md", id: null }),
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackSectionRects(wrapper); // section 0 = list:a [0,60), section 1 = list:b [60,120)
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    // handles[0] is x (in list "A"); release the drag over list "B"'s section.
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 })); // over list:b
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    expect(calls.find((c) => c.cmd === "move_task_to_list")?.args).toEqual({
      id: "v1",
      path: "C:/v/Tasks/A/x.md", // x currently lives under its list-A folder
      list: "B",
    });
    // x optimistically re-homes to B and adopts the landed path.
    const rows = (wrapper.vm as unknown as { tasks: { title: string; list: string; path: string }[] }).tasks;
    const moved = rows.find((t) => t.title === "x");
    expect(moved?.list).toBe("B");
    expect(moved?.path).toBe("C:/v/Tasks/B/x.md");
  });

  it("reflects an id the cross-list move stamps so copy-id shows without reload (Codex)", async () => {
    const { wrapper } = mountManual([inList("x", "A", 1024), inList("y", "B", 2048)], {
      move_task_to_list: () => ({ path: "C:/v/Tasks/B/x.md", id: "movedid9" }),
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackSectionRects(wrapper);
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 }));
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    const rows = (wrapper.vm as unknown as { tasks: { title: string; id: string | null }[] }).tasks;
    expect(rows.find((t) => t.title === "x")?.id).toBe("movedid9");
  });

  it("ignores a drag released over a non-target section like Done (Codex PR #59)", async () => {
    // Dropping a list row onto Done (or any section that isn't a valid list
    // move target) must do NOTHING: the drag gate commits on a different
    // over-section (to allow a slot-unchanged move), but the UI showed neither
    // a target highlight nor the origin's drop line, so persisting a rank from
    // the origin's pointer slot would be a silent surprise.
    const done: TaskItem = {
      path: "C:/v/Tasks/z.md", title: "z", status: "done", created: "2026-07-06",
      done: true, due: null, priority: null, tags: [], list: "", order: null, id: null,
    };
    const { wrapper, calls } = mountManual([inList("x1", "A", 1024), inList("x2", "A", 2048), done]);
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackRowRects(wrapper); // x1 [0,30), x2 [30,60), z [60,90)
    stackSectionRects(wrapper); // section 0 = list:a [0,60), section 1 = done [60,120)
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    // Drag x1 (in list A) and release over the Done section.
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 })); // over done
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    // No reorder write and no move — the drop was a no-op.
    expect(calls.some((c) => c.cmd === "update_task")).toBe(false);
    expect(calls.some((c) => c.cmd === "move_task_to_list")).toBe(false);
    // x1 is unchanged: still in list A, still ranked 1024.
    const rows = (wrapper.vm as unknown as { tasks: { title: string; list: string; order: number | null }[] }).tasks;
    const x1 = rows.find((t) => t.title === "x1");
    expect(x1?.list).toBe("A");
    expect(x1?.order).toBe(1024);
  });

  it("highlights the target section during a cross-list drag and drops the origin's drop line", async () => {
    const { wrapper } = mountManual([inList("x", "A", 1024), inList("y", "B", 2048)], {
      move_task_to_list: () => ({ path: "C:/v/Tasks/B/x.md", id: null }),
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackSectionRects(wrapper); // section 0 = list:a [0,60), section 1 = list:b [60,120)
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 })); // over list:b
    await wrapper.vm.$nextTick();
    // Mid-drag (before release): list:b is ringed as the drop target, list:a is not.
    const section = (key: string) =>
      wrapper.findAll("[data-section-key]").find((s) => s.attributes("data-section-key") === key)!;
    expect(section("list:b").classes()).toContain("ring-2");
    expect(section("list:a").classes()).not.toContain("ring-2");
    // And no row shows the in-section drop-line border while pointing away —
    // the origin's drop line is suppressed during a cross-list drag.
    const rows = wrapper.findAll('[data-testid="task-row"]');
    expect(rows.every((r) => !r.classes().includes("border-violet-400"))).toBe(true);
    window.dispatchEvent(new PointerEvent("pointerup", {})); // end the drag cleanly
    await flushPromises();
  });

  it("retains the last section target when the pointer passes over a gap", async () => {
    // onMove keeps the last known over-section when sectionAt returns null
    // (the pointer is momentarily between sections) so a brief gap on the way
    // to release doesn't silently drop the move target.
    const { wrapper, calls } = mountManual([inList("x", "A", 1024), inList("y", "B", 2048)], {
      move_task_to_list: () => ({ path: "C:/v/Tasks/B/x.md", id: null }),
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackSectionRects(wrapper); // sections span y ∈ [0,120); 500 is below them all
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 })); // over list:b
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 500 })); // over nothing
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    // The move still targets B — the gap retained list:b as the over-section.
    expect(calls.find((c) => c.cmd === "move_task_to_list")?.args).toMatchObject({ list: "B" });
  });

  it("drags a task out to the No list section (list becomes empty)", async () => {
    const { wrapper, calls } = mountManual([inList("x", "A", 1024), task("root", 2048)], {
      move_task_to_list: () => ({ path: "C:/v/Tasks/x.md", id: null }),
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackSectionRects(wrapper); // section 0 = list:a, section 1 = nolist
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 })); // over No list
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    expect(calls.find((c) => c.cmd === "move_task_to_list")?.args).toEqual({
      id: "v1",
      path: "C:/v/Tasks/A/x.md", // x currently lives under its list-A folder
      list: "",
    });
  });

  it("reverts and toasts when a cross-list move fails", async () => {
    const notifications = useNotificationsStore();
    const { wrapper, calls } = mountManual([inList("x", "A", 1024), inList("y", "B", 2048)], {
      move_task_to_list: () => {
        throw new Error("move failed");
      },
    });
    await flushPromises();
    await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
    await flushPromises();
    stackSectionRects(wrapper);
    const handles = wrapper.findAll('[data-testid="task-drag"]');
    await handles[0].trigger("pointerdown", { pointerType: "mouse", button: 0, clientY: 10 });
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 10, clientY: 90 }));
    window.dispatchEvent(new PointerEvent("pointerup", {}));
    await flushPromises();
    expect(calls.some((c) => c.cmd === "move_task_to_list")).toBe(true);
    const rows = (wrapper.vm as unknown as { tasks: { title: string; list: string }[] }).tasks;
    expect(rows.find((t) => t.title === "x")?.list).toBe("A"); // reverted
    expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
  });
});
