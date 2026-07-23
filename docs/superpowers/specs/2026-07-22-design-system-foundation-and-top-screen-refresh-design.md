# Design-system foundation + top-screen consolidation refresh — design

Date: 2026-07-22
Status: accepted (user request: after a full product review, "let's focus on
improving ux/ui of the app instead" of adding new capability; then chose,
from the review's options, a **design-system foundation + a consolidation
refresh of the top screens** — keep the dark-glass identity, make it
consistent and polished; not light mode, not an IA/nav overhaul, not a bolder
identity evolution)

## Problem

Vault Buddy is at v0.7.1 with deep, healthy feature coverage (the Gaps
backlog is 29 Low, 0 Medium, 0 High). After many feature increments the
**UI has accreted, not been designed**: the dark "glass" look is consistent
*enough* but held together by copy-pasted utility strings, with no token
layer and no shared component primitives. Measured against the current
`src/components` tree:

- **Zero shared UI primitives.** Every one of ~30 components rolls its own
  buttons, chips, badges, inputs, and status dots. There is no `Button`,
  `IconButton`, `Chip`, `Badge`, `Banner`, `Field`, `Avatar`, or
  `SectionHeader`. (`AppIcon` and `SelectMenu` are the only shared UI atoms.)
- **The duplication is quantified:**
  - the violet focus-ring string (`focus-visible:ring-violet-400`) appears in
    **25 files, 64 times**;
  - the icon-button hover pattern (`hover:bg-white/10`) appears **59 times**;
  - `ActionPanel.vue` alone copy-pastes the header icon-button class string
    **4 times**; `VaultList.vue` repeats its row-action button string **5
    times**; `TaskRow.vue` twice — with subtle drift between them (`p-1` vs
    `p-1.5`, `text-slate-400` vs `text-slate-300`).
- **A de-facto palette exists but is untokenized**, so it drifts: secondary
  text is *sometimes* `text-slate-400` (83×) and *sometimes* `text-slate-500`
  (78×); error text is `text-red-200` / `text-red-300` / `text-rose-400`
  interchangeably. The letter-avatar (`bg-violet-600/80`) appears in both
  `VaultList` (7×7, `rounded-lg`) and `TaskRow` (4×4, `rounded`) — the same
  idea diverging.
- **A fragmented type scale:** `text-sm`, `text-xs`, `text-[10px]`, and
  `text-[9px]` all coexist for captions/chips.
- **No motion.** `<Transition>` appears **zero times** — every view swap is an
  instant cut. (Reduced-motion is already respected globally in `style.css`.)

None of this is a bug, but it is the visible "not-quite-designed" feeling, it
slows and de-risks every future UI change, and it is a prerequisite for the
richer UX themes (density/IA, theming, motion) the review surfaced. This
increment builds the missing foundation and applies it — with a
consistency-and-polish pass — to the two highest-traffic screens.

## Goals & scope decisions

- **Foundation:** a semantic **token layer** (Tailwind 4 `@theme`) and a
  small set of **shared primitives** under `src/components/ui/`.
- **Apply it** to the shell (`ActionPanel`), the home (`VaultList`), and the
  **tasks view** components, with a consolidation refresh (spacing/hierarchy,
  consistent controls, proper empty states).
- **Identity preserved.** Every token maps to the value already in use, so
  **unconverted components render byte-identical** — there is no half-migrated
  broken state. The payoff is dedup + consistency where we convert and a
  reusable vocabulary everywhere.
- **Behavior-preserving.** Every `data-testid`, `aria-label`, prop, and emit
  is kept unchanged, so the existing Vitest suite is the regression net.
- **Density stays light-touch.** The list view's up-to-six stacked banners
  and VaultList's five-per-row action buttons are real density issues, but
  restructuring them is the *"declutter the panel"* theme the user did **not**
  pick. We tighten spacing and standardize controls; we do **not** rework
  navigation, collapse the banner stack into a new status component, or add a
  row overflow menu. Density opportunities are *noted* in Gaps, not built.

