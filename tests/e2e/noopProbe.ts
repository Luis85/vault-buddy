/**
 * The no-op sweep's in-page half (visual-parity Task 24; `noopSweep.ts` is
 * the runner). `installNoopProbe` is handed to `page.addInitScript`, so it
 * runs — self-contained, nothing from this module's scope — in every load
 * of the page, before the bundle, and leaves `window.__noopProbe`:
 *
 * - `controls(scope)`: every reachable control (optionally inside `scope`),
 *   keyed so the same control is found again after a reload;
 * - `mark(key)`: tags that control with `data-noop-target`;
 * - `watch()` / `effect()` / `unwatch()`: records the page's state, then
 *   answers whether anything observable happened since — a new IPC call
 *   (the debounced `editor_save_workspace` and log calls aside), a mutation
 *   on a node the idle baseline (`window.__noisy`, left by `settle`) did not
 *   see, focus moving to another element, a change in the number of open
 *   dialogs/menus, or the target's `checked` changing (a property, which no
 *   `MutationObserver` sees).
 */

/** What the runner learns about one control. */
export interface Control {
  key: string;
  name: string;
  testid: string | null;
  tag: string;
  disabled: boolean;
  /** Already the active tab / radio / preset, or a select with no other
   * choice: activating it is expected to do nothing (the brief's skip
   * rule). */
  active: boolean;
  /** For a disabled control: the reason a person can reach, or "". */
  reason: string;
}

export interface NoopProbe {
  controls(scope?: string): Control[];
  mark(key: string): boolean;
  watch(): void;
  effect(): boolean;
  unwatch(): void;
}

