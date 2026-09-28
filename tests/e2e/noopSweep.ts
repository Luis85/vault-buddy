import type { Page } from "@playwright/test";

import { type Control, installNoopProbe, type NoopProbe } from "./noopProbe";

/**
 * The no-op sweep's runner (visual-parity Task 24; design D14, the "No-op
 * sweep e2e" gate). `editorNoop.spec.ts` names the scenarios; this module
 * finds every control a scenario shows (through the in-page probe,
 * `noopProbe.ts`), activates each one on a freshly prepared page, and
 * reports the ones that did nothing — or that are disabled without saying
 * why.
 *
 * Every wait is a condition: `settle` waits for the DOM to go quiet (and
 * remembers which nodes kept changing, so a peak meter or a ticking clock
 * never reads as the click's effect), and the effect itself is polled for.
 * Only a control that really does nothing waits the whole effect window.
 */

/** How long an activation may take to show its effect before it is a
 * no-op. */
const EFFECT_WINDOW_MS = 1_500;

export interface Finding {
  scenario: string;
  name: string;
  testid: string | null;
  problem: string;
}

export interface SweepScenario {
  name: string;
  /** Selector every swept control must sit inside; the whole page if unset. */
  scope?: string;
  /** Loads the page (or reloads it) and brings it to the scenario's state. */
  prepare: () => Promise<void>;
  /** Extra checks on the prepared scenario, each problem a finding. */
  audit?: () => Promise<string[]>;
}

export interface SweepResult {
  findings: Finding[];
  /** Enabled controls activated. */
  activated: number;
  /** Disabled controls whose reason was checked. */
  disabled: number;
  /** Already-active tabs/radios/presets and one-choice selects. */
  skipped: number;
}

type ProbeWindow = Window & { __noopProbe: NoopProbe; __noisy?: Set<Node> };

/** The inert control every scenario plants in its own scope. */
const IDLE_PROOF = "noop-idle-proof";
const NO_EFFECT = "did nothing when activated (no IPC call, DOM change, focus move or menu/dialog change)";

/** Plants one inert button inside the scenario's scope (its last match —
 * the topmost dialog, the open submenu), fixed at the top-left above
 * everything so it can always be clicked. It must be reported as a no-op:
 * that is the scenario's proof that it is idle — that nothing it leaves
 * running (a debounced call, a listener, an animation) would credit an
 * inert control with an effect. */
async function plantIdleProof(page: Page, scope?: string): Promise<void> {
  await page.evaluate(
    ({ scope, id }) => {
      const hosts = document.querySelectorAll(scope ?? "body");
      const host = hosts[hosts.length - 1] ?? document.body;
      const button = document.createElement("button");
      button.type = "button";
      button.dataset.testid = id;
      button.textContent = "Idle proof";
      button.style.cssText = "position:fixed;left:0;top:0;z-index:2147483647;";
      host.appendChild(button);
    },
    { scope, id: IDLE_PROOF },
  );
  await settle(page);
}

/** Waits until the DOM has been quiet for `quietMs` (bounded by `maxMs`).
 * A page that never goes quiet (a meter, a clock) has its still-changing
 * nodes — those that changed in the last `quietMs * 2` — recorded on
 * `window.__noisy`, the idle baseline the effect check subtracts; a page
 * that did go quiet has none. */
export async function settle(page: Page, quietMs = 150, maxMs = 3_000): Promise<void> {
  await page.evaluate(
    ({ quietMs, maxMs }) =>
      new Promise<void>((resolve) => {
        const last = new Map<Node, number>();
        let quietTimer = setTimeout(() => done(true), quietMs);
        const cap = setTimeout(() => done(false), maxMs);
        const obs = new MutationObserver((records) => {
          for (const r of records) last.set(r.target, performance.now());
          clearTimeout(quietTimer);
          quietTimer = setTimeout(() => done(true), quietMs);
        });
        obs.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
        function done(quiet: boolean) {
          obs.disconnect();
          clearTimeout(quietTimer);
          clearTimeout(cap);
          const since = performance.now() - quietMs * 2;
          const noisy = quiet ? [] : [...last].filter(([, at]) => at >= since).map(([n]) => n);
          (window as unknown as ProbeWindow).__noisy = new Set(noisy);
          resolve();
        }
      }),
    { quietMs, maxMs },
  );
}

/**
 * Sweeps one scenario: every disabled control must carry a reason (Ruling
 * T24-1), and every enabled one must do something when activated. The page
 * is re-prepared before a control whenever the previous one changed it.
 */
