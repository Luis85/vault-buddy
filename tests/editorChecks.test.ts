/**
 * Before-you-share checks (Task 54; F-45, F-33, F-38; SCREENS 07):
 * `editor_get_checks`' findings, the Checks dialog that lists them by
 * severity, and the one action per finding that REVEALS its object —
 * selects it, opens its inspector tab or library panel, scrolls the
 * timeline, or opens the surface that fixes it. Render is blocked only by
 * `blocking` findings; warnings are the user's to ship knowingly.
 *
 * The fixture is asymmetric on purpose: clips start at different times on
 * different tracks, and the effect and caption sit on a 2x clip, so a
 * reveal that seeks to a SOURCE time or to the wrong clip lands visibly
 * elsewhere.
 */
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { createPinia, setActivePinia } from "pinia";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import ChecksDialog from "../src/components/editor/dialogs/ChecksDialog.vue";
import RenderDialog from "../src/components/editor/dialogs/RenderDialog.vue";
import LibraryPanel from "../src/components/editor/library/LibraryPanel.vue";
import EditorHeader from "../src/components/editor/shell/EditorHeader.vue";
import MixerPopover from "../src/components/editor/shell/MixerPopover.vue";
import PreviewToolbar from "../src/components/editor/shell/PreviewToolbar.vue";
import TimelineView from "../src/components/editor/timeline/TimelineView.vue";
import NotificationHost from "../src/components/NotificationHost.vue";
import { revealFinding } from "../src/editor/checkReveal";
import { decodeCheckFindings } from "../src/editor/decodeChecks";
import { EditorPortError } from "../src/editor/port";
import { checksDialogOpen, revealSerial } from "../src/editor/revealBus";
import { revealScrollLeft } from "../src/editor/timelineLayout";
import type { CheckFinding, Clip, EditorCommand, EditorOpenResult, Project } from "../src/editorTypes";
import { useEditorChecksStore } from "../src/stores/editorChecks";
import { useEditorProjectStore } from "../src/stores/editorProject";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";
import { useNotificationsStore } from "../src/stores/notifications";
import { fakeEditorPort } from "./helpers/fakeEditorPort";
import { openWithRenders } from "./helpers/renderFixtures";

enableAutoUnmount(afterEach);

const SESSION = "ses-checks";

let findings: CheckFinding[] = [];
let executed: EditorCommand[] = [];

beforeEach(() => {
  setActivePinia(createPinia());
  findings = [];
  executed = [];
  checksDialogOpen.value = false;
});

function clip(id: string, trackId: string, startMs: number, extra: Partial<Clip> = {}): Clip {
  return {
    id,
    asset_id: "cap",
    track_id: trackId,
    name: id,
    start_ms: startMs,
    in_ms: 0,
    out_ms: 4_000,
    fade_in_ms: 0,
    fade_out_ms: 0,
    fade_curve: "linear",
    opacity: 1,
    volume: 1,
    muted: false,
    x: 0,
    y: 0,
    w: 1,
    h: 1,
    ...extra,
  };
}

/** c1 plays [1.5 s, 5.5 s); c2 plays [6 s, 8 s) at 2x (source 0–4 s); s1 is
 * sound on a1 from 2.5 s. The effect and the caption sit on c2 at SOURCE
 * 1–3 s, which is OUTPUT 6.5–7.5 s. */
