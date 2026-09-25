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

This file carries **61 rows** today (T1–T61), of which **0** carry a result. An
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

## Task 36's row

Task 36 adds captions and chapters: `editor_import_captions`' native open
dialog on the `editor-captions` thread, the Captions and Chapters library
tabs, and the preview's caption layer (`CaptionOverlay.vue`). Rust and Vitest
cover the parser, the output-to-source conversion, the commands sent and the
DOM; nothing automated opens the real Windows file dialog, reads a real
subtitle file written by another tool, or shows WebView2 drawing the caption
text over a playing picture.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T18 | **Caption import, editing and chapters in the real window** | Open a staged capture and select its clip. (a) In the library's **Captions** tab press **Import SRT / WebVTT**, pick a real `.srt` saved with Windows line endings by another tool (and later a `.vtt` with a `STYLE` block), whose last cue starts after the clip ends. **Record**: the native dialog opens over the editor and lists `.srt`, `.vtt` and `.txt`; the status line reads "Imported N captions ... 1 cue fell outside the clip and was skipped."; one Ctrl+Z removes the whole import. Then pick a file with a broken timestamp: **Record** that the error names its line and no file path. (b) Play across a caption. **Record**: its text shows over the picture at the bottom; switching **Position** to Top and **Size** to 50 moves and enlarges it; unticking **Show captions** hides it. (c) Speed the clip to 2× (inspector **Speed**) and **Record** that the caption list's times halve while the captions still line up with the same words. (d) In the **Chapters** tab press **Add chapter at playhead**, rename it, then trim the clip's head past it. **Record** that the chapter's time follows the trim and that it leaves the list once trimmed away. | |

## Task 37's rows