### Non-goals (this increment)

- Light mode / theming toggle (the token layer is a prerequisite for it, but
  building it is out).
- IA / navigation changes; consolidating the banner stack; row overflow menus.
- New product features (no Task Tags/Todos — explicitly deferred earlier in
  the same conversation).
- A motion system. (A single, tasteful cross-view fade MAY be added if it is
  cheap and reduced-motion-safe — see §5 — but it is optional and not the
  point of the increment.)
- Migrating the other ~18 components (settings tabs, `Recordings`,
  `Transcriptions`, `McpSettings`, `Search`, import views, `UpdateView`).
  They keep working unchanged and migrate in a documented follow-up.

## Design

All new UI atoms live in `src/components/ui/`. All existing behavior,
containers, stores, and IPC are untouched — this is a **presentational**
refactor plus a token stylesheet. Nothing in `src-tauri` changes.

### 1. Token layer (`src/style.css`, Tailwind 4 `@theme`)

Tailwind 4 is configured in CSS (there is no `tailwind.config.*`). Add an
`@theme` block that defines semantic custom properties; Tailwind generates
matching utilities (`bg-fill`, `text-muted`, `ring-accent`, `rounded-control`,
…). Each token is set to the current de-facto value, and the mapping is where
**drift is resolved into a rule**:

| Category | Token → current value | Notes |
| --- | --- | --- |
| Surface | `panel` = `slate-900/90`; `fill` = `white/5`; `fill-hover` = `white/10`; `border` = `white/10` | the glass surfaces |
| Text | `text` = slate-100 (primary); `text-secondary` = slate-300; `text-muted` = slate-400; `text-subtle` = slate-500 | **rule:** `muted` = captions/values, `subtle` = section labels / placeholders / disabled hints. Collapses the 400-vs-500 drift |
| Accent | `accent` = violet-500; `accent-strong` = violet-600; `accent-text` = violet-200; `ring` = violet-400 | |
| Status | `success` = emerald-400; `danger` / `danger-muted` (collapses red-200/red-300/rose-400); `warning` = amber; `recording` = red-500 | |
| Radius | `control` = lg; `panel` = 2xl; pill = full | |
| Type | `body` = 0.875rem (sm); `caption` = 0.75rem (xs); `micro` = 0.625rem (10px) | the stray `[9px]` folds into `micro` |

The token names are semantic (`text-muted`, not `text-slate-400`) so a future
theming increment can reassign them without touching call sites. Exact hex/
opacity values are pinned in the implementation plan; the constraint here is
**they equal what renders today** (regression-tested for the converted
screens).

### 2. Primitives (`src/components/ui/`)

The set is chosen by measured duplication; each entry names what it collapses.

| Primitive | API sketch | Replaces / kills |
| --- | --- | --- |
| `IconButton.vue` | `label` (required aria), `title?`, `size?` (`sm`/`md`), `variant?` (`ghost` default, `danger`), `disabled?`; default slot = icon; emits `click` | the 59× icon-button pattern + 64× focus-ring; header, row edit/archive, vault actions |
| `AppButton.vue` | `variant` (`primary`/`secondary`/`ghost`/`danger`), `size?`, `disabled?`; slot = label; emits `click` | filled/outline text buttons (composer submit, settings actions) |
| `Chip.vue` | `variant` (`neutral`/`accent`/`interactive`), `label`/slot; `interactive` emits `click` | tag chips, filter chips |
| `CountBadge.vue` | `count: number`, `max?` (default 99 → `99+`) | the corner count badge (VaultList tasks button + ActionPanel all-tasks) |
| `StatusDot.vue` | `tone` (`success`/`recording`/`transcribing`/`priority-high`/`priority-low`/`muted`), `pulse?`, `title?` | the 1.5px dots in VaultList + TaskRow |
| `Avatar.vue` | `name: string`, `size?` (`sm`/`md`); renders the uppercase initial | the letter avatar in VaultList + TaskRow |
| `SectionHeader.vue` | slot = label | the uppercase tracking-wider group labels (VaultList groups, settings) |
| `Banner.vue` | `tone` (`danger`/`warning`/`info`/`success`), slot | the alert strips (error banner, warnings) |
| `Field.vue` (text input) | `v-model`, `type?`, `placeholder?`, `ariaLabel`; passes through `keydown` etc. | the input pattern (filter, search, composer) |