function project(): Project {
  return {
    schema: "vault-buddy-video-project/3",
    id: "project-c",
    title: "Checks",
    canvas: { width: 1280, height: 720, fps: 30 },
    master_gain: 1,
    assets: [{ id: "cap", kind: "video", name: "capture.mp4", duration_ms: 20_000 }],
    tracks: [
      { id: "v1", kind: "video", name: "Screen", visible: true, locked: false, muted: false, solo: false, volume: 1 },
      { id: "a1", kind: "audio", name: "Voice", visible: true, locked: false, muted: false, solo: false, volume: 1 },
    ],
    clips: [clip("c1", "v1", 1_500), clip("c2", "v1", 6_000, { speed: 2 }), clip("s1", "a1", 2_500)],
    effects: [
      { id: "e1", clip_id: "c2", kind: "mask", start_ms: 1_000, end_ms: 3_000, x: 0.3, y: 0.4, color: "#000000", w: 0.2, h: 0.1 },
    ],
    markers: [],
    transitions: [],
    captions: {
      enabled: true,
      burn_in: true,
      font_size: 30,
      position: "bottom",
      background: true,
      cues: [{ id: "q1", clip_id: "c2", start_ms: 1_000, end_ms: 3_000, text: "Hello" }],
    },
    destination: { vault: "", folder: "", dated: false },
  };
}

function openResult(): EditorOpenResult {
  return {
    snapshot: {
      sessionId: SESSION,
      projectId: "project-c",
      revision: 4,
      persistedRevision: 4,
      title: "Checks",
      durationMs: 8_000,
      canUndo: false,
      canRedo: false,
      undoLabel: null,
      redoLabel: null,
    },
    project: project(),
    workspace: {},
    missing: [],
    sourceBase: null,
    recovered: false,
  };
}

function finding(overrides: Partial<CheckFinding>): CheckFinding {
  const f: CheckFinding = {
    id: "",
    severity: "warning",
    code: "gap",
    message: "A finding",
    target: { kind: "project", id: null },
    action: null,
    ...overrides,
  };
  f.id = f.id || `chk-${f.code}-${f.target.id ?? "project"}`;
  return f;
}

async function openSession() {
  const store = useEditorProjectStore();
  store.setPort(
    fakeEditorPort({
      openStaged: () => Promise.resolve(openResult()),
      getChecks: () => Promise.resolve(findings),
      listVaults: () => Promise.resolve([{ id: "vault-1", name: "Notes" }]),
      execute: (req) => {
        executed.push(req.command);
        return Promise.resolve({ snapshot: { ...openResult().snapshot, revision: req.expectedRevision + 1 }, project: project() });
      },
    }),
  );
  await store.openStaged("base");
  return store;
}

describe("decodeCheckFindings", () => {
  it("decodes the literal the Rust side pins, nulls included", () => {
    const decoded = decodeCheckFindings([
      {
        id: "chk-missingMedia-cap",
        severity: "blocking",
        code: "missingMedia",
        message: '"Asset cap" is missing, and the render needs it. Reconnect the original file.',
        target: { kind: "asset", id: "cap" },
        action: "reconnect",
      },
      {
        id: "chk-emptyProject-project",
        severity: "blocking",
        code: "emptyProject",
        message: "The timeline is empty. Place a clip on it before rendering.",
        target: { kind: "project", id: null },
        action: null,
      },
    ]);
    expect(decoded.map((f) => [f.code, f.target, f.action])).toEqual([
      ["missingMedia", { kind: "asset", id: "cap" }, "reconnect"],
      ["emptyProject", { kind: "project", id: null }, null],
    ]);
  });

  it("refuses an unknown code, a missing action key, a score, and a project target that names an id", () => {
    const good = { id: "x", severity: "info", code: "gap", message: "m", target: { kind: "clip", id: "c1" }, action: "select" };
    expect(() => decodeCheckFindings([{ ...good, code: "quality" }])).toThrow(/code/);
    const { action: _dropped, ...noAction } = good;
    expect(() => decodeCheckFindings([noAction])).toThrow(/action/);
    expect(() => decodeCheckFindings([{ ...good, severity: "score" }])).toThrow(/severity/);
    expect(() => decodeCheckFindings([{ ...good, target: { kind: "project", id: "p" } }])).toThrow(/target/);
    expect(() => decodeCheckFindings([{ ...good, target: { kind: "clip", id: null } }])).toThrow(/target/);
  });
});

