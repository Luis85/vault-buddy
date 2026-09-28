/**
 * The webview's memo of derived media (Task 28).
 *
 * **Waveforms** are memoized per (session, asset, buckets), in flight or
 * settled, so ten clips of one recording ask Rust once and a clip scrolled
 * out of the virtualization window and back does not ask again. Peaks are
 * DATA — a settled answer stays true. A REJECTED request is forgotten at
 * once, so a user who installs ffmpeg and scrolls back gets a fresh
 * attempt. The memo is bounded (`MAX_ENTRIES`, oldest first).
 *
 * **Thumbnails** share only an IN-FLIGHT request (fix round 1, review
 * Important 1). A thumbnail answer is a PATH into Rust's LRU-bounded
 * `cache\`, and Rust refreshes a file's LRU position only when asked: a
 * memoized path would stop every refresh, turn the LRU into
 * first-rendered-first-evicted, and hand a remounted clip a path to a file
 * that was since deleted, with nothing ever retrying. Asking again costs
 * one stat and one touch on a hit, and re-renders an evicted frame.
 *
 * **A reconnect** (Task 40) changes the file behind an asset id, which
 * nothing above can see: `forgetDerived` drops that asset's memo entries
 * and bumps its `mediaVersion`, which `ClipWaveform`/`ClipThumbnail` watch.
 */
import { reactive } from "vue";

import type { EditorPort } from "./port";

const MAX_ENTRIES = 256;

const peaks = new Map<string, Promise<number[]>>();
const thumbnailsInFlight = new Map<string, Promise<string>>();

function remember<T>(memo: Map<string, Promise<T>>, key: string, ask: () => Promise<T>): Promise<T> {
  const known = memo.get(key);
  if (known) return known;
  // `then` so a port that throws synchronously still becomes a rejection
  // the caller's own try/catch sees.
  const request = Promise.resolve().then(ask);
  memo.set(key, request);
  request.catch(() => {
    if (memo.get(key) === request) memo.delete(key);
  });
  if (memo.size > MAX_ENTRIES) {
    const oldest = memo.keys().next().value;
    if (oldest !== undefined) memo.delete(oldest);
  }
  return request;
}

export function loadPeaks(port: EditorPort, sessionId: string, assetId: string, buckets: number): Promise<number[]> {
  return remember(peaks, `${sessionId}|${assetId}|${buckets}`, () => port.mediaPeaks(sessionId, assetId, buckets));
}

export function loadThumbnail(port: EditorPort, sessionId: string, assetId: string, atMs: number): Promise<string> {
  const key = `${sessionId}|${assetId}|${atMs}`;
  const inFlight = thumbnailsInFlight.get(key);
  if (inFlight) return inFlight;
  const request = Promise.resolve().then(() => port.mediaThumbnail(sessionId, assetId, atMs));
  thumbnailsInFlight.set(key, request);
  const settled = () => {
    if (thumbnailsInFlight.get(key) === request) thumbnailsInFlight.delete(key);
  };
  request.then(settled, settled);
  return request;
}

/** How many times each asset's FILE changed under it this process — a
 * reconnect bumps it (Task 40 fix round 1). Reactive, so a mounted clip
 * that reads it in its watch source asks for its media again. */
const versions = reactive(new Map<string, number>());

export function mediaVersion(assetId: string): number {
  return versions.get(assetId) ?? 0;
}

function dropAssets(memo: Map<string, unknown>, ids: ReadonlySet<string>): void {
  for (const key of [...memo.keys()]) {
    if (ids.has(key.split("|")[1] ?? "")) memo.delete(key);
  }
}

/** A reconnect replaced these assets' files (GAP-176, webview half): drop
 * their memoized waveforms and shared in-flight thumbnail requests, and
 * bump their version so every mounted clip re-requests. */
export function forgetDerived(assetIds: readonly string[]): void {
  const ids = new Set(assetIds);
  dropAssets(peaks, ids);
  dropAssets(thumbnailsInFlight, ids);
  for (const id of ids) versions.set(id, mediaVersion(id) + 1);
}

/** Test-only: every suite starts with an empty memo. */
export function clearMediaDerivedForTest(): void {
  peaks.clear();
  thumbnailsInFlight.clear();
  versions.clear();
}
