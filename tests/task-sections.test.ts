import { describe, expect, it } from "vitest";

import type { AggTask } from "../src/types";
import { type Bucket, crossListDropTargetKey, dateBuckets, dropTargetList, listSections, remapListRef, tagSections } from "../src/utils/taskSections";

const t = (title: string, extra: Partial<AggTask> = {}): AggTask => ({
  path: `C:/v/Tasks/${title.replace(/\s+/g, "-")}.md`,
  title,
  status: extra.done ? "done" : "new",
  created: "2026-07-08",
  done: false,
  due: null,
  priority: null,
  tags: [],
  list: "",
  order: null,
  id: null,
  vaultId: "v",
  vaultName: "",
  ...extra,
});

const labels = (b: { label: string | null }[]) => b.map((x) => x.label);

describe("listSections", () => {
  it("orders configured lists first, the rest alphabetically, then No list and Done", () => {
    const tasks = [
      t("W", { list: "Waiting" }),
      t("A", { list: "Archive-ideas" }),
      t("N", { list: "Next" }),
      t("Root"),
      t("Fin", { done: true, status: "done", list: "Next" }),
    ];
    const sections = listSections(tasks, [], ["Next", "Waiting"], { includeEmpty: false, archived: [] });
    expect(labels(sections)).toEqual(["Next", "Waiting", "Archive-ideas", "No list", "Done"]);
    // Headers always render in list mode (label is never null).
    expect(sections.every((s) => s.label !== null)).toBe(true);
  });

  it("merges same-named lists case-insensitively, first casing wins (aggregate)", () => {
    const tasks = [
      t("A", { list: "Next", vaultId: "va", vaultName: "Alpha" }),
      t("B", { list: "next", vaultId: "vb", vaultName: "Beta" }),
    ];
    const sections = listSections(tasks, [], [], { includeEmpty: false, archived: [] });
    expect(sections).toHaveLength(1);
    expect(sections[0].label).toBe("Next");
    expect(sections[0].tasks).toHaveLength(2);
  });

  it("includeEmpty surfaces a fresh task-less list; without it the section is skipped", () => {
    const withEmpty = listSections([t("Root")], ["Someday"], [], { includeEmpty: true, archived: [] });
    expect(labels(withEmpty)).toEqual(["Someday", "No list"]);
    expect(withEmpty[0].tasks).toHaveLength(0);
    const without = listSections([t("Root")], ["Someday"], [], { includeEmpty: false, archived: [] });
    expect(labels(without)).toEqual(["No list"]);
  });

  it("keeps a done task out of its list section (Done owns it)", () => {
    const sections = listSections([t("Fin", { done: true, status: "done", list: "Next" })], [], [], {
      includeEmpty: false,
      archived: [],
    });
    expect(labels(sections)).toEqual(["Done"]);
  });

  it("listOrder names that match nothing are ignored", () => {
    const sections = listSections([t("N", { list: "Next" })], [], ["Ghost", "Next"], {
      includeEmpty: false,
      archived: [],
    });
    expect(labels(sections)).toEqual(["Next"]);
  });

  it("excludes an archived list's section AND its tasks", () => {
    const tasks = [t("Hidden", { list: "Old" }), t("Shown", { list: "Keep" })];
    const secs = listSections(tasks, ["Old", "Keep"], [], { includeEmpty: true, archived: ["Old"] });
    expect(secs.map((s) => s.label)).not.toContain("Old");
    expect(secs.flatMap((s) => s.tasks.map((task) => task.title))).not.toContain("Hidden");
    const keep = secs.find((s) => s.label === "Keep");
    expect(keep?.list).toBe("Keep"); // bucket carries the raw list name
  });
});

