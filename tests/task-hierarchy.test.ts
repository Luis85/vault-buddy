import { mockIPC } from "@tauri-apps/api/mocks";
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { type Ref,ref } from "vue";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

import { useTaskDetail } from "../src/composables/useTaskDetail";
import { useTaskHierarchy } from "../src/composables/useTaskHierarchy";
import { useNotificationsStore } from "../src/stores/notifications";
import type { AggTask } from "../src/types";
import { buildHierarchyInfoByVault, buildParentIndexByVault } from "../src/utils/taskHierarchy";

const task = (o: Partial<AggTask> = {}): AggTask => ({
  path: "/v/Tasks/t.md", title: "T", status: "new", created: "2026-07-01",
  done: false, due: null, scheduled: null, priority: null, tags: [], list: "",
  order: null, id: null, description: null, parentId: null, parentLink: null,
  vaultId: "v1", vaultName: "V", ...o,
});

// Pure resolution: parent/children/progress derived from an already-loaded
// task set, with no IPC involved. Mirrors core::tasks::hierarchy exactly (see
// src/utils/taskHierarchy.ts) so the frontend and core can never disagree
// about the same vault (Codex P2, PR #77).
describe("useTaskHierarchy", () => {
  beforeEach(() => setActivePinia(createPinia()));

  it("resolves the parent and children per vault, ignoring cross-vault ids", () => {
    // Ids are only unique within a vault; the aggregate view must never link across.
    const a = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const b = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const foreign = task({ vaultId: "v2", id: "x", parentId: "p", path: "/v2/x.md" });
    const h = useTaskHierarchy(ref(a), ref([a, b, foreign]));
    expect(h.children.value.map((t) => t.path)).toEqual(["/v1/c.md"]);
  });

  it("renders both rows of an on-disk cycle as top-level, matching core", () => {
    // Core's drop_cyclic_edges removes both edges of A->B->A. Straight id matching
    // would show them as each other's parent/subtask — the two surfaces
    // disagreeing about the same vault (Codex P2, PR #77).
    const a = task({ vaultId: "v1", id: "a", parentId: "b", path: "/v1/a.md" });
    const b = task({ vaultId: "v1", id: "b", parentId: "a", path: "/v1/b.md" });
    const all = ref([a, b]);
    expect(useTaskHierarchy(ref(a), all).parent.value).toBeNull();
    expect(useTaskHierarchy(ref(a), all).children.value).toEqual([]);
    expect(useTaskHierarchy(ref(b), all).parent.value).toBeNull();
  });

  it("resolves nothing through a duplicated id, matching core's ambiguity rule", () => {
    // Two files share "p" after a manual copy or sync conflict. Core renders the
    // child as an orphan; the frontend must agree rather than picking one
    // duplicate and showing a confident but wrong parent (Codex P2, PR #77).
    const p1 = task({ vaultId: "v1", id: "p", path: "/v1/p1.md", title: "One" });
    const p2 = task({ vaultId: "v1", id: "p", path: "/v1/p2.md", title: "Two" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const all = ref([p1, p2, child]);
    expect(useTaskHierarchy(ref(child), all).parent.value).toBeNull();
    expect(useTaskHierarchy(ref(p1), all).children.value).toEqual([]);
  });

  it("counts progress over children", () => {
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const c1 = task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md", done: true, status: "done" });
    const c2 = task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md" });
    const h = useTaskHierarchy(ref(p), ref([p, c1, c2]));
    expect(h.progress.value).toEqual({ done: 1, total: 2 });
  });

  it("resolves no parent when the parent id matches nothing (an orphan)", () => {
    const orphan = task({ vaultId: "v1", id: "c", parentId: "gone", path: "/v1/c.md" });
    const h = useTaskHierarchy(ref(orphan), ref([orphan]));
    expect(h.parent.value).toBeNull();
  });

  it("excludes an archived child from the Subtasks list and its progress count", () => {
    // Fix 1 (subtasks vault-UX-polish increment): allTasks is now archived-
    // inclusive (so an archived PARENT still resolves — see the setParent
    // describe block below), but archiving a task removes it from view
    // everywhere else; it must not resurface as a subtask row just because
    // the loaded set now includes it for resolution purposes.
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const archivedKid = task({
      vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md", status: "archived",
    });
    const openKid = task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md" });
    const h = useTaskHierarchy(ref(p), ref([p, archivedKid, openKid]));
    expect(h.children.value.map((t) => t.path)).toEqual(["/v1/c2.md"]);
    expect(h.progress.value).toEqual({ done: 0, total: 1 });
  });

  it("still resolves an archived task as a parent (only the CHILD list excludes archived)", () => {
    // The other half of Fix 1: an archived task must still resolve AS a
    // parent (that's the whole point of the fix — the parent row must not
    // go blind just because the parent was archived), even though the
    // exclusion above hides archived CHILDREN from its own Subtasks list.
    const archivedParent = task({
      vaultId: "v1", id: "p", path: "/v1/p.md", status: "archived", title: "Old Parent",
    });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const h = useTaskHierarchy(ref(child), ref([archivedParent, child]));
    expect(h.parent.value?.path).toBe("/v1/p.md");
    expect(h.parent.value?.title).toBe("Old Parent");
  });
});

// The write path: setParent(path | null) -> update_task. Fresh parent/child/
// busy fixtures per test (the brief's own listing reuses one `parent`/`child`
// pair across several cases without re-declaring them — recreated here in
// beforeEach instead of sharing mutable module-level objects, so a mutation
// applied by one test's setParent call can never leak into the next).
describe("useTaskHierarchy.setParent", () => {
  let parent: AggTask;
  let child: AggTask;
  let busy: Ref<boolean>;

  beforeEach(() => {
    setActivePinia(createPinia());
    parent = task({ id: "p", path: "/v1/p.md", title: "Parent" });
    child = task({ id: "c", parentId: "p", path: "/v1/c.md", title: "Child" });
    busy = ref(false);
  });

  it("sends the parent PATH and clears with clearParent", async () => {
    const calls: any[] = [];
    mockIPC((cmd, args) => { calls.push([cmd, args]); return cmd === "update_task" ? { id: null, parentId: "p", parentLink: null } : undefined; });
    const h = useTaskHierarchy(ref(child), ref([parent, child]));
    await h.setParent("/v1/p.md");
    expect(calls[0][1].patch).toEqual({ parentPath: "/v1/p.md" });
    await h.setParent(null);
    expect(calls[1][1].patch).toEqual({ clearParent: true });
  });

  it("shares ONE busy guard with useTaskDetail, so a save and a parent write cannot overlap", async () => {
    // Two guards would let both writers read the old document and the later
    // replacement discard the other's edit (Codex P2, PR #77).
    let resolveSave: (() => void) | undefined;
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return cmd === "update_task"
        ? new Promise((r) => { resolveSave = () => r({ id: null, parentId: null, parentLink: null, idsEnabled: false }); })
        : undefined;
    });
    const t = ref(child);
    const detail = useTaskDetail(t);
    const hierarchy = useTaskHierarchy(t, ref([parent, child]), detail.busy);
    const pending = detail.save({ title: "New" }); // slow field write holds the guard
    await new Promise((r) => setTimeout(r));
    expect(detail.busy.value).toBe(true);
    await hierarchy.setParent("/v1/p.md"); // must be suppressed, not raced
    expect(calls.filter((c) => c === "update_task")).toHaveLength(1);
    resolveSave?.();
    await pending;
  });

  it("reloads the whole task set when the write enabled ids, not just the two rows", async () => {
    // The cached set was loaded id-suppressed, so a pre-existing dormant hierarchy
    // stays orphaned unless everything is re-read (Codex P2, PR #77).
    let reloads = 0;
    const reload = async () => { reloads += 1; };
    mockIPC((cmd) => (cmd === "update_task" ? { id: null, parentId: "pid", parentLink: null, idsEnabled: true } : undefined));
    const h = useTaskHierarchy(ref(child), ref([parent, child]), busy, reload);
    await h.setParent("/v1/p.md");
    expect(reloads).toBe(1);
  });

  it("does NOT reload when ids were already enabled (cheap optimistic patch)", async () => {
    let reloads = 0;
    const reload = async () => { reloads += 1; };
    mockIPC((cmd) => (cmd === "update_task" ? { id: null, parentId: "pid", parentLink: null, idsEnabled: false } : undefined));
    const all = ref([parent, child]);
    const h = useTaskHierarchy(ref(child), all, busy, reload);
    await h.setParent("/v1/p.md");
    expect(reloads).toBe(0);
    expect(all.value.find((t) => t.path === "/v1/p.md")!.id).toBe("pid");
  });

  it("writes the stamped id onto the PARENT's cached row (IDs-off bootstrap)", async () => {
    // With IDs off — the default — the loaded parent row has id: null. The backend
    // enables IDs and stamps it, returning parentId. If only the child is updated,
    // the parent's cached id stays null and (since resolution compares ids) the
    // relationship the user just made is invisible until a reload (Codex P1, PR #77).
    const localParent = task({ vaultId: "v1", id: null, path: "/v1/p.md", title: "Parent" });
    const localChild = task({ vaultId: "v1", id: null, path: "/v1/c.md", title: "Child" });
    mockIPC((cmd) => (cmd === "update_task" ? { id: "cid", parentId: "pid", parentLink: "[[Tasks/p]]" } : undefined));
    const all = ref([localParent, localChild]);
    const h = useTaskHierarchy(ref(localChild), all);
    await h.setParent("/v1/p.md");
    expect(all.value.find((t) => t.path === "/v1/p.md")!.id).toBe("pid"); // parent stamped in cache
    expect(localChild.parentId).toBe("pid");
    expect(h.parent.value?.path).toBe("/v1/p.md"); // resolves WITHOUT a reload
  });

  it("surfaces the note when the write turned Task IDs on, and not otherwise", async () => {
    mockIPC((cmd) => (cmd === "update_task" ? { id: null, parentId: "p", parentLink: null, idsEnabled: true } : undefined));
    const notify = vi.spyOn(useNotificationsStore(), "notify");
    await useTaskHierarchy(ref(child), ref([parent, child])).setParent("/v1/p.md");
    expect(notify).toHaveBeenCalledWith("success", expect.stringContaining("Task IDs"), expect.anything());
  });

  it("does not surface the Task IDs note when ids were already enabled", async () => {
    // The "and not otherwise" half of the case above: a routine parent write
    // that does NOT flip the vault's ids setting must stay silent about it.
    mockIPC((cmd) => (cmd === "update_task" ? { id: null, parentId: "pid", parentLink: null, idsEnabled: false } : undefined));
    const notify = vi.spyOn(useNotificationsStore(), "notify");
    await useTaskHierarchy(ref(child), ref([parent, child])).setParent("/v1/p.md");
    expect(notify).not.toHaveBeenCalled();
  });

  it("reverts nothing and surfaces an error on failure, releasing the guard", async () => {
    mockIPC((cmd) => { if (cmd === "update_task") throw new Error("boom"); return undefined; });
    const err = vi.spyOn(useNotificationsStore(), "error");
    const h = useTaskHierarchy(ref(child), ref([parent, child]), busy);
    await h.setParent("/v1/other.md");
    expect(busy.value).toBe(false);
    expect(err).toHaveBeenCalledWith(expect.stringContaining("boom"));
    expect(child.parentId).toBe("p"); // untouched — nothing was applied before the throw
  });

  it("is a no-op while busy is already true (the guard itself, independent of who set it)", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => { calls.push(cmd); return undefined; });
    busy.value = true;
    const h = useTaskHierarchy(ref(child), ref([parent, child]), busy);
    await h.setParent("/v1/p.md");
    expect(calls).toEqual([]);
  });
});

