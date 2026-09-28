import { expect, type Page } from "@playwright/test";

import { PARITY_OPEN_RESULT } from "./fixtures/parityProject";
import { settle, type SweepScenario } from "./noopSweep";
import { openParity, type OpenParityOptions } from "./parity";

/**
 * The no-op sweep's scenarios (visual-parity Task 24): every state of the
 * editor the gate sweeps, each reached through its real path over the
 * parity sample project. `editorNoop.spec.ts` runs each as its own test;
 * AGENTS.md's Testing conventions name what this list covers — keep the
 * two in step.
 */

export const WIDE = { width: 1600, height: 1000 };
export const COMPACT = { width: 960, height: 640 };

/** The guide's progress with the invitation already dismissed, so it sits
 * on top of nothing unless a scenario asks for it. */
const QUIET_GUIDE = {
  contentRevision: 1,
  currentStepId: null,
  reviewed: [],
  explored: [],
  invitationDismissed: true,
  active: false,
  collapsed: false,
  completed: false,
  preferences: { dimming: true, motion: "system" },
};

/** One Checks finding, so the Checks dialog has a row and its buttons. */
const FINDING = {
  id: "chk-gap-c3",
  severity: "warning",
  code: "gap",
  message: "A gap before Save to your vault.",
  target: { kind: "clip", id: "c3" },
  action: "select",
};

/** The destination finding, whose action opens `ChecksDestination`. */
const NO_DESTINATION = {
  id: "chk-noDestination-project",
  severity: "warning",
  code: "noDestination",
  message: "Choose the vault this tutorial is published to.",
  target: { kind: "project", id: null },
  action: "setDestination",
};

/** No camera, ever: the one `getUserMedia` the editor makes is refused
 * before it can reach the browser, and no device is listed. */
async function blockCamera(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const md = navigator.mediaDevices as MediaDevices | undefined;
    if (!md) return;
    md.getUserMedia = () => Promise.reject(new DOMException("The camera is blocked in this test.", "NotAllowedError"));
    md.enumerateDevices = () => Promise.resolve([]);
  });
}

export const INSPECTOR = '[data-testid="editor-shell-inspector"]';
const LIBRARY = '[data-testid="editor-shell-library"]';
const DIALOG = '[role="dialog"]';
export const scopeOf = (testid: string) => `[data-testid="${testid}"]`;
const CONTEXT = scopeOf("editor-context-menu");
const SUBMENU = scopeOf("editor-context-menu-submenu");

export type Step = (page: Page) => Promise<void>;

export const click =
  (testid: string, opts: { modifiers?: ("Control" | "Shift")[] } = {}): Step =>
  async (page) => {
    await page.getByTestId(testid).first().click(opts);
    await settle(page);
  };

export const rightClick =
  (testid: string, position?: { x: number; y: number }): Step =>
  async (page) => {
    await page.getByTestId(testid).first().click({ button: "right", position });
    await settle(page);
  };

/** A scenario's own proof that it reached the state it names — so a step
 * that silently did nothing cannot leave a scope swept in the wrong state. */
const shows =
  (testid: string): Step =>
  async (page) => {
    await expect(page.getByTestId(testid).first()).toBeVisible();
  };

const onTab = (tab: string): Step[] => [
  click(`inspector-tab-${tab}`),
  async (page) => {
    await expect(page.getByTestId(`inspector-tab-${tab}`)).toHaveAttribute("aria-selected", "true");
  },
];

type StubWindow = Window & Record<string, unknown> & { __calls: { cmd: string; args: unknown }[] };

/** Rust emitting `event` to the editor window: the handler the page
 * registered through Tauri's `listen` (`plugin:event|listen`), called the
 * way the runtime calls it. */
const emitToEditor =
  (event: string): Step =>
  async (page) => {
    await page.evaluate((event) => {
      const w = window as unknown as StubWindow;
      const call = w.__calls.find((c) => c.cmd === "plugin:event|listen" && (c.args as { event?: string }).event === event);
      if (!call) throw new Error(`nothing listens for ${event}`);
      const handler = (call.args as { handler: number }).handler;
      (w[`_${handler}`] as (e: unknown) => void)({ event, id: 0, payload: {} });
    }, event);
    await settle(page);
  };

/** The render Rust started finishing: its terminal on the job's own
 * Channel (the one `editor_start_render` was handed), naming the product. */
