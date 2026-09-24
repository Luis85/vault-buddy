/**
 * The webcam takes still OPEN in this window (Task 49): begun, and not yet
 * finished or discarded. Such a take exists only as its `.part` in the
 * project's `takes\`, which a closing session removes — so the close guard
 * reads this record to warn first ("You have an unsaved webcam take").
 *
 * Written by `port.ts` alone (the one place every take call passes
 * through), so no caller can forget to record a take it began. Reactive,
 * so a view can show it; keyed by take id, valued by session id.
 */
import { reactive } from "vue";

const open = reactive(new Map<string, string>());

/** A take was begun in `sessionId`. */
export function noteTakeOpen(sessionId: string, takeId: string): void {
  open.set(takeId, sessionId);
}

/** A take landed (finished, or kept raw) or was discarded. */
export function noteTakeSettled(takeId: string): void {
  open.delete(takeId);
}

/** The ids of `sessionId`'s open takes, in the order they were begun. */
export function openWebcamTakes(sessionId: string): string[] {
  return [...open].filter(([, session]) => session === sessionId).map(([takeId]) => takeId);
}
