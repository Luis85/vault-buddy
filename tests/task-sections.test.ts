import { describe, expect, it } from "vitest";

import type { AggTask } from "../src/types";
import { dateBuckets, listSections, tagSections } from "../src/utils/taskSections";

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
