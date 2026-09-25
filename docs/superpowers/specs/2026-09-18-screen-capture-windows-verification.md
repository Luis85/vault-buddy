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

**The deferral is LIFTED.** The user began running this checklist on
2026-09-21, batch by batch, and 23 of its 57 rows (1–55 plus 27a and 27b) now carry a result. A row
whose *Result* is empty has not been run, which is **not** the same as a row
that failed — never convert one to the other. Rows 11 and 12 read DEFERRED by
the author's decision, which is also not a pass.

**All six phases are implemented**, so every row here is reachable against a
build of this branch. What is outstanding for this feature is this file (34
unrun rows) plus docs/Gaps.md, not further implementation.

**Task 59 (tutorial editor) retired the phase-4 editor and the phase-5
export**, and with them the ground 15 of these rows stood on: 19, 20, 21,
27, 27a, 27b, 28, 29, 30, 31, 32, 33, 36, 37 and 38 each now open with
**SUPERSEDED**, naming the tutorial-editor row
(`2026-09-21-tutorial-editor-windows-verification.md`) that replaces it,
and row 34's editor half likewise. They are kept, never deleted — three of
them (29, 37, 38) carry results that are still the record of what the
retired path did. Of the 34 unrun rows, 12 are superseded (19, 20, 21, 27,
27a, 27b, 28, 30, 31, 32, 33, 36), so **22 unrun rows still test something
that exists**. Re-measure the superseded count (15, row 34's half not
counted) with
`grep -cE '^\| ([0-9]+|27a|27b) \| .*SUPERSEDED by tutorial-editor Task 59' docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`.

Re-measure the counts rather than incrementing them; both have been wrong
before from incrementing:

```bash
grep -cE '^\| ([0-9]+|27a|27b) \|' docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md
```

Spec: [2026-09-18-screen-capture-intake-design.md](2026-09-18-screen-capture-intake-design.md)
(§5.2 the region overlay, §5.3 excluding our own windows, §6 the pipeline,
§6.2/6.3 clock and pause, §6.4 fragmented MP4, §6.5 audio, §7.1/7.2 the
picker, §7.3 during capture, §10 staging, §11 the IPC surface, §13 phasing,
§14 error handling, §15 testing).

Run against a build of `claude/screen-capture-intake-g0j49q`
(`npx tauri build --features gpu`, or `npm run test-build` for a dev run).

**Phase 5 needs one thing installed before any of rows 29–36 will run:
ffmpeg.** The export shells out to a user-installed one and is refused
without it, so confirm `ffmpeg -version` answers in a fresh terminal first.
(There IS now an in-app card — Buddy settings → Integrations, beside
Pandoc — with a status line and a Browse for an off-PATH binary, and the
Record Screen picker shows a non-blocking notice. GAP-144 closed; this
paragraph said otherwise for longer than that was true.)

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
| 10 | **4K @ 60 fps for two minutes** (spec §17.3) | **UNBLOCKED 2026-09-21** — the hand-edit this row was deferred over is gone: Vault settings → **Screen** → *Frame rate* → 60 (it applies to the NEXT recording, as the control says). Capture a 4K monitor playing video for 2 minutes, Stop. **Record:** total frames, dropped frames and average fps from `vault-buddy.log`, plus the file's size and whether playback is smooth. **Expect a refusal, not a recording, if this machine is the one from 2026-09-21**: 60 fps raised `MF_E_INVALIDMEDIATYPE` (0xC00D36B4) there, and `bc28ac2` turned that HRESULT into an app-authored message naming the dimensions and frame rate the encoder refused. So this row now asks TWO questions: does 4K60 work at all here, and — if not — **is the refusal the readable one rather than the raw HRESULT?** Write down the message verbatim either way. | **2026-09-21: RUN. 4K60 is REFUSED by this machine's encoder, and the refusal is the readable one — `bc28ac2` verified.** Observed, whole screen at 3840x2400 @ 60: `[ERROR][vault_buddy_screen::sink::imp] screen sink: configure the video stream: … (0xC00D36B4)` then `[ERROR][vault_buddy_lib::screen_commands] screen capture: failed: This machine's H.264 encoder will not record 3840x2400 at 60 fps. Set Frame rate to 30 fps in Vault settings → Screen, or capture a window or a region instead of the whole screen.` The raw HRESULT stays in the log where a diagnostician wants it; what reaches the user names the dimensions, the frame rate and two remedies. **A REGION at 60 fps recorded fine on the same machine**, so the limit is the 3840x2400 frame size at 60, not 60 fps as such — which is exactly what the second remedy offers, and it works. §17.3's throughput measurement (frames, drops, average fps at 4K60) is therefore NOT obtainable here and never will be: carry it to hardware whose encoder accepts the format. (2026-09-19: deferred until a settings surface existed; that landed 2026-09-21 as GAP-103.) |
| 53 | **A window titled after a reserved DEVICE name records normally** (GAP-108) | Open `cmd.exe` and run `title CON` (the console title is then exactly `CON`, which no file dialog will let you produce). Record that window for ~15 s and Stop. Repeat with `title NUL` and `title COM1`. **Record** whether the capture starts, whether Stop stages a file, and the exact file name in `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`. **Expected: an ordinary capture named `YYYY-MM-DD HHmm CON.mp4`** — GAP-108 asserted this failed at `File::create` after recording had started, and the analysis that closed it says the `YYYY-MM-DD HHmm ` prefix makes the name ordinary. That reasoning is Linux-tested; only this row tests it against the Win32 name resolver that actually decides. A file that is NOT created, or one named `... CON_.mp4`, both contradict the close-out. | |
| 54 | **The window-size PREDICTION, across window styles** (GAP-122) | The sink's frame size is frozen before any frame arrives, predicted from `DWMWA_EXTENDED_FRAME_BOUNDS`. Microsoft documents no equality between that and what WGC delivers, so this row collects the evidence. Record ~15 s of EACH: (a) a plain decorated window (Notepad); (b) one MAXIMIZED; (c) a browser (custom-drawn frame); (d) a window with no decorations if you have one; (e) a DPI-scaled window on a 125/150 % display. **Record, per case, whether the capture produced video at all**, and — if the app reports a zero-video capture or the log carries an `undersized` drop — the TWO sizes that line names (delivered vs declared). Both numbers are already logged for exactly this purpose; a case where they differ is the prediction drifting, and is what would justify the first-frame rework GAP-122 describes. | |
| 55 | **A window RESIZED mid-capture** (GAP-122) | Record a window, and part way through drag its edge to make it BIGGER, then SMALLER, then stop. The declared size is fixed at start, so `pacing::usable_frame` rejects every frame that no longer reaches the crop's far edge. **Record** whether the resulting file holds the footage from before the resize, what the dropped-frame chip shows, and whether the app explains itself or just stops producing video. This is the one case where the prediction is KNOWN to go stale, and nothing has ever observed what the user sees when it does. | |

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