const renderCompletes: Step = async (page) => {
  await page.waitForFunction(() => (window as unknown as StubWindow).__calls.some((c) => c.cmd === "editor_start_render"));
  await page.evaluate((sessionId) => {
    const w = window as unknown as StubWindow;
    const call = w.__calls.find((c) => c.cmd === "editor_start_render") as { args: { onProgress: { id: number } } };
    const message = {
      sessionId,
      jobId: "job-noop",
      kind: "render",
      sequence: 1,
      phase: "complete",
      fraction: 1,
      terminal: { productId: "prod1" },
    };
    (w[`_${call.args.onProgress.id}`] as (m: unknown) => void)({ index: 0, message });
  }, PARITY_OPEN_RESULT.snapshot.sessionId);
  await settle(page);
};

/** Ruling T21-3: the editor's dialogs use `DialogButton`. The panel
 * window's `AppButton` and `IconButton` draw with `white/N` literals, so
 * any such class inside an open dialog (the mixer popover included) is one
 * of them (the unit test `editorDialogButtons.test.ts` pins the source
 * side). */
async function panelButtonStyles(page: Page): Promise<string[]> {
  const offenders = await page.evaluate(() =>
    Array.from(document.querySelectorAll('[role="dialog"] [class*="white/"]')).map(
      (el) => el.getAttribute("data-testid") ?? el.tagName.toLowerCase(),
    ),
  );
  return offenders.map((o) => `renders the panel's white/N button style inside a dialog (${o})`);
}

/** Opens every closed `<details>` in `scope`, so the controls behind a
 * disclosure are swept too (its summary is swept as well: clicking it
 * closes the disclosure again). Setting `open` fires the same `toggle`
 * event a click on the summary does, in one round trip — the learning
 * center's answers alone hold dozens. */
async function openDisclosures(page: Page, scope: string): Promise<void> {
  const opened = await page.evaluate((scope) => {
    const closed = Array.from(document.querySelectorAll<HTMLDetailsElement>(`:is(${scope}) details:not([open])`));
    for (const d of closed) d.open = true;
    return closed.length;
  }, scope);
  if (opened > 0) await settle(page);
}

export interface Scenario {
  name: string;
  size?: { width: number; height: number };
  scope?: string;
  steps?: Step[];
  /** Leaves the guide invitation showing (a fresh vault). */
  invitation?: boolean;
  options?: OpenParityOptions;
}

/** Opens the scenario once, then reloads the page (the stub and routes
 * survive a reload) and replays its steps each time it is re-prepared. */
export function scenario(page: Page, sc: Scenario): SweepScenario {
  let opened = false;
  return {
    name: sc.name,
    scope: sc.scope,
    audit: () => panelButtonStyles(page),
    prepare: async () => {
      if (!opened) {
        await blockCamera(page);
        await openParity(page, sc.size ?? WIDE, {
          ...sc.options,
          replies: {
            editor_get_guide_progress: sc.invitation ? { ...QUIET_GUIDE, invitationDismissed: false } : QUIET_GUIDE,
            editor_get_checks: [FINDING],
            ...sc.options?.replies,
          },
        });
        opened = true;
      } else {
        await page.reload();
        await page.getByTestId("editor-shell").waitFor();
      }
      await page.getByTestId("clip-c1").waitFor();
      await settle(page);
      for (const step of sc.steps ?? []) await step(page);
      await openDisclosures(page, sc.scope ?? "body");
    },
  };
}

// ---- the post-edit and extra-content fixtures ----------------------------

/** An Undo that landed: the projection Rust answers the first
 * `editor_execute` with — a new revision, Redo now offered. */
const UNDONE = {
  snapshot: { ...PARITY_OPEN_RESULT.snapshot, revision: 4, canUndo: false, canRedo: true, undoLabel: null, redoLabel: "Split clip" },
  project: PARITY_OPEN_RESULT.project,
};

/** A title card on the detail track, before the detail clip. */
const CARD_PROJECT = {
  assets: [
    ...PARITY_OPEN_RESULT.project.assets,
    { id: "card-intro", kind: "video" as const, name: "Intro card", duration_ms: 3_000, builtin: "card" as const },
  ],
  clips: [
    ...PARITY_OPEN_RESULT.project.clips,
    {
      ...PARITY_OPEN_RESULT.project.clips[0],
      id: "cc1",
      asset_id: "card-intro",
      track_id: "v2",
      name: "Welcome",
      start_ms: 1_000,
      in_ms: 0,
      out_ms: 3_000,
      card: {
        preset: "intro" as const,
        title: "Welcome",
        subtitle: "Your first project",
        background: "#111827",
        foreground: "#ffffff",
        accent: "#7c3aed",
      },
    },
  ],
};

/** The detail capture's original, missing on this PC. */
const MISSING_DETAIL = [{ assetId: "detail", name: "Project detail.capture", expectedSize: 1_000_000, expectedDurationMs: 18_000 }];

