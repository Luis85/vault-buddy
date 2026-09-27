import { expect, test } from "@playwright/test";

import { click, rightClick, scenario, SCENARIOS, scopeOf, type Step } from "./noopScenarios";
import { describe as describeFindings, sweep } from "./noopSweep";

/**
 * The no-op sweep gate (visual-parity Task 24; design D14 and "Gates added
 * by this work"): in every scenario `noopScenarios.ts` lists — the
 * workspace, the inspector tabs and modes, the library tabs, and the menus,
 * submenus and dialogs it names — every visible enabled control must do something
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
 * The scenarios themselves live in `noopScenarios.ts`. The webcam dialog
 * never reaches a real camera: `getUserMedia` is replaced before the bundle
 * loads, so no scenario can raise a camera prompt. CI runs this file in its
 * own job (`editor-noop-sweep`, Ruling T24-2); the `frontend` job's e2e
 * step skips it through `E2E_SKIP_NOOP_SWEEP`.
 */

test.describe.configure({ mode: "parallel" });

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

  // The gate must be able to fail: a control that does nothing, one that is
  // disabled without a reason, and a roving-tabindex pair whose only
  // response is its focus handler rewriting `tabindex` (a toolstrip's —
  // focus is not an effect of the click), planted in the workspace, are
  // all named.
  test("the sweep names a planted no-op and a planted reasonless disabled control", async ({ page }) => {
    const plant: Step = async (p) => {
      await p.evaluate(() => {
        const host = document.createElement("div");
        host.dataset.testid = "noop-plant";
        host.innerHTML =
          '<button type="button" data-testid="noop-plant-inert">Inert</button>' +
          '<button type="button" data-testid="noop-plant-disabled" disabled>Mute</button>' +
          '<button type="button" data-testid="noop-plant-explained" disabled title="Nothing to mute.">Mute</button>' +
          '<button type="button" data-testid="noop-plant-rove-a" tabindex="0">Rove A</button>' +
          '<button type="button" data-testid="noop-plant-rove-b" tabindex="-1">Rove B</button>';
        const rovers = Array.from(host.querySelectorAll<HTMLButtonElement>('[data-testid^="noop-plant-rove"]'));
        for (const r of rovers) {
          r.addEventListener("focus", () => rovers.forEach((o) => (o.tabIndex = o === r ? 0 : -1)));
        }
        document.body.appendChild(host);
      });
    };
    const result = await sweep(page, scenario(page, { name: "planted", steps: [plant], scope: scopeOf("noop-plant") }));
    const noop = "did nothing when activated (no IPC call, DOM change, focus move or menu/dialog change)";
    expect(result.findings.map((f) => `${f.testid}: ${f.problem}`)).toEqual([
      `noop-plant-inert: ${noop}`,
      "noop-plant-disabled: disabled without a reason (title, aria-describedby or aria-description)",
      `noop-plant-rove-a: ${noop}`,
      `noop-plant-rove-b: ${noop}`,
    ]);
  });

  // Every scenario proves it is idle: the sweep plants one inert control in
  // its scope, which must be reported as a no-op. A page that answers ANY
  // click (here, a document listener that grows the DOM) would credit every
  // control with an effect — so that scenario must fail, not pass.
  test("the sweep refuses a scenario that is not idle", async ({ page }) => {
    const answerEveryClick: Step = async (p) => {
      await p.evaluate(() => {
        const host = document.createElement("div");
        host.dataset.testid = "noop-plant";
        host.innerHTML = '<button type="button" data-testid="noop-plant-inert">Inert</button>';
        document.body.appendChild(host);
        document.addEventListener("click", () => document.body.appendChild(document.createElement("i")));
      });
    };
    const result = await sweep(page, scenario(page, { name: "busy", steps: [answerEveryClick], scope: scopeOf("noop-plant") }));
    expect(result.findings.map((f) => `${f.testid}: ${f.problem}`)).toEqual([
      "noop-idle-proof: the scenario is not idle: an inert control planted in it was credited with an effect",
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
