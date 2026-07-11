import { afterEach, describe, expect, it, vi } from "vitest";

import type { AggTask } from "../src/types";
import {
  directionApplies,
  loadSortPref,
  NATURAL_DIR,
  saveSortPref,
  taskComparator,
} from "../src/utils/taskSort";

vi.mock("../src/logging", () => ({ logWarning: vi.fn(), logBreadcrumb: vi.fn() }));

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
  vaultId: "v",
  vaultName: "",
  ...extra,
});

const titles = (tasks: AggTask[]) => tasks.map((x) => x.title);

afterEach(() => localStorage.clear());

describe("taskComparator", () => {
  it("default matches the historical chain (due asc → priority → created desc → title; done last by created desc)", () => {
    const list = [
      t("NoDue", { created: "2026-07-09" }),
      t("Later", { due: "2026-07-20", created: "2026-07-01" }),
      t("Sooner", { due: "2026-07-10", created: "2026-07-01" }),
      t("SoonerHigh", { due: "2026-07-10", priority: "high", created: "2026-07-01" }),
      t("BadDue", { due: "tomorrow", created: "2026-07-08" }),
      t("Done", { done: true, status: "done", due: "2026-07-01", created: "2026-07-09" }),
    ];
    list.sort(taskComparator({ key: "default", dir: "asc" }));
    // The exact order core::tasks::list_tasks produces for the same fixture.
    expect(titles(list)).toEqual(["SoonerHigh", "Sooner", "Later", "NoDue", "BadDue", "Done"]);
  });

  it("due asc and desc both keep undated tasks last", () => {
    const list = () => [
      t("None"),
      t("Early", { due: "2026-07-01" }),
      t("Late", { due: "2026-07-20" }),
    ];
    const asc = list().sort(taskComparator({ key: "due", dir: "asc" }));
    expect(titles(asc)).toEqual(["Early", "Late", "None"]);
    const desc = list().sort(taskComparator({ key: "due", dir: "desc" }));
    expect(titles(desc)).toEqual(["Late", "Early", "None"]);
  });

  it("priority asc puts high first; desc flips; absent stays the middle tier", () => {
    const list = () => [t("Low", { priority: "low" }), t("Mid"), t("High", { priority: "high" })];
    expect(titles(list().sort(taskComparator({ key: "priority", dir: "asc" })))).toEqual([
      "High",
      "Mid",
      "Low",
    ]);
    expect(titles(list().sort(taskComparator({ key: "priority", dir: "desc" })))).toEqual([
      "Low",
      "Mid",
      "High",
    ]);
  });

  it("created desc is newest-first; title asc is alphabetical", () => {
    const byCreated = [t("Old", { created: "2026-07-01" }), t("New", { created: "2026-07-10" })];
    byCreated.sort(taskComparator({ key: "created", dir: "desc" }));
    expect(titles(byCreated)).toEqual(["New", "Old"]);
    const byTitle = [t("Zebra"), t("Apple")];
    byTitle.sort(taskComparator({ key: "title", dir: "asc" }));
    expect(titles(byTitle)).toEqual(["Apple", "Zebra"]);
  });

  it("manual orders ranked tasks by order asc, unranked after them in default order", () => {
    const list = [
      t("Unranked new", { created: "2026-07-10" }),
      t("Second", { order: 2048 }),
      t("First", { order: 1024 }),
      t("Unranked due", { due: "2026-07-09", created: "2026-07-01" }),
    ];
    list.sort(taskComparator({ key: "manual", dir: "asc" }));
    // Ranked first by rank; the unranked keep the familiar default order
    // (dated before undated) so pre-feature tasks don't jump around.
    expect(titles(list)).toEqual(["First", "Second", "Unranked due", "Unranked new"]);
  });

  it("every key keeps done tasks last", () => {
    for (const key of ["due", "priority", "created", "title", "manual"] as const) {
      const list = [t("Done", { done: true, status: "done", order: 1 }), t("Open", { order: 2 })];
      list.sort(taskComparator({ key, dir: "asc" }));
      expect({ key, titles: titles(list) }).toEqual({ key, titles: ["Open", "Done"] });
    }
  });

  it("ties fall through to the default chain (aggregate vault tiebreak included)", () => {
    const a = t("Same", { due: "2026-07-10", vaultName: "Alpha", path: "C:/a/t.md" });
    const b = t("Same", { due: "2026-07-10", vaultName: "Beta", path: "C:/b/t.md" });
    const list = [b, a];
    list.sort(taskComparator({ key: "due", dir: "asc" }));
    expect(list[0].vaultName).toBe("Alpha");
  });
});

describe("sort preference persistence", () => {
  it("round-trips per view and isolates views", () => {
    saveSortPref("all", { key: "due", dir: "desc" });
    saveSortPref("vault-1", { key: "title", dir: "asc" });
    expect(loadSortPref("all")).toEqual({ key: "due", dir: "desc" });
    expect(loadSortPref("vault-1")).toEqual({ key: "title", dir: "asc" });
    expect(loadSortPref("vault-2")).toEqual({ key: "default", dir: "asc" });
  });

  it("degrades corrupted storage to the default pref", () => {
    localStorage.setItem("vault-buddy:task-sort", "not json");
    expect(loadSortPref("all")).toEqual({ key: "default", dir: "asc" });
    localStorage.setItem("vault-buddy:task-sort", JSON.stringify({ all: { key: "bogus", dir: "up" } }));
    expect(loadSortPref("all")).toEqual({ key: "default", dir: "asc" });
    localStorage.setItem("vault-buddy:task-sort", JSON.stringify([1, 2]));
    expect(loadSortPref("all")).toEqual({ key: "default", dir: "asc" });
  });

  it("declares natural directions and where the toggle applies", () => {
    expect(NATURAL_DIR.due).toBe("asc");
    expect(NATURAL_DIR.created).toBe("desc");
    expect(directionApplies("due")).toBe(true);
    expect(directionApplies("default")).toBe(false);
    expect(directionApplies("manual")).toBe(false);
  });
});