export function installNoopProbe(): void {
  /** Anything a person can activate. Options and radios are included (the
   * design's gate names options); a `<summary>` opens its disclosure. */
  const SELECTOR = [
    "button",
    "[role=button]",
    "[role=menuitem]",
    "[role=menuitemcheckbox]",
    "[role=menuitemradio]",
    "[role=tab]",
    "[role=option]",
    "select",
    "input[type=checkbox]",
    "input[type=radio]",
    "summary",
  ].join(",");
  /** Open surfaces whose count changing is itself an effect. */
  const OVERLAYS = '[role="dialog"],[role="menu"],[role="alertdialog"],[role="listbox"]';
  const TARGET = "data-noop-target";

  type Win = Window & { __calls: { cmd: string }[]; __noisy?: Set<Node>; __noopProbe?: NoopProbe };
  const win = window as unknown as Win;

  /** Rendered, not `visibility: hidden`, not inside a closed `<details>`
   * (content-visibility) and not in a hidden drawer (`aria-hidden`/`inert`). */
  const reachable = (el: HTMLElement): boolean =>
    el.checkVisibility({ visibilityProperty: true, contentVisibilityAuto: true }) &&
    el.closest('[aria-hidden="true"],[inert]') === null;

  const nameOf = (el: HTMLElement): string =>
    (el.getAttribute("aria-label") || (el.textContent ?? "").trim().replace(/\s+/g, " ") || el.getAttribute("title") || "")
      .slice(0, 60);

  const baseOf = (el: HTMLElement): string =>
    el.getAttribute("data-testid") ?? `${el.getAttribute("role") ?? el.tagName.toLowerCase()}:${nameOf(el)}`;

  /** Every reachable control with its key: its test id (or role and name)
   * plus its position among controls sharing that id. */
  function keyed(): { el: HTMLElement; key: string }[] {
    const seen = new Map<string, number>();
    return Array.from(document.querySelectorAll<HTMLElement>(SELECTOR))
      .filter(reachable)
      .map((el) => {
        const base = baseOf(el);
        const n = seen.get(base) ?? 0;
        seen.set(base, n + 1);
        return { el, key: `${base}#${n}` };
      });
  }

  const textOfIds = (ids: string | null): string =>
    (ids ?? "")
      .split(/\s+/)
      .map((id) => document.getElementById(id)?.textContent?.trim() ?? "")
      .join(" ")
      .trim();

  /** A `title`, an `aria-describedby` that resolves to text, or an
   * `aria-description` (Ruling T24-1). */
  const reasonOf = (el: HTMLElement): string =>
    [el.getAttribute("title"), textOfIds(el.getAttribute("aria-describedby")), el.getAttribute("aria-description")]
      .map((s) => (s ?? "").trim())
      .find((s) => s !== "") ?? "";

  const isDisabled = (el: HTMLElement): boolean =>
    (el as HTMLButtonElement).disabled === true || el.getAttribute("aria-disabled") === "true";

  /** A select whose only enabled option is the current one: opening it
   * lists that one choice (a native popup no DOM observer sees), which is
   * all it can do. */
  const onlyChoice = (el: HTMLElement): boolean =>
    el instanceof HTMLSelectElement && !Array.from(el.options).some((o) => !o.disabled && o.value !== el.value);

  const checkedRadio = (el: HTMLElement): boolean => el instanceof HTMLInputElement && el.type === "radio" && el.checked;

  const ariaActive = (el: HTMLElement): boolean =>
    ["aria-selected", "aria-checked", "aria-pressed"].some((a) => el.getAttribute(a) === "true");

  const describe = (el: HTMLElement, key: string): Control => ({
    key,
    name: nameOf(el),
    testid: el.getAttribute("data-testid"),
    tag: el.tagName.toLowerCase(),
    disabled: isDisabled(el),
    active: onlyChoice(el) || checkedRadio(el) || ariaActive(el),
    reason: reasonOf(el),
  });

  function controls(scope?: string): Control[] {
    return keyed()
      .filter(({ el }) => !scope || el.closest(scope) !== null)
      .map(({ el, key }) => describe(el, key));
  }

  function mark(key: string): boolean {
    document.querySelectorAll(`[${TARGET}]`).forEach((e) => e.removeAttribute(TARGET));
    const hit = keyed().find((c) => c.key === key);
    hit?.el.setAttribute(TARGET, "");
    return hit !== undefined;
  }

  const target = () => document.querySelector<HTMLInputElement>(`[${TARGET}]`);
  const overlayCount = () => document.querySelectorAll(OVERLAYS).length;
  const newCalls = (from: number) =>
    win.__calls.slice(from).filter((c) => c.cmd !== "editor_save_workspace" && !c.cmd.startsWith("plugin:log"));

  let watching: {
    mutations: number;
    calls: number;
    active: Element | null;
    overlays: number;
    checked: boolean | undefined;
    obs: MutationObserver;
  } | null = null;

  function watch(): void {
    const noisy = win.__noisy ?? new Set<Node>();
    const state = {
      mutations: 0,
      calls: win.__calls.length,
      active: document.activeElement,
      overlays: overlayCount(),
      checked: target()?.checked,
      obs: new MutationObserver((records) => {
        state.mutations += records.filter((r) => r.attributeName !== TARGET && !noisy.has(r.target)).length;
      }),
    };
    state.obs.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    watching = state;
  }

  const focusMoved = (before: Element | null): boolean => {
    const now = document.activeElement;
    return now !== null && now !== before && now !== target() && now !== document.body;
  };

  const checkedChanged = (before: boolean | undefined): boolean => {
    const el = target();
    return el !== null && before !== undefined && el.checked !== before;
  };

  function effect(): boolean {
    if (!watching) return false;
    return (
      newCalls(watching.calls).length > 0 ||
      watching.mutations > 0 ||
      focusMoved(watching.active) ||
      overlayCount() !== watching.overlays ||
      checkedChanged(watching.checked)
    );
  }

  function unwatch(): void {
    watching?.obs.disconnect();
    watching = null;
  }

  win.__noopProbe = { controls, mark, watch, effect, unwatch };
}
