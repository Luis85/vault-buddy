# Tutorial Editor — Windows Verification Checklist

Manual end-to-end verification on a real Windows machine. **A running
document across tasks**, not one task's gate — the tutorial-editor increment
lands in many small tasks (see
`docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md`),
and each one that touches something no automated gate on any platform can
observe appends its own rows here rather than opening a second file. The
screen-capture feature's own checklist
(`docs/superpowers/specs/2026-09-18-screen-capture-windows-verification.md`)
is the model this file's header, table shape and measurement discipline are
copied from — read that file's own header once if any of the conventions
below are unclear.

**Created by Task 15.** Its first five rows exist for two different reasons
that happen to land in the same task:

- **T1–T3** verify the thing Task 15 itself ships: opening a staged screen
  capture into the new Rust-backed editor session (`editor_open_staged`),
  from both entry points the panel offers, and that a second open of the
  SAME capture reuses the session rather than minting a duplicate one.
- **T4–T5** close out GAP-170 (docs/Gaps.md), a gap Task 11 opened and this
  file did not exist yet to carry: `build.rs`'s `AppManifest::commands(...)`
  now lists every command in `generate_handler!`, not just the `editor_*`
  ones, because leaving any command out of that list silently disables ACL
  enforcement's grant for every command NOT listed — the near-miss GAP-170
  documents at length. A Rust unit test
  (`editor::capability_guard::the_generated_acl_artifact_resolves_the_
  partition_correctly`) replicates Tauri's own resolution algorithm over the
  generated `gen/schemas/{capabilities,acl-manifests}.json` artifact and
  proves the DATA is right; it cannot exercise `RuntimeAuthority::
  resolve_access` itself, `webview/mod.rs`'s dispatch path, or a real IPC
  round trip inside a running app — no automated test in this repo can. T4
  and T5 are that proof, and until both carry a result GAP-170 stays at its
  current severity (High, unverified): a resolution mismatch here would not
  be a scoping gap, it would be the WHOLE APP refusing every command from
  every window.

Re-measure the row count rather than incrementing it — the screen-capture
checklist's own header explains why that discipline exists (it was wrong
twice from incrementing):

```bash
grep -cE '^\| T[0-9]+ \|' docs/superpowers/specs/2026-09-21-tutorial-editor-windows-verification.md
```

This file carries **17 rows** today (T1–T17), of which **0** carry a result. An
empty *Result* column means unrun, which is not the same as failed — never
convert one to the other, and never claim a manual run that was not actually
performed on this host.

Spec: `docs/superpowers/specs/2026-09-21-tutorial-editor-integration-design.md`
(the ADR this task and its siblings implement); GAP-170 in `docs/Gaps.md` (the
gap T4/T5 close).

Run against a build of `claude/vault-buddy-improvement-polish-95f5bc`
(`npx tauri build`, or `npm run test-build` for a dev run).

## Measurement discipline

Same rule the screen-capture checklist established: **prefer instrumentation
that reports over assertions that confirm.** Write the observed value into
the *Result* column, not a tick — which project id opened, whether a second
open reused it or minted a new session id, the exact ACL refusal message,
which commands were exercised and that each one actually returned data
rather than silently no-op'ing.

## Where to look

