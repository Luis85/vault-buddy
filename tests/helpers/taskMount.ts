import { mockIPC } from "@tauri-apps/api/mocks";
import { mount } from "@vue/test-utils";

import Tasks from "../../src/components/Tasks.vue";
import type { TaskItem } from "../../src/types";

// Shared mount fixtures for the Tasks-view suites (tests/tasks.test.ts and
// tests/tasks-lists.test.ts) — the suite outgrew one file when the lists/
// sorting increment landed, and the mocks must stay single-sourced so the
// two files can never drift on what the IPC surface looks like.

const vaultsFixture = [
  { id: "va", name: "Alpha", path: "C:/va", open: false },
  { id: "vb", name: "Beta", path: "C:/vb", open: false },
];

export const aggTask = (
  vault: "va" | "vb",
  title: string,
  created: string,
  extra: Partial<TaskItem> = {},
): TaskItem => ({
  path: `C:/${vault}/Tasks/${title.replace(/\s+/g, "-")}.md`,
  title, status: "new", created, done: false, due: null, priority: null, tags: [], list: "", order: null, ...extra,
});

type Handlers = Partial<Record<string, (args: unknown) => unknown>>;
type Calls = Array<{ cmd: string; args: unknown }>;

export function mountAggregate(handlers: Handlers = {}) {
  const calls: Calls = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "list_vaults") return vaultsFixture;
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], taskIdEnabled: false, taskIdProperty: "task-id" };
    if (cmd === "list_tasks") {
      const id = (args as { id: string }).id;
      return id === "va"
        ? [aggTask("va", "Alpha task", "2026-07-08")]
        : [aggTask("vb", "Beta task", "2026-07-09")];
    }
    if (cmd === "set_task_status") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: null } });
  return { wrapper, calls };
}

export function mountAggregateAttached(handlers: Handlers = {}) {
  const calls: Calls = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "list_vaults") return vaultsFixture;
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], taskIdEnabled: false, taskIdProperty: "task-id" };
    if (cmd === "list_tasks") return [];
    if (cmd === "add_task") {
      const a = args as { id: string; title: string };
      return { path: `C:/${a.id}/Tasks/new.md`, title: a.title, status: "new", created: "2026-07-10", done: false, due: null, priority: null, tags: [], list: "", order: null };
    }
  });
  const wrapper = mount(Tasks, { props: { vaultId: null }, attachTo: document.body });
  return { wrapper, calls };
}

export const sample: TaskItem[] = [
  { path: "C:/v/Tasks/2026-07-08-b.md", title: "B open", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list: "", order: null },
  { path: "C:/v/Tasks/2026-07-06-a.md", title: "A done", status: "done", created: "2026-07-06", done: true, due: null, priority: null, tags: [], list: "", order: null },
];

export function mountView(handlers: Handlers = {}) {
  const calls: Calls = [];
  // Per-item clone, not just a new array: toggle() mutates task.done/status in
  // place on the object it's handed, and sample's objects are shared across
  // tests — a shallow array copy would leak state between tests.
  let list = sample.map((t) => ({ ...t }));
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (handlers[cmd]) return handlers[cmd]!(args);
    if (cmd === "list_task_lists") return [];
    if (cmd === "get_tasks_config") return { tasksFolder: null, defaultList: null, listOrder: [], taskIdEnabled: false, taskIdProperty: "task-id" };
    if (cmd === "list_tasks") return list;
    if (cmd === "add_task") {
      const a = args as { title: string; due?: string; priority?: string; tags?: string[] };
      const created = { path: "C:/v/Tasks/2026-07-08-new.md", title: a.title, status: "new", created: "2026-07-08", done: false, due: a.due ?? null, priority: a.priority ?? null, tags: a.tags ?? [], list: "", order: null };
      list = [created, ...list];
      return created;
    }
    if (cmd === "set_task_status") return null;
  });
  const wrapper = mount(Tasks, { props: { vaultId: "v1" } });
  return { wrapper, calls };
}
