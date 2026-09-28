# Tutorial Editor Visual Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the shipped tutorial editor look and behave like the concept bundle (`docs/concepts/vault-buddy-editor/`), region by region, and make sure no enabled control is a no-op.

**Architecture:** A presentation-layer port inside the existing Vue 3 + Pinia + Tailwind 4 editor. First the foundations: a parity e2e harness with a populated sample project, an icon component built from the concept's own SVGs, the concept's tokens and type, and the concept's grid. Then every region in screen order: header, library, preview, transport, inspector, timeline, menus, dialogs, guide. The no-op fixes land early, and two e2e gates (parity geometry, no-op sweep) hold the result. Stores, IPC and Rust are not redesigned.

**Tech Stack:** Vue 3 SFCs, Pinia, Tailwind 4 (`@theme` tokens in `src/style.css`), Vitest + happy-dom (`tests/*.test.ts`), Playwright on the built `dist/` (`tests/e2e/*.spec.ts`, `tests/e2e/tauriStub.ts`).

**Spec:** `docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-design.md` (decisions D1–D16, the gates, the geometry table), which is the authority. It points to four supporting files in the same folder, which every task reads as needed:
- `…-visual-parity-concept-spec.md`: every value, cited as §N.M in this plan.
- `…-visual-parity-icons.json`: the 74 concept SVGs.
- `…-noop-audit.md`: findings 1–9.
- `…-built-inventory.md`: which component renders which region, and which tests couple to markup.

The visual truth is `docs/concepts/vault-buddy-editor/screens/*.png`.

## Global Constraints

- Never put a path, file name, caption or title into a log or an error message; the user sees role wording, never a `<path:#…>` / `<name:#…>` handle (`src/editor/errorCopy.ts` strips them at the webview boundary; keep every new message surface behind it).
- Never force-push, never bare `git stash`, never `git clean -fdx`, never `remove_dir_all`.
- Commits are Conventional Commits (`feat(editor): …`, `fix(editor): …`, `test(editor): …`, `docs: …`), with the body explaining the why, and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- Commit locally on `claude/editor-visual-parity`. **Do not push.**
- TDD: write the failing test first and see it fail for the right reason, then implement. For every new guard, mutate the implementation to confirm the test catches it, then restore.
- LOC caps: 500 nonblank lines per `.vue` / `.ts` file (`npm run check:loc`). Split a component before it crosses the cap, and never add to `scripts/loc-baseline.json`.
- The quality ratchet (`npm run check:quality`, `scripts/quality-baseline.json`) must not regress: deadCode 0 with no suppressions, complexFunctions ≤ 13, criticalComplexity ≤ 3, cloneGroups 0, circularDependencies 0, averageMaintainability ≥ 90.0. Hand-edit the baseline only when a metric IMPROVES. Never run `--update`.
- No new npm dependency. Icons come verbatim from the concept JSON.
- Existing `data-testid`s may be renamed only when every test and the guide target registry (`src/editor/guide/targets.ts`) move with them. All 22 guide lessons must still resolve a live target at 960×640 (`tests/e2e/editorGuide.spec.ts`).
- The contrast e2e (`tests/e2e/editorKeyboard.spec.ts`: 4.5:1 text, 3:1 UI boundaries, dark AND light) and the forced-colors rules (`.vb-playhead` / `.vb-handle` hooks in `src/style.css`) stay green.
- `tests/editorThemeTokens.test.ts` forbids any `white/N` literal under `src/components/editor/**`. Colours come from tokens.
- Every enabled control has an observable effect; every refusal is visible; every disabled control carries a reason. An item with no native backend is omitted, never shown disabled "until later" (spec D14).
- Browser-only concept copy is replaced with the native wording in spec D10. Keep the slot, change the words.
- **Gates after each task:**
  - `npm run lint`
  - `npm run check:loc`
  - `npm run check:quality` (with no `coverage/` dir present)
  - `npx vitest run`
  - `npm run build`
  - `npx playwright test` (needs the fresh build)
  - `npm run test:coverage` last, where coverage floors apply
  - Rust gates only if a task touches Rust (none should).

## Review Focus

1. **The light theme and Windows contrast themes after the palette port.** A person who switches to Light in the View menu, or runs a Windows contrast theme, must still read every label, and must still see the playhead, the fade and trim handles, the selected clip and focus rings. Owned by Task 3 (tokens) and re-measured by Task 25.
2. **Narrow windows (960×640 floor; the 1080 and 860 drawer breakpoints).** Nothing overflows horizontally. Save project and Render video stay on screen. Every guide lesson still finds its control. Opening a drawer never covers the control the coach is explaining. Owned by Task 4, with the drawer transitions re-asserted by Task 23.
3. **Long and extreme content.**
   - A 120-character project, track, clip or asset name ellipsizes and never pushes a control out of its row.
   - A 3-pixel-wide clip keeps its handles usable and doesn't render a broken badge.
   - An empty project (no tracks, no clips) shows the empty states instead of blank panels.
   - Twenty tracks scroll with the label column pinned.
   Owned by Tasks 9, 16, 17 and 18, each adding the extreme-content test for its region.
4. **Keyboard-only use of the new surfaces.**
   - Every new menu, submenu, toolstrip and drawer is reachable without a mouse.
   - Escape closes the innermost surface and returns focus to its opener.
   - Shift+F10 opens the context menu of the focused clip, track or cue.
   Owned by Task 5 (MenuPanel), Task 17 (track headers) and Task 19 (cues).
5. **Refusals and disabled reasons.**
   - A refused edit always produces a toast in role wording, with no redaction handle.
   - A disabled shortcut says why.
   - A revision conflict offers Retry.
   - Copying clips confirms what was copied.
   Owned by Task 7; Task 24's sweep re-asserts it for every control.

---

## Task order and dependencies

Tasks 1 through 7 build foundations every later task consumes (the harness, icons, tokens, grid, MenuPanel, dialog chrome, feedback). Tasks 8 through 23 port one region each; each one can be reviewed independently. Task 24 adds the no-op sweep gate across everything. Task 25 is the final parity pass and docs.

---

### Task 1: Parity harness and populated sample project

**Files:**
- Create: `tests/e2e/fixtures/parityProject.ts`
- Create: `tests/e2e/fixtures/thumb-160x90.jpg` (a real JPEG, generated once with ffmpeg from `tests/e2e/fixtures/capture-1920x1080.webm`, or any 160×90 JPEG under 20 KB)
- Create: `tests/e2e/parity.ts` (helpers)
- Create: `tests/e2e/editorParity.spec.ts`
- Modify: `tests/e2e/tauriStub.ts` (image URLs)

**Interfaces:**
- Produces:
  - `PARITY_OPEN_RESULT` and `PARITY_REPLIES: Record<string, unknown>` from `parityProject.ts`.
  - `openParity(page, size, opts?: { theme?: "dark" | "light"; invitation?: boolean })`, `box(page, testId): Promise<{x,y,width,height}>` and `composite(page, conceptScreen: string, builtPath: string, outName: string)` from `parity.ts`.
  - Every later task appends its region's `test(...)` blocks to `editorParity.spec.ts`, using these helpers.

- [ ] **Step 1: The sample project.** In `parityProject.ts`, build a decode-valid `EditorOpenResult`. Check every field against `src/editor/decodeProject.ts` and `src/editor/decode.ts`; read `decodeEffect`, `decodeTransition`, `decodeMarker` and the caption decoder for exact shapes. It mirrors concept screen 02:
  - title "Create your first project", canvas 1280×720 at 30 fps.
  - assets:
    - `capture` video "Getting started.capture", 36 000 ms
    - `presenter` video "Presenter · demo", 33 000 ms
    - `detail` video "Project detail.capture", 18 000 ms
    - `music` audio "Guide cues · synth", 36 000 ms
    - `bed` audio "Ambient bed · synth", 36 000 ms
  - tracks, top to bottom: `v3` "Webcam · presenter", `v2` "Detail overlay", `v1` "Screen recording", `a1` "Guide cues".
  - clips:
    - `c1` / `c2` / `c3` on v1 at 0 / 9 500 / 23 500 ms, lengths 9 500 / 14 000 / 10 000 ms, named "Open your workspace", "Create a project" and "Save to your vault".
    - `c4` "A closer look" on v2, 11 000 ms for 7 200 ms.
    - `c5` "Presenter · demo" on v3, 1 500 ms for 32 000 ms, with layout `x .75, y .05, w .2, h .2` and `fade_in_ms` / `fade_out_ms` 600.
    - `c6` "Chapter cues · demo" on a1, 300 ms for 33 000 ms.
  - effects (Teaching cues): a text cue "A little structure. A lot more clarity." 500–6 500 ms, a highlight 2 700–8 000 ms, an arrow 10 000–17 000 ms, a text cue "Give your project a name. Make it yours." 10 000–17 000 ms, a zoom 18 000–23 000 ms (factor 1.65), and a text cue "Ready to find." 24 000–30 000 ms. Each is attached to the clip it overlaps, per the decoder's attach field.
  - markers at 9 500 and 23 500 ms.
  - one transition between c1 and c2 (500 ms).
  - two captions on c5.
  - `persistedRevision` 1 against revision 3, so the project reads unsaved.

  `PARITY_REPLIES`:
  - `editor_get_guide_progress`: fresh, invitation NOT dismissed.
  - `editor_save_guide_progress`: null.
  - `editor_get_workspace`: `{}`.
  - `editor_save_workspace`: null.
  - `editor_get_checks`: `[]`.
  - `editor_get_products`: one available product.
  - `editor_get_jobs`: `[]`.
  - `list_vaults`: `[{id:"vault-e2e", name:"Knowledge vault", path:"C:/v", open:false}]`.
  - `editor_media_url`: `"C:/fixture/capture.webm"`.
  - `editor_media_thumbnail`: `"C:/fixture/thumb.jpg"`.
  - `editor_media_peaks`: `{peaks: Array.from({length:400},(_,i)=>Math.abs(Math.sin(i/7))*0.6)}`.
  - `editor_list_projects`: two summaries (the open project and "Onboarding walkthrough").
