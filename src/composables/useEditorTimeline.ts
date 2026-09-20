import { invoke } from "@tauri-apps/api/core";
import { computed, shallowRef } from "vue";

import { logWarning } from "../logging";
import type { TimelineDto } from "../types";
import { outputDurationMs, segmentAtOutputMs } from "../utils/timelineGeometry";

/** Undo/redo as a stack of whole-timeline snapshots, plus spec 10's
 * save-on-every-edit.
 *
 * Spec 8.1 makes every operation return a NEW timeline, which is what lets
 * this be a snapshot stack rather than a log of inverse operations. A
 * timeline is a handful of integer pairs, so a snapshot costs nothing and
 * the correctness argument is "we kept a copy" rather than "our undo of
 * reorder is right".
 *
 * Nothing here ever mutates a timeline or a segment in place: every
 * operation builds new arrays and the stacks are replaced rather than
 * pushed. That is what makes it safe for snapshots to SHARE the segment
 * objects they did not change, and it is why `shallowRef` is the right
 * depth — a deep `ref` would wrap every snapshot in a reactive Proxy for
 * no gain, and a Proxy is not `structuredClone`able (it throws
 * `DataCloneError`), which is the trap waiting for anyone who later
 * reaches for the obvious deep copy. `snapshot` below is that copy, and
 * it is written to work on either. */

/** A caller-independent copy, and the one definition of a timeline's shape.
 *
 * Not `structuredClone`: the argument may be a reactive Proxy (Vue's deep
 * refs hand those out), which `structuredClone` refuses. Rebuilding the two
 * integers by name also normalises key order, which `sameTimeline` below
 * then does not have to care about. */
function snapshot(t: TimelineDto): TimelineDto {
  return {
    segments: t.segments.map((s) => ({
      sourceStartMs: s.sourceStartMs,
      sourceEndMs: s.sourceEndMs,
    })),
  };
}

/** Structural equality. Deliberately not `JSON.stringify` on both sides:
 * that answers "different" for two identical timelines whose segment keys
 * were serialised in a different order, and a timeline arriving over IPC
 * carries whatever order the sender wrote. */
function sameTimeline(a: TimelineDto, b: TimelineDto): boolean {
  return (
    a.segments.length === b.segments.length &&
    a.segments.every(
      (s, i) =>
        s.sourceStartMs === b.segments[i].sourceStartMs &&
        s.sourceEndMs === b.segments[i].sourceEndMs,
    )
  );
}

