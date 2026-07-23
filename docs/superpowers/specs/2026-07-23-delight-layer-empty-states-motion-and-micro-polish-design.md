# Delight layer — empty states, motion & micro-polish — design

Date: 2026-07-23
Status: accepted (user request: "lets continue with improving ux/ui" → chose,
of the remaining UX directions, the **visible delight layer**: proper
empty/loading states + subtle motion + consistent micro-interactions.
Explicitly additive — no layout/IA restructuring (that's the separate
declutter increment), no light mode (needs the full token migration), no new
features)

## Problem

The design-system increment (PR #69, merged) made the frontend **consistent**
— tokens + nine `src/components/ui/` primitives, with `ActionPanel`,
`VaultList`, and `TaskRow` refactored onto them. What it deliberately did not
touch is the app's **feel**, and three flat edges now stand out against the
newly-tidy surfaces:

1. **Empty & degraded states are lonely gray text.** Every "there's nothing
   here" / "something's off" moment renders as a bare `<p class="text-xs
   text-slate-400">` one-liner — *Obsidian not found — no vaults discovered…*,
   *No vaults match "…"*, an empty task list, an empty recordings list, no
   search results, no transcriptions, the import picker's empty/blocked
   states. Functional, but they read as unfinished next to the polished rows
   above them, and they waste the one moment where a friendly nudge (an icon,
   a next action) helps most.
2. **Zero motion.** `<Transition>` appears **0 times** in the codebase — every
   panel view swap (list ↔ tasks ↔ search ↔ settings ↔ recordings ↔ …) is an
   instant hard cut. For a companion the PRD calls "delightful, alive," the
   navigation feels abrupt.
3. **Loading is ad-hoc.** ~10 components hand-roll an `animate-spin` ring with
   slightly different sizes/markup; there's no shared spinner, so loading
   moments look subtly inconsistent.

None of this is a bug. It's the layer of polish that turns "consistent" into
"considered," and it's low-risk because it is **additive** — it changes how
things appear, not what renders or where.

## Goals & scope decisions

- A shared **`EmptyState`** primitive and its rollout to the panel's
  empty/degraded states — the core, most-visible win.
- A subtle, **reduced-motion-safe cross-view fade** in the panel.
- **Micro-interaction** consistency folded into the existing primitives
  (light, cut-able if it reads wrong on the desktop shell).
- A shared **`Spinner`** for the ad-hoc loading sites (cheap wins only,
  opportunistic).
- **Behavior-preserving.** Empty-state message text and every `data-testid` /
  `aria-label` stay byte-identical, so the existing Vitest suite is the
  regression net. Transitions/animations never change what renders.
- **Additive only.** No view is added or removed; no navigation, banner
  stack, window sizing, or window-system invariant is touched.

### Non-goals

- Layout / information-architecture restructuring (the declutter increment:
  the banner stack, the dense tasks toolbar, the five-per-row vault actions).
- Light mode / theming (needs the full token migration first — GAP-66).
- New features or new IPC/store/`src-tauri` changes. This is frontend-only.
- Migrating the remaining ~18 components onto tokens (still GAP-66); this
  increment only *adds* two primitives and *touches* the views whose empty
  states it upgrades.

## Design

Everything extends the existing design system. Two new presentational
primitives under `src/components/ui/`, a small motion layer in
`src/style.css` + `ActionPanel`, and a light touch inside the existing
interactive primitives. No `src-tauri`.

### 1. `EmptyState` primitive + rollout

`src/components/ui/EmptyState.vue` — a centered, presentational block:

- Props: `{ title: string; hint?: string; icon?: <slot>; }` + a default/`action`
  slot for an optional `AppButton`. Renders an `AppIcon`-sized glyph (via an
  `icon` slot so each caller supplies its own line-icon), a `text-fg` title,
  an optional `text-fg-muted` hint line, and the optional action, vertically
  centered with comfortable padding (`px-4 py-8 text-center`).
- Tokens only (`text-fg` / `text-fg-muted` / `text-micro`), so it inherits the
  palette and a future light mode for free.

Roll it out to the known bare-`<p>` states, **preserving the exact message
strings and any `data-testid`** so tests stay green (the plan enumerates each
call site and its current text verbatim):

- `ActionPanel.vue` — *Obsidian not found…* (degraded) and *No vaults match
  "{{filter}}"* (empty filter).
- `Tasks.vue` — the empty task list (overall, and the filtered-to-nothing
  case).
- `Recordings.vue` — the empty recordings list.
- `Search.vue` — no results for a valid query (distinct from the pre-query /
  too-short-query states — leave those as they are).
- `Transcriptions.vue` — the empty transcriptions list.
- `ImportVaultPicker.vue` — the empty state under its checking/blocked/empty
  branches, where a bare message renders today.

Each becomes an `EmptyState` with a fitting icon and, where there's an obvious
next step, an action (e.g. *Obsidian not found* → an "Open Obsidian's site" /
retry affordance only if one already exists; otherwise title + hint only —
**do not invent new commands or IPC**). Where a state already has an action
button today, reuse it in the slot.

### 2. Cross-view fade

- Wrap the panel's view-body region in `ActionPanel.vue` in a
  `<Transition name="view" mode="out-in">` keyed on the active `view`.
- Add to `src/style.css`:
  ```css
  .view-enter-active,
  .view-leave-active { transition: opacity 120ms ease; }
  .view-enter-from,
  .view-leave-to { opacity: 0; }
  ```
- Opacity-only (no transform — avoids any layout thrash in the fixed panel),
  `mode="out-in"` so the leaving view fully fades before the entering one
  appears. The global `prefers-reduced-motion` rule already in `style.css`
  forces the duration to ~0, so it is reduced-motion-safe with no extra work.
- **Guard:** the transition must not disturb the `panel-scroll` containers,
  focus landing (the `panel-shown` refresh, the search/`/` focus jump), or the
  view-swap test-ids. If any existing `ActionPanel` test needs its assertions
  changed to pass, that's a signal the wrapper altered behavior — stop and
  reconsider rather than editing the test. (The wrapper was prototyped as an
  optional step in the prior increment's plan and deferred; this promotes it
  to a real, tested step.)

### 3. Micro-interactions (light)

Fold a single, subtle **press feedback** into the interactive primitives so it
applies everywhere they're used at once:

- `IconButton`, `AppButton`, `Chip` (interactive): add `active:scale-95`
  (or `active:opacity-80` if scale reads too springy) + `transition-transform`
  already implied by their `transition-colors`. Keep it barely perceptible —
  "personality without becoming distracting" (PRD).
- This is the one genuinely subjective part; the plan makes it a **separable,
  final task** so it can be tuned or dropped after a look on Windows without
  affecting the rest of the increment.

### 4. `Spinner` consistency (cheap wins)

`src/components/ui/Spinner.vue` — the shared `animate-spin` ring
(`h-4 w-4 border-2 border-white/30 border-t-white`, size prop), replacing the
hand-rolled copies **only where it's a clean 1:1 swap** (same size/markup) and
the site is one of the already-touched views. Anything that needs restructuring
to adopt it is left for GAP-66. Purely opportunistic dedup.

## Error handling

No new runtime paths — presentational. `EmptyState`/`Spinner` are dumb
containers; callers keep owning their state logic and any live regions. The
one discipline: an `EmptyState` with an action must forward the click to the
caller's existing handler (no new commands invented), and a transition must
never swallow focus or a keystroke (see the §2 guard).

## Testing

- **Unit tests** for `EmptyState` (renders title/hint/icon slot; renders the
  action slot; tokens applied) and `Spinner` (renders, size prop, has
  `role="status"`/`aria-label` where the originals did).
- **Regression net:** every existing test for the touched views (`ActionPanel`,
  `Tasks`, `Recordings`, `Search`, `Transcriptions`, `ImportVaultPicker`) must
  stay green **unchanged** — the empty-state refactors preserve message text
  and test-ids, and the transition is a wrapper. A test that must change means
  behavior changed; treat it as a defect, not a test update.
- **Reduced motion** is already enforced globally; add one test asserting the
  transition classes exist (the behavior itself is CSS/browser, not asserted).
- TDD per the repo convention: failing test first for the new primitives;
  regression tests name the failure mode.

## Quality gates & guardrails

- New primitives under `src/components/ui/`; follow the AGENTS.md UI-primitives
  contract (tokens, declared prop names, unit-tested).
- LOC baseline should hold or shrink (EmptyState/Spinner dedup the bare-`<p>`
  and spinner repetition) — re-run `--update` if a metric improves, in-PR.
- `npm run lint && npm run check:loc && npm run check:quality &&
  npm run test:coverage` must pass; keep lint at 0 new warnings (the prior
  increment's lesson).

## Docs

- Add `EmptyState` and `Spinner` to the AGENTS.md "UI primitives & design
  tokens" list.
- If any empty-state / spinner site is intentionally left un-migrated, note it
  under GAP-66 rather than silently skipping it.

## Rollout / compatibility

- Frontend-only; the `frontend` CI job is the gate. No config/IPC/`src-tauri`
  change, no migration, no schema change.
- Purely additive: a user sees friendlier empty states, a soft fade between
  panel views, and consistent press/loading feedback; everything else is
  visually unchanged.
- Suggested phasing for the plan: (1) `EmptyState` primitive + tests; (2) roll
  it out view-by-view (each its own task, gated by that view's existing
  tests); (3) the cross-view fade; (4) `Spinner` primitive + opportunistic
  swaps; (5) the micro-interaction press feedback (separable, last, tunable);
  (6) docs + baselines. Each phase is independently green and shippable.
