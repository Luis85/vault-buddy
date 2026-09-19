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
| 1 | **Monitor + one microphone** | Capture knowledge → *Record screen* → **Screen** tab → pick a monitor → tick exactly one input device → Start. Record ~30 s with speech. Stop from the tray menu. Play the staged `.mp4`. | Duration, resolution, is the speech audible? |
| 2 | **Window + mic AND loopback, mixed** | Same picker, **Window** tab → pick a window. Tick one **input** (mic) and one **output** (loopback / speakers). Play music through the output while speaking. Stop and play back. | Are BOTH audible in one stereo track? Note relative levels |
| 3 | **Zero audio devices** (spec §6.5) | Pick any source, tick **no** devices, Start, record ~20 s, Stop. | Does the capture succeed? Is the file silent (an audio track that is silent, or no audio track) and still playable? |
| 4 | **Pause excludes its stretch** (spec §6.2/6.3) | Record ~20 s with a visible timer on screen, Pause from the tray, wait ~20 s (change the screen visibly during the pause), Resume, record ~20 s more, Stop. | File duration (expect ~40 s, NOT ~60 s); is the paused stretch absent; do A/V stay in sync after the resume? |
| 5 | **Recorded window closes mid-capture** (spec §14) | Record a window, close it after ~20 s, then Stop (or let it self-finalize). | Was a warning surfaced? Did it finalize cleanly rather than fail? Does the file hold everything up to the close? Duration |
| 6 | **Mutual exclusion, both directions** (spec §7.3) | (a) Start an **audio** recording, then try to start a screen capture. (b) Stop it; start a **screen** capture, then try to start an audio recording. | The refusal text each way — it must name the running kind ("A recording is already in progress." / "A screen capture is already in progress."). Confirm the running capture is undisturbed and still saves correctly |
| 7 | **Hide to tray mid-capture** | Mid-capture, tray → *Show / Hide* (and try the buddy's own hide). | Does the buddy stay visible? The log should carry `hide ignored: a capture is in progress` |
| 8 | **Kill the process mid-capture** (spec §6.4, through the real pipeline) | Record ~60 s, then Task Manager → *End task* on Vault Buddy. Relaunch. Copy the orphaned `.<base>.mp4.part` out of the staging dir, rename it `.mp4`, and open it in a player / `ffprobe -count_frames` it. | Byte count of the `.part`; how much decoded (seconds and frames) vs. what was recorded. This is §6.4's measured property exercised through the real capture pipeline rather than the synthetic spike |
| 9 | **Static-screen heartbeat** | Start a capture and leave the screen **completely still** for ~60 s (no cursor movement), then move something, then Stop. | Does the result play through the still stretch at a steady rate, with the timeline advancing (not a freeze that jumps)? Note any stall |
| 10 | **4K @ 60 fps for two minutes** (spec §17.3) | Set 60 fps in config, capture a 4K monitor playing video for 2 minutes, Stop. | **Record the real numbers**: `screen capture: finalizing after N dropped frame(s)` from the log, the `fps` readings, the output file size, and whether playback is smooth. §17.3 wants a measurement, not a guess — this is where the hardware-vs-software-encoder question gets its data |

Extra observations worth writing down whatever the outcome: the CPU/GPU load
during (10), whether the encoder chosen was hardware or software (if the log
or Task Manager makes it visible), and the wall-clock time Stop took to
return for each capture (the stop wait is bounded at 30 s and reports
`stillSaving` on expiry).

## Known Phase-2 absences (do not file these as failures)

- **No live `fps` readout.** `ScreenCaptureBar.vue` (plan Task 10) landed —
  it renders elapsed, paused state, the source title, warnings and the
  **dropped-frame count**, so item 10's drop figure can be read off the bar
  as well as the log. The `fps` value the `screen:frames` event carries is
  deliberately **not** shown: a rate is not an anomaly, and spec §17.3's
  signal is the drop count. For item 10, take the fps readings from
  `vault-buddy.log` as the table says.
- **Three deliberate picker deviations from the spec** — no per-device audio
  level bars (§7.2), no Region tab, and the "Screen or window" chooser hint
  where §7.1 says "Screen, window, or region". Each is recorded and reasoned
  in docs/Gaps.md **GAP-111**; Phase 3 restores them deliberately.
- **Vault Buddy's own windows DO appear in a Phase 2 recording.** This is
  expected, not a bug: `WDA_EXCLUDEFROMCAPTURE` (spec §5.3) is Phase 3. If
  the buddy is in frame in a recording made here, that is the documented
  state.
- **No staging recovery.** A capture killed at item 8 leaves its `.part`
  behind permanently; nothing sweeps it until Phase 5's
  `run_screen_recovery` (docs/Gaps.md GAP-115). Clean the staging directory
  by hand between verification runs.

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
