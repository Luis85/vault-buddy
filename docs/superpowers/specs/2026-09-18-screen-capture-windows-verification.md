# Screen Capture — Windows Verification Checklist

Manual end-to-end verification on a real Windows machine. **A running
document across phases**, not one phase's gate: each phase appends its own
table of rows and carries forward the rows earlier phases left open, so the
file always shows the whole verified surface of the feature rather than the
last increment's slice. No CI runner can record a screen, so every claim
below is one the automated gates structurally cannot make. `rust-core` proves
the pure modules on Linux and `windows-app` compiles and runs the crate on
Windows (GAP-102), but the `cfg(windows)` arms of `sink.rs`, `source.rs`,
`session/windows_session.rs`, `exclusion.rs` and the shell's
`capture_exclusion.rs` are exercised by nothing automated anywhere
(docs/Gaps.md GAP-117) — they are exercised **here**.

**Manual Windows re-testing is currently DEFERRED by the user until after
the final phase.** Rows are written as they are earned; nobody is being asked
to run them yet. A row whose *Result* is empty has not been run, which is not
the same as a row that failed.

Spec: [2026-09-18-screen-capture-intake-design.md](2026-09-18-screen-capture-intake-design.md)
(§5.2 the region overlay, §5.3 excluding our own windows, §6 the pipeline,
§6.2/6.3 clock and pause, §6.4 fragmented MP4, §6.5 audio, §7.1/7.2 the
picker, §7.3 during capture, §10 staging, §11 the IPC surface, §13 phasing,
§14 error handling, §15 testing).

Run against a build of `claude/screen-capture-intake-g0j49q`
(`npx tauri build --features gpu`, or `npm run test-build` for a dev run).

## Measurement discipline

Follow what §6.4's spike earned: **prefer instrumentation that reports over
assertions that confirm.** Every row below has a *Result* column — write the
observed value into it, not a tick. "No drops observed" is not a
measurement; "1 847 frames, 0 dropped, 59.4 fps average" is. The Windows
feedback loop is ~10 minutes per attempt, so a run that reports values
answers its question on the first attempt where a run that asserts a
hoped-for outcome fails for unrelated reasons and tells you nothing.

## Where to look

