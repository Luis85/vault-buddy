import type { EditorPort } from "../../src/editor/port";

/**
 * The one fake `EditorPort` every editor suite builds on (Task 25 fix
 * round 1): every method throws "not stubbed" unless the test overrides it,
 * so a suite states only the IPC it actually exercises, and a NEW port
 * method is added here once instead of to every suite's own copy.
 */
export function fakeEditorPort(overrides: Partial<EditorPort> = {}): EditorPort {
  const unimplemented = (name: string) => (): never => {
    throw new Error(`fakeEditorPort.${name} not stubbed for this test`);
  };
  return {
    openStaged: unimplemented("openStaged"),
    openProject: unimplemented("openProject"),
    listProjects: unimplemented("listProjects"),
    getSnapshot: unimplemented("getSnapshot"),
    execute: unimplemented("execute"),
    save: unimplemented("save"),
    closeSession: unimplemented("closeSession"),
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
    listVaults: unimplemented("listVaults"),
    openScreenCapture: unimplemented("openScreenCapture"),
    webcamBegin: unimplemented("webcamBegin"),
    webcamAppend: unimplemented("webcamAppend"),
    webcamFinish: unimplemented("webcamFinish"),
    webcamDiscard: unimplemented("webcamDiscard"),
    ...overrides,
  };
}
