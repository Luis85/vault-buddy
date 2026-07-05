# Increment 3 — Greeting Speech Bubble (design)

## Summary

Give the buddy a voice. On app launch the buddy shows a **speech bubble**
with a short, warm greeting chosen from the current **time of day** and
whether it's a **weekday or weekend**. The bubble fades in, stays a few
seconds, and auto-dismisses. This is the first increment of a broader
"buddy can talk" capability; it deliberately ships the *bubble mechanism*
plus a *startup greeting* and nothing more.

## Goals

- A speech bubble renders next to the buddy without being clipped by the
  transparent window, and without introducing a large invisible
  click-blocking area.
- A greeting is picked from a small phrase pool keyed by
  `{daypart, weekend?}` and reads naturally at any hour and any day.
- The bubble auto-dismisses after a few seconds; the user never has to
  interact with it.
- The greeting never fights the vault panel: opening the panel supersedes
  the bubble cleanly.

## Non-goals (explicitly out of scope for this increment)

- Per-character voice (the wizard sounding mystical, the dwarf gruff). All
  characters share one phrase set for now.
- Special dates / holidays / first-launch specials.
- A settings toggle to disable greetings. (An obvious later add; not now.)
- Any trigger other than app launch (no idle chatter, no reactions to
  events, no click-to-talk).
- Persistence of "already greeted today" — the greeting shows once per app
  launch (per frontend mount), which is the natural cadence.

## Product decisions (from brainstorming)

| Question | Decision |
| --- | --- |
| Dismissal | Auto-dismiss after a few seconds; no interaction required. |
| Content model | Time-of-day × weekday/weekend, random pick from a small pool. |
| Character voice | Character-neutral (one shared phrase set). |
| Space mechanism | Approach A — a dedicated transient "bubble" window geometry. |

## Architecture

```
greeting.ts (pure)  ──►  useGreeting.ts (composable)  ──►  App.vue
  time → message           lifecycle + timer               <SpeechBubble>
                                    │
                                    └──► useCompanionWindow (extended)
                                         collapsed / bubble / expanded
```

Three new units plus one extension, each with a single clear purpose:

### `src/greeting.ts` (pure logic — no Vue, no Tauri)

The single source of greeting *content* and *selection*. Fully testable
anywhere.

```ts
export type Daypart = "morning" | "afternoon" | "evening" | "night";

/** Hour buckets (local time), matched as whole ranges. */
// morning   05:00–11:59
// afternoon 12:00–16:59
// evening   17:00–21:59
// night     22:00–04:59
export function daypartFor(date: Date): Daypart;

/** Sat/Sun are the weekend; getDay() 0 = Sunday, 6 = Saturday. */
export function isWeekend(date: Date): boolean;

/**
 * Pick one greeting for the given moment. `pick(n)` returns an index in
 * [0, n) — defaults to Math.random-based selection, injected in tests for
 * determinism.
 */
export function greetingFor(
  date: Date,
  pick?: (n: number) => number,
): string;
```

- Phrase pools are a `Record<Daypart, { weekday: string[]; weekend: string[] }>`
  literal in this module. Each cell holds a few (≈3–4) phrasings so repeated
  launches don't feel canned.
- `greetingFor` computes `daypart` + `weekend?`, reads that cell, and returns
  `pool[pick(pool.length)]`.
- Selection is the only randomness; keeping it behind `pick` makes the whole
  module deterministic under test.

**Boundary of this unit:** in → a `Date` (and optional `pick`); out → a
finished string. It knows nothing about windows, Vue, or timers.

### `src/composables/useGreeting.ts` (lifecycle)

Owns *when* the bubble is shown and drives the geometry ref.

```ts
export function useGreeting(): {
  bubbleVisible: Ref<boolean>;
  bubbleText: Ref<string>;
  dismiss: () => void;
};
```

- On mount: `bubbleText = greetingFor(new Date())`, `bubbleVisible = true`,
  and start a `setTimeout` of `GREETING_MS` (a single tunable constant,
  ≈5000 ms) that flips `bubbleVisible = false`.
- Returns `dismiss()` — cancels the timer and sets `bubbleVisible = false`.
  `App.vue` calls it when the panel opens so the greeting yields immediately.
- Clears the timer on unmount and on any early dismissal (idempotent).
- Under unit tests (no Tauri), nothing here throws — it only touches refs and
  timers; geometry effects live in `useCompanionWindow`, which already
  swallows non-Tauri errors.

### `src/components/SpeechBubble.vue` (presentational)

- Props: `text: string`, `side: "left" | "right"`, `valign: "up" | "down"`.
- Renders a Tailwind speech bubble with a small tail. The tail's corner is
  derived from `side`/`valign` so it points back at the buddy even when the
  window was edge-shifted (bubble unfolds away from the edge, tail points
  toward the buddy).
- Vue `<Transition>` for fade in/out. Purely visual; no logic, no store.

### `src/composables/useCompanionWindow.ts` (extended — Approach A)

Add a **third geometry state** between collapsed and the full panel.

- New size constant next to `COLLAPSED`/`EXPANDED`:
  `export const BUBBLE = { width: 260, height: 150 };` (tunable during build).
- `useCompanionWindow` gains a second reactive input: `bubbleOpen: Ref<boolean>`.
  Signature becomes `useCompanionWindow(panelOpen, bubbleOpen)`.
- The existing serialized transition **queue** watches both refs and resolves
  a single target state with this precedence:
  1. `panelOpen` → `EXPANDED` (unchanged behaviour).
  2. else `bubbleOpen` → `BUBBLE`.
  3. else → `COLLAPSED`.