| What | Where |
| --- | --- |
| The staged capture | `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\` — `<base>.mp4` after a clean stop, `.<base>.mp4.part` (hidden, dot-prefixed) while capturing or after a crash |
| The sidecar | `<base>.json` beside it (source title/kind, recorded-at, duration, inputs) |
| Frame stats, warnings, failures | `vault-buddy.log` (tray → *Open logs folder*). `screen capture: dropped a frame (…); N so far` — first drop then one in 300 — and `screen capture: finalizing after N dropped frame(s)` at teardown |
| Live capture controls | `ScreenCaptureBar.vue` on the panel's **vault-list view**, beside the audio `RecordingBar` — elapsed (paused time excluded), source title, Pause / Resume / Stop, an inline warning line, and a dropped-frame chip that appears only once frames have dropped. The **tray / buddy right-click menu** drives the same Stop / Pause / Resume |

## Covered by Phase 2

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| 1 | **Monitor + one microphone** | Capture knowledge → *Record screen* → **Screen** tab → pick a monitor → tick exactly one input device → Start. Record ~30 s with speech. Stop from the tray menu. Play the staged `.mp4`. | **2026-09-19: reported working** (monitor capture produced a playable file). Duration / resolution / speech-audible not recorded — rerun for the numbers. |
| 2 | **Window + mic AND loopback, mixed** | Same picker, **Window** tab → pick a window. Tick one **input** (mic) and one **output** (loopback / speakers). Play music through the output while speaking. Stop and play back. | **2026-09-19: FAILED, then FIXED, then confirmed.** First run captured zero frames and finalize raised `MF_E_SINK_NO_SAMPLES_PROCESSED` (0xC00D4A44) — the declared size came from `GetWindowRect`, which includes DWM's invisible resize border, so every frame failed the `got >= want` guard and was dropped. Fixed in `130d700` (declare from `DWMWA_EXTENDED_FRAME_BOUNDS`). Re-test: window is captured. The mic+loopback mix question is still unanswered — rerun for levels. |
| 3 | **Zero audio devices** (spec §6.5) | Pick any source, tick **no** devices, Start, record ~20 s, Stop. | **2026-09-19: PASS.** Capture starts and runs with zero devices ticked, as §6.5 requires. |
| 4 | **Pause excludes its stretch** (spec §6.2/6.3) | Record ~20 s with a visible timer on screen, Pause from the tray, wait ~20 s (change the screen visibly during the pause), Resume, record ~20 s more, Stop. | **2026-09-19: PASS on duration** — file measured **40 s, not 60**, so the shared `CaptureClock` excludes paused time correctly. A/V sync after resume was NOT judgeable on that run: the video track was ~10x short (see the timeline bug under item 9), so both tracks disagreed regardless. Re-check sync once `d5ed392` is in the build. |
| 5 | **Recorded window closes mid-capture** (spec §14) | Record a window, close it after ~20 s, then Stop (or let it self-finalize). | **2026-09-19: PARTIAL.** Closing the recorded window stops the capture, which is the intended clean self-finalize rather than a failure. Not yet checked: whether a warning was surfaced, and whether the file holds everything up to the moment of the close. |
| 6 | **Mutual exclusion, both directions** (spec §7.3) | (a) Start an **audio** recording, then try to start a screen capture. (b) Stop it; start a **screen** capture, then try to start an audio recording. | **2026-09-19: PASS both directions.** Audio running → the screen-capture intake button is disabled. Screen capture running → starting a voice recording raises an explicit refusal. `CaptureGuard` holds. |
| 7 | **Hide to tray mid-capture** | Mid-capture, tray → *Show / Hide* (and try the buddy's own hide). | **2026-09-19: PASS on the refusal** — the buddy cannot be hidden to tray while capturing. The badge half of this row (buddy red/amber dot, vault-row dot) was not separately confirmed; worth a glance on the next run, since that is the affordance which makes the refusal legible rather than looking wedged. |
| 8 | **Kill the process mid-capture** (spec §6.4, through the real pipeline) | Record ~60 s, then Task Manager → *End task* on Vault Buddy. Relaunch. Copy the orphaned `.<base>.mp4.part` out of the staging dir, rename it `.mp4`, and open it in a player / `ffprobe -count_frames` it. | **2026-09-19: PASS — the headline result of this phase.** Process killed mid-capture via Task Manager; the orphaned `.part` renamed to `.mp4` plays. §6.4's crash-resilient-prefix property, previously only proven by the synthetic spike, now holds through the real pipeline. Byte/frame counts not recorded — optional, the qualitative result is the one that mattered. |
| 9 | **Static-screen heartbeat** | Start a capture and leave the screen **completely still** for ~60 s (no cursor movement), then move something, then Stop. | **2026-09-19: BLOCKED, then unblocked.** Untestable on that run: every capture played ~10x too fast (40 s recorded, 3–4 s elapsed). Root cause was this row's own subject — `VideoPacer` stamped each sample with the nominal 1/fps duration while WGC delivers only on change, so a still screen wrote ~2 samples/s and the summed track was ~10x short (audio, derived from sample counts, stayed correct, so the tracks also desynced). Fixed in `d5ed392` by repeating once per elapsed frame slot, making the stream constant-rate by construction. **A still screen is the worst case for that bug, so this row is now the sharpest test of the fix — run it first.** |
| 10 | **4K @ 60 fps for two minutes** (spec §17.3) | There is no settings surface for this in Phase 2 (`ScreenCaptureConfigTab` and `set_screen_capture_config` are Phase 6): with the app CLOSED, hand-edit the target vault's entry in `%APPDATA%\vault-buddy\config.json`, setting `"screenFps": 60`, then relaunch. Now capture a 4K monitor playing video for 2 minutes, Stop. | **2026-09-19: DEFERRED by the user** until a settings surface exists (Phase 6). Measuring 4K60 behind an app-closed `config.json` hand-edit tells us little, and §17.3's measurement is worth more once the knob is real. Carry this row into Phase 6. |

Extra observations worth writing down whatever the outcome: the CPU/GPU load
during (10), whether the encoder chosen was hardware or software (if the log
or Task Manager makes it visible), and the wall-clock time Stop took to
return for each capture (the stop wait is bounded at 30 s and reports
`stillSaving` on expiry).

Rows **2** (the mic+loopback mix), **5** (whether a warning surfaced and
whether the file holds everything up to the close), **7**'s badge half (the
buddy red/amber dot and the vault-row dot) and **9** (the still-screen
heartbeat, now unblocked by `d5ed392` and the sharpest test of that fix)
remain OPEN and are carried forward, not closed. Row **10** stays deferred to
Phase 6.

## Covered by Phase 3

The same measurement discipline applies, and applies hardest here: rows 12
and 13 are the phase gate, and a DPI bug shows up as an exact numeric factor.
**Write the numbers in.** A tick in row 12 hides precisely the failure row 12
exists to find.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| 11 | **Region on the primary monitor at 100%** | Capture knowledge → *Record screen* → **Region** tab → pick the primary screen → *Select region…* → drag a rectangle over something with readable text → Start → record ~20 s → Stop. Play the staged `.mp4`. **Record the file's pixel dimensions** and whether the recorded content is exactly the rectangle drawn — check all four edges, not just that "it looks right". | |
| 12 | **Region at 125 / 150 / 200% display scaling** | Repeat row 11 at each scale (Settings → System → Display → Scale), relaunching the app after each change. **Record, per scale: the rectangle drawn (approximate logical size), the file's actual pixel dimensions, and the ratio between them.** The expected ratio is the scale factor — this row IS the phase gate. An off-by-scale bug shows here as a file that is exactly 1/1.5 or 1/2 of the expected size, or a recording offset from the rectangle by the origin times the scale. | |
| 13 | **Region on a SECONDARY monitor, at a different scale from the primary** | With two monitors at different scales, select a region on the non-primary one. **Record which monitor the overlay appeared on, and whether the recorded content matches the rectangle.** This is the mixed-DPI case `core::screen_geometry`'s module doc warns about; if any row fails, it will be this one. | |
| 14 | **The overlay's own behaviour** | Open *Select region…* and, in separate attempts: (a) press Escape; (b) click once without dragging; (c) drag a tiny box (< 8 px); (d) drag from bottom-right to top-left; (e) complete one selection, then open *Select region…* a SECOND time in the same app run and draw another. **Record for each:** whether the overlay closed, whether the panel came back with focus, and whether the picker shows a region. Expected: (a)(b)(c) no region, (d) the same region as an equivalent top-left-to-bottom-right drag, (e) a normal second selection — (e) is the `region:begin` re-arm, which no automated gate on any platform can reach. | |
| 15 | **The panel does not auto-hide during a selection** | With the panel open, start a region selection and leave the overlay up for ~10 s before dragging. **Record whether the panel is still open underneath afterwards.** This is `DIALOG_ACTIVE`; a failure here loses the picker's state mid-selection. | |
| 16 | **`WDA_EXCLUDEFROMCAPTURE`** (spec §5.3) | Start any screen capture with the buddy and the panel plainly in frame. **Record whether they are visible to you during the capture (they must be) and whether they appear in the played-back file (they must not).** Note the Windows build number. | |
| 17 | **The exclusion is lifted afterwards** | After the capture from row 16 stops, record the same screen with a **different** tool (Xbox Game Bar `Win+Alt+R`, or a Teams screen share). **Record whether Vault Buddy's windows are visible in THAT recording.** They must be. A failure here is the leaked-exclusion case the structural test exists to prevent, and is invisible from inside the app — nothing looks wrong to the user, because the buddy is still on their own screen. | |
| 18 | **Resolution changed between selecting and starting** | Select a region near the right or bottom edge, then change the display resolution to something smaller **before** pressing Start. **Record the message.** Expected: a refusal naming the source as gone, not a started capture. | |


## Covered by Phase 4

Phase 4 builds the editor window. **The entry point has now landed** (Task 7):
after a screen capture stops, the capture bar on the panel's list view shows
the finished capture with an **Edit** button, and that is what calls
`open_capture_editor`. Every row below is therefore reachable by hand. (There
is still no staged-capture browser — only the capture that finished most
recently in this app run is offered. That is Phase 5. To reach an older
staged file, record a short new one.)

Rows 22–28 were added with the entry point. They cover what no automated
gate on any platform can: the editor is the first window of ours that is
`skipTaskbar: false`, decorated, resizable, hidden-not-destroyed on its own
close, destroyed on quit, excluded from a recording's pixels, and NOT taken
by hide-to-tray — seven properties of a real OS window, none of which a
Vitest fixture or a Linux `cargo test` can observe. The Rust-side tests that
exist for them read `tauri.conf.json` and scan source text; they prove the
LISTS are right, never that Windows behaves as the lists assume.

Row 19 stays the sharpest of the arithmetic-adjacent rows. Everything else
the editor computes is arithmetic a Vitest fixture can drive against a fake
capture; **loading the video is not** — it needs a real staged file, a real
asset-protocol request and a real scope check, which exist on Windows and
nowhere else. It is also the row that would have caught P-5, where
`assetPath` carried a bare file name and `convertFileSrc` — which joins
nothing — turned it into `http://asset.localhost/cap%20one.mp4`: a URL
naming no file, matching no entry in `$APPLOCALDATA/screen-captures/*`, and
failing with no error anywhere in the app.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| 19 | **The preview actually loads its video** (spec §8.2, the asset protocol) | Stage a capture (any row 1–3 recording will do), open it in the editor, and **look at the video frame before touching anything**. Expected: the first frame of the recording is visible and **Play** plays it with audio. **Record: whether a frame appeared at all**, and — in the editor window's DevTools (`F12`) — the `<video>` element's resolved `src` plus any failed request on the Network tab. A blank black frame with a `net::ERR_*` or a 403 on an `asset.localhost` request is the failure this row exists to find; so is a `src` that is a bare file name rather than a full `…\screen-captures\<base>.mp4` path. | |
| 20 | **An edit survives closing and reopening the editor** (spec §10 Resume, C-1) | Open a staged capture, scrub to ~4 s, **Split**, select the FIRST block, **Delete** — the first 4 s are now cut. Close the editor window. Open the SAME capture again: it must come back showing the trimmed timeline, not the whole recording. Now make any edit and press **Undo**. **Record: what `<base>.json`'s `timeline` field holds afterwards** (open it in a text editor — it is beside the `.mp4` in the staging dir). Expected: the trim, i.e. one segment starting at ~4000. A `timeline` that is absent or `null` there is the C-1 erasure: the edit is still on screen, but the file says the capture was never touched, and Phase 5's fast path would export the whole recording. | |
| 21 | **A failed save is visible** (spec §10 disk pressure, I-7) | Hard to stage honestly; run it only if it is cheap on the machine at hand — make the staging directory unwritable (deny your own user Write on `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`), then make an edit in an already-open editor. Expected: an amber banner above the preview saying the edit could not be saved, the edit still on screen, and the editor still usable. **Record whether the banner appeared**, and restore the permission afterwards. | |
| 22 | **The editor opens on a real capture, as a real window** (Task 7's entry point) | Record ~30 s, Stop, and wait for the capture bar to change from *Recording* to the finished row naming the capture. Click **Edit**. **Record:** whether the window opens at all; whether it has a normal title bar and can be resized and maximised; whether it appears in the taskbar and in Alt+Tab (it must — it is the only window of ours that is `skipTaskbar: false`); and whether the video plays. A window that opens but shows a black frame is row 19's failure, not this one. | |
| 23 | **The editor's own X hides it rather than quitting anything** (`window_close.rs`, I-1/I-2) | With the editor open and the buddy running, click the editor's titlebar **X**. **Record:** whether the app is still running, whether the buddy is still on screen, and whether clicking **Edit** again brings the editor back **showing the same capture** (it must — the window is hidden and reused, never destroyed, which is what `editor:open` exists for). Then stage a SECOND capture and click **Edit**: the editor must come back showing the NEW capture, not the old one. That second half is the one thing no test on any platform reaches. | |
| 24 | **Hide to tray does not take the editor** (`COMPANION_LABELS`) | With the editor open and a capture loaded in it, tray → **Show / Hide**. **Record:** whether the buddy and the panel disappear, and whether the editor stays. The editor must stay — hiding it from the outside would strand an in-progress edit off-screen with no way back. Then tray → **Show / Hide** again and confirm the buddy returns without disturbing the editor. | |
| 25 | **Quit destroys the editor** (`ALL_WINDOW_LABELS`, `Chrome_WidgetWin_0`) | With the editor open, tray → **Quit**. **Record:** whether the process really exits (check Task Manager — not just that the windows vanished), and whether `vault-buddy.log` carries `Failed to unregister class Chrome_WidgetWin_0`. It must not. A live WebView2 window at exit fails that unregister every time, which is the whole reason the editor is on the destroy list but not the hide list. | |
| 26 | **The editor is not in its own recording** (spec §5.3, and GAP-116's other half) | Start a capture of the whole screen with the editor open and plainly in frame. **Record:** whether the editor is visible to you during the capture (it must be) and whether it appears in the played-back file (it must not). This is the first excluded window that is NOT `skipTaskbar`, so it is the first real test of `capture_exclusion`. **While the picker is open, also check the Window tab**: Vault Buddy's editor must NOT be offered as a capture source — it is the first window of ours that enumerates at all, so this is the first time the title filter actually runs (GAP-116). | |
| 27 | **Split, delete, reorder, undo, redo — and the keyboard** (spec §8.1, §8.2) | On a ~30 s capture: scrub and **Split** twice, **Delete** a middle block, drag a block to reorder it, then **Ctrl+Z** all the way back to the whole recording and **Ctrl+Shift+Z** (and separately **Ctrl+Y**) forward again. **Record:** whether the strip and the preview agree after every step (scrub to a moment and check the frame is the one the strip says), whether **Undo** is ever disabled while an edit is still visible on screen, and whether a split on an exact block boundary is correctly a no-op that does NOT consume an undo step. | |
| 28 | **Edits survive a crash** (spec §10, save-on-every-edit) | Open a staged capture, **Split** twice, then kill the process from Task Manager (**End task**) — do not close the editor first. Relaunch, record a throwaway capture to get the bar back… or, quicker, read `<base>.json` in the staging dir directly. **Record:** how many segments the `timeline` field holds. Expected: three — both splits, because every edit is written through immediately. Anything fewer means the sidecar write is not landing per-edit and spec §10's "a crash loses at most the last operation" does not hold. | |

## Known absences (do not file these as failures)

- **No live `fps` readout.** `ScreenCaptureBar.vue` (plan Task 10) landed —
  it renders elapsed, paused state, the source title, warnings and the
  **dropped-frame count**. Read that count off the bar only under two
  conditions: the capture is still **live** and the panel is open on the
  **list view**. The bar unmounts and `reset()` zeroes `dropped` the instant
  `screen:stopped` lands, so the teardown total (`screen capture: finalizing
  after N dropped frame(s)`) stays **log-only** — and item 10's own scenario
  (a 4K video playing for two minutes) takes foreground focus, which
  auto-hides the panel (`schedule_focus_out_check`). **The log remains the
  record for item 10's numbers**, as the table says; the bar is a live
  glance, not the measurement. The `fps` value the `screen:frames` event
  carries is deliberately **not** shown at all: a rate is not an anomaly,
  and spec §17.3's signal is the drop count.
- **One deliberate picker deviation from the spec remains** — the per-device
  audio level bars (§7.2). Nothing emits a per-device level outside a running
  audio recording, so restoring them is a Rust change first. The other two
  (no Region tab, and the "Screen or window" chooser hint) were closed by
  Phase 3. All three are recorded and reasoned in docs/Gaps.md **GAP-111**.
- **Vault Buddy's own windows are now EXCLUDED from a recording** (spec
  §5.3, Phase 3 — row 16). A build older than Windows 10 2004 has no
  `WDA_EXCLUDEFROMCAPTURE`: there the call logs a warning and the capture
  records with the buddy in frame, by design, and that is the documented
  state rather than a failure. On any supported build, the buddy appearing in
  the footage IS a failure — file it against row 16. Note that the exclusion
  is applied fire-and-forget, so the first frame or two of a capture may
  still contain our windows (docs/Gaps.md GAP-124); that is known, and is not
  what row 16 is asking about.
- **No staging recovery, and nothing collects finished captures either.** A
  capture killed at item 8 leaves its `.part` behind permanently, and every
  capture that stops CLEANLY leaves a `<base>.mp4` + `<base>.json` in the
  same directory with no in-app way to see, open or delete them. Nothing
  sweeps either until Phase 5's `run_screen_recovery` / Phase 6's "Clear
  staged captures" (docs/Gaps.md GAP-115). Clean the staging directory by
  hand between verification runs, and expect it to grow fast at 4K.
- **There is no keyboard-only way to draw a region** (docs/Gaps.md GAP-127).
  The overlay reads pointer events only; Escape cancels, but nothing selects.
  Row 14 is a pointer test by necessity, not by preference.
- **The editor writes nothing into a vault either, and there is no Save.**
  Phase 4 edits the staged capture in place, in
  `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`. Export, the vault
  write and the companion note are all Phase 5; the editor says so in its own
  footer. Do not file "I edited it and nothing appeared in Obsidian".
- **Only the most recently finished capture can be opened.** The Edit action
  reads the store's `lastStaged`, which a restart clears and a new capture
  replaces. Spec §10's staged-capture browser is Phase 5 (docs/Gaps.md
  GAP-115).
- **The stop notification says "Screen capture ready", not "saved", on
  purpose.** Phases 2–4 write nothing into any vault. If you go looking in
  Obsidian for a captured file you will not find one — the file is in
  `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`. That is the
  documented state, not a failure.

## NOT reachable yet

Do not attempt these; they have nothing behind them yet. Listed so an
untested item is never mistaken for a passing one. A row leaves this table
only into a phase's own table above, never into a tick here.

| Item | Arrives in |
| --- | --- |
| Opening a staged capture that is NOT the most recent one (the resume-or-discard browser) | Phase 5 |
| Export exactness (cut boundaries, reordered segments) | Phase 5 |
| The untouched-timeline fast path (remux rather than re-encode) | Phase 5 |
| The vault write, the companion note, and playback of the saved note inside Obsidian | Phase 5 |
| `run_screen_recovery` sweeping an orphaned `.part` | Phase 5 |
| The settings tab, staging size + clear action | Phase 6 |

## Sign-off

- Build / commit verified: ______________________
- Machine (CPU, GPU, Windows build, monitor layout + DPI scales): ______________________
- Date: ______________________
- Outcome: ______________________