Task 37 (Part A) adds the recovery journal (`recovery.json`, written by the
`editor-journal` thread after every acknowledged edit), the close guard (the
editor's X now emits `editor:closeRequested` to the editor window instead of
hiding it) and the startup re-pin sweep (`editor-recovery-sweep`). Rust tests
cover the journal's write/delete rules and the sweep on a tempdir, and Vitest
the dialogs; nothing automated kills a real process mid-edit, relaunches it,
or clicks a real WebView2 titlebar X.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T19 | **Kill mid-edit, relaunch, Resume restores the last acknowledged edit** | Open a staged capture in the editor and make three distinct edits (e.g. rename the project, split the clip, nudge the second clip). Wait one second, then end the process from Task Manager (End task on Vault Buddy — no Save, no close). **Record**: that `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\recovery.json` exists and its `sessionRevision` is higher than `project.json`'s `record.revision`. Relaunch and open the same capture again. **Record**: whether the "Unsaved changes from last time" dialog names the project; press **Resume** and record whether all three edits are back and the header reads unsaved. Then save and **Record** that `recovery.json` is gone. Repeat once pressing **Discard** instead: record that the saved project opens without the three edits, `recovery.json` is gone and `project.json` is byte-identical to before (compare file hashes). | |
| T20 | **Close with unsaved changes → Keep for later → reopen** | Open a staged capture, make one edit, and click the editor window's titlebar X. **Record**: the close guard shows **Save project**, **Keep for later**, **Discard changes** and **Cancel**. Press **Keep for later**: the window hides. Reopen the same capture from the capture bar's **Edit**. **Record**: the edit is still there, the header still reads unsaved, and NO recovery dialog appeared (the journal is this session's own). Then click the X on a CLEAN editor (after a save) and **Record** that it hides at once with no dialog; and with only the legacy editor showing (no new session), that the X still hides as it always did. | |

## Task 39's rows

Task 39 adds the portable and lightweight project files: `editor_export_package`
(the header's Save project menu -> **Save a portable copy…** / **Save a
lightweight copy…**, through `SaveProjectDialog` and Rust's own save dialog)
and `editor_import_package` (**Open a project file…**, Rust's own open
dialog). Rust tests cover the whole export/import round trip, the refusals and
the crash-safe install on tempdirs, and Vitest the dialog; nothing automated
opens the real native dialogs, moves a file between folders on a Windows
volume, or plays the imported media in WebView2. (The brief called this row
"T7"; that number was taken, so it is the next free one.)

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T21 | **Portable export, move, import, play** | Open a staged capture in the editor, import one more video (T11's steps) and place it on the timeline, rename the project, and do NOT save. (a) Header **▾** beside **Save project** -> **Save a portable copy…**. **Record**: the dialog explains that the portable file includes the originals (the amber warning), the radio buttons are full-size, the heading and buttons stay visible when the window is made short enough to scroll the dialog, and pressing the save button opens the NATIVE save dialog offering `<title>.vbproject.zip`. Save it to Documents. **Record**: the dialog reads "Saved to <that file name>" only after the native dialog closes, and no `.part-` file is left beside it. (b) Save again onto the SAME file (confirm the overwrite): **Record** it succeeds. Then save onto an unrelated existing `.zip` renamed to `x.vbproject.zip`: **Record** the message "Choose a new name — that file is not this project" and that the unrelated file is unchanged. (c) Move the saved `.vbproject.zip` to another folder (e.g. Desktop). Header **▾** -> **Open a project file…**, pick it. **Record**: the editor switches to the imported project (a COPY with a new id, since the original is still in the store — check `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\`), its title and clips match, the preview PLAYS both the screen capture and the imported video, no staged capture's sidecar gained a second `editorProjectId`, and no `.<id>.importing` directory is left in the store. (d) Repeat (a) and (c) with **Save a lightweight copy…**: **Record** that the imported copy lists both originals as missing and the preview shows them as unavailable. | |

## Task 40's rows

Task 40 adds `editor_relink_media` (the media library's **Reconnect…**,
through `ReconnectDialog` and Rust's own open dialog). Rust tests cover the
matching, the copy, the `sources.json` rewrite, the replacement rule and the
cache purge on tempdirs, and Vitest the dialog and the preview's re-lookup;
nothing automated opens the real native dialog, probes real media with the
installed ffprobe, or plays the reconnected file in WebView2.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T22 | **Reconnect after a lightweight import, including an ambiguous pick** | Do T21 (d) so the imported copy lists both originals as missing. Make a byte-identical copy of the imported video beside the original (e.g. `clip.mp4` and `clip (copy).mp4`). (a) In the media library press **… originals are missing — Reconnect…**. **Record**: each missing original is listed with its size and length, and no file path appears anywhere. (b) Press **Find all…**; in the NATIVE dialog (it should allow several files) pick the screen capture's `.mp4`, BOTH copies of the video, and one unrelated video. **Record**: the capture's row reads "Reconnected to …" and the preview now PLAYS it (no reload); the video's row reads "2 chosen files match equally well: … Choose the right one." and the video is still shown as unavailable in the preview. (c) On the video's row press **Choose file…** — the dialog should allow only ONE file — and pick `clip.mp4`. **Record** "Reconnected to “clip.mp4”", that the preview plays it, and that `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\media\` now holds both files under their asset ids. (d) Repeat on a fresh lightweight import, but pick a SHORTER video for the video's row: **Record** the reason ("different duration: … vs …"); press **Replace…** and pick the same file: **Record** the refusal naming it as shorter. Pick a LONGER video instead via **Replace…**: **Record** "Replaced with …", that its `sources.json` entry carries `replacedFrom`, and that every clip, cue and caption on the timeline is exactly where it was. | |

## Task 45's rows

Task 45 adds the render runner (`screen::render::run::render`: the capability
refusal, the two ASS documents in the job dir, the ffmpeg run and the ffprobe
verification) and the shell's capability probe (`ffmpeg::probe_capabilities`).
`screen/tests/render_roundtrip.rs` renders real projects through a real
ffmpeg and decodes the pixels and audio back — on synthesized lavfi inputs,
with the fonts libass happens to find, and with libx264. What no automated
test can reach: a real fragmented-MP4 screen capture from the capture sink,
the fonts libass resolves on a real Windows install, and a hardware or
Media Foundation H.264 encoder. The brief called these rows "T8–T10"; those
numbers were taken, so they are the next free ones (T26 was added by the
fix round, for the untouched-capture remux). **They are runnable once
the render job and its Render control exist (Task 46 onward)**; until then
leave the Result empty.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T23 | **Render a real staged screen capture with cues** | Record a 20–30 s screen capture of a window with visible motion, stop it, open it in the editor. Add a text cue (a few words), an arrow and a 2× zoom somewhere in the middle, plus one burned-in caption. Render the whole project. **Record**: whether the render finishes and the product plays in the Windows player AND in Obsidian's preview; the product's duration (file properties) against the editor header's; whether the first second is picture (not black or frozen — the capture is a fragmented MP4, which no automated test renders); and, at each cue's time, whether the cue is drawn where the preview drew it and the zoom scales the cue with the picture while the caption stays unzoomed. | |
| T24 | **libass finds a font on a real Windows install** | With the product from T23 open, look at the text cue, the caption and (if one exists) a title card's text. **Record**: whether every text appears at all, and whether it is Segoe UI (compare the lowercase `g`/`y` with the editor preview, which uses Segoe UI) or a fallback face; and whether `vault-buddy.log` or the render's failure text mentions `fontselect` or a missing font. The render passes no `fontsdir` yet, so this is libass's own font discovery on Windows. | |
| T25 | **A non-libx264 encoder and a build missing a filter** | In Buddy settings → Integrations → ffmpeg, note the reported H.264 encoder. Point the ffmpeg path at a build whose encoder is `h264_mf`, `h264_nvenc`, `h264_qsv` or `h264_amf` (e.g. an LGPL "essentials"/"shared" build without libx264) and Recheck. (a) Render a project with a dissolve and a zoom. **Record**: the encoder the settings card shows; whether the render is refused BEFORE it starts with a message naming each missing filter and the feature that needs it (the zoom, a dissolve, ...), with "4.3" only for `xfade`, or runs; write down WHICH filters the build lacked (this is the only place that gets measured); if it runs, whether the product plays and its duration matches. (b) Render a plain trimmed clip (no zoom, no colour grade, no rounded/circle frame, no cue) with the same build. **Record**: whether it renders with the hardware/MF encoder and plays. | |
| T26 | **Render an UNTOUCHED real staged capture (the R1 remux)** | Record a 20–60 s screen capture WITH audio (a microphone or system sound) and include a few seconds of a completely still screen near the end (the still-screen heartbeat, GAP-112, and the unclocked audio, GAP-113, are what move the container's length away from the sidecar's). Open it in the editor and render it WITHOUT any edit. **Record**: the sidecar's `durationMs` (`%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\<base>.json`) and the staged `.mp4`'s own length (file properties, or `ffprobe -v error -show_entries format=duration -of csv=p=0 <file>`); whether the render finishes and is kept (not refused as "the render is X ms long where Y ms were planned"); that it was fast (a stream copy, seconds not minutes); and the product's length against the staged `.mp4`'s. | |

## Render jobs and the shutdown gate (Task 46)

`render_jobs_tests.rs` drives the job over a FAKE runner, so the cancel, the
discard and the quit are proven against the job's own bookkeeping, and
`quit_cancels_a_render_before_finalizing_captures` only reads the quit
workers' SOURCE. What no automated test reaches: a real ffmpeg child killed
by a quit, by Alt+F4 or by a discard on Windows (a file an ffmpeg still holds
open is exactly what Windows refuses to delete), and the updater refusing a
render in the real panel. **Runnable once the Render control exists (Task 47
onward)**; until then leave the Result empty.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T27 | **Quit, Alt+F4, Install & restart and Discard while a render runs** | Open a project with at least 60 s of edited footage (a dissolve or a zoom, so it re-encodes) and start a render. (a) While it is rendering, choose **Quit** from the tray. **Record**: how long the app took to exit (the render's cancel is bounded at 5 s); that no `ffmpeg.exe` is left in Task Manager; and that `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\jobs\` holds no `<jobId>` folder and `products\` no new file. (b) Relaunch, start a render again and press **Alt+F4** on the buddy: **Record** the same three things, and that the app exited once (it did not keep re-opening a close). (c) Start a render and use Settings → Updates → **Install & restart** (with an update available, or record that none was): **Record** the refusal text ("A video is being rendered in the editor…") and that the render kept running. (d) Start a render and choose **Discard** for the project in the editor's close dialog: **Record** that `ffmpeg.exe` ended, that the project folder is gone, and that the staged capture is still listed in Record Screen. (e) Let one render finish, close the editor WITHOUT saving, reopen the project from the project list: **Record** that the product is listed and plays. | |

## The Render dialog, the product library and Review (Task 47)

`editorRenderDialog.test.ts` and `editorProductLibrary.test.ts` drive the
dialog, the library and the toolbar's Review over a fake port in happy-dom,
and `render_review_tests.rs` drives a Review job over a fake runner. What no
automated test reaches: a real product and a real review PLAYING in WebView2
through the asset protocol (`products\` and `cache\` in the R7 scope), a
review file actually removed on a real session close, and the real render's
phases and Cancel as a user sees them.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T28 | **Render, watch, restore, and Review for real** | Open a project with at least 30 s of edited footage. (a) **Render video** in the header: note the default name (`<title> v<n>`), choose **A range of the output** and render 5–15 s at **Low**. **Record**: that the phase text moves through preparing, rendering and saving; that the bar never shows 100 % until "Render complete" appears; then **Watch rendered file**: that the video plays WITH sound in the dialog. (b) Start a second whole render and press **Cancel render**. **Record**: that the dialog says "Render cancelled" (not failed), that no new product appears in the library, and that no `ffmpeg.exe` is left in Task Manager. (c) Library → **Products**: **Record** each card's name, `r<revision>`, range and created time, and that **Watch** plays the product (not the editable preview). Make an edit, then **Restore this edit** → **Restore** on the first product: **Record** that the edit is back to the rendered one and that Undo returns your edit; the product still plays. (d) Rename `products\<productId>.mp4` in `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\` and reopen the Products tab: **Record** that the card reads "Unavailable" with Watch disabled, and Restore still works. Rename it back. (e) Select a clip and press **Review** in the preview toolbar: **Record** that a review renders just that clip's span and plays; that `cache\review-<jobId>.mp4` exists while it plays, that `products\` and `products.json` did not change, and that after closing the editor (Keep) the review file is gone. | |

## Publish to vault and subtitle export (Task 48)

`publish_tests.rs` drives the tenth vault write over a tempdir vault with the
copy and the note injectable, `publish_io`'s own tests the copy, and
`subtitle_commands_tests.rs` the export with the save dialog answered by a
fixed path. What no automated test reaches: a real Obsidian vault on a real
volume (a sync client, a second drive, a full disk), the real native save
dialog, and a quit or a crash on Windows while a copy holds its files open.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T29 | **Publish into a vault that already has a same-name file** | Render a product, then in the vault's target folder (the Publish dialog's folder; blank = the vault's screen-capture folder, dated if ticked) create `<the name it will take>.mp4` AND `.md` by hand (publish once first to learn the name, `YYYY-MM-DD HHmm <product name>`, then copy both files). Open **Publish to vault…** from the Products tab. **Record**: that the vault picker preselects the vault the capture was RECORDED for (not the one last used in the panel); the names the dialog reports; that they carry the next ` (N)` suffix TOGETHER (the `.mp4` and `.md` share it); that both hand-made files are byte-identical afterwards; that the note embeds the ` (N)` video and plays it in Obsidian; its `## Chapters` times against the rendered video; and that **Open in Obsidian** opens the note. Then **Export .srt** and **Export .vtt** from the Captions library: **Record** that the native save dialog opens, that the files' times match where each caption plays in the rendered video, and that choosing an existing file name is refused with the file untouched. | |
| T30 | **Disk full during publish (R-H5)** | Use a small volume (a USB stick or a VHD of a few hundred MB) as, or inside, a vault, fill it until less space is free than the product's size but more than zero, and publish a product into it. **Record**: whether it is refused BEFORE copying ("Not enough disk space in that vault…") and that nothing was created (no folder, no file, no hidden `.…vault-buddy.tmp`). Then free JUST enough for the check to pass and fill the rest during the copy (start a large file copy onto the volume right after pressing Publish): **Record** the error shown, that no `.mp4`, `.md` or hidden temp is left in the folder, that a dated folder this publish created is gone again, and that the product still plays in the Products tab. | |
| T31 | **Quit, Install & restart and a crash while publishing** | Publish a LARGE product (a long render at High) into a vault on a slow or network drive so the copy takes several seconds. (a) While it copies, **Quit** from the tray: **Record** how long the app took to exit (the publish cancel is bounded at 5 s) and that the vault folder holds no new `.mp4`, `.md` or hidden `.…vault-buddy.tmp`. (b) Start another publish and use Settings → Updates → **Install & restart**: **Record** the refusal ("A video is being published into a vault…"). (c) Start another publish and end the process in Task Manager mid-copy, then relaunch: **Record** the `editor-recovery-sweep: A publish was interrupted …` line in `vault-buddy.log`, that `editor-projects\<projectId>\jobs\<jobId>\publish.json` is still there after the relaunch, and whether a hidden partial temp was left in the vault folder (docs/Gaps.md GAP-192). | |

## Webcam takes and presenter placement (Task 50)

`webcamRecorder.test.ts` and `editorWebcamDialog.test.ts` drive the dialog
and the take's wire against a faked `navigator.mediaDevices` and
`MediaRecorder`: nothing is requested before **Enable camera**, chunks go to
`editor_webcam_append` strictly one at a time, every track is stopped on
close, `pagehide` and errors, and **Add to timeline** sends `addTrack` (index
0) → `insertClip` → `setLayout` at the ADR's `PRESENTER_CORNER`. What no
automated test reaches is a real camera (R-H1): WebView2's own permission
prompt, a device another app holds, the camera's light going off, and a
real `MediaRecorder` WebM surviving Rust's `-c copy` remux. The brief called
these rows "T13–T17"; those numbers were taken, so they are T32–T36.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T32 | **Opening Webcam touches no camera; Enable asks once** | Open a project, Library → Media → **Webcam…**. **Record**: that no permission prompt appears and the camera's light stays off while the dialog shows its explanation; then press **Enable camera**: that WebView2's own prompt appears once (or not at all, if already allowed) and names the editor, that the live preview shows the camera, and the cameras the **Camera** list offers (their labels). Switch to another camera in the list: **Record** that the preview switches and that the first camera's light goes off. Tick **Record microphone too**: **Record** whether a second prompt asks for the microphone. | |
| T33 | **Refused and busy cameras leave the project alone** | (a) With the dialog open, press Enable camera and **Block** the prompt (or turn camera access off in Windows Settings → Privacy & security → Camera first). **Record** the text shown (it must say access was blocked and name Windows Settings), and that the project's title bar shows no unsaved change and Undo is unchanged. (b) Start a video call in another app that holds the camera (Teams, the Windows Camera app), then press Enable camera. **Record** the text shown ("No camera could be opened…") and the same two project facts. (c) With ffmpeg's path pointed at a missing file (Buddy settings → Integrations), enable the camera and press **Record**: **Record** that after the countdown the dialog says ffmpeg is needed and to install it (not a camera error) and that the camera's light went off. | |
| T34 | **Record, review and retake a real take** | Enable the camera with the microphone, press **Record**, watch the 3-2-1 countdown, speak for 30–60 s, press **Stop**. **Record**: the countdown's three numbers; that "Finishing the take…" gives way to a player that plays the take WITH sound; its length against how long you recorded; and the files in `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-projects\<projectId>\takes\` (a `<takeId>.webm`, no `.part` left). Press **Retake**, record a second take and stop. **Record**: that the first take is still in the Media library and still plays there, and that the second is a separate asset. Start a third take and press **Discard recording**: **Record** that no `.<takeId>.webm.part` is left in `takes\`. | |
| T35 | **Add to timeline places the presenter** | With a take in review, park the playhead a few seconds into a screen capture and press **Add to timeline**. **Record**: that the dialog closes; that a new track named "Presenter" is the TOP video track; that the take starts at the playhead as its own clip (the screen clip is unchanged); that the preview shows it as a circle in the top-right corner; and that Undo removes the placement, the clip and the track in three steps. Select the clip and read Inspector → Layout: **Record** x, y, w, h (expect 0.775, 0.06, 0.19, 0.3378 on a 16:9 canvas), Circle, Cover. Drag it to another corner in the preview, then render a 5 s range: **Record** that the rendered file shows the circle where the preview does and that it is a circle, not an oval. | |
| T36 | **Every way out turns the camera off** | For each of these, **Record** whether the camera's light goes off and what `takes\` holds afterwards: (a) **Close** with the camera enabled and nothing recorded (closes at once); (b) **Close** while recording — the dialog must ask; choose **Discard recording and close** (no `.part` left); (c) **Close** with a finished take not yet added — the dialog must ask; record the wording (it must say the take stays in the media library), choose **Keep in library and close**, and confirm the take is in the library; (d) reload the editor window (Ctrl+R in a dev build) while the camera is live; (e) close the editor window with its titlebar X while recording — the close guard must say "You have an unsaved webcam take". | |

## Synchronized webcam beside a screen capture (Task 52)

`session/webcam.rs`'s pure rules are unit-tested (the clock mapping, the
pause discard and re-anchor, the file's rebase to its own first frame, each
frame's duration, the mode choice, the NV12/YUY2 conversion), and the picker
and the start's `webcamId` are covered in `screenSourcePicker.test.ts`. The
producer itself — `session/webcam_windows.rs`, an `IMFSourceReader` feeding
its own fragmented-MP4 sink — **executes in no automated test on any
platform**. These rows are its gate (and GAP-199's and GAP-200's). The brief
called them "T18–T22"; those numbers were taken, so they are T37–T41.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T37 | **Ten minutes of screen + webcam stay in sync** | Record Screen → pick a screen, pick a webcam in **Webcam**, start, and clap once in view of the webcam at the start, once at ~5 min and once at ~10 min, with the screen showing a stopwatch (e.g. a browser stopwatch page) and a microphone selected. Stop. **Record**: the files in `%LOCALAPPDATA%\com.vaultbuddy.desktop\screen-captures\` (`<base>.mp4`, `<base>.webcam.mp4`, `<base>.json`, no `.part` left); the sidecar's `webcam` block (`offsetMs`, `durationMs`, `width`, `height`, `deviceLabel`); `ffprobe -show_entries format=duration` of both files. Open it in the editor: **Record**, for each clap, the offset between the clap's sound on the screen track and the hands meeting on the presenter clip (frame-step in the preview) — the FIRST clap's offset on its own (a constant presenter lag from the device's capture latency, GAP-200 item 7) — and whether it grows from the first clap to the last (drift, GAP-200 item 2). Also **Record** whether the camera's native format is MJPG (Device Manager → the camera → Details, or the log's chosen mode) and that it opened (review fix round 1's two-step format selection). | |
| T38 | **Unplug the webcam mid-capture** | Start a screen + webcam capture, unplug the USB webcam after ~30 s, keep recording ~30 s, stop. **Record**: the warning the panel showed (it must say the webcam stopped and the screen capture continues); that the screen capture ran to the end; the sidecar's `webcam.durationMs` against when you unplugged; and in the editor, that the presenter clip ENDS where the webcam ended rather than running on frozen or black (GAP-199). | |
| T39 | **A webcam another app is using** | Open the Windows Camera app (or a Teams call) holding the webcam, then start a screen capture with that webcam chosen. **Record**: whether the start is refused with "The webcam could not be opened. It may be in use by another app…", that no capture started (no bar, no `.part` in `screen-captures\`), and that picking **No webcam** then starts normally. If instead it starts: **Record** what the webcam file holds. | |
| T40 | **Pause and resume keep both tracks aligned** | Start screen + webcam with a stopwatch on screen, clap, pause for ~20 s (keep moving in front of the camera), resume, clap, stop. **Record**: the capture's reported length against wall time minus the pause; the webcam file's length (`ffprobe`); and in the editor, the offset between each clap on the screen track and on the presenter clip — the paused stretch must be in NEITHER, and the second clap's offset must match the first's. | |
| T41 | **No webcam selected = unchanged capture** | Record a screen capture with **No webcam** (the default). **Record**: that no `<base>.webcam.mp4` or `.webcam.mp4.part` appears; that the sidecar has no `webcam` key; that the camera's light never came on; and that the capture plays and edits exactly as before. Also with NO webcam connected at all: **Record** that the picker says "No webcam found." and Start still works. | |

## Separate audio stems (Task 53)

`session/stems.rs`'s pure rules are unit-tested — every stem is cut from the
exact post-resample slice the mixer sums and stamped with the mixed chunk's
own timestamp (`stems_share_the_mixed_sample_stream`), a stalled input's stem
is padded like the mix, a writer that falls behind is abandoned rather than
waited for, and with stems off the mixed track is byte-identical
(`stems_default_off`). Migration, the sidecar list, `resolve_source`,
discard/Clear/usage and the recovery sweep are covered on disk. What NO
automated test executes is the Windows writer — `session/stems_windows.rs`
and `FragmentedSink::create_audio_only` (an audio-only fragmented MP4 with a
null video type): these rows are its gate (R-H4). The brief called them
"T23–T24"; those numbers were taken, so they are T42–T44.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T42 | **Two inputs record as two stems that match the mix** | In Vault settings → Screen, turn on **Keep each audio input as a separate track (new recordings only)**. Record Screen with TWO audio inputs selected (a microphone and the speakers/loopback), for ~2 minutes: speak into the mic and play a video with sound, and clap once near the start and once near the end. Stop. **Record**: that `screen-captures\` now holds `<base>.stem-1.m4a` and `<base>.stem-2.m4a` beside `<base>.mp4` (and no `.m4a.part` left behind); that the sidecar's `stems` block lists both with the right input names; that each `.m4a` plays in a media player and contains ONLY its own input; the length of each stem against the capture (ffprobe `-show_entries format=duration`) — they must agree within one AAC frame (~21 ms); and, laying a stem over the `.mp4`'s own audio in an editor, whether the two claps line up at BOTH ends (no drift). Then open the capture in the editor: **Record** that two audio tracks named after the inputs appear, each with one clip per screen clip, and that the screen clip plays silent while the stems play. | |
| T43 | **Pause, and a stem that cannot be written** | (a) With stems on, record ~30 s, pause ~20 s while still speaking, resume ~30 s, stop. **Record**: that no speech from the paused stretch is in either stem or the mix, and that the stems and the mix still line up after the resume. (b) Force a stem failure: start a capture with stems on, then (e.g. with a tool that holds an exclusive lock) deny writes to `screen-captures\.<base>.stem-1.m4a.part`, or fill the disk to near-full mid-capture. **Record** the warning shown (it must name the input and say the mixed recording still contains it), that the screen capture itself stops and stages normally, and that no `stem-1` file is left or listed afterwards. | |
| T44 | **Stems off (the default) records exactly what it did before** | With the toggle OFF, record a capture with two inputs. **Record**: that no `.stem-*.m4a` or `.m4a.part` file appears in `screen-captures\`, that the sidecar has no `stems` key, and that the editor shows only the "Audio" track with the screen clip audible — and that selecting the screen clip shows the Audio inspector's note explaining that the inputs are mixed into one track and naming the setting. | |

## Before-you-share checks (Task 54)

`core::editor::checks` is unit-tested rule by rule — one minimal project per
finding code next to a control that raises nothing, the wire literal, and
"no score, ever" — and `editor_get_checks`' shell facts (missing media from
`sources.json` and the disk, `hasAudio`, the open webcam takes) are tested on
a real tempdir store. The reveal logic and the dialog are tested in Vitest
against happy-dom. What no automated test can show is a reveal LANDING in
real WebView2 — a popover really opening over the preview, focus really
arriving on the ratio control after the dialog closes, the timeline really
scrolling — and a real missing file blocking a real render. These rows are
that gate.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T45 | **Every action reveals its object in WebView2** | Build a project that raises one finding of each action: a gap over 1 s on the top video track (Show it), a clip at opacity 0.02 (Open Layout), two overlapping sound clips at full volume (Open the mixer), a caption turned off or reading too fast (Open Captions), a privacy cover (Show it), a 16:10 source full-frame on 16:9 (Review the canvas), an unfinished webcam take (Open Webcam). Open **Checks** and press each finding's button in turn, reopening Checks between them. **Record**, per button: that the dialog closed; what became selected (the clip on the timeline, the cue in the preview with the effect inspector showing, the caption row in the Captions list, scrolled into view); where the playhead landed (the cue's OUTPUT time on a sped-up clip, not its source time); that the timeline scrolled a clip that was off screen into view; that the mixer popover / Webcam dialog / Layout tab actually opened; and for Review the canvas, which element has keyboard focus afterwards (it must be the ratio control, not the Checks button). Repeat Open Layout and Open Captions at a window narrower than 1180 px: **record** whether the closed library/inspector drawer opened. | |
| T46 | **A missing file blocks Render; warnings do not** | Save a project with an imported video, close the editor, move that video's copy out of the project's `media\` folder, reopen the project. **Record**: the header's Checks badge count; the Checks dialog's summary line and the missing file's sentence under "Fix before rendering"; that **Render video**'s dialog shows the same sentence and a disabled **Render video** with "Fix the blocking check first." beside it; and that the dialog's **Continue to render** is disabled with the same reason. Press **Reconnect media**, reconnect the file, and **record** that the finding, the badge count and the disabled state all clear without reopening the project. Then make a project with only warnings (e.g. a privacy cover): **record** that it renders. | |
| T47 | **The canvas toast, the destination picker and the wording** | On a capture-backed project, pick **9:16 Portrait** in the preview toolbar. **Record**: that the toast reads "Canvas changed. Review crop, text and caption placement in Checks." with an **Open Checks** button that opens the dialog, and which Review-the-canvas findings the new canvas raised. On a project with no destination, press **Choose a vault** in Checks, pick a vault and a folder, **Set destination**: **record** that the finding disappears, that Undo restores it, and that the Publish dialog then defaults to that vault. Read the dialog aloud with Narrator: **record** that the summary, each group's heading and each finding's button are announced, and that nothing in the dialog is a score. | |

## Guide content, targets and saved progress (Task 55)

`core::editor::guide` is unit-tested on the strict save (a closed schema,
the 22 lessons compiled in from the webview's own `steps.json`, 16 KiB, no
refusal that echoes what it refused) and the lenient read (an unknown or
retired lesson resumed at its chapter's first lesson, a malformed or
oversized file read as fresh progress); `prefs_commands` round-trips the file
through a real tempdir. The target registry is tested by mounting the whole
shell in happy-dom and resolving all 22 keys. What no automated test can show
is the real `%LOCALAPPDATA%` folder, a real file the app cannot write, and the
new **Edit actions** menu opening where the button really is in WebView2.
The coach that USES these targets is Task 56's; these rows only prove the
storage and the one new control.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T48 | **Guide progress lands in the app-wide prefs folder and nowhere else** | Open any project in the editor, close the editor window, quit the app. **Record**: whether `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-prefs\` exists (it is created on the first SAVE — since Task 56, answering the guide's invitation or moving through a lesson is one, so if you touched neither it may legitimately be absent — say which), that no `guide-progress.json` appears inside any `editor-projects\<projectId>\` folder, and whether "Session only" appeared beside **Help** in the header (it must NOT on a healthy machine). | |
| T49 | **A malformed file reads as fresh; an unreadable one says Session only** | With the app closed, create `%LOCALAPPDATA%\com.vaultbuddy.desktop\editor-prefs\guide-progress.json` containing `{ not json`, then open a project in the editor. **Record**: that the editor opens normally, that `vault-buddy.log` carries "editor guide progress: the saved file is oversized or malformed, starting fresh" and NOT the file's content, and that "Session only" does NOT show beside **Help** (a malformed file is replaced by the next save). Close the app, then deny your own user Read on that FILE (Properties → Security → Advanced → add a Deny: Read entry) and open the editor again. **Record**: that the editor still opens, that "Session only" now shows beside Help with the tooltip "Guide progress cannot be stored on this device right now. It lasts until the editor closes.", and the log line "editor guide progress: cannot read it (…)". Remove the Deny entry afterwards. | |
| T50 | **Edit actions opens the timeline's menu at the button** | In a project with two clips, select one and click **Edit actions** in the timeline toolbar. **Record**: that the action menu opens directly below the button (not at the window corner), that its items act on the selected clip (Split enabled with the playhead inside it), that Escape closes it and returns focus to the button, and that with nothing selected Delete is disabled with its reason as the tooltip. | |

## The guided walkthrough (Task 56)

The invitation, the coach and suspension. `tests/editorGuideCoach.test.ts`
walks all 22 lessons in happy-dom (every box stubbed) and
`tests/e2e/editorGuide.spec.ts` measures the coach in Chromium at 960x640;
neither is WebView2, a real window being hidden and reopened, Narrator, a
Windows high-contrast theme or a rendered file. These rows are.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T51 | **The coach finds and clears every control in a real window** | On a fresh profile (no `editor-prefs\guide-progress.json`), open a project with at least two clips. **Record**: that the invitation appears bottom-right WITHOUT taking focus (type in the title field first — keystrokes keep going there) and that editing still works behind it. Click **Show me around** and press **Next** through all 22 lessons at the 960x640 minimum size and again maximised. **Record** for each size: any lesson whose card says "This control is not on screen right now", any lesson where the card covers the outlined control, any lesson where Pause/Back/Next cannot be reached, and whether the Project's title, Undo state and "Saved" status changed at all (they must not). | |
| T52 | **Resume after hiding, reopening and quitting** | Walk to lesson 13 ("Give clips a softer start and end."), then close the editor with its X (a clean project hides at once). Reopen the editor from the capture bar and choose **Help → Resume walkthrough** (Task 57 made Help a menu). **Record**: the lesson shown and its "13 / 22". Walk to lesson 14 and quit the app from the tray within one second of pressing Next. Start the app, open the editor, press **F1**. **Record**: the lesson shown and the `currentStepId` in `editor-prefs\guide-progress.json` — 14 if the save landed, 13 if the quit destroyed the window inside the 400 ms save debounce (a hide flushes it; a quit is not known to — docs/Gaps.md GAP-205; say which). | |
| T53 | **Keys with Narrator: F6, Escape, F1 and ?** | With Narrator on, start the guide. **Record**: what Narrator reads when the card takes focus; that **F6** moves focus to the outlined control and **F6** again back to the card; that on lesson 10 opening the top track's menu and pressing **Escape** closes the MENU while the coach stays; that **Escape** with no menu open pauses the guide; and that **?** in the project title field types a "?" instead of opening the guide. | |
| T54 | **A dialog suspends the coach; a native dialog does not need to** | On lesson 18 ("Catch problems before you share."), open **Checks**. **Record**: that the ring and card disappear while the dialog is open and come back on the same lesson when it closes, with focus back on the Checks button. Repeat with **Render video** (lesson 20) and the **Webcam** dialog (lesson 11 — confirm Windows shows NO camera prompt and the camera light stays off). Then on lesson 2 press **Import media** and cancel the native file picker. **Record**: whether the coach stays visible behind the native picker (it does not suspend for OS dialogs — say whether that reads as a problem). | |
| T55 | **Nothing guide-related reaches a render; high contrast and reduced motion** | On lesson 15 ("Point to the next action.", ring around the preview's teaching tools), render a short range with **Review**. **Record**: that the decoded frames show no outline, label or card. Then turn on a Windows contrast theme and, separately, Settings → Accessibility → Visual effects → Animation effects OFF. **Record**: that the ring stays visible in the contrast theme, and that with animations off the page is NOT dimmed around the ring and the ring does not slide between lessons. | |

## The learning center (Task 57)

The Help menu, the learning center and its portable progress file.
`tests/editorLearningCenter.test.ts` drives the learning center over a fake
port and `guide_commands_tests.rs` drives the file commands over a fake
chooser; neither opens Windows' own save/open dialogs, which are the only way
a path reaches these two commands. These rows are.

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T56 | **Save progress file writes lesson progress only, and never replaces a stranger's file** | Walk the guide to lesson 6, pause it, then choose **Help → Learning center** and press **Save progress file…**. **Record**: that Windows' own save dialog opens (not a webview prompt) suggesting `vault-buddy-guide-progress.json`; save it to Documents; that the status line reads "Saved to <name>. It holds lesson progress only — no project or media."; and open the file in Notepad: it must hold only `contentRevision`, `currentStepId`, `reviewed`, `explored`, the four flags and `preferences` — no path, file name, project title or media. Save again onto the SAME file (confirm Windows' replace prompt): **Record** that it succeeds. Then save onto any other existing `.json` (e.g. a copy of a `project.json`) and confirm the replace prompt: **Record** the refusal shown and that the file is unchanged (compare its size and modified time). | |
| T57 | **Restore progress file installs progress paused and touches nothing else** | With T56's file saved, choose **Help → Learning center → Start over** and confirm, then reopen the learning center and press **Restore progress file…**, picking T56's file. **Record**: that Windows' own open dialog opens; that the progress line changes back to what the file held and the status reads "Guide progress restored…"; that no coach card appears until **Resume walkthrough** is pressed (then lesson 6); that the camera light stays off and Windows shows no camera or microphone prompt; and that the header still reads "Saved" and Undo is unchanged. Then restore a `project.json` and a text file renamed to `.json`: **Record** the refusal shown for each and that the progress line did not change. | |

## Keyboard, screen readers, contrast and diagnostics (Task 58)

`tests/e2e/editorKeyboard.spec.ts` drives a keyboard-only journey, forced
colours and light-theme contrast in Playwright's Chromium, and
`tests/editorA11y.test.ts` pins accessible names and focus return in
happy-dom. Neither is Narrator, NVDA, a Windows contrast theme at real
display scaling, or Windows' own save dialog. These rows are (the brief
numbered them T25–T27; those numbers were taken, so they land here).

| # | Check | Steps | Result |
| --- | --- | --- | --- |
| T58 | **The keyboard journey with Narrator, then NVDA** (R-A1) | Open a staged capture in the editor with the mouse put away. Turn on Narrator (Ctrl+Win+Enter). With Tab only, reach a clip on the timeline, press Enter to select it, S to split it at the playhead, Delete, then Tab to the preview toolbar's **Text** and press Enter; Tab to the inspector's **Text** field, type a line, Enter, Tab on to a button and press Ctrl+S. **Record**: what Narrator announces for the clip (its name and "selected"), for the track header's eye/lock/M/S/⋮ buttons (their labels, e.g. "Mute Video", not "M"), for the timeline's − and + ("Zoom out"/"Zoom in") and for the ratio control ("Canvas ratio"); that the header's status is announced or readable as "Saved" at the end; and that Escape in the Help, Save project, Edit actions, mixer and track menus puts focus back on the button that opened each. Repeat the whole journey with NVDA and record any difference. | |
| T59 | **A Windows contrast theme at 150 % scaling** (R-A2) | Settings → Display → Scale 150 %, then Settings → Accessibility → Contrast themes → **Night sky**, and open the editor on a project with several clips and a text cue. **Record**: that the selected clip carries a visible outline (not only a colour change), the playhead is a visible line in the theme's highlight colour, every clip's trim and fade handles show as small solid blocks, the layout box handles are visible over the preview, and Tab draws a visible focus outline on every stop (header, library tabs, toolbar, timeline clips, inspector fields). Take a screenshot for the record. The forced-colours rules are app-wide (`style.css`), so also **record**, in the same theme: in the panel, that a search result picked with the arrow keys and a focused vault row show a visible outline; and on the 88×88 buddy (click it, then Tab), whether its 2 px focus outline at 2 px offset is clipped by the window's edge. | |
| T60 | **The same at 200 %, and with a light contrast theme** (R-A2) | Repeat T59 at Scale 200 % with **Desert** (a light contrast theme). **Record**: the same five marks; that nothing is clipped off the header or toolbar at 200 % (use the drawers at the compact width); and that the light editor theme (header's theme toggle) with contrast themes OFF reads dark text on every panel, menu and the guide card — GAP-206's defect, fixed in Task 58. | |
| T61 | **Export diagnostics writes counts and codes only, through Windows' own dialog** | With a project open whose title and clip names are distinctive (e.g. "Payroll review"), run one import that fails on a file named after a person, then choose **Help → Export diagnostics**. **Record**: that Windows' own save dialog opens suggesting `vault-buddy-diagnostics.json`; that the toast names the file saved; open the file in Notepad and confirm it holds only `appVersion`, `os`, `ffmpeg {found, version, filters}`, the `sessions`/`projects`/`products` counts, `jobs` as kind/phase/errorCode, and `webview2Version` (record the WebView2 version it reports) — and that searching it for the project title, the file name, `Users` or the vault name finds nothing. Export again to the SAME name and record that it is refused and the first file is unchanged. Finally open `%LOCALAPPDATA%\com.vaultbuddy.desktop\logs\vault-buddy.log` and record that the lines this session wrote about media, takes or captures show `<path:#…>`/`<name:#…>` handles, never a path or capture name. | |
