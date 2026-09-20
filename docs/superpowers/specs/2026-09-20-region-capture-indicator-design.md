# Region Capture Indicator — Design

**Status:** approved (2026-09-20)
**Relates to:** `docs/superpowers/specs/2026-09-18-screen-capture-intake-design.md` §5.2, §5.3, §7.3
**Lands on:** `claude/screen-capture-intake-g0j49q` (PR #79), after Phase 4's editor tasks

## Context

Manual Windows testing of phase 3's region capture surfaced a real gap: while
recording a region, the user sees Windows' yellow capture border around the
**entire monitor**, and nothing at all showing which rectangle is actually
being recorded. The functionality is correct; the feedback is not.

This is not a bug in our code, and it cannot be fixed by changing what we ask
Windows for. Region capture resolves to `SourceHandle::Screen(monitor)` —
WGC captures the whole monitor and `convert::bgra_crop_to_nv12` crops on the
CPU (the design in §6, kept that way so the crop stays testable on Linux,
docs/Gaps.md GAP-125). Windows draws its border around what it is genuinely
capturing, which really is the whole screen. WGC captures monitors or windows,
never an arbitrary rectangle, so there is no way to retarget that border.

The indicator therefore has to be ours. The enabling fact is phase 3's
`WDA_EXCLUDEFROMCAPTURE` (§5.3): every window in
`capture_exclusion::EXCLUDED_LABELS` is invisible to screen capture while
remaining fully visible to the user. A border we draw is seen by the user and
**absent from the recording** — which is what makes drawing one over the
recorded area free, rather than something we would have to route around.

## Goals

- While a REGION capture runs, the recorded rectangle is visibly outlined on
  the user's screen.
- The outline never appears in the recording.
- The outline never blocks interaction with whatever is underneath it.
- The outline distinguishes recording from paused, using the visual language
  the capture bar and the buddy already use.

## Non-goals — deliberate, not deferred-by-accident

- **No indicator for screen or window captures.** Windows' own border is
  already truthful for those: it surrounds exactly what is being captured.
  Adding a second, redundant border would be noise.
- **The border is not draggable.** Re-aiming a live capture means re-targeting
  a running session — a different feature with its own correctness questions
  (what happens to frames already written at the old crop?). Out of scope.
- **No dimming of the area outside the region.** Considered and rejected: a
  full-monitor tint for a whole session is visually heavy, and a scrim risks
  being mistaken for something that is itself being recorded.
- **No region coordinates in the capture bar.** Text tells the user the
  numbers; this feature is about showing them the rectangle.

## The window

A sixth window, `region-indicator`, declared in `tauri.conf.json` alongside
`main`, `panel`, `bubble`, `overlay` and phase 4's `editor`:

```jsonc
{ "label": "region-indicator", "title": "Vault Buddy — Recording Region",
  "visible": false, "transparent": true, "decorations": false,
  "alwaysOnTop": true, "resizable": false, "skipTaskbar": true,
  "shadow": false, "focus": false }
```

**Declared in the config, never built at runtime.** This is load-bearing and
is recorded here because it has already been identified as a trap: the
config-derived label tests AND `capture_exclusion`'s one-shot apply both
assume every window exists in `tauri.conf.json`. A runtime-built window goes
blind to all of them simultaneously — the tests never check a window they
cannot see, and an exclusion applied once at capture start never reaches a
window created later.

**Click-through.** `set_ignore_cursor_events(true)`, applied before the window
is first shown. Without it the indicator is a full-region transparent window
swallowing every click for the whole capture — strictly worse than no
indicator at all.

**`focus: false`**, and it is never focused. Taking focus would blur the panel
and trip `schedule_focus_out_check`, hiding the panel mid-capture.

### List memberships

The window joins five lists. Each has a test that fails until it does, which
is the mechanism by which this section stays true:

| List | Why |
| --- | --- |
| `tray::ALL_WINDOW_LABELS` | Destroyed on quit — every webview shares WebView2's `Chrome_WidgetWin_0` class, and leaving one alive fails the unregister with `ERROR_CLASS_HAS_WINDOWS` |
| `tray::COMPANION_LABELS` | Hidden on hide-to-tray. Unreachable in practice (hide is refused mid-capture, and the indicator only exists mid-capture) — included so that if it is ever visible outside a capture, which would be a bug, hide-to-tray still takes it down |
| `capture_exclusion::EXCLUDED_LABELS` | The whole feature depends on this: the border must not reach the footage |
| `tray::POSITION_DENYLIST` | Its position is computed per capture; a restored one is junk |
| `capabilities/default.json` `windows` | Without it the webview gets no permissions and every `invoke` fails |

## Geometry

The region rect (`region::RegionSource.rect`) is in **physical pixels relative
to its own monitor's origin**. A window's position is in **virtual-desktop
coordinates**. So:

```
indicator.position = monitor.position() + (rect.x, rect.y)
indicator.size     = (rect.width, rect.height)
```

**This addition is the single most likely thing to get wrong, and it fails
silently in the worst way**: omit the monitor offset and the border lands on
the primary monitor while the capture records the secondary one — precisely
the confusion this feature exists to remove. The overlay already performs the
same translation for the same reason (`region_commands::select_region_inner`
positions at `monitor.position()`), so this is an established pattern, not a
new one.

Positioned and sized **while hidden**, then shown — the discipline every
companion window follows, for the stale-frame reason the window-system
section documents.

The border is drawn at the window's own edges, 2px, inside the bounds. It
overlaps the recorded area by design: the window is excluded from capture, so
overlap costs nothing in the file, and tracing the exact rectangle is more
useful than approximating it from outside.

## Lifecycle

**Show** hooks into `screen_capture_worker::start_screen_capture_blocking`,
immediately beside `capture_exclusion::apply(app)`. That function has already
parsed the source id (`let parsed = source::SourceId::parse(...)` at its top),
so the show is gated on `SourceId::Region` and skipped for screen and window
captures. Placing it beside the exclusion apply is deliberate: both are
capture-scoped window side effects, both are fire-and-forget on the main
thread, and both belong after the reservation is installed so every failure
below them funnels through the same teardown.

**Hide** goes into `screen_commands::clear_active_screen`, the one chokepoint
every teardown path funnels through — clean stop, self-finalize, and all ten
of the start path's early returns. It is **unconditional**: hiding an
indicator that was never shown is a no-op, which is strictly safer than a
conditional that could be wrong in the other direction and strand a border on
the user's desktop with no capture behind it. This is the identical reasoning
`clear_active_screen`'s existing exclusion clear already documents.

Both sites get the same structural test the exclusion has — one show, one
hide, each pinned to its enclosing function — because a leaked indicator is
exactly as invisible from inside the app as a leaked exclusion, and shows up
only as a border the user cannot dismiss.

## Pause

`RegionIndicatorRoot` subscribes to `screen:paused` and `screen:resumed` and
flips the border between the `recording` semantic token and amber, matching
`ScreenCaptureBar`'s status dot and the buddy exactly — one visual language
across all three surfaces. Note the bar's dot is deliberately bespoke rather
than the `StatusDot` primitive (which has no amber tone); the indicator
follows the bar, not the primitive, for the same reason.

It installs **no Pinia store**. It mirrors a single boolean with no derived
state, and the per-window `init()` wiring a store would require is more
machinery than the thing it tracks. `RegionRoot` sets this precedent: it is
already the one root that mirrors no Rust state.

## Error handling

Every step is best-effort and logged; **a failed indicator must never fail a
capture**. If the window is missing, the position or size call fails, or
`set_ignore_cursor_events` errors, the recording proceeds without a border and
a `log::warn!` records why. The inverse — letting a cosmetic surface abort the
feature it decorates — would be a worse bug than the one this fixes.

This differs from `region:begin`'s hard-error treatment in phase 3, and the
difference is deliberate: an un-armed overlay IS an unusable full-screen
scrim, whereas a missing indicator merely returns the user to today's
behaviour.

## Testing

The geometry that carries correctness is already pure and tested —
`region_dims`, `clamp_to_frame`, `to_physical`. This feature adds no new
arithmetic to that layer.

What it does add, and what the tests cover:

- **List membership**, in both directions, by the config-derived tests that
  already exist for the other windows.
- **The single show site and the single hide site**, each pinned to its
  enclosing function — the `capture_exclusion` pattern, which exists because a
  file-granular assertion let a relocation pass while leaking permanently.
- **The show is gated on a Region source** — a screen or window capture must
  not raise the indicator. A test asserts the gate, since the failure mode
  (a border around a full-screen capture) is cosmetic-but-confusing and would
  otherwise only surface on hardware.

**What cannot be tested here, stated plainly:** whether the border actually
appears on screen, whether it is genuinely click-through, whether it is
genuinely absent from the recording, and whether the monitor-offset arithmetic
puts it on the right display. All four are `cfg(windows)` window behaviour
that executes in no automated test on any platform (GAP-117's class). They
belong on the manual verification checklist, and the mixed-DPI, multi-monitor
row is the one most likely to fail.

## Verification checklist rows to add

Written now, run after the final phase per the standing decision on manual
Windows testing.

| Check | Steps |
| --- | --- |
| The region border appears and traces the recorded rectangle | Record a region ~in the middle of a monitor. **Record** whether a border appears, and whether it traces the rectangle drawn on all four edges. |
| The border is click-through | With the capture running, click something underneath the border's edge and inside the region. **Record** whether the click reached the application underneath. |
| The border is not in the recording | Play back the staged file. **Record** whether the border appears in any frame. |
| The border is on the right monitor | Record a region on a NON-primary monitor. **Record** which monitor the border appeared on. This is the monitor-offset arithmetic; a failure here puts it on the primary. |
| The border tracks pause | Pause, then resume. **Record** the border's colour in each state, and whether it stayed in place. |
| The border goes when the capture does | Stop the capture. **Record** whether the border disappeared. Then repeat for a capture that self-finalizes (close the recorded source) and one that fails. |
| Display scaling | Repeat the first row at 125% / 150% / 200%. **Record** the drawn rectangle and whether the border still traces it. |