export function useEditorTimeline(base: string, initial: TimelineDto) {
  // The capture as it was opened, copied so the caller's object cannot
  // redefine "untouched" out from under us later. Everything `isDirty`,
  // `revert` and the null-write rule below say is measured against this.
  const original = snapshot(initial);
  const timeline = shallowRef<TimelineDto>(snapshot(initial));
  const past = shallowRef<TimelineDto[]>([]);
  const future = shallowRef<TimelineDto[]>([]);
  let pending: Promise<void> = Promise.resolve();

  const isDirty = computed(() => !sameTimeline(timeline.value, original));

  /** Write what is on screen — or `null` when what is on screen IS the
   * capture we opened.
   *
   * One rule, one place, so undo and revert cannot disagree about it.
   * Clearing the field rather than storing the whole capture as a
   * one-segment edit is what phase 5's fast path keys on: absent means
   * untouched. Undoing every edit reaches exactly the state revert reaches,
   * so it has to leave the same thing on disk.
   *
   * The writes are chained rather than fired in parallel: they all target
   * one sidecar through a replacing rename, so the last one to be ISSUED
   * must also be the last one to land. */
  function persistCurrent() {
    const value = isDirty.value ? timeline.value : null;
    pending = pending.then(() =>
      invoke("save_capture_timeline", { base, timeline: value }).then(
        () => undefined,
        (e) => {
          // Never throw out of an edit: the edit already landed on screen and
          // undo still works. A failed save means this one operation is not
          // crash-safe, which is worth a log, not a lost interaction.
          logWarning(`save_capture_timeline failed: ${String(e)}`);
        },
      ),
    );
  }

  /** Apply an operation, recording undo and writing only if it CHANGED
   * something. A rejected click that still pushed an undo entry would make
   * Ctrl+Z appear to do nothing while actually consuming a step, and a
   * rejected click that still wrote would make "saved on each edit" a claim
   * about clicks rather than about edits. */
  function apply(next: TimelineDto) {
    if (sameTimeline(next, timeline.value)) return;
    past.value = [...past.value, timeline.value];
    future.value = [];
    timeline.value = next;
    persistCurrent();
  }

  function splitAt(outputMs: number) {
    const index = segmentAtOutputMs(timeline.value, outputMs);
    if (index === null) return;
    const seg = timeline.value.segments[index];
    // Where this segment starts on the OUTPUT clock, which is the only
    // thing standing between output time and the source time we cut at.
    const before = outputDurationMs({ segments: timeline.value.segments.slice(0, index) });
    const cut = seg.sourceStartMs + (outputMs - before);
    // Spec 8.1: a split on a boundary is a no-op, not a zero-length segment
    // — the exporter cannot encode one.
    if (cut <= seg.sourceStartMs || cut >= seg.sourceEndMs) return;
    apply({
      segments: [
        ...timeline.value.segments.slice(0, index),
        { sourceStartMs: seg.sourceStartMs, sourceEndMs: cut },
        { sourceStartMs: cut, sourceEndMs: seg.sourceEndMs },
        ...timeline.value.segments.slice(index + 1),
      ],
    });
  }

  function deleteSegment(index: number) {
    // Spec 8.1: deleting the last segment yields an empty timeline that Save
    // refuses. Refuse it here instead, so the editor cannot reach a state
    // whose only exit is Discard.
    if (timeline.value.segments.length <= 1) return;
    // An index outside the range needs no guard of its own, and a guard here
    // could not be tested: `filter` keeps every segment and `apply` refuses
    // an operation that changed nothing. That redundancy is a property of
    // `filter` specifically — `splice`/`toSpliced` read a negative index from
    // the END and would delete the wrong segment — so a stale selection
    // surviving a reorder is pinned by a test rather than by a branch.
    apply({ segments: timeline.value.segments.filter((_, i) => i !== index) });
  }

  /** Move the segment at `from` so that it ENDS UP at index `to`.
   *
   * `to` is a destination index in `[0, n)`, matching
   * `core::timeline::reorder` — NOT the insertion slot `dropIndex` returns,
   * which runs to `n` inclusive because "dropped at the end" is a real
   * destination there. A caller holding a slot converts first:
   *
   *     const to = slot > from ? slot - 1 : slot;
   *
   * after rejecting the slots that mean "did not move" (`slot === from` and
   * `slot === from + 1`). Passing a raw slot in is refused here rather than
   * reinterpreted: `n` would otherwise be silently clamped into a move the
   * user did not ask for, and the range guard is load-bearing in the other
   * direction too — an out-of-range `from` would splice `undefined` into the
   * timeline instead of doing nothing. */
  function reorder(from: number, to: number) {
    const n = timeline.value.segments.length;
    if (from < 0 || from >= n || to < 0 || to >= n) return;
    const segments = [...timeline.value.segments];
    const [moved] = segments.splice(from, 1);
    segments.splice(to, 0, moved);
    // `from === to` needs no guard of its own: it rebuilds the same order,
    // and `apply` already refuses an operation that changed nothing.
    apply({ segments });
  }

  function undo() {
    if (past.value.length === 0) return;
    const prev = past.value[past.value.length - 1];
    past.value = past.value.slice(0, -1);
    future.value = [...future.value, timeline.value];
    timeline.value = prev;
    persistCurrent();
  }

  function redo() {
    if (future.value.length === 0) return;
    const next = future.value[future.value.length - 1];
    future.value = future.value.slice(0, -1);
    past.value = [...past.value, timeline.value];
    timeline.value = next;
    persistCurrent();
  }

  /** Back to the whole capture. Literally "apply the capture we opened", so
   * it inherits every rule `apply` already carries: it records one undo step,
   * it drops the redo branch, it is a no-op on an untouched timeline, and —
   * through `persistCurrent` — it clears the stored timeline rather than
   * storing a one-segment edit. */
  function revert() {
    apply(snapshot(original));
  }

  return {
    timeline,
    canUndo: computed(() => past.value.length > 0),
    canRedo: computed(() => future.value.length > 0),
    isDirty,
    outputMs: computed(() => outputDurationMs(timeline.value)),
    splitAt,
    deleteSegment,
    reorder,
    undo,
    redo,
    revert,
    /** Await every queued save. Tests use it; the window-close path will. */
    flushPending: () => pending,
  };
}
