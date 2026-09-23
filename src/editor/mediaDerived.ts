/**
 * The webview's memo of derived media (Task 28): one in-flight or settled
 * request per (session, asset, buckets) waveform and per (session, asset,
 * time) thumbnail, so ten clips of one recording ask Rust once, and a clip
 * scrolled out of the virtualization window and back does not ask again.
 *
 * A REJECTED request is forgotten at once — a user who installs ffmpeg and
 * scrolls back gets a fresh attempt, not the old refusal. The memo is
 * bounded (`MAX_ENTRIES`, oldest first); Rust's own on-disk cache is the
 * durable one, this only saves round trips.
 */
import type { EditorPort } from "./port";

const MAX_ENTRIES = 256;

const peaks = new Map<string, Promise<number[]>>();
const thumbnails = new Map<string, Promise<string>>();

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
  return remember(thumbnails, `${sessionId}|${assetId}|${atMs}`, () =>
    port.mediaThumbnail(sessionId, assetId, atMs),
  );
}

/** Test-only: every suite starts with an empty memo. */
export function clearMediaDerivedForTest(): void {
  peaks.clear();
  thumbnails.clear();
}