Icons standardize on `AppIcon` — inline `<svg>` blocks in VaultList/TaskRow
and the text-glyph star (`★`/`☆`) are replaced with `AppIcon` paths so the
icon language is uniform. `Card.vue` / `SurfaceSection.vue` is **optional**,
added only if a refreshed screen's sections need it (settings screens, which
would be its main consumer, are out of scope this increment). `SelectMenu`
already exists and is reused as-is.

Each primitive is a thin, presentational, fully-typed SFC with a focused unit
test. They own the focus ring, hover, and disabled treatments so those can
never drift again.

### 3. Screen refresh — VaultList (home)

Behavior-preserving refactor + polish:

- The five row actions (favorite, daily-note, tasks, capture, capture-
  settings) rebuild on `IconButton` — uniform hit target, hover, focus.
- Status dots → `StatusDot` (`success` open, `recording`, `transcribing`);
  the initial → `Avatar`; the per-vault task count → `CountBadge`; group
  labels → `SectionHeader`.
- Visual hierarchy: the primary **open** affordance reads as primary; the
  four secondary icons are consistently sized and de-emphasized (`text-muted`)
  so the row scans cleanly. (No overflow menu — that is the declutter theme.)
- The bare `<p>` empty/degraded states ("Obsidian not found…", "No vaults
  match…") become a small, consistent empty-state block (icon + message).
- Favorites/open/other grouping logic, ambiguity disambiguation, and all
  emits/test-ids are unchanged.

### 4. Screen refresh — Tasks view

Scope = the tasks *view* components only: `Tasks.vue`, `TaskRow.vue`,
`TaskComposer.vue`, `TaskViewControls.vue`, `TaskEditor.vue`,
`TaskSectionMenu.vue`, `TaskListPicker.vue`, `TaskDragHandle.vue`. (The
`Task*Settings` / `TasksConfigTab` components render inside the Vault-settings
view, not the tasks screen, and are **out of scope**.)

- `TaskRow`: edit/archive → `IconButton`; tag chips → `Chip` (`interactive`);
  due chip → `Chip`/`caption` token; priority + vault marker → `StatusDot` /
  `Avatar`; checkbox accent tokenized. The nested-interactive-content
  structure (open button as sibling of chips, per the PR #46 comment) is
  preserved.
- `TaskViewControls`: the `Lists | Dates | Tags` grouping control and the
  sort `SelectMenu` get standardized spacing and token colors; no logic
  change. (A `SegmentedControl.vue` primitive MAY be extracted for the
  grouping toggle if it reads cleaner than inline buttons — optional.)
- `TaskComposer` / `TaskEditor`: inputs → `Field`; action buttons →
  `AppButton` / `IconButton`; the `TaskListPicker` inline-create flow keeps
  its behavior.
- Section headers → `SectionHeader`; empty/"no tasks"/"no match" states get
  the same empty-state treatment as VaultList.
- The `text-[9px]`/`text-[10px]` fragments in these components collapse to the
  `micro` token.

### 5. Shell (`ActionPanel`) touch-ups

- The header's four hand-rolled icon-buttons (all-tasks, search, settings,
  back) → `IconButton`; the count pill → `Chip`/`CountBadge`; the save-status
  text → tokens.
- **Optional, reduced-motion-safe:** a single lightweight cross-view
  `<Transition>` (opacity-only, ~120ms) around the view `<div>` swap, so
  navigation feels less abrupt. Gated by the global reduced-motion rule
  already in `style.css`. This is the one small motion concession; it is
  cut if it complicates the existing `panel-scroll`/focus behavior at all.
- The banner stack (`RecordingBar`, `TranscriptionSummary`, `ImportProgress`,
  `RenamePrompt`, error) is **not restructured** — only its error `<p>`
  adopts `Banner`. Consolidating the stack is explicitly deferred (declutter
  theme) and noted in Gaps.

## Error handling

No new runtime error paths — this is presentational. The one discipline: a
primitive must never *swallow* an interaction. `IconButton`/`AppButton`
forward `disabled` and `click` faithfully; a disabled control emits nothing
(matching today). Empty-state blocks are static markup. `Banner` is a dumb
container — callers still own their error strings and live regions
(`role="status"`/`aria-live` stay on the call sites that have them, e.g. the
save-status indicator).

## Testing

- **Per-primitive unit tests** (Vitest + happy-dom + `@vue/test-utils`): each
  renders its variants, forwards `aria-label`/`title`/`disabled`, emits
  `click` (and does not when disabled), and exposes the focus ring. `Avatar`
  derives the initial; `CountBadge` caps at `99+`; `StatusDot` maps tone →
  class; `Chip` `interactive` emits.
- **Regression net:** every existing test for `ActionPanel`, `VaultList`,
  `Tasks`, `TaskRow`, and the tasks sub-components must stay green
  **unchanged** — this is the proof the refactor preserved behavior. Any test
  that has to change means a behavior change slipped in; treat that as a bug,
  not a test update, unless the change is a deliberate, called-out polish.
- **Token-parity check for the converted screens:** because tokens equal
  current values, the converted screens' snapshot/class expectations should
  match pre-refactor for the semantically-equivalent element (spot-checked in
  a couple of tests rather than a blanket snapshot).
- Follow the repo's TDD convention: for each primitive, failing test first;
  regression tests name the failure mode (e.g. "IconButton must forward
  disabled so a busy row can't double-submit").

## Quality gates & guardrails

- New primitives sit under `src/components/ui/`; the LOC baseline
  (`scripts/loc-baseline.json`) should **shrink** overall (dedup outweighs the
  new files) — re-run the guard with `--update` and commit the baseline in the
  same PR; if a converted file grows transiently, justify per repo policy.
- Respect the fallow quality ratchet (`scripts/quality-baseline.json`) and the
  coverage floors in `vite.config.ts`; update baselines only in the
  improving direction, in-PR.
- `npm run lint && npm run check:loc && npm run check:quality &&
  npm run test:coverage` must pass (coverage last, per the pipeline).

## Docs

- Add a short **"UI primitives"** subsection to AGENTS.md (Frontend state
  area): the token vocabulary, the primitive set + when to use each, and the
  rule that **new UI must use them instead of re-growing utility strings**.
- Add a Gaps entry recording the deferred work: (a) the density/banner-stack
  and VaultList row-action-count opportunities (declutter theme), and (b) the
  ~18 unconverted components still on raw utility classes, so the follow-up
  migration is tracked rather than forgotten.

## Rollout / compatibility

- Windows-only shipping app is unaffected at the Rust/build level (no
  `src-tauri` change); the three CI Rust jobs are untouched. The `frontend`
  CI job (ESLint, LOC, quality, typecheck, Vitest coverage) is the gate.
- Purely additive + refactor: no config schema change, no IPC change, no
  migration. A user sees a slightly more consistent, polished home and tasks
  view; everything else is visually identical.
- Suggested implementation phasing (for the plan): (1) tokens; (2) primitives
  + their tests; (3) ActionPanel; (4) VaultList; (5) Tasks view; (6) docs +
  baselines. Each phase is independently green and shippable.