/** A clean session whose project keeps unsaved changes for recovery. */
const RECOVERABLE: OpenParityOptions = {
  snapshot: { revision: 1, persistedRevision: 1, canUndo: false, undoLabel: null },
  replies: {
    editor_list_projects: [
      {
        projectFileId: PARITY_OPEN_RESULT.snapshot.projectId,
        title: PARITY_OPEN_RESULT.snapshot.title,
        updatedAt: "2026-09-25T10:05:00.000Z",
        persistedRevision: 1,
        hasRecovery: true,
        sourceBase: PARITY_OPEN_RESULT.sourceBase,
      },
    ],
  },
};

// ---- the scenarios ----------------------------------------------------------

const VIDEO_TABS = ["clip", "layout", "fades", "audio", "speed", "color"];
const AUDIO_TABS = ["clip", "fades", "audio", "speed"];
const selectTwo: Step[] = [click("clip-c1"), click("clip-c2", { modifiers: ["Shift"] })];

const WORKSPACE: Scenario[] = [
  { name: "base" },
  {
    name: "clip selected",
    steps: [click("clip-c2")],
    scope: '[data-testid="editor-timeline"],[data-testid="editor-shell-inspector"],[data-testid="editor-shell-preview"]',
  },
  {
    name: "after an edit (Undo landed)",
    steps: [
      click("timeline-toolbar-undo"),
      async (page) => {
        await expect(page.getByTestId("timeline-toolbar-redo")).not.toHaveAttribute("aria-disabled", "true");
      },
    ],
    scope: '[data-testid="editor-header"],[data-testid="timeline-toolbar"]',
    options: { sequences: { editor_execute: [UNDONE] } },
  },
  { name: "compact 960x640", size: COMPACT },
  { name: "compact: properties drawer", size: COMPACT, steps: [click("clip-c2"), click("preview-properties-toggle")], scope: INSPECTOR },
];

const INSPECTORS: Scenario[] = [
  ...VIDEO_TABS.map((t) => ({ name: `video clip: ${t} tab`, steps: [click("clip-c2"), ...onTab(t)], scope: INSPECTOR })),
  ...AUDIO_TABS.map((t) => ({ name: `audio clip: ${t} tab`, steps: [click("clip-c6"), ...onTab(t)], scope: INSPECTOR })),
  ...VIDEO_TABS.map((t) => ({ name: `webcam clip: ${t} tab`, steps: [click("clip-c5"), ...onTab(t)], scope: INSPECTOR })),
  ...["layout", "color"].map((t) => ({
    name: `two clips selected: ${t} tab`,
    steps: [...selectTwo, shows("multi-inspector-list"), ...onTab(t)],
    scope: INSPECTOR,
  })),
  { name: "track selected", steps: [click("track-header-v2-badge"), shows("track-inspector-name")], scope: INSPECTOR },
  { name: "teaching cue selected", steps: [click("timeline-cue-fx5"), shows("effect-section")], scope: INSPECTOR },
];

const LIBRARIES: Scenario[] = [
  ...["media", "titles", "captions", "chapters"].map((t) => ({ name: `library: ${t}`, steps: [click(`library-tab-${t}`)], scope: LIBRARY })),
  { name: "library: captions of the selected clip", steps: [click("clip-c5"), click("library-tab-captions")], scope: LIBRARY },
  { name: "library: project section", steps: [click("editor-statusbar-products")], scope: LIBRARY },
  {
    name: "library: a product being watched",
    steps: [click("editor-statusbar-products"), click("product-watch-prod1"), shows("product-card-prod1")],
    scope: scopeOf("product-card-prod1"),
  },
  {
    name: "library: a product's restore confirm",
    steps: [click("editor-statusbar-products"), click("product-restore-prod1"), shows("product-restore-question-prod1")],
    scope: scopeOf("product-card-prod1"),
  },
];