- [ ] **Step 2: Stub images.** Today `tauriStub.ts`'s `convertFileSrc` returns the video URL for every path. Add `FIXTURE_IMAGE_URL = "/__fixture__/thumb.jpg"` and return it for paths ending in `.jpg`, `.jpeg` or `.png`. Export it.
- [ ] **Step 3: Helpers.** Add these to `parity.ts`:
  - `openParity` installs the stub with `PARITY_OPEN_RESULT` and `PARITY_REPLIES`. It routes the video and image fixtures, then `page.emulateMedia({ colorScheme: opts.theme ?? "dark" })`, `setViewportSize`, `goto("/")`. It waits for `editor-shell`, then 800 ms for layout. When `invitation` is false it dismisses the guide invitation through its "Not now" button.
  - `box` returns `page.getByTestId(id).boundingBox()` and throws a named error if it's null.
  - `composite` reads `docs/concepts/vault-buddy-editor/screens/<conceptScreen>` and `builtPath` as base64. It `page.setContent`s a white page with the two images side by side (concept left, built right, each captioned) at their natural size, then screenshots to `test-results/parity/<outName>.png`. `page.setContent` needs no server.
- [ ] **Step 4: The first parity test (it fails today).** Add to `editorParity.spec.ts`:

```ts
import { expect, test } from "@playwright/test";
import { box, composite, openParity } from "./parity";

test.describe("parity 1600x1000", () => {
  test("frame: header 56, library 244, inspector 276, status 25", async ({ page }) => {
    await openParity(page, { width: 1600, height: 1000 }, { invitation: false });
    await page.screenshot({ path: "test-results/parity/built-02-workspace.png" });
    await composite(page, "02-workspace.png", "test-results/parity/built-02-workspace.png", "vs-02-workspace");
    expect((await box(page, "editor-header")).height).toBeCloseTo(56, 0);
    expect((await box(page, "editor-shell-library")).width).toBeCloseTo(244, 0);
    expect((await box(page, "editor-shell-inspector")).width).toBeCloseTo(276, 0);
    expect((await box(page, "editor-statusbar")).height).toBeCloseTo(25, 0);
  });
});
```

  Check the test ids against `src/components/editor/shell/EditorShell.vue`. The library and inspector wrappers already carry `editor-shell-library` / `editor-shell-inspector`; `editor-statusbar` does not exist yet, and Task 4 creates it.
- [ ] **Step 5: Run it and confirm the failure.** `npm run build && npx playwright test tests/e2e/editorParity.spec.ts`. Expected: FAIL. The header is not 56 px, or `editor-statusbar` is not found. The composite PNG is still written, because the screenshot runs before the asserts; confirm the file exists.
- [ ] **Step 6: Commit.** The failing test is the gate Tasks 3–4 turn green. Mark it `test.fail()` until Task 4, with a comment `// turned green by Task 4 (frame)`, so the suite stays green between tasks.

```bash
git add tests/e2e/fixtures/parityProject.ts tests/e2e/fixtures/thumb-160x90.jpg tests/e2e/parity.ts tests/e2e/editorParity.spec.ts tests/e2e/tauriStub.ts
git commit -m "test(editor): add the concept-parity e2e harness and sample project"
```

---

### Task 2: EditorIcon from the concept's SVGs

**Files:**
- Create: `src/components/editor/icons/conceptIcons.ts` (typed data module)
- Create: `src/components/editor/icons/EditorIcon.vue`
- Create: `tests/editorIcon.test.ts`
- Create: `scripts/gen-editor-icons.mjs` (one-shot generator, kept for regeneration)

**Interfaces:**
- Produces:
  - `type EditorIconName` (the 74 names)
  - `ICONS: Readonly<Record<EditorIconName, string>>` (inner SVG markup)
  - `<EditorIcon :name="…" :size="17" />`, which renders `<svg viewBox="0 0 24 24" aria-hidden="true" class="vb-icon" width height fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">` plus the inner markup
- Consumes: `docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json`, which maps each name to `{viewBox, svgInnerMarkup, …}`. Skip its `_meta` key.

- [ ] **Step 1: Failing test.**

```ts
import { mount } from "@vue/test-utils";
import { describe, expect, it } from "vitest";
import json from "../docs/superpowers/specs/2026-09-26-tutorial-editor-visual-parity-icons.json";
import EditorIcon from "../src/components/editor/icons/EditorIcon.vue";
import { ICONS } from "../src/components/editor/icons/conceptIcons";

describe("EditorIcon", () => {
  const names = Object.keys(json).filter((k) => k !== "_meta");
  it("carries every concept icon verbatim", () => {
    expect(Object.keys(ICONS).sort()).toEqual(names.sort());
    for (const n of names) expect(ICONS[n as keyof typeof ICONS]).toBe((json as any)[n].svgInnerMarkup);
  });
  it("renders a decorative currentColor stroke svg at the requested size", () => {
    const w = mount(EditorIcon, { props: { name: "scissors", size: 14 } });
    const svg = w.find("svg");
    expect(svg.attributes()).toMatchObject({ "aria-hidden": "true", width: "14", height: "14", stroke: "currentColor", fill: "none", viewBox: "0 0 24 24" });
    expect(svg.html()).toContain(ICONS.scissors);
  });
  it("defaults to 17px", () => {
    expect(mount(EditorIcon, { props: { name: "plus" } }).find("svg").attributes("width")).toBe("17");
  });
});
```

