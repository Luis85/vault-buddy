import { describe, expect, it } from "vitest";

import type { AggTask } from "../src/types";
import { planReorder, RANK_STEP, rankBetween } from "../src/utils/taskOrder";

const t = (title: string, order: number | null): AggTask => ({
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
  vaultId: "v",
  vaultName: "",
});

describe("rankBetween", () => {
  it("midpoints between two ranks", () => {
    expect(rankBetween(1024, 2048)).toBe(1536);
    expect(rankBetween(1024, 1025)).toBe(1024.5);
  });

  it("steps past the ends and into empty space", () => {
    expect(rankBetween(undefined, 1024)).toBe(0);
    expect(rankBetween(2048, undefined)).toBe(2048 + RANK_STEP);
    expect(rankBetween(undefined, undefined)).toBe(RANK_STEP);
  });

  it("returns null when no representable gap exists", () => {
    // Float precision exhausted (equal neighbors)…
    expect(rankBetween(1024, 1024)).toBeNull();
    // …or disordered neighbors (defensive) — never invent a rank between them.
    expect(rankBetween(2048, 1024)).toBeNull();
  });
});

describe("planReorder", () => {
  it("is a no-op for same-slot or out-of-range moves", () => {
    const s = [t("a", 1024), t("b", 2048)];
    expect(planReorder(s, 0, 0)).toBeNull();
    expect(planReorder(s, 0, 5)).toBeNull();
    expect(planReorder(s, -1, 0)).toBeNull();
  });

  it("plans a single midpoint write between ranked neighbors", () => {
    const s = [t("a", 1024), t("b", 2048), t("c", 3072)];
    // Move c between a and b.
    expect(planReorder(s, 2, 1)).toEqual({ kind: "single", order: 1536 });
    // Move a to the end: past b… wait, past c → 3072 + step.
    expect(planReorder(s, 0, 2)).toEqual({ kind: "single", order: 3072 + RANK_STEP });
  });

  it("materializes when a target neighbor is unranked", () => {
    const s = [t("a", 1024), t("b", null), t("c", 2048)];
    // Move c to slot 1: lands between a (ranked) and b (unranked) → the
    // position is inexpressible without ranking b too.
    const plan = planReorder(s, 2, 1);
    // Final order a, c, b → spaced ranks. Rows already AT their spaced rank
    // are skipped (no pointless writes): a holds 1024 = 1*STEP and c holds
    // 2048 = 2*STEP, so only b needs a write.
    expect(plan).toEqual({ kind: "materialize", orders: new Map([["C:/v/Tasks/b.md", 3 * RANK_STEP]]) });
  });

  it("materializes when the gap between neighbors is exhausted", () => {
    const s = [t("a", 1024), t("b", 1024), t("c", 4096)];
    const plan = planReorder(s, 2, 1); // c between a and b: gap 1024..1024
    expect(plan?.kind).toBe("materialize");
  });

  it("plans single writes at the very top and bottom", () => {
    const s = [t("a", 1024), t("b", 2048)];
    expect(planReorder(s, 1, 0)).toEqual({ kind: "single", order: 1024 - RANK_STEP });
    expect(planReorder(s, 0, 1)).toEqual({ kind: "single", order: 2048 + RANK_STEP });
  });
});