| What | Where |
| --- | --- |
| A staged capture's editor session | `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\` — `project.json` is the workspace envelope; the PIN back to the staged capture is `editorProjectId` in the capture's own sidecar (`%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\<base>.json`) |
| Whether a second open reused the session | The editor window's title/duration line (`EditorRoot.vue`'s temporary shell, testid `editor-shell` in the DOM) shows the opened project's own title; `vault-buddy.log` logs each `editor_open_staged` call and, on reuse, does NOT create a new `<projectId>` folder under `editor-projects\` |
| The app-wide IPC ACL | `src-tauri/gen/schemas/{capabilities,acl-manifests}.json`, regenerated on every `cargo build`/`tauri build` of the shell crate (git-ignored, 1:1 derived from `src-tauri/tauri.conf.json` + `src-tauri/capabilities/*.json` + `src-tauri/build.rs`'s `ALL_COMMANDS`) |
| Logs / crash records | `vault-buddy.log` (tray → *Open logs folder*) |

## Task 15's rows

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T1 | **Open from the capture bar's Edit button** | Record a short screen capture and stop it. On the panel's list view, the finished capture's row offers **Edit** (`ScreenCaptureBar`'s finished-row affordance). Click it. **Record**: whether the editor window opens, whether its temporary shell line (top of the window) shows a title/duration, and whether `vault-buddy.log` shows an `editor_open_staged` call for that capture's base. | |
| T2 | **Open from the staged-capture list** | Capture knowledge → *Record screen* → with at least one staged (unsaved) capture already sitting in staging, the picker's `StagedCaptureList` shows it with a **Resume editing** action. Click it for a capture that is NOT the one T1 already opened. **Record**: same three observations as T1, for this second, independently-opened capture. | |
| T3 | **A second open of the SAME capture reuses the session, not a new one** | With the editor open on the capture from T1 (or T2), go back to the panel and click **Edit** again for that exact same capture (from the capture bar if it is still the most recent, or from the staged list otherwise). **Record**: the `<projectId>` folder under `editor-projects\` before this second click (list the directory, or read the project id off the editor shell / log line), then click, then record it again. Expect: identical — no second folder created, and `vault-buddy.log` shows Rust reusing the live session rather than opening a fresh one (`open_staged_session_reuses_a_live_session_and_reports_missing_media`'s production behavior). A DIFFERENT id here means a duplicate project was minted for one capture. | |
| T4 | **Ordinary commands still dispatch under the exhaustive app ACL** (GAP-170, closing) | With the app freshly launched, drive it as a user would, through windows OTHER than the editor: open the panel and confirm the vault list populates (`list_vaults`); start and stop a short audio recording (`start_capture`/`stop_capture`); open Buddy settings and save any per-vault or app-global setting (e.g. toggle a Screen tab field via `set_screen_capture_config`, or `set_capture_config`). **Record**, per command, whether it worked exactly as before this task's `build.rs` change (which made the app manifest list ALL commands, not just the eight `editor_*` ones) — a regression here means the app is effectively bricked from every window, not a security issue in one feature. | |
| T5 | **An `editor_*` command is refused from a non-editor window** (GAP-170, closing) | With the PANEL window focused, open its devtools console (right-click → Inspect, or the equivalent dev shortcut) and run `window.__TAURI__.core.invoke("editor_execute", { request: { sessionId: "not-a-real-session", expectedRevision: 0, commandId: "manual-check", command: { kind: "undo" } } })`. **Record**: the exact rejection — it must be refused by the ACL (a permission-denied shape) BEFORE `session_commands::editor_execute`'s own body ever runs (which would instead reject with `sessionGone` for a made-up session id — a DIFFERENT failure that would mean the ACL scoping failed silently and only the native `authz::require_editor_window` caught it). The two are distinguishable by the error's own shape/message; write down which one was observed. | |

## Task 21's rows

Task 21 adds direct manipulation to the timeline: pointer-captured drag and
trim, Escape-to-cancel, keyboard nudge and a keyboard shortcut dispatcher on
the editor shell. The Vitest suite drives all of it through happy-dom, which
has no real pointer capture, no real focus model and no second listener
competing for a keystroke; the Playwright layout check is Chromium, not
WebView2. These two rows are what only the shipped window can show.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T6 | **Drag, trim and Escape behave in the real WebView2 window** | Open a staged capture in the editor and split it once (`S` with the playhead mid-clip) so the track holds two clips. (a) Drag the second clip's body a few seconds right, moving the pointer fast and past the clip's own edge and outside the timeline before releasing. (b) Drag its left trim handle right, well past the point where it would shrink below a tenth of a second. (c) Start a body drag, press Escape while still holding the button, then move and release. **Record**: for (a), whether the clip tracked the pointer the whole way (pointer capture) and whether exactly one move landed (one Undo reverts it entirely); for (b), whether the clip stopped shrinking at ~100 ms instead of vanishing or inverting, and the committed in/out in the Inspector's Clip section; for (c), whether the clip snapped back on Escape and whether anything was committed (Undo label unchanged). | |
| T7 | **Ctrl+Z in the new timeline does not also undo the legacy strip** | With the legacy capture editor still showing below the new shell (`SHOW_LEGACY_EDITOR`), make one edit in the LEGACY strip (split a block) and one in the new timeline (nudge a clip with the arrow key). Click a clip in the new timeline so focus is inside the new shell, then press Ctrl+Z once. **Record**: which surface changed — expected only the new timeline's nudge is undone and the legacy strip keeps its split. Then click an empty area outside the new shell (focus on the page body) and press Ctrl+Z once more. **Record** which surface changed this time (expected: the legacy strip, since the new shell's dispatcher only sees keystrokes whose focus is inside it). | |

## Task 22's rows

Task 22 adds the layered preview (`PreviewSurface.vue` + the non-reactive
`PreviewController`) and widens the asset protocol's scope to ADR R7's
enumerated list. happy-dom has no decoder and no Web Audio, and the
Playwright check is Chromium serving a same-origin fixture, so neither can
show the three things below, which depend on WebView2 itself: the asset
protocol's real CORS answer feeding a `MediaElementAudioSourceNode`, the
scope as Tauri actually resolves it, and element-seek sync on real media.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T8 | **The new preview plays a staged capture, with sound, through `editor_media_url`** | Record a short screen capture WITH an audio input (a microphone), stop it, and open it in the editor. In the new shell's preview (below the preview toolbar — not the legacy preview further down), press the transport's play button (or Space with focus on the page body). **Record**: whether the picture plays; whether the SOUND is audible (every layer is routed through Web Audio for monitoring). Diagnosis if not: the media elements are `crossOrigin="anonymous"`, so a failed CORS check on the asset response fails the WHOLE load — no picture AND no sound, and the "Not shown in the preview (media unavailable)" line may stay empty because the path lookup itself succeeded. A moving picture with no sound points elsewhere — most likely the `AudioContext` still `suspended` (it is resumed on play; check `vault-buddy.log` and the devtools console), or the transport's **Muted**/volume state; and whether the transport's time advances and the timeline playhead follows it. | |
| T9 | **Monitor mute, volume and rate are local, and mute/rate survive a reopen** | During T8's playback: click **Sound** (it becomes **Muted**), drag the volume slider, and pick 1.5x in the rate menu. **Record**: that the sound stops/changes level immediately, that playback speeds up, and that the header's Undo label did NOT change (no editor command was sent). Close the editor window and reopen the same capture. **Record** whether Muted and 1.5x are restored (both are `workspace.json` fields) and that the volume came back at full (deliberately not persisted — R16's workspace has no field for it). | |
| T10 | **The widened asset scope still refuses what R7 leaves out** | Run the app as a DEBUG build with `npm run test-build` (`tauri dev`): a release `npx tauri build` ships without devtools, and `window.__TAURI__` does not exist because `tauri.conf.json` does not set `withGlobalTauri`. With the editor window open, right-click → Inspect to open its devtools console and, for each path below, run ``const url = window.__TAURI_INTERNALS__.convertFileSrc(String.raw`<path>`, "asset"); await fetch(url).then(r => r.status)`` (`__TAURI_INTERNALS__.convertFileSrc(filePath, protocol)` is exactly what `@tauri-apps/api/core`'s `convertFileSrc` calls). The three paths: the open project's own `editor-projects\<projectId>\project.json`, a file you create under that project's `jobs\` directory, and any note in one of your vaults. **Record** each status: all three must be refused (403), while the same call for the project's staged `.mp4` under `screen-captures\` returns 200. A 200 for any of the first three means the scope Tauri resolved is wider than the pinned `tauri.conf.json` array says. | |


## Task 25's rows

Task 25 adds media import (`editor_import_media`): Rust opens its own native
multi-file dialog, copies each file into the project's `media\`, probes the
copy with the user's ffprobe, and reports per-file results on a job
`Channel`. The Rust suite drives the pipeline against a FAKE prober and a
tempdir; `ffmpeg::probe_media` is exercised against a real ffprobe on
synthesized clips; nothing automated opens the real Windows file dialog,
feeds a real damaged file to the real ffprobe, or watches the result land in
the WebView2 library column. This row is that proof.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T11 | **A mixed batch with one corrupt file imports everything else** | With ffmpeg installed (Buddy settings → Integrations shows it), open a staged capture in the editor. Prepare a folder with: one ordinary `.mp4` video, one `.mp3` WITH embedded cover art (most music files have it), one `.png` screenshot, and one CORRUPT file — a plain `.txt` renamed to `broken.mov` (a truncated `.mp4` is NOT a reliable stand-in: a fast-start file keeps its index at the front, so ffprobe can still read its streams and length from the first few KB). In the library column press **Import…**; the Windows file dialog opens (parented to the editor window, filtered to video/audio/image types). Multi-select all four and press Open. **Record**: (a) the progress bar appears and finishes; (b) the summary line's imported count, and that the per-file list names ONLY the corrupt file (by its file name, with no folder path in the message); (c) that the three good files appear as cards with the right kind (Video / Audio / Image) and a plausible duration (the image reads 0:05); (d) the files under `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\media\` — exactly three `<assetId>.<ext>` files, and NO `.part` file and no copy of the corrupt one; (e) that ONE Ctrl+Z (or the header's Undo) removes all three cards at once; (f) run it again and press **Cancel** while a large file is copying: the files already imported stay, and the summary reads "Import stopped". | |

## Task 27's rows

Task 27 adds the mixer (per-track volume/mute/solo and the master gain), the
Audio inspector (clip volume and mute) and **Detach audio**. The Rust suite
proves the commands and the `sources.json` wiring; the Vitest suite proves
every control sends one command (or, for the monitoring mute, none), over a
faked Web Audio graph. Nothing automated plays a detached clip through a real
WebView2 `<audio>` element or reads a real `AnalyserNode`.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T12 | **Detach audio from an imported video plays the sound on its own clip** | Import an `.mp4` WITH sound (T11's steps) and add it to a video track. Select the clip, open the inspector's **Audio** tab and press **Detach audio**. **Record**: (a) a new clip named "… · audio" appears on an audio track at the SAME start and length; (b) the original clip's Audio tab reads **Unmute clip audio** (it is muted); (c) pressing play, the sound is audible exactly once (not doubled) and in sync with the picture; (d) muting the NEW audio clip silences the sound while the picture keeps playing; (e) ONE Ctrl+Z removes the audio clip and unmutes the original. Then import a SILENT video (e.g. a screen recording made with no microphone) and press **Detach audio** on it: **Record** the error shown (expected: "<name> has no audio to detach") and that nothing changed. (GAP-175, fixed 2026-09-23: a staged capture's own picture now shows in the new preview too, not only an imported video's — either works for (c).) | |
| T13 | **The mixer's levels, solo and preview peak** | During T12's playback open **Audio mixer** (beside the transport's speaker). **Record**: (a) the **Preview peak** bar moves with the sound and its dBFS figure is at or below 0.0; (b) with **Mute preview (does not affect the video)** ticked the sound stops and the peak reads −∞ dBFS, while the header's Undo label does NOT change; (c) **Solo** on the audio track labels every other track "Silenced by solo" and only that track is heard; (d) dragging the master slider changes the readout while dragging but the Undo label changes only ONCE, on release. | |

## Task 28's rows

Task 28 derives waveforms and thumbnails with ffmpeg into the project's own
`cache\` directory (`src-tauri/src/editor/media_derive.rs`). The Rust tests run
the real ffmpeg round trip (a synthesized tone then silence, a synthesized
test pattern) and kill a stand-in decode through a closing session; Vitest
draws the polyline from faked peaks. Nothing automated shows a real
thumbnail through WebView2's asset protocol from `cache\`, or watches the
decode stay off the UI thread on a long real recording.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T14 | **Waveforms and thumbnails on the timeline, and the no-ffmpeg hint** | Open a staged capture recorded WITH a microphone that is at least 10 minutes long, detach its audio (T12's steps) and zoom the timeline out. **Record**: (a) the audio clip draws a waveform that is flat where the recording was silent and tall where someone spoke; (b) while the first waveform is computing, the playhead, scrolling and the preview stay responsive; (c) the video clip shows a small poster frame at its left edge (the asset protocol serving `cache\<id>-<ms>.jpg`); (d) closing and reopening the editor draws the same waveform at once (from `cache\`, no second decode — the Rust log shows no `editor-peaks` work); (e) trimming the audio clip's start moves the drawn waveform with the handle. Then point Buddy settings → Integrations at a non-existent ffmpeg path (or rename ffmpeg), restart, open a project whose `cache\` has been deleted: **Record** that the audio lane reads "Install ffmpeg to see waveforms" and nothing else breaks. | |

## Task 31's rows

Task 31 adds speed, layout and transforms: `setSpeed`/`setLayout`
(`core::editor::commands::layout`), the preview's picture-in-picture handles
(`LayoutHandles.vue`, a sibling overlay of the stage) and the placement of each
picture inside its box (`src/editor/previewTransform.ts`: fit, crop, rotation,
mirror, flip, rounded and circular frames). Vitest measures the arithmetic and
the DOM styles in happy-dom; nothing automated shows WebView2 clipping a
playing `<video>` to a rounded or circular frame, rotating it, or honouring
`preservesPitch`, and nothing drags a real handle with a real mouse.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T15 | **Picture-in-picture handles, transforms and speed in the real window** | Open a staged capture, import a second video (T11's steps) and place it on a new video track ABOVE the capture, so both show at once. Select the upper clip. (a) In the preview, drag its body, then its bottom-right handle, then its left edge handle; press Escape once while still holding a handle. (b) In the inspector's **Layout** tab press **Top right**, then choose **Circle**, then **Fill** with crop zoom 2 and Focus X 20, then rotation 90 and **Mirror**. (c) In the **Speed** tab press **2×**, play, then untick **Preserve pitch** and play again; then try **0.5×** where the next clip on the same track is close behind. **Record**: for (a), that the picture moved with the handles during each drag, that each drag is ONE Undo step (the header's Undo label reads "Change layout"), and that Escape put the box back with nothing committed; for (b), that the circle looks round (not an oval) and stays inside its frame edge while cropped, rotated and mirrored, and that the handles' box outline sits exactly on the picture; for (c), that playback is twice as fast with the voice at normal pitch, then higher-pitched with the box unticked, and that 0.5× is refused with a message naming the clip in the way (nothing else moved). | |

## Task 32's row

Task 32 adds canvas formats and basic colour treatment: `setCanvas`/
`setAdjustments` (`core::editor::commands::layout`), the ratio control (a
native `<select>` in `PreviewToolbar.vue`) and the Color inspector category
(`ColorSection.vue`, six presets plus five sliders mapped to a CSS
`filter:` via `src/editor/colorPresets.ts`). Vitest measures the command
refusals and the exact CSS filter STRING in happy-dom; nothing automated
shows WebView2 actually reflowing a playing `<video>` at a new canvas
aspect ratio or rendering `brightness()`/`contrast()`/`saturate()`/
`sepia()`/`grayscale()` visibly correctly.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T16 | **Canvas ratio and colour presets in the real window** | Open a staged capture (landscape source). (a) In the preview toolbar's **Aspect ratio** control, pick **9:16 Portrait**, then **1:1 Square**, then **4:3 Classic**, then back to **16:9 Landscape**. **Record**: that the stage letterboxes/pillarboxes correctly at each pick, that a small toast appears each time naming Checks and disappears on its own after a few seconds (or on its own Dismiss button), and that re-picking the CURRENTLY active ratio does nothing (no toast, no Undo entry). (b) Select a video clip, open the inspector's **Color** tab, and click through **Vivid**, **Warm**, **Cool**, **Mono** and **Sepia** while the clip plays. **Record**: that each preset visibly changes the picture (Vivid punchier, Warm more orange, Cool slightly desaturated, Mono black-and-white, Sepia brown-toned) and that the teaching-tool overlays (if any are on screen) are NOT tinted — only the source picture. (c) Drag the Saturation slider to its two extremes (0 and 2) and the Sepia/Grayscale sliders to 1: **Record** that the picture responds live as you drag, with no flash or a lag longer than a frame or two. (d) Select a title-card clip (Insert intro, or any `addCard` clip once one exists) and open **Color**: **Record** that the tab shows "Colour applies to footage, not title cards." instead of controls. | |

## Task 35's row

Task 35 adds the teaching cues' preview surface: the seven toolbar tools
(`src/editor/cueActions.ts`), the SVG overlay drawn into the stage
(`CueOverlay.vue`), the separate grab layer beside it (`CueHandles.vue`),
the zoom's stage transform and the effect inspector (`EffectSection.vue`).
Vitest checks the commands sent, the SVG attributes and the DOM layering in
happy-dom; nothing automated shows WebView2 drawing the SVG exactly over the
playing video at a real window size, the zoom's clip-path inside the
letterbox, or a pointer reaching a cue's hit shape through a real layout box.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T17 | **Teaching cues drawn over the video, grabbed, zoomed** | Open a staged capture and select its (full-frame) clip with the playhead on it. (a) Press **Arrow** in the preview toolbar. **Record**: an arrow appears over the picture, its endpoints show as round handles, and the inspector switches to the arrow's fields with **Starts at (ms)** equal to the playhead. (b) Drag the arrow's head handle somewhere else, release, then press Ctrl+Z. **Record**: the arrow follows the pointer while dragging, the header's Undo label reads "Update effect" after release, and one Undo puts it back. (c) Press **Zoom**, then scrub the playhead through the zoom's span. **Record**: the picture scales up towards the focal marker and back, never showing black bars or the letterbox inside the canvas, and the arrow (if on screen) scales with the picture. (d) Press **Privacy cover**. **Record**: an opaque box covers that area of the picture, and the inspector shows "Covers pixels only while visible. It does not track motion and the original recording is unchanged." (e) Click the bare picture: **Record** that the cue selection drops and the clip's layout box (Task 31) is back; move the playhead before the clip starts and **Record** that the layout box disappears. (f) Resize the editor window: **Record** that the cues stay pinned to the same spots of the picture. | |
