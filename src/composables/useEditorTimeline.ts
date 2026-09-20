import { invoke } from "@tauri-apps/api/core";
import { computed, ref, shallowRef } from "vue";

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
  // The capture as this editor OPENED it, copied so the caller's object
  // cannot redefine it out from under us later. On a capture opened unedited
  // that is the whole recording; on spec 10's Resume it is the edit the
  // previous session saved. `isDirty` and `revert` both measure against
  // this, and both mean "since this editor opened" — nothing here claims to
  // know what the UNEDITED recording looked like, because nothing here is
  // told its duration.
  const original = snapshot(initial);
  const timeline = shallowRef<TimelineDto>(snapshot(initial));
  const past = shallowRef<TimelineDto[]>([]);
  const future = shallowRef<TimelineDto[]>([]);
  let pending: Promise<void> = Promise.resolve();

  /** Does the timeline on screen differ from the one this editor was opened
   * with? Derived rather than a flag, which is the point: a flag has to be
   * maintained by undo, redo and revert alike.
   *
   * It is NOT "there is unsaved work" — every edit is written immediately,
   * and `saveFailed` below is what speaks to a write that did not land — and
   * it is NOT Rust's `Timeline::is_untouched`, which needs the source
   * duration and belongs to the exporter. */
  const isDirty = computed(() => !sameTimeline(timeline.value, original));

  /** True once a sidecar write has failed and no later one has succeeded.
   *
   * A failed save is not a lost interaction — the edit is on screen and undo
   * still works — but it does mean spec 10's "saved on each edit, so a crash
   * loses at most the last one" no longer holds, and only the user can act
   * on that (spec 10's own disk-pressure case). The log alone left it
   * invisible, which is the half of the diagnostics invariant a swallowed
   * error still fails: `EditorRoot` renders this. */
  const saveFailed = ref(false);

  /** Write the timeline that is on screen. Always the timeline, never
   * `null`.
   *
   * This used to write `null` whenever the timeline equalled the one the
   * editor was handed, under the rule "absent means untouched". The two
   * coincide only for a capture opened unedited. On spec 10's Resume the
   * editor is handed a PREVIOUS SESSION'S EDIT, so undoing back to it — or
   * reverting — cleared the field and told phase 5 the recording had never
   * been touched: the fast path would remux the whole capture and resurrect
   * footage the user had cut, with nothing on screen changing and nothing in
   * the log (C-1).
   *
   * The fix is to stop deciding it here. "Untouched" has exactly one
   * authority, `core::timeline::Timeline::is_untouched(source_duration_ms)`,
   * which this side cannot evaluate (it is never given the duration) and
   * which spec 8.3's fast path already calls. A whole-capture timeline
   * stored here answers that predicate exactly as an absent one does, so the
   * fast path is unchanged — while an edit can no longer be erased by a rule
   * that could not see it. Writing a second copy of `is_untouched` in
   * TypeScript would have made it the third implementation of one idea in
   * this feature (`toSourceMs` is already two, and phase 5 is warned to
   * check them against each other); this leaves it at one.
   *
   * The writes are chained rather than fired in parallel: they all target
   * one sidecar through a replacing rename, so the last one to be ISSUED
   * must also be the last one to land.
   *
   * The error handler sits at the END of the chain rather than inside the
   * `invoke` call's own `then`. Both positions catch a rejected write, which
   * is the only failure reachable today — `invoke` is an `async function`,
   * so it can reject but cannot throw. This position ALSO catches a
   * synchronous throw out of the callback, which would leave `pending`
   * REJECTED and make every later save in the session skip its callback in
   * silence. No fixture can tell the two positions apart, so this is a
   * structural guard against a future edit adding synchronous work here, not
   * a fix for a live bug; it is the same handler, in a wider place, at no
   * cost. `logWarning` is a no-op outside Tauri and swallows its own
   * failures, so the handler cannot re-reject the chain it protects. */
  function persistCurrent() {
    const value = timeline.value;
    pending = pending
      .then(() =>
        invoke("save_capture_timeline", { base, timeline: value }).then(() => {
          saveFailed.value = false;
        }),
      )
      .catch((e: unknown) => {
        // Never throw out of an edit: the edit already landed on screen and
        // undo still works. A failed save means this one operation is not
        // crash-safe, which is worth a log and a banner, not a lost
        // interaction.
        saveFailed.value = true;
        logWarning(`save_capture_timeline failed: ${String(e)}`);
      });
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
    // Round FIRST, so which segment this lands in and where it cuts agree on
    // one integer. `Segment`'s fields are `u64` in the Rust twin, and the
    // playhead arriving here is `el.currentTime * 1000` — a double, in
    // seconds, so essentially never a whole millisecond. Unrounded it minted
    // fractional boundaries `serde_json` refuses to read back as `u64`
    // (phase 5 would drop the field and export the whole capture), and it
    // walked straight past the boundary no-op below: a split at 2999.9999
    // beside an existing 3000 produced a 0.0001 ms segment — the unplayable
    // zero-length segment that guard exists to prevent, reached by
    // arithmetic instead of by the guard (I-2).
    const target = Math.round(outputMs);
    const index = segmentAtOutputMs(timeline.value, target);
    if (index === null) return;
    const seg = timeline.value.segments[index];
    // Where this segment starts on the OUTPUT clock, which is the only
    // thing standing between output time and the source time we cut at.
    const before = outputDurationMs({ segments: timeline.value.segments.slice(0, index) });
    const cut = seg.sourceStartMs + (target - before);
    // Spec 8.1: a split on a boundary is a no-op, not a zero-length segment
    // — the exporter cannot encode one. Only the LOW side needs a guard:
    // `segmentAtOutputMs` answers this index because `target` is strictly
    // BELOW the output time at which the segment ends, so `cut <
    // seg.sourceEndMs` holds by construction. A `cut >= seg.sourceEndMs`
    // arm sat here and was unreachable, so no fixture could ever kill it; it
    // went for the same reason `deleteSegment`'s range guard went below —
    // one policy, not two (m-1). The coupling it rested on is
    // `segmentAtOutputMs`'s strict `<`, pinned by that function's own
    // "a boundary belongs to the segment it STARTS" test.
    if (cut <= seg.sourceStartMs) return;
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

  /** Back to the capture AS THIS EDITOR OPENED IT — every edit made in this
   * session, taken back at once. For a capture opened unedited that IS the
   * whole recording; on spec 10's Resume it is the edit the last session
   * saved, which this deliberately does not throw away.
   *
   * The words said "back to the whole capture", which was true only in the
   * first case and false on every resume (C-1). Making the code true the
   * other way would discard a previous session's work on one click, and
   * would need a source duration this composable is never given. "Undo
   * everything I did since I opened this" is both the safer verb and the one
   * that pairs exactly with `isDirty`, which measures against the same
   * reference.
   *
   * Literally "apply the capture we opened", so it inherits every rule
   * `apply` carries: one undo step, the redo branch dropped, a no-op when
   * nothing has changed, and one sidecar write of the timeline it restored. */
  function revert() {
    apply(snapshot(original));
  }

  return {
    timeline,
    canUndo: computed(() => past.value.length > 0),
    canRedo: computed(() => future.value.length > 0),
    isDirty,
    saveFailed,
    outputMs: computed(() => outputDurationMs(timeline.value)),
    splitAt,
    deleteSegment,
    reorder,
    undo,
    redo,
    revert,
    /** Await every queued save.
     *
     * `EditorRoot.load` awaits this before reading the next capture's
     * sidecar: `load` replaces the composable outright, abandoning this
     * chain, and the chain writes the very file the next load reads. It is
     * NOT awaited on window close — `window_close.rs` answers the editor's X
     * wholly in Rust with `prevent_close()` + `hide()`, so this webview never
     * sees a close event at all (an earlier comment here claimed otherwise). */
    flushPending: () => pending,
  };
}