export async function sweep(page: Page, scenario: SweepScenario): Promise<SweepResult> {
  await page.addInitScript(installNoopProbe);
  const sc: SweepScenario = {
    ...scenario,
    prepare: async () => {
      await scenario.prepare();
      await plantIdleProof(page, scenario.scope);
    },
  };
  const result: SweepResult = { findings: [], activated: 0, disabled: 0, skipped: 0 };
  await sc.prepare();
  for (const problem of (await sc.audit?.()) ?? []) {
    result.findings.push({ scenario: sc.name, name: "(scenario)", testid: null, problem });
  }
  const controls = await page.evaluate((scope) => (window as unknown as ProbeWindow).__noopProbe.controls(scope), sc.scope);
  if (controls.length === 0) {
    result.findings.push({ scenario: sc.name, name: "(scenario)", testid: null, problem: "shows no control" });
  }
  let dirty = false;
  for (const c of controls) dirty = await visit(page, sc, c, dirty, result);
  return result;
}

/** Checks one control; answers whether the page is now changed (and must
 * be prepared again before the next control). */
async function visit(page: Page, sc: SweepScenario, c: Control, dirty: boolean, result: SweepResult): Promise<boolean> {
  const note = (problem: string) => result.findings.push({ scenario: sc.name, name: c.name, testid: c.testid, problem });
  if (c.disabled) {
    result.disabled++;
    if (c.reason === "") note("disabled without a reason (title, aria-describedby or aria-description)");
    return dirty;
  }
  if (c.active) {
    result.skipped++;
    return dirty;
  }
  if (dirty) await sc.prepare();
  const outcome = await tryControl(page, c);
  if (c.testid === IDLE_PROOF) {
    if (outcome.problem !== NO_EFFECT) {
      note("the scenario is not idle: an inert control planted in it was credited with an effect");
    }
    return outcome.changed;
  }
  result.activated++;
  if (outcome.problem) note(outcome.problem);
  return outcome.changed;
}

/** Finds the control again, hovers and focuses it and lets both settle —
 * what a pointer or a focus handler does (a roving tabindex, a menu's hint
 * line) is not the click's effect — then activates it and waits for one. */
async function tryControl(page: Page, c: Control): Promise<{ changed: boolean; problem?: string }> {
  const found = await page.evaluate((key) => (window as unknown as ProbeWindow).__noopProbe.mark(key), c.key);
  if (!found) return { changed: true, problem: "was not on the page again after the scenario was re-prepared" };
  const target = page.locator("[data-noop-target]");
  if (c.tag !== "select") await target.hover({ timeout: 2_000 }).catch(() => undefined);
  await target.focus({ timeout: 2_000 }).catch(() => undefined);
  await settle(page);
  await page.evaluate(() => (window as unknown as ProbeWindow).__noopProbe.watch());
  const problem = await activate(page, c);
  if (problem !== null) return { changed: true, problem };
  if (await sawEffect(page)) return { changed: true };
  return { changed: false, problem: NO_EFFECT };
}

/** Polls for an effect for up to `EFFECT_WINDOW_MS`; true once one shows. */
async function sawEffect(page: Page): Promise<boolean> {
  try {
    await page.waitForFunction(() => (window as unknown as ProbeWindow).__noopProbe.effect(), undefined, {
      timeout: EFFECT_WINDOW_MS,
      polling: 50,
    });
    return true;
  } catch {
    return false;
  } finally {
    await page.evaluate(() => (window as unknown as ProbeWindow).__noopProbe.unwatch());
  }
}

/** Clicks the control, or picks another option of a `select`. Returns a
 * problem when it could not be activated at all. */
async function activate(page: Page, c: Control): Promise<string | null> {
  const target = page.locator("[data-noop-target]");
  try {
    if (c.tag === "select") {
      const next = await target.evaluate(
        (s: HTMLSelectElement) => Array.from(s.options).find((o) => !o.disabled && o.value !== s.value)?.value ?? "",
      );
      await target.selectOption(next, { timeout: 2_000 });
    } else {
      await target.click({ timeout: 2_000 });
    }
    return null;
  } catch (e) {
    // The first line is only "Timeout exceeded"; Playwright's call log says
    // why (another element intercepts the pointer, it is not visible…).
    const lines = String(e).split("\n").map((l) => l.trim());
    const why = [...lines].reverse().find((l) => /intercepts|not visible|not enabled|not stable|outside of the viewport/.test(l));
    return `could not be activated: ${(why ?? lines[0]).slice(0, 200)}`;
  }
}

/** One line per finding, for the assertion message. */
export function describe(findings: Finding[]): string {
  return findings
    .map((f) => `[${f.scenario}] "${f.name}" (data-testid=${f.testid ?? "none"}) ${f.problem}`)
    .join("\n");
}
