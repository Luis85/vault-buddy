import type { AggTask } from "../types";

// Rank math for manual ordering. Ranks are plain numbers in the task's
// `order:` frontmatter, spaced RANK_STEP apart so a drop between two
// neighbors is a midpoint write to ONE file; only when a gap can't be
// represented (or a neighbor is unranked, making the position
// inexpressible) does a section materialize — every row gets a fresh
// spaced rank, written per file by the caller.

export const RANK_STEP = 1024;

/** The rank landing between two neighbors: midpoint when both exist, one
 * step past the end otherwise, RANK_STEP into empty space. `null` when the
 * midpoint isn't strictly between (float precision exhausted, or disordered
 * neighbors) — the caller materializes. */
export function rankBetween(
  before: number | undefined,
  after: number | undefined,
): number | null {
  if (before === undefined && after === undefined) return RANK_STEP;
  if (before === undefined) return (after as number) - RANK_STEP;
  if (after === undefined) return before + RANK_STEP;
  const mid = (before + after) / 2;
  return mid > before && mid < after ? mid : null;
}

export type ReorderPlan =
  | { kind: "single"; order: number }
  | { kind: "materialize"; orders: Map<string, number> };

/** Plan moving `section[fromIndex]` so it lands at `toIndex` of the final
 * visual order. A single midpoint write when the target slot's neighbors
 * carry usable ranks; a section-wide materialization (path → new rank, only
 * for rows whose rank actually changes) when they don't; `null` for a
 * no-op/out-of-range move. */
export function planReorder(
  section: AggTask[],
  fromIndex: number,
  toIndex: number,
): ReorderPlan | null {
  if (
    fromIndex === toIndex ||
    fromIndex < 0 ||
    toIndex < 0 ||
    fromIndex >= section.length ||
    toIndex >= section.length
  ) {
    return null;
  }
  const finalOrder = section.filter((_, i) => i !== fromIndex);
  finalOrder.splice(toIndex, 0, section[fromIndex]);
  const before = toIndex > 0 ? finalOrder[toIndex - 1] : undefined;
  const after = toIndex < finalOrder.length - 1 ? finalOrder[toIndex + 1] : undefined;
  if ((before && before.order === null) || (after && after.order === null)) {
    return materialize(finalOrder);
  }
  const order = rankBetween(before?.order ?? undefined, after?.order ?? undefined);
  if (order === null) return materialize(finalOrder);
  return { kind: "single", order };
}

function materialize(finalOrder: AggTask[]): ReorderPlan {
  const orders = new Map<string, number>();
  finalOrder.forEach((t, i) => {
    const want = (i + 1) * RANK_STEP;
    if (t.order !== want) orders.set(t.path, want);
  });
  return { kind: "materialize", orders };
}