const MENUS: Scenario[] = [
  { name: "Project menu", steps: [click("editor-header-project-menu")], scope: scopeOf("editor-project-menu") },
  { name: "View menu", steps: [click("preview-view-menu")], scope: scopeOf("preview-view-panel") },
  { name: "More tools menu", steps: [click("preview-more-tools")], scope: scopeOf("preview-more-tools-panel") },
  {
    name: "compact: More tools menu",
    size: COMPACT,
    steps: [click("preview-more-tools")],
    scope: scopeOf("preview-more-tools-panel"),
  },
  { name: "Add track menu", steps: [click("timeline-add-track")], scope: scopeOf("timeline-add-track-panel") },
  { name: "Help menu", steps: [click("editor-header-help")], scope: scopeOf("editor-help-menu") },
  { name: "Edit actions menu", steps: [click("clip-c2"), click("timeline-toolbar-more")], scope: CONTEXT },
  { name: "clip menu", steps: [rightClick("clip-c2")], scope: CONTEXT },
  { name: "clip menu: an audio clip", steps: [rightClick("clip-c6")], scope: CONTEXT },
  { name: "clip menu: a webcam clip", steps: [rightClick("clip-c5")], scope: CONTEXT },
  {
    name: "clip menu: a title card",
    steps: [rightClick("clip-cc1")],
    scope: CONTEXT,
    options: { project: CARD_PROJECT },
  },
  { name: "multi-clip menu", steps: [...selectTwo, rightClick("clip-c2")], scope: CONTEXT },
  { name: "track menu", steps: [rightClick("track-lane-header-v2", { x: 150, y: 20 })], scope: CONTEXT },
  { name: "lane menu", steps: [rightClick("track-lane-body-v2", { x: 20, y: 20 })], scope: CONTEXT },
  { name: "cue menu", steps: [rightClick("timeline-cue-fx1")], scope: CONTEXT },
  { name: "asset menu", steps: [rightClick("library-asset-webcam")], scope: CONTEXT },
  ...["fades", "color", "trim", "speed", "transform", "audio"].map((id) => ({
    name: `clip menu: ${id} submenu`,
    steps: [rightClick("clip-c2"), click(`editor-context-menu-item-${id}`)],
    scope: SUBMENU,
  })),
  ...["fades", "color"].map((id) => ({
    name: `multi-clip menu: ${id} submenu`,
    steps: [...selectTwo, rightClick("clip-c2"), click(`editor-context-menu-item-${id}`)],
    scope: SUBMENU,
  })),
];

const openProjectMenuItem = (id: string): Step[] => [click("editor-header-project-menu"), click(`editor-project-menu-item-${id}`)];

const DIALOGS: Scenario[] = [
  { name: "Checks dialog", steps: [click("editor-header-checks")], scope: DIALOG },
  {
    name: "Checks dialog: choosing a destination",
    steps: [click("editor-header-checks"), click(`check-action-${NO_DESTINATION.id}`), shows("checks-destination")],
    scope: DIALOG,
    options: { replies: { editor_get_checks: [FINDING, NO_DESTINATION] } },
  },
  { name: "Save a copy dialog", steps: openProjectMenuItem("saveCopy"), scope: DIALOG },
  { name: "Render dialog", steps: [click("editor-header-render")], scope: DIALOG },
  {
    name: "Render dialog: a finished render",
    steps: [click("editor-header-render"), click("render-dialog-start"), renderCompletes, shows("render-dialog-publish")],
    scope: DIALOG,
    options: { replies: { editor_start_render: { jobId: "job-noop", revision: 3 } } },
  },
  { name: "Review dialog", steps: [click("preview-review")], scope: DIALOG },
  { name: "Publish dialog", steps: [click("editor-statusbar-products"), click("product-publish-prod1")], scope: DIALOG },
  { name: "Webcam dialog", steps: [click("library-webcam")], scope: DIALOG },
  { name: "Mixer", steps: [click("mixer-toggle")], scope: scopeOf("mixer-popover") },
  ...["walkthrough", "answers", "shortcuts"].map((tab) => ({
    name: `Learning center: ${tab}`,
    steps: [click("editor-header-help"), click("editor-help-menu-item-learningCenter"), click(`learning-tab-${tab}`)],
    scope: DIALOG,
  })),
  { name: "Frame dialog", steps: [click("preview-ratio")], scope: DIALOG },
  { name: "Rename dialog", steps: openProjectMenuItem("rename"), scope: DIALOG },
  { name: "Open project dialog", steps: openProjectMenuItem("open"), scope: DIALOG },
  { name: "Discard dialog", steps: openProjectMenuItem("discard"), scope: DIALOG },
  {
    name: "Remove track dialog",
    steps: [rightClick("track-lane-header-v2", { x: 150, y: 20 }), click("editor-context-menu-item-track-remove")],
    scope: DIALOG,
  },
  {
    name: "Reconnect dialog",
    steps: [click("library-reconnect"), shows("reconnect-dialog")],
    scope: DIALOG,
    options: { missing: MISSING_DETAIL },
  },
  { name: "Recovery dialog", steps: [shows("recovery-dialog")], scope: DIALOG, options: RECOVERABLE },
  { name: "Close guard dialog", steps: [emitToEditor("editor:closeRequested"), shows("close-guard")], scope: DIALOG },
];

const GUIDE: Scenario[] = [
  { name: "guide invitation", invitation: true, scope: scopeOf("guide-invitation") },
  { name: "guide coach", invitation: true, steps: [click("guide-invitation-start")], scope: scopeOf("guide-coach") },
];

export const SCENARIOS: Scenario[] = [...WORKSPACE, ...INSPECTORS, ...LIBRARIES, ...MENUS, ...DIALOGS, ...GUIDE];