**Rows 16, 17 and 43 must be RE-RUN on any build carrying the region
indicator.** They verify the capture-exclusion round trip, and that feature
gained a SIXTH excluded window (`region-indicator`, GAP-165) — so their
earlier passes, taken when `EXCLUDED_LABELS` was `["main"]` alone, do not
carry over. Row 44 is the new window's own version of the same question.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| 11 | **Region on the primary monitor at 100%** | Capture knowledge → *Record screen* → **Region** tab → pick the primary screen → *Select region…* → drag a rectangle over something with readable text → Start → record ~20 s → Stop. Play the staged `.mp4`. **Record the file's pixel dimensions** and whether the recorded content is exactly the rectangle drawn — check all four edges, not just that "it looks right". | **2026-09-21: PARTIAL — the measurement this row exists for was NOT taken.** The picker reported `1840x1684 at (770, 290)` on a 3840x2400 primary, the capture ran 12:17:45 → 12:18:29 (44 s) with **0 dropped frames**, and the user judged the footage "correctly bounded to the region". But the staged capture was DISCARDED at 12:19:55 before `ffprobe` ran, so the file's actual pixel dimensions were never compared against the picker's stated rectangle — which is the whole check. Both reported dimensions are even (consistent with `clamp_to_frame`'s `& !1`) and neither edge clamps (770+1840=2610 ≤ 3840; 290+1684=1974 ≤ 2400), so the file SHOULD be 1840x1684 — "should" is an inference, not a measurement. **Re-run and ffprobe before discarding.** **DEFERRED 2026-09-21 by the author's decision:** the ffprobe comparison is not run in this pass; it is to be run after the remaining Phase 3 work, not before. Not a pass. |
| 12 | **Region at 125 / 150 / 200% display scaling** | Repeat row 11 at each scale (Settings → System → Display → Scale), relaunching the app after each change. **Record, per scale: the rectangle drawn (approximate logical size), the file's actual pixel dimensions, and the ratio between them.** The expected ratio is the scale factor — this row IS the phase gate. An off-by-scale bug shows here as a file that is exactly 1/1.5 or 1/2 of the expected size, or a recording offset from the rectangle by the origin times the scale. | **DEFERRED 2026-09-21 by the author's decision:** the 125 / 150 / 200 % scaling pass is not run in this pass; it is to be run after the remaining Phase 3 work, not before. Not a pass — this row is still the Phase 3 gate, and a deferred result here means the region path is unproven across DPI scales. |
| 13 | **Region on a SECONDARY monitor, at a different scale from the primary** | With two monitors at different scales, select a region on the non-primary one. **Record which monitor the overlay appeared on, and whether the recorded content matches the rectangle.** This is the mixed-DPI case `core::screen_geometry`'s module doc warns about; if any row fails, it will be this one. | **2026-09-21: BLOCKED — NOT RUNNABLE on the verification machine, which has a single monitor.** This is a hardware limitation, not a deferral and not a pass: the one row most likely to fail is the one this pass structurally cannot reach. `core::screen_geometry::to_physical` uses a single scale factor and its module doc says why a mixed-DPI virtual desktop cannot be described that way, so the risk is real and remains entirely unverified. Needs a second monitor at a different scale from the primary; carry it forward and do not let a green Batch 2 be read as covering it. |
| 14 | **The overlay's own behaviour** | Open *Select region…* and, in separate attempts: (a) press Escape; (b) click once without dragging; (c) drag a tiny box (< 8 px); (d) drag from bottom-right to top-left; (e) complete one selection, then open *Select region…* a SECOND time in the same app run and draw another. **Record for each:** whether the overlay closed, whether the panel came back with focus, and whether the picker shows a region. Expected: (a)(b)(c) no region, (d) the same region as an equivalent top-left-to-bottom-right drag, (e) a normal second selection — (e) is the `region:begin` re-arm, which no automated gate on any platform can reach. | |
| 15 | **The panel does not auto-hide during a selection** | With the panel open, start a region selection and leave the overlay up for ~10 s before dragging. **Record whether the panel is still open underneath afterwards.** This is `DIALOG_ACTIVE`; a failure here loses the picker's state mid-selection. | |
| 16 | **`WDA_EXCLUDEFROMCAPTURE`** (spec §5.3, amended 2026-09-21) | Start any screen capture with the BUDDY plainly in frame. **Record whether it is visible to you during the capture (it must be) and whether it appears in the played-back file (it must not).** The panel, bubble and editor are IN the recording by design since GAP-166 — record that they are, and do not file it. Note the Windows build number. | **2026-09-21: PASS for the buddy, on the GAP-166 fix rebuild** (`EXCLUDED_LABELS = ["main"]`): absent from a Win+Shift+S snip during the capture and from the played-back file, visible to the user throughout. Windows build number not noted — capture it on the re-run. Re-run on the next installer build to close. |
| 17 | **The exclusion is lifted afterwards** | After the capture from row 16 stops, record the same screen with a **different** tool (Xbox Game Bar `Win+Alt+R`, or a Teams screen share). **Record whether the BUDDY is visible in THAT recording.** It must be (the other windows are never excluded now, so they prove nothing here). A failure here is the leaked-exclusion case the structural test exists to prevent, and is invisible from inside the app — nothing looks wrong to the user, because the buddy is still on their own screen. | **2026-09-21: PASS on the GAP-166 fix build.** A Win+Shift+S snip taken AFTER the capture stopped shows the buddy present, so the exclusion is lifted at stop and nothing leaks into later captures. Together with row 16 this is the exclusion's full round-trip verified on hardware; both re-run on the next installer build to close. |
| 18 | **Resolution changed between selecting and starting** | Select a region near the right or bottom edge, then change the display resolution to something smaller **before** pressing Start. **Record the message.** Expected: a refusal naming the source as gone, not a started capture. | |

| 44 | **Other applications survive a region capture** (GAP-165 / GAP-166) | Record a region, stop it, and — with Vault Buddy **still running** — click Explorer's toolbar and Notepad's menu bar. **Record whether each responds.** This is GAP-166's exact symptom against the SIXTH excluded window, which is the one thing the premise probe could not test (it probed `panel`, an existing window). A failure here means the indicator must lose its exclusion, and the feature goes with it — do not work around it. | |
| 45 | **The panel is not hidden when the indicator appears** | With the panel open, start a region capture and touch nothing for ~10 s. **Record whether the panel is still open underneath.** `show()` on an always-on-top window may activate it despite `focus: false`; the activation would blur the panel and `schedule_focus_out_check` would hide it mid-capture. Undeterminable off Windows, and not covered by row 15 (that one is the overlay, during SELECTION; this is the indicator, during CAPTURE). | |
| 46 | **The border appears and traces the rectangle** | Record a region ~in the middle of a monitor. **Record** whether a border appears at all, and whether it traces the drawn rectangle **on all four edges** — not merely that it "looks right". The border is 2 px, drawn inside the window's own bounds, and overlaps the recorded area by design. | |
| 47 | **The border is click-through** | With the capture running, click something underneath the border's edge, and then something inside the region. **Record whether each click reached the application underneath.** This is `set_ignore_cursor_events(true)`; without it the indicator swallows every click over the whole region for the whole capture. | |
| 48 | **The border is not in the recording** | Play back the staged file from row 46. **Record whether the border appears in any frame.** This is what the exclusion buys, and without it the feature is worse than nothing — a border burned into the footage the user is keeping. | |
| 49 | **The border is on the right monitor** | Record a region on a NON-primary monitor. **Record which monitor the border appeared on.** This is `indicator_bounds`' monitor offset; a failure puts the border on the primary while the capture records the secondary — precisely the confusion the indicator exists to remove. **Expected BLOCKED on the current verification machine** (single monitor), exactly like row 13. Record it as blocked, not as passed: the arithmetic is covered by `indicator_bounds`' own Linux tests, and nothing else about this row is. | |
| 50 | **The border tracks pause** | Pause the capture, then resume it. **Record the border's colour in each state**, and whether it stayed in place across both. Expected: the capture bar's own red while recording, amber while paused. | |
| 51 | **The border goes when the capture does** | Stop the capture. **Record whether the border disappeared.** Then repeat for a capture that SELF-FINALIZES (close the recorded source mid-capture) and one that FAILS. All three teardowns funnel through `clear_active_screen`, but only the self-finalize exercises the path that no command call reaches — it is the one most likely to strand a border. | |
| 52 | **Display scaling** | Repeat row 46 at 125 / 150 / 200 %, relaunching the app after each change. **Record, per scale, the rectangle drawn and whether the border still traces it.** Pairs with row 12 — the same arithmetic seen from the user's side rather than from `ffprobe`. | |


## Covered by Phase 4

Phase 4 builds the editor window. **The entry point has now landed** (Task 7):
after a screen capture stops, the capture bar on the panel's list view shows
the finished capture with an **Edit** button, and that is what calls
`open_capture_editor`. Every row below is therefore reachable by hand. (Phase
5 added the SECOND door, `StagedCaptureList` at the top of the Record Screen
picker, which reaches every staged capture rather than only the most recent —
so an older staged file no longer needs a fresh recording to get at.)

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
| 19 | **The preview actually loads its video** (spec §8.2, the asset protocol) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T8 (the tutorial editor's preview plays a staged capture through `editor_media_url`) instead; kept, not deleted, so its history stays. Stage a capture (any row 1–3 recording will do), open it in the editor, and **look at the video frame before touching anything**. Expected: the first frame of the recording is visible and **Play** plays it with audio. **Record: whether a frame appeared at all**, and — in the editor window's DevTools (`F12`) — the `<video>` element's resolved `src` plus any failed request on the Network tab. A blank black frame with a `net::ERR_*` or a 403 on an `asset.localhost` request is the failure this row exists to find; so is a `src` that is a bare file name rather than a full `…\screen-captures\<base>.mp4` path. | |
| 20 | **An edit survives closing and reopening the editor** (spec §10 Resume, C-1) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T19 and T20 (the tutorial editor's edits survive a close and a reopen) instead; kept, not deleted, so its history stays. Open a staged capture, scrub to ~4 s, **Split**, select the FIRST block, **Delete** — the first 4 s are now cut. Close the editor window. Open the SAME capture again: it must come back showing the trimmed timeline, not the whole recording. Now make any edit and press **Undo**. **Record: what `<base>.json`'s `timeline` field holds afterwards** (open it in a text editor — it is beside the `.mp4` in the staging dir). Expected: the trim, i.e. one segment starting at ~4000. A `timeline` that is absent or `null` there is the C-1 erasure: the edit is still on screen, but the file says the capture was never touched, and Phase 5's fast path would export the whole recording. | |
| 21 | **A failed save is visible** (spec §10 disk pressure, I-7) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T20 (the close guard's Save; a failed project save says "Save failed" in the header) instead; kept, not deleted, so its history stays. Hard to stage honestly; run it only if it is cheap on the machine at hand — make the staging directory unwritable (deny your own user Write on `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`), then make an edit in an already-open editor. Expected: an amber banner above the preview saying the edit could not be saved, the edit still on screen, and the editor still usable. **Record whether the banner appeared**, and restore the permission afterwards. | |
| 22 | **The editor opens on a real capture, as a real window** (Task 7's entry point) | Record ~30 s, Stop, and wait for the capture bar to change from *Recording* to the finished row naming the capture. Click **Edit**. **Record:** whether the window opens at all; whether it has a normal title bar and can be resized and maximised; whether it appears in the taskbar and in Alt+Tab (it must — it is the only window of ours that is `skipTaskbar: false`); and whether the video plays. A window that opens but shows a black frame is row 19's failure, not this one. | |
| 23 | **The editor's own X hides it rather than quitting anything** (`window_close.rs`, I-1/I-2) | With the editor open and the buddy running, click the editor's titlebar **X**. **Record:** whether the app is still running, whether the buddy is still on screen, and whether clicking **Edit** again brings the editor back **showing the same capture** (it must — the window is hidden and reused, never destroyed, which is what `editor:open` exists for). Then stage a SECOND capture and click **Edit**: the editor must come back showing the NEW capture, not the old one. That second half is the one thing no test on any platform reaches. | |
| 24 | **Hide to tray does not take the editor** (`COMPANION_LABELS`) | With the editor open and a capture loaded in it, tray → **Show / Hide**. **Record:** whether the buddy and the panel disappear, and whether the editor stays. The editor must stay — hiding it from the outside would strand an in-progress edit off-screen with no way back. Then tray → **Show / Hide** again and confirm the buddy returns without disturbing the editor. | |
| 25 | **Quit destroys the editor** (`ALL_WINDOW_LABELS`, `Chrome_WidgetWin_0`) | With the editor open, tray → **Quit**. **Record:** whether the process really exits (check Task Manager — not just that the windows vanished), and whether `vault-buddy.log` carries `Failed to unregister class Chrome_WidgetWin_0`. It must not. A live WebView2 window at exit fails that unregister every time, which is the whole reason the editor is on the destroy list but not the hide list. | |
| 26 | **The editor is not in its own recording** (spec §5.3, and GAP-116's other half) | Start a capture of the whole screen with the editor open and plainly in frame. **Record:** whether the editor is visible to you during the capture (it must be) and whether it appears in the played-back file (it must not). This is the first excluded window that is NOT `skipTaskbar`, so it is the first real test of `capture_exclusion`. **While the picker is open, also check the Window tab**: Vault Buddy's editor must NOT be offered as a capture source — it is the first window of ours that enumerates at all, so this is the first time the title filter actually runs (GAP-116). | |
| 27 | **Split, delete, reorder, undo, redo — and the keyboard** (spec §8.1, §8.2) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T6 and T58 (drag, trim, Escape and the keyboard in the tutorial editor) instead; kept, not deleted, so its history stays. On a ~30 s capture: scrub and **Split** twice, **Delete** a middle block, drag a block to reorder it (drag one **leftwards** as well as rightwards — a leftward drop is a different code path and it used to be the untested one), then **Ctrl+Z** all the way back to the whole recording and **Ctrl+Shift+Z** (and separately **Ctrl+Y**) forward again. **Record:** whether the strip and the preview agree after every step (scrub to a moment and check the frame is the one the strip says), whether the playhead lines up exactly with the block boundary it marks, whether **Undo** is ever disabled while an edit is still visible on screen, and whether a split on an exact block boundary is correctly a no-op that does NOT consume an undo step. | |
| 27a | **The selection survives an edit that re-indexes the strip** (spec §8.1; the phase-4 review's I-1 — the one editor bug that destroyed content) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T6 (the tutorial editor's selection is Rust's, re-indexed by every acknowledged edit) instead; kept, not deleted, so its history stays. **Select LAST, act FIRST is the whole point of this row, so do it in this order.** Click the **last** block to select it (it highlights). Now scrub into the **first** block and **Split** there — the selection must still be on the same block, which is now one place further right, NOT on whatever block took its old number. Click **Delete** and check the footage that disappeared is the one that was highlighted. Then: select a block, **Ctrl+Z**, **Ctrl+Shift+Z**, and confirm the highlight is still on the same footage each time. Finally select a block and **Split inside that block** — the selection must CLEAR (neither half is the block you selected) and **Delete** must go disabled. **Record:** each of those four observations separately. A highlight that jumps to a different block, or a Delete that removes footage you did not select, is this row failing. | |
| 27b | **The playhead is clamped by an edit that shortens the film** (the phase-4 review's m-3) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T6 (the tutorial editor's playhead) instead; kept, not deleted, so its history stays. Scrub to near the END of the capture, then select and **Delete** a block that sits before the playhead. **Record:** where the playhead and the scrub thumb are afterwards (they must be at the new end of the film, not past it), and whether scrubbing and splitting work normally straight afterwards without having to drag the scrubber first. | |
| 28 | **Edits survive a crash** (spec §10, save-on-every-edit) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T19 (kill mid-edit, relaunch, Resume) instead; kept, not deleted, so its history stays. Open a staged capture, **Split** twice, then kill the process from Task Manager (**End task**) — do not close the editor first. Relaunch, record a throwaway capture to get the bar back… or, quicker, read `<base>.json` in the staging dir directly. **Record:** how many segments the `timeline` field holds. Expected: three — both splits, because every edit is written through immediately. Anything fewer means the sidecar write is not landing per-edit and spec §10's "a crash loses at most the last operation" does not hold. | |

## Covered by Phase 5

Phase 5 is where this feature first touches a vault: a staged capture is
exported through a **user-installed ffmpeg** and saved as a playable `.mp4`
plus a companion note — the ninth sanctioned vault write. An unedited
capture takes a no-re-encode `-c copy` remux; an edited one is a single
`filter_complex` pass. Interrupted work is swept at startup
(`run_screen_recovery`), and every staged capture is reachable from the
Record Screen picker with *Resume editing* or *Discard*.

**This phase is the first in the whole feature to carry executable
end-to-end proof.** `rust-core` installs ffmpeg and runs
`screen/tests/export_roundtrip.rs`: synthesize a clip, apply a real
`Timeline`, read the output back and check both its length and that each
block still carries its own colour and its own tone. So rows 29–36 are not
carrying the whole burden the way rows 11–28 did. What they carry is what
Linux cannot reach: a **Windows** ffmpeg build meeting a **Media
Foundation** fragmented MP4, the disk probe's Windows arm, and every
judgement about what the user actually sees.

**Before running any of rows 29–36: install ffmpeg and confirm it is on
PATH** (`ffmpeg -version` in a fresh terminal). Without it every row below
fails at Save with:

> Saving a screen capture needs ffmpeg, which is not installed. Install it,
> then set its location in Buddy settings → Integrations if it is not on
> your PATH.

That screen **exists** (`FfmpegSettings.vue`: status, Browse, Recheck), so
the message is actionable and following it is not a finding. This paragraph
previously said the opposite — that the message pointed at a screen that did
not exist — which would have had a runner filing a defect against a closed
gap. Observing the refusal ONCE, deliberately, is still worth doing as part
of row 29; what is NOT worth doing is re-filing it.

**What is still missing is the PRE-FLIGHT, and it is a different thing:**
nothing checks ffmpeg when the EDITOR opens, so a capture resumed from the
staged list meets the dependency only at Save (GAP-144's largest residual).
The picker's notice deliberately does not gate Start — recording and editing
work fine without ffmpeg, and only the save needs it.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| 29 | **The untouched fast path, and ffmpeg against a real fMP4** (spec §8.3; the highest-value row in the phase) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T62 (an untouched capture rendered and published end to end) and T26 (the R1 remux) instead; kept, not deleted, so its history stays. Record ~60 s. Open the editor, **change nothing**, press **Save to vault**. **Record:** how long the save took in seconds (a re-encode of 60 s would be plainly slower — seconds, not minutes); the saved `.mp4`'s byte size beside the staged one's; and whether it plays. Then check `vault-buddy.log` for the line `screen export: saved … (remuxed: true)` — `remuxed: false` means the fast path did not take, which is the one thing this row exists to find. Windows ffmpeg meeting Media Foundation's fragmented output is proven nowhere but here: if `-c copy` cannot remux it, this is where that shows. **Also do the no-ffmpeg case once, first**, before installing it: press Save and write down the exact message and confirm Buddy settings → Integrations really holds the ffmpeg card it names. | **2026-09-21: PASS on the fast path.** `screen export: saved 2026-09-21 1152 Region on to … (remuxed: true)` — **`-c copy` remuxes Media Foundation's fragmented output on Windows ffmpeg 9.0.1**, which was proven nowhere before this line. Saved file plays. **TWO sub-questions still open:** (a) the SAVE DURATION was not isolated — the log gives capture finalize at 11:53:33 and the save at 11:54:54, but the 81 s between them includes row 37's window-resizing, so it is not a measurement; re-run timing Save alone. (b) The staged vs. saved BYTE SIZES were not compared, which is the other half of "did it really remux". (c) The **no-ffmpeg sub-case was NOT captured** — ffmpeg was already installed before the run, and the opportunity is gone on this machine short of removing it from PATH. |
| 30 | **An edited export matches the preview** (spec §8.1/§8.2; the two implementations of the segment algebra, docs/Gaps.md GAP-136) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T23 and T28 (a render of real edits, watched and Reviewed; the preview-vs-render gap is GAP-173) instead; kept, not deleted, so its history stays. Record ~60 s of something with a **visible running clock**. In the editor: trim the first 20 s, delete a middle block, and drag one block to reorder it. Note the clock value the preview shows at each cut. Save, then open the result in a player. **Record: the clock values at each cut in the EXPORTED file, beside the values the preview showed.** This is the only comparison anyone has ever made between the TypeScript algebra the user watched and the Rust one the export planned on. Also record the exported file's total duration against the editor's own readout. | |
| 31 | **The companion note** (spec §9) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T62 and T29 (the Tutorial note a Publish writes) instead; kept, not deleted, so its history stays. Open the saved note in Obsidian. **Record:** whether the video plays **inside Obsidian** from the note's embed; whether `duration` is the EXPORTED length and not the original (a capture trimmed from ten minutes to two must not claim ten); whether `resolution` matches the file's real pixels; and whether `source` survived a window title containing a colon or a quote — repeat the recording once with such a title if none is at hand. Also record whether the note's file name matches the video's. | |
| 32 | **Never clobber** (the ninth vault write's discipline) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T29 (a Publish into a same-name file) instead; kept, not deleted, so its history stays. Save a capture. Then record and save a second one **in the same minute with the same window title**, so both derive the same base name. **Record both file names, and both note names.** The second must carry ` (2)` on BOTH, and the first must be byte-for-byte untouched (check its size and play it). A second pair that landed as `Demo.mp4` + `Demo (2).md` — video and note disagreeing about which suffix they took — is the pairwise-reservation failure this is really testing. | |
| 33 | **Cancel** (spec §14: a cancel is not a failure) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T27 and T28 (cancelling a render) instead; kept, not deleted, so its history stays. Start a save of a LONG edited capture (so the re-encode path runs for a while) and press **Cancel** at roughly 30%. **Record:** whether the editor returns to normal with no error banner and Save offered again; whether the staged capture is still listed in Record Screen; whether the vault gained ANY file (check the target folder); and whether a `.export.mp4.part` was left in `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`. Expected: no vault file, no leftover temp, staged capture intact. Then press Save again and confirm it completes. | |
| 34 | **Discard** (spec §10: nothing is ever deleted silently) | **Its EDITOR half is superseded by tutorial-editor Task 59 — run T63 (Discard project, then the staged capture) instead; the Record Screen list half still stands.** Discard a staged capture, from the editor's **Discard** and, separately, from a row in the Record Screen list. **Record for each:** whether it takes two clicks and whether the first click can be backed out of; whether `<base>.mp4` AND `<base>.json` are both gone from the staging directory; and whether the panel's capture bar stops offering **Edit** for it. In the list, also **arm one row's Discard and then click a DIFFERENT row's** — the second must still require its own confirm. | |
| 35 | **Recovery** (spec §10, `run_screen_recovery`) | Start a capture, let it run ~60 s, then kill the app from Task Manager (**End task**) — do not stop the capture. Wait a minute, relaunch, and open Record Screen. **Record:** whether the orphaned `.part` appears as a staged capture; whether it plays; and roughly how much of the recording it holds. Then repeat with a capture killed after only a second or two: a `.part` with no fragments should be swept rather than offered as a zero-length recording. Also drop a file of your own named `.something.mp4.part` into the staging directory beforehand and **record whether it survives** — it must. | |
| 36 | **The disk check** (spec §10 disk pressure) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T30 (a full disk during Publish) instead; kept, not deleted, so its history stays. Run only if cheap to stage on the machine at hand. Fill the vault's volume until less remains than the export's estimate, then press **Save**. **Record the exact message.** Restore the space and confirm the same capture then saves. If filling the volume is not practical, record that this row was not attempted rather than leaving it blank — `disk::free_bytes`'s Windows arm is type-checked and executes in no test anywhere (docs/Gaps.md GAP-140), so this row is its only evidence. | |
| 37 | **The editor window at several sizes** (the 2026-09-21 hardware session's two layout defects) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run `tests/e2e/editorShell.spec.ts` (the tutorial editor at four window sizes) and T59–T60 instead; kept, not deleted, so its history stays. Open the editor and drag the window **wider**, then **maximise** it, then make it **short**. **Record at each size:** whether the timeline strip keeps its full height (it collapsed to a hairline as the window widened, because the unbounded preview took the space); whether **Save to vault** and **Discard** stay on screen and clickable (maximised, they were pushed past the bottom edge with nothing to scroll); and whether the video keeps its aspect ratio rather than stretching. Then make the window short enough that the header, strip, verbs and bar alone will not fit, and confirm the column **scrolls** to them. Reported on a 3840x2400 display, where the effect is largest. | **2026-09-21: PASS.** All sizes worked as intended on the reporting machine — the strip keeps its height, Save and Discard stay reachable, the picture is not stretched, and a short window scrolls. This is the hardware confirmation of `5018dc5`; `tests/e2e/editorLayout.spec.ts` covers the same four geometries in Chromium on every CI run, which is evidence but not WebView2. |
| 38 | **A save really lands** (the same session's export failure) | **SUPERSEDED by tutorial-editor Task 59**, which retired the phase-4 editor and the phase-5 export this row tested — run T62 instead; kept, not deleted, so its history stays. This is row 29 re-run for a different reason and must not be skipped as a duplicate. Every export failed before writing a byte — ffmpeg picks its muxer from the output extension and the export writes to `.<base>.export.mp4.part`, which names no format. **Record:** that Save completes at all, and that the vault holds a playable `.mp4`. CI proves this on Linux now, against the real `.export.mp4.part` name; what this row adds is the **Windows** ffmpeg build, which is a different binary with a different muxer set. | **2026-09-21: PASS — the ninth vault write lands on Windows.** ffmpeg 9.0.1-full (gyan.dev, gcc 16.1.0, `--enable-mediafoundation --enable-libx264`). A 38 s region capture (11:52:55 start → 11:53:33 finalize, **0 dropped frames**) exported to `C:\Projects\vault-buddy\Screen Captures\2026-09-21 1152 Region on.mp4`, and the note opened via `obsidian://open?vault=ed405dcea029250c&file=Screen%20Captures%2F…`. Played fine in the vault. `GAP-161` is confirmed fixed against a real Windows ffmpeg, not just CI's Linux one. |
| 39 | **The per-vault Screen settings surface** (GAP-103, and the unreadable-dropdown defect on top of it) | Vault settings → **Screen** (second, beside Recording). **Record:** whether all seven fields are there (capture folder, dated/flat, quality, frame rate, companion note, extra frontmatter, body template), and — opening **Quality** and **Frame rate** — whether the OPEN dropdown is readable. Both were native `<select>`s, the only ones left in the repo, and rendered white-on-white on this machine. Also confirm the two honesty notes are ON the controls: quality applies only to an EDITED save, frame rate only to the NEXT recording. | **2026-09-21: PASS.** "All there and working" — the tab exists, all seven fields present, and both dropdowns readable after the `SelectMenu` conversion (`26fb9e1`). GAP-103 confirmed closed on hardware. |
| 40 | **The Window tab is a control, not a wall** (the long-list defect) | Capture knowledge → *Record screen* → **Window** tab with a dozen or more windows open. **Record:** whether it is ONE dropdown row rather than a card per window, whether entries stay distinguishable (the label carries the process name, which is often the only thing separating three "… - Visual Studio Code" entries), and whether the **Screen** tab still shows CARDS — that asymmetry is deliberate (monitors are few and their resolution/primary line is what tells them apart; open windows are unbounded). | **2026-09-21: PASS.** "All there" — one dropdown on the Window tab, cards retained on the Screen tab (`bad0aa6`). |
| 41 | **A monitor with no usable name** (GAP-164, found BY this pass) | Not a step so much as something to read: after any REGION capture, look at the source title in the log, in the capture bar, and in the saved file name. **Record the title verbatim.** It must name a monitor — `Region on DELL U2720Q`, or `Region on Screen 2` when Windows gives no name. `Region on ` with nothing after it means `display_label`'s fallback is not firing, which is what shipped until 2026-09-21. | **2026-09-21: FAILED, then FIXED — this row exists because of it.** Observed `screen capture: started (Region on )` and a file committed to the vault as `2026-09-21 1152 Region on.mp4`. `Monitor::name()` returned `Ok("")`; all three `.unwrap_or_else(|_| …)` sites caught `Err` only. Fixed by the pure `source::display_label`. **2026-09-21, second run: PASS on the fixed build (`46ccd05`).** `screen capture: started (Region on Screen 1) -> …\.2026-09-21 1217 Region on Screen 1.mp4.part`, and the picker row reads `Region on Screen 1 / 1840x1684 at (770, 290)`. `display_label`'s fallback fires on the real machine, so `Monitor::name()` really was the `Ok("")` source and the three `cfg(windows)` call sites — which execute in no automated test anywhere (GAP-117) — are exercised. |
| 42 | **Windows' yellow capture border during a REGION capture** (GAP-165) | Start a REGION capture and look at the screen, not the file. **Record:** what Windows outlines (expected: the WHOLE monitor, because WGC captures the monitor and we crop on the CPU), whether anything marks the rectangle actually being recorded (expected today: nothing), and — separately — whether the RECORDING is correctly cropped. The two answers are independent, and conflating them is the trap: correct footage with misleading feedback is this row's expected state, not a pass. | **2026-09-21: CONFIRMED as described.** Yellow border around the whole screen, no region box, footage correctly bounded. Not a capture defect — `docs/superpowers/specs/2026-09-20-region-capture-indicator-design.md` is an APPROVED design for the indicator that was never implemented and was tracked nowhere until GAP-165. Re-run this row once that lands; until then the expected result is exactly what was observed. |
| 43 | **Vault Buddy must not break other applications' input** (GAP-166) | With Vault Buddy running, use another app and try its toolbar and menus. **Run the discriminators in this order, because the first two settle it:** (a) select a region and CANCEL it (Escape / a click with no drag) without recording — separates the overlay from the capture; (b) a WHOLE-SCREEN capture that succeeds (set Frame rate to 30 fps first, since 4K60 is refused on this machine) — tests whether it is region-specific at all, because a region IS a display capture; (c) a WINDOW capture; (d) does Windows' yellow border persist after the capture stops; (e) granularity — does the client area away from the toolbar still click, does the address bar still type, does hovering a toolbar button still highlight, does Alt+F still open the menu. **Record** which app, which control, and whether tray → **Quit** restores it. This row exists because a companion app that quietly disables another program's UI is worse than any defect inside our own surfaces, and it is invisible from inside our app. | **2026-09-21: FAILED, still unlocalised.** File Explorer's top toolbar stops accepting clicks — the WHOLE toolbar — and **Notepad behaves the same way**, so it is not Explorer-specific. **Minimize / maximize / close keep working** on the affected windows, which rules out any of our surfaces covering that strip. Reported as happening **only after a REGION capture**; closing the editor does not fix it, hiding every window to the tray does not fix it, only **Quit** does. Exonerated by code since: the editor (`alwaysOnTop: false`, and its ✕ only hides), the 1 s metronome (`window_upkeep_tick` returns early unless `main` is visible), every input-affecting Win32 call (there are none in the shell), and the overlay (one unconditional cleanup hides it, and `COMPANION_LABELS` hides it again). **Caution on "only region":** a region IS a display capture, and this machine's whole-screen capture was REFUSED at 4K60 (row 10), so no successful whole-screen capture has ever run here — step (b) is what decides it. **Second sitting, same day:** (a) cancelling a region selection does NOT reproduce it — the overlay is out; (b) a WHOLE-SCREEN capture at 30 fps, editor never opened, DOES reproduce it — **not region-specific; triggered by starting a capture**; (d) the yellow border does NOT persist; the log has no `capture exclusion:` line in either direction; a snip taken while broken shows the editor rendering normally (buddy not snipped). Still to run: (e) granularity, plus a REFUSED capture (4K60), a capture with no audio devices, (c) a window capture, and a snip of the buddy. **Third sitting, same day:** on a fresh launch, Resume → edit on a waiting capture was CLEAN — the editor is out as a trigger too. A new screen recording, started with the editor visible and showing content: at stop the editor was on screen BLANK (its content had stopped painting; clicking Edit reloaded it) and the toolbars were dead — **and they do not react to hover either**, so pointer input is not reaching them. Nothing in the stop path touches the editor (`screen_commands.rs:324` shows `main` at start), so it was not shown blank; it stopped painting during the capture. Closing it afterwards still did not help; Quit did. **Now pending, independent, any order:** (1) does it break DURING recording or only after stop, and does the editor blank at start; (2) does an AUDIO recording break it (audio never calls `capture_exclusion::apply`); (3) does a REFUSED capture (whole screen @ 60 fps) break it. **Fourth sitting:** the trigger is the STOP. With the editor open and showing a capture, a new screen recording was fine DURING recording (toolbar working, editor content showing); at stop, in one instant, the editor went white and the toolbar died (min/max/close still working). The whole start path is exonerated. **Now pending, independent, any order:** (1) an AUDIO recording, stopped — runs the toast and tray change only; (2) a REFUSED screen capture (60 fps) — runs affinity clear + toast + MF, no WGC session; (3) a screen capture with Do Not Disturb ON — toast runs, display withheld. The stop-path map in GAP-166 says what each answer leaves standing. **Fifth sitting — ROOT CAUSE CONFIRMED by rebuild:** with `set_affinity` in `capture_exclusion.rs` made a no-op, a screen capture recorded and stopped with the editor open left the editor PAINTED and Explorer's toolbar ALIVE. Both symptoms are the `SetWindowDisplayAffinity` round-trip on our WebView2 windows. Not yet known: which window's affinity does the cross-process damage — one more rebuild (exclude `main` alone) decides whether the exclusion survives for the buddy or is removed. Still FAILED until the fix lands and this row is re-run on it. **Sixth sitting — FIXED and verified on the fix rebuild:** with `EXCLUDED_LABELS = ["main"]` (the committed fix, hand-applied to a dev build), a screen capture recorded and stopped with the editor open left Explorer's toolbar ALIVE, the editor painted, and the buddy absent from both the snip and the recording. PASS on the rebuild; re-run on the next installer build to close, since a hand-edited dev build is not what users install. See GAP-166. |

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
- ~~**No staging recovery, and nothing collects finished captures
  either.**~~ **RETIRED — both landed.** `run_screen_recovery` sweeps the
  staging directory at startup (promoting an orphaned `.part` that holds real
  footage, deleting an abandoned export temp, never touching anything
  Foreign), `StagedCaptureList` shows every staged capture with Resume and
  Discard, and Buddy settings → System has a size readout with **Clear staged
  captures**. What is still true: nothing EXPIRES a completed staged capture,
  deliberately (docs/Gaps.md GAP-115), so expect the directory to grow fast
  at 4K and clear it between runs — now from inside the app.
- **There is no keyboard-only way to draw a region** (docs/Gaps.md GAP-127).
  The overlay reads pointer events only; Escape cancels, but nothing selects.
  Row 14 is a pointer test by necessity, not by preference.
- ~~**The editor writes nothing into a vault either, and there is no
  Save.**~~ **RETIRED — Phase 5 shipped the export**, and **tutorial-editor
  Task 59 retired that export in turn**: a staged capture now reaches a vault
  through the tutorial editor's **Render video** and **Publish to vault…**
  (the tenth sanctioned vault write). Rows 29–36 are superseded accordingly;
  the tutorial-editor checklist's T23–T31 and T62–T63 cover the new path.
- ~~**Only the most recently finished capture can be opened.**~~ **RETIRED.**
  The capture bar's **Edit** still reads `lastStaged` (cleared by a restart,
  replaced by a new capture), but `StagedCaptureList` in the Record Screen
  picker reaches every staged capture. Both go through the same
  `open_capture_editor`.
- **The stop notification says "Screen capture ready", not "saved", on
  purpose.** A stop STAGES the capture; the vault write happens later, when
  you render it in the editor and publish the render (since Task 59 the
  toast says "Open it in the editor to render and publish it"). So a just-stopped capture is in
  `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures` and not yet in any
  vault. That is the documented state, not a failure, and the wording is
  pinned by a test precisely so it never claims a save that has not happened.

## NOT reachable yet

Do not attempt these; they have nothing behind them yet. Listed so an
untested item is never mistaken for a passing one. A row leaves this table
only into a phase's own table above, never into a tick here.

Phase 5 emptied five rows out of this table and into the Phase 5 section
above (rows 29–36): opening a staged capture that is not the most recent one,
export exactness, the untouched fast path, the vault write and its note, and
`run_screen_recovery`. Phase 6 emptied the rest of its own: the settings tab
(GAP-103) and the staging size readout + bulk clear (GAP-115) both landed, so
**nothing below is waiting on a phase of this feature** — every remaining row
is either already shipped and needing verification, or explicitly
out-of-scope work with no phase attached. What is left:

| Item | Arrives in |
| --- | --- |
| ~~The screen-capture settings tab~~ — **LANDED 2026-09-21** (GAP-103): Vault settings → **Screen**, all seven `screen_*` fields settable. Verify it rather than expecting a hand-edit | — |
| An ffmpeg **pre-flight in the EDITOR** — the status card and Browse LANDED (Buddy settings → Integrations) and the picker shows a notice, but a capture resumed from the staged list still meets the dependency at Save (docs/Gaps.md GAP-144's remaining residual) | unscheduled |
| Renaming a saved capture, or re-exporting one (docs/Gaps.md GAP-142) | unscheduled |
| Saved screen captures in the Recordings browser, and transcribing them (docs/Gaps.md GAP-143) | unscheduled |

## Sign-off

- Build / commit verified: ______________________
- Machine (CPU, GPU, Windows build, monitor layout + DPI scales): ______________________
- Date: ______________________
- Outcome: ______________________