- [ ] **Step 2: Run it.** `npx vitest run tests/editorIcon.test.ts`. Expected: FAIL, because the modules don't exist yet.
- [ ] **Step 3: Implement.**
  - The generator writes `conceptIcons.ts` as `export const ICONS = { name: "<markup>", … } as const; export type EditorIconName = keyof typeof ICONS;`, with LF line endings and one line per icon.
  - `EditorIcon.vue` binds the inner markup with `v-html` (the markup is the app's own constant, never user input; say so in a comment).
  - If `conceptIcons.ts` exceeds 500 lines, split it alphabetically into two data files re-exported by one index.
- [ ] **Step 4: Run it.** Expected: PASS. Then `npm run lint && npm run check:loc && npm run check:quality`.
- [ ] **Step 5: Commit.** `feat(editor): add the concept's icon set as one EditorIcon component`

---

### Task 3: Concept tokens, type and base controls; dark by default

**Files:**
- Modify: `src/style.css` (the editor token block: `@theme` defaults, `[data-theme="light"]`, `[data-theme="dark"]`; add an editor-scoped base layer)
- Modify: `src/stores/editorWorkspace.ts` (default theme, D1)
- Modify: `tests/editorThemeTokens.test.ts`, `tests/e2e/editorKeyboard.spec.ts` (only if a measured pair legitimately changes)
- Test: `tests/editorWorkspaceTheme.test.ts` (new)

**Interfaces:**
- Produces:
  - Token utilities later tasks use: `bg-app`, `bg-panel`, `bg-raised`, `bg-stage`, `border-line`, `bg-hover`, `text-fg` / `-secondary` / `-muted` / `-subtle`, `text-accent`, `bg-accent-bg`, `text-accent-ink`, `bg-primary`, `text-video` / `bg-video-bg`, `text-audio` / `bg-audio-bg`, `text-gold` / `bg-gold-bg`, `text-danger`, `outline-ring`.
  - Map each §1.1 row onto an existing token name where one exists. Add the missing ones (`--color-accent-bg`, `--color-accent-ink`, `--color-primary`, `--color-primary-hover`, `--color-ring`, `--color-guide-edge`, `--color-guide-dim`, `--color-backdrop`).
  - A scoped class `.vb-editor` (put on EditorRoot's root) carrying the base rules.

- [ ] **Step 1: Failing tests.**

```ts
// tests/editorWorkspaceTheme.test.ts
import { createPinia, setActivePinia } from "pinia";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useEditorWorkspaceStore } from "../src/stores/editorWorkspace";

describe("editor theme default (D1)", () => {
  beforeEach(() => setActivePinia(createPinia()));
  it("opens dark even when the OS prefers light", () => {
    vi.stubGlobal("matchMedia", (q: string) => ({ matches: q.includes("light"), media: q, addEventListener() {}, removeEventListener() {} }));
    expect(useEditorWorkspaceStore().theme).toBe("dark");
    vi.unstubAllGlobals();
  });
});
```

  Also add a parity test that reads computed styles at 1600×1000 in dark:
  - `getComputedStyle(document.body).fontSize === "12px"`
  - the header background `rgb(32, 33, 39)` (#202127)
  - `.vb-editor` background `rgb(24, 25, 30)` (#18191e)
  - a primary button background `rgb(139, 106, 212)` (#8b6ad4)
- [ ] **Step 2: Run both.** Expected: FAIL. The theme follows the OS, and the colours differ.
- [ ] **Step 3: Implement.**
  - Port §1.1 dark into the `@theme` defaults and `[data-theme="dark"]`, and light into `[data-theme="light"]`.
  - Port §1.2 into `.vb-editor`: `font: 12px/1.5 "Segoe UI", system-ui, sans-serif`, plus a `.vb-mono` utility (`ui-monospace, Consolas, monospace; font-variant-numeric: tabular-nums`).
  - Port the §1.5 button, input, select, checkbox and range base rules, scoped under `.vb-editor`:
    - buttons: radius 7, min-height 32, hover `--color-hover`, disabled opacity .4
    - inputs: radius 6, min-height 34, bg `app`
  - Port the §1.6 focus ring (2px ring, offset 2), the `.active` pressed state and the thin timeline scrollbars.
  - `editorWorkspace`: seed the theme with `"dark"`, remove `prefersLightTheme`, and keep the saved `theme` winning. Keep `data-theme` applied by `EditorShell`.
  - Rerun `tests/e2e/editorKeyboard.spec.ts`'s contrast checks. For any pair under 4.5:1 (text) or 3:1 (UI), adjust that one token minimally and record the delta in the commit body.
- [ ] **Step 4: Run it.** Expected: PASS: vitest, `npx playwright test tests/e2e/editorKeyboard.spec.ts tests/e2e/editorParity.spec.ts`, and the full gates.
- [ ] **Step 5: Commit.** `feat(editor): take the concept's palette, type and control styles; open dark by default`

---

### Task 4: The frame: grid, splitter, status bar, panel toggles at every width

**Files:**
- Modify: `src/components/editor/shell/EditorShell.vue` (grid, breakpoints, drawers, the toggles, D4 and D5)
- Modify: `src/roots/EditorRoot.vue` (root class `vb-editor`, no outer gutters)
- Create: `src/components/editor/shell/EditorStatusBar.vue` (`data-testid="editor-statusbar"`, D10 copy)
- Modify: `src/components/editor/timeline/TimelineView.vue` (resize handle → the 8 px splitter row, §6.1)
- Modify: `src/stores/editorWorkspace.ts` (timeline height clamp, if it's not already there)
- Test: `tests/e2e/editorParity.spec.ts` (drop `test.fail()` from Task 1; add compact), `tests/e2e/editorShell.spec.ts` (update the size contract), `tests/editorShell.test.ts`

**Interfaces:**
- Consumes: the Task 3 tokens.
- Produces:
  - The regions carry `data-testid`s `editor-header`, `editor-shell-library`, `editor-shell-preview`, `editor-shell-inspector`, `editor-splitter`, `editor-timeline`, `editor-statusbar`.
  - `editorWorkspace` has `libraryVisible`, `inspectorVisible` and `focusPreview`, each a real grid state at all widths.

- [ ] **Step 1: Failing tests.**
  - Drop Task 1's `test.fail()`.
  - Add compact (960×640): header 52, preview header 44, transport 40, timeline 270 ±2, status 23, and label column 174 (the label column is measured once Task 17 exists; put it behind `test.fixme` with the note "Task 17").
  - Add a D5 test at 1600×1000: open the View options (the ⋯ button in the preview toolbar, or its current equivalent before Task 11) and choose "Focus preview". Assert `editor-shell-library` and `editor-shell-inspector` have width 0 or are hidden, and that `editor-shell-preview`'s width grows by ≥ 500 px. Repeat for the library toggle alone (the preview grows by 244 ±1).
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement §1.3, §1.4, §1.5 and §6.1.**
  - Grid rows `56px minmax(170px,1fr) 8px var(--timeline) 25px`.
  - Columns `var(--editor-sidebar) minmax(0,1fr) var(--editor-inspector)`, with each column 0 when hidden.
  - 1px `line` separators, no gutters or radii on regions.
  - The ≤1080 inspector overlay drawer (276, shadow), the ≤860 library drawer (250) and the ≤760 tall variant.
  - Keep the `window.innerWidth` ref approach (happy-dom has no media queries, per the inventory §2), with the thresholds 1350 / 1080 / 860 and height 760.
  - Splitter: 8 px, grip 42×2 muted at 50%, `role="separator"`, ArrowUp/Down ±25, drag clamped to `170 … min(540, innerHeight − 370)`. Persisted as today.
  - Status bar (§7): 9 px muted, three slots, with the D10 copy. The centre recovery state reads `editorProject.dirty`; the right slot is the product count from `editorProducts` and opens the library Project section. Until Task 10 builds that section, emit the existing reveal for the Products tab.
- [ ] **Step 4: Update the old shell contract.** `tests/e2e/editorShell.spec.ts` asserted the 1180 px drawer threshold and the preview-toolbar row. Move its assertions to the new thresholds (1080/860), keeping its intent: Save and Render stay on screen at all four sizes, there's no horizontal scroll, and there's one preview toolbar row.
- [ ] **Step 5: Run everything.** Expected: PASS, including `tests/e2e/editorGuide.spec.ts` at 960×640 (the coach must still find every lesson's control).
- [ ] **Step 6: Commit.** `feat(editor): take the concept's frame; make the panel toggles work at every width`

---

### Task 5: MenuPanel and the context-menu item sets

**Files:**
- Create: `src/components/editor/menus/MenuPanel.vue` (§8 panel: heading, items, separators, submenus, hint footer)
- Create: `src/components/editor/menus/menuModel.ts` (types plus positioning/clamping helpers)
- Create: `src/editor/menuSets.ts` (the item sets for clip, multi, track, lane/gap, cue and media asset; split into `menuSetsClip.ts` / `menuSetsOther.ts` if it nears the cap)
- Modify: `src/components/editor/menus/ContextMenu.vue` (render through MenuPanel with the sets)
- Test: `tests/editorMenuPanel.test.ts`, `tests/editorMenuSets.test.ts`, plus a parity block for screen 03

**Interfaces:**
- Consumes: `resolveActions(ctx)` / `activateEditorAction` from `src/editor/actions.ts` and `clipboard.ts`; `EditorIcon`.
- Produces:
  - `type MenuItem = { id: string; label: string; icon?: EditorIconName; kbd?: string; checked?: boolean; danger?: boolean; disabledReason?: string | null; submenu?: MenuItem[]; run?: () => void } | { separator: true }`.
  - `<MenuPanel :heading :subtitle :items :anchor="{x,y} | HTMLElement" @close>`.
  - `clipMenu(ctx)`, `multiClipMenu(ctx)`, `trackMenu(ctx, trackId)`, `laneMenu(ctx, trackId, atMs)`, `cueMenu(ctx, effectId)` and `assetMenu(ctx, assetId)`, each returning `MenuItem[]`.

- [ ] **Step 1: Failing unit tests.**
  - `MenuPanel`:
    - It renders the heading and mono subtitle.
    - Items get `role="menuitem"` (or `menuitemcheckbox` when `checked` is defined).
    - A separator gets `role="separator"`.
    - ArrowDown/ArrowUp move focus across enabled AND disabled items.
    - Focusing a disabled item writes its reason into the `aria-live` hint.
    - Enter/→ on a submenu item opens the submenu and focuses its first item; ← or Escape closes the submenu only.
    - Escape on the root emits `close`.
    - The kbd renders right-aligned.
    - `danger` items carry the danger class.
    - Clicking a disabled item does nothing but show its reason.
  - `menuSets`:
    - `clipMenu` has exactly the §8 single-clip labels and order that have a native backend. For each label, assert its `run` activates the registry action or command named in the mapping table below.
    - No item has `run === undefined && !submenu` unless it's disabled with a reason.
    - No label from the omitted list below appears.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement §8.**
  - Panel: 282 px (260 at ≤1300 w), radius 10, padding 6, the §8 shadow, a sticky heading (11 px/600 name plus a mono 9 px subtitle "Direct editing · originals unchanged").
  - Items: 31 px tall, 11 px, a 17 px glyph slot with a 14 px icon.
  - Hover and focus use `accent-bg` / `accent-ink`. Disabled items are at opacity .5.
  - Separators: 1px line, margin 5 8.
  - The hint footer defaults to "↑ ↓ navigate · Enter choose · Esc close".
  - Submenus open after 180 ms hover or on Enter/→, at right+4, flipping to the left when they would overflow.
  - The panel is clamped 8 px inside the viewport.

  Mapping (native), each to an existing registry action / `editor_execute` command / dialog / reveal:
  - **Clip:**
    - Go to this clip: seek plus timeline scroll.
    - Fit this clip: timeline zoom-to-range. If no zoom-to-range exists, implement it in `editorWorkspace` view state (zoom + scroll only, no edit).
    - Copy clip, Cut clip, Duplicate: existing registry actions.
    - Rename… (F2): the inspector Clip tab, name field focused.
    - Split at {playhead}: `splitClip`.
    - Trim to pointer ▸ Trim start to here / Trim end to here: `trimClip`.
    - Speed ▸ 0.25× … 4×, Speed & timing options…: `setSpeed`, then the inspector Speed tab.
    - Transform ▸: `setLayout` for fit/fill/rotate/flip/center/full frame/reset, where `setLayout` supports the field. An item whose field the model lacks is omitted.
    - Color treatment ▸ Original / Clear / Warm / Soft / Mono, Adjust color…: `setAdjustments` presets as defined in `ColorSection.vue`, then the Color tab.
    - Cover private information: the existing Privacy cover tool.
    - Fades ▸: `setFades` presets, then the Fades tab.
    - Audio ▸ Mute / Unmute clip, Detach audio · keep aligned, Audio properties…: `setClipMix`, `detachAudio`, the Audio tab.
    - Copy / paste look ▸: only if the clipboard layer already supports a "look" copy. Otherwise omit the item (D14).
    - Add caption here…: `addCaption` at the playhead on this clip, then the Captions tab.
    - Clear selection (V), Delete · leave gap (Delete, danger), Delete · ripple this track (danger): `deleteClips` in the two modes.
  - **Multi-clip:** per §8, using `groupClips` / `ungroupClips` / `setFades` / `setAdjustments` / `setClipMix` / `deleteClips`.
  - **Track:**
    - Select clips on this track.
    - Rename track…: `renameTrack` via an inline rename.
    - Visible video, Mute audio, Solo audio, Lock track: `setTrackFlags`, checkbox items.
    - Move track up / down: `moveTrack`, disabled with "Already the top track" / "Already the bottom track" (audit finding 6).
    - Close gaps on this track: the existing close-gap action.
    - Clear selection.
    - Remove track… (danger): `deleteTrack` behind the existing confirm.
  - **Lane/gap:**
    - Move playhead to {t}.
    - Paste clips here (Ctrl+V).
    - Close this gap · this track.
    - Close all gaps · this track.
    - Add title here: `addCard`.
    - Insert intro · shift all tracks: `insertIntro`.
    - Add video track, Add audio track: `addTrack` (audit finding 3).
    - Fit timeline (F).
    - Snapping (N): checkbox.
  - **Cue:**
    - Edit annotation: select it, inspector.
    - Duplicate annotation: `addEffect` with a copy offset by 500 ms, or omit if `addEffect` can't take a full effect.
    - Select attached clip.
    - Clear selection.
    - Delete annotation (danger): `removeEffect`.
  - **Media asset:**
    - Add at playhead.
    - Add on a new track: `addTrack` then `insertClip`.
    - Reconnect original… (when missing).
    - Rename library label / Remove from library have no backend and are OMITTED.

  **Omitted list:** "New project…", "Restore sample project…", "Download annotated frame…", "Rename library label…", "Remove from library".
- [ ] **Step 4: Rewire `ContextMenu.vue`** onto `MenuPanel` with the clip / multi / lane sets, keeping the same openers (right-click, Shift+F10, Edit actions). Keep its existing `data-testid`s on the new root, or update the tests that use them.
- [ ] **Step 5: Parity test for screen 03.** Right-click `clip-c2` and assert:
  - the menu is 282 ±1 wide
  - the heading reads "Create a project"
  - the first item is "Go to this clip"
  - a `.danger` item "Delete · leave gap" exists
  - every non-separator item contains an `svg`

  Write the composite `vs-03-context-menu`.
- [ ] **Step 6: Run everything.** Expected: PASS.
- [ ] **Step 7: Commit.** `feat(editor): one MenuPanel with the concept's context menus, every item wired or omitted`

---

### Task 6: Dialog chrome and toast

**Files:**
- Modify: `src/components/editor/shell/DialogHost.vue` (§9 chrome)
- Modify: the notification rendering the editor uses (find where `NotificationHost` is mounted in `EditorShell.vue`; restyle it for the editor per §9.12 without changing the panel window's look. If it's shared with the panel, add an `editor` variant prop.)
- Test: `tests/editorDialogHost.test.ts` (extend the existing test if there is one)

**Interfaces:**
- Produces: DialogHost slots `title`, `subtitle`, default (body) and `footer`; a close ✕ icon button in the header; a sticky footer; a backdrop `#080711aa` with a 4px blur; width via a prop (`560` default, `680` checks, `960` webcam, `660` session, `870` learning center).

- [ ] **Step 1: Failing test.**
  - DialogHost renders a header with the title (16px/600) and the subtitle; a ✕ `button[aria-label="Close"]` containing an `svg`; the body; and a footer outside the scrolling body.
  - The width prop applies.
  - Escape still closes, and focus returns to the opener (existing behaviour).
- [ ] **Step 2: Run it.** Expected: FAIL.
- [ ] **Step 3: Implement §9 shared chrome and §9.12.**
  - Toast: fixed, centred, bottom 43, `raised`, radius 8, 11/17 padding, 12 px, sliding up 15 px over .16 s (none under reduced motion).
  - Migrate every dialog using DialogHost to the named slots. Behaviour must not change; only markup moves into slots.
- [ ] **Step 4: Run everything.** Expected: PASS, including every dialog's existing unit tests.
- [ ] **Step 5: Commit.** `feat(editor): concept dialog chrome and toast`

---

### Task 7: Feedback: refusals, conflicts, disabled reasons, copy, free-track inserts (audit 1, 1a, 4, 7, 8, 9)

**Files:**
- Create: `src/composables/useEditorFeedback.ts` (the watchers, mounted once by `EditorShell`)
- Modify: `src/components/editor/shell/EditorShell.vue` (mount it; announce disabled shortcut reasons, finding 9)
- Modify: `src/editor/clipboard.ts` (copy confirmation, finding 8)
- Modify: the Media "+" and Titles card insert paths (find them via `MediaLibrary.vue` / `TitlesLibrary.vue`; audit finding 1a), reusing `TimelineView`'s `addTrackThenInsert` logic moved into `src/editor/placeOnFreeTrack.ts`
- Modify: `src/components/editor/shell/EditorHeader.vue` ("Save failed" reason, finding 7; the header itself is restyled in Task 8)
- Test: `tests/editorFeedback.test.ts`, `tests/editorPlaceOnFreeTrack.test.ts`

**Interfaces:**
- Produces:
  - `useEditorFeedback()`.
  - `placeOnFreeTrack(project, kind, atMs, lengthMs): { trackId: string } | { newTrackIndex: number }`, a pure function.
  - The first track of `kind`, top-down, with no clip overlapping `[atMs, atMs+lengthMs)`. Otherwise `{ newTrackIndex }` for a new track above the topmost track of that kind (video) or below the last audio track (audio).

- [ ] **Step 1: Failing tests.**
  - When `editorProject.lastError` becomes a non-null `EditorError`, exactly one error toast shows its message after `errorCopy` (no `<path:#`). The same error object never toasts twice.
  - When `editorProject.conflictIntent` is set, a toast shows "Your edit wasn't applied because the project changed. Retry?" with a Retry action that calls `retryConflict()`.
  - Copying 2 clips raises an info toast "Copied 2 clips" ("Copied 1 clip" in the singular).
  - Pressing `S` with the playhead outside every clip raises an info toast with the registry's disabled reason for split, and the keydown still does not split.
  - A save failure with `saveError.message = "The disk is full."` renders that text as the header's save-state `title` and toasts it once.
  - `placeOnFreeTrack`:
    - an overlap on v3 at 0 ms picks v2 when v2 is free
    - every video track busy returns `{newTrackIndex: 0}`
    - audio returns the first free audio track, or a new one after the last audio track
  - The Titles "Intro" card on the parity-like fixture sends `insertClip`/`addCard` on a free track, or `addTrack` then the insert. Assert the command sequence against a mocked port.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
  - Toasts go through the existing notifications store (the store's timers must call plain functions, per the AGENTS.md testing rule about Pinia action timers).
  - `lastError` is cleared on the next success by the store (existing).
  - The disabled-shortcut announcement goes through the same toast, rate-limited to one per 1.5 s.
- [ ] **Step 4: Run everything.** Expected: PASS.
- [ ] **Step 5: Commit.** `fix(editor): make every refusal, conflict and disabled shortcut visible; insert onto a free track`

---

### Task 8: Header and the Project menu (screens 01–02, §2)

**Files:**
- Modify: `src/components/editor/shell/EditorHeader.vue` (split out `BrandMark.vue`, `ProjectMenuButton.vue` and `SaveStateIndicator.vue` so it stays under the cap)
- Modify: `src/components/editor/shell/ChecksButton.vue`, `GuideHelpButton.vue`, `RenderVideoButton.vue`
- Delete: `src/components/editor/menus/SaveProjectMenu.vue` (its items move into the Project menu, D6/D7), and update its tests
- Create: `src/components/editor/dialogs/OpenProjectDialog.vue` (picker, D7)
- Create: `src/components/editor/dialogs/RenameDialog.vue` (§9.10: "Rename tutorial", Title field, Cancel / Apply → `rename`)
- Test: `tests/editorHeader.test.ts` (update), `tests/editorProjectMenu.test.ts`, parity blocks

**Interfaces:**
- Consumes: `MenuPanel` (Task 5), DialogHost (Task 6), `editorProject.openProject`, `port.listProjects`, `port.importPackage` (existing), SaveProjectDialog, DiscardProjectDialog.
- Produces: `data-testid`s `editor-header-project-menu`, `editor-header-title` (the button), `editor-header-save-state`. The existing `editor-header-help/-checks/-save/-render` stay. `editor-header-theme-toggle` is removed (Task 11 moves the theme to the View menu). Remove it from tests and guide targets consistently.

- [ ] **Step 1: Failing tests.**
  - The header height is 56 (1600) / 52 (compact).
  - Brand mark and wordmark "vault-buddy"; the wordmark is hidden at ≤1350.
  - The Project menu has exactly the D7 labels in order, with icons; "Open project…" is absent when `listProjects` returns only the open project.
  - Each item's effect:
    - Open project… opens `OpenProjectDialog`, and choosing a row calls `openProject(id)`.
    - Open a project file… → `editor_import_package`.
    - Rename tutorial… → `RenameDialog` → `editor_execute {kind:"rename"}`.
    - Workspace & rendered products → the library Project section, or the Products reveal until Task 10.
    - Save a copy as project file… → SaveProjectDialog.
    - Discard project… → DiscardProjectDialog.
  - The title click opens RenameDialog.
  - The save state is a gold dot + "Unsaved changes" when dirty; teal + "Saved" when clean.
  - No vault id text anywhere in the header.
  - Help, Checks, Save project and Render video each contain an `svg`.
  - Checks shows the count chip only when there are findings.
  - The parity block at 1600×1000 writes the composite `vs-01-welcome` (with the invitation) and asserts the header geometry.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement §2 with the D6/D7/D10 native adaptations.** The brand mark is the CSS drawing from §2 (no image file).
- [ ] **Step 4: Run everything.** Expected: PASS, including the guide spec (the lessons that target the header's Save/Render/Help/Checks still resolve).
- [ ] **Step 5: Commit.** `feat(editor): concept header with the brand, a native Project menu and save state`

---

### Task 9: Library frame and the Media tab (§3.1–3.2)

**Files:**
- Modify: `src/components/editor/library/LibraryPanel.vue` (four pill tabs; Products leaves the tabs, D9)
- Modify: `src/components/editor/library/MediaLibrary.vue`, `LibraryAssetCard.vue`, `ImportStatus.vue`
- Test: `tests/editorMediaLibrary.test.ts` (update), parity blocks

**Interfaces:**
- Consumes: `editor_media_thumbnail` (existing port method) for the 58×40 thumbnail at 1 000 ms or at the asset's midpoint if it's shorter; `assetMenu` (Task 5) on right-click / Shift+F10.
- Produces: `data-testid`s `library-tab-media|titles|captions|chapters`, `library-import`, `library-webcam`, `library-search`, `library-asset-<id>`, `library-asset-<id>-add`.

- [ ] **Step 1: Failing tests.**
  - The tabs are exactly Media / Titles / Captions / Chapters; the active one is a filled pill (class `bg-accent-bg`).
  - Import media and Webcam are 40 px tall with icons.
  - The search input has a search icon and placeholder "Find media…", and filters by name.
  - The heading reads "SOURCE MEDIA" with the pill "{n} assets".
  - Each row: a thumbnail `img` 58×40 (video) or a music tile (audio); the name ellipsized; the meta "MM:SS · W × H" (video) / "MM:SS · Local audio" / "Missing source"; a + button (`aria-label="Add {name} to timeline"`), or a link icon "Reconnect {name}" when the asset is missing (opens ReconnectDialog).
  - Right-click opens the asset menu.
  - Extreme content: a 120-character asset name ellipsizes without widening the row.
  - An empty library shows the concept's empty text.
  - Parity at 1600: the tab row is 48; the import button is 40; the thumbnail is 58×40.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement §3.1–3.2.** The + insert uses Task 7's `placeOnFreeTrack`.
- [ ] **Step 4: Run everything.** Expected: PASS, including the guide ("media" lesson).
- [ ] **Step 5: Commit.** `feat(editor): concept library tabs and media list`

---

### Task 10: Titles, Captions and Chapters tabs, and the Project section (§3.3–3.6)

**Files:**
- Modify: `src/components/editor/library/TitlesLibrary.vue`, `CaptionsLibrary.vue` (+ `CaptionCueRow.vue`, `CaptionsToolbar.vue`, `CaptionSettingsPanel.vue`, `CaptionsExport.vue`), `ChaptersLibrary.vue`
- Create: `src/components/editor/library/ProjectSection.vue` (§3.6; hosts the existing `ProductLibrary` / `ProductCard` restyled per §3.6)
- Modify: `src/editor/revealBus.ts` (a `projectSection` surface; the Task 4 status bar and the Task 8 Project menu switch to it)
- Test: update the four libraries' unit tests; parity block for screen 06 (Captions)

**Interfaces:**
- Consumes: `placeOnFreeTrack` (Titles insert), existing caption and chapter commands.
- Produces: `data-testid` `library-project-section` with a "Back to media" link.

- [ ] **Step 1: Failing tests.**
  - Titles:
    - the "GIVE IT STRUCTURE" heading
    - the Insert-intro button → `insertIntro`
    - four 16:9 template cards with the §3.3 texts and colours, each inserting a title card on a free track
    - the still-image import is omitted if no backend
  - Captions:
    - "EVERY WORD, ACCESSIBLE" + count pill
    - the attached-source box
    - Add caption; Import SRT / VTT (disabled with a reason when no clip is selected)
    - a "Caption appearance" `<details>`
    - caption cards with a mono gold time, edit, Split cue and trash
    - the empty text
    - native "Export timeline SRT / VTT" (replacing "Download")
  - Chapters: "TUTORIAL CHAPTERS", numbered rows, "Add chapter at playhead", and the tip box.
  - Project section:
    - "YOUR WORKSPACE" + revision pill
    - product cards (Watch, Restore) + an empty state + "Render a new video" (opens the Render dialog)
    - Back to media
  - Parity writes `vs-06-captions`.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.** The guide lesson targeting products must resolve in the Project section (check `targets.ts` and `prepare.ts`: preparing that lesson reveals the section).
- [ ] **Step 4: Run everything.** Expected: PASS (guide spec included).
- [ ] **Step 5: Commit.** `feat(editor): concept titles, captions, chapters and the workspace section`

---

### Task 11: Preview header, the View and More-tools menus, Frame dialog, stage selection (§4.1–4.2, D15)

**Files:**
- Modify: `src/components/editor/shell/PreviewToolbar.vue` (split into `PreviewHeader.vue` + `Toolstrip.vue` if needed), `RatioSelect.vue` → `RatioButton.vue`
- Delete: `src/components/editor/shell/ToolbarOverflowMenu.vue` (replaced by MenuPanel menus)
- Create: `src/components/editor/dialogs/FrameDialog.vue` (§9.11: four ratio cards → `setCanvas`)
- Modify: `src/components/editor/preview/PreviewSurface.vue` (stage padding, canvas radius/shadow/outline; pointerdown selects the topmost clip under the point, or clears on empty stage; D15, audit 5)
- Modify: `src/roots/EditorRoot.vue` (remove the dead `canvas-pointerdown` wiring if the selection moves into PreviewSurface)
- Test: update `tests/editorPreviewToolbar*.test.ts`, `tests/editorPreviewSurface*.test.ts`; parity block

**Interfaces:**
- Consumes: `MenuPanel`, `EditorIcon`, `editorWorkspace` panel state (Task 4), the theme (D1), the mixer popover (existing, opened from View → Audio mixer…).
- Produces: `data-testid`s `preview-header`, `preview-library-toggle`, `preview-ratio`, `preview-tool-text|arrow|highlight|zoom`, `preview-more-tools`, `preview-review`, `preview-view-menu`, `preview-properties-toggle`.

- [ ] **Step 1: Failing tests.**
  - The header is 48 px (44 at ≤760 tall). Left: the panel toggle (panelLeft icon, `aria-pressed`) and "Preview". Then the ratio button "16:9 ⌄" (mono), which opens FrameDialog.
  - The toolstrip (role toolbar, roving tabindex, arrows/Home/End) has Text/Arrow/Highlight/Zoom with icons, then More tools ⌄ → Spotlight, Numbered step, Privacy cover (disabled reason from the registry), —, Browse all teaching tools (opens the learning center Quick answers on "teaching tools", or omit if no target exists).
  - Right: Review, View ⋯ → Show media library ☑, Show properties ☑, Focus preview, Reset panel layout, —, Light theme ☐, —, Audio mixer…, Keyboard shortcuts & help…; then the properties toggle.
  - Density: <800 hides "Preview" and makes "More tools" read "More"; <520 moves Highlight/Zoom into More and makes Review icon-only. Test via container width.
  - Light theme toggles `data-theme` and persists (existing workspace field).
  - Clicking the stage over c1's rect selects c1; clicking outside every layer clears the selection.
  - Parity writes the composite for screen 02 with the toolstrip centred.
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement §4.1–4.2, §9.11 and §11 (density via ResizeObserver, with a fallback when it's absent in happy-dom).**
- [ ] **Step 4: Run everything.** Expected: PASS (guide: the "preview" and tool lessons).
- [ ] **Step 5: Commit.** `feat(editor): concept preview header, view and tool menus; clicking the picture selects`

---

### Task 12: Transport (§4.3)

**Files:**
- Modify: `src/components/editor/preview/TransportBar.vue`
- Test: `tests/editorTransport*.test.ts` (update), parity block

**Interfaces:**
- Consumes: `PreviewController` peak (existing `readPeak()`), `editorWorkspace` monitor mute/rate/playhead.

- [ ] **Step 1: Failing tests.**
  - The transport is 46 px (40 at ≤760 tall).
  - Left: monitor volume/muted icon button ("Mute monitoring"), a 48×8 peak meter (fill `audio`, `danger` when hot), and a borderless speed select 0.5× / 1× / 1.5× / 2×.
  - Centre: Go to start (skipBack), a 34 px round Play/Pause (accent-bg, accent-ink), Go to end (skipForward), and a mono time `MM:SS.d / MM:SS.d` (current in ink, total muted).
  - Right: the D10 badge "1280 × 720 · 30 fps · PREVIEW" (9 px uppercase muted).
  - Every control has an effect: seek 0, seek end, play toggles, the mute flag toggles, the rate changes.
  - "Audio mixer" and "Sound" leave the transport (the mixer is reachable from View, the Audio tab and the timeline footer, Task 20).
- [ ] **Step 2: Run them.** Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run everything.** Expected: PASS.
- [ ] **Step 5: Commit.** `feat(editor): concept transport`

---

### Task 13: Inspector frame and selection states (§5 heading, card, tabs, none/track/multi)

**Files:**
- Modify: `src/components/editor/inspector/InspectorPanel.vue` (split: `InspectorHeading.vue`, `SelectionCard.vue`, `InspectorTabs.vue`)
- Create: `src/components/editor/inspector/EmptyInspector.vue` (no selection + Project summary + "Review readiness" → Checks dialog)
- Create: `src/components/editor/inspector/TrackInspector.vue` (§5 track state: Name → `renameTrack`, volume → `setTrackFlags`/track volume, Up/Down → `moveTrack`, Remove track… → `deleteTrack`)
- Create: `src/components/editor/inspector/MultiInspector.vue` (§5 multi-clip)
- Modify: `src/stores/editorWorkspace.ts` (if no track selection exists: add `selectedTrackId` view state, cleared by a clip selection; never an edit)
- Test: `tests/editorInspector*.test.ts` (update/add), parity

**Interfaces:**
- Produces:
  - `data-testid`s `inspector-heading`, `inspector-hide`, `inspector-card`, `inspector-tab-clip|layout|fades|audio|speed|color`.
  - Audio clips show Clip / Fades / Audio / Speed only.
  - The track badge click (Task 17) sets `selectedTrackId`.

- [ ] **Step 1: Failing tests.**
  - The heading is 48 px: "Clip properties" / "Properties" / "Track properties" / "Teaching properties" / "Selection properties", with an ✕ that hides the inspector (D5).
  - The card is a 34×34 glyph tile + name 12px/550 + "{track} · {d.d}s".
  - The tabs are a 2×3 grid (3 columns, bg app, border line, radius 8); the active one is `.active`.
  - An audio clip has 4 tabs.
  - Empty: the cursor icon, "Select something to shape it.", and the Project facts "N clips · MM:SS.d" / "W × H · fps"; "Review readiness" opens Checks.
  - Track and multi states per §5, each control mapped to a command, with no-op-free buttons.
  - Parity: the inspector heading is 48; the tab grid has 3 columns.
- [ ] **Step 2–5:** Run them (expected FAIL), implement §5 frame/states, run everything (expected PASS), then commit: `feat(editor): concept inspector frame and selection states`.

---

### Task 14: Inspector Clip and Layout tabs (§5 Clip, Layout)

**Files:**
- Modify: `src/components/editor/inspector/ClipSection.vue`, `LayoutSection.vue` (split `CornerPresets.vue`, `FrameCropDisclosure.vue`, `TransformSource.vue`)
- Test: update both sections' tests; parity for screen 02's inspector

- [ ] **Step 1: Failing tests.**
  - Clip tab, **Placement**:
    - Clip name
    - a Track select (same-kind tracks, "· locked" suffix) → `moveClips` to that track
    - Timeline start (s), in SECONDS with 0.1 step → `moveClips`
    - Earlier / Later / Duplicate → `reorderClip` ×2 / `duplicateClips`
    - help text
  - Clip tab, **Source range**: In (s) / Out (s) in seconds → `trimClip`, with the "Original length MM:SS.d" help.
  - No field shows milliseconds.
  - Layout tab:
    - "Webcam overlay" (a webcam asset) or "Video layout"
    - Full frame / Picture-in-picture → `setLayout`
    - Horizontal (%) / Vertical (%)
    - a Size range 10–100 with a mono value
    - the 2×2 corner presets with the CSS corner glyph
    - the help line
    - `<details>` Frame & crop (shape presets, fitting, crop zoom / focus when fill, mirror, opacity, where the layout model has each field)
    - `<details>` Transform source (rotate / flip / center / fit / fill)
    - A disclosure's open state is remembered per selected clip (view state).
    - Every control → `setLayout`. A field the model lacks is omitted.
- [ ] **Step 2–5:** Run them (FAIL), implement, run everything (PASS), commit: `feat(editor): concept clip and layout tabs, in seconds`.

---

### Task 15: Inspector Fades, Audio, Speed and Color tabs, plus the cue state (§5)

**Files:**
- Modify: `FadesSection.vue` (+ `FadeGraph.vue`), `AudioSection.vue` (+ its children), `SpeedSection.vue`, `ColorSection.vue` (+ `FilterTile.vue`), `EffectSection.vue`
- Test: update each section's tests; parity for screen 04

- [ ] **Step 1: Failing tests.**
  - Fades:
    - h3 "A softer entrance. A cleaner exit." ("Let the sound arrive naturally." for audio)
    - a 58 px SVG fade graph (trapezoid, knee dots) that updates with the values
    - Fade in / Fade out (s)
    - Curve (Linear / Smooth / Equal power for audio)
    - presets None / Quick · 0.5s / Gentle · 1s → `setFades`
    - "Preview entrance" plays from the clip start
    - the "Between two clips" section: Blend 0.5s / 1s → `addTransition` / `setTransitionDuration`, and Remove crossfade → `removeTransition`
  - Audio: a Volume range 0–200 %, Mute this clip, Detach source audio (disabled with a reason when silent), a Mix section with "Open audio mixer", and help.
  - Speed: "Keep the useful pace", the big readout "1×" + timeline/source durations, the Clip speed select, the ripple select, and Preserve voice pitch.
  - Color: 3×2 filter tiles (Original, Clear, Warm, Soft, Mono), "Fine adjustments" sliders with mono % (Brightness, Contrast, Saturation), and Reset color.
  - Cue (EffectSection) per §5 "Teaching cue": the per-kind sections, the colour row (five swatches + custom), timing in seconds, position in %.
  - Every control maps to an existing command.
- [ ] **Step 2–5:** Run them (FAIL), implement, run everything (PASS), commit: `feat(editor): concept fades, audio, speed, color and cue properties`.

---

### Task 16: Timeline toolbar, header row, ruler and markers (§6.2–6.3)

**Files:**
- Modify: `src/components/editor/timeline/TimelineToolbar.vue`, `TimelineRuler.vue`, `TimelineView.vue` (the ruler row with its sticky label cell)
- Create: `src/components/editor/timeline/AddTrackButton.vue` (menu: Add video track / Add audio track → `addTrack`; audit finding 3)
- Modify: `src/editor/actions.ts` / `actionMeta.ts` (give `addTrackVideo` / `addTrackAudio` real resolvers and builders, and remove the "later update" reason)
- Test: update the toolbar and ruler tests; `tests/editorTickStep.test.ts` (new, pure)

**Interfaces:**
- Produces: `tickStep(pps: number): number`, the first of `[.1,.2,.5,1,2,5,10,15,30,60,120,300,600]` with `step*pps >= 70`; and `formatTick(ms, step)` (MM:SS, or MM:SS.d when the step is below 1 s).

- [ ] **Step 1: Failing tests.**
  - `tickStep`:
    - 1600 w at the default zoom gives 2 s
    - 960 w gives 5 s
    - `formatTick(2000,2) === "00:02"`, `formatTick(500,.5) === "00:00.5"`
  - Toolbar (44 px), in order:
    - "Timeline"
    - undo / redo icon buttons with their labels as tooltips
    - a divider
    - Split (scissors + label)
    - trash
    - Delete mode select "Delete: leave gap / Delete: close gap"
    - bookmark (Add chapter marker, M)
    - Edit actions (more icon)
    - a divider
    - Snap (magnet, active = accent)
    - zoom out / a 66 px range / zoom in / Fit
  - Every enabled control has an effect.
  - Header row (32 px, sticky top): the label cell with "Add track" (menu) and "Layers ↓".
  - Ruler: mono 9 px ticks with a 7 px line; gold ◆ markers (24 px hit area) that seek on click.
  - Parity: toolbar 44, ruler 32, label cell 196.
- [ ] **Step 2–5:** Run them (FAIL), implement §6.2–6.3, run everything (PASS), commit: `feat(editor): concept timeline toolbar, ruler and add-track menu`.

---

### Task 17: Track rows with a sticky label column (§6.4, D12)

**Files:**
- Modify: `src/components/editor/timeline/TrackLane.vue`, `TrackHeader.vue`
- Test: `tests/e2e/editorParity.spec.ts` (the scroll-bug regression and extreme content), `tests/editorTrackHeader*.test.ts`

- [ ] **Step 1: Failing tests.**
  - **Scroll regression (e2e):** at 1600×1000 with the parity project, select `clip-c3` and press ArrowRight three times (nudge; `ClipItem`'s `.focus()` scrolls). Then, for every track header, assert `box(track-lane-header-<id>).x` equals the timeline's left edge ±1 (the column stays pinned). It fails today.
  - Header anatomy (196 px, 174 compact):
    - a 28×28 mono badge V3/V2/V1/A1 (video badges numbered top-down descending)
    - the title (11px/550, ellipsis)
    - the controls row, with eye/eyeOff (video only), M, S and lock/unlock at 26×26
    - the active state uses `gold-bg` / `gold` (visibility is "active" when hidden)
  - The ⋮ button is removed. Right-click on the header, or Shift+F10 on the focused header, opens `trackMenu` (Task 5).
  - The badge click selects the track (Task 13 inspector).
  - Rows: min-height 58 (the real height ≈68), lane grid lines at the tick step, a locked hatch, the empty-lane text "Drop video here · or add from Media", and a drop highlight.
  - Extreme content: 20 tracks scroll vertically with the ruler sticky, and a 120-character track name ellipsizes.
- [ ] **Step 2–5:** Run them (FAIL), implement (`position: sticky; left: 0` for the label cells, in the one scroll container), run everything (PASS; also the compact label 174 test from Task 4, un-`fixme` it), commit: `fix(editor): pin the track label column; concept track headers`.

---

### Task 18: Clip anatomy, gap hints, playhead, snap guide (§6.5)

**Files:**
- Modify: `src/components/editor/timeline/ClipItem.vue` (split `ClipFadeShape.vue`, `ClipBadges.vue` to stay under the cap; it is at 424 LOC), `ClipThumbnail.vue` (→ repeating filmstrip), `ClipWaveform.vue` (→ bars), `TimelineView.vue` (playhead head, snap guide, gap hints)
- Test: update the clip tests; parity for screen 04

- [ ] **Step 1: Failing tests.**
  - The clip body is 47 px at top 5, radius 5, with the border/bg per kind.
  - The filmstrip is a repeating thumbnail at 32 % opacity (`editor_media_thumbnail` once per clip).
  - The name sits bottom-left with a 10 px glyph.
  - The duration pill "9.5s" appears top-right when the clip is wider than 140 px.
  - A speed badge ("2×") shows when the speed isn't 1.
  - The fade SVG is gold triangles plus a diagonal line.
  - The fade handles are 13 px gold circles on the TOP edge; the trim handles are 9×33 with an ink bar. Both are visible on hover/selected and keep `vb-handle`.
  - The selected clip has a 2 px accent outline.
  - An audio clip has its name at the top and a bar waveform (≤130 bars), with the no-peaks text "waveform unavailable · audio still plays".
  - A gap wider than 70 px shows a dashed "Close gap" button (opacity .16 → 1 on lane hover), which closes that gap.
  - The playhead is 1px accent with the pentagon head and keeps `vb-playhead`.
  - Dragging shows a dashed gold snap guide.
  - Extreme content: a 3 px clip renders without badges or name overflow, and its trim is still reachable via the keyboard nudge.
- [ ] **Step 2–5:** Run them (FAIL), implement, run everything (PASS, including forced-colors CSS still targeting `vb-handle`/`vb-playhead`), commit: `feat(editor): concept clip anatomy, gap hints and playhead`.

---

### Task 19: Teaching layers row and Captions row (§6.4, D11)

**Files:**
- Create: `src/components/editor/timeline/TeachingLayersRow.vue`, `CueChip.vue`, `CaptionsRow.vue`
- Create: `src/editor/cueLanes.ts` (pure packing: `packCues(effects): { id: string; lane: number }[]`, greedy by start then id; a cue goes to the first lane whose last end ≤ its start)
- Modify: `TimelineView.vue` (insert the rows: Teaching layers above the video tracks, Captions after the ruler when captions exist)
- Test: `tests/editorCueLanes.test.ts`, `tests/editorTeachingLayers.test.ts`, parity (screen 02 lane)

**Interfaces:**
- Consumes: `updateEffect` (move/trim), `removeEffect`, `cueMenu` (Task 5), the inspector cue state (Task 15), and the caption selection reveal (Task 10).

- [ ] **Step 1: Failing tests.**
  - `packCues`: two overlapping cues get lanes 0 and 1; a third that starts after the first ends gets lane 0.
  - The row height is `max(44, lanes*22+5)`.
  - Cues are 19 px, top `lane*22+3`, labelled with their text or kind; zoom cues are gold and read "1.65× Focus".
  - Click selects the cue (→ the inspector cue state). Drag moves it via ONE `updateEffect` on pointer-up (Escape cancels); edge grips trim; Shift+F10 opens the cue menu.
  - The badge is the text icon on accent-bg, with the title "Teaching layers" and the sub "Attached to video".
  - Captions row: 35 px, a gold captions label "Captions N", and 24 px gold cues; click opens the Captions tab on that cue.
  - Parity: the teaching row exists above V3, is 49 ±2 tall for the parity project, and holds 6 cues.
- [ ] **Step 2–5:** Run them (FAIL), implement, run everything (PASS), commit: `feat(editor): teaching layers and captions rows on the timeline`.

---

### Task 20: Timeline footer and status-bar content (§7)

**Files:**
- Create: `src/components/editor/timeline/TimelineFooter.vue` (27 px: link icon + live edit hint left; "Audio mixer" + audio-track-count pill right → opens the mixer)
- Modify: `EditorStatusBar.vue` (Task 4) if its copy needs the final D10 wording; `TimelineView.vue` (mount the footer; feed the hint from drag state: "Moving N clips together · tracks stay fixed · Esc cancels", default "Callouts follow their clip.")
- Test: `tests/editorTimelineFooter.test.ts`, parity (footer 27, status 25)

- [ ] **Steps 1–5:** failing tests (the footer content, the hint changing during a drag, the mixer opening, the status-bar clicks → Save / Project section), run (FAIL), implement, run everything (PASS), commit: `feat(editor): concept timeline footer and status bar`.

---

### Task 21: Checks, Save-a-copy, Render and Publish dialogs (§9.4–9.6)

**Files:**
- Modify: `dialogs/ChecksDialog.vue` (+ `ChecksFindingList.vue`, `ChecksFindingRow.vue`, `ChecksDestination.vue`), `SaveProjectDialog.vue`, `RenderDialog.vue` (+ `RenderSettingsForm.vue`, `RenderOutcome.vue`), `PublishDialog.vue` (+ `PublishForm.vue`, `DestinationFields.vue`)
- Test: update each dialog's tests; parity composites `vs-07-checks`, `vs-08-save-project`, `vs-09-render`

- [ ] **Step 1: Failing tests.**
  - Checks (680 wide): the summary box; issue rows with NOTE / REVIEW / FIX mono chips, h3, text and an action button with a chevron (each action reveals, as today); the closing help; a footer with Export diagnostics · Back to edit · Continue to render (primary).
  - Save a copy (screen 08):
    - the intro tile + h3 "Your workspace. Ready to continue." + text
    - the Project name field
    - two `.save-option` radio cards (Portable project + "Recommended" pill / Lightweight project file) with the D10 native texts
    - Include rendered videos
    - the 2×2 teal checklist
    - footer Keep editing · Save copy (primary)
  - Render (screen 09, native):
    - the review range box (checkbox + From/To seconds)
    - the render-parent card ("PROJECT · r{rev}" pill, title, "This render becomes a product. The project stays editable.")
    - Rendered video name
    - native quality radios styled as the profile card
    - the accent callout "A new output, never an overwrite."
    - Checks summary with "Review all checks"
    - footer "Save project instead" (runs Save) · Render video (primary)
    - the vault NAME, not the id, in the completion/publish line
  - Publish: the vault select shows names; same chrome.
- [ ] **Step 2–5:** Run them (FAIL), implement, run everything (PASS), commit: `feat(editor): concept checks, save-a-copy, render and publish dialogs`.

---

### Task 22: Webcam, mixer, session and small dialogs (§9.7–9.10)

**Files:**
- Modify: `dialogs/WebcamDialog.vue` (+ `WebcamLive.vue`, `WebcamControls.vue`, `WebcamReview.vue`, `WebcamCloseConfirm.vue`), `shell/MixerPopover.vue` (+ rows → the §9.8 mixer look, as a dialog or popover; keep the existing open paths), `dialogs/RecoveryDialog.vue`, `CloseGuardDialog.vue`, `DiscardProjectDialog.vue`, `ReconnectDialog.vue` (+ `ReconnectRow.vue`), `ReviewDialog.vue`
- Test: update the tests; parity composite `vs-05-webcam` (open the Webcam dialog; the camera stays off)

- [ ] **Steps 1–5:**
  - Failing tests:
    - Webcam: 960 wide; left the 16:9 view with the "Camera is off" empty state and a status row; right "Set up your take" settings; a privacy strip; footer per phase (idle: Enable camera; no demo overlay, D10).
    - Mixer rows `125px 1fr 52px`: name + M/S (gold active) + range + mono dB; Master output.
    - Recovery / close guard / discard / reconnect use the chrome and keep their behaviour.
  - Run them (FAIL), implement, run everything (PASS).
  - Commit: `feat(editor): concept webcam, mixer and session dialogs`.

---

### Task 23: Guide: invitation, docked coach, target label, learning center (§9.1–9.3)

**Files:**
- Modify: `guide/GuideInvitation.vue`, `guide/GuideCoach.vue`, `guide/GuideCoachCard.vue`, `guide/LearningCenter.vue` (+ `LearningWalkthrough.vue`, `LearningAnswers.vue`, `LearningShortcuts.vue`, `LearningPreferences.vue`)
- Modify: `src/editor/guide/position.ts` (placement per §9.2)
- Test: `tests/editorGuidePosition.test.ts` (update), `tests/e2e/editorGuide.spec.ts` (docking assertions), parity composites `vs-10-onboarding`, `vs-11-learning-center`

- [ ] **Step 1: Failing tests.**
  - Placement (pure):
    - card width `min(362, vw−24)`
    - candidates right (gap 18, vertically centred, clamped 12) → left → below → above
    - the first that fits fully inside wins, preferring no overlap with an open menu
    - otherwise the larger free band ≥240 with scroll ("compact-scroll"), else pinned
    - no target: top-right `(vw−w−12, min(95,…))`
    - a unit test per branch
  - The ring is 2px guide-edge at target −4/+8. The target label chip sits above the target (below near the top) and is hidden under 700 px wide.
  - The invitation (screen 01): 350 wide at right 24 / top 76, the overline, the compass tile, the h2, the body, "Show me around ›" / "Not now", and the footer. It doesn't steal focus.
  - Coach card anatomy per §9.2 (chapter title, meta row + mono step counter, h2 23px, task box variants, Pause guide / Back / Next, foot, 3 px progress).
  - Learning center (870 wide): hero with the progress ring, underlined tabs, a chapter card grid (last card spanning), the safety box, the shortcut rows, a footer `<details>`.
  - e2e at 1600×1000: on the Fades lesson the card docks LEFT of the inspector (it doesn't cover it), and the ring surrounds the Fades section. The 960×640 run still resolves all 22 lessons.
- [ ] **Step 2–5:** Run them (FAIL), implement, run everything (PASS), commit: `feat(editor): concept guide invitation, docked coach and learning center`.

---

### Task 24: The no-op sweep gate

**Files:**
- Create: `tests/e2e/editorNoop.spec.ts` (adapted from the audit's throwaway `.superpowers/research/visual-parity/noop.spec.ts`, which is on disk and git-ignored; read it first)
- Modify: `tests/e2e/tauriStub.ts` (an optional `refuse: string[]` option: those commands reject with an `EditorError`-shaped value)

**Interfaces:**
- Consumes: the Task 1 harness.

- [ ] **Step 1: The sweep.**
  - Scenarios: base; clip selected; each inspector tab for a video, an audio and a webcam clip; each library tab and the Project section; every menu (Project, View, More tools, Add track, Help, Edit actions, clip / multi / track / lane / cue / asset context menus, each submenu); every dialog (Checks, Save a copy, Render, Publish, Webcam, Mixer, Learning center, Frame, Rename, Open project, Discard); compact 960×640.
  - For each visible enabled `button`, `[role=menuitem]`, `[role=menuitemcheckbox]`, `[role=tab]`, `select` (pick another option) and `input[type=checkbox]` (read the `checked` property):
    - reload the scenario
    - click
    - assert at least one of: new `window.__calls` entries (excluding debounced `editor_save_workspace`); net DOM mutations above the idle baseline; a focus move to another element; a change in the count of open dialogs/menus; a checkbox `checked` change
  - Skip a re-click of an already-active tab, radio or preset, identified by `aria-selected="true"`, `aria-checked="true"` or `aria-pressed="true"`.
  - The failure message names the scenario, the control's accessible name and its test id.
- [ ] **Step 2: Refusal visibility.** With `refuse: ["editor_execute"]`, clicking Titles → Intro, clip menu → Duplicate, and an inspector field commit each produce a visible toast whose text contains no `<path:#` / `<name:#`.
- [ ] **Step 3: Run it.** `npm run build && npx playwright test tests/e2e/editorNoop.spec.ts`. Expected: PASS. Any failure is a real no-op: fix it in its component (rule on it in the ledger if it needs a design choice), never in the test.
- [ ] **Step 4: Commit.** `test(editor): sweep every control for a no-op`

---

### Task 25: Final parity pass, contrast and docs

**Files:**
- Modify: `tests/e2e/editorParity.spec.ts` (ensure a composite exists for every concept screen 01–12 with a built counterpart; add the light-theme screenshot of screen 02 for review)
- Modify: `AGENTS.md`:
  - the editor window bullet (dark default)
  - the "UI primitives & design tokens" editor paragraph (the new tokens, `EditorIcon`, `.vb-editor`)
  - the tutorial editor domain (MenuPanel, Teaching layers row, sticky labels, the no-op rule)
  - the Testing conventions (the two new e2e gates)
  - the repository map (new components)
- Modify: `docs/Gaps.md` (close the scroll bug if an entry exists; file anything left open with its reason)
- Modify: `docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md` (add rows T74+ for the visual pass on real WebView2: the dark default, drawers at 1080/860, the sticky labels, the context menus with submenus, the docked coach, a Windows contrast theme on the new chrome; re-measure the count with the file's own one-liner, never increment)
- Modify: `docs/superpowers/specs/2026-09-21-tutorial-editor-acceptance-evidence.md` (update any test names that moved; `tests/editorEvidence.test.ts` must stay green)

- [ ] **Step 1:** Run the full gates list, and `npx playwright test tests/e2e/editorParity.spec.ts tests/e2e/editorKeyboard.spec.ts` in dark and light.
- [ ] **Step 2:** Update the docs as listed. Re-measure counts on the tree, never increment.
- [ ] **Step 3:** Commit: `docs: record the editor's visual parity, its gates and the new checklist rows`