- `BUBBLE` placement reuses `planPanelPlacement(home, monitorRect, scale,
  COLLAPSED, BUBBLE)`, so the same edge-shift offset and `side`/`valign`
  mirroring apply — the buddy stays visually pinned and the bubble unfolds
  toward free space.
- Offset is reported to Rust via the existing `set_panel_offset` path, so
  tray-quit / Alt-F4 / updater still restore the unshifted home position even
  if they fire while the bubble is up. (The bubble is transient and typically
  gone before any quit, but the invariant is preserved for free by routing
  through the same code.)
- No new Rust command. Geometry is still one `set_window_geometry` call per
  transition — the "one native call" invariant holds.

**Interaction with the panel:** opening the panel while the bubble is up is
just a newer toggle; the existing `stale()` / "newer toggle wins" logic
supersedes the bubble transition. When the panel is open the bubble is not
shown as a separate geometry — if a greeting were somehow still logically
"visible", the panel's `EXPANDED` window already contains the buddy area.

## Wiring in `App.vue`

- Call `const { bubbleVisible, bubbleText } = useGreeting();`.
- Pass `bubbleVisible` as the new `bubbleOpen` arg to `useCompanionWindow`,
  alongside `panelOpen`.
- Render `<SpeechBubble>` in the buddy cell region when
  `bubbleVisible && !panelOpen`, passing `side`/`valign` (already returned by
  `useCompanionWindow`) so it anchors correctly.
- When the panel opens (`store.togglePanel()` from the buddy click), call the
  greeting's `dismiss()` so the timer is cancelled and no stale bubble
  lingers. Simplest form: watch `panelOpen`; when it becomes true, dismiss.

## Data flow (startup)

```
app mount
  → useGreeting: bubbleText = greetingFor(now); bubbleVisible = true
  → useCompanionWindow queue: COLLAPSED → BUBBLE (one set_window_geometry)
  → SpeechBubble fades in, tail toward buddy
  … ~5 s …
  → timer: bubbleVisible = false
  → queue: BUBBLE → COLLAPSED; offset restored to 0
  → SpeechBubble fades out
```

Early-dismissal path:

```
user clicks buddy before timer
  → panelOpen = true; watch fires dismiss() (timer cleared, bubbleVisible=false)
  → queue: newer toggle wins → EXPANDED (panel), bubble geometry skipped
```

## Error handling & edge cases

- **No Tauri runtime (unit tests):** `greetingFor` and the timer run fine;
  `useCompanionWindow`'s IPC calls already `.catch()` to no-ops, so the
  greeting degrades to "text computed, no resize" without throwing.
- **No monitor/window info:** `applyOpen`'s existing catch grows in place
  (right/down) — the bubble still shows, just unshifted. Same fallback the
  panel uses.
- **Rapid mount/unmount:** timer cleared on unmount; no dangling callback
  flips a ref on a torn-down component.
- **Panel opened mid-greeting:** queue precedence + `dismiss()` guarantee a
  single clean transition to the panel; no intermediate flash (single native
  geometry call preserved).
- **Clock at exact bucket boundaries:** ranges are half-open and contiguous
  (…21:59 evening, 22:00 night, …04:59 night, 05:00 morning) so every hour
  maps to exactly one daypart.

## Testing plan (TDD — failing test first)

- `tests/greeting.test.ts` (**written first**):
  - `daypartFor` at each boundary: 04:59→night, 05:00→morning, 11:59→morning,
    12:00→afternoon, 16:59→afternoon, 17:00→evening, 21:59→evening,
    22:00→night.
  - `isWeekend`: Mon–Fri false, Sat/Sun true.
  - `greetingFor` with injected `pick` returns the expected phrase from the
    expected cell; `pick` receives the pool length.
  - Every pool cell is non-empty (guards against an empty-array `pick`).
- `tests/companion-window.test.ts` (**extend existing**):
  - `bubbleOpen` true grows the window to `BUBBLE`.
  - `panelOpen` wins when both are set (target is `EXPANDED`).
  - Closing the bubble returns to `COLLAPSED` and zeroes/reports the offset.
- `tests/use-greeting.test.ts` (fake timers):
  - Shows on mount (`bubbleVisible` true, `bubbleText` non-empty).
  - Hides after `GREETING_MS`.
  - `dismiss()` (panel open) cancels the timer and hides immediately.
- `tests/speech-bubble.test.ts`:
  - Renders the passed text.
  - Tail class/position reflects `side`/`valign`.

All tests run under Vitest + happy-dom with no real Tauri runtime, per repo
convention.

## Files touched

| File | Change |
| --- | --- |
| `src/greeting.ts` | new — pure content + selection |
| `src/composables/useGreeting.ts` | new — lifecycle + timer |
| `src/components/SpeechBubble.vue` | new — presentational bubble |
| `src/composables/useCompanionWindow.ts` | extend — `BUBBLE` state + `bubbleOpen` input |
| `src/App.vue` | wire greeting + render bubble |
| `tests/greeting.test.ts` | new |
| `tests/use-greeting.test.ts` | new |
| `tests/speech-bubble.test.ts` | new |
| `tests/companion-window.test.ts` | extend |

No Rust changes. The shell crate is untouched, so this increment compiles and
is fully testable on Linux/CI without the Windows-only build.

## Tunables (single constants, easy to adjust during build)

- `GREETING_MS` ≈ 5000 — how long the bubble stays.
- `BUBBLE = { width: 260, height: 150 }` — bubble window size.
- Phrase pools in `greeting.ts`.