describe("each finding's action reveals its target", () => {
  it("reconnect opens the media library and asks it for the Reconnect dialog", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    const before = revealSerial("reconnect");
    const close = revealFinding(finding({ code: "missingMedia", severity: "blocking", target: { kind: "asset", id: "cap" }, action: "reconnect" }));
    expect(close).toBe(true);
    expect(workspace.libraryTab).toBe("media");
    expect(revealSerial("reconnect")).toBe(before + 1);
  });

  it("select on a clip selects it, seeks to it and scrolls the timeline to it", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    const before = revealSerial("timeline");
    revealFinding(finding({ code: "gap", severity: "info", target: { kind: "clip", id: "c2" }, action: "select" }));
    expect(workspace.selectionClipIds).toEqual(["c2"]);
    expect(workspace.selected).toBeNull();
    expect(workspace.playheadMs).toBe(6_000);
    expect(revealSerial("timeline")).toBe(before + 1);
  });

  it("select on a cover selects the cue on its clip and seeks to where it PLAYS", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    const before = revealSerial("inspector");
    revealFinding(finding({ code: "privacyCover", target: { kind: "effect", id: "e1" }, action: "select" }));
    expect(workspace.selectionClipIds).toEqual(["c2"]);
    expect(workspace.selected).toEqual({ type: "effect", id: "e1" });
    // Source 1 s on a 2x clip starting at 6 s: output 6.5 s, never 1 s or 7 s.
    expect(workspace.playheadMs).toBe(6_500);
    expect(revealSerial("inspector")).toBe(before + 1);
  });

  it("openCaptions opens the Captions library on the cue", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    revealFinding(finding({ code: "captionDensity", target: { kind: "caption", id: "q1" }, action: "openCaptions" }));
    expect(workspace.libraryTab).toBe("captions");
    expect(workspace.selected).toEqual({ type: "caption", id: "q1" });
    expect(workspace.playheadMs).toBe(6_500);
  });

  it("openLayout selects the clip (or a hidden track's first clip) on the Layout tab", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    revealFinding(finding({ code: "transparentClip", target: { kind: "clip", id: "c2" }, action: "openLayout" }));
    expect(workspace.selectionClipIds).toEqual(["c2"]);
    expect(workspace.propertyTab).toBe("layout");
    revealFinding(finding({ code: "transparentClip", target: { kind: "track", id: "v1" }, action: "openLayout" }));
    expect(workspace.selectionClipIds).toEqual(["c1"]);
    expect(workspace.playheadMs).toBe(1_500);
  });

  it("openAudio opens the mixer, on the clip's Audio tab when it names one", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    const before = revealSerial("mixer");
    revealFinding(finding({ code: "clipping", target: { kind: "clip", id: "s1" }, action: "openAudio" }));
    expect(workspace.selectionClipIds).toEqual(["s1"]);
    expect(workspace.propertyTab).toBe("audio");
    expect(revealSerial("mixer")).toBe(before + 1);
  });

  it("openWebcam opens the media library's webcam recorder", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    const before = revealSerial("webcam");
    revealFinding(finding({ code: "pendingTake", action: "openWebcam" }));
    expect(workspace.libraryTab).toBe("media");
    expect(revealSerial("webcam")).toBe(before + 1);
  });

  it("reviewCanvas selects the clip and points at the ratio control", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    const before = revealSerial("ratio");
    revealFinding(finding({ code: "canvasReview", target: { kind: "clip", id: "c1" }, action: "reviewCanvas" }));
    expect(workspace.selectionClipIds).toEqual(["c1"]);
    expect(revealSerial("ratio")).toBe(before + 1);
  });

  it("setDestination stays in the dialog: it is answered there, not by revealing anything", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    expect(revealFinding(finding({ code: "noDestination", action: "setDestination" }))).toBe(false);
    expect(workspace.selectionClipIds).toEqual([]);
  });

  it("the surfaces answer: the library tab follows, Reconnect opens, the mixer opens", async () => {
    await openSession();
    const library = mount(LibraryPanel);
    const mixer = mount(MixerPopover, { props: { readPeak: () => 0 } });
    revealFinding(finding({ code: "missingMedia", severity: "blocking", target: { kind: "asset", id: "cap" }, action: "reconnect" }));
    await flushPromises();
    expect(library.get('[data-testid="library-tab-media"]').attributes("aria-selected")).toBe("true");
    expect(library.find('[data-testid="dialog-host"]').exists()).toBe(true);
    revealFinding(finding({ code: "captionOverlap", target: { kind: "caption", id: "q1" }, action: "openCaptions" }));
    await flushPromises();
    expect(library.find('[data-testid="captions-library"]').exists()).toBe(true);
    revealFinding(finding({ code: "allMuted", action: "openAudio" }));
    await flushPromises();
    expect(mixer.find('[data-testid="mixer-popover"]').exists()).toBe(true);
  });
});

