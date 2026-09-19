# Screen Capture — Windows Verification Checklist (Phase 2)

Manual end-to-end verification on a real Windows machine. This is **Phase
2's gate**, not a formality: no CI runner can record a screen, so every
claim below is one the automated gates structurally cannot make. `rust-core`
proves the pure modules on Linux and `windows-app` now compiles and runs the
crate on Windows (GAP-102), but the `cfg(windows)` arms of `sink.rs`,
`source.rs` and `session/windows_session.rs` are exercised by nothing
automated anywhere — they are exercised **here**.

Spec: [2026-09-18-screen-capture-intake-design.md](2026-09-18-screen-capture-intake-design.md)
(§6 the pipeline, §6.2/6.3 clock and pause, §6.4 fragmented MP4, §6.5 audio,
§7.3 during capture, §10 staging, §13 phasing, §14 error handling,
§15 testing).

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

## Known Phase-2 absences (do not file these as failures)

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
- **Three deliberate picker deviations from the spec** — no per-device audio
  level bars (§7.2), no Region tab, and the "Screen or window" chooser hint
  where §7.1 says "Screen, window, or region". Each is recorded and reasoned
  in docs/Gaps.md **GAP-111**; Phase 3 restores them deliberately.
- **Vault Buddy's own windows DO appear in a Phase 2 recording.** This is
  expected, not a bug: `WDA_EXCLUDEFROMCAPTURE` (spec §5.3) is Phase 3. If
  the buddy is in frame in a recording made here, that is the documented
  state.
- **No staging recovery, and nothing collects finished captures either.** A
  capture killed at item 8 leaves its `.part` behind permanently, and every
  capture that stops CLEANLY leaves a `<base>.mp4` + `<base>.json` in the
  same directory with no in-app way to see, open or delete them. Nothing
  sweeps either until Phase 5's `run_screen_recovery` / Phase 6's "Clear
  staged captures" (docs/Gaps.md GAP-115). Clean the staging directory by
  hand between verification runs, and expect it to grow fast at 4K.
- **The stop notification says "Screen capture ready", not "saved", on
  purpose.** Phase 2 writes nothing into any vault. If you go looking in
  Obsidian for a captured file you will not find one — the file is in
  `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures`. That is the
  documented state, not a failure.

## NOT reachable in Phase 2

Do not attempt these; they have nothing behind them yet. Listed so an
untested item is never mistaken for a passing one.

| Item | Arrives in |
| --- | --- |
| Region capture; region accuracy across mixed-DPI monitors | Phase 3 |
| `WDA_EXCLUDEFROMCAPTURE` — excluding our own windows from the recording | Phase 3 |
| The editor window: preview, timeline edits, undo/redo, sidecar persistence | Phase 4 |
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