describe("moved builders keep their contracts", () => {
  it("dateBuckets buckets and hides headers when nothing is dated", () => {
    const flat = dateBuckets([t("A")], "2026-07-09");
    expect(labels(flat)).toEqual([null]);
    const dated = dateBuckets(
      [t("Over", { due: "2026-07-08" }), t("Now", { due: "2026-07-09" }), t("Soon", { due: "2026-07-10" })],
      "2026-07-09",
    );
    expect(labels(dated)).toEqual(["Overdue", "Today", "Upcoming"]);
  });

  it("tagSections repeats a task under each tag with No tags and Done last", () => {
    const sections = tagSections([
      t("Both", { tags: ["Work", "home"] }),
      t("Plain"),
      t("Fin", { done: true, status: "done" }),
    ]);
    expect(labels(sections)).toEqual(["#home", "#Work", "No tags", "Done"]);
  });
});

describe("dropTargetList (drag-to-move target)", () => {
  const list = (name: string): Bucket => ({ key: `list:${name.toLowerCase()}`, label: name, list: name, tasks: [] });
  it("returns the over-section's list name for a real list", () => {
    expect(dropTargetList(list("B"), "list:a")).toBe("B");
  });
  it("returns '' for the No-list section", () => {
    expect(dropTargetList({ key: "nolist", label: "No list", tasks: [] }, "list:a")).toBe("");
  });
  it("returns null over the same section (a within-section reorder)", () => {
    expect(dropTargetList(list("A"), "list:a")).toBeNull();
  });
  it("returns null over Done or nothing (not a list target)", () => {
    expect(dropTargetList({ key: "done", label: "Done", tasks: [] }, "list:a")).toBeNull();
    expect(dropTargetList(undefined, "list:a")).toBeNull();
  });
});

describe("crossListDropTargetKey (drag target highlight)", () => {
  const b = (key: string, list?: string): Bucket => ({ key, label: key, list, tasks: [] });
  const buckets = [b("list:a", "A"), b("list:b", "B"), b("nolist"), b("done")];
  const drag = (sectionKey: string, overSectionKey: string | null) => ({ sectionKey, overSectionKey });
  it("returns the over-section key when dragging onto a different list", () => {
    expect(crossListDropTargetKey(drag("list:a", "list:b"), "lists", buckets)).toBe("list:b");
    expect(crossListDropTargetKey(drag("list:a", "nolist"), "lists", buckets)).toBe("nolist");
  });
  it("returns null within the same section, over Done, or with no drag", () => {
    expect(crossListDropTargetKey(drag("list:a", "list:a"), "lists", buckets)).toBeNull();
    expect(crossListDropTargetKey(drag("list:a", "done"), "lists", buckets)).toBeNull();
    expect(crossListDropTargetKey(null, "lists", buckets)).toBeNull();
  });
  it("returns null outside Lists grouping (moves only happen under Lists)", () => {
    expect(crossListDropTargetKey(drag("list:a", "list:b"), "dates", buckets)).toBeNull();
    expect(crossListDropTargetKey(drag("list:a", "list:b"), "tags", buckets)).toBeNull();
  });
});

describe("remapListRef (lifecycle reference reconcile)", () => {
  it("rewrites the exact list under the landed name on a rename (case-insensitive)", () => {
    expect(remapListRef("Inbox", "Inbox", "Later")).toBe("Later");
    expect(remapListRef("inbox", "Inbox", "Later")).toBe("Later"); // case-folded match
  });
  it("prefix-rewrites descendants on a rename (a rename moves the subtree)", () => {
    expect(remapListRef("Work/Q3", "Work", "Projects")).toBe("Projects/Q3");
    expect(remapListRef("work/q3/deep", "Work", "Projects")).toBe("Projects/q3/deep");
  });
  it("drops the exact list on an archive/delete (to === null)", () => {
    expect(remapListRef("Inbox", "Inbox", null)).toBeNull();
  });
  it("leaves descendants UNCHANGED on an archive/delete (neither removes children)", () => {
    // A delete never removes sub-lists, so a descendant reference must survive.
    expect(remapListRef("Work/Q3", "Work", null)).toBe("Work/Q3");
  });
  it("leaves an unrelated list untouched", () => {
    expect(remapListRef("Someday", "Inbox", "Later")).toBe("Someday");
    expect(remapListRef("Inboxes", "Inbox", "Later")).toBe("Inboxes"); // not a path segment
  });
});