// Task 10's list-level derivation: buildParentIndexByVault (one index per
// distinct vault, since the aggregate view holds every vault's rows in one
// array) and buildHierarchyInfoByVault (every row's parent + open-subtask-
// count pair, built in ONE pass rather than one lookup per row — Task 12's
// perf fix: the list used to call a per-task resolver (an allTasks.find +
// allTasks.filter) once PER ROW, so a render was O(n) work n times over,
// Θ(n²) at list scale, re-run on every reactive update). Tested directly
// here, with full control over array order, rather than only through
// Tasks.vue — a component-level fixture's order is at the mercy of Tasks.
// vue's own sortInPlace (manual sort ties break on title), which would make
// a same-path-collision fixture pass or fail by accident.
describe("buildParentIndexByVault / buildHierarchyInfoByVault", () => {
  it("builds one index per distinct vault, scoped independently", () => {
    const a = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const b = task({ vaultId: "v2", id: "p", path: "/v2/p.md" }); // same id, different vault: not ambiguous
    const child = task({ vaultId: "v2", id: "c", parentId: "p", path: "/v2/c.md" });
    const byVault = buildParentIndexByVault([a, b, child]);
    expect(byVault.get("v1")).toEqual(new Map());
    expect(byVault.get("v2")).toEqual(new Map([["/v2/c.md", "/v2/p.md"]]));
  });

  it("resolves a task's own parent and open-subtask count", () => {
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const openKid = task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md" });
    const doneKid = task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md", done: true });
    const all = [p, openKid, doneKid];
    const byVault = buildParentIndexByVault(all);
    const info = buildHierarchyInfoByVault(all, byVault, new Map());
    expect(info.get("v1")!.get("/v1/p.md")).toEqual({ parent: null, openSubtaskCount: 1 });
    expect(info.get("v1")!.get("/v1/c1.md")!.parent).toBe(p);
  });

  it("never counts an unrelated same-path row from a different vault as an open subtask", () => {
    // Path is only unique WITHIN a vault (the invariant useTaskHierarchy's own
    // children/parent computeds defend the same way) — a count that matched
    // by path alone would double-count a same-path row from another vault.
    const parent = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const openKid = task({ vaultId: "v1", id: "c1", parentId: "p", path: "/Shared.md" });
    const unrelated = task({ vaultId: "v2", id: "x", path: "/Shared.md" });
    const all = [parent, openKid, unrelated];
    const byVault = buildParentIndexByVault(all);
    const info = buildHierarchyInfoByVault(all, byVault, new Map());
    expect(info.get("v1")!.get("/v1/p.md")!.openSubtaskCount).toBe(1);
  });

  it("never resolves a parent through a same-path row in a different vault", () => {
    // Listing the wrong-vault row FIRST means a path-only lookup (dropping
    // the vaultId scoping) would return it — a wrong object, not silence.
    const wrongVaultParent = task({ vaultId: "v2", id: "z", path: "/Shared.md", title: "Wrong" });
    const parent = task({ vaultId: "v1", id: "p", path: "/Shared.md", title: "Right" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const all = [wrongVaultParent, parent, child];
    const byVault = buildParentIndexByVault(all);
    const info = buildHierarchyInfoByVault(all, byVault, new Map());
    expect(info.get("v1")!.get("/v1/c.md")!.parent).toBe(parent);
  });

  it("never resolves a parent through a same-path row in a different vault, whichever is inserted last", () => {
    // The companion of the case above, with the collision reversed (RIGHT
    // vault first, WRONG vault second). buildHierarchyInfoByVault resolves
    // through a Map (last write wins on a key collision), the opposite
    // failure direction from the original find()-based implementation (first
    // match wins) the sibling case above was written against — a vault-
    // scoping guard removed from a map-based rewrite would pass THAT case by
    // accident (the correct row happens to be inserted after the wrong one)
    // while failing this one, so both orderings must be pinned.
    const parent = task({ vaultId: "v1", id: "p", path: "/Shared.md", title: "Right" });
    const wrongVaultParent = task({ vaultId: "v2", id: "z", path: "/Shared.md", title: "Wrong" });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const all = [parent, wrongVaultParent, child];
    const byVault = buildParentIndexByVault(all);
    const info = buildHierarchyInfoByVault(all, byVault, new Map());
    expect(info.get("v1")!.get("/v1/c.md")!.parent?.title).toBe("Right");
  });

  it("builds nothing when every vault's index is empty (Task IDs off — the default)", () => {
    // The perf fix's early-out: with no parent-child edges anywhere (the
    // common case — Task IDs default off), the per-task passes are skipped
    // entirely rather than walked for a trivially null/0 answer everywhere.
    const a = task({ vaultId: "v1", path: "/v1/a.md" });
    const b = task({ vaultId: "v1", path: "/v1/b.md" });
    const byVault = buildParentIndexByVault([a, b]);
    expect(buildHierarchyInfoByVault([a, b], byVault, new Map()).size).toBe(0);
  });

  it("never merges open-subtask counts across two vaults whose PARENT rows share a literal path", () => {
    // Pass 2's own vault-scoping: the path collision above (a same-path
    // CHILD/unrelated row) doesn't exercise the count accumulator at all,
    // because the unrelated row there never resolves a parent-id edge in
    // the first place. This fixture makes both vaults' PARENT resolve to the
    // identical literal path, so an unscoped accumulator would sum the two
    // vaults' child counts into one bucket (2) instead of each vault seeing
    // only its own child (1).
    const parentV1 = task({ vaultId: "v1", id: "p", path: "/Shared-Parent.md" });
    const childV1 = task({ vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md" });
    const parentV2 = task({ vaultId: "v2", id: "p", path: "/Shared-Parent.md" }); // same literal path
    const childV2 = task({ vaultId: "v2", id: "c2", parentId: "p", path: "/v2/c2.md" });
    const all = [parentV1, childV1, parentV2, childV2];
    const byVault = buildParentIndexByVault(all);
    const info = buildHierarchyInfoByVault(all, byVault, new Map());
    expect(info.get("v1")!.get("/Shared-Parent.md")!.openSubtaskCount).toBe(1);
    expect(info.get("v2")!.get("/Shared-Parent.md")!.openSubtaskCount).toBe(1);
  });

  it("excludes an archived task from its former parent's open-subtask count", () => {
    // Fix 1 (subtasks vault-UX-polish increment): archiving a task removes it
    // from view everywhere else; it must not keep inflating its parent's open
    // count. One-directional — the sibling case below shows an archived task
    // still resolves AS a parent.
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const archivedKid = task({
      vaultId: "v1", id: "c1", parentId: "p", path: "/v1/c1.md", status: "archived",
    });
    const openKid = task({ vaultId: "v1", id: "c2", parentId: "p", path: "/v1/c2.md" });
    const all = [p, archivedKid, openKid];
    const byVault = buildParentIndexByVault(all);
    expect(buildHierarchyInfoByVault(all, byVault, new Map()).get("v1")!.get("/v1/p.md")!.openSubtaskCount).toBe(1);
  });

  it("still resolves an archived task as a parent (only the subtask COUNT excludes archived)", () => {
    const archivedParent = task({
      vaultId: "v1", id: "p", path: "/v1/p.md", status: "archived", title: "Old Parent",
    });
    const child = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const all = [archivedParent, child];
    const byVault = buildParentIndexByVault(all);
    expect(buildHierarchyInfoByVault(all, byVault, new Map()).get("v1")!.get("/v1/c.md")!.parent).toBe(archivedParent);
  });

  it("excludes an open child in an archived LIST from its parent's count", () => {
    // GAP-91 (count facet): archiving a list hides it from the Lists view and
    // from count_open_tasks, but the subtask badge kept counting its children —
    // so the badge and the open counts rendered beside it disagreed about the
    // same task, even after a full reload.
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const c = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", list: "Old" });
    const all = [p, c];
    const info = buildHierarchyInfoByVault(all, buildParentIndexByVault(all), new Map([["v1", ["Old"]]]));
    expect(info.get("v1")!.get("/v1/p.md")!.openSubtaskCount).toBe(0);
  });

  it("keys archived lists PER VAULT so one vault cannot suppress another's count", () => {
    // Ids AND archived sets are vault-scoped. A flattened set would let "Old"
    // archived in v1 silently zero an identically-named LIVE list in v2 — the
    // aggregate ("All tasks") view renders both vaults at once.
    const p1 = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const c1 = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", list: "Old" });
    const p2 = task({ vaultId: "v2", id: "p", path: "/v2/p.md" });
    const c2 = task({ vaultId: "v2", id: "c", parentId: "p", path: "/v2/c.md", list: "Old" });
    const all = [p1, c1, p2, c2];
    const info = buildHierarchyInfoByVault(all, buildParentIndexByVault(all), new Map([["v1", ["Old"]]]));
    expect(info.get("v1")!.get("/v1/p.md")!.openSubtaskCount).toBe(0);
    expect(info.get("v2")!.get("/v2/p.md")!.openSubtaskCount).toBe(1);
  });

  it("matches archived list names case-insensitively, like every other surface", () => {
    // archivedMatcher is the ONE membership rule (Lists grouping, the pickers,
    // count_open_tasks); a bespoke comparison here would silently drift from it.
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md" });
    const c = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md", list: "OLD" });
    const all = [p, c];
    const info = buildHierarchyInfoByVault(all, buildParentIndexByVault(all), new Map([["v1", ["old"]]]));
    expect(info.get("v1")!.get("/v1/p.md")!.openSubtaskCount).toBe(0);
  });

  it("still resolves a parent that sits in an archived list (only the COUNT is scoped)", () => {
    // The same one-directional rule the archived-STATUS pair above pins: an
    // archived-list task must not inflate a count, but hiding it from
    // resolution would reintroduce the silent-overwrite bug PR #77 closed.
    const p = task({ vaultId: "v1", id: "p", path: "/v1/p.md", list: "Old", title: "Parent" });
    const c = task({ vaultId: "v1", id: "c", parentId: "p", path: "/v1/c.md" });
    const all = [p, c];
    const info = buildHierarchyInfoByVault(all, buildParentIndexByVault(all), new Map([["v1", ["Old"]]]));
    expect(info.get("v1")!.get("/v1/c.md")!.parent).toBe(p);
  });
});
