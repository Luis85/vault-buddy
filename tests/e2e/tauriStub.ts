import type { Page } from "@playwright/test";

/**
 * Boot the real bundle as the EDITOR window, with no Tauri runtime.
 *
 * `main.ts` reads `getCurrentWindow().label` inside a try/catch and falls back
 * to `"main"`, so a browser with no Tauri installs `BuddyRoot` and the editor
 * is never reached. The label lives at
 * `window.__TAURI_INTERNALS__.metadata.currentWindow.label` (verified against
 * @tauri-apps/api 2.11.1's window.js), so installing that object BEFORE the
 * module graph evaluates is all it takes.
 *
 * This is a stub of the RUNTIME, not of the app: every line of
 * `EditorRoot.vue` and the tutorial editor it mounts that the build emits
 * is the code under test. Only the IPC replies and the asset URL are ours.
 * (Task 59 retired the phase-4 editor, and with it this stub's replies for
 * its sidecar read and timeline write.)
 */

/** The staged capture the editor opens. */
const DETAIL = {
  base: "2026-09-21 0848 Screen Capture",
  durationMs: 6000,
  sourceTitle: "Screen 1",
};

/** Where the stubbed `convertFileSrc` points the preview. Same-origin on
 *  purpose: a cross-origin media URL drags in questions this test is not
 *  about, and `page.route` fulfils it from the fixture either way. */
export const FIXTURE_VIDEO_URL = "/__fixture__/capture.webm";

/**
 * A decode-valid `EditorOpenResult` (`src/editor/decode.ts`'s
 * `decodeOpenResult`/`decodeProject`/`decodeSnapshot`) for `editor_open_staged`
 * — `EditorRoot` opens every drained staged base through
 * `editorProject.openStaged`, so a stub that left this command unanswered
 * would make the store decode `undefined` and throw a `ProtocolError` on
 * every page load a spec drives.
 */
const EDITOR_OPEN_RESULT = {
  snapshot: {
    sessionId: "ses-e2e",
    projectId: "project-e2e",
    revision: 1,
    persistedRevision: null,
    title: DETAIL.sourceTitle,
    durationMs: DETAIL.durationMs,
    canUndo: false,
    canRedo: false,
    undoLabel: null,
    redoLabel: null,
  },
  project: {
    schema: "vault-buddy-video-project/3",
    id: "project-e2e",
    title: DETAIL.sourceTitle,
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [],
    tracks: [],
    clips: [],
    effects: [],
    markers: [],
    transitions: [],
    captions: null,
    destination: { vault: "vault-e2e", folder: "", dated: false },
  },
  workspace: {},
  missing: [],
  sourceBase: DETAIL.base,
  recovered: false,
};

/** What a spec may change about the stub: the project `editor_open_staged`
 * answers with, extra command replies (e.g. the guide's progress), and
 * replies that change call by call (`sequences`: each call to the command
 * takes the next one — the keyboard journey's edits, Task 58). An
 * exhausted sequence answers `undefined`, which the port's decoders refuse
 * loudly, so an unexpected extra edit fails the spec rather than passing. */
export interface StubOptions {
  openResult?: unknown;
  replies?: Record<string, unknown>;
  sequences?: Record<string, unknown[]>;
}

export async function installTauriStub(page: Page, options: StubOptions = {}) {
  await page.addInitScript(
    ({ base, videoUrl, openResult, replies, sequences }) => {
      const listeners = new Map<number, unknown>();
      let nextId = 1;
      // Every command the page invoked, in order — what a spec reads to
      // prove something was NOT sent.
      const invoked: string[] = [];
      (window as unknown as Record<string, unknown>).__invoked = invoked;
      // The same calls with their arguments — what a spec reads to prove
      // WHAT was sent (Task 58's keyboard journey).
      const calls: { cmd: string; args: unknown }[] = [];
      (window as unknown as Record<string, unknown>).__calls = calls;

      // Only the surfaces the editor touches. Anything else returns
      // undefined rather than throwing, so a command added later shows up as
      // a behaviour change in the test rather than an unhandled rejection
      // that could be mistaken for a layout failure.
      const table: Record<string, unknown> = {
        take_editor_request: { kind: "staged", value: base },
        editor_open_staged: openResult,
        editor_get_jobs: [],
        ...replies,
      };
      const invoke = async (cmd: string, args?: Record<string, unknown>) => {
        invoked.push(cmd);
        calls.push({ cmd, args: args ?? null });
        if (Object.prototype.hasOwnProperty.call(sequences, cmd)) return sequences[cmd].shift();
        if (Object.prototype.hasOwnProperty.call(table, cmd)) return table[cmd];
        if (cmd.startsWith("plugin:event|listen")) {
          listeners.set(nextId, args);
          return nextId++;
        }
        if (cmd.startsWith("plugin:event|unlisten")) return null;
        if (cmd.startsWith("plugin:log|")) return null;
        return undefined;
      };

      (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
        metadata: { currentWindow: { label: "editor" }, currentWebview: { label: "editor" } },
        invoke,
        convertFileSrc: () => videoUrl,
        transformCallback: (cb: unknown) => {
          const id = nextId++;
          (window as unknown as Record<string, unknown>)[`_${id}`] = cb;
          return id;
        },
      };
    },
    {
      base: DETAIL.base,
      videoUrl: FIXTURE_VIDEO_URL,
      openResult: options.openResult ?? EDITOR_OPEN_RESULT,
      replies: options.replies ?? {},
      sequences: options.sequences ?? {},
    },
  );
}
