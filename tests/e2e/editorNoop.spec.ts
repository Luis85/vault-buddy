import { expect, type Page, test } from "@playwright/test";

import { describe as describeFindings, settle, sweep, type SweepScenario } from "./noopSweep";
import { openParity, type OpenParityOptions } from "./parity";

/**
 * The no-op sweep gate (visual-parity Task 24; design D14 and "Gates added
 * by this work"): in every scenario below — the workspace, each inspector
 * tab and mode, each library tab, every menu and submenu, every dialog, and
 * the compact window — every visible enabled control must do something
 * observable, and every visible disabled control must say why
 * (`noopSweep.ts`); and no open dialog draws the panel window's buttons
 * (Ruling T21-3). A second block proves a refused edit is always said, in
 * role wording. Each scenario is its own test, run in parallel.
 *
 * Adapted from the no-op audit's throwaway harness
 * (docs/superpowers/specs/2026-09-26-tutorial-editor-noop-audit.md). It
 * opens the parity sample project; `editor_execute` has no stubbed reply, so
 * every edit comes back as a refusal the editor has to show — which is the
 * visible effect a real refusal must have too.
 *
 * The webcam dialog never reaches a real camera: `getUserMedia` is replaced
 * before the bundle loads, so no scenario can raise a camera prompt.
 */

test.describe.configure({ mode: "parallel" });

const WIDE = { width: 1600, height: 1000 };
const COMPACT = { width: 960, height: 640 };

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

/** Rust's refusal as the port receives it, carrying a redaction handle the
 * webview must strip before a person reads it. */
const REFUSAL = {
  code: "invalidRequest",
  message: "That clip would overlap <name:#1a2b3c4d> on this track.",
  retryable: false,
  operationId: "op-noop",
};

/** Every refused command answers with `REFUSAL` (the brief's `refuse`
 * option, built on the stub's existing `rejects`). */
function refuse(commands: string[]): Record<string, unknown> {
  return Object.fromEntries(commands.map((c) => [c, REFUSAL]));
}

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

const INSPECTOR = '[data-testid="editor-shell-inspector"]';
const LIBRARY = '[data-testid="editor-shell-library"]';
const DIALOG = '[role="dialog"]';
const menu = (testid: string) => `[data-testid="${testid}"]`;
const CONTEXT = menu("editor-context-menu");
const SUBMENU = menu("editor-context-menu-submenu");

type Step = (page: Page) => Promise<void>;

const click =
  (testid: string, opts: { modifiers?: ("Control" | "Shift")[] } = {}): Step =>
  async (page) => {
    await page.getByTestId(testid).first().click(opts);
    await settle(page);
  };

const rightClick =
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

const onTab =
  (tab: string): Step[] => [
    click(`inspector-tab-${tab}`),
    async (page) => {
      await expect(page.getByTestId(`inspector-tab-${tab}`)).toHaveAttribute("aria-selected", "true");
    },
  ];

/** Ruling T21-3: the editor's dialogs use `DialogButton`. The panel
 * window's `AppButton` draws its secondary and ghost looks with `white/N`
 * literals, so any such class inside an open dialog (the mixer popover
 * included) is one of them (the
 * unit test `editorDialogButtons.test.ts` pins the source side). */
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

