# Tutorial editor — visual parity with the concept, and no no-op controls

Date: 2026-09-26 · Branch `claude/editor-visual-parity` (from PR 79's head `0b9da762`).

## Why

The tutorial editor shipped every F-ID of the concept bundle
(`docs/concepts/vault-buddy-editor/`) as behaviour, but none of its visual
and interaction design: no icons, floating rounded cards instead of a dense
edge-to-edge workspace, text-link toolbars, a flat context menu, no Teaching
layers lane, raw milliseconds in the inspector, a timeline squeezed to about
200 px. No task ever ported `reference/shell.html` / `editor.css` /
`workspace-ui.css`, and no gate ever compared the app with `screens/*.png`.
The user's verdict: "the editor is not even close to the concept". A no-op
audit found enabled controls that do nothing observable and refusals nobody
sees.

## Authority

1. **The concept screenshots** `docs/concepts/vault-buddy-editor/screens/01–12`
   are the visual truth (1600×1000; `12-compact` at 960×640).
2. **The port spec** `2026-09-26-tutorial-editor-visual-parity-concept-spec.md`
   (this folder) gives every value — tokens, grid, sizes, anatomy, menu item
   lists — with a grep-able citation into the bundle. Section numbers below
   (§N.M) refer to it.
3. **The icons** `2026-09-26-tutorial-editor-visual-parity-icons.json`: the
   concept's 74 inline SVGs, verbatim.
4. **The no-op audit** `2026-09-26-tutorial-editor-noop-audit.md`: every
   control's verdict and nine prioritized findings.
5. **The built inventory** `2026-09-26-tutorial-editor-built-inventory.md`:
   which of the 92 current components renders which region, and which tests
   couple to markup.
6. Where this document decides something the concept does not (a native
   adaptation), this document wins; everything else follows the concept.

The editor's behaviour, IPC, Rust side and store contracts are NOT redesigned.
This is a presentation and interaction port plus the no-op fixes. No Rust
change is expected; a task that finds it needs one rules on it in the ledger.

## Decisions

- **D1 Dark by default.** The concept is dark-first (`shell.html:
  data-theme="dark"`); light is the View menu's "Light theme" item. The
  editor stops seeding its theme from `prefers-color-scheme`: a project with
  no saved `theme` opens dark. A saved `theme` still wins (Task 18's
  persistence is unchanged). The header's sun/moon emoji toggle goes away.
- **D2 Concept palette and type.** The editor tokens take the concept's values
  (§1.1) in both themes; body type is Segoe UI 12px/1.5 with the §1.2 ladder;
  mono tabular figures for every timecode. The existing token NAMES stay (so
  `bg-panel`, `text-video` … keep working); values change, and missing roles
  are added (`--color-hover` already exists; add `ring`, `accent-ink`,
  `accent-bg`, `primary`, `danger`, `guide-edge`, `guide-dim` equivalents as
  needed). Both contrast e2e checks (4.5:1 text, 3:1 UI boundaries, both
  themes) must stay green — the concept's values are expected to pass; a pair
  that fails is adjusted minimally and ledgered.
- **D3 One icon system.** `src/components/editor/EditorIcon.vue` renders the
  concept's SVGs from one typed module generated from the icons JSON
  (`<svg viewBox="0 0 24 24" aria-hidden="true">` + stroke `currentColor`
  1.7, round caps/joins, size prop, default 17). No icon dependency. Every
  emoji and HTML-entity glyph in the editor is replaced (the brand mark, `◆`
  markers, `◇` crossfade chip, "M"/"S" track text and "Layers ↓" stay text,
  as in the concept).
- **D4 Frame.** Rows `56 / minmax(170px,1fr) / 8 / var(--timeline) / 25`,
  columns `244 / minmax(0,1fr) / 276`, 1px `line` separators, no gutters, no
  rounded region cards (§1.3, §1.5). Breakpoints per §1.4: ≤1350 hides the
  wordmark and save text; ≤1080 the inspector becomes an overlay drawer;
  ≤860 the library becomes a drawer; ≤760 tall shrinks header/preview
  header/transport. Timeline height default 400, clamped `170 … min(540,
  innerHeight − 370)`, keyboard ±25, persisted as today.
- **D5 Panel toggles work at every width** (audit finding 2). "Show media
  library", "Show properties" and "Focus preview" collapse the grid column(s)
  to 0 at full width; below the drawer breakpoints they open/close the
  drawer. No toggle is ever a silent flag.
- **D6 Header** (§2): brand mark (CSS, `··` + ears), **Project** menu,
  document title with hover pencil (click = rename dialog), save state
  (dot + text), Help, Checks (+count chip), **Save project** (bordered),
  **Render video** (primary). The destination vault id leaves the header (the
  concept shows none); the Render and Publish dialogs show the vault NAME
  (`list_vaults` lookup). The Save ▾ split button is removed; its items move
  into the Project menu.
- **D7 Project menu, native items** (replacing §2's browser list):
  folder **Open project…** (a picker over `editor_list_projects` →
  `editorProject.openProject`, the current project excluded; empty list →
  the item is omitted), upload **Open a project file…**
  (`editor_import_package`), edit **Rename tutorial…**, —, layers
  **Workspace & rendered products** (library Project section, D9), save
  **Save a copy as project file…** (the Save project dialog, screen 08 look),
  —, trash **Discard project…** (danger). "New project…" and "Restore sample
  project…" have no native equivalent and are omitted — never shown disabled.
- **D8 Save project** keeps its native meaning: Ctrl+S / the header button
  commits the project to the store (no dialog). The screen-08 dialog is the
  "Save a copy as project file…" export (portable/lightweight), restyled.
- **D9 Products move out of the library tabs.** Tabs are Media / Titles /
  Captions / Chapters (§3.1). Products live in the library's Project section
  ("YOUR WORKSPACE", §3.6), reached from the Project menu, from the Render
  dialog's completion and from the status bar. The guide lesson that targets
  products must still resolve (its target moves with the component).
- **D10 Browser-only copy is replaced, slots kept** (§ BROWSER-ONLY):
  save state "Unsaved changes" / "Saved" / "Saving…" / "Save failed" (the
  reason in its tooltip and in the toast, audit finding 7); status bar left
  shield "Local only. No media is uploaded.", centre the recovery state
  ("Unsaved changes are journaled for recovery" when dirty, "All changes
  saved" when clean — clickable → Save project), right
  "{n} rendered video(s) · Workspace & rendered products" (clickable → the
  Project section); transport badge "{W} × {H} · {fps} fps · PREVIEW";
  Render dialog has no "Browser review" profile card (native quality radios
  in its place); Checks has no "not downloaded" note; the webcam dialog has no
  "Try demo overlay"; coach/learning-center storage text uses the native
  "Progress remembered on this PC" / "Session only".
- **D11 Timeline gains the Teaching layers row and the Captions row** (§6.4):
  every project effect renders as a cue in the Teaching layers row (packed
  rows 22 px apart), selectable (→ the inspector's cue state), movable and
  edge-trimmable through `updateEffect`, with the cue context menu. Captions
  render in a 35 px Captions row when any exist; clicking one opens the
  Captions tab on that cue.
- **D12 The track label column is sticky** (`position: sticky; left: 0`) in
  the one scroll container — the ruler row's label cell too — which fixes the
  header-column-scrolls-away bug (inventory §5: `ClipItem`'s `.focus()` after
  a nudge).
- **D13 Context menus** use one `MenuPanel` (§8: 282 px, heading + mono
  subtitle, 31 px icon items, kbd or chevron, separators, submenus on 180 ms
  hover / → / Enter, danger styling, a live hint footer showing the focused
  item's disabled reason). Item sets per §8 — clip, multi-clip, track
  header, empty lane/gap, teaching cue, media asset — each item mapped to an
  existing action/command/dialog. An item with no native backend is OMITTED.
  The same `MenuPanel` renders the Project, View, More tools, Help and
  Add-track menus.
- **D14 No control is a no-op.** Every enabled control has an observable
  effect; every refused command is visible; every disabled control carries a
  reason reachable by pointer (`title`) AND keyboard (the menu hint line, or
  a toast when a disabled shortcut is pressed). The audit's nine findings
  are fixed as specified in Task 7. Controls with no backend are removed, not
  left disabled "until a later update" (the two registry `addTrack*` actions
  become real — the backend command exists).
- **D15 Clicking the preview picture selects** the topmost visible clip under
  the pointer (audit finding 5); clicking empty stage clears the selection.
- **D16 Accessibility floors stay**: every control keyboard-reachable,
  Escape restores focus to the opener, roving tabindex in toolstrips,
  forced-colors rules keep the playhead/handles/selection visible (`vb-playhead`
  / `vb-handle` hooks), reduced motion disables transitions.

## Gates added by this work

- **Parity e2e** `tests/e2e/editorParity.spec.ts` over a populated sample
  project (`tests/e2e/fixtures/parityProject.ts`): at 1600×1000 it measures
  each region against the table below (±1 px unless noted), and writes a
  side-by-side composite (concept screenshot | built screenshot) per screen
  to `test-results/parity/` for human review. At 960×640 it checks the
  compact frame.
- **No-op sweep e2e** `tests/e2e/editorNoop.spec.ts`: every visible enabled
  `button` / `menuitem` / `tab` / `option` / `select` in every reachable menu
  and dialog must produce an IPC call, a DOM change, a focus move or an
  opened/closed menu or dialog; a stubbed refusal must produce a visible
  toast.

| Region (1600×1000) | Measure | Value |
|---|---|---|
| header | height | 56 |
| library | width | 244 |
| inspector | width | 276 |
| library tabs / preview header / inspector heading | height | 48 |
| transport | height | 46 |
| splitter | height | 8 |
| timeline section | height | 400 (±2) |
| timeline toolbar | height | 44 |
| ruler row | height | 32 |
| track label column | width | 196 |
| video track row | height | 68 (±3) |
| clip body | height | 47 |
| timeline footer | height | 27 |
| status bar | height | 25 |
| Import media / Webcam buttons | height | 40 |
| media thumbnail | size | 58×40 |
| context menu | width | 282 |
| compact (960×640): header / preview header / transport / timeline / status | height | 52 / 44 / 40 / 270 (±2) / 23 |
| compact: label column | width | 174 |

## Out of scope

- Any Rust/IPC change (a task that needs one rules on it; none is expected).
- Features the concept has and the native app does not (sample project,
  demo overlay, "Download annotated frame", title-template still-image import
  if no backend) — omitted, never stubbed.
- Pixel-identical rendering: sample media differs; parity is the frame,
  anatomy, sizes, icons, colours and interactions.
