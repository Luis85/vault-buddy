import type { EditorPort } from "../../src/editor/port";

/**
 * The one fake `EditorPort` every editor suite builds on (Task 25 fix
 * round 1): every method throws "not stubbed" unless the test overrides it,
 * so a suite states only the IPC it actually exercises, and a NEW port
 * method is added here once instead of to every suite's own copy.
 *
 * "Throws" means REJECTS: every real port method is async, so an unstubbed
 * one returns a rejected promise rather than throwing synchronously. A sync
 * throw skips the caller's own `.catch` — a path production can never take
 * — and once escaped a debounced `saveWorkspace` that outlived its test as
 * an "Unhandled Error" in a later test. A rejection nobody handles still
 * fails the run as an unhandled rejection.
 */
export function fakeEditorPort(overrides: Partial<EditorPort> = {}): EditorPort {
  const unimplemented = (name: string) => (): Promise<never> =>
    Promise.reject(new Error(`fakeEditorPort.${name} not stubbed for this test`));
  return {
    openStaged: unimplemented("openStaged"),
    openProject: unimplemented("openProject"),
    listProjects: unimplemented("listProjects"),
    getSnapshot: unimplemented("getSnapshot"),
    execute: unimplemented("execute"),
    save: unimplemented("save"),
    closeSession: unimplemented("closeSession"),
    discardProject: unimplemented("discardProject"),
    hideWindow: unimplemented("hideWindow"),
    getWorkspace: unimplemented("getWorkspace"),
    saveWorkspace: unimplemented("saveWorkspace"),
    mediaUrl: unimplemented("mediaUrl"),
    mediaPeaks: unimplemented("mediaPeaks"),
    mediaThumbnail: unimplemented("mediaThumbnail"),
    importMedia: unimplemented("importMedia"),
    cancelJob: unimplemented("cancelJob"),
    getJobs: unimplemented("getJobs"),
    importCaptions: unimplemented("importCaptions"),
    exportPackage: unimplemented("exportPackage"),
    importPackage: unimplemented("importPackage"),
    relinkMedia: unimplemented("relinkMedia"),
    startRender: unimplemented("startRender"),
    getProducts: unimplemented("getProducts"),
    restoreProduct: unimplemented("restoreProduct"),
    publishProduct: unimplemented("publishProduct"),
    exportSubtitles: unimplemented("exportSubtitles"),
    getChecks: unimplemented("getChecks"),
    getGuideProgress: unimplemented("getGuideProgress"),
    saveGuideProgress: unimplemented("saveGuideProgress"),
    exportGuideProgress: unimplemented("exportGuideProgress"),
    importGuideProgress: unimplemented("importGuideProgress"),
    exportDiagnostics: unimplemented("exportDiagnostics"),
    listVaults: unimplemented("listVaults"),
    publishDefaults: unimplemented("publishDefaults"),
    openScreenCapture: unimplemented("openScreenCapture"),
    webcamBegin: unimplemented("webcamBegin"),
    webcamAppend: unimplemented("webcamAppend"),
    webcamFinish: unimplemented("webcamFinish"),
    webcamDiscard: unimplemented("webcamDiscard"),
    ...overrides,
  };
}
