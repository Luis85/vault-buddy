# Region Capture Indicator — Design

**Status:** approved (2026-09-20); **AMENDED 2026-09-21** — see
[Amendment](#amendment-2026-09-21--gap-166-and-the-premise-probe) at the foot of this
file. Four sections below are superseded there and say so inline. The amendment is
part of the design, not a footnote: one of its items corrects a claim in
*List memberships* that GAP-166 made false.
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
is first shown. **Amended 2026-09-21 (A4): applied on EVERY show, not once at
startup.** Without it the indicator is a full-region transparent window
swallowing every click for the whole capture — strictly worse than no
indicator at all.

**`focus: false`**, and it is never focused. Taking focus would blur the panel
and trip `schedule_focus_out_check`, hiding the panel mid-capture.

### List memberships

The window joins five lists. Each has a test that fails until it does, which
is the mechanism by which this section stays true.

> **Superseded in part, 2026-09-21 (A2).** That sentence is no longer true of
> `EXCLUDED_LABELS`. GAP-166 inverted its test, so declaring this window makes
> the test fail *because* the window is excluded, not *until* it is. The other
> four rows stand. See amendment A2.

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

**Amended 2026-09-21 (A3): this arithmetic is no longer inline.** It is the
pure, Linux-tested `core::screen_geometry::indicator_bounds`, for the reason
the next sentence gives.

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

**Amended 2026-09-21 (A5): two more events.** Subscribing to the pause edges
alone leaves the border amber into the NEXT capture, because this window's
webview mounts once per process. The root also listens to `screen:stopped` and
`screen:failed`, each resetting to the recording colour.

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
`region_dims`, `clamp_to_frame`, `to_physical`. ~~This feature adds no new
arithmetic to that layer.~~ **Amended 2026-09-21 (A3): it adds one,
`indicator_bounds`, and it belongs in that layer rather than inline.**

What it does add, and what the tests cover:

- **List membership**, in both directions. Three of the four are the
  config-derived tests that already exist for the other windows;
  `EXCLUDED_LABELS` is a test-side allowlist naming its hardware evidence
  (A2), because its test is inverted.
- **The single show site and the single hide site**, each pinned to its
  enclosing function — the `capture_exclusion` pattern, which exists because a
  file-granular assertion let a relocation pass while leaking permanently.
- **The show is gated on a Region source** — a screen or window capture must
  not raise the indicator. A test asserts the gate, since the failure mode
  (a border around a full-screen capture) is cosmetic-but-confusing and would
  otherwise only surface on hardware.

**What cannot be tested here, stated plainly:** whether the border actually
appears on screen, whether it is genuinely click-through, whether it is
genuinely absent from the recording, and whether the border lands on the
right display. The offset ARITHMETIC is now tested (A3); what stays untestable
is everything between it and a lit pixel — monitor enumeration, the GDI
display-number join, and `set_position` itself. All four are `cfg(windows)` window behaviour
that executes in no automated test on any platform (GAP-117's class). They
belong on the manual verification checklist, and the mixed-DPI, multi-monitor
row is the one most likely to fail.

## Verification checklist rows to add

Written now, run after the final phase per the standing decision on manual
Windows testing. **Amended 2026-09-21 (A6): three more rows, and the standing
decision itself is gone** — the deferral was lifted on 2026-09-21 and the
checklist is being run batch by batch.

| Check | Steps |
| --- | --- |
| The region border appears and traces the recorded rectangle | Record a region ~in the middle of a monitor. **Record** whether a border appears, and whether it traces the rectangle drawn on all four edges. |
| The border is click-through | With the capture running, click something underneath the border's edge and inside the region. **Record** whether the click reached the application underneath. |
| The border is not in the recording | Play back the staged file. **Record** whether the border appears in any frame. |
| The border is on the right monitor | Record a region on a NON-primary monitor. **Record** which monitor the border appeared on. This is the monitor-offset arithmetic; a failure here puts it on the primary. |
| The border tracks pause | Pause, then resume. **Record** the border's colour in each state, and whether it stayed in place. |
| The border goes when the capture does | Stop the capture. **Record** whether the border disappeared. Then repeat for a capture that self-finalizes (close the recorded source) and one that fails. |
| Display scaling | Repeat the first row at 125% / 150% / 200%. **Record** the drawn rectangle and whether the border still traces it. |

---

## Amendment 2026-09-21 — GAP-166 and the premise probe

This design was approved on 2026-09-20 and not implemented; GAP-165 is the
backlog entry that finally tracked it. In the day between approval and
implementation, **GAP-166 changed the fact this whole design rests on**, and
Phase 5 landed. This amendment records what that forced. It is part of the
design: where it disagrees with a section above, this wins, and the section
above says so inline.

**Phase 5 forced nothing.** The export runs after a capture has ended, touches
no window, and reads `ExportState` rather than anything here. Checked rather
than assumed, because the residual in GAP-165 asked for it.

### A1 — The premise was put on trial, and it survived

The design's enabling fact is that a window in `EXCLUDED_LABELS` is invisible
to screen capture while fully visible to the user. Between approval and
implementation, **GAP-166 found that the `SetWindowDisplayAffinity` round-trip
on our WebView2-hosting windows stopped the editor painting and stopped OTHER
applications' toolbars taking pointer input until the process exited.**
`EXCLUDED_LABELS` was narrowed to `["main"]` — the buddy, and only the buddy —
and its config-pin test was inverted, so a window is no longer excluded by
being declared.

A sixth WebView2 window under affinity is precisely that hazard, so the
premise was re-established on hardware before any code: one one-variable
rebuild with `EXCLUDED_LABELS = ["main", "panel"]`, with the prediction stated
first. `panel` is the right probe because it is configurationally identical to
the indicator proposed here, and it is the one window that can be kept visible
and clicked throughout a capture.

**Result: clean.** Across several captures, Explorer's toolbar and Notepad's
menu bar both still took clicks after the capture stopped with Vault Buddy
still running, and the panel painted normally throughout. Recorded in full in
docs/Gaps.md GAP-165, including what it does NOT license: `bubble` and
`overlay` were never probed individually, and the root cause remains
unidentified.

So the indicator IS excluded — and the exclusion ships in the same commit as
the window, by the author's decision, with the verification rows below
covering both.

### A2 — `EXCLUDED_LABELS` membership is a test-side allowlist, not a config derivation

*Supersedes the third row of* **List memberships**.

The other four lists are derived from `tauri.conf.json` and behave exactly as
that table says. `EXCLUDED_LABELS` no longer can: its test asserts the
INVERSE — `main` in, every other declared window out — so adding this window
to the constant reddens it.

The guard keeps its teeth by holding the allowlist in the **test**, never in
production: the test carries `["main", "region-indicator"]`, each with the
hardware evidence for it named in a comment, asserts `EXCLUDED_LABELS` matches
that set exactly, and asserts every other declared window is absent. A seventh
window added to the config is still not excluded by declaring it, which is the
property GAP-166 bought and the one worth keeping.

### A3 — The monitor-offset addition is a pure function

*Supersedes* **Geometry**'s inline arithmetic.

The section above calls this addition "the single most likely thing to get
wrong" and says "it fails silently in the worst way" — and then leaves it
inline. Those two things do not belong together in this repository. It is:

```rust
core::screen_geometry::indicator_bounds(monitor_origin, rect) -> (i32, i32, u32, u32)
```

Pure, Linux-tested, beside `to_physical`. Its fixtures are deliberately
**asymmetric** — a non-zero monitor origin and a non-square rect — so that
dropping the offset, adding it to the wrong axis, and transposing width and
height each fail with a distinct number. A square fixture at the origin
distinguishes none of them. This is the `source::region_dims` precedent, which
exists for exactly this reason.

### A4 — `set_ignore_cursor_events(true)` runs on every show

*Supersedes* **The window**'s "applied before the window is first shown".

Applying it once at startup means a single silent failure turns every
subsequent capture's indicator into a full-region window swallowing every
click — the outcome this design itself calls "strictly worse than no indicator
at all" — with no retry and nothing to notice it. Applying it in the show path
costs one idempotent call per capture and is retried each time.

### A5 — The pause colour resets on `screen:stopped` and `screen:failed`

*Supersedes* **Pause**.

This window is declared in the config and hidden-not-destroyed, so its webview
mounts **once per process** — the property that forced `editor:open` to exist.
Subscribing to the pause edges alone never returns the border to the recording
colour: pause a region capture, stop it while paused, start another, and the
border comes up amber on a capture that is genuinely recording. A silent lie
in the one surface whose entire purpose is telling the truth about what is
being recorded.

The root therefore listens to four app-wide events — `screen:paused` → amber,
`screen:resumed`, `screen:stopped` and `screen:failed` → recording. Every path
that showed the indicator is past the commit point, so one of the latter two
always arrives.

A single-window `region-indicator:show { paused }` event, on the `region:begin`
precedent, was considered and not taken: it adds a third single-window event
and a new emit site to carry state that four events the window already
receives describe completely.

### A6 — Three more verification rows

The rows in the section above stand. Three join them, and the first two exist
only because of GAP-166:

| Check | Steps |
| --- | --- |
| **Other applications survive a region capture** | Record a region, stop it, and — with Vault Buddy still running — click Explorer's toolbar and Notepad's menu bar. **Record whether each responds.** This is GAP-166's exact symptom against the sixth excluded window. A failure here means the indicator must lose its exclusion, and the feature with it. |
| **The panel is not hidden when the indicator appears** | With the panel open, start a region capture and do not touch anything. **Record whether the panel is still open afterwards.** `show()` on an always-on-top window may activate it despite `focus: false`; if it does, the activation blurs the panel and `schedule_focus_out_check` hides it mid-capture. This cannot be determined off Windows. |
| **Rows 16, 17 and 43 re-run** | A sixth excluded window changes exactly what those rows verify, so their earlier passes do not carry over to this build. |