interface Scenario {
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
function scenario(page: Page, sc: Scenario): SweepScenario {
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

const VIDEO_TABS = ["clip", "layout", "fades", "audio", "speed", "color"];
const AUDIO_TABS = ["clip", "fades", "audio", "speed"];

const SCENARIOS: Scenario[] = [
  { name: "base" },
  {
    name: "clip selected",
    steps: [click("clip-c2")],
    scope: '[data-testid="editor-timeline"],[data-testid="editor-shell-inspector"],[data-testid="editor-shell-preview"]',
  },
  ...VIDEO_TABS.map((t) => ({ name: `video clip: ${t} tab`, steps: [click("clip-c2"), ...onTab(t)], scope: INSPECTOR })),
  ...AUDIO_TABS.map((t) => ({ name: `audio clip: ${t} tab`, steps: [click("clip-c6"), ...onTab(t)], scope: INSPECTOR })),
  ...VIDEO_TABS.map((t) => ({ name: `webcam clip: ${t} tab`, steps: [click("clip-c5"), ...onTab(t)], scope: INSPECTOR })),
  ...["layout", "color"].map((t) => ({
    name: `two clips selected: ${t} tab`,
    steps: [click("clip-c1"), click("clip-c2", { modifiers: ["Shift"] }), shows("multi-inspector-list"), ...onTab(t)],
    scope: INSPECTOR,
  })),
  { name: "track selected", steps: [click("track-header-v2-badge"), shows("track-inspector-name")], scope: INSPECTOR },
  { name: "teaching cue selected", steps: [click("timeline-cue-fx5"), shows("effect-section")], scope: INSPECTOR },
  ...["media", "titles", "captions", "chapters"].map((t) => ({ name: `library: ${t}`, steps: [click(`library-tab-${t}`)], scope: LIBRARY })),
  {
    name: "library: captions of the selected clip",
    steps: [click("clip-c5"), click("library-tab-captions")],
    scope: LIBRARY,
  },
  { name: "library: project section", steps: [click("editor-statusbar-products")], scope: LIBRARY },
  { name: "Project menu", steps: [click("editor-header-project-menu")], scope: menu("editor-project-menu") },
  { name: "View menu", steps: [click("preview-view-menu")], scope: menu("preview-view-panel") },
  { name: "More tools menu", steps: [click("preview-more-tools")], scope: menu("preview-more-tools-panel") },
  {
    name: "compact: More tools menu",
    size: COMPACT,
    steps: [click("preview-more-tools")],
    scope: menu("preview-more-tools-panel"),
  },
  { name: "Add track menu", steps: [click("timeline-add-track")], scope: menu("timeline-add-track-panel") },
  { name: "Help menu", steps: [click("editor-header-help")], scope: menu("editor-help-menu") },
  { name: "Edit actions menu", steps: [click("clip-c2"), click("timeline-toolbar-more")], scope: CONTEXT },
  { name: "clip menu", steps: [rightClick("clip-c2")], scope: CONTEXT },
  { name: "multi-clip menu", steps: [click("clip-c1"), click("clip-c2", { modifiers: ["Shift"] }), rightClick("clip-c2")], scope: CONTEXT },
  { name: "track menu", steps: [rightClick("track-lane-header-v2", { x: 150, y: 20 })], scope: CONTEXT },
  { name: "lane menu", steps: [rightClick("track-lane-body-v2", { x: 20, y: 20 })], scope: CONTEXT },
  { name: "cue menu", steps: [rightClick("timeline-cue-fx1")], scope: CONTEXT },
  { name: "asset menu", steps: [rightClick("library-asset-webcam")], scope: CONTEXT },
  ...["fades", "color", "trim", "speed", "transform", "audio"].map((id) => ({
    name: `clip menu: ${id} submenu`,
    steps: [rightClick("clip-c2"), click(`editor-context-menu-item-${id}`)],
    scope: SUBMENU,
  })),
  { name: "Checks dialog", steps: [click("editor-header-checks")], scope: DIALOG },
  {
    name: "Save a copy dialog",
    steps: [click("editor-header-project-menu"), click("editor-project-menu-item-saveCopy")],
    scope: DIALOG,
  },
  { name: "Render dialog", steps: [click("editor-header-render")], scope: DIALOG },
  { name: "Review dialog", steps: [click("preview-review")], scope: DIALOG },
  { name: "Publish dialog", steps: [click("editor-statusbar-products"), click("product-publish-prod1")], scope: DIALOG },
  { name: "Webcam dialog", steps: [click("library-webcam")], scope: DIALOG },
  { name: "Mixer", steps: [click("mixer-toggle")], scope: menu("mixer-popover") },
  ...["walkthrough", "answers", "shortcuts"].map((tab) => ({
    name: `Learning center: ${tab}`,
    steps: [click("editor-header-help"), click("editor-help-menu-item-learningCenter"), click(`learning-tab-${tab}`)],
    scope: DIALOG,
  })),
  { name: "Frame dialog", steps: [click("preview-ratio")], scope: DIALOG },
  { name: "Rename dialog", steps: [click("editor-header-project-menu"), click("editor-project-menu-item-rename")], scope: DIALOG },
  {
    name: "Open project dialog",
    steps: [click("editor-header-project-menu"), click("editor-project-menu-item-open")],
    scope: DIALOG,
  },
  {
    name: "Discard dialog",
    steps: [click("editor-header-project-menu"), click("editor-project-menu-item-discard")],
    scope: DIALOG,
  },
  {
    name: "Remove track dialog",
    steps: [rightClick("track-lane-header-v2", { x: 150, y: 20 }), click("editor-context-menu-item-track-remove")],
    scope: DIALOG,
  },
  { name: "guide invitation", invitation: true, scope: menu("guide-invitation") },
  { name: "guide coach", invitation: true, steps: [click("guide-invitation-start")], scope: menu("guide-coach") },
  { name: "compact 960x640", size: COMPACT },
  { name: "compact: properties drawer", size: COMPACT, steps: [click("clip-c2"), click("preview-properties-toggle")], scope: INSPECTOR },
];

test.describe("no-op sweep: every control does something, every disabled one says why", () => {
  for (const sc of SCENARIOS) {
    test(sc.name, async ({ page }) => {
      test.setTimeout(600_000);
      const result = await sweep(page, scenario(page, sc));
      test.info().annotations.push({
        type: "swept",
        description: `${result.activated} activated, ${result.disabled} disabled checked, ${result.skipped} already active`,
      });
      expect(describeFindings(result.findings)).toBe("");
    });
  }

  // The gate must be able to fail: a control that does nothing, and one that
  // is disabled without a reason, planted in the workspace, are both named.
  test("the sweep names a planted no-op and a planted reasonless disabled control", async ({ page }) => {
    const plant: Step = async (p) => {
      await p.evaluate(() => {
        const host = document.createElement("div");
        host.dataset.testid = "noop-plant";
        host.innerHTML =
          '<button type="button" data-testid="noop-plant-inert">Inert</button>' +
          '<button type="button" data-testid="noop-plant-disabled" disabled>Mute</button>' +
          '<button type="button" data-testid="noop-plant-explained" disabled title="Nothing to mute.">Mute</button>';
        document.body.appendChild(host);
      });
    };
    const result = await sweep(page, scenario(page, { name: "planted", steps: [plant], scope: menu("noop-plant") }));
    expect(result.findings.map((f) => `${f.testid}: ${f.problem}`)).toEqual([
      "noop-plant-inert: did nothing when activated (no IPC call, DOM change, focus move or menu/dialog change)",
      "noop-plant-disabled: disabled without a reason (title, aria-describedby or aria-description)",
    ]);
  });
});

test.describe("no-op sweep: a refused edit is always said, in role wording", () => {
  const refusals: { name: string; steps: Step[]; act: Step }[] = [
    {
      name: "Titles → Intro",
      steps: [click("library-tab-titles")],
      act: click("titles-add-intro"),
    },
    {
      name: "clip menu → Duplicate",
      steps: [rightClick("clip-c2")],
      act: click("editor-context-menu-item-duplicate"),
    },
    {
      name: "an inspector field commit",
      steps: [click("clip-c2"), click("inspector-tab-clip")],
      act: async (page) => {
        const name = page.getByTestId("clip-section-name");
        await name.fill("A renamed clip");
        await name.press("Enter");
      },
    },
  ];
  for (const r of refusals) {
    test(r.name, async ({ page }) => {
      const sc = scenario(page, { name: r.name, steps: r.steps, options: { rejects: refuse(["editor_execute"]) } });
      await sc.prepare();
      await r.act(page);
      const toast = page.getByTestId("notification").last();
      await expect(toast).toBeVisible();
      const text = (await toast.textContent()) ?? "";
      // The refusal's own words, in role wording: the handle is gone.
      expect(text).toContain("That clip would overlap on this track.");
      expect(text).not.toContain("<path:#");
      expect(text).not.toContain("<name:#");
    });
  }
});