describe("ChecksDialog", () => {
  it("summarizes blockers and warnings, groups by severity, and never shows a score", async () => {
    findings = [
      finding({ code: "missingMedia", severity: "blocking", target: { kind: "asset", id: "cap" }, action: "reconnect", message: "Capture is missing." }),
      finding({ code: "privacyCover", severity: "warning", target: { kind: "effect", id: "e1" }, action: "select", message: "Cover." }),
      finding({ code: "clipping", severity: "warning", target: { kind: "clip", id: "s1" }, action: "openAudio", message: "May clip." }),
      finding({ code: "gap", severity: "info", target: { kind: "clip", id: "c2" }, action: "select", message: "A gap." }),
    ];
    await openSession();
    await useEditorChecksStore().refresh();
    const w = mount(ChecksDialog, { props: { open: true } });
    await flushPromises();
    expect(w.get('[data-testid="checks-summary"]').text()).toContain("1 blocker · 2 review warnings");
    expect(w.get('[data-testid="checks-group-blocking"]').text()).toContain("Capture is missing.");
    expect(w.get('[data-testid="checks-group-warning"]').findAll("li")).toHaveLength(2);
    expect(w.get('[data-testid="checks-group-info"]').text()).toContain("A gap.");
    // The one mention of a score is the sentence saying there is none.
    expect(w.text().toLowerCase().replace("not a quality score", "")).not.toContain("score");
    expect(w.text()).toContain("These checks inspect the edit, not the meaning of your tutorial.");
  });

  it("a row's button reveals its object and closes the dialog", async () => {
    findings = [finding({ code: "gap", severity: "info", target: { kind: "clip", id: "c2" }, action: "select" })];
    await openSession();
    await useEditorChecksStore().refresh();
    checksDialogOpen.value = true;
    const w = mount(ChecksDialog, { props: { open: true } });
    await flushPromises();
    await w.get('[data-testid="check-action-chk-gap-c2"]').trigger("click");
    expect(useEditorWorkspaceStore().selectionClipIds).toEqual(["c2"]);
    expect(w.emitted("close")).toHaveLength(1);
  });

  it("choosing a vault sends setDestination, and a finding with no action has no button", async () => {
    findings = [
      finding({ code: "noDestination", action: "setDestination" }),
      finding({ code: "emptyProject", severity: "blocking" }),
    ];
    await openSession();
    await useEditorChecksStore().refresh();
    const w = mount(ChecksDialog, { props: { open: true } });
    await flushPromises();
    expect(w.find('[data-testid="check-action-chk-emptyProject-project"]').exists()).toBe(false);
    await w.get('[data-testid="check-action-chk-noDestination-project"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="checks-destination-vault"]').setValue("vault-1");
    await w.get('[data-testid="checks-destination-folder"]').setValue("Tutorials");
    await w.get('[data-testid="checks-destination-save"]').trigger("click");
    await flushPromises();
    expect(executed).toEqual([{ kind: "setDestination", vaultId: "vault-1", folder: "Tutorials", dated: false }]);
    expect(w.emitted("close")).toBeUndefined();
  });

  it("a retried destination clears the last refusal while it is in flight", async () => {
    // Fix round 1 (review Minor 3): the stale "could not be set" line used
    // to stay on screen through the retry.
    findings = [finding({ code: "noDestination", action: "setDestination" })];
    const store = await openSession();
    let release: (() => void) | null = null;
    let calls = 0;
    store.setPort({
      ...store.port,
      execute: (req) => {
        calls += 1;
        if (calls === 1) {
          return Promise.reject(
            new EditorPortError({ code: "invalidRequest", message: "That vault is gone.", retryable: false, operationId: "op" }),
          );
        }
        return new Promise((resolve) => {
          release = () =>
            resolve({ snapshot: { ...openResult().snapshot, revision: req.expectedRevision + 1 }, project: project() });
        });
      },
    });
    await useEditorChecksStore().refresh();
    const w = mount(ChecksDialog, { props: { open: true } });
    await flushPromises();
    await w.get('[data-testid="check-action-chk-noDestination-project"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="checks-destination-vault"]').setValue("vault-1");
    await w.get('[data-testid="checks-destination-save"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="checks-destination-error"]').text()).toBe("That vault is gone.");
    await w.get('[data-testid="checks-destination-save"]').trigger("click");
    await flushPromises();
    expect(w.find('[data-testid="checks-destination-error"]').exists()).toBe(false);
    (release as unknown as () => void)();
    await flushPromises();
  });

  it("says so when the checks could not be read, rather than reading as a pass", async () => {
    const store = useEditorProjectStore();
    store.setPort(
      fakeEditorPort({
        openStaged: () => Promise.resolve(openResult()),
        getChecks: () =>
          Promise.reject(
            new EditorPortError({ code: "internal", message: "sources.json is not valid", retryable: false, operationId: "op" }),
          ),
      }),
    );
    await store.openStaged("base");
    await useEditorChecksStore().refresh();
    const w = mount(ChecksDialog, { props: { open: true } });
    await flushPromises();
    expect(w.get('[data-testid="checks-error"]').text()).toBe("Checks could not be read. sources.json is not valid");
    expect(w.find('[data-testid="checks-empty"]').exists()).toBe(false);
  });
});

