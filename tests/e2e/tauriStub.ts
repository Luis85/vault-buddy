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
 * `EditorRoot.vue`, `CapturePreview.vue`, `TimelineStrip.vue` and `ExportBar`
 * that the build emits is the code under test. Only the IPC replies and the
 * asset URL are ours.
 */

/** The staged capture the editor opens. 1920x1080 matches the fixture clip. */
export const DETAIL = {
  base: "2026-09-21 0848 Screen Capture",
  assetPath: "C:\\Users\\me\\AppData\\Local\\com.vaultbuddy.desktop\\screen-captures\\cap.mp4",
  durationMs: 6000,
  sourceTitle: "Screen 1",
  width: 1920,
  height: 1080,
  recordedAt: "2026-09-21T08:48:00Z",
  timeline: {
    segments: [
      { sourceStartMs: 0, sourceEndMs: 2000 },
      { sourceStartMs: 2000, sourceEndMs: 4000 },
      { sourceStartMs: 4000, sourceEndMs: 6000 },
    ],
  },
};

/** Where the stubbed `convertFileSrc` points the preview. Same-origin on
 *  purpose: a cross-origin media URL drags in questions this test is not
 *  about, and `page.route` fulfils it from the fixture either way. */
export const FIXTURE_VIDEO_URL = "/__fixture__/capture.webm";

export async function installTauriStub(page: Page) {
  await page.addInitScript(
    ({ detail, videoUrl }) => {
      const listeners = new Map<number, unknown>();
      let nextId = 1;

      // Only the three surfaces the editor touches. Anything else returns
      // undefined rather than throwing, so a command added later shows up as
      // a behaviour change in the test rather than an unhandled rejection
      // that could be mistaken for a layout failure.
      const invoke = async (cmd: string, args?: Record<string, unknown>) => {
        if (cmd === "take_editor_request") return detail.base;
        if (cmd === "load_staged_capture") return detail;
        if (cmd === "save_capture_timeline") return null;
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
    { detail: DETAIL, videoUrl: FIXTURE_VIDEO_URL },
  );
}
