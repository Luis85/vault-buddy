import { clearMocks } from "@tauri-apps/api/mocks";
import { flushPromises } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useNotificationsStore } from "../src/stores/notifications";
import { aggTask, mountAggregate, mountAggregateAttached, mountView } from "./helpers/taskMount";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

// The lists / sorting / composer-picker / editor-move half of the Tasks-view
// suite (split from tests/tasks.test.ts when the lists increment landed).
describe("Tasks — lists & sorting", () => {
  beforeEach(() => setActivePinia(createPinia()));
  afterEach(() => clearMocks());

  describe("lists grouping", () => {
    it("shows list sections ordered by listOrder, with empty known lists, No list and Done", async () => {
      const { wrapper } = mountView({
        list_tasks: () => [
          { path: "C:/v/Tasks/w.md", title: "W", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list: "Waiting", order: null },
          { path: "C:/v/Tasks/n.md", title: "N", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list: "Next", order: null },
          { path: "C:/v/Tasks/r.md", title: "Root", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list: "", order: null },
          { path: "C:/v/Tasks/d.md", title: "Fin", status: "done", created: "2026-07-07", done: true, due: null, priority: null, tags: [], list: "Next", order: null },
        ],
        list_task_lists: () => ["Next", "Someday", "Waiting"],
        get_tasks_config: () => ({ tasksFolder: null, defaultList: null, listOrder: ["Next"] }),
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
      const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
      // Next first (listOrder), then alphabetical (Someday empty but known,
      // Waiting), then No list, then Done.
      expect(headers).toEqual(["Next", "Someday", "Waiting", "No list", "Done"]);
    });

    it("aggregate mode fans out list_task_lists and merges same-named lists", async () => {
      const { wrapper, calls } = mountAggregate({
        list_tasks: (args: unknown) => {
          const id = (args as { id: string }).id;
          return id === "va"
            ? [aggTask("va", "Alpha task", "2026-07-08", { list: "Next" })]
            : [aggTask("vb", "Beta task", "2026-07-09", { list: "next" })];
        },
        list_task_lists: (args: unknown) =>
          (args as { id: string }).id === "va" ? ["Next", "Empty-one"] : ["next"],
      });
      await flushPromises();
      expect(calls.filter((c) => c.cmd === "list_task_lists")).toHaveLength(2);
      await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
      const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
      // Merged case-insensitively; first-seen casing in SORT order labels the
      // section (Beta's task is newest → "next"), the tags precedent. The
      // aggregate skips empty lists (no cross-vault noise), so Empty-one and
      // a second Next section must not appear.
      expect(headers).toEqual(["next"]);
      const rows = wrapper.findAll('[data-testid="task-row"]');
      expect(rows).toHaveLength(2);
    });

    it("shows a fresh empty list when the vault has list folders but no tasks (Codex #53 re-review)", async () => {
      // includeEmpty surfaces a task-less list folder — but the view used to
      // short-circuit to "No tasks yet" when tasks.length was 0, hiding both
      // the grouping control and the empty list. The control now stays
      // reachable and the empty section renders in Lists mode.
      const { wrapper } = mountView({
        list_tasks: () => [],
        list_task_lists: () => ["Someday"],
        get_tasks_config: () => ({ tasksFolder: null, defaultList: null, listOrder: [] }),
      });
      await flushPromises();
      // The grouping control is available despite zero tasks.
      await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
      await flushPromises();
      const headers = wrapper.findAll('[data-testid="task-bucket-header"]').map((h) => h.text());
      expect(headers).toContain("Someday");
    });

    it("shows No tasks match, not empty list headers, when a Lists-grouped filter excludes all (Codex #53 re-review)", async () => {
      // includeEmpty must go off while a filter narrows the list: an empty-list
      // header would otherwise mask the "No tasks match" state when the filter
      // excludes everything.
      const many = Array.from({ length: 6 }, (_, i) => ({
        path: `C:/v/Tasks/t${i}.md`, title: `Task ${i}`, status: "new", created: "2026-07-08",
        done: false, due: null, priority: null, tags: [], list: "Work", order: null,
      }));
      const { wrapper } = mountView({
        list_tasks: () => many,
        list_task_lists: () => ["Work", "Someday"],
        get_tasks_config: () => ({ tasksFolder: null, defaultList: null, listOrder: [] }),
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-grouping-lists"]').trigger("click");
      await wrapper.get('[data-testid="task-filter"]').setValue("zzzznope");
      await flushPromises();
      expect(wrapper.findAll('[data-testid="task-bucket-header"]')).toHaveLength(0);
      expect(wrapper.text()).toContain("No tasks match");
    });
  });

  describe("composer list picker", () => {
    it("omits list for an untouched configured default so the backend applies it leniently", async () => {
      // The picker displays the configured default, but an UNTOUCHED add omits
      // list so add_task's list: None path applies (and can leniently degrade)
      // the config default — sending it explicitly would force the write-strict
      // path (Codex #53 re-review). The task still lands in the default list.
      const { wrapper, calls } = mountView({
        get_tasks_config: () => ({ tasksFolder: null, defaultList: "Inbox", listOrder: [] }),
        list_task_lists: () => ["Inbox", "Next"],
      });
      await flushPromises();
      // The picker shows the configured default.
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      expect(wrapper.get('[data-testid="task-add-list"]').text()).toContain("Inbox");
      await wrapper.get('[data-testid="task-input"]').setValue("Defaulted");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).not.toHaveProperty("list");
    });

    it("omits list for an untouched UNSAFE configured default (does not force the strict path)", async () => {
      // A hand-edited unsafe default like ".hidden" must not be sent
      // explicitly — that would take add_task's write-strict path and fail
      // every quick add. Omitting it lets the backend degrade it to the root
      // (Codex #53 re-review).
      const { wrapper, calls } = mountView({
        get_tasks_config: () => ({ tasksFolder: null, defaultList: ".hidden", listOrder: [] }),
        list_task_lists: () => [],
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-input"]').setValue("Safe");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).not.toHaveProperty("list");
    });

    it("omits list on a quick add before the default has loaded (Codex #53)", async () => {
      // The composer is usable before get_tasks_config resolves, so an
      // untouched picker still shows "" while the real default is unknown. A
      // quick add must NOT send "" (the backend would read it as an explicit
      // No-list override and drop the task in the tasks root) — it omits list
      // so the backend applies the configured default freshly.
      let resolveCfg!: (v: unknown) => void;
      const { wrapper, calls } = mountView({
        get_tasks_config: () => new Promise((r) => (resolveCfg = r)),
        list_task_lists: () => ["Inbox"],
      });
      // Deliberately NOT flushing the config promise — add during the window.
      await wrapper.get('[data-testid="task-input"]').setValue("Quick");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      const call = calls.find((c) => c.cmd === "add_task");
      expect(call?.args).not.toHaveProperty("list");
      resolveCfg({ tasksFolder: null, defaultList: "Inbox", listOrder: [] });
    });

    it("picking No list overrides the configured default", async () => {
      const { wrapper, calls } = mountView({
        get_tasks_config: () => ({ tasksFolder: null, defaultList: "Inbox", listOrder: [] }),
        list_task_lists: () => ["Inbox"],
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-input"]').setValue("Rooted");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).toMatchObject({ list: "" });
    });

    it("New list… creates the folder, selects it, and the next add lands there", async () => {
      const { wrapper, calls } = mountView({
        list_task_lists: () => ["Inbox"],
        create_task_list: () => "Someday",
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-.__new__"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-add-list-new-name"]').setValue(" Someday ");
      await wrapper.get('[data-testid="task-add-list-new-confirm"]').trigger("click");
      await flushPromises();
      // The picker trims before emitting (core re-validates the same way).
      expect(calls.find((c) => c.cmd === "create_task_list")?.args).toMatchObject({ id: "v1", name: "Someday" });
      // The picker re-renders showing the created list (new-mode exited).
      expect(wrapper.get('[data-testid="task-add-list"]').text()).toContain("Someday");
      await wrapper.get('[data-testid="task-input"]').setValue("Later");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).toMatchObject({ list: "Someday" });
    });

    it("a failed list creation stays in the flow and raises a toast", async () => {
      const notifications = useNotificationsStore();
      const { wrapper } = mountView({
        create_task_list: () => {
          throw new Error("List names need at least one character");
        },
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-.__new__"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-add-list-new-name"]').setValue("x");
      await wrapper.get('[data-testid="task-add-list-new-confirm"]').trigger("click");
      await flushPromises();
      expect(notifications.items.some((n) => n.kind === "error")).toBe(true);
      // Still in new-list mode for a retry.
      expect(wrapper.find('[data-testid="task-add-list-new-name"]').exists()).toBe(true);
    });

    it("blocks the add while a New list… create is still in flight (Codex #53 re-review)", async () => {
      // creatingList stays true until create_task_list resolves; the picker is
      // untouched (setList runs only on success), so an add now would omit the
      // list and drop the task in the default/root instead of the chosen list.
      let resolveCreate!: (v: string) => void;
      const { wrapper, calls } = mountView({
        list_task_lists: () => ["Inbox"],
        create_task_list: () => new Promise<string>((r) => (resolveCreate = r)),
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      await wrapper.get('[data-testid="task-input"]').setValue("Later");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-.__new__"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-add-list-new-name"]').setValue("Someday");
      await wrapper.get('[data-testid="task-add-list-new-confirm"]').trigger("click");
      await flushPromises();
      // Create pending: Add is disabled and clicking it is a no-op.
      const add = wrapper.get('[data-testid="task-add"]');
      expect((add.element as HTMLButtonElement).disabled).toBe(true);
      await add.trigger("click");
      await flushPromises();
      expect(calls.some((c) => c.cmd === "add_task")).toBe(false);
      // Once the list lands it is selected and the add targets it.
      resolveCreate("Someday");
      await flushPromises();
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).toMatchObject({ list: "Someday" });
    });

    it("a late New list… create does not overwrite a later explicit pick (Codex #53 re-review)", async () => {
      // The picker stays usable while create_task_list is in flight (only Add
      // is blocked), so the user can cancel the inline input and explicitly
      // pick another list. The create's late resolution must NOT setList over
      // that later choice — the next add would land in the just-created list
      // instead of the one shown.
      let resolveCreate!: (v: string) => void;
      const { wrapper, calls } = mountView({
        list_task_lists: () => ["Inbox"],
        create_task_list: () => new Promise<string>((r) => (resolveCreate = r)),
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-.__new__"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-add-list-new-name"]').setValue("Someday");
      await wrapper.get('[data-testid="task-add-list-new-confirm"]').trigger("click");
      await flushPromises();
      // Create pending: cancel the inline control, explicitly pick Inbox.
      await wrapper.get('[data-testid="task-add-list-new-cancel"]').trigger("click");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-Inbox"]') as HTMLElement).click();
      await flushPromises();
      resolveCreate("Someday"); // lands AFTER the explicit pick
      await flushPromises();
      // The later pick wins — display and the add payload both.
      expect(wrapper.get('[data-testid="task-add-list"]').text()).toContain("Inbox");
      await wrapper.get('[data-testid="task-input"]').setValue("Where");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).toMatchObject({ list: "Inbox" });
    });

    it("does not adopt a created list after switching the composer vault mid-create (Codex #53 re-review)", async () => {
      // Aggregate: create a list for va, switch the composer to vb before it
      // resolves — the va list must NOT be selected on the vb composer, or the
      // next add would send it as vb's explicit (wrong) target.
      let resolveCreate!: (v: string) => void;
      const { wrapper, calls } = mountAggregateAttached({
        list_task_lists: () => [],
        get_tasks_config: () => ({ tasksFolder: null, defaultList: null, listOrder: [] }),
        create_task_list: () => new Promise<string>((r) => (resolveCreate = r)),
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      await wrapper.get('[data-testid="task-add-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-list-option-.__new__"]') as HTMLElement).click();
      await flushPromises();
      await wrapper.get('[data-testid="task-add-list-new-name"]').setValue("VaList");
      await wrapper.get('[data-testid="task-add-list-new-confirm"]').trigger("click");
      await flushPromises();
      // create for va is in flight — switch the composer to vb.
      await wrapper.get('[data-testid="task-add-vault"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-vault-option-vb"]') as HTMLElement).click();
      await flushPromises();
      resolveCreate("VaList"); // lands for va, but the composer is on vb now
      await flushPromises();
      await wrapper.get('[data-testid="task-input"]').setValue("OnB");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      const add = calls.find((c) => c.cmd === "add_task");
      expect(add?.args).toMatchObject({ id: "vb" });
      expect(add?.args).not.toHaveProperty("list"); // NOT the stale "VaList"
    });

    it("aggregate: switching the composer vault fetches that vault's lists config", async () => {
      const { wrapper, calls } = mountAggregateAttached({
        get_tasks_config: (args: unknown) =>
          (args as { id: string }).id === "vb"
            ? { tasksFolder: null, defaultList: "Waiting", listOrder: [] }
            : { tasksFolder: null, defaultList: null, listOrder: [] },
        list_task_lists: (args: unknown) =>
          (args as { id: string }).id === "vb" ? ["Waiting"] : [],
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-vault"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-vault-option-vb"]') as HTMLElement).click();
      await flushPromises();
      expect(calls.some((c) => c.cmd === "get_tasks_config" && (c.args as { id: string }).id === "vb")).toBe(true);
      // The add targets vb and omits list (untouched) so the backend applies
      // vb's own configured default — not sent explicitly (Codex #53 re-review).
      await wrapper.get('[data-testid="task-input"]').setValue("Cross");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      const cross = calls.find((c) => c.cmd === "add_task")?.args;
      expect(cross).toMatchObject({ id: "vb" });
      expect(cross).not.toHaveProperty("list");
    });

    it("does not carry the previous vault's default into a quick add after switching (Codex #53 re-review)", async () => {
      // va's default is "Inbox"; switching to vb whose config is still pending
      // must NOT send "Inbox" on a quick add — the composer clears the draft
      // list on switch, so the backend applies vb's own default.
      let resolveVb!: (v: unknown) => void;
      const { wrapper, calls } = mountAggregateAttached({
        get_tasks_config: (args: unknown) => {
          const id = (args as { id: string }).id;
          return id === "va"
            ? { tasksFolder: null, defaultList: "Inbox", listOrder: [] }
            : new Promise((r) => (resolveVb = r)); // vb config stays pending
        },
        list_task_lists: (args: unknown) => ((args as { id: string }).id === "va" ? ["Inbox"] : []),
      });
      await flushPromises(); // va (the initial target) loads → draft follows "Inbox"
      await wrapper.get('[data-testid="task-add-vault"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-vault-option-vb"]') as HTMLElement).click();
      await flushPromises();
      // Quick add before vb's config resolves.
      await wrapper.get('[data-testid="task-input"]').setValue("Quick");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      const call = calls.find((c) => c.cmd === "add_task");
      expect(call?.args).toMatchObject({ id: "vb" });
      expect(call?.args).not.toHaveProperty("list"); // NOT the stale "Inbox"
      resolveVb({ tasksFolder: null, defaultList: "Waiting", listOrder: [] });
    });

    it("still shows the default in the picker after switching to a vault with the SAME default (Codex #53 re-review)", async () => {
      // Both vaults' configured default is "Inbox". The earlier fix mirrored
      // props.defaultList into a ref via a watcher, but a watcher only fires
      // on a value CHANGE — switching va→vb left props.defaultList at "Inbox",
      // so the mirror never re-ran and the switch-cleared picker showed "No
      // list" while the backend still applied the real default. displayList is
      // now a computed that tracks the default reactively, so display and
      // effective target can't drift.
      const { wrapper, calls } = mountAggregateAttached({
        get_tasks_config: () => ({ tasksFolder: null, defaultList: "Inbox", listOrder: [] }),
        list_task_lists: () => ["Inbox"],
      });
      await flushPromises();
      await wrapper.get('[data-testid="task-add-options"]').trigger("click");
      expect(wrapper.get('[data-testid="task-add-list"]').text()).toContain("Inbox");
      // Switch to vb, whose configured default is ALSO "Inbox".
      await wrapper.get('[data-testid="task-add-vault"]').trigger("click");
      await flushPromises();
      (document.body.querySelector('[data-testid="task-add-vault-option-vb"]') as HTMLElement).click();
      await flushPromises();
      // The picker still reflects the default, not a stale "No list".
      expect(wrapper.get('[data-testid="task-add-list"]').text()).toContain("Inbox");
      // And an untouched add still omits list so the backend applies it.
      await wrapper.get('[data-testid="task-input"]').setValue("Cross");
      await wrapper.get('[data-testid="task-add"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "add_task")?.args).not.toHaveProperty("list");
    });
  });

  describe("editor list move", () => {
    const inList = () => [
      { path: "C:/v/Tasks/e.md", title: "Mover", status: "new", created: "2026-07-08", done: false, due: null, priority: null, tags: [], list: "", order: null },
    ];
    async function openEditorAndPick(wrapper: ReturnType<typeof mountView>["wrapper"], listOption: string) {
      await wrapper.get('[data-testid="task-edit"]').trigger("click");
      await wrapper.get('[data-testid="task-edit-list"]').trigger("click");
      await flushPromises();
      (document.body.querySelector(`[data-testid="task-edit-list-option-${listOption}"]`) as HTMLElement).click();
      await flushPromises();
    }

    it("saving a changed list moves the file and adopts the landed path", async () => {
      const { wrapper, calls } = mountView({
        list_tasks: inList,
        list_task_lists: () => ["Inbox"],
        move_task_to_list: () => "C:/v/Tasks/Inbox/e (2).md", // collision suffix
      });
      await flushPromises();
      await openEditorAndPick(wrapper, "Inbox");
      await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "move_task_to_list")?.args).toEqual({
        id: "v1",
        path: "C:/v/Tasks/e.md",
        list: "Inbox",
      });
      // No field changed — the move must not be preceded by an update_task.
      expect(calls.find((c) => c.cmd === "update_task")).toBeUndefined();
      const task = (wrapper.vm as unknown as { tasks: { path: string; list: string }[] }).tasks[0];
      expect(task.path).toBe("C:/v/Tasks/Inbox/e (2).md");
      expect(task.list).toBe("Inbox");
    });

    it("keeping the same list issues no move", async () => {
      const { wrapper, calls } = mountView({ list_tasks: inList, list_task_lists: () => ["Inbox"] });
      await flushPromises();
      await wrapper.get('[data-testid="task-edit"]').trigger("click");
      await wrapper.get('[data-testid="task-edit-title"]').setValue("Renamed");
      await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
      await flushPromises();
      expect(calls.find((c) => c.cmd === "update_task")?.args).toMatchObject({ patch: { title: "Renamed" } });
      expect(calls.find((c) => c.cmd === "move_task_to_list")).toBeUndefined();
    });

    it("a failed move after saved fields keeps the fields and names the move in the toast", async () => {
      const notifications = useNotificationsStore();
      const { wrapper } = mountView({
        list_tasks: inList,
        list_task_lists: () => ["Inbox"],
        move_task_to_list: () => {
          throw new Error("disk full");
        },
      });
      await flushPromises();
      await openEditorAndPick(wrapper, "Inbox");
      await wrapper.get('[data-testid="task-edit-title"]').setValue("Renamed");
      await wrapper.get('[data-testid="task-edit-save"]').trigger("click");
      await flushPromises();
      // The field patch stays applied (never silently half-reverted)…
      expect(wrapper.text()).toContain("Renamed");
      // …and the toast says the MOVE failed, naming the list.
      const err = notifications.items.find((n) => n.kind === "error");
      expect(err?.message).toContain("couldn't move");
      expect(err?.message).toContain("Inbox");
      // The task stays where it was.
      const task = (wrapper.vm as unknown as { tasks: { path: string; list: string }[] }).tasks[0];
      expect(task.list).toBe("");
    });
  });

  describe("sort control", () => {
    afterEach(() => localStorage.clear());

    // All undated so every open task shares ONE date bucket: the sort orders
    // rows WITHIN sections (buckets keep partitioning by design), so a
    // cross-bucket fixture would show bucket order no matter the sort.
    const undated = () => [
      { path: "C:/v/Tasks/a.md", title: "Alpha", status: "new", created: "2026-07-01", done: false, due: null, priority: null, tags: [], list: "", order: null },
      { path: "C:/v/Tasks/b.md", title: "Beta", status: "new", created: "2026-07-02", done: false, due: null, priority: null, tags: [], list: "", order: 1024 },
      { path: "C:/v/Tasks/c.md", title: "Carrot", status: "new", created: "2026-07-03", done: false, due: null, priority: null, tags: [], list: "", order: 2048 },
    ];
    const rowTitles = (wrapper: ReturnType<typeof mountView>["wrapper"]) =>
      wrapper.findAll('[data-testid="task-open"]').map((r) => r.text());

    async function pickSort(wrapper: ReturnType<typeof mountView>["wrapper"], key: string) {
      await wrapper.get('[data-testid="task-sort"]').trigger("click");
      await flushPromises();
      (document.body.querySelector(`[data-testid="task-sort-option-${key}"]`) as HTMLElement).click();
      await flushPromises();
    }

    it("re-sorts rows when a sort key is picked and persists the choice", async () => {
      const { wrapper } = mountView({ list_tasks: undated });
      await flushPromises();
      expect(rowTitles(wrapper)).toEqual(["Carrot", "Beta", "Alpha"]); // default: newest created first
      await pickSort(wrapper, "title");
      expect(rowTitles(wrapper)).toEqual(["Alpha", "Beta", "Carrot"]);
      expect(localStorage.getItem("vault-buddy:task-sort")).toContain('"title"');
    });

    it("direction toggle flips the order and is disabled for Default", async () => {
      const { wrapper } = mountView({ list_tasks: undated });
      await flushPromises();
      const dir = wrapper.get('[data-testid="task-sort-dir"]');
      expect(dir.attributes("disabled")).toBeDefined();
      await pickSort(wrapper, "created");
      // created's natural direction is newest-first…
      expect(rowTitles(wrapper)).toEqual(["Carrot", "Beta", "Alpha"]);
      expect(wrapper.get('[data-testid="task-sort-dir"]').attributes("disabled")).toBeUndefined();
      await wrapper.get('[data-testid="task-sort-dir"]').trigger("click");
      await flushPromises();
      // …and the toggle flips it to oldest-first.
      expect(rowTitles(wrapper)).toEqual(["Alpha", "Beta", "Carrot"]);
    });

    it("loads the persisted per-view sort on mount (manual: ranked first)", async () => {
      localStorage.setItem("vault-buddy:task-sort", JSON.stringify({ v1: { key: "manual", dir: "asc" } }));
      const { wrapper } = mountView({ list_tasks: undated });
      await flushPromises();
      // Ranked (1024, 2048) first by rank, unranked Alpha after.
      expect(rowTitles(wrapper)).toEqual(["Beta", "Carrot", "Alpha"]);
    });
  });
});