describe("the header's Checks badge and the canvas toast", () => {
  it("counts blockers and warnings, not notes, and opens the dialog", async () => {
    findings = [
      finding({ code: "missingMedia", severity: "blocking", target: { kind: "asset", id: "cap" }, action: "reconnect" }),
      finding({ code: "clipping", severity: "warning", target: { kind: "clip", id: "s1" }, action: "openAudio" }),
      finding({ code: "gap", severity: "info", target: { kind: "clip", id: "c2" }, action: "select" }),
    ];
    await openSession();
    const w = mount(EditorHeader, { props: { isCompact: false, libraryOpen: false, inspectorOpen: false, theme: "dark" } });
    await flushPromises();
    expect(w.get('[data-testid="editor-header-checks-count"]').text()).toBe("2");
    await w.get('[data-testid="editor-header-checks"]').trigger("click");
    await flushPromises();
    expect(checksDialogOpen.value).toBe(true);
    expect(w.find('[data-testid="checks-dialog"]').exists()).toBe(true);
  });

  it("the ratio toast offers to open Checks", async () => {
    await openSession();
    const toolbar = mount(PreviewToolbar, { props: { overflowCount: 0 } });
    const host = mount(NotificationHost);
    await toolbar.get('[data-testid="preview-toolbar-ratio"]').setValue("720x1280");
    await flushPromises();
    const notifications = useNotificationsStore();
    expect(notifications.items).toHaveLength(1);
    expect(notifications.items[0].action?.label).toBe("Open Checks");
    await host.get('[data-testid="notification-action"]').trigger("click");
    expect(checksDialogOpen.value).toBe(true);
  });
});

