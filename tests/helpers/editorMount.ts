import { mockIPC } from "@tauri-apps/api/mocks";
import { flushPromises, mount } from "@vue/test-utils";

import EditorRoot from "../../src/roots/EditorRoot.vue";

/**
 * Shared mount fixtures for the editor-window suites
 * (`tests/editorRoot.test.ts`, the lifecycle half, and
 * `tests/editorEditing.test.ts`, the editing half). The suite outgrew one
 * file when the selection/playhead work landed, and the fixtures must stay
 * single-sourced so the two files can never drift on what the IPC surface or
 * the staged capture looks like — `tests/helpers/taskMount.ts`'s own reason,
 * for the same reason.
 *
 * The `vi.mock` calls stay in each suite: they are hoisted per file, so they
 * cannot live here.
 */

/** `assetPath` is the staged file's OWN absolute path, the way
 * `load_staged_capture` hands it over. It was a bare file name until P-5;
 * `convertFileSrc` joins nothing, so that produced a URL naming no file on
 * disk and matching no entry in the asset protocol's scope. A Windows path
 * on purpose: it is the only platform this ships on, and it is the one whose
 * separators and drive letter have to survive percent-encoding. */
export const STAGED_MP4 =
  "C:\\Users\\me\\AppData\\Local\\com.vaultbuddy.desktop\\screen-captures\\cap one.mp4";

export const DETAIL = {
  base: "cap one",
  assetPath: STAGED_MP4,
  durationMs: 10_000,
  sourceTitle: "Screen 1",
  width: 1920,
  height: 1080,
  recordedAt: "2026-09-20T10:00:00Z",
  timeline: null as unknown,
};

/** Three equal blocks, so a re-index is visible and every block is
 * distinguishable by the source span it carries. */
export const THREE = {
  ...DETAIL,
  durationMs: 6000,
  timeline: {
    segments: [
      { sourceStartMs: 0, sourceEndMs: 2000 },
      { sourceStartMs: 2000, sourceEndMs: 4000 },
      { sourceStartMs: 4000, sourceEndMs: 6000 },
    ],
  },
};

export type Call = Record<string, unknown> & { cmd: string };
type Wrapper = ReturnType<typeof mount>;

/** Serve one staged capture, plus whatever `take_editor_request` should hand
 * back on each successive drain. */
export function mockEditor(
  detail: unknown = DETAIL,
  requests: (string | null)[] = ["cap one"],
  details?: Record<string, unknown>,
) {
  const seen: Call[] = [];
  const queue = [...requests];
  mockIPC((cmd, args) => {
    seen.push({ cmd, ...(args as object) });
    if (cmd === "take_editor_request") return queue.length > 0 ? queue.shift() : null;
    if (cmd === "load_staged_capture") {
      const base = (args as { base: string }).base;
      return details?.[base] ?? detail;
    }
    return undefined;
  });
  return seen;
}

export async function open(
  detail?: unknown,
  requests?: (string | null)[],
  details?: Record<string, unknown>,
) {
  mockEditor(detail, requests, details);
  const w = mount(EditorRoot);
  await flushPromises();
  return w;
}

export function segments(w: Wrapper) {
  return w.findAll('[data-testid^="segment-"]');
}

export function video(w: Wrapper) {
  return w.get('[data-testid="preview-video"]').element as HTMLVideoElement;
}

/** happy-dom rects are zero-sized; the strip reads its own width for a drop.
 * 200px at the origin, so a clientX is the percentage doubled. */
export function sizeStrip(w: Wrapper) {
  const strip = w.get('[data-testid="timeline-strip"]');
  strip.element.getBoundingClientRect = () =>
    ({ left: 0, width: 200, top: 0, height: 64, right: 200, bottom: 64, x: 0, y: 0 }) as DOMRect;
  return strip;
}

/** Which block the strip shows as SELECTED, read the way the user sees it.
 * Asserting the highlight rather than an internal ref is the point: the bug
 * it pins is that the highlight and the footage it pointed at came apart. */
export function pressed(w: Wrapper): number | null {
  const i = segments(w).findIndex((s) => s.attributes("aria-pressed") === "true");
  return i === -1 ? null : i;
}

type Seg = { sourceStartMs: number; sourceEndMs: number };

/** The timeline of the LAST sidecar write — what the editor actually did,
 * rather than what it rendered. */
export function lastSaved(seen: Call[]): Seg[] | undefined {
  const saves = seen.filter((c) => c.cmd === "save_capture_timeline");
  const last = saves[saves.length - 1];
  return last === undefined ? undefined : (last.timeline as { segments: Seg[] }).segments;
}