describe("warnings do not block render, blocking findings do", () => {
  it("warnings leave Render enabled and are summarized", async () => {
    findings = [
      finding({ code: "clipping", severity: "warning", target: { kind: "clip", id: "c1" }, action: "openAudio", message: "May clip." }),
    ];
    await openWithRenders({ getProducts: () => Promise.resolve([]), getChecks: () => Promise.resolve(findings) });
    const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
    await flushPromises();
    expect(w.get('[data-testid="render-dialog-checks-summary"]').text()).toBe("0 blockers · 1 review warning");
    expect(w.get('[data-testid="render-dialog-start"]').attributes("disabled")).toBeUndefined();
  });

  it("a blocking finding disables Render, says why, and lists what blocks", async () => {
    findings = [
      finding({ code: "missingMedia", severity: "blocking", target: { kind: "asset", id: "cap" }, action: "reconnect", message: '"capture.mp4" is missing.' }),
      finding({ code: "clipping", severity: "warning", target: { kind: "clip", id: "c1" }, action: "openAudio" }),
    ];
    const env = await openWithRenders({ getProducts: () => Promise.resolve([]), getChecks: () => Promise.resolve(findings) });
    const w = mount(RenderDialog, { props: { open: true, initialRange: null } });
    await flushPromises();
    expect(w.get('[data-testid="render-dialog-start"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="render-dialog-start-reason"]').text()).toBe("Fix the blocking check first.");
    expect(w.get('[data-testid="render-dialog-checks"]').text()).toContain('"capture.mp4" is missing.');
    await w.get('[data-testid="render-dialog-start"]').trigger("click");
    await flushPromises();
    expect(env.requests).toEqual([]);
  });
});

describe("the timeline scrolls a revealed object into view", () => {
  it("revealScrollLeft leaves an instant already in view alone and brings another a third of the way in", () => {
    // Zoom 1 is 0.05 px/ms; the 196 px label column scrolls with the lanes.
    expect(revealScrollLeft(2_000, 1, 0, 400)).toBeNull(); // x 296, inside [196, 400]
    expect(revealScrollLeft(6_000, 1, 0, 400)).toBe(232); // x 496 -> 300 - 204/3
    expect(revealScrollLeft(1_000, 1, 500, 400)).toBe(0); // behind the view, never negative
  });

  it("Show it on a clip off to the right scrolls the timeline to it", async () => {
    await openSession();
    const workspace = useEditorWorkspaceStore();
    mount(TimelineView, { props: { viewportWidth: 400 } });
    await flushPromises();
    revealFinding(finding({ code: "gap", severity: "info", target: { kind: "clip", id: "c2" }, action: "select" }));
    await flushPromises();
    expect(workspace.timelineScrollLeft).toBe(232);
  });
});

describe("Continue to render", () => {
  const HEADER = { isCompact: false, libraryOpen: false, inspectorOpen: false, theme: "dark" as const };

  it("opens the Render dialog when nothing blocks", async () => {
    findings = [finding({ code: "clipping", severity: "warning", target: { kind: "clip", id: "s1" }, action: "openAudio" })];
    await openSession();
    useEditorProjectStore().port.getProducts = () => Promise.resolve([]);
    const w = mount(EditorHeader, { props: HEADER });
    await w.get('[data-testid="editor-header-checks"]').trigger("click");
    await flushPromises();
    await w.get('[data-testid="checks-render"]').trigger("click");
    await flushPromises();
    expect(checksDialogOpen.value).toBe(false);
    expect(w.find('[data-testid="render-dialog"]').exists()).toBe(true);
  });

  it("is disabled, with the reason beside it, while anything blocks", async () => {
    findings = [finding({ code: "missingMedia", severity: "blocking", target: { kind: "asset", id: "cap" }, action: "reconnect" })];
    await openSession();
    const w = mount(EditorHeader, { props: HEADER });
    await w.get('[data-testid="editor-header-checks"]').trigger("click");
    await flushPromises();
    expect(w.get('[data-testid="checks-render"]').attributes("disabled")).toBeDefined();
    expect(w.get('[data-testid="checks-render-reason"]').text()).toBe("Fix the blocking check first.");
  });
});
